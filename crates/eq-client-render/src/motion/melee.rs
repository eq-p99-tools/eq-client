//! Opt-in positioning for an attack the player already requested.
//! Produces ordinary steering only; it never targets, attacks or changes reach.
use super::follow;
use bevy::prelude::*;
use eq_client_core::{GameDialect, SpawnKind, WorldPosition, abilities, world::ClientWorld};
use std::time::{Duration, Instant};

/// A distant or unreachable target is not a request to roam the zone.
const MAX_DISTANCE: f32 = 200.;
const MANUAL_PAUSE: Duration = Duration::from_millis(500);

/// Records scheduling before any movement, ownership or admission gate runs.
/// This is local diagnostic time, not a received/server movement timestamp.
#[derive(Default)]
pub(super) struct InputTrace {
    started: Option<Instant>,
    previous: Option<Instant>,
    sequence: u64,
}

impl InputTrace {
    pub fn entered(&mut self, now: Instant) -> (u64, f64, Option<f64>) {
        self.sequence = self.sequence.saturating_add(1);
        let elapsed = now.saturating_duration_since(*self.started.get_or_insert(now));
        let gap = self
            .previous
            .map(|previous| now.saturating_duration_since(previous));
        self.previous = Some(now);
        (
            self.sequence,
            elapsed.as_secs_f64(),
            gap.map(|gap| gap.as_secs_f64()),
        )
    }
}

/// Opt in with `eq_client_render::motion::melee=trace`; ordinary logging is quiet.
pub(super) fn held(reason: &'static str) {
    trace!(target: "eq_client_render::motion::melee", reason, forward = false, "Melee positioning held");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scope {
    session: u64,
    target_id: u16,
    target_revision: u64,
    spawn_revision: u64,
}

#[derive(Default)]
pub(super) struct Assist {
    scope: Option<Scope>,
    approaching: bool,
    paused_until: Option<Instant>,
    driving: bool,
}

/// Already granted ordinary movement and the current collision body.
pub(super) struct Body {
    pub position: WorldPosition,
    pub height: f32,
    pub offset: f32,
    pub seconds: f32,
    pub speeds: (f32, f32),
    pub walking: bool,
}

impl Assist {
    pub fn driving(&self) -> bool {
        self.driving
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Manual input keeps its direction and view, including a brief release gap.
    pub fn yield_input(&mut self, now: Instant) {
        *self = Self {
            paused_until: Some(now + MANUAL_PAUSE),
            ..Self::default()
        };
    }

    /// The caller supplies focus, attack, option, ownership and movement gates.
    /// Target authority and geometry are checked anew for every actual sample.
    pub fn step(
        &mut self,
        world: &ClientWorld,
        collision: &eq_client_core::movement::CollisionWorld,
        regions: &eq_client_assets::regions::ZoneRegions,
        body: &Body,
        now: Instant,
    ) -> Option<follow::Step> {
        if self.paused_until.is_some_and(|until| now < until) {
            held("manual_release_pause");
            return None;
        }
        let (scope, there, radius, in_range) = match target(world, body.position, now) {
            Ok(target) => target,
            Err(reason) => {
                self.clear();
                held(reason);
                return None;
            }
        };
        if !(body.seconds.is_finite() && (0.0..=0.25).contains(&body.seconds)) {
            self.clear();
            held("sample_interval");
            return None;
        }
        let from = Vec3::from_array(eq_client_core::render_position(body.position));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        let flat = Vec2::new(to.x - from.x, to.z - from.z);
        let distance = flat.length();
        let dz = to.y - from.y;
        if !distance.is_finite() || distance > MAX_DISTANCE || dz.abs() >= radius {
            self.clear();
            held(if distance > MAX_DISTANCE {
                "distance_limit"
            } else {
                "height_or_coordinate"
            });
            return None;
        }
        // The existing Titanium ability check is horizontal. Staying inside
        // its sphere is a conservative height approximation, not extra reach.
        let reach = (radius * radius - dz * dz).sqrt();
        let stop = (reach - 1_f32.min(reach * 0.1)).max(0.);
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.approaching = !in_range;
        } else if distance > reach && !in_range {
            self.approaching = true;
        } else if distance <= stop {
            self.approaching = false;
        }
        let desired = if distance > 1e-5 {
            eq_client_core::world_heading(flat.x.atan2(flat.y))
        } else {
            body.position.heading
        };
        let heading = super::turn_toward(
            body.position.heading,
            desired,
            super::TURN_RATE * body.seconds,
        );
        let run = !body.walking && distance - stop > 8.;
        let selected = if run { body.speeds.0 } else { body.speeds.1 };
        let speed = selected.min(body.speeds.0);
        let beyond = (distance - stop).max(0.);
        // Turn first, so an approach never travels along the old facing.
        let heading_error = super::arc(heading, desired).abs();
        let reason = if !self.approaching {
            "in_range"
        } else if !heading_error.is_finite() || heading_error > 0.01 {
            "turning"
        } else if !selected.is_finite() || !body.speeds.0.is_finite() || speed <= 0. {
            "speed_unavailable"
        } else {
            approach_reason(
                collision,
                regions,
                body,
                from,
                to,
                beyond.min(speed * body.seconds),
            )
        };
        let forward = reason == "forward";
        trace!(target: "eq_client_render::motion::melee", reason, forward, sample_seconds = body.seconds,
            selected_speed = speed, distance, beyond, heading_error, "Melee positioning decision");
        self.driving = forward;
        Some(follow::Step {
            heading,
            forward,
            run,
            beyond: if forward { beyond } else { 0. },
        })
    }
}

fn target(
    world: &ClientWorld,
    own: WorldPosition,
    now: Instant,
) -> Result<(Scope, WorldPosition, f32, bool), &'static str> {
    let session = world.session_id().ok_or("no_session")?;
    let intent = world.target_intent().ok_or("target_authority")?;
    let id = intent.spawn_id.ok_or("no_target")?;
    let player = world.player().ok_or("no_player")?;
    let spawn = world.spawn(id).ok_or("target_missing")?;
    if id == player.spawn_id {
        return Err("target_self");
    }
    if !matches!(spawn.state.kind, SpawnKind::Npc | SpawnKind::Player) {
        return Err("target_kind");
    }
    if spawn.health == Some(0) {
        return Err("target_zero_health");
    }
    if !world.interacts_with(&spawn.state) {
        return Err("target_not_interactable");
    }
    let there = spawn.carried(world.generation(), now);
    if ![own.x, own.y, own.z, own.heading, there.x, there.y, there.z]
        .into_iter()
        .all(f32::is_finite)
    {
        return Err("target_coordinates");
    }
    let size = if player.size.is_finite() && player.size > 0. {
        player.size
    } else {
        abilities::player_size(player.race)
    };
    if !spawn.state.size.is_finite() || spawn.state.size < 0. {
        return Err("target_size");
    }
    let own_body = abilities::Body {
        race: player.race,
        size,
        x: own.x,
        y: own.y,
    };
    let target_body = abilities::Body {
        race: spawn.state.race,
        size: spawn.state.size,
        x: there.x,
        y: there.y,
    };
    let (radius, in_range) = match world.generation() {
        GameDialect::EqMac => (16., abilities::eqmac::in_melee_range(own, there)),
        GameDialect::Titanium | GameDialect::Rof2 => (
            reach(
                abilities::Body {
                    x: 0.,
                    y: 0.,
                    ..own_body
                },
                abilities::Body {
                    x: 0.,
                    y: 0.,
                    ..target_body
                },
            ),
            abilities::in_melee_range(own_body, target_body),
        ),
    };
    Ok((
        Scope {
            session,
            target_id: id,
            target_revision: intent.revision,
            spawn_revision: spawn.revision,
        },
        there,
        radius,
        in_range,
    ))
}

/// Reuse the established race/size predicate, including its special races;
/// sixteen bisections give a conservative radius within 0.004 world units.
fn reach(own: abilities::Body, mut target: abilities::Body) -> f32 {
    let (mut low, mut high) = (0., MAX_DISTANCE);
    for _ in 0..16 {
        let middle = f32::midpoint(low, high);
        target.x = middle;
        if abilities::in_melee_range(own, target) {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

fn approach_reason(
    collision: &eq_client_core::movement::CollisionWorld,
    regions: &eq_client_assets::regions::ZoneRegions,
    body: &Body,
    from: Vec3,
    to: Vec3,
    distance: f32,
) -> &'static str {
    let offset = to - from;
    let length = offset.length();
    if collision
        .ray_distance(from, offset.normalize_or_zero(), length)
        .is_some_and(|hit| hit < length - 0.5)
    {
        return "line_of_sight";
    }
    let feet = from - Vec3::Y * body.offset;
    let flat = Vec3::new(offset.x, 0., offset.z).normalize_or_zero() * distance;
    let achieved = collision.step(feet, flat, body.height);
    let error = Vec2::new(achieved.x - feet.x - flat.x, achieved.z - feet.z - flat.z).length();
    trace!(target: "eq_client_render::motion::melee", requested_horizontal = distance,
        achieved_horizontal = (achieved - feet).with_y(0.).length(), error, "Melee approach clearance");
    if !error.is_finite() || error > 0.02 {
        "path_shortened"
    } else if collision.support(achieved).is_none() {
        "no_support"
    } else if collision.in_liquid(achieved) {
        "liquid"
    } else if let Some(reason) = transition_reason(
        collision,
        regions,
        from,
        achieved + Vec3::Y * body.offset,
        body.height,
        body.offset,
    ) {
        reason
    } else {
        "forward"
    }
}

/// A helper may approach on known ordinary ground, never choose a zone or
/// a carrier for the player. Unknown/unloaded boundaries remain unknown.
pub(super) fn transition_reason(
    collision: &eq_client_core::movement::CollisionWorld,
    regions: &eq_client_assets::regions::ZoneRegions,
    from: Vec3,
    to: Vec3,
    height: f32,
    offset: f32,
) -> Option<&'static str> {
    if regions.zone_line_at(from.to_array()).is_some()
        || regions
            .zone_line_entry(from.to_array(), to.to_array())
            .is_some()
    {
        return Some("zone_line");
    }
    let mut feet = from - Vec3::Y * offset;
    let delta = (to - from).with_y(0.);
    // Match ordinary walking's half-unit strides and first-floor footprint
    // query. Check intermediate support as well as the final landing.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (delta.length() / 0.5).ceil().clamp(1., 64.) as u8;
    let part = delta / f32::from(count);
    for index in 0..=count {
        if collision
            .ground_spawn(feet, 0.01, super::boats::sweep_depth(feet))
            .is_some()
        {
            return Some("carrier_support");
        }
        if index < count {
            feet = collision.step(feet, part, height);
        }
    }
    None
}

#[cfg(test)]
mod tests;

//! Opt-in positioning for an attack the player already requested.
//! Produces ordinary steering only; it never targets, attacks or changes reach.
use super::follow;
use bevy::prelude::*;
use eq_client_core::{GameDialect, SpawnKind, WorldPosition, abilities, world::ClientWorld};
use std::time::{Duration, Instant};

/// A distant or unreachable target is not a request to roam the zone.
const MAX_DISTANCE: f32 = 200.;
const MANUAL_PAUSE: Duration = Duration::from_millis(500);

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
        body: &Body,
        now: Instant,
    ) -> Option<follow::Step> {
        if self.paused_until.is_some_and(|until| now < until) {
            return None;
        }
        let Some((scope, there, radius, in_range)) = target(world, body.position, now) else {
            self.clear();
            return None;
        };
        if !(body.seconds.is_finite() && (0.0..=0.25).contains(&body.seconds)) {
            self.clear();
            return None;
        }
        let from = Vec3::from_array(eq_client_core::render_position(body.position));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        let flat = Vec2::new(to.x - from.x, to.z - from.z);
        let distance = flat.length();
        let dz = to.y - from.y;
        if !distance.is_finite() || distance > MAX_DISTANCE || dz.abs() >= radius {
            self.clear();
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
        let speed = if run { body.speeds.0 } else { body.speeds.1 };
        let beyond = (distance - stop).max(0.);
        // Turn first, so an approach never travels along the old facing.
        let forward = self.approaching
            && super::arc(heading, desired).abs() <= 0.01
            && speed.is_finite()
            && speed > 0.
            && clear_approach(collision, body, from, to, beyond.min(speed * body.seconds));
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
) -> Option<(Scope, WorldPosition, f32, bool)> {
    let session = world.session_id()?;
    let intent = world.target_intent()?;
    let id = intent.spawn_id?;
    let player = world.player()?;
    let spawn = world.spawn(id)?;
    if id == player.spawn_id
        || !matches!(spawn.state.kind, SpawnKind::Npc | SpawnKind::Player)
        || spawn.health == Some(0)
        || !world.interacts_with(&spawn.state)
    {
        return None;
    }
    let there = spawn.carried(world.generation(), now);
    if ![own.x, own.y, own.z, own.heading, there.x, there.y, there.z]
        .into_iter()
        .all(f32::is_finite)
    {
        return None;
    }
    let size = if player.size.is_finite() && player.size > 0. {
        player.size
    } else {
        abilities::player_size(player.race)
    };
    if !spawn.state.size.is_finite() || spawn.state.size < 0. {
        return None;
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
    Some((
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

fn clear_approach(
    collision: &eq_client_core::movement::CollisionWorld,
    body: &Body,
    from: Vec3,
    to: Vec3,
    distance: f32,
) -> bool {
    let offset = to - from;
    let length = offset.length();
    if collision
        .ray_distance(from, offset.normalize_or_zero(), length)
        .is_some_and(|hit| hit < length - 0.5)
    {
        return false;
    }
    let feet = from - Vec3::Y * body.offset;
    let flat = Vec3::new(offset.x, 0., offset.z).normalize_or_zero() * distance;
    let achieved = collision.step(feet, flat, body.height);
    Vec2::new(achieved.x - feet.x - flat.x, achieved.z - feet.z - flat.z).length() <= 0.02
        && collision.support(achieved).is_some()
        && !collision.in_liquid(achieved)
}

#[cfg(test)]
mod tests;

//! Move-end room and the original client's delayed automatic posture.
use super::{PlayerBody, post_posture};
use bevy::prelude::Vec3;
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{Posture, PostureState, movement::CollisionWorld, world::ClientWorld};
use std::time::{Duration, Instant};

/// One refused own-movement proposal, kept separately from worker receipts.
#[derive(Clone, Copy, Debug)]
pub(super) struct Refused {
    pub floor: Option<f32>,
    pub ceiling: Option<f32>,
    pub stand: f32,
    pub room: f32,
    pub origin: Vec3,
    pub candidate: Vec3,
}

/// Standing probes after a successful ducked candidate. These are disposable
/// geometry queries, never movement proposals or inferred worker acceptance.
pub(super) struct StandingPath {
    pub displacement: Vec3,
    pub seconds: f32,
    pub lift: f32,
    pub diagonal: bool,
}

/// The scope and observed posture belong to the current admission. Queuing
/// Duck/Stand is never treated as observing that posture.
#[derive(Default)]
pub(super) struct State {
    automatic: Option<Automatic>,
    placement_revision: Option<u64>,
    swimming: bool,
    #[cfg(test)]
    suppress_standing_rate: bool,
}

struct Automatic {
    session: u64,
    spawn: u16,
    zone: String,
    client: InstalledClient,
    deadline: Instant,
    requested: Option<(Posture, u32)>,
    ducked: bool,
    /// A launch requested Standing, but its ordinary worker report has not
    /// reached the host yet. The pre-launch Duck is not an automatic Duck.
    launch_stand: Option<u32>,
    /// Observed standing rates before the automatic Duck, used only for
    /// disposable return queries. Never a movement grant or submission.
    standing_rate: f32,
}

impl State {
    /// Placement or water entry drops an unobserved request, but an observed
    /// automatic Duck retains its original return deadline at the new place.
    pub(super) fn observe(
        &mut self,
        world: &ClientWorld,
        placement_revision: u64,
        swimming: bool,
    ) -> bool {
        let replaced = self.placement_revision.replace(placement_revision);
        let entered_water = swimming && !self.swimming;
        self.swimming = swimming;
        let changed = entered_water || replaced.is_some_and(|prior| prior != placement_revision);
        if changed {
            let owned_duck = self.automatic.as_mut().filter(|automatic| {
                world.session_id() == Some(automatic.session)
                    && world.zone() == automatic.zone
                    && world
                        .player()
                        .is_some_and(|player| player.spawn_id == automatic.spawn)
                    && world.posture(automatic.spawn) == Some(PostureState::Ducking)
                    && (automatic.ducked
                        || matches!(automatic.requested, Some((Posture::Ducking, _))))
            });
            if let Some(automatic) = owned_duck {
                automatic.ducked = true;
                automatic.requested = None;
                automatic.launch_stand = None;
            } else {
                self.clear();
            }
        }
        changed
    }

    pub(super) fn recovering(&self, world: &ClientWorld) -> bool {
        self.automatic.as_ref().is_some_and(|automatic| {
            world.session_id() == Some(automatic.session)
                && world
                    .player()
                    .is_some_and(|player| player.spawn_id == automatic.spawn)
                && world.zone() == automatic.zone
        })
    }

    pub(super) fn clear(&mut self) {
        self.automatic = None;
    }

    pub(super) fn remember_standing_rate(&mut self, rate: f32) {
        if let Some(automatic) = self.automatic.as_mut()
            && rate.is_finite()
            && rate >= 0.0
        {
            automatic.standing_rate = rate;
        }
    }

    /// Titanium arms only on a standing refusal with `AllowAutoStand` on.
    /// Mac arms on every headroom refusal, ignoring that option (0x50453a).
    /// `RoF2` provisionally follows Titanium.
    pub(super) fn refused(
        &mut self,
        client: InstalledClient,
        world: &ClientWorld,
        posture: Option<PostureState>,
        allow_auto_stand: bool,
        now: Instant,
    ) -> bool {
        let Some((session, player)) = world.session_id().zip(world.player()) else {
            return false;
        };
        let ducked = posture == Some(PostureState::Ducking);
        let standing = posture.is_none_or(|posture| posture == PostureState::Standing);
        let arms = match client {
            InstalledClient::EqMac => true,
            InstalledClient::Titanium | InstalledClient::Rof2 => standing && allow_auto_stand,
        };
        if !arms {
            return false;
        }
        self.automatic = Some(Automatic {
            session,
            spawn: player.spawn_id,
            zone: world.zone().to_owned(),
            client,
            deadline: now + Duration::from_millis(2_000),
            requested: None,
            ducked,
            launch_stand: (posture == Some(PostureState::Standing)
                && world.posture(player.spawn_id) == Some(PostureState::Ducking))
            .then(|| world.player_posture_changes()),
            standing_rate: 0.0,
        });
        standing
    }

    /// Before cadence, `candidate` is None: settle observed posture and
    /// request the delayed Duck. Return to Standing is tested only with a
    /// resolved, submitted own-movement candidate (Titanium 0x48be15),
    /// or the unchanged origin on the host's existing idle-return path.
    /// Queuing a posture never makes it observed.
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "one admission-scoped posture cycle"
    )]
    pub(super) fn drive(
        &mut self,
        client: InstalledClient,
        world: &ClientWorld,
        collision: &CollisionWorld,
        body: &PlayerBody,
        mounted: bool,
        outbox: &crate::outbox::Outbox,
        now: Instant,
        candidate: Option<Vec3>,
        standing_path: Option<StandingPath>,
    ) -> Result<bool, crate::outbox::Refusal> {
        let Some(automatic) = self.automatic.as_mut() else {
            return Ok(false);
        };
        let Some(player) = world.player().filter(|player| {
            world.session_id() == Some(automatic.session)
                && player.spawn_id == automatic.spawn
                && world.zone() == automatic.zone
                && client == automatic.client
        }) else {
            self.clear();
            return Ok(false);
        };
        let posture = world.posture(player.spawn_id);
        if let Some(changes) = automatic.launch_stand {
            if posture == Some(PostureState::Standing) {
                automatic.launch_stand = None;
            } else {
                if world.player_posture_changes() != changes {
                    self.clear();
                }
                return Ok(false);
            }
        }
        if let Some((requested, changes)) = automatic.requested {
            let observed = match requested {
                Posture::Ducking => posture == Some(PostureState::Ducking),
                Posture::Standing => posture == Some(PostureState::Standing),
                Posture::Sitting => false,
            };
            if !observed {
                if world.player_posture_changes() != changes {
                    // An intervening observed posture makes this host wait
                    // ambiguous. Abandon the cycle without calling it an
                    // acknowledgement or sending a second queued action.
                    self.clear();
                    return Ok(false);
                }
                return Ok(true);
            }
            if requested == Posture::Standing {
                self.clear();
                return Ok(false);
            }
            automatic.requested = None;
            automatic.ducked = true;
        }
        // An observed return to standing completes this host-owned cycle,
        // including a manual stand; it must not immediately duck again.
        if automatic.ducked && posture == Some(PostureState::Standing) {
            self.clear();
            return Ok(false);
        }
        let Some(rule) = client.headroom() else {
            self.clear();
            return Ok(false);
        };
        let stand = crate::posture::stand_height(client, world, body);
        let probe = crate::posture::headroom_probe(client, world, stand);
        let requested = if posture == Some(PostureState::Ducking) {
            automatic.ducked = true;
            let Some(origin) = candidate else {
                return Ok(false);
            };
            // Titanium 0x48be0d and Mac 0x504680 use jbe: exactly
            // now+2000 still waits on both generations.
            let room = match client {
                InstalledClient::EqMac => rule.room(stand, false, false),
                InstalledClient::Titanium | InstalledClient::Rof2 => {
                    // Current ducked clearance is plain S here, not the
                    // movement test's S+1: 2S+r, then ordinary tested Stand.
                    2.0 * stand + eq_client_assets::falls::sphere_radius(stand)
                }
            };
            let bounds = collision.probe_headroom(origin, probe);
            if now <= automatic.deadline
                || !probe.fits(bounds, room, 0.0)
                || !probe.fits(bounds, rule.needed(stand, false, mounted), rule.margin)
                || bounds
                    .ceiling
                    .is_some_and(|ceiling| ceiling - origin.y < stand)
                || !outbox.posture_allowed(world, Posture::Standing)
            {
                return Ok(false);
            }
            if let Some(path) = standing_path {
                let feet = origin - Vec3::Y * body.feet_offset;
                let horizontal = Vec3::new(path.displacement.x, 0.0, path.displacement.z);
                let rate = automatic.standing_rate
                    * if path.diagonal {
                        std::f32::consts::SQRT_2
                    } else {
                        1.0
                    };
                #[cfg(test)]
                let rate = if self.suppress_standing_rate {
                    0.0
                } else {
                    rate
                };
                let ahead =
                    horizontal.normalize_or_zero() * horizontal.length().max(rate * path.seconds);
                let displacement = ahead + Vec3::Y * path.displacement.y;
                // Return's native unlifted probe alone can pass where the
                // following lifted standing move fails. Check the present
                // standing position AND the continuation of held input with
                // the ordinary standing helper, without changing its bounds.
                if refused_probe(
                    client,
                    collision,
                    (body, stand),
                    (feet, feet),
                    (false, mounted),
                    (probe, path.lift, path.seconds),
                )
                .is_some()
                    || refused_probe(
                        client,
                        collision,
                        (body, stand),
                        (feet, feet + displacement),
                        (false, mounted),
                        (probe, path.lift, path.seconds),
                    )
                    .is_some()
                {
                    return Ok(false);
                }
            }
            Posture::Standing
        } else {
            let origin = Vec3::from_array(eq_client_core::render_position(player.position));
            // Titanium's ordinary standing-to-duck test may itself refuse.
            // Mac has no such transition test. Do not infer success from the
            // outbox's intentionally silent geometric-refusal Ok result.
            let tests_duck = match client {
                InstalledClient::EqMac => false,
                InstalledClient::Titanium | InstalledClient::Rof2 => true,
            };
            if tests_duck
                && (!probe.fits(
                    collision.probe_headroom(origin, probe),
                    rule.needed(stand, true, mounted),
                    rule.margin,
                ) || !outbox.posture_allowed(world, Posture::Ducking))
            {
                self.clear();
                return Ok(false);
            }
            Posture::Ducking
        };
        // A queue refusal leaves the cycle retryable. Admission and ordinary
        // capability checks run again; no requested posture was installed.
        post_posture(outbox, world, requested)?;
        automatic.requested = Some((requested, world.player_posture_changes()));
        Ok(true)
    }
}

/// Shared ordinary-walk probe lift; linked callers pass `raised = false`.
pub(super) fn walking_lift(
    client: InstalledClient,
    world: &ClientWorld,
    body: &PlayerBody,
    raised: bool,
) -> f32 {
    client
        .falls()
        .and_then(|falls| falls.edge_lift)
        .map_or(0.0, |rule| {
            rule.lift(eq_client_assets::falls::EdgeBody {
                race: world.player().map_or(0, |player| player.race),
                stand_height: crate::posture::stand_height(client, world, body),
                size: body.size,
                lifted: raised,
            })
        })
}

/// A pointless duck at a wall must not arm an automatic posture cycle. Use
/// the same intended displacement and a disposable ordinary ducked step;
/// neither its position nor its vertical state is submitted or installed.
#[allow(clippy::too_many_arguments)]
pub(super) fn duck_step_passes(
    client: InstalledClient,
    state: &ClientWorld,
    world: &CollisionWorld,
    body: &PlayerBody,
    feet: Vec3,
    delta: Vec3,
    seconds: f32,
    airborne: Option<&eq_client_core::movement::AirborneController>,
    mounted: bool,
) -> bool {
    if Vec3::new(delta.x, 0.0, delta.z) == Vec3::ZERO {
        return false;
    }
    let stand = crate::posture::stand_height(client, state, body);
    let ducked = world
        .clone()
        .with_collision_height(crate::posture::movement_height(
            client,
            state,
            body,
            Some(PostureState::Ducking),
        ));
    let candidate = if let Some(airborne) = airborne {
        let mut trial = airborne.clone();
        let physics = super::walking(
            super::physics(client, state.zone_gravity()),
            client,
            body,
            state.player().map_or(0, |player| player.race),
            (false, false),
        );
        super::fall_step_with_water(
            &mut trial,
            &ducked,
            physics,
            client.falls().map(|_| physics.reach()),
            feet,
            delta,
            seconds,
            body.height,
            false,
            false,
        )
    } else {
        ducked.step(feet, delta, body.height)
    };
    (candidate - feet).dot(Vec3::new(delta.x, 0.0, delta.z)) > 0.0
        && refused_probe(
            client,
            &ducked,
            (body, stand),
            (feet, candidate),
            (true, mounted),
            (
                crate::posture::headroom_probe(client, state, stand),
                0.0,
                seconds,
            ),
        )
        .is_none()
}

/// Test helper using the same active query, without the walking caller's lift.
#[cfg(test)]
pub(super) fn refused(
    client: InstalledClient,
    world: &CollisionWorld,
    (body, stand): (&PlayerBody, f32),
    (feet, candidate): (Vec3, Vec3),
    (ducking, mounted): (bool, bool),
) -> Option<Refused> {
    let probe = crate::posture::headroom_dimensions(client, stand);
    refused_probe(
        client,
        world,
        (body, stand),
        (feet, candidate),
        (ducking, mounted),
        (probe, 0.0, 0.05),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn refused_probe(
    client: InstalledClient,
    world: &CollisionWorld,
    (body, stand): (&PlayerBody, f32),
    (feet, candidate): (Vec3, Vec3),
    (ducking, mounted): (bool, bool),
    (probe, lift, seconds): (eq_client_core::movement::HeadroomProbe, f32, f32),
) -> Option<Refused> {
    let rule = client.headroom()?;
    let room = rule.room(stand, ducking, mounted);
    let offset = Vec3::Y * (body.feet_offset + lift);
    let need = rule.needed(stand, ducking, mounted);
    // The existing 60 Hz program-frame mapping is [I], not a native frame
    // cap. A capped host sample spans at most nine of those frames. Probe
    // their candidate spheres, not the extra volume between them.
    let samples = (1..=9_u8)
        .find(|count| seconds <= f32::from(*count) / eq_client_assets::falls::PROGRAM_FRAME_RATE)
        .unwrap_or(9);
    let step_fraction = if seconds > 0.0 {
        1.0 / (seconds * eq_client_assets::falls::PROGRAM_FRAME_RATE)
    } else {
        1.0
    };
    let hit = world.headroom_on_path(
        feet + offset,
        candidate + offset,
        probe,
        need,
        rule.margin,
        (samples, step_fraction),
    );
    if bevy::log::tracing::enabled!(bevy::log::Level::TRACE) {
        let bounds = world.probe_headroom(candidate + offset, probe);
        bevy::log::trace!(
            ?client, ducking, mounted, ?feet, ?candidate,
            probe_from = ?(feet + offset), probe_to = ?(candidate + offset),
            floor = ?bounds.floor, ceiling = ?bounds.ceiling,
            need, margin = rule.margin, stand, lift, seconds, samples, step_fraction, ?probe,
            endpoint_fits = probe.fits(bounds, need, rule.margin),
            refused = hit.is_some(), blocked_origin = ?hit.map(|hit| hit.origin),
            "Movement headroom probe"
        );
    }
    let hit = hit?;
    Some(Refused {
        floor: hit.bounds.floor,
        ceiling: hit.bounds.ceiling,
        stand,
        room,
        origin: hit.origin,
        candidate,
    })
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "strict room and exact rollback boundaries"
)]
mod tests;

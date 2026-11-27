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

/// The scope and observed posture belong to the current admission. Queuing
/// Duck/Stand is never treated as observing that posture.
#[derive(Default)]
pub(super) struct State {
    automatic: Option<Automatic>,
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
}

impl State {
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

/// At the resolved candidate, including its step-up floor. Passive carrying
/// is already part of `feet`; an unchanged own position isn't a room test.
#[cfg(test)]
pub(super) fn refused(
    client: InstalledClient,
    world: &CollisionWorld,
    (body, stand): (&PlayerBody, f32),
    (feet, candidate): (Vec3, Vec3),
    (ducking, mounted): (bool, bool),
) -> Option<Refused> {
    let mac = client == InstalledClient::EqMac;
    let probe = eq_client_core::movement::HeadroomProbe {
        floor_radius: if mac { 0.0 } else { 0.1 * stand },
        ceiling_radius: if mac {
            0.5
        } else {
            eq_client_assets::falls::sphere_radius(stand)
        },
        floor_bias: if mac { 0.0 } else { 0.001 },
        floor_offset: if mac { -0.5 } else { 0.0 },
        permits_missing_floor: false,
        permits_missing_ceiling: !mac,
    };
    refused_probe(
        client,
        world,
        (body, stand),
        (feet, candidate),
        (ducking, mounted),
        (probe, 0.0),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn refused_probe(
    client: InstalledClient,
    world: &CollisionWorld,
    (body, stand): (&PlayerBody, f32),
    (feet, candidate): (Vec3, Vec3),
    (ducking, mounted): (bool, bool),
    (probe, lift): (eq_client_core::movement::HeadroomProbe, f32),
) -> Option<Refused> {
    if candidate.distance_squared(feet) <= 1e-10 {
        return None;
    }
    let rule = client.headroom()?;
    let room = rule.room(stand, ducking, mounted);
    let offset = Vec3::Y * (body.feet_offset + lift);
    let need = rule.needed(stand, ducking, mounted);
    let hit = world.headroom_on_path(feet + offset, candidate + offset, probe, need, rule.margin);
    if bevy::log::tracing::enabled!(bevy::log::Level::TRACE) {
        let bounds = world.probe_headroom(candidate + offset, probe);
        bevy::log::trace!(
            ?client, ducking, mounted, ?feet, ?candidate,
            probe_from = ?(feet + offset), probe_to = ?(candidate + offset),
            floor = ?bounds.floor, ceiling = ?bounds.ceiling,
            need, margin = rule.margin, stand, lift, ?probe,
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

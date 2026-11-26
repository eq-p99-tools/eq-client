//! Move-end room and the original client's delayed automatic posture.
use super::*;
use eq_client_core::PostureState;

/// One refused own-movement proposal, kept separately from worker receipts.
#[derive(Clone, Copy, Debug)]
pub(super) struct Refused {
    pub floor: f32,
    pub ceiling: f32,
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
    requested: Option<Posture>,
    ducked: bool,
}

impl State {
    pub(super) fn clear(&mut self) {
        self.automatic = None;
    }

    /// Titanium arms only on a standing refusal with AllowAutoStand on.
    /// Mac arms on every headroom refusal, ignoring that option (0x50453a).
    /// RoF2 provisionally follows Titanium.
    pub(super) fn refused(
        &mut self,
        client: InstalledClient,
        world: &ClientWorld,
        posture: Option<PostureState>,
        allow_auto_stand: bool,
        now: Instant,
    ) {
        let Some((session, player)) = world.session_id().zip(world.player()) else {
            return;
        };
        let ducked = posture == Some(PostureState::Ducking);
        let standing = posture.is_none_or(|posture| posture == PostureState::Standing);
        if client != InstalledClient::EqMac && !(standing && allow_auto_stand) {
            return;
        }
        self.automatic = Some(Automatic {
            session,
            spawn: player.spawn_id,
            zone: world.zone().to_owned(),
            client,
            deadline: now + Duration::from_millis(2_000),
            requested: None,
            ducked,
        });
    }

    /// Runs before the movement cadence gate, on a later input pass than
    /// the refusal. Returns true while an ordinary posture command is in
    /// flight. No repeated queue action or movement based on an assumed duck.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn drive(
        &mut self,
        client: InstalledClient,
        world: &ClientWorld,
        collision: &CollisionWorld,
        body: &PlayerBody,
        mounted: bool,
        outbox: &crate::outbox::Outbox,
        now: Instant,
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
        if let Some(requested) = automatic.requested {
            let observed = match requested {
                Posture::Ducking => posture == Some(PostureState::Ducking),
                Posture::Standing => posture == Some(PostureState::Standing),
                _ => false,
            };
            if !observed {
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
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        let requested = if posture != Some(PostureState::Ducking) {
            // Titanium's ordinary standing-to-duck test may itself refuse.
            // Mac has no such transition test. Do not infer success from the
            // outbox's intentionally silent geometric-refusal Ok result.
            if client != InstalledClient::EqMac
                && (!collision.headroom(origin, rule.room(stand, true, mounted))
                    || !outbox.posture_allowed(world, Posture::Ducking))
            {
                self.clear();
                return Ok(false);
            }
            Posture::Ducking
        } else {
            automatic.ducked = true;
            // Native reads say "past" now+2000; exact equality is retained
            // as a read gap. The strict endpoint here never stands early.
            let room = match client {
                InstalledClient::EqMac => rule.room(stand, false, false),
                InstalledClient::Titanium | InstalledClient::Rof2 => {
                    // Current ducked clearance is plain S here, not the
                    // movement test's S+1: 2S+r, then ordinary tested Stand.
                    2.0 * stand + eq_client_assets::falls::sphere_radius(stand)
                }
            };
            if now <= automatic.deadline
                || !collision.headroom(origin, room)
                || !outbox.posture_allowed(world, Posture::Standing)
            {
                return Ok(false);
            }
            Posture::Standing
        };
        // A queue refusal leaves the cycle retryable. Admission and ordinary
        // capability checks run again; no requested posture was installed.
        post_posture(outbox, world, requested)?;
        automatic.requested = Some(requested);
        Ok(true)
    }
}

/// At the resolved candidate, including its step-up floor. Passive carrying
/// is already part of `feet`; an unchanged own position isn't a room test.
pub(super) fn refused(
    client: InstalledClient,
    world: &CollisionWorld,
    (body, stand): (&PlayerBody, f32),
    (feet, candidate): (Vec3, Vec3),
    (ducking, mounted): (bool, bool),
) -> Option<Refused> {
    if candidate.distance_squared(feet) <= 1e-10 {
        return None;
    }
    let room = client.headroom()?.room(stand, ducking, mounted);
    let origin = candidate + Vec3::Y * body.feet_offset;
    let (floor, ceiling) = world.headroom_bounds(origin)?;
    (floor + room >= ceiling).then_some(Refused {
        floor,
        ceiling,
        stand,
        room,
        origin,
        candidate,
    })
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "strict room and exact rollback boundaries"
)]
mod tests;

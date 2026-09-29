//! Carries an explicit movement calibration across an uninterrupted normal handoff.
use eq_client_core::{ClientCommand, MotionCalibration, PlayerState, WorldEvent};
use std::time::Instant;

/// Observable metadata, not a measurement of effective speed or buffs.
#[derive(Clone, Copy, PartialEq)]
struct Identity {
    race: u32,
    size: f32,
    walk: f32,
    run: f32,
}
impl Identity {
    fn from_player(player: &PlayerState) -> Option<Self> {
        (player.size.is_finite()
            && player.size >= 0.0
            && player.walk_speed.is_finite()
            && player.walk_speed > 0.0
            && player.run_speed.is_finite()
            && player.run_speed > 0.0
            && player.hp_percent != Some(0))
        .then_some(Self {
            race: player.race,
            size: player.size,
            walk: player.walk_speed,
            run: player.run_speed,
        })
    }
}

struct Admission {
    session: u64,
    spawn: u16,
    identity: Option<Identity>,
    granted: bool,
}

/// Calibration remains opt-in; only an uninterrupted normal transfer can carry it.
pub(super) struct Continuity {
    calibration: Option<MotionCalibration>,
    admission: Option<Admission>,
    transfer: Option<Identity>,
}
impl Continuity {
    pub(super) fn new(calibration: Option<MotionCalibration>) -> Self {
        Self {
            calibration,
            admission: None,
            transfer: None,
        }
    }

    /// Consumes ordered events and creates a fresh destination-scoped calibration.
    pub(super) fn observe(&mut self, event: &WorldEvent, now: Instant) -> Option<ClientCommand> {
        match event {
            WorldEvent::Entered {
                session_id, player, ..
            } => {
                if self
                    .admission
                    .as_ref()
                    .is_some_and(|active| active.session == *session_id)
                {
                    return None;
                }
                let identity = Identity::from_player(player);
                let initial = self.admission.is_none();
                let carry = self
                    .transfer
                    .take()
                    .is_some_and(|old| Some(old) == identity);
                self.admission = Some(Admission {
                    session: *session_id,
                    spawn: player.spawn_id,
                    identity,
                    granted: false,
                });
                if (initial || carry) && player.hp_percent != Some(0) {
                    return self
                        .calibration
                        .map(|calibration| ClientCommand::ConfigureMotion {
                            session_id: *session_id,
                            calibration,
                            created: now,
                        });
                }
            }
            WorldEvent::MotionState {
                session_id,
                units_per_second,
                backward_units_per_second,
                walk_units_per_second,
                strafe_units_per_second,
                ..
            } => {
                if let Some(active) = &mut self.admission
                    && active.session == *session_id
                {
                    active.granted = self.calibration.is_some_and(|value| {
                        Some(value.units_per_second) == *units_per_second
                            && value.strafe.map(|mode| mode.units_per_second)
                                == *strafe_units_per_second
                            && value.walk.map(|mode| mode.units_per_second)
                                == *walk_units_per_second
                            && value.backward.map(|mode| mode.units_per_second)
                                == *backward_units_per_second
                    });
                }
            }
            WorldEvent::ZoneTransfer(offer) => {
                self.transfer = self
                    .admission
                    .as_ref()
                    .filter(|active| active.granted && !offer.to_bind)
                    .and_then(|active| active.identity);
            }
            WorldEvent::ZoneTransferRejected { session_id, .. }
                if self
                    .admission
                    .as_ref()
                    .is_some_and(|active| active.session == *session_id) =>
            {
                self.invalidate();
            }
            // A server correction resets the session's movement grant; the measured
            // speed is unchanged, so grant it again for the same admission. Stock
            // EQEmu corrects the position right after zone entry.
            WorldEvent::Position { spawn_id, .. }
                if self
                    .admission
                    .as_ref()
                    .is_some_and(|active| active.spawn == *spawn_id) =>
            {
                self.invalidate();
                let session_id = self.admission.as_ref()?.session;
                return self
                    .calibration
                    .map(|calibration| ClientCommand::ConfigureMotion {
                        session_id,
                        calibration,
                        created: now,
                    });
            }
            WorldEvent::Death(death)
                if self
                    .admission
                    .as_ref()
                    .is_some_and(|active| u32::from(active.spawn) == death.spawn_id) =>
            {
                self.invalidate();
            }
            _ => (),
        }
        None
    }

    /// A correction or death invalidates the measured mode; other entities do not.
    fn invalidate(&mut self) {
        self.transfer = None;
        if let Some(active) = &mut self.admission {
            active.granted = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{Death, WorldPosition, ZoneOffer};

    fn calibration() -> MotionCalibration {
        MotionCalibration {
            walk: None,
            strafe: None,
            backward: None,
            units_per_second: 6.0,
            velocity_scale: 0.1,
            animation: 27,
        }
    }
    fn entered(session_id: u64, run_speed: f32) -> WorldEvent {
        WorldEvent::Entered {
            session_id,
            zone: "example".into(),
            player: Box::new(PlayerState {
                base_attributes: None,
                spawn_id: 7,
                race: 1,
                deity: None,
                class: Some(1),
                gender: 0,
                level: 1,
                position: WorldPosition::default(),
                mana: 0,
                endurance: None,
                skills: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.4,
                run_speed,
                hp_percent: Some(100),
            }),
        }
    }
    fn grant(session_id: u64) -> WorldEvent {
        WorldEvent::MotionState {
            session_id,
            units_per_second: Some(6.0),
            strafe_units_per_second: None,
            walk_units_per_second: None,
            backward_units_per_second: None,
            falls: false,
        }
    }
    fn transfer(to_bind: bool) -> WorldEvent {
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 42,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind,
        })
    }
    #[test]
    fn normal_handoff_reissues_once_with_new_session_and_creation_time() {
        let mut policy = Continuity::new(Some(calibration()));
        let now = Instant::now();
        assert!(policy.observe(&entered(1, 0.7), now).is_some());
        policy.observe(&grant(1), now);
        policy.observe(&transfer(false), now);
        policy.observe(
            &WorldEvent::MotionState {
                session_id: 1,
                units_per_second: None,
                strafe_units_per_second: None,
                walk_units_per_second: None,
                backward_units_per_second: None,
                falls: false,
            },
            now,
        );
        let later = now + std::time::Duration::from_secs(10);
        policy.observe(
            &WorldEvent::ZoneTransferRejected {
                session_id: 99,
                reason: eq_client_core::ZoneRejection::Server(-1),
            },
            later,
        );
        assert_eq!(
            policy.observe(&entered(2, 0.7), later),
            Some(ClientCommand::ConfigureMotion {
                session_id: 2,
                calibration: calibration(),
                created: later
            })
        );
        assert!(policy.observe(&entered(2, 0.7), later).is_none());
        policy.observe(&grant(1), later);
        policy.observe(&transfer(false), later);
        assert!(policy.observe(&entered(3, 0.7), later).is_none());
    }
    #[test]
    fn a_position_correction_grants_movement_again_in_the_same_zone() {
        let now = Instant::now();
        let mut policy = Continuity::new(Some(calibration()));
        policy.observe(&entered(1, 0.7), now);
        policy.observe(&grant(1), now);
        let correction = WorldEvent::Position {
            spawn_id: 7,
            position: WorldPosition::default(),
        };
        assert!(matches!(
            policy.observe(&correction, now),
            Some(ClientCommand::ConfigureMotion { session_id: 1, .. })
        ));
        let other = WorldEvent::Position {
            spawn_id: 8,
            position: WorldPosition::default(),
        };
        assert!(policy.observe(&other, now).is_none());
    }

    #[test]
    fn changed_speed_death_correction_and_unsolicited_admission_do_not_resume() {
        let now = Instant::now();
        for interruption in [
            WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Server(-1),
            },
            WorldEvent::Position {
                spawn_id: 7,
                position: WorldPosition::default(),
            },
            WorldEvent::Death(Death {
                spawn_id: 7,
                killer_id: 0,
                corpse_id: 0,
                bind_zone_id: 0,
            }),
            transfer(true),
        ] {
            let mut policy = Continuity::new(Some(calibration()));
            policy.observe(&entered(1, 0.7), now);
            policy.observe(&grant(1), now);
            policy.observe(&transfer(false), now);
            policy.observe(&interruption, now);
            assert!(policy.observe(&entered(2, 0.7), now).is_none());
        }
        let mut policy = Continuity::new(Some(calibration()));
        policy.observe(&entered(1, 0.7), now);
        policy.observe(&grant(1), now);
        policy.observe(&transfer(false), now);
        assert!(policy.observe(&entered(2, 0.4), now).is_none());
        assert!(policy.observe(&entered(3, 0.7), now).is_none());
        assert!(
            Continuity::new(None)
                .observe(&entered(1, 0.7), now)
                .is_none()
        );
    }
}

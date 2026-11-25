//! One actual pad probe, followed by its admission-bound movement result.
use super::{AirborneController, CollisionWorld, Controls};
use bevy::prelude::Vec3;
use eq_client_core::{
    ClientCommand, LocalPadCompletion, LocalPadCrossing, PadArrivalStep, WorldPosition,
};
use std::time::Instant;

pub(super) struct PendingPad {
    crossing: LocalPadCrossing,
    prepared: Option<Prepared>,
    submitted: bool,
    retried: bool,
    stopped: bool,
    notice: Option<String>,
}

struct Prepared {
    step: PadArrivalStep,
    position: WorldPosition,
    airborne: AirborneController,
    created: Instant,
}

impl Controls {
    pub(crate) fn begin_pad(&mut self, crossing: LocalPadCrossing) {
        self.local_pad_landed();
        self.pad = Some(PendingPad {
            crossing,
            prepared: None,
            submitted: false,
            retried: false,
            stopped: false,
            notice: None,
        });
    }

    /// Retain the single observation through a full outbox; do not probe a
    /// later floor or consume vertical momentum twice while retrying.
    pub(super) fn prepare_pad(&mut self, collision: &CollisionWorld, stand_height: f32) {
        let Some(pad) = self
            .pad
            .as_mut()
            .filter(|pad| pad.prepared.is_none() && !pad.stopped)
        else {
            return;
        };
        let origin = Vec3::from_array(eq_client_core::render_position(pad.crossing.landing));
        let Some(arrival) = collision.pad_arrival(origin, stand_height) else {
            self.stop_pad("invalid arrival body or position");
            return;
        };
        let mut airborne = self.airborne.clone().unwrap_or_default();
        let step = PadArrivalStep {
            floor: arrival.floor(),
            stand_height,
            vertical_speed: airborne.velocity(),
        };
        let position = eq_client_core::world_position(
            arrival.first_step(&mut airborne).to_array(),
            pad.crossing.landing.heading,
        );
        pad.prepared = Some(Prepared {
            step,
            position,
            airborne,
            created: Instant::now(),
        });
    }

    pub(super) fn submit_pad(
        &mut self,
        outbox: &crate::outbox::Outbox,
        world: &eq_client_core::world::ClientWorld,
    ) {
        let Some(pad) = self
            .pad
            .as_mut()
            .filter(|pad| !pad.submitted && !pad.stopped)
        else {
            return;
        };
        let Some(prepared) = pad.prepared.as_ref() else {
            return;
        };
        match outbox.try_post(world, |_| ClientCommand::FinishLocalPad {
            session_id: pad.crossing.session_id,
            revision: pad.crossing.revision,
            step: prepared.step,
            created: prepared.created,
        }) {
            Ok(()) => pad.submitted = true,
            Err(crate::outbox::Refusal::Busy) => (),
            Err(reason) => self.stop_pad(&reason.to_string()),
        }
    }

    pub(crate) fn pad_refused(&mut self, revision: u64, reason: &str) {
        let Some(pad) = self
            .pad
            .as_mut()
            .filter(|pad| pad.crossing.revision == revision && pad.submitted && !pad.stopped)
        else {
            return;
        };
        if pad.retried {
            self.stop_pad(reason);
        } else {
            // A refusal consumes no network revision. Retry the same observation
            // once; never sample a later floor or loop on invalid physics.
            pad.retried = true;
            pad.submitted = false;
        }
    }

    fn stop_pad(&mut self, reason: &str) {
        let Some(pad) = self.pad.as_mut() else {
            return;
        };
        pad.stopped = true;
        pad.notice = Some(format!(
            "Pad arrival could not finish: {reason}. Movement is paused. Use /quit, then log in and enter the world again."
        ));
        self.refused = pad.notice.clone();
        self.autorun = false;
        self.following = None;
    }

    /// A failed handshake leaves chat's existing /quit recovery available.
    /// It cannot release an unverified raw landing into ordinary movement.
    pub(super) fn take_pad_notice(&mut self) -> Option<String> {
        self.pad.as_mut()?.notice.take()
    }

    /// Only the matching local movement result commits this trial physics.
    pub(crate) fn complete_pad(&mut self, completed: LocalPadCompletion) {
        let Some(pad) = self
            .pad
            .as_ref()
            .filter(|pad| pad.crossing == completed.crossing && pad.submitted && !pad.stopped)
        else {
            return;
        };
        let Some(prepared) = pad
            .prepared
            .as_ref()
            .filter(|prepared| prepared.position == completed.position)
        else {
            self.stop_pad("completion did not match the prepared arrival");
            return;
        };
        let mut airborne = prepared.airborne.clone();
        self.pad_landing = airborne.take_landing();
        // The next ordinary sample covers time after this actual first step.
        self.last_accepted = prepared.created;
        self.airborne = self.airborne.as_ref().map(|_| airborne);
        if prepared
            .step
            .floor
            .is_some_and(|floor| floor > completed.crossing.landing.z - prepared.step.stand_height)
        {
            let (horizontal, _) = self.wet.units_per_second();
            self.wet = eq_client_core::movement::water::WetVelocity::from_units_per_second(
                horizontal, 0.0,
            );
        }
        self.pad = None;
        self.refused = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submitted_pad() -> (Controls, LocalPadCompletion) {
        let crossing = LocalPadCrossing {
            session_id: 11,
            spawn_id: 7,
            revision: 1,
            entry: WorldPosition::default(),
            landing: WorldPosition {
                z: 38.0,
                ..WorldPosition::default()
            },
        };
        let mut airborne = AirborneController::default();
        airborne.carry_from_water(-12.0, Vec3::ZERO, 70.0);
        let mut controls = Controls {
            airborne: Some(airborne),
            ..Controls::default()
        };
        controls.begin_pad(crossing);
        let collision = CollisionWorld::new([
            [
                [-100.0, 38.0, -100.0],
                [100.0, 38.0, 100.0],
                [100.0, 38.0, -100.0],
            ],
            [
                [-100.0, 38.0, -100.0],
                [-100.0, 38.0, 100.0],
                [100.0, 38.0, 100.0],
            ],
        ])
        .unwrap();
        controls.prepare_pad(&collision, 3.75);
        let pad = controls.pad.as_mut().unwrap();
        pad.submitted = true;
        let completed = LocalPadCompletion {
            crossing,
            position: pad.prepared.as_ref().unwrap().position,
        };
        (controls, completed)
    }

    #[test]
    fn pad_completion_mismatch_stops_once_without_committing_trial_physics() {
        let (mut controls, completed) = submitted_pad();
        let accepted = controls.last_accepted;
        controls.autorun = true;
        let mut mismatch = completed;
        mismatch.position.z += 1.0;
        controls.complete_pad(mismatch);
        assert!(controls.pad.as_ref().unwrap().stopped);
        assert!(!controls.autorun);
        assert_eq!(controls.last_accepted, accepted);
        assert!(controls.pad_landing.is_none());
        assert!((controls.airborne.as_ref().unwrap().velocity() + 12.0).abs() < f32::EPSILON);
        let notice = controls.take_pad_notice().unwrap();
        assert!(notice.contains("did not match") && notice.contains("Use /quit"));
        // A late matching result cannot reopen a terminal stop or repeat its line.
        controls.complete_pad(completed);
        controls.complete_pad(mismatch);
        assert!(controls.pad.as_ref().unwrap().stopped);
        assert!(controls.take_pad_notice().is_none());
        assert_eq!(controls.refused.as_deref(), Some(notice.as_str()));
    }

    #[test]
    fn pad_retry_success_has_no_failure_notice() {
        let (mut controls, completed) = submitted_pad();
        controls.pad_refused(completed.crossing.revision, "retryable refusal");
        assert!(!controls.pad.as_ref().unwrap().submitted);
        assert!(controls.take_pad_notice().is_none());
        assert!(controls.refused.is_none());
        controls.pad.as_mut().unwrap().submitted = true;
        controls.complete_pad(completed);
        assert!(controls.pad.is_none());
        assert!(controls.take_pad_notice().is_none());
        assert!(controls.refused.is_none());
        assert!(controls.pad_landing.is_some());
    }

    #[test]
    fn stale_or_cancelled_pad_completion_is_silent() {
        let (mut controls, completed) = submitted_pad();
        let mut stale = completed;
        stale.crossing.revision += 1;
        stale.position.z += 1.0;
        controls.complete_pad(stale);
        assert!(!controls.pad.as_ref().unwrap().stopped);
        assert!(controls.take_pad_notice().is_none());
        controls.complete_pad(completed);
        controls.complete_pad(completed);
        assert!(controls.pad.is_none());
        assert!(controls.refused.is_none());
        for reset in [false, true] {
            let (mut controls, completed) = submitted_pad();
            if reset {
                controls.reset(None);
            } else {
                controls.same_zone_landed();
            }
            controls.complete_pad(completed);
            assert!(controls.pad.is_none());
            assert!(controls.take_pad_notice().is_none());
            assert!(controls.refused.is_none());
        }
    }

    #[test]
    fn current_refusal_retries_once_then_offers_reentry_without_releasing_raw_placement() {
        let mut controls = Controls::default();
        controls.begin_pad(LocalPadCrossing {
            session_id: 11,
            spawn_id: 7,
            revision: 1,
            entry: WorldPosition::default(),
            landing: WorldPosition::default(),
        });
        controls.autorun = true;
        controls.pad.as_mut().unwrap().submitted = true;
        controls.pad_refused(2, "stale");
        assert!(controls.pad.as_ref().unwrap().submitted);
        controls.pad_refused(1, "invalid observation");
        assert!(!controls.pad.as_ref().unwrap().submitted);
        assert!(!controls.pad.as_ref().unwrap().stopped);
        assert!(controls.take_pad_notice().is_none());
        assert!(controls.refused.is_none());
        controls.pad.as_mut().unwrap().submitted = true;
        controls.pad_refused(1, "invalid observation");
        assert!(controls.pad.as_ref().unwrap().stopped);
        assert!(!controls.autorun);
        assert!(controls.take_pad_notice().unwrap().contains("Use /quit"));
        assert!(controls.take_pad_notice().is_none());
        controls.pad_refused(1, "duplicate");
        assert!(controls.take_pad_notice().is_none());
        controls.reset(None);
        assert!(controls.pad.is_none());
    }
}

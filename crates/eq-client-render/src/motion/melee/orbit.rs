//! Local lock intent and ordinary, bounded target-relative steering.
use super::super::{MotionInput, MovementMode, Pace, TURN_RATE, arc};
use bevy::prelude::*;
use std::time::Instant;

/// Only continuously observed held input earns an orbit interval. Waiting for
/// a worker, a blocked path or a released tap never banks lateral travel.
#[derive(Default)]
pub(super) struct Held {
    right: Option<bool>,
    since: Option<Instant>,
    observed: Option<Instant>,
}

impl Held {
    pub fn observe(&mut self, axis: f32, now: Instant, eligible: bool) {
        if !eligible || !axis.is_finite() || axis == 0. {
            *self = Self::default();
        } else {
            if self.right != Some(axis > 0.) || self.since.is_none() {
                self.since = Some(now);
            }
            self.right = Some(axis > 0.);
            self.observed = Some(now);
        }
    }

    pub fn seconds(&self, now: Instant, span: f32) -> f32 {
        if self.observed != Some(now) {
            return 0.;
        }
        self.since.map_or(0., |since| {
            now.saturating_duration_since(since).as_secs_f32().min(span)
        })
    }

    pub fn consume(&mut self) {
        self.since = self.observed;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Disabled,
    Approaching,
    Locked,
    Suspended,
    Released,
    Invalidated,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Approaching => "approaching",
            Self::Locked => "locked",
            Self::Suspended => "suspended",
            Self::Released => "released",
            Self::Invalidated => "invalidated",
        }
    }
}

/// Explicit local intent; received target repetitions cannot change this key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Arm {
    pub admission: u64,
    pub attack: u64,
    pub choice: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Lock {
    pub phase: Phase,
    pub reason: &'static str,
    arm: Option<Arm>,
    pub radius: Option<f32>,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            phase: Phase::Disabled,
            reason: "inactive",
            arm: None,
            radius: None,
        }
    }
}

impl Lock {
    pub fn set(&mut self, phase: Phase, reason: &'static str) {
        if self.phase != phase || self.reason != reason {
            trace!(target: "eq_client_render::motion::melee", state=phase.word(), reason,
                "Melee positioning state");
        }
        self.phase = phase;
        self.reason = reason;
    }
    pub fn sync(&mut self, arm: Option<Arm>) -> bool {
        let changed = self.arm != arm;
        if arm.is_none() {
            *self = Self::default();
        } else if self.arm != arm {
            *self = Self {
                arm,
                phase: Phase::Approaching,
                reason: "explicit_intent",
                ..Self::default()
            };
        }
        changed
    }

    pub fn active(self) -> bool {
        matches!(
            self.phase,
            Phase::Approaching | Phase::Locked | Phase::Suspended
        )
    }

    pub fn camera_locked(self) -> bool {
        self.active() && self.radius.is_some()
    }

    pub fn release(&mut self, reason: &'static str) {
        self.set(Phase::Released, reason);
        self.radius = None;
    }

    pub fn invalidate(&mut self, reason: &'static str) {
        self.set(Phase::Invalidated, reason);
        self.radius = None;
    }

    pub fn suspend(&mut self, reason: &'static str) {
        if self.active() {
            self.set(Phase::Suspended, reason);
        }
    }

    pub fn capture(&mut self, distance: f32, stop: f32) {
        let minimum = 2. * eq_client_core::movement::capsule_radius();
        if distance < minimum || stop < minimum {
            self.suspend("orbit_radius");
        } else {
            self.radius.get_or_insert(distance.min(stop));
            self.set(Phase::Locked, "in_range");
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::motion) struct Input {
    /// Positive is right. Bound strafe and turn actions share this axis.
    pub side: f32,
    pub strafe: f32,
    pub walking: bool,
    /// Current ordinary turn budget in heading units; absent uses its cap.
    pub turn_budget: Option<f32>,
    pub observed_seconds: Option<f32>,
}

pub(in crate::motion) struct Plan {
    pub heading: f32,
    pub motion: MotionInput,
}

/// A chord whose arc and facing change both fit the current ordinary grants.
/// Radial recovery uses the existing approach owner, never added full-speed
/// on top of this lateral sample.
pub(super) fn plan(
    from: Vec3,
    target: Vec3,
    heading: f32,
    seconds: f32,
    input: Input,
) -> Option<Plan> {
    plan_with_budget(from, target, heading, seconds, input, TURN_RATE * seconds)
}

/// The same chord, bounded by facing already consumed by the keyboard owner.
pub(super) fn plan_with_budget(
    from: Vec3,
    target: Vec3,
    heading: f32,
    seconds: f32,
    input: Input,
    turn_budget: f32,
) -> Option<Plan> {
    let valid_interval = seconds > 0. && seconds <= 0.25;
    if !valid_interval
        || !(0.0..=f32::MAX).contains(&turn_budget)
        || !input.strafe.is_finite()
        || input.strafe <= 0.
        || !input.side.is_finite()
        || input.side == 0.
    {
        return None;
    }
    let radial = (from - target).with_y(0.);
    let radius = radial.length();
    if radius < 2. * eq_client_core::movement::capsule_radius() || !radius.is_finite() {
        return None;
    }
    let toward = -radial;
    let desired = eq_client_core::world_heading(toward.x.atan2(toward.z));
    let budget = input.turn_budget.unwrap_or(TURN_RATE * seconds);
    if !budget.is_finite() || budget < 0. {
        return None;
    }
    let angular_budget =
        (budget.min(turn_budget).min(TURN_RATE * seconds) - arc(heading, desired).abs()).max(0.);
    let angle =
        (input.strafe * seconds / radius).min(angular_budget * std::f32::consts::TAU / 512.);
    if angle <= f32::EPSILON {
        return None;
    }
    // Y cross radial equals facing cross Y, the ordinary right-strafe basis.
    let end = Quat::from_rotation_y(input.side.signum() * angle) * radial;
    let delta = end - radial;
    let facing = -end;
    Some(Plan {
        heading: eq_client_core::world_heading(facing.x.atan2(facing.z)),
        motion: MotionInput {
            mode: MovementMode::Strafe(if input.walking { Pace::Walk } else { Pace::Run }),
            speed: (delta.length() / seconds).min(input.strafe),
            direction: delta.normalize_or_zero(),
            turn: 0.,
            preserve_facing: true,
            diagonal: None,
        },
    })
}

#[cfg(test)]
mod guidance_tests {
    use super::*;

    #[test]
    fn orbit_chord_consumes_only_the_achieved_facing_budget() {
        let from =
            -10. * eq_client_core::axes::from_world(eq_client_core::axes::heading_direction(0.));
        for side in [-1., 1.] {
            let input = Input {
                side,
                strafe: 6.,
                walking: false,
                ..Default::default()
            };
            let normal = plan(from, Vec3::ZERO, 0., 0.1, input).unwrap();
            let limited = plan_with_budget(from, Vec3::ZERO, 0., 0.1, input, 1. / 3.).unwrap();
            assert!(arc(0., limited.heading).abs() <= 1. / 3. + 0.0001);
            assert!(limited.motion.speed < normal.motion.speed);
            assert!(limited.motion.direction.dot(Vec3::Y.cross(from)) * side > 0.);
            let end = from + limited.motion.direction * limited.motion.speed * 0.1;
            assert!((end.length() - from.length()).abs() < 0.0001);
            assert!(plan_with_budget(from, Vec3::ZERO, 0., 0.1, input, 0.).is_none());
            assert!(plan_with_budget(from, Vec3::ZERO, 0., 0.1, input, f32::NAN).is_none());
        }
    }
}

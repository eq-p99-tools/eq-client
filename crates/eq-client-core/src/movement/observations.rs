//! Per-trial evidence from the collision owner's original queries.
use super::CollisionWorld;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

const CHECKED: u32 = 1;
const SOLID: u32 = 2;
const SUPPORT: u32 = 4;
const LIQUID: u32 = 8;
const UNKNOWN: u32 = 16;

/// Outcomes retained from an observed collision-world trial.
/// This reports only the queries actually made; it is not a movement grant.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // Independent owner outcomes, not movement states.
pub struct ContactReport {
    /// At least one owner query or guard outcome was observed.
    pub checked: bool,
    /// An original sweep/contact query met a solid, even if a retry cleared it.
    pub solid: bool,
    /// An original support probe found a nearby floor.
    pub support: bool,
    /// An original liquid guard shortened or refused a leg.
    pub liquid_stop: bool,
    /// An original collision query could not establish a result.
    pub unknown: bool,
    /// Number of owner outcomes recorded, for bounded offline cost checks.
    pub checks: u32,
}

impl ContactReport {
    /// All observed outcomes were clear. Callers must separately establish
    /// initial overlap, the required path coverage and the resolved medium.
    #[must_use]
    pub const fn clear(self) -> bool {
        self.checked && !(self.solid || self.support || self.liquid_stop || self.unknown)
    }
}

#[derive(Default)]
struct Record {
    flags: AtomicU32,
    checks: AtomicU32,
}

/// A local trial's observation token. Ordinary collision worlds do not have one.
#[derive(Clone, Default)]
pub struct MotionObservation(Arc<Record>);

impl MotionObservation {
    /// Reads completed owner outcomes without changing movement or geometry.
    #[must_use]
    pub fn report(&self) -> ContactReport {
        let flags = self.0.flags.load(Ordering::Relaxed);
        ContactReport {
            checked: flags & CHECKED != 0,
            solid: flags & SOLID != 0,
            support: flags & SUPPORT != 0,
            liquid_stop: flags & LIQUID != 0,
            unknown: flags & UNKNOWN != 0,
            checks: self.0.checks.load(Ordering::Relaxed),
        }
    }

    fn record(&self, flags: u32) {
        self.0.flags.fetch_or(CHECKED | flags, Ordering::Relaxed);
        self.0.checks.fetch_add(1, Ordering::Relaxed);
    }
}

impl CollisionWorld {
    /// Shares this exact world's geometry and rules while retaining its own
    /// query outcomes. No collider is rebuilt, omitted, or replaced. The token
    /// is fresh even when this source world already belongs to another trial.
    #[must_use]
    pub fn observe_motion(&self) -> (Self, MotionObservation) {
        let observation = MotionObservation::default();
        let mut world = self.clone();
        world.observation = Some(observation.clone());
        (world, observation)
    }

    pub(super) fn observe_sweep<T>(&self, result: &Result<Option<T>, parry3d::query::Unsupported>) {
        if let Some(observation) = &self.observation {
            observation.record(match result {
                Ok(None) => 0,
                Ok(Some(_)) => SOLID,
                Err(_) => UNKNOWN,
            });
        }
    }

    pub(super) fn observe_support(&self, present: bool) {
        if let Some(observation) = &self.observation {
            observation.record(if present { SUPPORT } else { 0 });
        }
    }

    pub(super) fn observe_unknown(&self) {
        if let Some(observation) = &self.observation {
            observation.record(UNKNOWN);
        }
    }

    pub(super) fn observe_liquid_stop(&self) {
        if let Some(observation) = &self.observation {
            observation.record(LIQUID);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::{AirborneController, MotionStep, PROVISIONAL_PHYSICS};
    use glam::Vec3;

    fn plane(y: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [[-20., y, -20.], [20., y, 20.], [20., y, -20.]],
            [[-20., y, -20.], [-20., y, 20.], [20., y, 20.]],
        ]
    }
    fn floor(y: f32) -> CollisionWorld {
        CollisionWorld::new(plane(y)).unwrap()
    }
    fn bits(v: Vec3) -> [u32; 3] {
        v.to_array().map(f32::to_bits)
    }

    #[test]
    fn trial_tokens_are_isolated_and_share_the_original_geometry() {
        let original = floor(0.);
        let (observed, first) = original.observe_motion();
        assert!(Arc::ptr_eq(&original.terrain, &observed.terrain));
        assert!(Arc::ptr_eq(&original.obstacles, &observed.obstacles));
        assert!(Arc::ptr_eq(
            &original.spawn_obstacles,
            &observed.spawn_obstacles
        ));
        assert!(original.observation.is_none());
        assert!(!first.report().clear());
        let _ = observed.support(Vec3::ZERO);
        assert!(first.report().support);
        let (_, second) = observed.observe_motion();
        assert_eq!(second.report(), ContactReport::default());
        assert!(first.report().support);
    }

    #[test]
    fn observing_the_ordinary_solver_changes_no_motion_or_state() {
        for floor_y in [0_f32, -100.] {
            for jump in [false, true] {
                let original = floor(floor_y);
                let (observed, _) = original.observe_motion();
                let (mut a, mut b) = (AirborneController::default(), AirborneController::default());
                let mut feet = Vec3::ZERO;
                for (index, span) in [0.016_666_668_f32, 0.05, 0.1, 0.117, 0.15, 0.167, 0.2, 0.25]
                    .into_iter()
                    .enumerate()
                {
                    let step = MotionStep {
                        horizontal: Vec3::X * 4. * span,
                        seconds: span,
                        height: 6.,
                        jump: jump && index == 0,
                    };
                    let old = a.step_into_water(&original, feet, PROVISIONAL_PHYSICS, step);
                    let new = b.step_into_water(&observed, feet, PROVISIONAL_PHYSICS, step);
                    assert_eq!(bits(old), bits(new));
                    assert_eq!(a.velocity().to_bits(), b.velocity().to_bits());
                    assert_eq!(bits(a.drift()), bits(b.drift()));
                    assert_eq!(a.peak(), b.peak());
                    assert_eq!(a.take_landing(), b.take_landing());
                    assert_eq!(a.take_jump(), b.take_jump());
                    feet = new;
                }
            }
        }
    }

    #[test]
    fn an_unblocked_flight_does_not_prove_initial_swim_clearance() {
        let original = floor(0.);
        let (moving, motion) = original.observe_motion();
        let end = moving.fly_step(Vec3::ZERO, Vec3::X, 4.);
        assert_eq!(bits(end), bits(Vec3::X));
        assert!(
            motion.report().clear(),
            "negative control: nonclosing floor is retired by flight"
        );
        let (checked, contact) = original.observe_motion();
        assert!(checked.swim_clearance(Vec3::ZERO, 4.).is_some());
        assert!(contact.report().solid);
        assert!(!contact.report().clear());
        // The independent contact qualification never changes ordinary flight.
        assert_eq!(bits(checked.fly_step(Vec3::ZERO, Vec3::X, 4.)), bits(end));
    }

    #[test]
    fn the_first_upper_body_hit_survives_a_clear_downward_retry() {
        // Existing Temple of Life ramp fixture, not a new collision rule.
        let (lip, crease, far) = (-6., -3., 22.);
        let original = CollisionWorld::new(
            [
                [[0., 1., 0.], [0., 8., far], [14., 8., far]],
                [[14., 8., far], [0., 0., crease], [0., 1., 0.]],
                [[14., 1., 0.], [14., 0., crease], [0., 0., crease]],
                [[0., 0., crease], [14., 8., far], [14., 1., 0.]],
                [[0., 1., 0.], [0., 10., far], [0., 1., lip]],
                [[0., 10., far], [0., 1., 0.], [0., 8., far]],
                [[0., 1., 0.], [0., 0., lip], [0., 0., crease]],
                [[0., 0., lip], [0., 1., 0.], [0., 1., lip]],
                [[14., -1., lip], [0., -1., lip], [0., 0., crease]],
                [[0., 0., crease], [14., 0., crease], [14., -1., lip]],
                [[0., 0., crease], [0., 0., lip], [0., -1., lip]],
            ]
            .into_iter()
            .chain(plane(-16.)),
        )
        .unwrap();
        let mut witnessed = 0;
        for origin in [Vec3::new(1.5, -1.6, 0.025), Vec3::new(1.55, -1.5, -0.005)] {
            let feet = origin - Vec3::Y * 3.75;
            for height in [6_f32, 6.677_319_5] {
                let drop = -0.58;
                let first = original.sweep(feet, Vec3::Y * drop, height).unwrap();
                let resolved = original.fall_sweep(feet, drop, height).unwrap();
                let retries =
                    first.is_some_and(|hit| hit.time_of_impact == 0.) && resolved.is_none();
                println!("retry fixture origin={origin:?} height={height} retry={retries}");
                if !retries {
                    continue;
                }
                let (observed, contact) = original.observe_motion();
                let mut old = AirborneController::default();
                old.carry_from_water(-10., Vec3::ZERO, feet.y);
                let mut new = old.clone();
                let step = MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: 0.05,
                    height,
                };
                let a = old.step_into_water(&original, feet, PROVISIONAL_PHYSICS, step);
                let b = new.step_into_water(&observed, feet, PROVISIONAL_PHYSICS, step);
                assert_eq!(bits(a), bits(b));
                assert!(b.y < feet.y);
                assert!(contact.report().solid);
                assert!(!contact.report().clear());
                witnessed += 1;
            }
        }
        assert!(
            witnessed > 0,
            "fixture must establish an actual initial hit and clear lower-body retry"
        );
    }

    #[test]
    fn floor_wall_and_invalid_owner_inputs_are_not_clear_trials() {
        let (supported, support) = floor(0.).observe_motion();
        let _ = supported.support(Vec3::ZERO);
        assert!(support.report().support);
        assert!(!support.report().clear());
        let original = CollisionWorld::new(plane(-100.).into_iter().chain([
            [[0., -20., -20.], [0., 20., 20.], [0., 20., -20.]],
            [[0., -20., -20.], [0., -20., 20.], [0., 20., 20.]],
        ]))
        .unwrap();
        let (wall, contact) = original.observe_motion();
        let start = Vec3::new(-2., 0., 0.);
        assert_eq!(
            bits(wall.fly_step(start, Vec3::X * 4., 6.)),
            bits(original.fly_step(start, Vec3::X * 4., 6.))
        );
        assert!(contact.report().solid);
        assert!(!contact.report().clear());
        let (invalid, unknown) = floor(-100.).observe_motion();
        assert_eq!(invalid.swim_clearance(Vec3::ZERO, f32::NAN), None);
        assert!(unknown.report().unknown);
        assert!(!unknown.report().clear());
    }

    #[test]
    fn an_unsupported_owner_result_is_retained_before_masking() {
        let (world, contact) = floor(-100.).observe_motion();
        // Interface contract, not a claim of an installed unsupported mesh.
        let result: Result<Option<()>, _> = Err(parry3d::query::Unsupported);
        world.observe_sweep(&result);
        assert!(result.ok().flatten().is_none());
        assert!(contact.report().unknown);
        assert!(!contact.report().clear());
    }
}

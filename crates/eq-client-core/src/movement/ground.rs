//! Ordinary dry velocity belongs to observed frames, never to a future sample.
use super::{
    AirborneController, CollisionWorld, Liquid, LiquidContact, MAX_GROUNDED_STEP, MotionStep,
    PROGRAM_STEP_SECONDS, SUPPORT_OFFSETS, VerticalPhysics,
};
use glam::{DVec3, Vec3};
use std::time::Duration;

const DRAG: f32 = 0.625;
const INPUT: f32 = 0.8;
const OWN_CLEAR: f32 = 0.01;
const FRAME_CAP: f32 = 0.15;

/// Locally owned ordinary dry position and horizontal velocity.
///
/// The Titanium read (6.670/253) supplies the per-frame drag/input recurrence,
/// the own-velocity clear and the 150 ms frame cap. The host uses its existing
/// precise frame clock rather than emulating a whole-millisecond OS clock.
/// This choice must be compared separately with the native whole-ms reference.
/// Construction, stepping and a drawn pose establish no movement admission.
#[derive(Clone, Copy, Debug)]
pub struct GroundMotion {
    // Retain sub-coordinate rounding in clear travel. Every collision query and
    // drawn/network position still uses the same f32 scene coordinates.
    position: DVec3,
    velocity: Vec3,
}

impl GroundMotion {
    /// Begins at an observed, stationary placement. The first step still has
    /// to establish its dry, static, grounded support from the real collision owner.
    #[must_use]
    pub fn new(feet: Vec3) -> Option<Self> {
        feet.is_finite().then_some(Self {
            position: feet.as_dvec3(),
            velocity: Vec3::ZERO,
        })
    }

    /// Latest collision-resolved feet, in the scene's render coordinates.
    #[must_use]
    pub fn feet(self) -> Vec3 {
        self.position.as_vec3()
    }

    /// Current owned horizontal velocity, in scene units per second.
    #[must_use]
    pub const fn velocity(self) -> Vec3 {
        self.velocity
    }

    /// One actual frame's input and geometry, on a disposable copy.
    ///
    /// `forward` is the genuinely observed input; a zero displacement never
    /// implies release. `direction` is its horizontal facing and `speed` is
    /// the current independently granted ordinary Forward cap. The velocity
    /// remains owned state when collision stops a position; it is not rebuilt
    /// from interval travel. No native stationary-delta inference is added.
    ///
    /// None changes nothing. Unknown liquid, moving support, an unsupported edge or
    /// an invalid body/control keeps the existing ordinary movement path.
    #[must_use]
    pub fn frame(
        self,
        world: &CollisionWorld,
        height: f32,
        direction: Vec3,
        speed: f32,
        forward: bool,
        elapsed: Duration,
    ) -> Option<Self> {
        self.frame_using(world, height, direction, speed, forward, elapsed, None)
    }

    /// Uses the same grounded body physics as the ordinary walking controller.
    /// An airborne result declines; no new vertical velocity owner is created.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn frame_with_physics(
        self,
        world: &CollisionWorld,
        height: f32,
        direction: Vec3,
        speed: f32,
        forward: bool,
        elapsed: Duration,
        physics: VerticalPhysics,
    ) -> Option<Self> {
        self.frame_using(
            world,
            height,
            direction,
            speed,
            forward,
            elapsed,
            Some(physics),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn frame_using(
        self,
        world: &CollisionWorld,
        height: f32,
        direction: Vec3,
        speed: f32,
        forward: bool,
        elapsed: Duration,
        physics: Option<VerticalPhysics>,
    ) -> Option<Self> {
        let climb = physics.map_or(MAX_GROUNDED_STEP, |physics| physics.reach());
        if !climb.is_finite() || climb <= 0.0 {
            return None;
        }
        let feet = self.feet();
        if !speed.is_finite()
            || speed <= 0.0
            || !direction.is_finite()
            || !height.is_finite()
            || height < 1.0
            || world.support(feet).is_none()
            || !world.plain_ground(feet, height, climb)
        {
            return None;
        }
        let direction = direction.with_y(0.0).normalize_or_zero();
        if forward && direction == Vec3::ZERO {
            return None;
        }
        // Normalize the read's native input to the *existing* calibrated steady
        // speed. This keeps the 0.8 input factor, not a fitted speed multiplier.
        let input = direction * (speed * (1.0 - DRAG) / INPUT);
        let mut velocity = self.velocity * DRAG;
        if forward {
            velocity += input * INPUT;
        } else {
            for component in [&mut velocity.x, &mut velocity.z] {
                if (*component * PROGRAM_STEP_SECONDS).abs() <= OWN_CLEAR {
                    *component = 0.0;
                }
            }
        }
        let magnitude = velocity.x.hypot(velocity.z);
        if magnitude > speed {
            velocity *= speed / magnitude;
            // f32 normalization can round one ULP above the unchanged cap.
            if velocity.x.hypot(velocity.z) > speed {
                velocity *= 1.0 - f32::EPSILON;
            }
        }
        let seconds = elapsed.as_secs_f32().min(FRAME_CAP);
        let proposed = self.position + (velocity * seconds).as_dvec3();
        let target = proposed.as_vec3();
        // Keep the ordinary water-entry handback: a grounded sweep can stop
        // at a dry prefix, which must not strand this owner at the shoreline.
        if !world.plain_ground_segment(feet, target, height)
            || !matches!(
                world.liquid_contact(target, height),
                LiquidContact::Unmarked
            )
        {
            return None;
        }
        // The existing ordinary sweep is the collision owner. Its diagnostic
        // first-contact query is deliberately not used as movement evidence.
        let landed = if let Some(physics) = physics {
            let mut airborne = AirborneController::default();
            let landed = airborne.step(
                world,
                feet,
                physics,
                MotionStep {
                    horizontal: target - feet,
                    jump: false,
                    seconds,
                    height,
                },
            );
            if airborne.velocity() != 0.0 || airborne.peak().is_some() {
                return None;
            }
            landed
        } else {
            world.step(feet, target - feet, height)
        };
        if world.support(landed).is_none()
            || !world.plain_ground(landed, height, climb)
            || !world.plain_ground_segment(feet, landed, height)
        {
            return None;
        }
        let position = if (landed.x, landed.z) == (target.x, target.z) {
            DVec3::new(proposed.x, f64::from(landed.y), proposed.z)
        } else {
            landed.as_dvec3()
        };
        Some(Self { position, velocity })
    }
}

impl CollisionWorld {
    /// Positive footprint evidence from this same geometry; virtual liquid
    /// floors and transient door/carrier coordinates never qualify it.
    fn plain_ground(&self, feet: Vec3, height: f32, climb: f32) -> bool {
        if self.liquid_support
            || !matches!(self.liquid_contact(feet, height), LiquidContact::Unmarked)
        {
            return false;
        }
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for (x, z) in SUPPORT_OFFSETS {
            let point = feet + Vec3::new(x, 0., z);
            let Some(floor) = self.solid_ground(point, climb, MAX_GROUNDED_STEP) else {
                return false;
            };
            if self.ground_door(point, climb, MAX_GROUNDED_STEP).is_some()
                || self.ground_spawn(point, climb, MAX_GROUNDED_STEP).is_some()
            {
                return false;
            }
            low = low.min(floor);
            high = high.max(floor);
        }
        // Use the ordinary walk's existing rise/drop budget for the footprint,
        // not a flatness test. The caller separately requires actual narrow
        // support at the starting and resolved feet: this wider lookahead can
        // never acquire an airborne body. world.step still owns every move.
        high - low <= climb.max(MAX_GROUNDED_STEP)
    }

    fn plain_ground_segment(&self, from: Vec3, to: Vec3, height: f32) -> bool {
        if from == to {
            return true;
        }
        self.liquids.as_ref().is_some_and(|liquids| {
            [0.1, height * 0.5, height].into_iter().all(|rise| {
                let (from, to) = (from + Vec3::Y * rise, to + Vec3::Y * rise);
                liquids.segment_known(from, to)
                    && !liquids.passes_through(from, to, Liquid::Water)
                    && !liquids.passes_through(from, to, Liquid::Lava)
            })
        })
    }
}

#[cfg(test)]
#[path = "ground_tests.rs"]
mod tests;

//! Ordinary dry velocity belongs to observed frames, never to a future sample.
use super::{CollisionWorld, Liquid, LiquidContact, PROGRAM_STEP_SECONDS, SUPPORT_OFFSETS};
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
    /// to establish its dry, static, level support from the real collision owner.
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
    /// None changes nothing. Unknown liquid, moving support, a slope/edge or
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
        let feet = self.feet();
        if !speed.is_finite()
            || speed <= 0.0
            || !direction.is_finite()
            || !height.is_finite()
            || height < 1.0
            || !world.plain_ground(feet, height)
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
        if !world.plain_ground(target, height) || !world.plain_ground_segment(feet, target, height)
        {
            return None;
        }
        // The existing ordinary sweep is the collision owner. Its diagnostic
        // first-contact query is deliberately not used as movement evidence.
        let landed = world.step(feet, target - feet, height);
        if !world.plain_ground(landed, height) {
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
    fn plain_ground(&self, feet: Vec3, height: f32) -> bool {
        if self.liquid_support
            || !matches!(self.liquid_contact(feet, height), LiquidContact::Unmarked)
        {
            return false;
        }
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        for (x, z) in SUPPORT_OFFSETS {
            let point = feet + Vec3::new(x, 0., z);
            let Some(floor) = self.solid_ground(point, 0.01, 0.05) else {
                return false;
            };
            if self.ground_door(point, 0.01, 0.05).is_some()
                || self.ground_spawn(point, 0.01, 0.05).is_some()
            {
                return false;
            }
            low = low.min(floor);
            high = high.max(floor);
        }
        // The normal support probe already uses a 0.01 upward window. This
        // eligibility is narrower than ordinary walking; it changes no sweep.
        high - low <= 0.01
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

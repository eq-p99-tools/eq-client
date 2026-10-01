//! Explicit vertical physics for offline locomotion; no protocol tuning is assumed.
use super::{CollisionWorld, Support};
use glam::Vec3;

/// Explicit local tuning, separate from network motion calibration.
#[derive(Clone, Copy)]
pub struct VerticalPhysics {
    /// Downward acceleration in world units per second squared.
    pub gravity: f32,
    /// Maximum downward speed in world units per second.
    pub terminal_speed: f32,
    /// Initial upward speed for a grounded jump.
    pub jump_speed: f32,
}

/// Provisional tuning shared by the offline preview, online falls and path search.
/// Not measured EQ gravity or jump impulse; the terminal speed is the fastest
/// descent the network accepts for a fall.
pub const PROVISIONAL_PHYSICS: VerticalPhysics = VerticalPhysics {
    gravity: 32.0,
    terminal_speed: super::MAX_FALL_SPEED,
    jump_speed: 10.0,
};

/// One bounded local simulation step, using renderer Y-up coordinates.
#[derive(Clone, Copy)]
pub struct MotionStep {
    /// Requested horizontal displacement, already bounded by the caller's speed budget.
    pub horizontal: Vec3,
    /// A new jump press, not a held key.
    pub jump: bool,
    /// Elapsed simulation time; capped at 50 ms rather than catching up after stalls.
    pub seconds: f32,
    /// Character collision height.
    pub height: f32,
}

/// A local airborne-to-ground transition, independent of server damage rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    /// Drop from the highest simulated position, in world units.
    pub fall_distance: f32,
    /// Downward speed at contact, in world units per second, not damage or wire velocity.
    pub impact_speed: f32,
}

/// Ground directly under feet that are below its surface, within the capsule's
/// lift, when there is head room to stand on it. Feet end up there after a
/// server-placed move made with a different feet offset; since the floating
/// capsule never touches that floor, the character would otherwise fall until
/// the capsule rested on the ground, leaving the feet buried in it.
fn buried_floor(world: &CollisionWorld, feet: Vec3, height: f32) -> Option<f32> {
    let floor = world.ground(feet, super::LIFT, 0.0)?;
    (floor > feet.y + 0.001
        && matches!(
            world.sweep(feet, Vec3::Y * (floor - feet.y), height),
            Ok(None)
        ))
    .then_some(floor)
}

/// Whether feet stand on ground the next step finds: under the footprint, or a
/// floor they are buried in.
fn supported(world: &CollisionWorld, feet: Vec3, height: f32) -> bool {
    world.support_height(feet, 0.01, 0.05).is_some() || buried_floor(world, feet, height).is_some()
}

/// Vertical velocity survives frames with no keyboard input.
#[derive(Clone, Default)]
pub struct AirborneController {
    velocity: f32,
    peak: Option<f32>,
    landing: Option<Landing>,
}

impl AirborneController {
    /// Instantaneous upward-positive velocity; not an EQ wire velocity.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Resets momentum after a correction, teleport, or a new admission.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Takes the latest unconsumed landing; standing frames never repeat it.
    pub fn take_landing(&mut self) -> Option<Landing> {
        self.landing.take()
    }

    fn finish_landing(&mut self, floor: f32, impact_speed: f32) {
        if let Some(peak) = self.peak.take() {
            self.landing = Some(Landing {
                fall_distance: (peak - floor).max(0.0),
                impact_speed,
            });
        }
        self.velocity = 0.0;
    }

    /// Advances horizontal collision and vertical flight, clipping against floors and ceilings.
    /// Invalid inputs preserve position and momentum; no network packets are generated.
    pub fn step(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        physics: VerticalPhysics,
        input: MotionStep,
    ) -> Vec3 {
        if !feet.is_finite()
            || !input.horizontal.is_finite()
            || !input.height.is_finite()
            || input.height < 1.0
            || !input.seconds.is_finite()
            || input.seconds <= 0.0
            || ![physics.gravity, physics.terminal_speed, physics.jump_speed]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0)
        {
            return feet;
        }
        let dt = input.seconds.min(0.05);
        let feet = match buried_floor(world, feet, input.height) {
            Some(floor) if self.velocity <= 0.0 => Vec3::new(feet.x, floor, feet.z),
            _ => feet,
        };
        let floor = world.support_height(feet, 0.01, 0.05);
        let grounded = self.velocity <= 0.0 && floor.is_some();
        if grounded {
            self.finish_landing(floor.unwrap_or(feet.y), -self.velocity);
            self.velocity = if input.jump { physics.jump_speed } else { 0.0 };
        }
        let support = if grounded && !input.jump {
            Support::Optional
        } else {
            Support::Airborne
        };
        let position = world.walk(feet, input.horizontal, input.height, support);
        if self.velocity <= 0.0
            && let Some(floor) = world.support_height(position, 0.01, 0.05)
        {
            self.finish_landing(floor, -self.velocity);
            return Vec3::new(position.x, floor, position.z);
        }
        self.peak = Some(self.peak.unwrap_or(feet.y).max(position.y));
        let initial = self.velocity.max(-physics.terminal_speed);
        let accelerating = ((initial + physics.terminal_speed) / physics.gravity).clamp(0.0, dt);
        let end = (initial - physics.gravity * dt).max(-physics.terminal_speed);
        let distance = initial * accelerating
            - 0.5 * physics.gravity * accelerating * accelerating
            - physics.terminal_speed * (dt - accelerating);
        let mut target = position + Vec3::Y * distance;
        let mut landed = false;
        if distance <= 0.0
            && let Some(floor) = world.support_height(position, 0.01, -distance + 0.05)
            && target.y <= floor
        {
            target.y = floor;
            landed = true;
        }
        let distance = target.y - position.y;
        if distance.abs() < 0.000_001 {
            if landed {
                self.finish_landing(target.y, -initial);
            } else {
                self.velocity = end;
            }
            return position;
        }
        match world.sweep(position, Vec3::Y * distance, input.height) {
            Ok(None) => {
                self.peak = self.peak.map(|peak| peak.max(target.y));
                if landed {
                    let impact = (initial * initial + 2.0 * physics.gravity * (-distance).max(0.0))
                        .sqrt()
                        .min(physics.terminal_speed);
                    self.finish_landing(target.y, impact);
                } else {
                    self.velocity = end;
                }
                target
            }
            Ok(Some(hit)) => self.blocked(world, position, distance, &hit, end, input.height),
            Err(_) => position,
        }
    }

    /// Ends a vertical move that met something: rests on it, or keeps falling
    /// along it when the feet do not stand on it (a slope too steep to stand on,
    /// or the edge of a step the feet came down beside) instead of hanging there.
    fn blocked(
        &mut self,
        world: &CollisionWorld,
        position: Vec3,
        distance: f32,
        hit: &parry3d::query::ShapeCastHit,
        end: f32,
        height: f32,
    ) -> Vec3 {
        let fraction = (hit.time_of_impact - 0.005 / distance.abs()).clamp(0.0, 1.0);
        let contact = position + Vec3::Y * distance * fraction;
        self.peak = self.peak.map(|peak| peak.max(contact.y));
        // The obstacle's surface normal (the hit's is the capsule's own).
        let surface = -Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
        let rest = Vec3::Y * distance * (1.0 - fraction);
        let slide = rest - surface * rest.dot(surface);
        if distance >= 0.0
            || surface.y <= 0.0
            || slide.length_squared() <= 1e-8
            || supported(world, contact, height)
        {
            self.velocity = 0.0;
            return contact;
        }
        self.velocity = end;
        match world.sweep(contact, slide, height) {
            Ok(None) => contact + slide,
            Ok(Some(block)) => {
                contact + slide * (block.time_of_impact - 0.005 / slide.length()).clamp(0.0, 1.0)
            }
            Err(_) => contact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn floor(y: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [[-20.0, y, -20.0], [20.0, y, -20.0], [20.0, y, 20.0]],
            [[-20.0, y, -20.0], [20.0, y, 20.0], [-20.0, y, 20.0]],
        ]
    }
    fn tick(
        state: &mut AirborneController,
        world: &CollisionWorld,
        feet: Vec3,
        jump: bool,
    ) -> Vec3 {
        state.step(
            world,
            feet,
            VerticalPhysics {
                gravity: 32.0,
                terminal_speed: 40.0,
                jump_speed: 10.0,
            },
            MotionStep {
                horizontal: Vec3::ZERO,
                jump,
                seconds: 0.05,
                height: 6.0,
            },
        )
    }
    #[test]
    fn a_fall_onto_a_slope_too_steep_to_stand_on_slides_down_it() {
        // Floor at y 0 for x < 0, then a 60-degree slope rising along +x.
        let rise = 60.0_f32.to_radians().tan() * 10.0;
        let world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [0.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
            [[0.0, 0.0, -20.0], [10.0, rise, -20.0], [10.0, rise, 20.0]],
            [[0.0, 0.0, -20.0], [10.0, rise, 20.0], [0.0, 0.0, 20.0]],
        ])
        .unwrap();
        let mut state = AirborneController::default();
        let mut position = Vec3::new(5.0, 20.0, 0.0);
        for _ in 0..400 {
            position = tick(&mut state, &world, position, false);
        }
        assert!(position.y.abs() < 0.01 && position.x < 0.5, "{position:?}");
        assert!(state.velocity().abs() < 0.001);
    }

    #[test]
    fn a_fall_beside_a_step_slides_off_its_edge_onto_the_floor() {
        // Floor at y 0, and a step 1.5 high for x >= 0. The feet start below its
        // top, beside it, where the floating capsule meets its edge on the way down.
        let world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
            [[0.0, 1.5, -20.0], [20.0, 1.5, -20.0], [20.0, 1.5, 20.0]],
            [[0.0, 1.5, -20.0], [20.0, 1.5, 20.0], [0.0, 1.5, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 1.5, -20.0], [0.0, 1.5, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 1.5, 20.0], [0.0, 0.0, 20.0]],
        ])
        .unwrap();
        for x in [-0.1, -0.24, -0.35] {
            let mut state = AirborneController::default();
            let mut position = Vec3::new(x, 1.0, 0.0);
            for _ in 0..100 {
                position = tick(&mut state, &world, position, false);
            }
            // Never left hanging on the edge with no ground under the feet.
            assert!(
                position.y.abs() < 0.001 && position.x < x,
                "{x}: {position:?}"
            );
            assert!(state.velocity().abs() < 0.001, "{x}: {}", state.velocity());
        }
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn released_input_continues_falling_and_lands_without_crossing_floor() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        let mut state = AirborneController::default();
        let mut position = Vec3::Y * 10.0;
        for _ in 0..80 {
            let next = tick(&mut state, &world, position, false);
            assert!(next.y <= position.y && next.y >= -0.001);
            position = next;
        }
        assert!(position.y.abs() < 0.001);
        assert_eq!(state.velocity(), 0.0);
        let landing = state.take_landing().unwrap();
        assert!((landing.fall_distance - 10.0).abs() < 0.001);
        assert!((landing.impact_speed - (2.0_f32 * 32.0 * 10.0).sqrt()).abs() < 0.1);
        assert!(state.take_landing().is_none());
        tick(&mut state, &world, position, false);
        assert!(state.take_landing().is_none());
    }
    #[test]
    fn jump_has_an_apex_and_cannot_restart_in_midair() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        let mut state = AirborneController::default();
        let mut position = tick(&mut state, &world, Vec3::ZERO, true);
        assert!(position.y > 0.0);
        let mut max_height = position.y;
        for _ in 0..40 {
            position = tick(&mut state, &world, position, position.y > 0.1);
            max_height = max_height.max(position.y);
        }
        assert!(max_height > 1.4 && max_height < 1.6);
        assert!(position.y.abs() < 0.001);
        state.reset();
        assert!(state.take_landing().is_none());
    }
    #[test]
    fn feet_just_under_the_ground_stand_on_it_instead_of_sinking() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        let mut state = AirborneController::default();
        // As after a teleport whose height assumed a different feet offset.
        let stood = tick(&mut state, &world, Vec3::new(0.0, -0.3, 0.0), false);
        assert!(stood.y.abs() < 0.001, "{stood:?}");
        assert!(state.velocity().abs() < 0.001);
        assert!(state.take_landing().is_none());
        // Deeper than the capsule's lift is inside the ground, not standing on it.
        let mut state = AirborneController::default();
        let deep = Vec3::new(0.0, -1.0, 0.0);
        assert!(tick(&mut state, &world, deep, false).y <= deep.y);
        // No head room: a ceiling right above the capsule keeps it where it is.
        let covered = CollisionWorld::new(floor(0.0).into_iter().chain(floor(6.7))).unwrap();
        let mut state = AirborneController::default();
        let pinned = Vec3::new(0.0, -0.3, 0.0);
        assert!(tick(&mut state, &covered, pinned, false).y <= pinned.y);
    }

    #[test]
    fn low_ceiling_stops_ascent_then_gravity_returns_to_floor() {
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(floor(7.5))).unwrap();
        let mut state = AirborneController::default();
        let mut position = Vec3::ZERO;
        for i in 0..50 {
            position = tick(&mut state, &world, position, i == 0);
            assert!(position.y < 0.7);
        }
        assert!(position.y.abs() < 0.001);
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Exact equality verifies unchanged or explicitly assigned state"
    )]
    fn walking_off_a_ledge_starts_falling_and_stalls_do_not_add_catch_up_time() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        let physics = VerticalPhysics {
            gravity: 32.0,
            terminal_speed: 40.0,
            jump_speed: 10.0,
        };
        let input = MotionStep {
            horizontal: Vec3::X * 0.6,
            jump: false,
            seconds: 0.05,
            height: 6.0,
        };
        let mut normal = AirborneController::default();
        let mut stalled = AirborneController::default();
        let start = Vec3::new(19.9, 0.0, 0.0);
        let next = normal.step(&world, start, physics, input);
        let late = stalled.step(
            &world,
            start,
            physics,
            MotionStep {
                seconds: 5.0,
                ..input
            },
        );
        assert!(next.x > 20.4 && next.y < 0.0);
        assert!(next.distance(late) < 0.0001);
        assert!(normal.velocity() < 0.0);
        let momentum = normal.velocity();
        assert_eq!(
            normal.step(
                &world,
                next,
                physics,
                MotionStep {
                    seconds: f32::NAN,
                    ..input
                }
            ),
            next
        );
        assert_eq!(normal.velocity(), momentum);
        for _ in 0..100 {
            normal.step(
                &world,
                Vec3::new(100.0, 100.0, 100.0),
                physics,
                MotionStep {
                    horizontal: Vec3::ZERO,
                    ..input
                },
            );
        }
        assert_eq!(normal.velocity(), -40.0);
        normal.reset();
        assert_eq!(normal.velocity(), 0.0);
    }
}

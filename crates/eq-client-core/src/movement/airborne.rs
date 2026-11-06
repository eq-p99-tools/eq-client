//! Explicit vertical physics for offline locomotion; no protocol tuning is assumed.
use super::{CollisionWorld, LiquidGuard, Support};
use glam::Vec3;

/// Explicit local tuning, separate from network motion calibration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerticalPhysics {
    /// Downward acceleration in world units per second squared; a zone may
    /// have none.
    pub gravity: f32,
    /// Maximum downward speed in world units per second.
    pub terminal_speed: f32,
    /// Initial upward speed for a grounded jump.
    pub jump_speed: f32,
    /// The share of the speed kept after each 50 ms program step in the air,
    /// vertical and horizontal alike; 1 keeps all of it.
    pub air_factor: f32,
    /// How far below the feet a floor may lie for a walking step to keep
    /// them on it, in world units; past it the step leaves the ground and
    /// falls. Both programs count a spawn on the ground while its feet,
    /// moved by this step's vertical speed, are within 0.5 + 0.001 of the
    /// floor (R-s: Titanium 0x48a2f4, `EQMac` 0x50ba07).
    pub ground_tolerance: f32,
    /// How far one step on the ground may lift the feet onto a floor ahead,
    /// in world units: the player's own reach in the programs (the floor
    /// sweep from the origin, read 6.344), a riser where unread.
    pub climb: f32,
    /// Whether the walk's input still moves the body in the air. Neither
    /// program's does: off the ground a step adds no input and keeps the air
    /// factor's share of the take-off speed (Titanium 0x4c0462-0x4c04ba,
    /// TAKP R1). The provisional physics keep steering, which makes no
    /// claim about either program.
    pub steers_in_air: bool,
}

/// Provisional tuning shared by the offline preview, online falls and path search.
/// Not measured EQ gravity or jump impulse; the terminal speed is the fastest
/// descent the network accepts for a fall. A walk keeps the feet on a floor
/// up to a riser below or above them, the grounded step's own rule.
pub const PROVISIONAL_PHYSICS: VerticalPhysics = VerticalPhysics {
    gravity: 32.0,
    terminal_speed: super::MAX_FALL_SPEED,
    jump_speed: 10.0,
    air_factor: 1.0,
    ground_tolerance: super::MAX_GROUNDED_STEP,
    climb: super::MAX_GROUNDED_STEP,
    steers_in_air: true,
};

/// The length of the programs' movement step, in seconds: a fall's speed
/// changes, and the feet move by it, once a step (R-s).
const STEP: f32 = super::PROGRAM_STEP_SECONDS;

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

/// The floor above the feet, up to `climb` over them, that a step stands
/// them on: the highest under their footprint, met straight down from
/// `climb` up, as the programs' floor sweep from the origin meets it.
fn risen_floor(world: &CollisionWorld, feet: Vec3, climb: f32) -> Option<f32> {
    world
        .support_height(feet, climb, 0.0)
        .filter(|floor| *floor > feet.y + 0.001)
}

/// Whether feet stand on ground the next step finds: under the footprint, or a
/// floor they are buried in.
fn supported(world: &CollisionWorld, feet: Vec3, height: f32) -> bool {
    world.support(feet).is_some() || buried_floor(world, feet, height).is_some()
}

/// A body's motion as the programs step it, one 50 ms step at a time (R-s,
/// both generations): on the ground the walk moves it; in the air each
/// step takes the zone's gravity off its vertical speed, keeps the air
/// factor's share of all its speed, and moves the feet by it, with no
/// steering (Titanium 0x4c0462-0x4c04ba, TAKP R1: no input off the ground).
/// A step that ends with the feet below a floor lands there, at that
/// step's speed. Vertical velocity survives frames with no keyboard input.
#[derive(Clone, Default)]
pub struct AirborneController {
    velocity: f32,
    /// The horizontal speed carried in the air, in world units a second:
    /// the walk's at take-off, less the air's share each step.
    drift: Vec3,
    /// Seconds since the last step, under one step.
    clock: f32,
    peak: Option<f32>,
    landing: Option<Landing>,
}

impl AirborneController {
    /// Instantaneous upward-positive velocity; not an EQ wire velocity.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// The highest the feet have been since they left the ground; None
    /// while they stand on it.
    #[must_use]
    pub const fn peak(&self) -> Option<f32> {
        self.peak
    }

    /// The horizontal speed carried in the air, in world units a second.
    #[must_use]
    pub const fn drift(&self) -> Vec3 {
        self.drift
    }

    /// Bounds carry into the existing fall mode after a grounded diagonal.
    pub fn limit_drift(&mut self, speed: f32) {
        if speed.is_finite() && speed >= 0.0 {
            self.drift = self.drift.clamp_length_max(speed);
        }
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

    /// Ends a fall the water and lava guard stopped short of them. It lands
    /// on nothing, so it leaves no landing: one in water or lava would do
    /// no harm ([`CollisionWorld::landing_damage`]).
    fn hold(&mut self) {
        self.peak = None;
        self.velocity = 0.0;
    }

    /// Advances horizontal collision and vertical flight, clipping against floors and ceilings.
    /// Invalid inputs preserve position and momentum; no network packets are generated.
    ///
    /// Each call adds its seconds to the step clock; the vertical motion
    /// moves once for each whole step, so a call shorter than a step may
    /// move only across. The programs' on-ground test is made where the
    /// feet stand before the move: feet moved by the step's vertical speed
    /// within the ground tolerance of a floor take no gravity that step
    /// (0x48a2f4 / 0x50ba07).
    #[allow(clippy::too_many_lines)]
    pub fn step(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        physics: VerticalPhysics,
        input: MotionStep,
    ) -> Vec3 {
        self.step_with_guard(world, feet, physics, input, LiquidGuard::All)
    }

    /// Ground and air motion for a session that can transition into water.
    /// The caller stops the ordinary motion at water entry and resets the fall.
    pub fn step_into_water(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        physics: VerticalPhysics,
        input: MotionStep,
    ) -> Vec3 {
        self.step_with_guard(world, feet, physics, input, LiquidGuard::Lava)
    }

    #[allow(clippy::too_many_lines)]
    fn step_with_guard(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        physics: VerticalPhysics,
        input: MotionStep,
        liquids: LiquidGuard,
    ) -> Vec3 {
        if !feet.is_finite()
            || !input.horizontal.is_finite()
            || !input.height.is_finite()
            || input.height < 1.0
            || !input.seconds.is_finite()
            || input.seconds <= 0.0
            || ![physics.terminal_speed, physics.jump_speed]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0)
            || !(physics.gravity.is_finite() && physics.gravity >= 0.0)
            || !(physics.air_factor > 0.0 && physics.air_factor <= 1.0)
            || !(physics.ground_tolerance.is_finite() && physics.ground_tolerance >= 0.0)
            || !(physics.climb.is_finite() && physics.climb >= 0.0)
        {
            return feet;
        }
        let dt = input.seconds.min(STEP);
        // The programs' floor sweep starts at the origin, the stand height
        // above the feet, and takes the first surface it meets, so feet a
        // server's move left under a floor within the body's reach stand on
        // it at once (6.344, R-s); deeper, it starts under the floor, meets
        // nothing and the body falls.
        let start = feet;
        let feet = match risen_floor(world, feet, physics.climb)
            .or_else(|| buried_floor(world, feet, input.height))
        {
            Some(floor) if self.velocity <= 0.0 => Vec3::new(feet.x, floor, feet.z),
            _ => feet,
        };
        // That one sweep is the whole step's rise: a step that stands up
        // walks on only by the reach the lift left it, so it never rises
        // more than its climb from where it started. Its move goes as far
        // up as that allows (inferred, checklist 6.366: the program's
        // horizontal sphere against the face is unread).
        let climb = (physics.climb - (feet.y - start.y)).max(0.0);
        let floor = world.support(feet);
        let grounded = self.velocity <= 0.0 && floor.is_some();
        if grounded {
            self.finish_landing(floor.unwrap_or(feet.y), -self.velocity);
            self.velocity = if input.jump { physics.jump_speed } else { 0.0 };
        }
        // A body in the air whose feet this step's speed would carry to
        // within the tolerance of a floor counts as on the ground: it takes
        // no gravity, and the walk moves it.
        let fall = -self.velocity * STEP;
        let alighting = !grounded
            && self.velocity <= 0.0
            && world
                .support_height(feet, 0.01, fall + physics.ground_tolerance)
                .is_some_and(|floor| feet.y - fall <= floor + physics.ground_tolerance);
        let on_ground = grounded || alighting;
        let steps = (self.clock + dt + 1e-6) / STEP;
        let stepped = steps >= 1.0;
        self.clock = if stepped {
            (self.clock + dt - STEP).max(0.0)
        } else {
            self.clock + dt
        };
        let horizontal = if on_ground || physics.steers_in_air {
            self.drift = input.horizontal / dt;
            input.horizontal
        } else {
            if stepped {
                self.drift *= physics.air_factor;
            }
            self.drift * dt
        };
        let support = if grounded && !input.jump {
            Support::Optional {
                drop: physics.ground_tolerance,
                climb,
            }
        } else {
            Support::Airborne
        };
        if liquids == LiquidGuard::Lava && world.swim_depth(feet, input.height).is_some() {
            return feet;
        }
        let position = world.walk_with_guard(feet, horizontal, input.height, support, liquids);
        if liquids == LiquidGuard::Lava && world.swim_depth(position, input.height).is_some() {
            return position;
        }
        if self.velocity <= 0.0
            && let Some(floor) = world.support(position)
        {
            self.finish_landing(floor, -self.velocity);
            return Vec3::new(position.x, floor, position.z);
        }
        self.peak = Some(self.peak.unwrap_or(feet.y).max(position.y));
        if !stepped {
            return position;
        }
        if !on_ground || input.jump {
            self.velocity = ((self.velocity - physics.gravity * STEP) * physics.air_factor)
                .max(-physics.terminal_speed);
        }
        let speed = self.velocity;
        let distance = speed * STEP;
        let mut target = position + Vec3::Y * distance;
        let mut landed = false;
        // A step on the ground stands the feet on the floor it found; the
        // programs' land a step later at the same speed, and a body at rest
        // there would hover.
        let reach = if alighting {
            -distance + physics.ground_tolerance
        } else {
            -distance
        };
        if distance <= 0.0
            && let Some(floor) = world.support_height(position, 0.01, reach + 0.05)
            && (alighting || target.y <= floor)
        {
            target.y = floor;
            landed = true;
        }
        // A fall that comes down lower than it was judged stops short of
        // water and lava.
        let held = if distance < 0.0 {
            world.held(position, target, input.height, liquids)
        } else {
            None
        };
        let target = held.unwrap_or(target);
        let distance = target.y - position.y;
        if distance.abs() < 0.000_001 {
            if held.is_some() {
                self.hold();
            } else if landed {
                self.finish_landing(target.y, -speed);
            }
            return position;
        }
        match world.sweep(position, Vec3::Y * distance, input.height) {
            Ok(None) if held.is_some() => {
                self.hold();
                target
            }
            Ok(None) => {
                if liquids == LiquidGuard::Lava
                    && let Some(entry) = world.water_entry(position, target, input.height)
                {
                    return entry;
                }
                self.peak = self.peak.map(|peak| peak.max(target.y));
                if landed {
                    // The program lands at the speed of the step that ended
                    // below the floor (0x48b6c5 / 0x4b479e).
                    self.finish_landing(target.y, -speed);
                }
                target
            }
            Ok(Some(hit)) => self.blocked(
                world,
                position,
                distance,
                &hit,
                speed,
                input.height,
                liquids,
            ),
            Err(_) => position,
        }
    }

    /// Ends a vertical move that met something: rests on it, or keeps falling
    /// along it when no floor is under the feet (the edge of a step the feet
    /// came down beside) instead of hanging there. A face at any angle under
    /// the feet is floor ([`CollisionWorld::ground`]), so nothing slides down
    /// a slope.
    /// A slide that would carry the feet into lava or the head under water
    /// stops short of it, and the fall is held where it stops.
    #[allow(clippy::too_many_arguments)]
    fn blocked(
        &mut self,
        world: &CollisionWorld,
        position: Vec3,
        distance: f32,
        hit: &parry3d::query::ShapeCastHit,
        end: f32,
        height: f32,
        liquids: LiquidGuard,
    ) -> Vec3 {
        let fraction = (hit.time_of_impact - 0.005 / distance.abs()).clamp(0.0, 1.0);
        let contact = position + Vec3::Y * distance * fraction;
        if liquids == LiquidGuard::Lava
            && let Some(entry) = world.water_entry(position, contact, height)
        {
            return entry;
        }
        self.peak = self.peak.map(|peak| peak.max(contact.y));
        // The obstacle's surface normal (the hit's is the capsule's own).
        let surface = -Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
        let rest = Vec3::Y * distance * (1.0 - fraction);
        let slide = rest - surface * rest.dot(surface);
        let resting = distance >= 0.0
            || surface.y <= 0.0
            || slide.length_squared() <= 1e-8
            || supported(world, contact, height);
        // A slide stops short of deep water and lava, as a step does.
        let (slide, held) = if resting {
            (Vec3::ZERO, false)
        } else {
            let fraction = world.dry_fraction(contact, slide, height, liquids);
            (slide * fraction, fraction < 1.0)
        };
        if slide.length_squared() <= 1e-8 {
            if held {
                self.hold();
            } else {
                self.velocity = 0.0;
            }
            return contact;
        }
        self.velocity = end;
        let end = match world.sweep(contact, slide, height) {
            Ok(None) => contact + slide,
            Ok(Some(block)) => {
                contact + slide * (block.time_of_impact - 0.005 / slide.length()).clamp(0.0, 1.0)
            }
            Err(_) => contact,
        };
        if liquids == LiquidGuard::Lava {
            world.water_entry(contact, end, height).unwrap_or(end)
        } else {
            end
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
    /// The provisional physics' numbers, which [`tick`] steps by.
    const TICK: VerticalPhysics = VerticalPhysics {
        gravity: 32.0,
        terminal_speed: 40.0,
        jump_speed: 10.0,
        air_factor: 1.0,
        ground_tolerance: crate::movement::MAX_GROUNDED_STEP,
        climb: crate::movement::MAX_GROUNDED_STEP,
        steers_in_air: true,
    };
    fn tick(
        state: &mut AirborneController,
        world: &CollisionWorld,
        feet: Vec3,
        jump: bool,
    ) -> Vec3 {
        state.step(
            world,
            feet,
            TICK,
            MotionStep {
                horizontal: Vec3::ZERO,
                jump,
                seconds: 0.05,
                height: 6.0,
            },
        )
    }
    #[test]
    fn a_fall_onto_a_steep_slope_lands_on_it_without_sliding() {
        // Floor at y 0 for x < 0, then a 60-degree slope rising along +x.
        // Both programs take any face under the feet as floor, at any angle,
        // and nothing turns its normal into a slide (Decompile R-s, 11-01).
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
        // On the face where it came down: no lower than the face under the
        // feet, and no higher than the face under the footprint's edge.
        let face = rise * 0.5;
        let edge = 0.4 * 60.0_f32.to_radians().tan();
        assert!(
            (position.x - 5.0).abs() < 0.001
                && position.y >= face - 0.001
                && position.y <= face + edge + 0.001,
            "{position:?}"
        );
        assert!(state.velocity().abs() < 0.001);
        assert!(state.take_landing().is_some());
    }

    #[test]
    fn walking_down_a_steep_face_stands_on_it_each_step_and_never_falls_far() {
        // A plateau at y 40 for x < 0, a 55-degree face down to the floor,
        // and the floor beyond: walking off the plateau with falls allowed
        // goes down the face, standing on it each step (the programs make
        // that a run of short falls, each landing on the face), and stands
        // on the floor at its foot. No fall on the way hurts, let alone
        // kills, and the walk is never refused.
        let foot = 40.0 / 55.0_f32.to_radians().tan();
        let world = CollisionWorld::new([
            [[-20.0, 40.0, -20.0], [0.0, 40.0, -20.0], [0.0, 40.0, 20.0]],
            [[-20.0, 40.0, -20.0], [0.0, 40.0, 20.0], [-20.0, 40.0, 20.0]],
            [[0.0, 40.0, -20.0], [foot, 0.0, -20.0], [foot, 0.0, 20.0]],
            [[0.0, 40.0, -20.0], [foot, 0.0, 20.0], [0.0, 40.0, 20.0]],
            [[foot, 0.0, -20.0], [80.0, 0.0, -20.0], [80.0, 0.0, 20.0]],
            [[foot, 0.0, -20.0], [80.0, 0.0, 20.0], [foot, 0.0, 20.0]],
        ])
        .unwrap();
        let physics = VerticalPhysics {
            gravity: 32.0,
            terminal_speed: 40.0,
            jump_speed: 10.0,
            air_factor: 1.0,
            ground_tolerance: crate::movement::MAX_GROUNDED_STEP,
            climb: crate::movement::MAX_GROUNDED_STEP,
            steers_in_air: true,
        };
        let mut state = AirborneController::default();
        let mut feet = Vec3::new(-1.0, 40.0, 0.0);
        for _ in 0..200 {
            let before = feet;
            feet = state.step(
                &world,
                feet,
                physics,
                MotionStep {
                    horizontal: Vec3::X * 0.5,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            assert!(feet.x > before.x, "refused at {before:?}");
            if let Some(landing) = state.take_landing() {
                assert!(landing.fall_distance < 2.0, "{landing:?} at {feet:?}");
                assert_eq!(super::super::fall_damage(landing.fall_distance, 0), 0);
            }
            // On the face each step: never under it, and never higher over
            // it than the footprint's edge reaches up it.
            let face = (40.0 - feet.x.clamp(0.0, foot) * 55.0_f32.to_radians().tan()).max(0.0);
            assert!(
                feet.y >= face - 0.001 && feet.y <= face + 0.6,
                "{feet:?} over the face at {face}"
            );
            if feet.x > foot + 5.0 {
                break;
            }
        }
        assert!(feet.x > foot + 5.0 && feet.y.abs() < 0.001, "{feet:?}");
    }

    /// The Titanium program's falls (sittings 8g-8i; R-s): gravity 0.4 a
    /// 50 ms step, the air's 0.99, and the on-ground tolerance 0.5 + 0.001;
    /// a size 6 human's climb, 3.75 − 0.6 a step (6.344).
    const PROGRAM: VerticalPhysics = VerticalPhysics {
        gravity: 160.0,
        terminal_speed: 792.0,
        jump_speed: 44.55,
        air_factor: 0.99,
        ground_tolerance: 0.501,
        climb: 3.15,
        steers_in_air: false,
    };

    /// A plateau at y `height` for x < 0, a face at `degrees` down to the
    /// floor at y 0, and the floor beyond; the face's foot's x.
    fn face(height: f32, degrees: f32) -> (CollisionWorld, f32) {
        let foot = height / degrees.to_radians().tan();
        let world = CollisionWorld::new([
            [
                [-20.0, height, -20.0],
                [0.0, height, -20.0],
                [0.0, height, 20.0],
            ],
            [
                [-20.0, height, -20.0],
                [0.0, height, 20.0],
                [-20.0, height, 20.0],
            ],
            [[0.0, height, -20.0], [foot, 0.0, -20.0], [foot, 0.0, 20.0]],
            [[0.0, height, -20.0], [foot, 0.0, 20.0], [0.0, height, 20.0]],
            [
                [foot, 0.0, -20.0],
                [foot + 80.0, 0.0, -20.0],
                [foot + 80.0, 0.0, 20.0],
            ],
            [
                [foot, 0.0, -20.0],
                [foot + 80.0, 0.0, 20.0],
                [foot, 0.0, 20.0],
            ],
        ])
        .unwrap();
        (world, foot)
    }

    /// Runs at 43 units a second off the plateau for `slices` 50 ms
    /// slices: where the feet end, each landing, and whether every slice
    /// kept close to the run's pace.
    fn run(world: &CollisionWorld, slices: usize) -> (Vec3, Vec<Landing>, bool) {
        let mut state = AirborneController::default();
        let top = world
            .ground(Vec3::new(-1.0, 100.0, 0.0), 0.0, 200.0)
            .unwrap();
        let mut feet = Vec3::new(-1.0, top, 0.0);
        let (mut landings, mut full) = (Vec::new(), true);
        for _ in 0..slices {
            let before = feet;
            feet = state.step(
                world,
                feet,
                PROGRAM,
                MotionStep {
                    horizontal: Vec3::X * 43.0 * 0.05,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            // In the air the take-off speed carries on, less the air's 1% a
            // step, so a hop slows a little; it never stalls.
            full &= feet.x - before.x > 2.15 * 0.85 && feet.x - before.x < 2.15 + 0.01;
            landings.extend(state.take_landing());
        }
        (feet, landings, full)
    }

    #[test]
    fn a_run_down_a_steep_face_keeps_its_pace_and_lands_on_the_face() {
        // A walking step keeps the feet on the floor only within the
        // programs' on-ground tolerance; past it they fall under gravity and
        // land on the face, as in both programs (R-s). A gentle face keeps
        // them on the ground; a steep one goes by falls that each land on
        // it; either way at the run's pace.
        let (gentle, gentle_foot) = face(40.0, 10.0);
        let (feet, landings, full) = run(&gentle, 40);
        assert!(full && feet.x < gentle_foot, "{feet:?}");
        assert!(landings.is_empty(), "{landings:?}");
        let (steep, foot) = face(60.0, 55.0);
        let (feet, landings, full) = run(&steep, 30);
        assert!(full, "slowed: {feet:?}");
        assert!(feet.x > foot && feet.y.abs() < 0.001, "{feet:?}");
        // Falls that land on the face, then at its foot, none of them
        // lethal by the programs' rule: under 9 + 1.5 + 2.5 units a step at
        // touchdown (sitting 6, T2).
        assert!(landings.len() >= 2, "{landings:?}");
        assert!(
            landings
                .iter()
                .all(|landing| landing.impact_speed * 0.05 < 13.0),
            "{landings:?}"
        );
    }

    #[test]
    fn a_walk_off_a_cliff_still_falls_to_the_floor_below() {
        // A face at 89.9 degrees is a cliff: the feet leave it at once and
        // land at its foot.
        let (cliff, foot) = face(30.0, 89.9);
        let (feet, landings, full) = run(&cliff, 30);
        assert!(full && feet.x > foot && feet.y.abs() < 0.001, "{feet:?}");
        assert_eq!(landings.len(), 1, "{landings:?}");
        assert!(
            (landings[0].fall_distance - 30.0).abs() < 0.5,
            "{landings:?}"
        );
    }

    #[test]
    fn feet_beside_a_step_within_the_reach_of_its_top_stand_on_it_and_past_it_fall() {
        // Floor at y 0, and a step 1.5 high for x >= 0. Feet beside it, below
        // its top by less than the body's reach (2 for these physics), whose
        // footprint takes in its top, stand on it at once, as the programs'
        // floor sweep from the origin meets it, rather than hang on its edge.
        let world = CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
            [[0.0, 1.5, -20.0], [20.0, 1.5, -20.0], [20.0, 1.5, 20.0]],
            [[0.0, 1.5, -20.0], [20.0, 1.5, 20.0], [0.0, 1.5, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 1.5, -20.0], [0.0, 1.5, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, 1.5, 20.0], [0.0, 0.0, 20.0]],
        ])
        .unwrap();
        for (x, floor) in [(-0.1, 1.5), (-0.24, 1.5), (-0.35, 1.5), (-0.45, 0.0)] {
            let mut state = AirborneController::default();
            let mut position = Vec3::new(x, 1.0, 0.0);
            for _ in 0..100 {
                position = tick(&mut state, &world, position, false);
            }
            assert!(
                (position.y - floor).abs() < 0.001 && (position.x - x).abs() < 0.001,
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
        // At the read step's speed, which the continuous fall's
        // sqrt(2gh) is above.
        let read = -read_fall(10.0, TICK) / STEP;
        assert!(
            (landing.impact_speed - read).abs() < 0.001,
            "{landing:?} against {read}"
        );
        assert!(landing.impact_speed < (2.0_f32 * 32.0 * 10.0).sqrt());
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
        // Stepped, 10 a second less 1.6 each 50 ms step: 0.42 + 0.34 + 0.26
        // + 0.18 + 0.10 + 0.02 units, under the continuous v²/2g of 1.5625.
        assert!((max_height - 1.32).abs() < 0.01, "{max_height}");
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
        // Within the body's reach (2 for these physics) the floor is stood
        // on too, as the programs' floor sweep from the origin finds it.
        let mut state = AirborneController::default();
        let stood = tick(&mut state, &world, Vec3::new(0.0, -1.9, 0.0), false);
        assert!(stood.y.abs() < 0.001, "{stood:?}");
        // Deeper than it is inside the ground, not standing on it.
        let mut state = AirborneController::default();
        let deep = Vec3::new(0.0, -2.1, 0.0);
        assert!(tick(&mut state, &world, deep, false).y <= deep.y);
        // Under the reach's floor, a ceiling above the capsule does not keep
        // it down: the sweep starts under the ceiling. Past the reach, the
        // capsule's lift still asks for head room.
        let covered = CollisionWorld::new(floor(0.0).into_iter().chain(floor(6.7))).unwrap();
        let mut state = AirborneController::default();
        let stood = tick(&mut state, &covered, Vec3::new(0.0, -0.3, 0.0), false);
        assert!(stood.y.abs() < 0.001, "{stood:?}");
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
    fn the_air_slows_each_step_by_its_factor() {
        let world = CollisionWorld::new(floor(-1000.0)).unwrap();
        let step = |air_factor| {
            let mut state = AirborneController::default();
            let physics = VerticalPhysics {
                air_factor,
                ..PROVISIONAL_PHYSICS
            };
            let input = MotionStep {
                horizontal: Vec3::ZERO,
                jump: false,
                seconds: 0.05,
                height: 6.0,
            };
            let feet = state.step(&world, Vec3::ZERO, physics, input);
            state.step(&world, feet, physics, input);
            state.velocity()
        };
        // Two steps of 1.6 units a second each, all of it kept or 0.99 of
        // it after each step.
        assert!((step(1.0) + 3.2).abs() < 1e-4, "{}", step(1.0));
        let slowed = (-1.6 * 0.99 - 1.6) * 0.99;
        assert!((step(0.99) - slowed).abs() < 1e-4, "{}", step(0.99));
        // A factor that would add speed or stop it dead is refused.
        assert!(step(1.5).abs() < f32::EPSILON);
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
            air_factor: 1.0,
            ground_tolerance: crate::movement::MAX_GROUNDED_STEP,
            climb: crate::movement::MAX_GROUNDED_STEP,
            steers_in_air: true,
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
        let stall = MotionStep {
            seconds: 5.0,
            ..input
        };
        // The step that walks off was on the ground where it started, so
        // it takes no gravity (the programs' on-ground test); the next falls.
        let off = normal.step(&world, start, physics, input);
        assert!(off.x > 20.4 && off.y.abs() < f32::EPSILON, "{off:?}");
        let next = normal.step(&world, off, physics, input);
        let late = stalled.step(&world, start, physics, stall);
        let late = stalled.step(&world, late, physics, stall);
        assert!(next.x > 20.4 && next.y < 0.0, "{next:?}");
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

    /// The read step by hand (R-s): from standing at the top of a drop of
    /// `height` onto a flat floor, a step whose feet, moved by its speed,
    /// end within 0.5 + 0.001 of the floor is on the ground and takes no
    /// gravity; any other takes 0.4 off the vertical speed a step and keeps
    /// 0.99 of it; then the feet move by it, and the step that ends below
    /// the floor lands, at that speed.
    fn read_drop(height: f32) -> f32 {
        read_fall(height, PROGRAM)
    }

    /// The same by `physics`, in units a step.
    fn read_fall(height: f32, physics: VerticalPhysics) -> f32 {
        let gravity = physics.gravity * STEP * STEP;
        let (mut z, mut vz) = (0.0_f32, 0.0_f32);
        loop {
            if z + vz <= -height + physics.ground_tolerance {
                return vz;
            }
            vz = (vz - gravity) * physics.air_factor;
            z += vz;
            if z < -height {
                return vz;
            }
        }
    }

    #[test]
    fn a_drop_lands_at_the_read_steps_speed_and_costs_what_it_does() {
        for height in [50.0_f32, 100.0] {
            let world = CollisionWorld::new([
                [[-20.0, 0.0, -20.0], [0.0, 0.0, -20.0], [0.0, 0.0, 20.0]],
                [[-20.0, 0.0, -20.0], [0.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
                [
                    [0.0, -height, -20.0],
                    [80.0, -height, -20.0],
                    [80.0, -height, 20.0],
                ],
                [
                    [0.0, -height, -20.0],
                    [80.0, -height, 20.0],
                    [0.0, -height, 20.0],
                ],
            ])
            .unwrap();
            let mut state = AirborneController::default();
            let mut feet = Vec3::new(-1.0, 0.0, 0.0);
            let mut landing = None;
            for _ in 0..200 {
                feet = state.step(
                    &world,
                    feet,
                    PROGRAM,
                    MotionStep {
                        horizontal: Vec3::X * 43.0 * 0.05,
                        jump: false,
                        seconds: 0.05,
                        height: 6.0,
                    },
                );
                if let Some(found) = state.take_landing() {
                    landing = Some(found);
                    break;
                }
            }
            let landing = landing.expect("landed");
            let ours = landing.impact_speed * STEP;
            let read = -read_drop(height);
            assert!(
                (ours - read).abs() < 0.001,
                "{height}: {ours} against {read}"
            );
            assert!((feet.y + height).abs() < 0.001, "{feet:?}");
        }
    }

    #[test]
    fn a_call_shorter_than_a_step_moves_only_across_and_the_step_comes_once_due() {
        let world = CollisionWorld::new(floor(-100.0)).unwrap();
        let mut state = AirborneController::default();
        let input = |seconds| MotionStep {
            horizontal: Vec3::ZERO,
            jump: false,
            seconds,
            height: 6.0,
        };
        // In the air at rest: 30 ms is under a step, so nothing falls yet.
        let feet = state.step(&world, Vec3::ZERO, PROGRAM, input(0.03));
        assert!(
            feet.y.abs() < f32::EPSILON && state.velocity() == 0.0,
            "{feet:?}"
        );
        // 30 ms more completes the step: one step's fall, 0.396 units.
        let feet = state.step(&world, feet, PROGRAM, input(0.03));
        assert!((feet.y + 0.396).abs() < 0.001, "{feet:?}");
        assert!((state.velocity() * STEP + 0.396).abs() < 0.001);
    }

    #[test]
    fn in_the_air_the_take_off_speed_carries_on_and_input_no_longer_steers() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        let mut state = AirborneController::default();
        let mut feet = Vec3::new(-15.0, 0.0, 0.0);
        let mut steps = 0_u8;
        let mut air = Vec::new();
        loop {
            steps += 1;
            // The run is along +x; after the take-off the keys turn to +z.
            let direction = if steps == 1 { Vec3::X } else { Vec3::Z };
            let before = feet;
            feet = state.step(
                &world,
                feet,
                PROGRAM,
                MotionStep {
                    horizontal: direction * 43.0 * 0.05,
                    jump: steps == 1,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            if state.velocity() == 0.0 {
                break;
            }
            if steps > 1 {
                air.push(feet - before);
            }
            assert!(steps < 100);
        }
        // Each step in the air keeps 0.99 of the last one's speed across,
        // all of it along +x, and nothing moves it along +z.
        assert!(air.len() > 5, "{air:?}");
        let mut last = 2.15;
        for moved in &air {
            assert!(moved.z.abs() < f32::EPSILON, "{air:?}");
            assert!((moved.x - last * 0.99).abs() < 0.001, "{air:?}");
            last = moved.x;
        }
    }

    #[test]
    fn feet_put_under_a_face_within_the_bodys_reach_stand_on_it_and_climb_it() {
        // Live's look (11-01): a goto left the feet 2.9 under a 30° hillside,
        // and the run held there moved nothing. The program's floor sweep
        // starts at the origin, the stand height above the feet, so a floor
        // up to the body's reach above them is stood on at once (6.344).
        let height = 100.0_f32;
        let run = MotionStep {
            horizontal: -Vec3::X * 2.15,
            jump: false,
            seconds: 0.05,
            height: 6.0,
        };
        for (degrees, under) in [
            (30.0_f32, 2.9_f32),
            (40.0, 2.9),
            (50.0, 2.9),
            (50.0, 3.1),
            // A goto a unit above the face falls onto it and climbs on.
            (30.0, -1.0),
            (50.0, -1.0),
        ] {
            let (world, foot) = face(height, degrees);
            let slope = degrees.to_radians().tan();
            let surface = |x: f32| height - x.clamp(0.0, foot) * slope;
            let start = foot * 0.7;
            let mut state = AirborneController::default();
            let mut feet = Vec3::new(start, surface(start) - under, 0.0);
            let mut landings = Vec::new();
            for _ in 0..30 {
                feet = state.step(&world, feet, PROGRAM, run);
                landings.extend(state.take_landing());
            }
            let case = format!("{degrees}° from {under} under: {feet}");
            assert!(start - feet.x > 60.0, "{case}");
            assert!(feet.y >= surface(feet.x) - 0.01, "{case}");
            assert!(
                feet.y <= surface(feet.x - crate::movement::SUPPORT_SPREAD) + 0.6,
                "{case}"
            );
            assert!(
                landings
                    .iter()
                    .all(|landing| landing.impact_speed * STEP <= 2.5),
                "{case}: {landings:?}"
            );
        }
        // Deeper than the reach, the sweep starts under the face and meets
        // nothing: the feet are not put on it.
        let (world, foot) = face(height, 30.0);
        let start = foot * 0.7;
        let surface = height - start * 30.0_f32.to_radians().tan();
        let mut state = AirborneController::default();
        let mut feet = Vec3::new(start, surface - 3.5, 0.0);
        for _ in 0..10 {
            feet = state.step(&world, feet, PROGRAM, run);
        }
        assert!(feet.y <= surface - 3.5 + 0.001, "{feet}");
    }

    #[test]
    fn a_step_that_stands_up_under_a_steep_face_climbs_only_by_the_reach_left() {
        // Live's look (11-01, 85b9d33d): a goto 1 u over a 50° face left a
        // human's feet 2.75 under it, and every run and walk moved nothing.
        // The stand-up and the walk each took the whole 3.15, so the step
        // rose past the reach and the player's step threw it all away, the
        // stand-up too, every time. One step's rise from where it started
        // stays within the climb (6.344): the walk after a stand-up gets
        // what the lift left (6.366), and the next steps climb at pace.
        let height = 200.0_f32;
        for degrees in [40.0_f32, 50.0, 55.0, 60.0] {
            for pace in [1.6_f32, 0.64] {
                let (world, foot) = face(height, degrees);
                let slope = degrees.to_radians().tan();
                let surface = |x: f32| height - x.clamp(0.0, foot) * slope;
                let start = foot * 0.7;
                let mut feet = Vec3::new(start, surface(start) + 1.0 - 3.75, 0.0);
                let mut state = AirborneController::default();
                let step = MotionStep {
                    horizontal: -Vec3::X * pace,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                };
                let mut moved = Vec::new();
                for _ in 0..20 {
                    let next = state.step(&world, feet, PROGRAM, step);
                    let case = format!("{degrees}° at {pace}: {feet} to {next}");
                    assert!(next.y - feet.y <= PROGRAM.climb + 0.001, "{case}");
                    // On the face under the footprint, never under it.
                    let under = surface(next.x + crate::movement::SUPPORT_SPREAD);
                    assert!(next.y >= under - 0.01, "{case}");
                    moved.push(feet.x - next.x);
                    feet = next;
                }
                let case = format!("{degrees}° at {pace}: {moved:?}");
                // Every step moves, the first by what the stand-up left,
                // and from the third at pace where the reach allows it.
                assert!(moved.iter().all(|x| *x > 0.1), "{case}");
                if pace * slope <= PROGRAM.climb {
                    assert!(
                        moved[2..].iter().all(|x| (x - pace).abs() < 0.001),
                        "{case}"
                    );
                }
                assert!(
                    state
                        .take_landing()
                        .is_none_or(|landing| landing.impact_speed * STEP <= 2.5),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn a_run_down_a_55_degree_face_lands_each_hop_at_the_read_steps_speed() {
        // The read step by hand on the floor this client stands on: the face
        // under the footprint's uphill edge (SUPPORT_SPREAD across). A step on
        // the ground (its feet, moved by its speed, within 0.501 of the floor
        // where they stand) takes no gravity and moves across at the run's
        // 2.15 a step; one in the air takes 0.4 off the vertical speed and
        // keeps 0.99 of it and of the speed across; the step that ends below
        // the floor lands at its speed.
        let (height, degrees) = (200.0_f32, 55.0_f32);
        let (world, foot) = face(height, degrees);
        let slope = degrees.to_radians().tan();
        let floor = |x: f32| {
            let under = |x: f32| (height - x.clamp(0.0, foot) * slope).max(0.0);
            under(x - crate::movement::SUPPORT_SPREAD)
        };
        let (mut x, mut z, mut vx, mut vz) = (-1.0_f32, height, 2.15_f32, 0.0_f32);
        let mut read = Vec::new();
        while x < foot - 5.0 {
            if z + vz <= floor(x) + 0.501 {
                vx = 2.15;
            } else {
                vz = (vz - 0.4) * 0.99;
                vx *= 0.99;
            }
            x += vx;
            z += vz;
            if z < floor(x) {
                read.push(-vz);
                (z, vz) = (floor(x), 0.0);
            }
        }
        let mut state = AirborneController::default();
        let mut feet = Vec3::new(-1.0, height, 0.0);
        let mut ours = Vec::new();
        while feet.x < foot - 5.0 {
            feet = state.step(
                &world,
                feet,
                PROGRAM,
                MotionStep {
                    horizontal: Vec3::X * 2.15,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            ours.extend(
                state
                    .take_landing()
                    .map(|landing| landing.impact_speed * STEP),
            );
        }
        // Each hop that passes the 2.5 a step gate, hop for hop.
        let hurt = |speeds: &[f32]| {
            speeds
                .iter()
                .copied()
                .filter(|v| *v > 2.5)
                .collect::<Vec<_>>()
        };
        let (read, ours) = (hurt(&read), hurt(&ours));
        assert!(read.len() >= 3, "{read:?}");
        assert_eq!(read.len(), ours.len(), "{read:?} against {ours:?}");
        for (read, ours) in read.iter().zip(&ours) {
            assert!((read - ours).abs() < 0.01, "{read} against {ours}");
        }
    }
}

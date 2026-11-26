//! Clear surface motion on the existing water clock. No movement grant.
use super::{
    CollisionWorld, SurfaceRegion,
    water::{WetCoefficients, WetFrameMedium, WetInput, WetVelocity},
};
use glam::Vec3;
use std::sync::Arc;

/// Why an entire surface trial keeps today's ordinary path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceDecline {
    /// No actual frame phase, or no known initial frame boundary.
    Phase,
    /// Unknown/mutable regions, zone line or another water kind.
    Region,
    /// Collision snapshot, body or calibration changed.
    Context,
    /// Input/time is outside the calibrated ordinary-stroke scope.
    Input,
    /// A physical piece touched support/solid or could not be established.
    Contact,
    /// The original owner shortened or refused a tiny piece.
    Shortened,
    /// A numerical tail has no adjacent same-frame predecessor.
    Tail,
    /// More pieces than a bounded ordinary sample can contain.
    Bound,
}

/// Native ordinary stroke selected by the renderer's eligibility checks.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceStroke {
    /// Existing calibrated wet factors.
    pub set: WetCoefficients,
    /// Existing calibrated stroke input, in world axes.
    pub input: WetInput,
    /// Unchanged sample interval, at most 250 ms.
    pub seconds: f32,
    /// Original standing collision body height.
    pub height: f32,
}

/// A committed clock and its immutable collision context.
#[derive(Clone)]
pub struct SurfaceClock {
    velocity: WetVelocity,
    medium: SurfaceRegion,
    water: SurfaceRegion,
    feet: Vec3,
    world: CollisionWorld,
    height: f32,
    set: WetCoefficients,
    crossed: bool,
    updates: u64,
    dry_peak: Option<f32>,
    water_region: Option<usize>,
}

/// A disposable trial. Commit only after the exact local result accepts it.
pub struct SurfacePlan {
    /// Legal endpoint returned by the original collision owner.
    pub feet: Vec3,
    /// Clock to stage with that endpoint.
    pub clock: SurfaceClock,
    /// Any actual interval time covered by a wet frame.
    pub wet: bool,
    /// Whether the continuation has crossed a surface, including earlier calls.
    pub active: bool,
    /// Number of original-owner physical moves.
    pub pieces: u32,
    /// Numeric tails joined without a new update or extra time.
    pub tails: u32,
    /// Genuine owner outcomes observed by this trial.
    pub checks: u32,
    /// Sum of the original owner's physical 3D moves, including joined tails.
    pub path: f64,
    /// Actual ordinary partial-frame time before a new wet boundary.
    /// Zero for an existing epoch or an already-known boundary.
    pub boundary_delay: f32,
}

impl SurfaceClock {
    /// The unmodified one-owner velocity/phase snapshot.
    #[must_use]
    pub const fn velocity(&self) -> WetVelocity {
        self.velocity
    }
    /// Medium latched for the current frame, not a request-boundary refresh.
    #[must_use]
    pub const fn medium(&self) -> SurfaceRegion {
        self.medium
    }
    /// Whether this clock has entered the surface continuation.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.crossed
    }
    /// Full velocity updates since this known tracking epoch began.
    #[must_use]
    pub const fn updates(&self) -> u64 {
        self.updates
    }
    /// Highest original-owner feet position in the current dry span.
    #[must_use]
    pub const fn dry_peak(&self) -> Option<f32> {
        self.dry_peak
    }
    /// Retains the actual ordinary wet API snapshot during passive all-wet
    /// tracking. No phase is reconstructed: outer legacy slice arithmetic
    /// may differ in phase rounding while its actual velocity agrees.
    pub fn ordinary_velocity(&mut self, velocity: WetVelocity) -> bool {
        let bits = |v: WetVelocity| {
            let (h, y) = v.units_per_second();
            [h[0].to_bits(), h[1].to_bits(), y.to_bits()]
        };
        if self.crossed
            || self.medium != self.water
            || velocity.frame_phase().is_none()
            || bits(velocity) != bits(self.velocity)
        {
            return false;
        }
        self.velocity = velocity;
        true
    }
    /// Rebase the accepted origin/feet conversion only after the exact
    /// requested world position was locally accepted. This adds no travel.
    pub fn accepted_feet(&mut self, feet: Vec3) -> bool {
        if !feet.is_finite() {
            return false;
        }
        if self.velocity.frame_phase().is_some_and(|p| p.at_boundary)
            && self
                .world
                .liquids
                .as_ref()
                .and_then(|l| l.surface_region(feet + self.world.swim_probe(self.height)))
                != Some(self.medium)
        {
            return false;
        }
        self.feet = feet;
        if self.medium == SurfaceRegion::Air {
            self.dry_peak = Some(self.dry_peak.unwrap_or(feet.y).max(feet.y));
        }
        true
    }

    fn begin(
        prior: Option<&Self>,
        world: &CollisionWorld,
        feet: Vec3,
        ordinary: WetVelocity,
        stroke: SurfaceStroke,
    ) -> Result<Self, SurfaceDecline> {
        if !stroke.seconds.is_finite()
            || stroke.seconds <= 0.
            || stroke.seconds > 0.25
            || !stroke.height.is_finite()
            || stroke.height < 1.
            || !feet.is_finite()
            || stroke.input.jump
            || !stroke.input.vertical.is_finite()
            || !stroke.input.horizontal.iter().all(|v| v.is_finite())
            || stroke.input.horizontal.iter().all(|&v| v == 0.)
        {
            return Err(SurfaceDecline::Input);
        }
        let liquids = world.liquids.as_ref().ok_or(SurfaceDecline::Region)?;
        let region = |feet: Vec3| liquids.surface_region(feet + world.swim_probe(stroke.height));
        let clock = if let Some(old) = prior {
            if !old.world.same_surface_context(world)
                || old.feet != feet
                || old.height.to_bits() != stroke.height.to_bits()
                || old.set != stroke.set
            {
                return Err(SurfaceDecline::Context);
            }
            old.clone()
        } else {
            if ordinary.frame_phase().is_none() {
                return Err(SurfaceDecline::Phase);
            }
            let water = region(feet)
                .filter(|&r| r != SurfaceRegion::Air)
                .ok_or(SurfaceDecline::Region)?;
            Self {
                velocity: ordinary,
                medium: water,
                water,
                feet,
                world: world.clone(),
                height: stroke.height,
                set: stroke.set,
                crossed: false,
                updates: 0,
                dry_peak: None,
                water_region: liquids
                    .surface_water_identity(feet + world.swim_probe(stroke.height)),
            }
        };
        Ok(clock)
    }

    /// Plans a clone, with one original-owner query for every physical move.
    /// A first trial starts at an actual known wet frame boundary. An
    /// ordinary partial may reach it without an update, provided its whole
    /// original-owner path stays in the same known water. The API's actual
    /// phase is preserved; no medium or phase is inferred across a crossing.
    /// All-wet trials may passively retain provenance while the renderer
    /// keeps today's ordinary wet commands bit-for-bit.
    ///
    /// # Errors
    /// Any uncertainty declines the whole interval; the input is untouched.
    #[allow(
        clippy::too_many_lines,
        reason = "one disposable trial orders frame, collision and medium events"
    )]
    pub fn plan(
        prior: Option<&Self>,
        world: &CollisionWorld,
        feet: Vec3,
        ordinary: WetVelocity,
        stroke: SurfaceStroke,
    ) -> Result<SurfacePlan, SurfaceDecline> {
        let liquids = world.liquids.as_ref().ok_or(SurfaceDecline::Region)?;
        let region = |feet: Vec3| liquids.surface_region(feet + world.swim_probe(stroke.height));
        let mut clock = Self::begin(prior, world, feet, ordinary, stroke)?;
        let mut awaiting_boundary = prior.is_none()
            && !ordinary
                .frame_phase()
                .ok_or(SurfaceDecline::Phase)?
                .at_boundary;
        let mut boundary_delay = 0.;
        let (observed, token) = world.observe_motion();
        let (mut left, mut pieces, mut tails, mut wet) = (stroke.seconds, 0_u32, 0_u32, false);
        let mut path = 0_f64;
        // 250ms has at most sixteen physical portions plus a numeric tail.
        for _ in 0..17 {
            if left <= 0. {
                break;
            }
            let phase = clock.velocity.frame_phase().ok_or(SurfaceDecline::Phase)?;
            let medium = api_medium(clock.medium, clock.water, stroke.set)?;
            let mut trial = clock.velocity;
            let raw = trial
                .advance_piece(medium, stroke.input, left)
                .ok_or(SurfaceDecline::Phase)?;
            if raw.starts_frame {
                clock.updates = clock.updates.checked_add(1).ok_or(SurfaceDecline::Bound)?;
            }
            if phase.at_boundary && !raw.starts_frame && !raw.completes_frame {
                return Err(SurfaceDecline::Tail); // a request consisting only of a tail
            }
            let mut delta = Vec3::new(raw.horizontal[0], raw.vertical, raw.horizontal[1]);
            let completion = clock.feet + delta;
            let mut joined = false;
            left -= raw.seconds;
            if raw.completes_frame && left > 0. {
                let next_phase = trial.frame_phase().ok_or(SurfaceDecline::Phase)?;
                let mut peek = trial;
                let tail = peek
                    .advance_piece(medium, stroke.input, left)
                    .ok_or(SurfaceDecline::Phase)?;
                if next_phase.at_boundary && !tail.starts_frame && !tail.completes_frame {
                    // These actual API facts imply its own boundary window.
                    // Flags alone are insufficient: an interior physical
                    // partial can have the same pair. No client time bound.
                    if trial.units_per_second() != peek.units_per_second() {
                        return Err(SurfaceDecline::Tail);
                    }
                    let extra = Vec3::new(tail.horizontal[0], tail.vertical, tail.horizontal[1]);
                    delta += extra;
                    left -= tail.seconds;
                    trial = peek;
                    tails += 1;
                    joined = true;
                }
            }
            let end = clock.checked_move(&observed, delta, stroke.height)?;
            if awaiting_boundary {
                boundary_delay =
                    clock.retake_boundary(world, stroke.height, completion, end, raw)?;
                awaiting_boundary = false;
            }
            if joined && (region(completion).is_none() || region(completion) != region(end)) {
                return Err(SurfaceDecline::Region);
            }
            if !token.report().clear() {
                return Err(SurfaceDecline::Contact);
            }
            if clock.medium == SurfaceRegion::Air || region(end) == Some(SurfaceRegion::Air) {
                clock.water_below(&observed, clock.feet, stroke.height)?;
                clock.water_below(&observed, completion, stroke.height)?;
                if end != completion {
                    clock.water_below(&observed, end, stroke.height)?;
                }
            }
            clock.record_dry_move(completion, end);
            wet |= clock.medium != SurfaceRegion::Air;
            path += (end - clock.feet).as_dvec3().length();
            clock.feet = end;
            clock.velocity = trial;
            if raw.completes_frame {
                // Strict B: refresh at the checked frame-completion point,
                // before its adjacent no-update tail, not at request end.
                let next = region(completion).ok_or(SurfaceDecline::Region)?;
                if next != SurfaceRegion::Air && next != clock.water {
                    return Err(SurfaceDecline::Region);
                }
                clock.crossed |= clock.medium != next;
                if next == SurfaceRegion::Air && clock.medium != SurfaceRegion::Air {
                    clock.dry_peak = Some(completion.y.max(end.y));
                } else if next != SurfaceRegion::Air {
                    clock.dry_peak = None;
                }
                clock.medium = next;
                if next != SurfaceRegion::Air {
                    clock.water_region = liquids
                        .surface_water_identity(completion + world.swim_probe(stroke.height));
                }
            }
            pieces += 1;
        }
        if left > 0. {
            return Err(SurfaceDecline::Bound);
        }
        Ok(SurfacePlan {
            feet: clock.feet,
            wet,
            active: clock.crossed,
            pieces,
            tails,
            checks: token.report().checks,
            path,
            boundary_delay,
            clock,
        })
    }

    fn retake_boundary(
        &self,
        world: &CollisionWorld,
        height: f32,
        completion: Vec3,
        end: Vec3,
        raw: super::water::WetFramePiece,
    ) -> Result<f32, SurfaceDecline> {
        if raw.starts_frame || !raw.completes_frame {
            return Err(SurfaceDecline::Phase);
        }
        let liquids = world.liquids.as_ref().ok_or(SurfaceDecline::Region)?;
        let offset = world.swim_probe(height);
        if liquids.surface_region(completion + offset) != Some(self.water)
            || liquids.surface_region(end + offset) != Some(self.water)
            || liquids
                .first_outside(self.feet + offset, end + offset, super::Liquid::Water)
                .is_some()
            || self.water_region.is_none()
            || liquids.surface_water_identity(end + offset) != self.water_region
        {
            return Err(SurfaceDecline::Region);
        }
        Ok(raw.seconds)
    }

    fn record_dry_move(&mut self, completion: Vec3, end: Vec3) {
        self.dry_peak = if self.medium == SurfaceRegion::Air {
            Some(
                self.dry_peak
                    .unwrap_or(self.feet.y)
                    .max(self.feet.y)
                    .max(end.y)
                    .max(completion.y),
            )
        } else {
            None
        };
    }

    fn checked_move(
        &self,
        world: &CollisionWorld,
        delta: Vec3,
        height: f32,
    ) -> Result<Vec3, SurfaceDecline> {
        let liquids = world.liquids.as_ref().ok_or(SurfaceDecline::Region)?;
        let proposed = self.feet + delta;
        for offset in [
            world.swim_probe(height),
            Vec3::Y * super::liquids::SOLES,
            Vec3::Y * height,
        ] {
            if !liquids.surface_segment(self.feet + offset, proposed + offset, self.water) {
                return Err(SurfaceDecline::Region);
            }
        }
        if world.swim_clearance(self.feet, height).is_some() || world.support(self.feet).is_some() {
            return Err(SurfaceDecline::Contact);
        }
        let end = world.fly_step(self.feet, delta, height);
        if end == self.feet || end != proposed {
            return Err(SurfaceDecline::Shortened);
        }
        if world.swim_clearance(end, height).is_some() || world.support(end).is_some() {
            return Err(SurfaceDecline::Contact);
        }
        Ok(end)
    }

    fn water_below(
        &self,
        world: &CollisionWorld,
        feet: Vec3,
        height: f32,
    ) -> Result<(), SurfaceDecline> {
        let liquids = world.liquids.as_ref().ok_or(SurfaceDecline::Region)?;
        let origin = feet + world.swim_probe(height);
        let water_region = self.water_region.ok_or(SurfaceDecline::Region)?;
        match liquids.surface_region(origin) {
            Some(region)
                if region == self.water
                    && liquids.surface_water_identity(origin) == Some(water_region) =>
            {
                return Ok(());
            }
            Some(SurfaceRegion::Air) => (),
            _ => return Err(SurfaceDecline::Region),
        }
        // Original terrain domain bounds the query, not an invented water
        // height or a maximum distance from the body to the water.
        let bottom = world.terrain.mesh.local_aabb().mins.y;
        let water = liquids
            .surface_water_below(origin, bottom, self.water, water_region)
            .ok_or(SurfaceDecline::Region)?;
        let distance = origin.y - water;
        if !distance.is_finite() || distance <= 0. {
            return Err(SurfaceDecline::Region);
        }
        let obstruction = world.ray_distance(origin, Vec3::NEG_Y, distance);
        world.observe_sweep(&Ok::<_, parry3d::query::Unsupported>(obstruction));
        if obstruction.is_some() {
            return Err(SurfaceDecline::Contact);
        }
        Ok(())
    }
}

fn api_medium(
    current: SurfaceRegion,
    water: SurfaceRegion,
    set: WetCoefficients,
) -> Result<WetFrameMedium, SurfaceDecline> {
    let value = if current == SurfaceRegion::Air {
        WetFrameMedium::Air
    } else if current == water {
        WetFrameMedium::Water(set)
    } else {
        return Err(SurfaceDecline::Region);
    };
    match value {
        WetFrameMedium::Air | WetFrameMedium::Water(_) => Ok(value),
        _ => Err(SurfaceDecline::Region), // non-exhaustive: never assume a future medium
    }
}

impl CollisionWorld {
    fn same_surface_context(&self, other: &Self) -> bool {
        let same_liquids = match (&self.liquids, &other.liquids) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        Arc::ptr_eq(&self.terrain, &other.terrain)
            && (Arc::ptr_eq(&self.obstacles, &other.obstacles)
                || self.obstacles.len() == other.obstacles.len()
                    && self.obstacles.iter().zip(other.obstacles.iter()).all(
                        |((id, a, rule), (other_id, b, other_rule))| {
                            id == other_id && rule == other_rule && Arc::ptr_eq(a, b)
                        },
                    ))
            && (Arc::ptr_eq(&self.spawn_obstacles, &other.spawn_obstacles)
                || self.spawn_obstacles.len() == other.spawn_obstacles.len()
                    && self
                        .spawn_obstacles
                        .iter()
                        .zip(other.spawn_obstacles.iter())
                        .all(|((id, a), (other_id, b))| id == other_id && a.same_placement(b)))
            && same_liquids
            && self.swim_origin == other.swim_origin
            && self.collision_height == other.collision_height
            && self.liquid_support == other.liquid_support
            && self.titanium_walking_origin == other.titanium_walking_origin
    }
}

#[cfg(test)]
mod tests {
    use super::super::water::LIQUID_5;
    use super::*;
    use crate::movement::{Liquid, Liquids};
    use std::ops::Range;

    struct Pool;
    impl Liquids for Pool {
        fn surface_water_identity(&self, point: Vec3) -> Option<usize> {
            (self.liquid_at(point) == Some(Liquid::Water)).then_some(0)
        }
        fn surface_water_below(
            &self,
            origin: Vec3,
            bottom: f32,
            water: SurfaceRegion,
            region: usize,
        ) -> Option<f32> {
            (origin.is_finite()
                && bottom.is_finite()
                && origin.y > 0.
                && bottom < 0.
                && water == SurfaceRegion::Water
                && region == 0)
                .then_some(0.)
        }
        fn surface_region(&self, at: Vec3) -> Option<SurfaceRegion> {
            if !at.is_finite() || at.y == 0. {
                None
            } else if at.y < 0. {
                Some(SurfaceRegion::Water)
            } else {
                Some(SurfaceRegion::Air)
            }
        }
        fn surface_segment(&self, from: Vec3, to: Vec3, water: SurfaceRegion) -> bool {
            water == SurfaceRegion::Water
                && from.is_finite()
                && to.is_finite()
                && self.surface_region(from).is_some()
                && self.surface_region(to).is_some()
        }
        fn liquid_at(&self, at: Vec3) -> Option<Liquid> {
            (at.y < 0.).then_some(Liquid::Water)
        }
        fn passes_through(&self, from: Vec3, to: Vec3, liquid: Liquid) -> bool {
            liquid == Liquid::Water && (from.y < 0. || to.y < 0.)
        }
        fn first_outside(&self, from: Vec3, to: Vec3, _: Liquid) -> Option<Range<f64>> {
            if from.y < 0. && to.y < 0. {
                None
            } else {
                Some(0.0..1.0)
            }
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            None
        }
    }
    fn pool(floor: f32) -> CollisionWorld {
        CollisionWorld::new([
            [
                [-100., floor, -100.],
                [1000., floor, 100.],
                [1000., floor, -100.],
            ],
            [
                [-100., floor, -100.],
                [-100., floor, 100.],
                [1000., floor, 100.],
            ],
        ])
        .unwrap()
        .with_liquids(Pool)
        .with_swim_origin(3.75)
    }
    fn stroke(pitch: f32, ms: u64) -> SurfaceStroke {
        SurfaceStroke {
            set: LIQUID_5,
            input: WetInput {
                horizontal: [1.7, 0.],
                vertical: 1.7 * pitch.to_radians().sin(),
                jump: false,
            },
            seconds: std::time::Duration::from_millis(ms).as_secs_f32(),
            height: 6.677_319_5,
        }
    }
    fn reference(pitch: f32, millis: u64) -> (f64, f64) {
        // Independent f64 transcription of the fixed-60 read. Whole frames
        // update velocity once; the last partial uses that same velocity.
        let (mut x, mut y, mut vx, mut vy) = (0., -0.1, 0., 0.);
        let mut wet = true;
        let mut units = millis * 60;
        while units > 0 {
            let covered = units.min(1000);
            vy -= 0.4 / 3.;
            if wet {
                vx = 0.95 * vx + 0.025 * 1.7;
                vy = 0.2 * vy + 0.8 * 1.7 * f64::from(pitch).to_radians().sin();
            } else {
                vx *= 0.99;
                vy *= 0.99;
            }
            #[allow(clippy::cast_precision_loss)] // At most 1000 integer clock units.
            let seconds_in_steps = covered as f64 / 3000.;
            x += vx * seconds_in_steps;
            y += vy * seconds_in_steps;
            wet = y < 0.;
            units -= covered;
        }
        (x, y)
    }
    #[test]
    fn surface_owner_matrix_carries_clock_and_joins_only_numeric_remainders() {
        for pitch in [15., 60., 85.] {
            for cadence in [
                &[100_u64][..],
                &[150][..],
                &[250][..],
                &[100, 117, 167, 200, 250][..],
            ] {
                let world = pool(-1000.);
                let (mut feet, mut state) = (Vec3::Y * -3.85, None);
                let mut tails = 0;
                let mut elapsed = 0;
                let mut deep = WetVelocity::default();
                for i in 0..50 {
                    let ms = cadence[i % cadence.len()];
                    let input = stroke(pitch, ms);
                    let plan = SurfaceClock::plan(
                        state.as_ref(),
                        &world,
                        feet,
                        WetVelocity::default(),
                        input,
                    )
                    .unwrap_or_else(|e| panic!("pitch{pitch} cadence{cadence:?} row{i}: {e:?}"));
                    assert!(plan.checks > 0 && plan.pieces <= 16);
                    assert!(plan.tails <= 1);
                    tails += plan.tails;
                    let (mut left, mut deep_h, mut deep_path) = (input.seconds, 0., 0.);
                    while left > 0. {
                        let p = deep
                            .advance_piece(WetFrameMedium::Water(input.set), input.input, left)
                            .unwrap();
                        left -= p.seconds;
                        let delta = Vec3::new(p.horizontal[0], p.vertical, p.horizontal[1]);
                        deep_h += f64::from(delta.x);
                        deep_path += delta.as_dvec3().length();
                    }
                    assert!(f64::from(plan.feet.x - feet.x) <= deep_h);
                    assert!(plan.path <= deep_path);
                    elapsed += ms;
                    let (x, y) = reference(pitch, elapsed);
                    assert!((f64::from(plan.feet.x) - x).abs() < 0.000_2);
                    assert!((f64::from(plan.feet.y) + 3.75 - y).abs() < 0.000_2);
                    feet = plan.feet;
                    state = Some(plan.clock);
                }
                assert!(tails > 0);
            }
        }
    }
    #[test]
    fn surface_trials_do_not_mutate_the_original_and_contact_declines() {
        let clear = pool(-1000.);
        let first = SurfaceClock::plan(
            None,
            &clear,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            stroke(15., 100),
        )
        .unwrap();
        let original = first.clock.velocity();
        let _ = SurfaceClock::plan(
            Some(&first.clock),
            &clear,
            first.feet,
            original,
            stroke(15., 117),
        )
        .unwrap();
        assert_eq!(first.clock.velocity(), original);
        let changed = pool(-1000.);
        assert!(matches!(
            SurfaceClock::plan(
                Some(&first.clock),
                &changed,
                first.feet,
                original,
                stroke(15., 100)
            ),
            Err(SurfaceDecline::Context)
        ));
        let floor = pool(-3.85);
        assert!(matches!(
            SurfaceClock::plan(
                None,
                &floor,
                Vec3::Y * -3.85,
                WetVelocity::default(),
                stroke(15., 100)
            ),
            Err(SurfaceDecline::Contact)
        ));
    }
    #[test]
    fn surface_whole_wet_tracker_preserves_native_velocity_bits() {
        let world = pool(-1000.);
        let feet = Vec3::Y * -20.;
        let input = stroke(0., 100);
        let plan = SurfaceClock::plan(None, &world, feet, WetVelocity::default(), input).unwrap();
        assert!(!plan.active);
        let mut ordinary = WetVelocity::default();
        ordinary.advance(input.set, input.input, input.seconds);
        assert_eq!(ordinary, plan.clock.velocity());
        let mut partial = WetVelocity::default();
        partial.advance(input.set, input.input, 0.117);
        let remaining = partial.frame_phase().unwrap().remaining_seconds;
        let recovered = SurfaceClock::plan(None, &world, feet, partial, input).unwrap();
        assert_eq!(recovered.boundary_delay, remaining);
        assert!(!recovered.active);
        let mut expected = partial;
        expected.advance(input.set, input.input, input.seconds);
        assert_eq!(recovered.clock.velocity(), expected);
    }

    #[test]
    fn surface_retake_requires_a_checked_wet_boundary_not_a_crossing_guess() {
        let world = pool(-1000.);
        let mut seed = WetVelocity::default();
        let input = stroke(85., 100);
        seed.advance(input.set, input.input, 0.117);
        assert!(!seed.frame_phase().unwrap().at_boundary);
        // The stored partial moves out of water before its first boundary.
        // The old medium cannot be inferred from the new endpoint.
        assert!(matches!(
            SurfaceClock::plan(None, &world, Vec3::Y * -3.750_001, seed, input),
            Err(SurfaceDecline::Region)
        ));
        let mut short = input;
        short.seconds = seed.frame_phase().unwrap().remaining_seconds / 2.;
        assert!(matches!(
            SurfaceClock::plan(None, &world, Vec3::Y * -20., seed, short),
            Err(SurfaceDecline::Phase)
        ));
        assert!(matches!(
            SurfaceClock::plan(None, &pool(-20.), Vec3::Y * -20., seed, input),
            Err(SurfaceDecline::Contact)
        ));
    }

    #[test]
    fn surface_changed_stroke_is_latched_only_at_the_existing_frame_boundary() {
        let world = pool(-1000.);
        let first = SurfaceClock::plan(
            None,
            &world,
            Vec3::Y * -20.,
            WetVelocity::default(),
            stroke(15., 117),
        )
        .unwrap();
        let mut changed = stroke(-15., 100);
        changed.input.horizontal = [0., 1.7];
        let mut expected = first.clock.velocity();
        expected.advance(changed.set, changed.input, changed.seconds);
        let next = SurfaceClock::plan(
            Some(&first.clock),
            &world,
            first.feet,
            first.clock.velocity(),
            changed,
        )
        .unwrap();
        assert_eq!(next.clock.velocity(), expected);
        assert_eq!(next.boundary_delay, 0.);
        assert_eq!(next.clock.updates() - first.clock.updates(), 6);
    }

    #[test]
    fn surface_context_uses_exact_geometry_and_placements_not_list_allocations() {
        use super::super::{CollisionInstance, CollisionMesh};
        use glam::Quat;
        let mut world = pool(-1000.);
        let mesh = Arc::new(
            CollisionMesh::new([[[100., 100., 100.], [101., 100., 100.], [100., 101., 100.]]])
                .unwrap(),
        );
        let make = |at| CollisionInstance::new(Arc::clone(&mesh), at, Quat::IDENTITY).unwrap();
        world.set_door_obstacles(vec![(4, Arc::clone(&mesh))]);
        world.set_spawn_instances(vec![(8, make(Vec3::ZERO))]);
        let before = world.clone();
        world.set_door_obstacles(vec![(4, Arc::clone(&mesh))]);
        world.set_spawn_instances(vec![(8, make(Vec3::ZERO))]);
        assert!(before.same_surface_context(&world));
        world.set_spawn_instances(vec![(8, make(Vec3::X * f32::EPSILON))]);
        assert!(
            !before.same_surface_context(&world),
            "no placement tolerance"
        );
        world.set_spawn_instances(vec![(8, make(Vec3::ZERO))]);
        world.set_door_obstacles(vec![(5, Arc::clone(&mesh))]);
        assert!(
            !before.same_surface_context(&world),
            "door identity changed"
        );
        world.set_door_obstacles_with(vec![(
            4,
            Arc::clone(&mesh),
            super::super::Solidity {
                floor: false,
                wall: true,
            },
        )]);
        assert!(!before.same_surface_context(&world), "door rules changed");
        world.set_door_obstacles(Vec::new());
        world.set_spawn_instances(Vec::new());
        let empty = world.clone();
        world.set_door_obstacles(Vec::new());
        world.set_spawn_instances(Vec::new());
        assert!(empty.same_surface_context(&world));
    }

    #[test]
    fn surface_numeric_tail_alone_and_unavailable_regions_decline_untouched() {
        let w = pool(-1000.);
        let mut input = stroke(15., 100);
        input.seconds = 1e-8;
        assert!(matches!(
            SurfaceClock::plan(None, &w, Vec3::Y * -3.85, WetVelocity::default(), input),
            Err(SurfaceDecline::Tail)
        ));
        let no_regions = CollisionWorld::new([[
            [-100., -1000., -100.],
            [1000., -1000., 100.],
            [1000., -1000., -100.],
        ]])
        .unwrap();
        assert!(matches!(
            SurfaceClock::plan(
                None,
                &no_regions,
                Vec3::Y * -3.85,
                WetVelocity::default(),
                stroke(15., 100)
            ),
            Err(SurfaceDecline::Region)
        ));
    }

    struct Edge;
    impl Liquids for Edge {
        fn surface_water_identity(&self, point: Vec3) -> Option<usize> {
            (self.liquid_at(point) == Some(Liquid::Water)).then_some(0)
        }
        fn surface_region(&self, p: Vec3) -> Option<SurfaceRegion> {
            if !p.is_finite() || p.x.to_bits() == 0.5_f32.to_bits() || p.y == 0. {
                None
            } else {
                Some(if p.x < 0.5 && p.y < 0. {
                    SurfaceRegion::Water
                } else {
                    SurfaceRegion::Air
                })
            }
        }
        fn surface_segment(&self, a: Vec3, b: Vec3, water: SurfaceRegion) -> bool {
            water == SurfaceRegion::Water
                && self.surface_region(a).is_some()
                && self.surface_region(b).is_some()
        }
        fn surface_water_below(
            &self,
            origin: Vec3,
            bottom: f32,
            water: SurfaceRegion,
            region: usize,
        ) -> Option<f32> {
            (origin.is_finite()
                && bottom.is_finite()
                && origin.x < 0.5
                && origin.y > 0.
                && bottom < 0.
                && water == SurfaceRegion::Water
                && region == 0)
                .then_some(0.)
        }
        fn liquid_at(&self, p: Vec3) -> Option<Liquid> {
            (p.x < 0.5 && p.y < 0.).then_some(Liquid::Water)
        }
        fn passes_through(&self, a: Vec3, b: Vec3, liquid: Liquid) -> bool {
            liquid == Liquid::Water && (self.liquid_at(a).is_some() || self.liquid_at(b).is_some())
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            None
        }
    }

    struct TailEdge(f32);
    impl TailEdge {
        fn top(&self, point: Vec3) -> f32 {
            if point.x < self.0 { 0. } else { -1. }
        }
    }
    impl Liquids for TailEdge {
        fn surface_region(&self, point: Vec3) -> Option<SurfaceRegion> {
            if !point.is_finite()
                || point.x.to_bits() == self.0.to_bits()
                || point.y.to_bits() == self.top(point).to_bits()
            {
                None
            } else {
                Some(if point.y < self.top(point) {
                    SurfaceRegion::Water
                } else {
                    SurfaceRegion::Air
                })
            }
        }
        fn surface_water_identity(&self, point: Vec3) -> Option<usize> {
            (self.liquid_at(point) == Some(Liquid::Water)).then_some(0)
        }
        fn surface_water_below(
            &self,
            origin: Vec3,
            bottom: f32,
            water: SurfaceRegion,
            region: usize,
        ) -> Option<f32> {
            (origin.is_finite()
                && bottom.is_finite()
                && water == SurfaceRegion::Water
                && region == 0
                && origin.y > self.top(origin)
                && bottom < self.top(origin))
            .then(|| self.top(origin))
        }
        fn surface_segment(&self, a: Vec3, b: Vec3, water: SurfaceRegion) -> bool {
            water == SurfaceRegion::Water
                && self.surface_region(a).is_some()
                && self.surface_region(b).is_some()
        }
        fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
            (point.y < self.top(point)).then_some(Liquid::Water)
        }
        fn passes_through(&self, a: Vec3, b: Vec3, liquid: Liquid) -> bool {
            liquid == Liquid::Water && (self.liquid_at(a).is_some() || self.liquid_at(b).is_some())
        }
        fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
            None
        }
    }

    #[test]
    fn surface_strict_b_uses_the_frame_point_and_rejects_changed_or_unknown_tail_answers() {
        use super::super::water::WET_FRAME_SECONDS;
        let mut input = stroke(0., 100);
        input.seconds = WET_FRAME_SECONDS + 0.000_001;
        let mut velocity = WetVelocity::default();
        let raw = velocity
            .advance_piece(WetFrameMedium::Water(input.set), input.input, input.seconds)
            .unwrap();
        let phase = velocity.frame_phase().unwrap();
        let tail = velocity
            .advance_piece(
                WetFrameMedium::Water(input.set),
                input.input,
                input.seconds - raw.seconds,
            )
            .unwrap();
        assert!(
            phase.at_boundary && !tail.starts_frame && !tail.completes_frame,
            "actual API provenance, no client window copy"
        );
        let completion = raw.horizontal[0];
        let end = raw.horizontal[0] + tail.horizontal[0];
        assert!(end > completion);
        for boundary in [f32::midpoint(completion, end), completion] {
            let world = pool(-1000.).with_liquids(TailEdge(boundary));
            assert!(
                matches!(
                    SurfaceClock::plan(
                        None,
                        &world,
                        Vec3::Y * -3.85,
                        WetVelocity::default(),
                        input
                    ),
                    Err(SurfaceDecline::Region)
                ),
                "B declines a changed or unknown frame-completion answer"
            );
        }
        // A preceding frame may cross: its actual completion and tail end
        // must both be the same known air above that original water.
        input = stroke(15., 100);
        input.seconds = WET_FRAME_SECONDS + 0.000_001;
        let plan = SurfaceClock::plan(
            None,
            &pool(-1000.),
            Vec3::Y * -3.85,
            WetVelocity::default(),
            input,
        )
        .unwrap();
        assert_eq!(plan.tails, 1);
        assert_eq!(plan.clock.medium(), SurfaceRegion::Air);
        assert!(plan.active && plan.checks > 0);
    }

    #[test]
    fn surface_edge_declines_before_departure_and_declines_the_retained_300ms_state() {
        let edge = pool(-1000.).with_liquids(Edge);
        let first = SurfaceClock::plan(
            None,
            &edge,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            stroke(15., 100),
        )
        .unwrap();
        let second = SurfaceClock::plan(
            Some(&first.clock),
            &edge,
            first.feet,
            first.clock.velocity(),
            stroke(15., 100),
        )
        .unwrap();
        let snapshot = second.clock.velocity();
        assert!(matches!(
            SurfaceClock::plan(
                Some(&second.clock),
                &edge,
                second.feet,
                snapshot,
                stroke(15., 100)
            ),
            Err(SurfaceDecline::Region)
        ));
        assert_eq!(
            second.clock.velocity(),
            snapshot,
            "declined whole interval commits nothing"
        );
        // Historical state from the old integration, not an admission this
        // selector can now create: the frozen native 300ms prefix was
        // identical to infinite-water motion up to the edge-crossing frame.
        let infinite = pool(-1000.);
        let (mut state, mut feet) = (None, Vec3::Y * -3.85);
        for _ in 0..3 {
            let p = SurfaceClock::plan(
                state.as_ref(),
                &infinite,
                feet,
                WetVelocity::default(),
                stroke(15., 100),
            )
            .unwrap();
            feet = p.feet;
            state = Some(p.clock);
        }
        let mut historical = state.unwrap();
        assert!((feet.x - 0.564_379_75).abs() < 0.000_001);
        historical.world = edge.clone();
        assert!(
            matches!(
                SurfaceClock::plan(
                    Some(&historical),
                    &edge,
                    feet,
                    historical.velocity(),
                    stroke(15., 100)
                ),
                Err(SurfaceDecline::Region)
            ),
            "E3: 300 to 400ms is never submitted"
        );
    }

    #[test]
    fn surface_original_water_column_rejects_a_solid_below_an_otherwise_clear_body() {
        let mut world = pool(-1000.);
        let first = SurfaceClock::plan(
            None,
            &world,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            stroke(15., 100),
        )
        .unwrap();
        let platform = Arc::new(
            super::super::CollisionMesh::new([
                [[-100., 2., -100.], [100., 2., 100.], [100., 2., -100.]],
                [[-100., 2., -100.], [-100., 2., 100.], [100., 2., 100.]],
            ])
            .unwrap(),
        );
        world.set_door_obstacles(vec![(4, platform)]);
        let feet = Vec3::Y * 4.25;
        assert!(world.swim_clearance(feet, 6.677_319_5).is_none());
        let (observed, token) = world.observe_motion();
        assert!(matches!(
            first.clock.water_below(&observed, feet, 6.677_319_5),
            Err(SurfaceDecline::Contact)
        ));
        assert!(
            token.report().solid,
            "actual original vertical ray found the intervening platform"
        );
    }

    #[test]
    fn surface_dry_handback_keeps_the_original_owner_peak_through_a_later_fall() {
        let world = pool(-1000.);
        let first = SurfaceClock::plan(
            None,
            &world,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            stroke(85., 100),
        )
        .unwrap();
        let original_peak = first.clock.dry_peak();
        let second = SurfaceClock::plan(
            Some(&first.clock),
            &world,
            first.feet,
            first.clock.velocity(),
            stroke(85., 117),
        )
        .unwrap();
        let peak = second.clock.dry_peak().unwrap();
        assert!(
            peak > second.feet.y,
            "hand back on the descending side of the real arc"
        );
        assert!(peak >= first.clock.dry_peak().unwrap());
        assert_eq!(
            first.clock.dry_peak(),
            original_peak,
            "trial left prior history untouched"
        );
        let ([right, forward], up) = second.clock.velocity().units_per_second();
        let mut air = super::super::AirborneController::default();
        air.carry_surface_exit(up, Vec3::new(right, 0., forward), second.feet.y, peak);
        assert_eq!(air.peak(), Some(peak));
        let floor = second.feet.y - 8.;
        let landing_world = CollisionWorld::new([
            [
                [-100., floor, -100.],
                [1000., floor, 100.],
                [1000., floor, -100.],
            ],
            [
                [-100., floor, -100.],
                [-100., floor, 100.],
                [1000., floor, 100.],
            ],
        ])
        .unwrap();
        let physics = super::super::VerticalPhysics {
            gravity: 160.,
            air_factor: 0.99,
            frames_per_step: 3,
            steers_in_air: false,
            ..super::super::PROVISIONAL_PHYSICS
        };
        let mut feet = second.feet;
        let mut landing = None;
        for _ in 0..100 {
            feet = air.step(
                &landing_world,
                feet,
                physics,
                super::super::MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: 0.05,
                    height: 6.677_319_5,
                },
            );
            landing = air.take_landing();
            if landing.is_some() {
                break;
            }
        }
        let landed = landing.expect("ordinary original owner reached the floor");
        assert_eq!(landed.fall_distance, peak - floor);
        assert!(
            landed.fall_distance > second.feet.y - floor,
            "the prior peak cannot shrink to the handback point"
        );
        // A genuine wet refresh legitimately starts a new history epoch.
        let a = SurfaceClock::plan(
            None,
            &world,
            Vec3::Y * -3.85,
            WetVelocity::default(),
            stroke(15., 100),
        )
        .unwrap();
        let b = SurfaceClock::plan(
            Some(&a.clock),
            &world,
            a.feet,
            a.clock.velocity(),
            stroke(15., 100),
        )
        .unwrap();
        assert_eq!(b.clock.medium(), SurfaceRegion::Water);
        assert_eq!(b.clock.dry_peak(), None);
    }
}

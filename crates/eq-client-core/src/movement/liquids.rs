//! The zone's water and lava, which every walk keeps out of. The client
//! cannot swim yet, and reports no burns or drowning to the server, so a
//! step stops before the head would go under water or the feet into lava;
//! shallow water is waded while the head stays above it. A move in the air,
//! or off a ledge, is judged by where its fall would end; over no floor at
//! all the fall never ends, and it is made only if no water or lava lies
//! below. A slide down a slope too steep to stand on stops short of water
//! and lava as a step does. Feet that are already in deep water or lava, or
//! above it with no floor between, may go anywhere, so they can leave.
use super::{CollisionWorld, LIFT, Support};
use glam::Vec3;

/// What fills a point of the zone, where it is not air.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Liquid {
    /// Water, which the head stays above.
    Water,
    /// Lava, which the feet stay out of.
    Lava,
}

/// The zone's liquids, by point in the renderer's frame.
pub trait Liquids: Send + Sync {
    /// What fills the zone at a point, if anything.
    fn liquid_at(&self, point: Vec3) -> Option<Liquid>;

    /// What fills the zone first along a straight path from one point to
    /// another, if anything, however thin.
    fn liquid_along(&self, from: Vec3, to: Vec3) -> Option<Liquid>;
}

/// How far above the feet they are tested for lava, so that the plane of
/// the floor under them never decides it.
const SOLES: f32 = 0.1;
/// How far below the feet the floor a fall would end on is looked for.
const FALL_REACH: f32 = 10_000.0;

impl CollisionWorld {
    /// Keeps every walk in this world out of the zone's water and lava.
    #[must_use]
    pub fn with_liquids(mut self, liquids: impl Liquids + 'static) -> Self {
        self.liquids = Some(Box::new(liquids));
        self
    }

    /// Where feet here come to rest: where they stand, or the floor below
    /// them in the air. The look starts at the capsule's lift, so feet a
    /// server placed a little inside their floor find that floor. Feet over
    /// no floor at all never come to rest.
    fn rest(&self, feet: Vec3) -> Option<Vec3> {
        self.ray_distance(feet + Vec3::Y * LIFT, -Vec3::Y, FALL_REACH)
            .map(|distance| feet + Vec3::Y * (LIFT - distance))
    }

    /// Whether feet here stay clear of the zone's liquids where they come to
    /// rest: the head of a body this tall above water, and the feet out of
    /// lava. Feet over no floor at all, as over a sea drawn without a bed,
    /// fall for ever through whatever lies below, so they are clear only if
    /// no water or lava does.
    pub(super) fn dry(&self, feet: Vec3, height: f32) -> bool {
        let Some(liquids) = &self.liquids else {
            return true;
        };
        match self.rest(feet) {
            Some(rest) => {
                liquids.liquid_at(rest + Vec3::Y * height) != Some(Liquid::Water)
                    && liquids.liquid_at(rest + Vec3::Y * SOLES) != Some(Liquid::Lava)
            }
            None => liquids
                .liquid_along(feet + Vec3::Y * height, feet - Vec3::Y * FALL_REACH)
                .is_none(),
        }
    }

    /// How much of a straight move keeps feet clear of the zone's liquids
    /// where they come to rest, found by halving: all of it for feet that
    /// are not clear already, so they can leave.
    pub(super) fn dry_fraction(&self, feet: Vec3, displacement: Vec3, height: f32) -> f32 {
        if !self.dry(feet, height) || self.dry(feet + displacement, height) {
            return 1.0;
        }
        let (mut dry, mut wet) = (0.0, 1.0);
        for _ in 0..12 {
            let fraction = f32::midpoint(dry, wet);
            if self.dry(feet + displacement * fraction, height) {
                dry = fraction;
            } else {
                wet = fraction;
            }
        }
        dry
    }

    /// Whether feet here are in water or lava.
    pub(super) fn in_liquid(&self, feet: Vec3) -> bool {
        self.liquids
            .as_ref()
            .is_some_and(|liquids| liquids.liquid_at(feet + Vec3::Y * SOLES).is_some())
    }

    /// The longest part of a stride that stays clear of the zone's liquids,
    /// found by halving as a ledge's supported part is.
    pub(super) fn dry_part(
        &self,
        feet: Vec3,
        displacement: Vec3,
        height: f32,
        support: Support,
    ) -> Vec3 {
        let (mut dry, mut wet) = (0.0, 1.0);
        let mut reached = feet;
        for _ in 0..12 {
            let fraction = f32::midpoint(dry, wet);
            let next = self.stride(feet, displacement * fraction, height, support);
            if self.dry(next, height) {
                dry = fraction;
                reached = next;
            } else {
                wet = fraction;
            }
        }
        reached
    }
}

/// Liquids filling boxes, each from its lower corner to its upper, for the
/// movement tests.
#[cfg(test)]
pub(super) struct Boxes(pub(super) Vec<(Vec3, Vec3, Liquid)>);

#[cfg(test)]
impl Liquids for Boxes {
    fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
        self.0
            .iter()
            .find(|(low, high, _)| point.cmpge(*low).all() && point.cmple(*high).all())
            .map(|(_, _, liquid)| *liquid)
    }

    fn liquid_along(&self, from: Vec3, to: Vec3) -> Option<Liquid> {
        // Where the path enters each box it meets, as a fraction of it.
        let path = to - from;
        let entry = |low: Vec3, high: Vec3| {
            let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
            for axis in 0..3 {
                if path[axis] == 0.0 {
                    if from[axis] < low[axis] || from[axis] > high[axis] {
                        return None;
                    }
                    continue;
                }
                let a = (low[axis] - from[axis]) / path[axis];
                let b = (high[axis] - from[axis]) / path[axis];
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
            }
            (enter <= leave).then_some(enter)
        };
        self.0
            .iter()
            .filter_map(|(low, high, liquid)| Some((entry(*low, *high)?, *liquid)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, liquid)| liquid)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AirborneController, MotionStep, PROVISIONAL_PHYSICS, PathProgress, PathSearch,
    };
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// Flat ground at y 0 from x -20 to 10, then a bank sloping down to a
    /// lake bed at y -10 from x 30 on, under water up to y -1 from x 10.
    fn lake() -> (Vec<[[f32; 3]; 3]>, Boxes) {
        let mut triangles = quad(
            [-20.0, 0.0, -20.0],
            [10.0, 0.0, -20.0],
            [10.0, 0.0, 20.0],
            [-20.0, 0.0, 20.0],
        )
        .to_vec();
        triangles.extend(quad(
            [10.0, 0.0, -20.0],
            [30.0, -10.0, -20.0],
            [30.0, -10.0, 20.0],
            [10.0, 0.0, 20.0],
        ));
        triangles.extend(quad(
            [30.0, -10.0, -20.0],
            [60.0, -10.0, -20.0],
            [60.0, -10.0, 20.0],
            [30.0, -10.0, 20.0],
        ));
        let water = Boxes(vec![(
            Vec3::new(10.0, -20.0, -20.0),
            Vec3::new(60.0, -1.0, 20.0),
            Liquid::Water,
        )]);
        (triangles, water)
    }

    fn walk_on(world: &CollisionWorld, mut feet: Vec3, step: Vec3, steps: u8) -> Vec3 {
        for _ in 0..steps {
            feet = world.step(feet, step, 6.0);
        }
        feet
    }

    #[test]
    fn wading_stops_where_the_head_would_go_under() {
        let (triangles, water) = lake();
        let open = CollisionWorld::new(triangles.clone()).unwrap();
        assert!(walk_on(&open, Vec3::ZERO, Vec3::X, 40).x > 30.0);
        let world = CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(lake().1);
        let feet = walk_on(&world, Vec3::ZERO, Vec3::X, 40);
        // The head, six units up, meets the surface at y -1 where the bank
        // is at y -7, x 24: the feet wade in to there and no further.
        assert!((feet.x - 24.0).abs() < 0.1, "{feet:?}");
        assert_eq!(water.liquid_at(feet + Vec3::Y * SOLES), Some(Liquid::Water));
        assert_eq!(water.liquid_at(feet + Vec3::Y * 6.0), None);
        // Pressing on changes nothing; walking back out does.
        assert!(walk_on(&world, feet, Vec3::X, 5).distance(feet) < 0.01);
        assert!(walk_on(&world, feet, -Vec3::X, 5).x < feet.x - 4.0);
    }

    #[test]
    fn feet_in_deep_water_may_walk_out_of_it() {
        let (triangles, _) = lake();
        let world = CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(lake().1);
        let under = Vec3::new(40.0, -10.0, 0.0);
        assert!(!world.dry(under, 6.0));
        let out = walk_on(&world, under, -Vec3::X, 40);
        assert!(out.x < 10.0, "{out:?}");
    }

    #[test]
    fn no_step_reaches_lava_but_feet_in_it_may_leave() {
        let triangles = quad(
            [-20.0, 0.0, -20.0],
            [20.0, 0.0, -20.0],
            [20.0, 0.0, 20.0],
            [-20.0, 0.0, 20.0],
        );
        let lava = || {
            Boxes(vec![(
                Vec3::new(10.0, -1.0, -20.0),
                Vec3::new(20.0, 0.5, 20.0),
                Liquid::Lava,
            )])
        };
        let world = CollisionWorld::new(triangles).unwrap().with_liquids(lava());
        let feet = walk_on(&world, Vec3::ZERO, Vec3::X, 20);
        assert!(feet.x < 10.0 && feet.x > 9.9, "{feet:?}");
        let burning = Vec3::new(15.0, 0.0, 0.0);
        assert!(walk_on(&world, burning, -Vec3::X, 10).x < 6.0);
    }

    /// A platform at y 10 for x below 0, over ground at y 0 that is under
    /// water up to y 8 from x 0 on, or only past z 10 when `beside`.
    fn ledge(beside: bool) -> CollisionWorld {
        let mut triangles = quad(
            [-20.0, 10.0, -20.0],
            [0.0, 10.0, -20.0],
            [0.0, 10.0, 20.0],
            [-20.0, 10.0, 20.0],
        )
        .to_vec();
        triangles.extend(quad(
            [-20.0, 0.0, -20.0],
            [20.0, 0.0, -20.0],
            [20.0, 0.0, 20.0],
            [-20.0, 0.0, 20.0],
        ));
        let low = Vec3::new(0.0, -5.0, if beside { 10.0 } else { -20.0 });
        CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(Boxes(vec![(
                low,
                Vec3::new(20.0, 8.0, 20.0),
                Liquid::Water,
            )]))
    }

    fn airborne(world: &CollisionWorld, mut feet: Vec3, jump: bool) -> Vec3 {
        let mut controller = AirborneController::default();
        for tick in 0..60 {
            feet = controller.step(
                world,
                feet,
                PROVISIONAL_PHYSICS,
                MotionStep {
                    horizontal: Vec3::X * 0.3,
                    jump: jump && tick == 0,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
        }
        feet
    }

    #[test]
    fn a_fall_that_would_end_under_water_is_not_taken() {
        let start = Vec3::new(-2.0, 10.0, 0.0);
        // Walking off the ledge, and jumping off it, both stop at its edge.
        for jump in [false, true] {
            let feet = airborne(&ledge(false), start, jump);
            assert!(feet.x < 0.5 && (feet.y - 10.0).abs() < 0.01, "{feet:?}");
        }
        // Where the ground below is dry, the fall is taken.
        let feet = airborne(&ledge(true), start, false);
        assert!(feet.x > 5.0 && feet.y.abs() < 0.01, "{feet:?}");
    }

    #[test]
    fn a_jump_at_the_waters_edge_holding_forward_goes_up_and_down() {
        let world = ledge(false);
        let mut controller = AirborneController::default();
        let mut feet = Vec3::new(-0.5, 10.0, 0.0);
        let mut heights = Vec::new();
        for tick in 0..30 {
            feet = controller.step(
                &world,
                feet,
                PROVISIONAL_PHYSICS,
                MotionStep {
                    horizontal: Vec3::X * 0.3,
                    jump: tick == 0,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            heights.push(feet.y);
        }
        // Forward is held throughout. Only the step over the water is
        // refused: gravity still acts, so the jump rises, falls and lands
        // back on the ledge rather than hanging over the water.
        let peak = (0..heights.len())
            .max_by(|a, b| heights[*a].total_cmp(&heights[*b]))
            .unwrap_or_default();
        assert!(heights[peak] > 11.0, "{heights:?}");
        let landed = heights[peak..].iter().position(|y| (y - 10.0).abs() < 0.01);
        assert!(landed.is_some_and(|ticks| peak + ticks < 20), "{heights:?}");
        assert!((feet.y - 10.0).abs() < 0.01 && feet.x < 0.5, "{feet:?}");
        assert!(controller.velocity().abs() < 0.001);
    }

    /// A shore at y 0 for x below 0 with no floor past it, and these
    /// liquids.
    fn shore(liquids: Boxes) -> CollisionWorld {
        CollisionWorld::new(
            quad(
                [-20.0, 0.0, -20.0],
                [0.0, 0.0, -20.0],
                [0.0, 0.0, 20.0],
                [-20.0, 0.0, 20.0],
            )
            .to_vec(),
        )
        .unwrap()
        .with_liquids(liquids)
    }

    #[test]
    fn a_walk_stops_at_water_with_no_floor_under_it() {
        // Past the shore, water down to y -100 with nothing under it, as at
        // the edge of a zone's sea.
        let world = shore(Boxes(vec![(
            Vec3::new(0.0, -100.0, -20.0),
            Vec3::new(40.0, -0.5, 20.0),
            Liquid::Water,
        )]));
        // The walk stops at the shore rather than sinking for ever, and so
        // does a jump from it.
        let feet = airborne(&world, Vec3::new(-5.0, 0.0, 0.0), false);
        assert!(feet.x < 0.5 && feet.y.abs() < 0.01, "{feet:?}");
        let jumped = airborne(&world, Vec3::new(-1.0, 0.0, 0.0), true);
        assert!(jumped.x < 0.5 && jumped.y.abs() < 0.01, "{jumped:?}");
    }

    #[test]
    fn a_walk_off_a_floor_over_nothing_at_all_still_falls() {
        // With no water or lava below, the fall is taken, as into a pit
        // that leads to another zone.
        let feet = airborne(&shore(Boxes(Vec::new())), Vec3::new(-1.0, 0.0, 0.0), false);
        assert!(feet.x > 0.0 && feet.y < -10.0, "{feet:?}");
    }

    /// A slope too steep to stand on, rising at 60 degrees along x from x
    /// 0, above lava over the floor at its foot.
    fn steep_slope_over_lava() -> CollisionWorld {
        let rise = 60.0_f32.to_radians().tan() * 10.0;
        let mut triangles = quad(
            [-20.0, 0.0, -20.0],
            [0.0, 0.0, -20.0],
            [0.0, 0.0, 20.0],
            [-20.0, 0.0, 20.0],
        )
        .to_vec();
        triangles.extend(quad(
            [0.0, 0.0, -20.0],
            [10.0, rise, -20.0],
            [10.0, rise, 20.0],
            [0.0, 0.0, 20.0],
        ));
        CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(Boxes(vec![(
                Vec3::new(-20.0, -1.0, -20.0),
                Vec3::new(0.0, 0.5, 20.0),
                Liquid::Lava,
            )]))
    }

    #[test]
    fn a_slide_down_a_slope_too_steep_to_stand_on_stops_short_of_lava() {
        // Dropped onto the slope, the character slides to its foot, which
        // the lava stops short of.
        let world = steep_slope_over_lava();
        let mut controller = AirborneController::default();
        let mut feet = Vec3::new(5.0, 20.0, 0.0);
        for _ in 0..400 {
            feet = controller.step(
                &world,
                feet,
                PROVISIONAL_PHYSICS,
                MotionStep {
                    horizontal: Vec3::ZERO,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
        }
        assert!(!world.in_liquid(feet) && feet.y < 2.0, "{feet:?}");
    }

    #[test]
    fn a_walk_off_a_cliff_does_not_slide_down_its_face_into_lava() {
        // A plateau at y 20 for x below 0, a 75-degree face down to y 0, and
        // lava over the floor at its foot.
        let foot = 20.0 / 75.0_f32.to_radians().tan();
        let mut triangles = quad(
            [-20.0, 20.0, -20.0],
            [0.0, 20.0, -20.0],
            [0.0, 20.0, 20.0],
            [-20.0, 20.0, 20.0],
        )
        .to_vec();
        triangles.extend(quad(
            [0.0, 20.0, -20.0],
            [foot, 0.0, -20.0],
            [foot, 0.0, 20.0],
            [0.0, 20.0, 20.0],
        ));
        triangles.extend(quad(
            [foot, 0.0, -20.0],
            [40.0, 0.0, -20.0],
            [40.0, 0.0, 20.0],
            [foot, 0.0, 20.0],
        ));
        let world = CollisionWorld::new(triangles)
            .unwrap()
            .with_liquids(Boxes(vec![(
                Vec3::new(foot, -1.0, -20.0),
                Vec3::new(40.0, 0.5, 20.0),
                Liquid::Lava,
            )]));
        // The walk goes off the edge and slides down the face, which the
        // feet cannot stand on, but stops at its foot rather than in lava.
        let feet = airborne(&world, Vec3::new(-5.0, 20.0, 0.0), false);
        assert!(
            !world.in_liquid(feet) && feet.x < foot + 0.01 && feet.y < 1.0,
            "{feet:?}"
        );
    }

    #[test]
    fn routes_go_around_deep_water() {
        let triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        );
        let pond = || {
            Boxes(vec![(
                Vec3::new(-4.0, -5.0, -8.0),
                Vec3::new(4.0, 10.0, 8.0),
                Liquid::Water,
            )])
        };
        let world = CollisionWorld::new(triangles).unwrap().with_liquids(pond());
        let (start, goal) = (Vec3::new(-10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
        let mut search = PathSearch::new(start, goal, 2.0, 6.0).with_drops(true);
        let path = loop {
            match search.advance(&world, 64) {
                PathProgress::Searching => (),
                PathProgress::Found(path) => break path,
                PathProgress::Failed => panic!("expected a route around the pond"),
            }
        };
        assert!(path.iter().any(|feet| feet.z.abs() > 8.0), "{path:?}");
        for feet in &path {
            assert_eq!(pond().liquid_at(*feet + Vec3::Y * 6.0), None, "{path:?}");
        }
    }
}

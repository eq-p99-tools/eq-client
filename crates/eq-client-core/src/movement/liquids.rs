//! The zone's water and lava, which every walk keeps out of. The client
//! cannot swim yet, and reports no burns or drowning to the server, so a
//! step stops before the head would go under water or the feet into lava;
//! shallow water is waded while the head stays above it. A move in the air,
//! or off a ledge, is judged by where its fall would end. Feet that are
//! already in deep water or lava may go anywhere, so they can leave it.
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
    /// server placed a little inside their floor find that floor.
    fn rest(&self, feet: Vec3) -> Vec3 {
        self.ray_distance(feet + Vec3::Y * LIFT, -Vec3::Y, FALL_REACH)
            .map_or(feet, |distance| feet + Vec3::Y * (LIFT - distance))
    }

    /// Whether feet here stay clear of the zone's liquids where they come to
    /// rest: the head of a body this tall above water, and the feet out of
    /// lava.
    pub(super) fn dry(&self, feet: Vec3, height: f32) -> bool {
        let Some(liquids) = &self.liquids else {
            return true;
        };
        let rest = self.rest(feet);
        liquids.liquid_at(rest + Vec3::Y * height) != Some(Liquid::Water)
            && liquids.liquid_at(rest + Vec3::Y * SOLES) != Some(Liquid::Lava)
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

#[cfg(test)]
mod tests {
    use super::super::{
        AirborneController, MotionStep, PROVISIONAL_PHYSICS, PathProgress, PathSearch,
    };
    use super::*;

    /// Liquids filling boxes, each from its lower corner to its upper.
    struct Boxes(Vec<(Vec3, Vec3, Liquid)>);

    impl Liquids for Boxes {
        fn liquid_at(&self, point: Vec3) -> Option<Liquid> {
            self.0
                .iter()
                .find(|(low, high, _)| point.cmpge(*low).all() && point.cmple(*high).all())
                .map(|(_, _, liquid)| *liquid)
        }
    }

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

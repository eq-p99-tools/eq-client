use super::{CollisionWorld, Liquid};
use glam::Vec3;

/// Allows standing coordinates to round across a liquid boundary.
pub(super) const LIQUID_SURFACE_SLOP: f32 = 0.01;
const EDGE: f32 = 0.000_1;

impl CollisionWorld {
    /// Shares solid geometry and adds mapped water and lava tops to
    /// direct-motion ground probes. An immersed pose uses ordinary movement
    /// and recovery until it clears the liquid.
    #[must_use]
    pub fn with_liquid_support(&self, feet: Vec3, height: f32) -> Option<Self> {
        let liquids = self.liquids.as_ref()?;
        if !feet.is_finite()
            || !height.is_finite()
            || height < 1.0
            || self.submerged(feet, height)
            || liquids.liquid_at(feet + Vec3::Y * LIQUID_SURFACE_SLOP) == Some(Liquid::Lava)
        {
            return None;
        }
        Some(Self {
            liquid_support: true,
            ..self.clone()
        })
    }

    /// The mapped liquid top at the center of the feet within standing
    /// tolerance. Solid ground wins at the same height. This geometry query
    /// is separate from immersion, movement settings and server state.
    #[must_use]
    pub fn liquid_surface_at_feet(&self, feet: Vec3) -> Option<Liquid> {
        let (surface, liquid) =
            self.probe_liquid_surface(feet, LIQUID_SURFACE_SLOP, LIQUID_SURFACE_SLOP)?;
        self.solid_ground(feet, LIQUID_SURFACE_SLOP, LIQUID_SURFACE_SLOP)
            .is_none_or(|solid| solid < surface - EDGE)
            .then_some(liquid)
    }

    pub(super) fn liquid_ground(&self, feet: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        if !self.liquid_support {
            return None;
        }
        self.probe_liquid_surface(feet, step_up, drop)
            .map(|(surface, _)| surface)
    }

    fn probe_liquid_surface(&self, feet: Vec3, step_up: f32, drop: f32) -> Option<(f32, Liquid)> {
        let liquids = self.liquids.as_ref()?;
        // Liquid never uses the stair climb budget to lift someone from below.
        let upper = feet.y + step_up.min(LIQUID_SURFACE_SLOP);
        let lower = feet.y - drop;
        if lower > upper {
            return None;
        }
        let top = Vec3::new(feet.x, upper.next_up(), feet.z);
        // Give a boundary at the end of the probe a nonempty wet span.
        let (surface, liquid) = liquids.liquid_surface(top, (lower - EDGE).next_down())?;
        (surface >= lower - EDGE && surface <= upper).then(|| (surface.max(lower), liquid))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AirborneController, Liquid, MotionStep, PROVISIONAL_PHYSICS, liquids::Boxes,
    };
    use super::*;

    fn floor(left: f32, right: f32, y: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [[left, y, -8.0], [right, y, -8.0], [right, y, 8.0]],
            [[left, y, -8.0], [right, y, 8.0], [left, y, 8.0]],
        ]
    }

    fn pool(liquid: Liquid) -> (Vec3, Vec3, Liquid) {
        (
            Vec3::new(0.0, -20.0, -8.0),
            Vec3::new(8.0, 0.0, 8.0),
            liquid,
        )
    }

    fn banks() -> impl Iterator<Item = [[f32; 3]; 3]> {
        floor(-8.0, 0.0, 0.0)
            .into_iter()
            .chain(floor(0.0, 8.0, -12.0))
            .chain(floor(8.0, 16.0, 0.0))
    }

    #[test]
    fn jesus_mode_crosses_between_banks_only_in_the_scoped_world() {
        let shore = -Vec3::X * 2.0;
        let center = Vec3::X * 4.0;
        for liquid in [Liquid::Water, Liquid::Lava] {
            let ordinary = CollisionWorld::new(banks())
                .unwrap()
                .with_liquids(Boxes(vec![pool(liquid)]));
            let walking = ordinary.with_liquid_support(shore, 6.0).unwrap();
            assert!(ordinary.step(shore, Vec3::X * 6.0, 6.0).x < 1.0);
            let reached = walking.step(shore, Vec3::X * 6.0, 6.0);
            assert!(reached.distance(center) < 0.001, "{liquid:?}: {reached:?}");
            let across = walking.step(reached, Vec3::X * 6.0, 6.0);
            assert!(across.distance(Vec3::X * 10.0) < 0.001, "{across:?}");
            assert_eq!(walking.support(center), Some(0.0));
            assert_eq!(ordinary.support(center), None);
            assert_eq!(walking.nearest_ground(center, 0.01, 0.05), None);
            assert_eq!(walking.ground(center - Vec3::Y * 0.5, 3.0, 0.05), None);
            assert_eq!(ordinary.liquid_surface_at_feet(center), Some(liquid));
            assert_eq!(
                ordinary.liquid_contact(center, 6.0),
                super::super::LiquidContact::Unmarked
            );
        }
    }

    #[test]
    fn jesus_mode_defers_to_ordinary_recovery_after_a_lava_correction() {
        let world = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![pool(Liquid::Lava)]));
        let submerged = Vec3::new(4.0, -0.5, 0.0);
        assert_eq!(
            world.liquid_contact(submerged, 6.0),
            super::super::LiquidContact::Lava
        );
        assert!(world.with_liquid_support(submerged, 6.0).is_none());
        assert!(
            world
                .with_liquid_support(Vec3::new(4.0, -0.05, 0.0), 6.0)
                .is_none()
        );
        assert_eq!(world.liquid_surface_at_feet(submerged), None);
    }

    #[test]
    fn jesus_mode_cannot_walk_across_an_unmapped_gap() {
        let (low, high, liquid) = pool(Liquid::Lava);
        let first = (low, Vec3::new(2.0, high.y, high.z), liquid);
        let second = (Vec3::new(6.0, low.y, low.z), high, liquid);
        let start = Vec3::X;
        let world = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![first, second]))
            .with_liquid_support(start, 6.0)
            .unwrap();
        let stopped = world.step(start, Vec3::X * 6.0, 6.0);
        assert!((2.0..2.41).contains(&stopped.x), "{stopped:?}");
        assert_eq!(world.support(Vec3::X * 4.0), None);
    }

    #[test]
    fn jesus_mode_keeps_solid_support_and_collision() {
        let wall = [
            [[3.0, -1.0, -8.0], [3.0, 8.0, -8.0], [3.0, 8.0, 8.0]],
            [[3.0, -1.0, -8.0], [3.0, 8.0, 8.0], [3.0, -1.0, 8.0]],
        ];
        let world = CollisionWorld::new(banks().chain(wall).chain(floor(5.0, 7.0, 3.0)))
            .unwrap()
            .with_liquids(Boxes(vec![pool(Liquid::Lava)]))
            .with_liquid_support(Vec3::X, 6.0)
            .unwrap();
        let before_wall = world.step(Vec3::X, Vec3::X * 3.0, 6.0);
        assert!(
            before_wall.x > 1.5 && before_wall.x < 3.0,
            "{before_wall:?}"
        );
        let bridge_feet = Vec3::new(6.0, 3.0, 0.0);
        let bridge = world.ground(bridge_feet, 0.01, 4.0);
        assert!(bridge.is_some_and(|y| (y - 3.0).abs() < 0.001));
        assert_eq!(world.liquid_surface_at_feet(bridge_feet), None);
    }

    #[test]
    fn jesus_mode_checks_lava_between_valid_liquid_surfaces() {
        let start = Vec3::X * 2.0;
        // One strip catches the near-feet probe and the other catches the
        // ordinary soles probe. Neither is a top the feet reach from above.
        for (low, high) in [(0.005, 0.03), (0.04, 0.2)] {
            let lava = (
                Vec3::new(2.2, low, -8.0),
                Vec3::new(2.25, high, 8.0),
                Liquid::Lava,
            );
            let world = CollisionWorld::new(banks())
                .unwrap()
                .with_liquids(Boxes(vec![lava, pool(Liquid::Water)]))
                .with_liquid_support(start, 6.0)
                .unwrap();
            assert_eq!(world.support(Vec3::X * 4.0), Some(0.0));
            let stopped = world.step(start, Vec3::X * 2.0, 6.0);
            assert!((2.0..2.2).contains(&stopped.x), "{stopped:?}");
        }
    }

    #[test]
    fn jesus_mode_checks_the_swept_legs_of_a_shore_drop() {
        let start = Vec3::new(0.25, 1.0, 0.0);
        let delta = Vec3::X * 0.5;
        let landing = Vec3::X * 0.75;
        let terrain = CollisionWorld::new(
            floor(-8.0, 0.0, 1.0)
                .into_iter()
                .chain(floor(0.0, 8.0, -12.0)),
        )
        .unwrap();
        let with_lava = |left, right, low, high| {
            let lava = (
                Vec3::new(left, low, -8.0),
                Vec3::new(right, high, 8.0),
                Liquid::Lava,
            );
            terrain
                .clone()
                .with_liquids(Boxes(vec![lava, pool(Liquid::Water)]))
                .with_liquid_support(start, 6.0)
                .unwrap()
        };
        // The capsule travels at the bank's height before dropping. The
        // proposed chord misses a strip at its soles on that first leg.
        let across_lava = with_lava(0.44, 0.46, 1.08, 1.12);
        let stopped = across_lava.step(start, delta, 6.0);
        assert!((0.35..0.44).contains(&stopped.x), "{stopped:?}");
        // This strip is above the feet. It misses both probes on the
        // horizontal leg, but the upper probe would cross it in the drop.
        let above_drop = with_lava(0.73, 0.76, 1.025, 1.04);
        let stopped = above_drop.step(start, delta, 6.0);
        assert!((0.55..0.73).contains(&stopped.x), "{stopped:?}");
        // A mapped top below the feet can itself stop the descent.
        let lava_top = with_lava(0.73, 0.76, 0.80, 0.85);
        let reached = lava_top.step(start, delta, 6.0);
        assert!(
            reached.distance(Vec3::new(0.75, 0.85, 0.0)) < 0.001,
            "{reached:?}"
        );
        // A strip lower in the same column intersects only the chord.
        let above_lava = with_lava(0.44, 0.46, 0.67, 0.73);
        let reached = above_lava.step(start, delta, 6.0);
        assert!(reached.distance(landing) < 0.001, "{reached:?}");
    }

    #[test]
    fn jesus_mode_uses_normal_jump_and_landing() {
        let world = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![pool(Liquid::Lava)]))
            .with_liquid_support(Vec3::X * 4.0, 6.0)
            .unwrap();
        let midlake = Vec3::X * 4.0;
        let step = MotionStep {
            horizontal: Vec3::ZERO,
            jump: false,
            seconds: 0.05,
            height: 6.0,
        };
        let mut airborne = AirborneController::default();
        let mut feet = airborne.step(
            &world,
            midlake,
            PROVISIONAL_PHYSICS,
            MotionStep { jump: true, ..step },
        );
        assert!(feet.y > 0.0 && airborne.velocity() > 0.0, "{feet:?}");
        let mut landed = false;
        for _ in 0..60 {
            feet = airborne.step(&world, feet, PROVISIONAL_PHYSICS, step);
            if airborne.take_landing().is_some() {
                landed = true;
                break;
            }
        }
        assert!(landed && feet.distance(midlake) < 0.001, "{feet:?}");
    }
}

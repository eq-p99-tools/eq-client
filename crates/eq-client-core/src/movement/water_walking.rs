use super::CollisionWorld;
use glam::Vec3;

impl CollisionWorld {
    /// Shares the solid world and adds mapped water tops to direct-motion
    /// ground probes. Solid sweeps, vision rays and liquid queries keep
    /// their original geometry.
    #[must_use]
    pub fn with_water_support(&self) -> Self {
        Self {
            water_support: self.liquids.is_some(),
            ..self.clone()
        }
    }

    pub(super) fn water_ground(&self, feet: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        if !self.water_support {
            return None;
        }
        let liquids = self.liquids.as_ref()?;
        // The extra 0.01 only tolerates standing coordinate roundoff. Water
        // never uses the body-sized stair climb to lift someone from below.
        const EDGE: f32 = 0.000_1;
        let upper = feet.y + step_up.min(0.01);
        let lower = feet.y - drop;
        if lower > upper {
            return None;
        }
        let top = Vec3::new(feet.x, upper.next_up(), feet.z);
        // Give a water boundary at the end of the probe a nonempty wet span.
        let surface = liquids.water_surface(top, (lower - EDGE).next_down())?;
        (surface >= lower - EDGE && surface <= upper).then(|| surface.max(lower))
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

    fn lake() -> (Vec3, Vec3, Liquid) {
        (
            Vec3::new(0.0, -20.0, -8.0),
            Vec3::new(8.0, 0.0, 8.0),
            Liquid::Water,
        )
    }

    fn banks() -> impl Iterator<Item = [[f32; 3]; 3]> {
        floor(-8.0, 0.0, 0.0)
            .into_iter()
            .chain(floor(0.0, 8.0, -12.0))
            .chain(floor(8.0, 16.0, 0.0))
    }

    #[test]
    fn water_walking_crosses_between_banks_only_in_the_scoped_world() {
        let ordinary = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![lake()]));
        let walking = ordinary.with_water_support();
        let shore = -Vec3::X * 2.0;
        let midlake = Vec3::X * 4.0;
        assert!(ordinary.step(shore, Vec3::X * 6.0, 6.0).x < 1.0);
        let reached = walking.step(shore, Vec3::X * 6.0, 6.0);
        assert!(reached.distance(midlake) < 0.001, "{reached:?}");
        let across = walking.step(reached, Vec3::X * 6.0, 6.0);
        assert!(across.distance(Vec3::X * 10.0) < 0.001, "{across:?}");
        assert_eq!(walking.support(midlake), Some(0.0));
        assert_eq!(ordinary.support(midlake), None);
        assert_eq!(walking.nearest_ground(midlake, 0.01, 0.05), None);
        assert_eq!(walking.ground(midlake - Vec3::Y * 0.5, 3.0, 0.05), None);
    }

    #[test]
    fn water_walking_keeps_solid_support_and_collision() {
        let wall = [
            [[3.0, -1.0, -8.0], [3.0, 8.0, -8.0], [3.0, 8.0, 8.0]],
            [[3.0, -1.0, -8.0], [3.0, 8.0, 8.0], [3.0, -1.0, 8.0]],
        ];
        let world = CollisionWorld::new(banks().chain(wall).chain(floor(5.0, 7.0, 3.0)))
            .unwrap()
            .with_liquids(Boxes(vec![lake()]))
            .with_water_support();
        let before_wall = world.step(Vec3::X, Vec3::X * 3.0, 6.0);
        assert!(
            before_wall.x > 1.5 && before_wall.x < 3.0,
            "{before_wall:?}"
        );
        let bridge = world.ground(Vec3::new(6.0, 3.0, 0.0), 0.01, 4.0);
        assert!(bridge.is_some_and(|y| (y - 3.0).abs() < 0.001));
    }

    #[test]
    fn water_walking_checks_lava_between_valid_water_surfaces() {
        let lava = (
            Vec3::new(2.2, 0.04, -8.0),
            Vec3::new(2.25, 0.2, 8.0),
            Liquid::Lava,
        );
        let world = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![lava, lake()]))
            .with_water_support();
        assert_eq!(world.support(Vec3::X * 4.0), Some(0.0));
        let stopped = world.step(Vec3::X * 2.0, Vec3::X * 2.0, 6.0);
        assert!((2.0..2.2).contains(&stopped.x), "{stopped:?}");
    }

    #[test]
    fn water_walking_uses_normal_jump_and_landing() {
        let world = CollisionWorld::new(banks())
            .unwrap()
            .with_liquids(Boxes(vec![lake()]))
            .with_water_support();
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

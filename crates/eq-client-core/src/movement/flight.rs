//! Voluntary flight still collides with the character's solid world.

use super::{CollisionWorld, LIFT};
use glam::Vec3;

impl CollisionWorld {
    /// Sweeps a flying character through solid geometry without requiring ground
    /// support. Floors, ceilings and walls still stop the body; this grants no
    /// speed or session permission. The caller bounds the displacement.
    #[must_use]
    pub fn fly_step(&self, feet: Vec3, displacement: Vec3, height: f32) -> Vec3 {
        if !feet.is_finite() || !displacement.is_finite() || !height.is_finite() || height < 1.0 {
            return feet;
        }
        let guarded = !self.submerged(feet, height);
        if displacement.length_squared() < 0.000_001 {
            return feet;
        }
        // Ground walking lifts the capsule over small risers. Flight has no
        // step-up, so put its base at the feet to stop descents at the floor.
        // Stop on contact: projecting onto a slope could otherwise invent a
        // height change when the player only asked to move horizontally.
        let position = match self.sweep(feet - Vec3::Y * LIFT, displacement, height) {
            Ok(None) => feet + displacement,
            Ok(Some(hit)) => {
                let fraction = (hit.time_of_impact - 0.005 / displacement.length()).clamp(0.0, 1.0);
                feet + displacement * fraction
            }
            Err(_) => feet,
        };
        if guarded && !self.dry(position, height) {
            feet
        } else {
            position
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> CollisionWorld {
        CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, -20.0], [20.0, 0.0, 20.0]],
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [-20.0, 0.0, 20.0]],
            [
                [-20.0, 12.0, -20.0],
                [20.0, 12.0, 20.0],
                [20.0, 12.0, -20.0],
            ],
            [
                [-20.0, 12.0, -20.0],
                [-20.0, 12.0, 20.0],
                [20.0, 12.0, 20.0],
            ],
            [[5.0, 0.0, -20.0], [5.0, 12.0, -20.0], [5.0, 12.0, 20.0]],
            [[5.0, 0.0, -20.0], [5.0, 12.0, 20.0], [5.0, 0.0, 20.0]],
        ])
        .unwrap()
    }

    #[test]
    fn flight_crosses_air_but_stops_at_floors_ceilings_and_walls() {
        let world = room();
        let feet = Vec3::new(0.0, 2.0, 0.0);
        assert!((world.fly_step(feet, Vec3::Y, 6.0) - (feet + Vec3::Y)).length() < 0.001);
        let below = world.fly_step(feet, Vec3::NEG_Y * 10.0, 6.0);
        assert!(below.y >= 0.0 && below.y < 0.1, "{below:?}");
        let above = world.fly_step(feet, Vec3::Y * 10.0, 6.0);
        assert!(above.y <= 6.0 && above.y > 5.8, "{above:?}");
        let wall = world.fly_step(feet, Vec3::X * 10.0, 6.0);
        assert!(wall.x < 5.0 && wall.x > 4.0, "{wall:?}");
        let slope = CollisionWorld::new([
            [[3.0, 0.0, -20.0], [13.0, 10.0, -20.0], [13.0, 10.0, 20.0]],
            [[3.0, 0.0, -20.0], [13.0, 10.0, 20.0], [3.0, 0.0, 20.0]],
        ])
        .unwrap();
        let stopped = slope.fly_step(feet, Vec3::X * 10.0, 6.0);
        assert!(stopped.x < 6.0, "{stopped:?}");
        assert!(
            (stopped.y - feet.y).abs() < 0.001,
            "horizontal input cannot climb a slope"
        );
    }

    #[test]
    fn flight_refuses_nonfinite_or_invalid_body_queries() {
        let world = room();
        let feet = Vec3::new(0.0, 2.0, 0.0);
        for (delta, height) in [(Vec3::NAN, 6.0), (Vec3::Y, f32::NAN), (Vec3::Y, 0.0)] {
            assert!((world.fly_step(feet, delta, height) - feet).length() < f32::EPSILON);
        }
    }
}

//! Voluntary flight still collides with the character's solid world.

use super::{CAPSULE_RADIUS, CollisionWorld, LIFT, SKIN, body};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{Ray, RayCast, ShapeCastHit, ShapeCastOptions, Unsupported, cast_shapes},
    shape::Shape,
};
use std::cell::RefCell;

impl CollisionWorld {
    /// Sweeps a flying character through solid geometry without requiring ground
    /// support. Floors, ceilings and walls still stop the body. Liquids are
    /// observed separately at accepted positions. The caller grants and
    /// bounds the displacement.
    #[must_use]
    pub fn fly_step(&self, feet: Vec3, displacement: Vec3, height: f32) -> Vec3 {
        if !feet.is_finite() || !displacement.is_finite() || !height.is_finite() || height < 1.0 {
            return feet;
        }
        if displacement.length_squared() < 0.000_001 {
            return feet;
        }
        // Stop on contact: projecting onto a slope could otherwise invent a
        // height change when the player only asked to move horizontally.
        match self.flight_sweep(feet, displacement, height) {
            Ok(None) => feet + displacement,
            Ok(Some(hit)) => {
                let fraction = (hit.time_of_impact - 0.005 / displacement.length()).clamp(0.0, 1.0);
                feet + displacement * fraction
            }
            Err(_) => feet,
        }
    }

    /// Flight puts the capsule at the actual feet. A touching floor must not
    /// consume the mesh's nearest hit and conceal a later wall. Visit candidate
    /// triangles through the BVH and retire only nonclosing face contacts.
    pub(super) fn flight_sweep(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        let (pose, capsule) = body(feet - Vec3::Y * LIFT, height);
        let bounds = capsule.compute_aabb(&pose);
        let shift = -bounds.center();
        let margin = bounds.half_extents() + Vector::splat(SKIN);
        let velocity = Vector::from_array(delta.to_array());
        let ray = Ray::new(Vector::ZERO, velocity);
        let mut nearest: Option<ShapeCastHit> = None;
        for mesh in self.meshes() {
            let unsupported = RefCell::new(None);
            let hit = mesh
                .bvh()
                .find_best(
                    1.0,
                    |node, best| {
                        Aabb {
                            mins: node.mins() + shift - margin,
                            maxs: node.maxs() + shift + margin,
                        }
                        .cast_local_ray(&ray, best, true)
                        .unwrap_or(f32::MAX)
                    },
                    |index, best| {
                        let triangle = mesh.triangle(index);
                        if let Some(mut face) = triangle.normal() {
                            let mut distance = (pose.translation - triangle.a).dot(face);
                            if distance < 0.0 {
                                face = -face;
                                distance = -distance;
                            }
                            let extent =
                                CAPSULE_RADIUS + (height * 0.5 - CAPSULE_RADIUS) * face.y.abs();
                            // A geometric separating plane remains separating
                            // for this whole linear move when it is nonclosing.
                            // This also handles an exactly touching floor whose
                            // zero-distance GJK normal is numerically unstable.
                            // A capsule overlapping the plane can still meet a
                            // triangle edge, so keep its ordinary shape cast.
                            let roundoff = height * f32::EPSILON * 4.0;
                            if distance - extent >= -roundoff && velocity.dot(face) >= 0.0 {
                                return None;
                            }
                        }
                        let options = ShapeCastOptions {
                            max_time_of_impact: best,
                            target_distance: SKIN,
                            stop_at_penetration: false,
                            compute_impact_geometry_on_penetration: true,
                        };
                        let hit = match cast_shapes(
                            &pose,
                            velocity,
                            &capsule,
                            &Pose::IDENTITY,
                            Vector::ZERO,
                            &triangle,
                            options,
                        ) {
                            Ok(hit) => hit?,
                            Err(error) => {
                                *unsupported.borrow_mut() = Some(error);
                                return None;
                            }
                        };
                        Some(hit)
                    },
                )
                .map(|(_, hit)| hit);
            if let Some(error) = unsupported.into_inner() {
                return Err(error);
            }
            if let Some(hit) = hit
                && nearest
                    .as_ref()
                    .is_none_or(|previous| hit.time_of_impact < previous.time_of_impact)
            {
                nearest = Some(hit);
            }
        }
        Ok(nearest)
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
        let along = world.fly_step(Vec3::new(4.6, 0.0, 0.0), Vec3::Z, 6.0);
        assert!(
            (along.z - 1.0).abs() < 0.001,
            "wall contact permits a tangent: {along:?}"
        );
        let edge = world.fly_step(Vec3::new(4.8, 0.0, 20.37), -Vec3::Z, 6.0);
        assert!(
            edge.z > 20.0,
            "a closing edge contact remains solid: {edge:?}"
        );
    }

    #[test]
    fn flight_can_leave_a_supporting_floor_without_inventing_height() {
        let world = room();
        let feet = Vec3::ZERO;
        let across = world.fly_step(feet, Vec3::X, 6.0);
        assert!((across - Vec3::X).length() < 0.001, "{across:?}");
        let above = world.fly_step(feet, Vec3::Y, 6.0);
        assert!((above - Vec3::Y).length() < 0.001, "{above:?}");
        let below = world.fly_step(feet, -Vec3::Y, 6.0);
        assert!(below.y.abs() <= 0.001, "{below:?}");
        let wall = world.fly_step(feet, Vec3::X * 10.0, 6.0);
        assert!(
            wall.x > 4.0 && wall.x < 5.0,
            "the floor must not hide the wall: {wall:?}"
        );
    }

    #[test]
    fn flight_crosses_liquids_without_floor_support_or_liquid_walls() {
        use super::super::{Liquid, SwimDepth, liquids::Boxes};

        let world = CollisionWorld::new([
            [
                [-10.0, -12.0, -10.0],
                [10.0, -12.0, -10.0],
                [10.0, -12.0, 10.0],
            ],
            [
                [-10.0, -12.0, -10.0],
                [10.0, -12.0, 10.0],
                [-10.0, -12.0, 10.0],
            ],
        ])
        .unwrap()
        .with_liquids(Boxes(vec![
            (
                Vec3::splat(-10.0),
                Vec3::new(10.0, 1.0, 10.0),
                Liquid::Water,
            ),
            (
                Vec3::new(0.4, -3.0, -1.0),
                Vec3::new(0.6, -2.0, 1.0),
                Liquid::Lava,
            ),
        ]));
        let feet = Vec3::new(0.0, -2.5, 0.0);
        let height = 6.0;
        assert_eq!(world.swim_depth(feet, height), Some(SwimDepth::Surface));
        assert!(world.support(feet).is_none());
        let above = feet + Vec3::Y * 5.0;
        assert!(!world.dry(above, height));
        assert_eq!(world.fly_step(feet, Vec3::Y * 5.0, height), above);
        assert_eq!(world.fly_step(above, Vec3::X, height), above + Vec3::X);

        let below = feet - Vec3::Y * 3.0;
        assert_eq!(world.fly_step(feet, Vec3::NEG_Y * 3.0, height), below);
        assert_eq!(world.swim_depth(below, height), Some(SwimDepth::Submerged));
        assert_eq!(world.fly_step(below, Vec3::Y * 3.0, height), feet);
        assert_eq!(world.fly_step(feet, Vec3::X, height), feet + Vec3::X);
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

//! The floor cached by a local pad, separate from ordinary floor/climb queries.
use super::{AirborneController, CollisionWorld, Surface};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{ShapeCastOptions, cast_shapes},
    shape::Ball,
};

/// One observed pad arrival, consumed by its first physics step. Creating it
/// never changes the destination origin or the body's momentum. The caller
/// owns admission/placement identity and discards it when that identity changes.
#[derive(Debug)]
pub struct PadArrival {
    origin: Vec3,
    stand_height: f32,
    floor: Option<f32>,
}

impl PadArrival {
    /// The authored destination origin, before the first physics step.
    #[must_use]
    pub const fn origin(&self) -> Vec3 {
        self.origin
    }

    /// The pad's separate cached floor; absence is not ground support.
    #[must_use]
    pub const fn floor(&self) -> Option<f32> {
        self.floor
    }

    /// Consumes the cached floor once. Titanium's next motion step puts an
    /// origin whose soles are below this floor at F+S, with no climb limit,
    /// and clears vertical velocity after ordinary landing handling
    /// (0x48b6c5, read6.654). No hit, or a floor at/below the soles, leaves
    /// the origin and momentum for ordinary motion. This rule is pad-only.
    #[must_use]
    pub fn first_step(self, airborne: &mut AirborneController) -> Vec3 {
        let Some(floor) = self
            .floor
            .filter(|floor| *floor > self.origin.y - self.stand_height)
        else {
            return self.origin;
        };
        airborne.pad_floor(floor);
        Vec3::new(self.origin.x, floor + self.stand_height, self.origin.z)
    }
}

impl CollisionWorld {
    /// Observes the local pad's arrival floor without placing or snapping the
    /// body: one downward sphere from origin+1, radius0.1*S; the cached
    /// height is its contact centre minus radius plus0.001 (Titanium
    /// 0x48aaa6/0x48a0fc, read6.654). Only floor-solid authored front faces
    /// participate, including rigid placements. Invalid input yields no
    /// observation; a valid query that misses returns an observation with
    /// no floor. Ordinary support queries and their climb reach are unchanged.
    #[must_use]
    pub fn pad_arrival(&self, origin: Vec3, stand_height: f32) -> Option<PadArrival> {
        if !origin.is_finite() || !stand_height.is_finite() || stand_height <= 0.0 {
            return None;
        }
        let radius = 0.1 * stand_height;
        let start = origin + Vec3::Y;
        let floor = self
            .floor_meshes()
            .filter_map(|surface| surface.pad_floor(start, radius))
            .max_by(f32::total_cmp);
        Some(PadArrival {
            origin,
            stand_height,
            floor,
        })
    }
}

impl Surface<'_> {
    fn pad_floor(self, start: Vec3, radius: f32) -> Option<f32> {
        let reach = start.y - self.minimum_y() + radius;
        if !reach.is_finite() || reach <= 0.0 {
            return None;
        }
        let begin = self
            .pose
            .inverse_transform_point(Vector::from_array(start.to_array()));
        let down = self.pose.rotation.inverse() * -Vector::Y;
        let end = begin + down * reach;
        let margin = Vector::splat(radius);
        let bounds = Aabb::new(begin.min(end) - margin, begin.max(end) + margin);
        let ball = Ball::new(radius);
        let pose = Pose::translation(begin.x, begin.y, begin.z);
        let options = ShapeCastOptions {
            max_time_of_impact: reach,
            target_distance: 0.0,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let hit = self
            .mesh
            .bvh()
            .intersect_aabb(&bounds)
            .filter_map(|index| {
                let face = self.mesh.triangle(index);
                // The authored winding is authoritative; a returned contact
                // normal may have been turned toward the sphere. A two-sided
                // image does not make the underside a floor (read6.651).
                if face.scaled_normal().dot(-down) < 0.0 {
                    return None;
                }
                cast_shapes(
                    &pose,
                    down,
                    &ball,
                    &Pose::IDENTITY,
                    Vector::ZERO,
                    &face,
                    options,
                )
                .ok()
                .flatten()
                .map(|hit| hit.time_of_impact)
            })
            .min_by(f32::total_cmp)?;
        Some(start.y - hit - radius + 0.001)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "exactly retained or assigned pad state")]
mod tests {
    use super::*;
    use crate::movement::{CollisionInstance, CollisionMesh, MotionStep, PROVISIONAL_PHYSICS};
    use glam::Quat;
    use std::sync::Arc;

    fn floor(height: f32) -> [[[f32; 3]; 3]; 2] {
        [
            [
                [-20.0, height, -20.0],
                [-20.0, height, 20.0],
                [20.0, height, 20.0],
            ],
            [
                [-20.0, height, -20.0],
                [20.0, height, 20.0],
                [20.0, height, -20.0],
            ],
        ]
    }

    #[test]
    fn erudnext_pad_keeps_origin_then_raises_once_above_ordinary_climb() {
        let world = CollisionWorld::new(floor(38.0)).unwrap();
        let origin = Vec3::new(-8.0, 38.0, -4.0);
        // The old ordinary-floor query cannot reach the authored floor
        // from these soles; the separate arrival probe closes that gap.
        assert!(world.ground(origin - Vec3::Y * 3.75, 3.375, 0.0).is_none());
        let mut body = AirborneController::default();
        body.carry_from_water(-12.0, Vec3::new(3.0, 0.0, 4.0), 70.0);
        let arrival = world.pad_arrival(origin, 3.75).unwrap();
        assert_eq!(arrival.origin(), origin);
        assert!((arrival.floor().unwrap() - 38.001).abs() < 0.002);
        assert_eq!(body.velocity(), -12.0);
        let landed = arrival.first_step(&mut body);
        assert!((landed.y - 41.751).abs() < 0.002);
        assert!(landed.y - origin.y > 3.375);
        assert_eq!(body.velocity(), 0.0);
        assert_eq!(body.drift(), Vec3::new(3.0, 0.0, 4.0));
        assert_eq!(body.peak(), None);
        let landing = body.take_landing().unwrap();
        assert_eq!(landing.impact_speed, 12.0);
        assert!(body.take_landing().is_none());
    }

    #[test]
    fn pad_above_floor_keeps_velocity_and_falls_normally() {
        let world = CollisionWorld::new(floor(38.0)).unwrap();
        let origin = Vec3::new(0.0, 50.0, 0.0);
        let mut body = AirborneController::default();
        body.carry_from_water(-12.0, Vec3::X * 3.0, 46.25);
        let arrival = world.pad_arrival(origin, 3.75).unwrap();
        assert_eq!(arrival.origin(), origin);
        assert!((arrival.floor().unwrap() - 38.001).abs() < 0.002);
        assert_eq!(arrival.first_step(&mut body), origin);
        assert_eq!(body.velocity(), -12.0);
        let next = body.step(
            &world,
            origin - Vec3::Y * 3.75,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal: Vec3::ZERO,
                jump: false,
                seconds: 0.05,
                height: 6.0,
            },
        );
        assert!(next.y < 46.25);
        assert!(body.velocity() < -12.0);
    }

    #[test]
    fn pad_floor_misses_undersides_and_below_surface_starts() {
        let underside = floor(38.0).map(|[a, b, c]| [a, c, b]);
        let world = CollisionWorld::new(underside).unwrap();
        assert!(
            world
                .pad_arrival(Vec3::Y * 38.0, 3.75)
                .unwrap()
                .floor()
                .is_none()
        );
        let world = CollisionWorld::new(floor(38.0)).unwrap();
        assert!(
            world
                .pad_arrival(Vec3::Y * 35.0, 3.75)
                .unwrap()
                .floor()
                .is_none()
        );
        assert!(world.pad_arrival(Vec3::NAN, 3.75).is_none());
        assert!(world.pad_arrival(Vec3::ZERO, 0.0).is_none());
    }

    #[test]
    fn pad_floor_uses_sphere_footprint_and_rigid_placement() {
        let mut world = CollisionWorld::new(floor(-100.0)).unwrap();
        let mesh = Arc::new(CollisionMesh::new(floor(0.0)).unwrap());
        world.set_spawn_instances(vec![(
            7,
            CollisionInstance::new(mesh, Vec3::Y * 38.0, Quat::IDENTITY).unwrap(),
        )]);
        // Centre is outside the square, while the sphere reaches its front edge.
        let hit = world.pad_arrival(Vec3::new(20.2, 38.0, 0.0), 3.75).unwrap();
        assert!(hit.floor().unwrap() > 37.8);
        assert!(hit.floor().unwrap() < 38.01);
    }

    #[test]
    fn pad_consume_is_strict_and_preserves_momentum_until_a_snap() {
        for velocity in [-12.0, 0.0, 8.0] {
            for floor in [None, Some(34.25), Some(38.0)] {
                let mut body = AirborneController::default();
                body.carry_from_water(velocity, Vec3::Z * 4.0, 70.0);
                let arrival = PadArrival {
                    origin: Vec3::Y * 38.0,
                    stand_height: 3.75,
                    floor,
                };
                let next = arrival.first_step(&mut body);
                let snapped = floor == Some(38.0);
                assert_eq!(next.y, if snapped { 41.75 } else { 38.0 });
                assert_eq!(body.velocity(), if snapped { 0.0 } else { velocity });
                assert_eq!(body.drift(), Vec3::Z * 4.0);
                assert_eq!(body.peak(), if snapped { None } else { Some(70.0) });
            }
        }
    }
}

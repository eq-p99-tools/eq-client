//! Voluntary flight still collides with the character's solid world.

use super::{CAPSULE_RADIUS, CollisionWorld, LIFT, SKIN, body};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{
        PointQuery, PointQueryWithLocation, Ray, RayCast, ShapeCastHit, ShapeCastOptions,
        ShapeCastStatus, Unsupported, cast_shapes,
    },
    shape::{Shape, Triangle, TrianglePointLocation},
};
use std::cell::RefCell;

impl CollisionWorld {
    /// Sweeps a flying character through solid geometry without requiring ground
    /// support. Horizontal flight follows rising walkable ground while walls and
    /// ceilings still stop the body. Liquids are
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
        let collision = self.flight_contact(feet, displacement, height);
        self.observe_sweep(&collision);
        let (hit, slope) = match collision {
            Ok(Some(contact)) => contact,
            Ok(None) => return feet + displacement,
            Err(_) => return feet,
        };
        let travel = clear_fraction(&hit, displacement);
        let contact = feet + displacement * travel;
        let Some(normal) = slope else { return contact };
        let remaining = displacement * (1.0 - travel);
        // Spend only the remaining travel on the rising face. A separate
        // sweep still clips that motion against ceilings and other solids.
        let along = remaining - normal * remaining.dot(normal);
        if displacement.y < 0.0
            || along.y <= 0.0
            || along.y <= remaining.y
            || along.length_squared() < 1e-6
        {
            return contact;
        }
        match self.flight_sweep(contact, along, height) {
            Ok(None) => contact + along,
            Ok(Some(next)) => contact + along * clear_fraction(&next, along),
            Err(_) => contact,
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
        let result = self
            .flight_contact(feet, delta, height)
            .map(|contact| contact.map(|(hit, _)| hit));
        self.observe_sweep(&result);
        result
    }

    fn flight_contact(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Option<(ShapeCastHit, Option<Vec3>)>, Unsupported> {
        let (pose, capsule) = body(feet - Vec3::Y * LIFT, height);
        let bounds = capsule.compute_aabb(&pose);
        let shift = -bounds.center();
        let margin = bounds.half_extents() + Vector::splat(SKIN);
        let velocity = Vector::from_array(delta.to_array());
        let ray = Ray::new(Vector::ZERO, velocity);
        let mut nearest: Option<(ShapeCastHit, Option<Vec3>)> = None;
        for mesh in self.wall_meshes() {
            let unsupported = RefCell::new(None);
            let hit = mesh.mesh.bvh().find_best(
                1.0,
                |node, best| {
                    let node = Aabb {
                        mins: node.mins(),
                        maxs: node.maxs(),
                    }
                    .transform_by(&mesh.pose);
                    Aabb {
                        mins: node.mins + shift - margin,
                        maxs: node.maxs + shift + margin,
                    }
                    .cast_local_ray(&ray, best, true)
                    .unwrap_or(f32::MAX)
                },
                |index, best| {
                    let triangle = mesh.mesh.triangle(index).transformed(&mesh.pose);
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
                    if nonclosing_cap(&triangle, &pose, velocity, height) {
                        return None;
                    }
                    match face_sweep(&triangle, &pose, velocity, height, best) {
                        FaceSweep::Clear => return None,
                        FaceSweep::Hit(hit) => return Some(hit),
                        FaceSweep::Cast => {}
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
            );
            if let Some(error) = unsupported.into_inner() {
                return Err(error);
            }
            if let Some((index, hit)) = hit
                && nearest
                    .as_ref()
                    .is_none_or(|(previous, _)| hit.time_of_impact < previous.time_of_impact)
            {
                // An edge of a vertical wall can have an upward capsule normal.
                // Only an actual walkable triangle face can lift a flyer.
                let slope = mesh
                    .mesh
                    .triangle(index)
                    .transformed(&mesh.pose)
                    .normal()
                    .map(|n| if n.y < 0.0 { -n } else { n })
                    .filter(|n| n.y >= 0.7 && n.dot(hit.normal2) > 0.999)
                    .map(|n| Vec3::from_array(n.to_array()));
                nearest = Some((hit, slope));
            }
        }
        Ok(nearest)
    }
}

fn clear_fraction(hit: &ShapeCastHit, delta: Vec3) -> f32 {
    (hit.time_of_impact - 0.005 / delta.length()).clamp(0.0, 1.0)
}

/// At a capsule end, a triangle's exact closest point also gives the whole
/// capsule's separation when that point is beyond the end of its spine.
/// Retire only an existing contact moving tangent/away. This covers the
/// adjacent triangle at a floor seam without skipping an approaching edge.
fn nonclosing_cap(triangle: &Triangle, pose: &Pose, velocity: Vector, height: f32) -> bool {
    let roundoff = height * f32::EPSILON * 4.0;
    for end in [-1.0, 1.0] {
        let center = pose.translation + Vector::Y * ((height * 0.5 - CAPSULE_RADIUS) * end);
        let relative = Triangle::new(
            triangle.a - center,
            triangle.b - center,
            triangle.c - center,
        );
        let separation = -relative.project_local_point(Vector::ZERO, false).point;
        let distance = separation.length();
        if separation.y * end <= 0.0
            && distance > roundoff
            && distance <= CAPSULE_RADIUS + SKIN
            && velocity.dot(separation) >= -roundoff * distance
        {
            return true;
        }
    }
    false
}

enum FaceSweep {
    Cast,
    Clear,
    Hit(ShapeCastHit),
}

/// An interior face contact has an exact plane normal. GJK at a touching
/// triangle can tilt that normal, so the projected swim slide closes again
/// and stops at time zero. Keep the ordinary cast for edge/vertex approaches.
fn face_sweep(
    triangle: &Triangle,
    pose: &Pose,
    velocity: Vector,
    height: f32,
    best: f32,
) -> FaceSweep {
    let triangle = Triangle::new(
        triangle.a - pose.translation,
        triangle.b - pose.translation,
        triangle.c - pose.translation,
    );
    let Some(mut face) = triangle.normal() else {
        return FaceSweep::Cast;
    };
    if (-triangle.a).dot(face) < 0.0 {
        face = -face;
    }
    let support =
        -face * CAPSULE_RADIUS - Vector::Y * (height * 0.5 - CAPSULE_RADIUS) * face.y.signum();
    let start = support;
    let gap = (start - triangle.a).dot(face);
    let closing = velocity.dot(face);
    let roundoff = height * f32::EPSILON * 4.0;
    let time = if closing >= -roundoff {
        0.0
    } else {
        ((gap - SKIN) / -closing).max(0.0)
    };
    if time > best {
        return FaceSweep::Clear;
    }
    let witness = start + velocity * time - face * (gap + closing * time);
    let (projection, location) = triangle.project_local_point_and_get_location(witness, false);
    if !matches!(location, TrianglePointLocation::OnFace(..))
        && (projection.point - witness).length_squared() > roundoff * roundoff
    {
        return FaceSweep::Cast;
    }
    // The current capsule already reaches this face. Moving tangentially or
    // away cannot deepen that contact; a different face/edge is still swept.
    if closing >= -roundoff {
        return FaceSweep::Clear;
    }
    FaceSweep::Hit(ShapeCastHit {
        time_of_impact: time,
        witness1: support,
        witness2: witness + pose.translation,
        normal1: -face,
        normal2: face,
        status: if time == 0.0 {
            ShapeCastStatus::PenetratingOrWithinTargetDist
        } else {
            ShapeCastStatus::Converged
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> CollisionWorld {
        CollisionWorld::new([
            [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [20.0, 0.0, -20.0]],
            [[-20.0, 0.0, -20.0], [-20.0, 0.0, 20.0], [20.0, 0.0, 20.0]],
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
            [[3.0, 0.0, -20.0], [13.0, 10.0, 20.0], [13.0, 10.0, -20.0]],
            [[3.0, 0.0, -20.0], [3.0, 0.0, 20.0], [13.0, 10.0, 20.0]],
        ])
        .unwrap();
        let climbed = slope.fly_step(feet, Vec3::X * 10.0, 6.0);
        assert!(climbed.x > 6.0 && climbed.y > feet.y + 1.0, "{climbed:?}");
        assert!(climbed.distance(feet) <= 10.001);
        let farther = slope.fly_step(climbed, Vec3::X, 6.0);
        assert!(
            farther.x > climbed.x && farther.y > climbed.y,
            "{farther:?}"
        );
        let away = slope.fly_step(farther, Vec3::NEG_X * 4.0, 6.0);
        assert!((away.y - farther.y).abs() < 0.001 && away.x < farther.x - 3.99);
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
    fn uphill_flight_stops_below_a_ceiling_and_cannot_climb_a_steep_face() {
        for (rise, ceiling) in [(6.0, true), (24.0, false)] {
            let mut triangles = vec![
                [[0., 0., -10.], [12., rise, 10.], [12., rise, -10.]],
                [[0., 0., -10.], [0., 0., 10.], [12., rise, 10.]],
            ];
            if ceiling {
                triangles.extend([
                    [[-10., 8.5, -10.], [15., 8.5, -10.], [15., 8.5, 10.]],
                    [[-10., 8.5, -10.], [15., 8.5, 10.], [-10., 8.5, 10.]],
                ]);
            }
            let world = CollisionWorld::new(triangles).unwrap();
            let mut feet = Vec3::new(-1., 0., 1.);
            for _ in 0..100 {
                feet = world.fly_step(feet, Vec3::X * 0.2, 6.);
            }
            if ceiling {
                assert!(feet.x > 2. && feet.y > 1. && feet.y <= 2.51, "{feet:?}");
            } else {
                assert!(feet.x < 1. && feet.y.abs() < 0.001, "{feet:?}");
            }
        }
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
                [10.0, -12.0, 10.0],
                [10.0, -12.0, -10.0],
            ],
            [
                [-10.0, -12.0, -10.0],
                [-10.0, -12.0, 10.0],
                [10.0, -12.0, 10.0],
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

//! The distinct floor and upward headroom probes. Walking's body shape is
//! unchanged. A capsule here is the union of the same probe spheres along a
//! coalesced host stride, so a thin underside cannot fall between samples.
use super::{CollisionWorld, Obstacle, Surface};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{ShapeCastOptions, cast_shapes},
    shape::{Ball, Capsule},
};

const REACH: f32 = 32_767.0;

/// Probe dimensions, separate from walking reach and collision dimensions.
#[derive(Clone, Copy, Debug)]
pub struct HeadroomProbe {
    /// Titanium: .1S; Mac's floor primitive is still approximated by a ray.
    pub floor_radius: f32,
    /// Titanium: actor-model radius (currently estimated); Mac: .5.
    pub ceiling_radius: f32,
    /// Titanium's returned floor is sphere bottom + .001; Mac has no bias.
    pub floor_bias: f32,
    /// Native Mac starts its floor probe .5 below the supplied origin.
    pub floor_offset: f32,
    /// Titanium's explicit no-floor zone exceptions 0x47, 0x4b, 0x96.
    pub permits_missing_floor: bool,
    /// Only Titanium passes a missing ceiling; Mac refuses either sentinel.
    pub permits_missing_ceiling: bool,
}

impl HeadroomProbe {
    /// Apply this generation's missing-probe policy and strict comparisons.
    #[must_use]
    pub fn fits(self, bounds: HeadroomBounds, need: f32, margin: f32) -> bool {
        (self.permits_missing_ceiling || bounds.ceiling.is_some())
            && bounds.fits(need, margin, self.permits_missing_floor)
    }
}

/// Missing floor and ceiling have different native outcomes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadroomBounds {
    /// The downward probe's returned floor.
    pub floor: Option<f32>,
    /// The upward probe's returned sphere top.
    pub ceiling: Option<f32>,
}

impl HeadroomBounds {
    /// Titanium 0x48bdb0's strict comparison, then 0x48be3a's final check.
    /// The latter rejects unusable bounds even though an earlier branch
    /// accepts ceiling <= floor. Only a missing ceiling bypasses that check.
    #[must_use]
    pub fn fits(self, need: f32, margin: f32, permits_missing_floor: bool) -> bool {
        let Some(floor) = self.floor else {
            return permits_missing_floor;
        };
        let Some(ceiling) = self.ceiling else {
            return true;
        };
        (ceiling <= floor || need + floor + margin < ceiling) && ceiling - floor >= need
    }
}

/// A probe on the stride with insufficient room, for diagnostic readback.
#[derive(Clone, Copy, Debug)]
pub struct HeadroomHit {
    /// Sphere centre before the upward cast, on the supplied origin segment.
    pub origin: Vec3,
    /// Returned probe values, with missing surfaces preserved.
    pub bounds: HeadroomBounds,
}

impl CollisionWorld {
    /// Native-shaped probes at an ordinary posture or lifted movement origin.
    #[must_use]
    pub fn probe_headroom(&self, origin: Vec3, probe: HeadroomProbe) -> HeadroomBounds {
        let floor = self.headroom_floor(origin, probe);
        let ceiling = self
            .headroom_surfaces()
            .flat_map(|surface| ceiling_contacts(surface, origin, origin, probe.ceiling_radius))
            .map(|(_, top)| top)
            .min_by(f32::total_cmp);
        HeadroomBounds { floor, ceiling }
    }

    fn headroom_floor(&self, origin: Vec3, probe: HeadroomProbe) -> Option<f32> {
        let floor_origin = origin + Vec3::Y * probe.floor_offset;
        if probe.floor_radius > 0.0 {
            self.floor_meshes()
                .filter_map(|surface| sphere_floor(surface, floor_origin, probe.floor_radius))
                .max_by(f32::total_cmp)
                .map(|bottom| bottom + probe.floor_bias)
        } else {
            self.solid_ground(floor_origin, 0.0, floor_origin.y + REACH)
        }
    }

    /// Test the complete resolved stride without enlarging a probe or adding
    /// a tolerance. Native itself probes a candidate sphere; sweeping its
    /// union here accounts for this host's coalesced movement samples.
    #[must_use]
    pub fn headroom_on_path(
        &self,
        from: Vec3,
        to: Vec3,
        probe: HeadroomProbe,
        need: f32,
        margin: f32,
    ) -> Option<HeadroomHit> {
        let fits = |bounds: HeadroomBounds| probe.fits(bounds, need, margin);
        let bounds = self.probe_headroom(to, probe);
        if !fits(bounds) {
            return Some(HeadroomHit { origin: to, bounds });
        }
        // Query each underside separately, pairing it with its own local
        // floor. Combining a high floor and low roof from unrelated parts
        // of the stride would invent a refusal.
        self.headroom_surfaces()
            .flat_map(|surface| ceiling_contacts(surface, from, to, probe.ceiling_radius))
            .filter_map(|(origin, ceiling)| {
                let bounds = HeadroomBounds {
                    ceiling: Some(ceiling),
                    floor: self.headroom_floor(origin, probe),
                };
                (!fits(bounds)).then_some(HeadroomHit { origin, bounds })
            })
            .min_by(|a, b| {
                from.distance_squared(a.origin)
                    .total_cmp(&from.distance_squared(b.origin))
            })
    }

    fn headroom_surfaces(&self) -> impl Iterator<Item = Surface<'_>> {
        self.labelled_wall_meshes()
            .filter(|(obstacle, _)| !matches!(obstacle, Obstacle::Door(_)))
            .map(|(_, surface)| surface)
    }
}

fn options(reach: f32) -> ShapeCastOptions {
    ShapeCastOptions {
        max_time_of_impact: reach,
        target_distance: 0.0,
        stop_at_penetration: false,
        compute_impact_geometry_on_penetration: true,
    }
}

fn sphere_floor(surface: Surface<'_>, origin: Vec3, radius: f32) -> Option<f32> {
    let begin = surface
        .pose
        .inverse_transform_point(Vector::from_array(origin.to_array()));
    let down = surface.pose.rotation.inverse() * -Vector::Y;
    let reach = (origin.y + REACH).max(0.0);
    let end = begin + down * reach;
    let bounds = Aabb::new(
        begin.min(end) - Vector::splat(radius),
        begin.max(end) + Vector::splat(radius),
    );
    let pose = Pose::translation(begin.x, begin.y, begin.z);
    surface
        .mesh
        .bvh()
        .intersect_aabb(&bounds)
        .filter_map(|index| {
            let triangle = surface.mesh.triangle(index);
            if triangle.scaled_normal().dot(down) >= 0.0 {
                return None;
            }
            cast_shapes(
                &pose,
                down,
                &Ball::new(radius),
                &Pose::IDENTITY,
                Vector::ZERO,
                &triangle,
                options(reach),
            )
            .ok()
            .flatten()
            .map(|hit| origin.y - hit.time_of_impact - radius)
        })
        .max_by(f32::total_cmp)
}

fn ceiling_contacts(surface: Surface<'_>, from: Vec3, to: Vec3, radius: f32) -> Vec<(Vec3, f32)> {
    let begin = surface
        .pose
        .inverse_transform_point(Vector::from_array(from.to_array()));
    let end = surface
        .pose
        .inverse_transform_point(Vector::from_array(to.to_array()));
    let up = surface.pose.rotation.inverse() * Vector::Y;
    let reach = (REACH - from.y.min(to.y)).max(0.0);
    let high = up * reach;
    let bounds = Aabb::new(
        begin.min(end).min(begin + high).min(end + high) - Vector::splat(radius),
        begin.max(end).max(begin + high).max(end + high) + Vector::splat(radius),
    );
    let segment = end - begin;
    let shape = Capsule::new(begin, end, radius);
    surface
        .mesh
        .bvh()
        .intersect_aabb(&bounds)
        .filter_map(|index| {
            let triangle = surface.mesh.triangle(index);
            if triangle.scaled_normal().dot(up) >= 0.0 {
                return None;
            }
            let hit = if begin == end {
                cast_shapes(
                    &Pose::translation(begin.x, begin.y, begin.z),
                    up,
                    &Ball::new(radius),
                    &Pose::IDENTITY,
                    Vector::ZERO,
                    &triangle,
                    options(reach),
                )
            } else {
                cast_shapes(
                    &Pose::IDENTITY,
                    up,
                    &shape,
                    &Pose::IDENTITY,
                    Vector::ZERO,
                    &triangle,
                    options(reach),
                )
            }
            .ok()
            .flatten()?;
            // A capsule witness is expressed in its unadvanced local frame.
            // Subtract its radius to recover the sphere centre on its axis.
            let centre = if begin == end {
                begin
            } else {
                let axis_point = hit.witness1 - hit.normal1 * radius;
                begin
                    + segment
                        * ((axis_point - begin).dot(segment) / segment.length_squared())
                            .clamp(0.0, 1.0)
            };
            let origin = Vec3::from_array(surface.pose.transform_point(centre).to_array());
            (origin.y + hit.time_of_impact <= REACH)
                .then_some((origin, origin.y + hit.time_of_impact + radius))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> HeadroomProbe {
        HeadroomProbe {
            floor_radius: 0.375,
            ceiling_radius: 0.65,
            floor_bias: 0.001,
            floor_offset: 0.0,
            permits_missing_floor: false,
            permits_missing_ceiling: true,
        }
    }

    fn plane(y: f32, up: bool) -> Vec<[[f32; 3]; 3]> {
        let mut faces = vec![
            [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
            [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
        ];
        if !up {
            for face in &mut faces {
                face.swap(1, 2);
            }
        }
        faces
    }

    #[test]
    fn headroom_sphere_probes_keep_bias_fronts_carriers_and_door_filter() {
        use super::super::{CollisionInstance, CollisionMesh};
        use std::sync::Arc;
        let mut world = CollisionWorld::new(
            plane(0.0, true)
                .into_iter()
                .chain(plane(1.0, false))
                .chain(plane(5.0, true)),
        )
        .unwrap();
        let roof = Arc::new(CollisionMesh::new(plane(0.0, true)).unwrap());
        world.set_spawn_instances(vec![(
            50,
            CollisionInstance::new(
                roof,
                Vec3::Y * 8.0,
                glam::Quat::from_rotation_x(std::f32::consts::PI),
            )
            .unwrap()
            .with_spawn_kind(crate::SpawnKind::Npc),
        )]);
        let bounds = world
            .with_titanium_walking(3.75)
            .probe_headroom(Vec3::Y * 3.75, probe());
        assert!((bounds.floor.unwrap() - 0.001).abs() < 0.0001, "{bounds:?}");
        assert!((bounds.ceiling.unwrap() - 8.0).abs() < 0.0001, "{bounds:?}");
        assert!(!probe().fits(bounds, 7.5, 0.5));
        let mut world = CollisionWorld::new(plane(0.0, true)).unwrap();
        world.set_door_obstacles(vec![(
            7,
            Arc::new(CollisionMesh::new(plane(8.0, false)).unwrap()),
        )]);
        let sky = world.probe_headroom(Vec3::Y * 3.75, probe());
        assert!(sky.ceiling.is_none());
        assert!(probe().fits(sky, 7.5, 0.5));
        assert!(
            !HeadroomProbe {
                permits_missing_ceiling: false,
                ..probe()
            }
            .fits(sky, 7.5, 0.5)
        );
        let missing_floor = CollisionWorld::new(plane(8.0, false))
            .unwrap()
            .probe_headroom(Vec3::Y * 3.75, probe());
        assert!(!probe().fits(missing_floor, 7.5, 0.5));
    }

    #[test]
    fn headroom_stride_pairs_each_underside_with_its_local_floor() {
        let mut faces = plane(0.0, true);
        faces.extend(plane(20.0, false));
        faces.extend([
            [[-1.0, 10.0, -20.0], [1.0, 10.0, 20.0], [-1.0, 10.0, 20.0]],
            [[-1.0, 10.0, -20.0], [1.0, 10.0, -20.0], [1.0, 10.0, 20.0]],
            [[9.0, 4.0, -20.0], [9.0, 4.0, 20.0], [20.0, 4.0, 20.0]],
            [[9.0, 4.0, -20.0], [20.0, 4.0, 20.0], [20.0, 4.0, -20.0]],
        ]);
        let world = CollisionWorld::new(faces).unwrap();
        assert!(
            world
                .headroom_on_path(
                    Vec3::new(-5.0, 3.75, 0.0),
                    Vec3::new(12.0, 7.75, 0.0),
                    probe(),
                    7.5,
                    0.5
                )
                .is_none()
        );
    }

    #[test]
    fn headroom_native_final_check_and_missing_surface_cases() {
        let bounds = |floor, ceiling| HeadroomBounds { floor, ceiling };
        assert!(!bounds(Some(4.0), Some(12.0)).fits(7.5, 0.5, false));
        assert!(bounds(Some(4.0), Some(12.0)).fits(4.75, 0.5, false));
        for ceiling in [3.0, 4.0] {
            assert!(!bounds(Some(4.0), Some(ceiling)).fits(7.5, 0.5, false));
        }
        assert!(bounds(Some(4.0), None).fits(7.5, 0.5, false));
        assert!(!bounds(None, None).fits(7.5, 0.5, false));
        assert!(!bounds(None, Some(12.0)).fits(7.5, 0.5, false));
        assert!(bounds(None, Some(12.0)).fits(7.5, 0.5, true));
    }
}

//! The distinct floor and upward headroom probes. Walking's body shape is
//! unchanged. A capsule here is the union of the same probe spheres along a
//! coalesced host stride, so a thin underside cannot fall between samples.
use super::{CollisionWorld, Obstacle, Surface};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{PointQueryWithLocation, ShapeCastOptions, Unsupported, cast_shapes},
    shape::{Ball, Capsule, Triangle, TrianglePointLocation},
};

const REACH: f32 = 32_767.0;

/// Probe dimensions, separate from walking reach and collision dimensions.
#[derive(Clone, Copy, Debug)]
pub struct HeadroomProbe {
    /// Titanium: .1S; Mac: .25S (both use .1 when that product is zero).
    pub floor_radius: f32,
    /// Titanium: actor-model radius (currently estimated); Mac: .5.
    pub ceiling_radius: f32,
    /// Both helpers return sphere bottom + .001.
    pub floor_bias: f32,
    /// Effective floor start offset. Mac's caller -.5 and helper +.5 cancel.
    pub floor_offset: f32,
    /// Read graphics position retreat along a cast: Titanium .001, Mac zero.
    /// This is separate from floor bias and the strict room margin.
    pub retreat: f32,
    /// Titanium's explicit no-floor zone exceptions 0x47, 0x4b, 0x96.
    pub permits_missing_floor: bool,
    /// Only Titanium passes a missing ceiling; Mac refuses either sentinel.
    pub permits_missing_ceiling: bool,
}

impl HeadroomProbe {
    fn valid(self) -> bool {
        self.floor_radius.is_finite()
            && self.floor_radius > 0.0
            && self.ceiling_radius.is_finite()
            && self.ceiling_radius > 0.0
            && self.floor_bias.is_finite()
            && self.floor_offset.is_finite()
            && self.retreat.is_finite()
            && self.retreat >= 0.0
    }
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
    /// Downward query scalar, including the end bound of a successful clear
    /// cast. This is not evidence of physical support. None means failure.
    pub floor: Option<f32>,
    /// Upward query's returned sphere top, including a clear cast's end bound.
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
        if !origin.is_finite() || !probe.valid() {
            return HeadroomBounds {
                floor: None,
                ceiling: None,
            };
        }
        let floor = self.headroom_floor(origin, probe);
        let ceiling = self
            .headroom_ceiling_contacts(origin, origin, probe)
            .ok()
            .and_then(|contacts| {
                contacts
                    .into_iter()
                    .map(|(_, top)| top)
                    .min_by(f32::total_cmp)
                    .or_else(|| {
                        // A successful unobstructed native cast reaches its
                        // destination. It is not the query-failure sentinel.
                        (origin.y <= REACH).then(|| {
                            REACH - probe.retreat.min(REACH - origin.y) + probe.ceiling_radius
                        })
                    })
            });
        HeadroomBounds { floor, ceiling }
    }

    fn headroom_floor(&self, origin: Vec3, probe: HeadroomProbe) -> Option<f32> {
        let floor_origin = origin + Vec3::Y * probe.floor_offset;
        let mut floor: Option<f32> = None;
        for surface in self.floor_meshes() {
            if let Some(bottom) =
                sphere_floor(surface, floor_origin, probe.floor_radius, probe.retreat).ok()?
            {
                floor = Some(floor.map_or(bottom, |old| old.max(bottom)));
            }
        }
        floor
            .or_else(|| {
                (floor_origin.y >= -REACH).then(|| {
                    -REACH + probe.retreat.min(floor_origin.y + REACH) - probe.floor_radius
                })
            })
            .map(|bottom| bottom + probe.floor_bias)
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
        if !from.is_finite() || !to.is_finite() || !probe.valid() {
            return Some(HeadroomHit {
                origin: to,
                bounds: HeadroomBounds {
                    floor: None,
                    ceiling: None,
                },
            });
        }
        let fits = |bounds: HeadroomBounds| probe.fits(bounds, need, margin);
        let bounds = self.probe_headroom(to, probe);
        if !fits(bounds) {
            return Some(HeadroomHit { origin: to, bounds });
        }
        // Query each underside separately, pairing it with its own local
        // floor. Combining a high floor and low roof from unrelated parts
        // of the stride would invent a refusal.
        let Ok(contacts) = self.headroom_ceiling_contacts(from, to, probe) else {
            let bounds = HeadroomBounds {
                ceiling: None,
                ..bounds
            };
            return (!fits(bounds)).then_some(HeadroomHit { origin: to, bounds });
        };
        contacts
            .into_iter()
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

    fn headroom_ceiling_contacts(
        &self,
        from: Vec3,
        to: Vec3,
        probe: HeadroomProbe,
    ) -> Result<Vec<(Vec3, f32)>, Unsupported> {
        let mut contacts = Vec::new();
        for surface in self.headroom_surfaces() {
            contacts.extend(ceiling_contacts(
                surface,
                from,
                to,
                probe.ceiling_radius,
                probe.retreat,
            )?);
        }
        Ok(contacts)
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

fn sphere_floor(
    surface: Surface<'_>,
    origin: Vec3,
    radius: f32,
    retreat: f32,
) -> Result<Option<f32>, Unsupported> {
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
    let mut floor: Option<f32> = None;
    for index in surface.mesh.bvh().intersect_aabb(&bounds) {
        let triangle = surface.mesh.triangle(index);
        if triangle.scaled_normal().dot(down) > 0.0 {
            continue;
        }
        if let Some(hit) = cast_shapes(
            &pose,
            down,
            &Ball::new(radius),
            &Pose::IDENTITY,
            Vector::ZERO,
            &triangle,
            options(reach),
        )? {
            let bottom = face_height(surface, &triangle, begin, down, radius, false)
                .unwrap_or(origin.y - hit.time_of_impact - radius);
            let bottom = bottom + retreat.min((origin.y - radius - bottom).max(0.0));
            floor = Some(floor.map_or(bottom, |old| old.max(bottom)));
        }
    }
    Ok(floor)
}

fn ceiling_contacts(
    surface: Surface<'_>,
    from: Vec3,
    to: Vec3,
    radius: f32,
    retreat: f32,
) -> Result<Vec<(Vec3, f32)>, Unsupported> {
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
    let mut contacts = Vec::new();
    for index in surface.mesh.bvh().intersect_aabb(&bounds) {
        let triangle = surface.mesh.triangle(index);
        if triangle.scaled_normal().dot(up) > 0.0 {
            continue;
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
        }?;
        let Some(hit) = hit else { continue };
        // A capsule witness is expressed in its unadvanced local frame.
        // Subtract its radius to recover the sphere centre on its axis.
        let centre = if begin == end {
            begin
        } else {
            let axis_point = hit.witness1 - hit.normal1 * radius;
            begin
                + segment
                    * ((axis_point - begin).dot(segment) / segment.length_squared()).clamp(0.0, 1.0)
        };
        let origin = Vec3::from_array(surface.pose.transform_point(centre).to_array());
        let top = face_height(surface, &triangle, centre, up, radius, true)
            .unwrap_or(origin.y + hit.time_of_impact + radius);
        let top = top - retreat.min((top - origin.y - radius).max(0.0));
        if top >= origin.y + radius && top - radius <= REACH {
            contacts.push((origin, top));
        }
    }
    Ok(contacts)
}

/// For a face-interior contact the returned sphere top/bottom is exact plane
/// geometry: contact + normal*r +/- r. Recover it analytically rather than
/// accumulating the generic GJK time error into the strict room comparison.
/// Edges, nonclosing faces and initial penetration retain the cast result;
/// there is no rounding tolerance or change to the native comparison.
fn face_height(
    surface: Surface<'_>,
    triangle: &Triangle,
    centre: Vector,
    direction: Vector,
    radius: f32,
    ceiling: bool,
) -> Option<f32> {
    let normal = triangle.normal()?;
    let closing = normal.dot(direction);
    if closing >= 0.0 {
        return None;
    }
    let time = (radius - (centre - triangle.a).dot(normal)) / closing;
    if time < 0.0 {
        return None;
    }
    let (projection, location) =
        triangle.project_local_point_and_get_location(centre + direction * time, false);
    if !matches!(location, TrianglePointLocation::OnFace(..)) {
        return None;
    }
    let point = surface.pose.transform_point(projection.point);
    let normal = surface.pose.rotation * normal;
    Some(point.y + radius * (normal.y + if ceiling { 1.0 } else { -1.0 }))
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
            retreat: 0.0, // geometric control, independent of generation mapping
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
        assert_eq!(sky.ceiling, Some(REACH + probe().ceiling_radius));
        assert!(probe().fits(sky, 7.5, 0.5));
        assert!(
            HeadroomProbe {
                permits_missing_ceiling: false,
                ..probe()
            }
            .fits(sky, 7.5, 0.5)
        );
        let clear_down = CollisionWorld::new(plane(8.0, false))
            .unwrap()
            .probe_headroom(Vec3::Y * 3.75, probe());
        assert!(clear_down.floor.unwrap() < -REACH);
        assert!(probe().fits(clear_down, 7.5, 0.5));
        // A failed/invalid query still refuses outside the explicit exceptions.
        let failed = world.probe_headroom(Vec3::splat(f32::NAN), probe());
        assert_eq!(
            failed,
            HeadroomBounds {
                floor: None,
                ceiling: None
            }
        );
        assert!(!probe().fits(failed, 7.5, 0.5));
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

    #[test]
    fn headroom_flat_face_keeps_its_authored_height_without_a_tolerance() {
        let world =
            CollisionWorld::new(plane(0.0, true).into_iter().chain(plane(4.25, false))).unwrap();
        let mac = HeadroomProbe {
            floor_radius: 0.9375,
            ceiling_radius: 0.5,
            floor_offset: 0.0,
            floor_bias: 0.001,
            permits_missing_ceiling: false,
            ..probe()
        };
        let bounds = world.probe_headroom(Vec3::new(1.0, 3.75, 0.0), mac);
        assert_eq!(bounds.ceiling, Some(4.25));
        assert!(!mac.fits(bounds, 3.75, 0.5), "{bounds:?}");
    }
}

//! The distinct floor and upward headroom probes. Walking's body shape is
//! unchanged. A coalesced host stride tests candidate spheres at its mapped
//! program-frame spacing, without filling the gaps between native samples.
use super::{CollisionWorld, Obstacle, Surface};
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{PointQueryWithLocation, ShapeCastOptions, Unsupported, cast_shapes},
    shape::{Ball, Triangle, TrianglePointLocation},
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
            .headroom_ceiling_contacts(origin, probe)
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

    /// Test candidate spheres at `(count, step_fraction)` frame endpoints.
    /// The last is always `to`; a shorter final frame does not rescale the
    /// earlier frame spacing. The caller maps elapsed time to frames. This does not
    /// fill the extra crescent of a continuous capsule between those frames.
    #[must_use]
    pub fn headroom_on_path(
        &self,
        from: Vec3,
        to: Vec3,
        probe: HeadroomProbe,
        need: f32,
        margin: f32,
        (samples, step_fraction): (u8, f32),
    ) -> Option<HeadroomHit> {
        if !from.is_finite()
            || !to.is_finite()
            || !probe.valid()
            || !step_fraction.is_finite()
            || step_fraction <= 0.0
        {
            return Some(HeadroomHit {
                origin: to,
                bounds: HeadroomBounds {
                    floor: None,
                    ceiling: None,
                },
            });
        }
        let samples = samples.max(1);
        for sample in 1..=samples {
            let origin = if sample == samples {
                to
            } else {
                from.lerp(to, (f32::from(sample) * step_fraction).min(1.0))
            };
            // Each candidate keeps its own floor and ceiling. A high floor
            // and low roof from unrelated frames cannot invent a refusal.
            let bounds = self.probe_headroom(origin, probe);
            if !probe.fits(bounds, need, margin) {
                return Some(HeadroomHit { origin, bounds });
            }
        }
        None
    }

    fn headroom_surfaces(&self) -> impl Iterator<Item = Surface<'_>> {
        self.labelled_wall_meshes()
            .filter(|(obstacle, _)| !matches!(obstacle, Obstacle::Door(_)))
            .map(|(_, surface)| surface)
    }

    fn headroom_ceiling_contacts(
        &self,
        origin: Vec3,
        probe: HeadroomProbe,
    ) -> Result<Vec<(Vec3, f32)>, Unsupported> {
        let mut contacts = Vec::new();
        for surface in self.headroom_surfaces() {
            contacts.extend(ceiling_contacts(
                surface,
                origin,
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
        if triangle.scaled_normal().dot(down) >= 0.0 {
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
            // The native moving-sphere test rejects tangent/separating
            // initial contact; overlap alone is not a floor (DX9
            // 0x1019d821..0x1019d8a6). Keep approaching real contacts.
            if hit.normal1.dot(down) <= 0.0 {
                continue;
            }
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
    origin: Vec3,
    radius: f32,
    retreat: f32,
) -> Result<Vec<(Vec3, f32)>, Unsupported> {
    let begin = surface
        .pose
        .inverse_transform_point(Vector::from_array(origin.to_array()));
    let up = surface.pose.rotation.inverse() * Vector::Y;
    let reach = (REACH - origin.y).max(0.0);
    let high = up * reach;
    let bounds = Aabb::new(
        begin.min(begin + high) - Vector::splat(radius),
        begin.max(begin + high) + Vector::splat(radius),
    );
    let mut contacts = Vec::new();
    for index in surface.mesh.bvh().intersect_aabb(&bounds) {
        let triangle = surface.mesh.triangle(index);
        if triangle.scaled_normal().dot(up) >= 0.0 {
            continue;
        }
        let hit = cast_shapes(
            &Pose::translation(begin.x, begin.y, begin.z),
            up,
            &Ball::new(radius),
            &Pose::IDENTITY,
            Vector::ZERO,
            &triangle,
            options(reach),
        )?;
        let Some(hit) = hit else { continue };
        if hit.normal1.dot(up) <= 0.0 {
            continue;
        }
        let top = face_height(surface, &triangle, begin, up, radius, true)
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
                    0.5,
                    (3, 1.0 / 3.0),
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

    fn riser(top: f32) -> Vec<[[f32; 3]; 3]> {
        vec![
            [[0.0, 0.0, -20.0], [0.0, top, -20.0], [0.0, top, 20.0]],
            [[0.0, 0.0, -20.0], [0.0, top, 20.0], [0.0, 0.0, 20.0]],
        ]
    }

    #[test]
    fn headroom_wall_graze_under_open_sky_is_neither_floor_nor_ceiling() {
        let world = CollisionWorld::new(plane(0.0, true).into_iter().chain(riser(30.0))).unwrap();
        for x in [-0.2, 0.2] {
            let bounds = world.probe_headroom(Vec3::new(x, 3.75, 0.0), probe());
            assert!(bounds.floor.unwrap().abs() < 0.01, "{bounds:?}");
            assert!(bounds.ceiling.unwrap() > 1000.0, "{bounds:?}");
            assert!(probe().fits(bounds, 7.5, 0.5));
        }
    }

    #[test]
    fn headroom_ducked_riser_keeps_the_real_underside_and_local_floor() {
        let upper = [
            [[0.0, 4.0, -20.0], [0.0, 4.0, 20.0], [20.0, 4.0, 20.0]],
            [[0.0, 4.0, -20.0], [20.0, 4.0, 20.0], [20.0, 4.0, -20.0]],
        ];
        let world = CollisionWorld::new(
            plane(0.0, true)
                .into_iter()
                .chain(upper)
                .chain(riser(4.0))
                .chain(plane(12.0, false)),
        )
        .unwrap();
        for origin in [Vec3::new(-0.2, 3.75, 0.0), Vec3::new(0.2, 7.75, 0.0)] {
            let bounds = world.probe_headroom(origin, probe());
            assert!(bounds.ceiling.unwrap() > 11.9, "{origin:?}: {bounds:?}");
            assert!(probe().fits(bounds, 4.75, 0.5), "{origin:?}: {bounds:?}");
        }
    }

    #[test]
    fn headroom_initial_contact_with_a_real_underside_still_refuses() {
        let world =
            CollisionWorld::new(plane(0.0, true).into_iter().chain(plane(4.0, false))).unwrap();
        let bounds = world.probe_headroom(Vec3::new(1.0, 3.75, 0.0), probe());
        assert!(bounds.ceiling.unwrap() < 5.0, "{bounds:?}");
        assert!(!probe().fits(bounds, 7.5, 0.5));
    }

    #[test]
    fn headroom_tapered_riser_is_not_an_underside() {
        // A tapered vertical step face, not an underside. This synthetic
        // primitive covers the zero-time ceiling found in the bank replay.
        let world = CollisionWorld::new(
            plane(0.0, true)
                .into_iter()
                .chain(plane(12.0, false))
                .chain([[[-20.0, 3.28125, 0.0], [20.0, 4.0, 0.0], [-20.0, 4.0, 0.0]]]),
        )
        .unwrap();
        let bounds = world.probe_headroom(Vec3::new(5.0, 3.75, 0.61), probe());
        assert!(bounds.ceiling.unwrap() > 11.9, "{bounds:?}");
        assert!(probe().fits(bounds, 4.75, 0.5));
    }

    #[test]
    fn headroom_descent_edge_does_not_invent_a_ceiling_above_the_origin() {
        let world = CollisionWorld::new(
            plane(4.0, true)
                .into_iter()
                .chain(plane(12.0, false))
                .chain(riser(30.0)),
        )
        .unwrap();
        let bounds = world.probe_headroom(Vec3::new(0.5, 7.75, 0.0), probe());
        assert!(bounds.ceiling.unwrap() > 11.9, "{bounds:?}");
        assert!(probe().fits(bounds, 4.75, 0.5));
    }

    #[test]
    fn headroom_ogre_beyond_a_door_does_not_pair_wall_edges_as_floor_and_roof() {
        // A vertical face whose bottom edge is above the lifted candidate.
        // A tangent upward sphere can meet that edge at positive time too.
        let wall = [
            [[0.0, 25.0, -20.0], [0.0, 14.0, 20.0], [0.0, 14.0, -20.0]],
            [[0.0, 14.0, 20.0], [0.0, 25.0, -20.0], [0.0, 25.0, 20.0]],
        ];
        let world = CollisionWorld::new(
            plane(0.0, true)
                .into_iter()
                .chain(plane(25.0, false))
                .chain(wall),
        )
        .unwrap();
        let ogre = HeadroomProbe {
            floor_radius: 0.5625,
            ceiling_radius: 0.975,
            ..probe()
        };
        let bounds = world.probe_headroom(Vec3::new(0.43, 8.4375, 1.0), ogre);
        assert!(bounds.floor.unwrap().abs() < 0.01, "{bounds:?}");
        assert!(bounds.ceiling.unwrap() > 24.9, "{bounds:?}");
        assert!(ogre.fits(bounds, 11.25, 0.5));
    }

    #[test]
    fn headroom_one_native_frame_does_not_fill_between_endpoint_spheres() {
        // At 60 units/s these centres are consecutive 60 Hz samples. The
        // narrow corner lies in the old capsule's extra crescent, outside
        // either actual candidate sphere. No larger radius is warranted.
        let corner = [
            [[-0.05, 6.0, 0.62], [0.05, 6.0, 0.7], [-0.05, 6.0, 0.7]],
            [[-0.05, 6.0, 0.62], [0.05, 6.0, 0.62], [0.05, 6.0, 0.7]],
        ];
        let world = CollisionWorld::new(plane(0.0, true).into_iter().chain(corner)).unwrap();
        let from = Vec3::new(-0.5, 3.75, 0.0);
        let to = Vec3::new(0.5, 3.75, 0.0);
        assert!(probe().fits(world.probe_headroom(from, probe()), 7.5, 0.5));
        assert!(probe().fits(world.probe_headroom(to, probe()), 7.5, 0.5));
        assert!(
            world
                .headroom_on_path(from, to, probe(), 7.5, 0.5, (1, 1.0))
                .is_none()
        );
    }
}

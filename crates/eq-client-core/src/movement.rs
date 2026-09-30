//! Conservative ground locomotion and continuous collision queries.

mod airborne;
mod path;
mod route;
pub use airborne::{AirborneController, Landing, MotionStep, PROVISIONAL_PHYSICS, VerticalPhysics};
pub use path::{PathProgress, PathSearch};
pub use route::{Route, RouteStep};

#[derive(Clone, Copy)]
enum Support {
    Required,
    Optional,
    Airborne,
}

pub use eq_network_game::movement::{MAX_FALL_SPEED, MAX_GROUNDED_STEP};
use glam::Vec3;
use parry3d::{
    math::{Pose, Vector},
    query::{Ray, RayCast, ShapeCastOptions, cast_shapes, contact},
    shape::{Capsule, TriMesh},
};
use std::sync::Arc;

/// How far the collision capsule's base floats above the feet, so floors, stairs
/// and uneven ground never register as walls.
const LIFT: f32 = 0.85;

/// The character's collision capsule at `feet`, floating `LIFT` above them.
fn body(feet: Vec3, height: f32) -> (Pose, Capsule) {
    let radius = 0.4;
    let center = feet + Vec3::Y * (height * 0.5 + LIFT);
    (
        Pose::translation(center.x, center.y, center.z),
        Capsule::new_y(height * 0.5 - radius, radius),
    )
}

/// Shared solid geometry accelerated by Parry's triangle-mesh BVH.
pub struct CollisionMesh {
    mesh: TriMesh,
}

impl CollisionMesh {
    /// Builds reusable geometry from already transformed, Y-up solid triangles.
    ///
    /// # Errors
    /// Rejects empty geometry, excessive vertex counts, and invalid meshes.
    pub fn new(triangles: impl IntoIterator<Item = [[f32; 3]; 3]>) -> Result<Self, String> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for triangle in triangles {
            if triangle.iter().flatten().any(|v| !v.is_finite()) {
                return Err("non-finite collision triangle".into());
            }
            let base = u32::try_from(vertices.len()).map_err(|_| "too many collision vertices")?;
            if base > u32::MAX - 3 {
                return Err("too many collision vertices".into());
            }
            vertices.extend(triangle.map(Vector::from_array));
            indices.push([base, base + 1, base + 2]);
        }
        TriMesh::new(vertices, indices)
            .map(|mesh| Self { mesh })
            .map_err(|error| error.to_string())
    }
}

/// Static zone geometry plus replaceable nearby obstacles.
pub struct CollisionWorld {
    terrain: CollisionMesh,
    obstacles: Vec<Arc<CollisionMesh>>,
}

impl CollisionWorld {
    /// Builds the immutable part of a collision world.
    ///
    /// # Errors
    /// Rejects empty geometry, excessive vertex counts, and invalid meshes.
    pub fn new(triangles: impl IntoIterator<Item = [[f32; 3]; 3]>) -> Result<Self, String> {
        Ok(Self {
            terrain: CollisionMesh::new(triangles)?,
            obstacles: Vec::new(),
        })
    }

    /// Replaces dynamic obstacles without rebuilding the zone's acceleration structure.
    pub fn set_obstacles(&mut self, obstacles: Vec<Arc<CollisionMesh>>) {
        self.obstacles = obstacles;
    }

    fn meshes(&self) -> impl Iterator<Item = &TriMesh> {
        std::iter::once(&self.terrain.mesh).chain(self.obstacles.iter().map(|mesh| &mesh.mesh))
    }

    /// Returns the nearest solid obstruction along a normalized viewing ray.
    pub fn ray_distance(&self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<f32> {
        if !origin.is_finite()
            || !direction.is_finite()
            || !max_distance.is_finite()
            || max_distance <= 0.0
            || (direction.length_squared() - 1.0).abs() > 0.001
        {
            return None;
        }
        let ray = Ray::new(
            Vector::from_array(origin.to_array()),
            Vector::from_array(direction.to_array()),
        );
        self.meshes()
            .filter_map(|mesh| mesh.cast_local_ray(&ray, max_distance, false))
            .min_by(f32::total_cmp)
    }

    /// Finds nearby ground, never a roof above the permitted step height.
    pub fn ground(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        let origin = Vector::from_array((position + Vec3::Y * step_up).to_array());
        let hit = self
            .meshes()
            .filter_map(|mesh| {
                mesh.cast_local_ray_and_get_normal(
                    &Ray::new(origin, -Vector::Y),
                    // Include a floor exactly on the lower query boundary; the
                    // mesh traversal can otherwise discard an endpoint contact.
                    step_up + drop + 0.001,
                    false,
                )
            })
            .min_by(|a, b| a.time_of_impact.total_cmp(&b.time_of_impact))?;
        if hit.normal.y.abs() < 0.7 {
            return None;
        }
        let floor = origin.y - hit.time_of_impact;
        (floor >= position.y - drop && floor <= position.y + step_up).then_some(floor)
    }

    /// Signed distance from the character's collision capsule to the nearest solid
    /// within `range`; a negative value is how deep the capsule overlaps it.
    pub fn clearance(&self, feet: Vec3, height: f32, range: f32) -> Option<f32> {
        if !feet.is_finite()
            || !height.is_finite()
            || height < 1.0
            || !range.is_finite()
            || range < 0.0
        {
            return None;
        }
        let (pose, capsule) = body(feet, height);
        self.meshes()
            .filter_map(|mesh| {
                contact(&pose, &capsule, &Pose::IDENTITY, mesh, range)
                    .ok()
                    .flatten()
            })
            .map(|contact| contact.dist)
            .min_by(f32::total_cmp)
    }

    /// Sweeps the character capsule and slides along walls without crossing cliffs or steep slopes.
    /// Movement stops rather than attempting automatic jumps, swimming, or falling.
    pub fn step(&self, feet: Vec3, displacement: Vec3, height: f32) -> Vec3 {
        self.walk(feet, displacement, height, Support::Required)
    }

    /// Samples support under the capsule footprint, including contact before its center reaches a stair.
    fn support_height(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        const OFFSETS: [(f32, f32); 9] = [
            (0.0, 0.0),
            (0.4, 0.0),
            (-0.4, 0.0),
            (0.0, 0.4),
            (0.0, -0.4),
            (0.282_842_7, 0.282_842_7),
            (-0.282_842_7, 0.282_842_7),
            (0.282_842_7, -0.282_842_7),
            (-0.282_842_7, -0.282_842_7),
        ];
        OFFSETS
            .into_iter()
            .filter_map(|(x, z)| self.ground(position + Vec3::new(x, 0.0, z), step_up, drop))
            .max_by(f32::total_cmp)
    }

    fn walk(&self, feet: Vec3, displacement: Vec3, height: f32, support: Support) -> Vec3 {
        if !feet.is_finite() || !displacement.is_finite() || !height.is_finite() || height < 1.0 {
            return feet;
        }
        let mut position = feet;
        let mut remaining = Vec3::new(displacement.x, 0.0, displacement.z);
        for _ in 0..3 {
            let desired = position + remaining;
            // Sliding shares one height budget with the original proposal. Re-basing
            // the ground probe on each contact could exceed the network guard's limit.
            let probe = Vec3::new(desired.x, feet.y, desired.z);
            let ground = match support {
                Support::Required => {
                    if let Some(ground) =
                        self.support_height(probe, MAX_GROUNDED_STEP, MAX_GROUNDED_STEP)
                    {
                        ground
                    } else {
                        // Find the supported prefix instead of discarding the entire
                        // update at a ledge. Sweep it as usual so walls still win.
                        let mut supported = 0.0;
                        let mut unsupported = 1.0;
                        for _ in 0..12 {
                            let fraction = f32::midpoint(supported, unsupported);
                            let point = position + remaining * fraction;
                            let probe = Vec3::new(point.x, feet.y, point.z);
                            if self
                                .support_height(probe, MAX_GROUNDED_STEP, MAX_GROUNDED_STEP)
                                .is_some()
                            {
                                supported = fraction;
                            } else {
                                unsupported = fraction;
                            }
                        }
                        remaining *= supported;
                        let point = position + remaining;
                        let probe = Vec3::new(point.x, feet.y, point.z);
                        let Some(ground) =
                            self.support_height(probe, MAX_GROUNDED_STEP, MAX_GROUNDED_STEP)
                        else {
                            break;
                        };
                        ground
                    }
                }
                Support::Optional => self
                    .support_height(probe, MAX_GROUNDED_STEP, MAX_GROUNDED_STEP)
                    .unwrap_or(position.y),
                Support::Airborne => position.y,
            };
            let target = Vec3::new(position.x + remaining.x, ground, position.z + remaining.z);
            let delta = target - position;
            if delta.length_squared() < 0.000_001 {
                break;
            }
            match self.sweep(position, delta, height) {
                Ok(None) => {
                    position = target;
                    break;
                }
                Ok(Some(hit)) => {
                    if !matches!(support, Support::Airborne)
                        && target.y > position.y
                        && self.can_step_up(position, target, height)
                    {
                        position = target;
                        break;
                    }
                    let fraction = (hit.time_of_impact - 0.005 / delta.length()).clamp(0.0, 1.0);
                    position += delta * fraction;
                    let normal = Vec3::new(hit.normal1.x, 0.0, hit.normal1.z).normalize_or_zero();
                    if !normal.is_finite() || normal.length_squared() < 0.5 {
                        break;
                    }
                    remaining = Vec3::new(delta.x, 0.0, delta.z) * (1.0 - fraction);
                    remaining -= normal * remaining.dot(normal);
                    if remaining.length_squared() < 0.000_001 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        position
    }

    /// Clears a stair riser by checking both the upward and elevated horizontal sweeps.
    fn can_step_up(&self, feet: Vec3, target: Vec3, height: f32) -> bool {
        let lift = Vec3::Y * (target.y - feet.y);
        matches!(self.sweep(feet, lift, height), Ok(None))
            && matches!(
                self.sweep(feet + lift, target - feet - lift, height),
                Ok(None)
            )
    }

    /// Sweeps one segment with a skin gap, keeping the collision query separate from sliding.
    fn sweep(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Option<parry3d::query::ShapeCastHit>, parry3d::query::Unsupported> {
        let (pose, capsule) = body(feet, height);
        // A sweep that starts inside the margin (or overlapping) is refused only when it
        // goes deeper; one that stops there would pin the character in every direction.
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: 0.03,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let mut nearest: Option<parry3d::query::ShapeCastHit> = None;
        for mesh in self.meshes() {
            let hit = cast_shapes(
                &pose,
                Vector::from_array(delta.to_array()),
                &capsule,
                &Pose::IDENTITY,
                Vector::ZERO,
                mesh,
                options,
            )?;
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

/// Converts input into a bounded displacement; stalls never accumulate catch-up motion.
pub fn displacement(direction: Vec3, speed: f32, seconds: f32) -> Vec3 {
    if !direction.is_finite() || !speed.is_finite() || !seconds.is_finite() {
        return Vec3::ZERO;
    }
    direction.normalize_or_zero() * speed.max(0.0) * seconds.clamp(0.0, 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn floor(y: f32) -> Vec<[[f32; 3]; 3]> {
        vec![
            [[-20.0, y, -20.0], [20.0, y, -20.0], [20.0, y, 20.0]],
            [[-20.0, y, -20.0], [20.0, y, 20.0], [-20.0, y, 20.0]],
        ]
    }
    #[test]
    fn view_rays_use_nearest_static_or_dynamic_solid_and_respect_range() {
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        let origin = Vec3::Y * 10.0;
        assert!((world.ray_distance(origin, -Vec3::Y, 20.0).unwrap() - 10.0).abs() < 0.001);
        world.set_obstacles(vec![Arc::new(CollisionMesh::new(floor(3.0)).unwrap())]);
        assert!((world.ray_distance(origin, -Vec3::Y, 20.0).unwrap() - 7.0).abs() < 0.001);
        assert!(world.ray_distance(origin, -Vec3::Y, 6.0).is_none());
        assert!(world.ray_distance(origin, Vec3::Y, 20.0).is_none());
        assert!(world.ray_distance(origin, Vec3::ZERO, 20.0).is_none());
        assert!(
            world
                .ray_distance(origin, Vec3::splat(f32::NAN), 20.0)
                .is_none()
        );
    }

    #[test]
    fn replaceable_obstacles_block_sweeps_and_supply_ground_without_stale_geometry() {
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        let wall = Arc::new(
            CollisionMesh::new([
                [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
                [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
            ])
            .unwrap(),
        );
        let platform = Arc::new(CollisionMesh::new(floor(0.5)).unwrap());
        world.set_obstacles(vec![platform, wall]);
        assert!((world.ground(Vec3::ZERO, 0.8, 1.5).unwrap() - 0.5).abs() < 0.001);
        let contact = world.step(Vec3::ZERO, Vec3::X, 6.0);
        assert!(contact.x >= 0.0 && contact.x < 0.1, "{contact:?}");
        world.set_obstacles(Vec::new());
        assert!(world.ground(Vec3::ZERO, 0.8, 1.5).unwrap().abs() < 0.001);
        assert!(world.step(Vec3::ZERO, Vec3::X, 6.0).distance(Vec3::X) < 0.001);
        assert!(world.step(Vec3::new(19.9, 0.0, 0.0), Vec3::X, 6.0).x <= 20.4);
    }

    #[test]
    fn roof_cannot_capture_a_character_walking_below_it() {
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(floor(10.0))).unwrap();
        assert!(world.ground(Vec3::ZERO, 0.8, 1.5).unwrap().abs() < 0.001);
        assert!(
            world
                .step(Vec3::ZERO, Vec3::X * 0.2, 6.0)
                .distance(Vec3::X * 0.2)
                < 0.001
        );
    }
    #[test]
    fn capsule_stops_at_a_thin_wall_and_at_a_cliff() {
        let wall = [
            [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
            [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
        ];
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(wall)).unwrap();
        let contact = world.step(Vec3::ZERO, Vec3::X, 6.0);
        assert!(contact.x >= 0.0 && contact.x < 0.1);
        let edge = world.step(Vec3::new(-19.9, 0.0, 0.0), -Vec3::X, 6.0);
        assert!(edge.x < -20.39 && edge.x >= -20.4, "{edge:?}");
        let repeated = world.step(edge, -Vec3::X, 6.0);
        assert!(repeated.x >= -20.4, "{repeated:?}");
    }
    #[test]
    fn sliding_down_a_slope_stays_inside_one_grounded_packet_height_budget() {
        let slope = floor(0.0)
            .into_iter()
            .map(|triangle| triangle.map(|[x, _, z]| [x, 0.4 * x - 0.8 * z, z]));
        let wall = [
            [[1.9, -20.0, -20.0], [1.9, 20.0, -20.0], [1.9, 20.0, 20.0]],
            [[1.9, -20.0, -20.0], [1.9, 20.0, 20.0], [1.9, -20.0, 20.0]],
        ];
        let world = CollisionWorld::new(slope.chain(wall)).unwrap();
        let position = world.step(Vec3::ZERO, Vec3::new(3.0, 0.0, 3.0), 6.0);
        assert!(position.x > 0.0 && position.z > 0.0, "{position:?}");
        assert!(position.y.abs() <= MAX_GROUNDED_STEP, "{position:?}");
        let start = std::time::Instant::now();
        let now = start + std::time::Duration::from_millis(100);
        let mut guard = eq_network_game::movement::MovementGuard::new(
            7,
            crate::WorldPosition::default(),
            start,
        );
        guard.set_speed(Some(60.0), start);
        let request = eq_network_game::movement::MovementRequest {
            session_id: 7,
            mode: eq_network_game::movement::MovementMode::Forward,
            position: crate::world_position(position.to_array(), 0.0),
            created: now,
        };
        assert!(guard.accept(&request, now).is_ok());
    }

    #[test]
    fn diagonal_contact_slides_without_crossing_wall_or_gaining_distance() {
        let wall = [
            [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
            [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
        ];
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(wall)).unwrap();
        let requested = Vec3::new(1.0, 0.0, 1.0);
        let position = world.step(Vec3::ZERO, requested, 6.0);
        assert!(position.x >= 0.0 && position.x < 0.1, "{position:?}");
        assert!(position.z > 0.9, "{position:?}");
        assert!(position.length() <= requested.length());
        let next = world.step(position, requested, 6.0);
        assert!(next.x < 0.1 && next.z > 1.8, "{next:?}");
    }
    #[test]
    fn starting_in_contact_refuses_only_moves_that_go_deeper() {
        let wall = [
            [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
            [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
        ];
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(wall)).unwrap();
        // Inside the sweep's contact margin, then overlapping the wall slightly.
        for gap in [0.01, -0.02] {
            let start = Vec3::new(0.1 - gap, 0.0, 0.0);
            let clearance = world.clearance(start, 6.0, 1.0).unwrap();
            assert!((clearance - gap).abs() < 0.001, "{clearance}");
            let away = world.step(start, -Vec3::X * 0.5, 6.0);
            assert!(away.distance(start - Vec3::X * 0.5) < 0.001, "{away:?}");
            let along = world.step(start, Vec3::Z * 0.5, 6.0);
            assert!(along.distance(start + Vec3::Z * 0.5) < 0.001, "{along:?}");
            let into = world.step(start, Vec3::X * 0.5, 6.0);
            assert!(into.x <= start.x + 0.000_1, "{into:?}");
            let diagonal = world.step(start, Vec3::new(0.5, 0.0, 0.5), 6.0);
            assert!(diagonal.x <= start.x + 0.000_1, "{diagonal:?}");
            assert!(diagonal.z > 0.49, "{diagonal:?}");
            // Online falls walk through the airborne controller instead.
            let online = AirborneController::default().step(
                &world,
                start,
                PROVISIONAL_PHYSICS,
                MotionStep {
                    horizontal: -Vec3::X * 0.5,
                    jump: false,
                    seconds: 0.05,
                    height: 6.0,
                },
            );
            assert!(online.distance(start - Vec3::X * 0.5) < 0.001, "{online:?}");
        }
    }

    #[test]
    fn sliding_into_a_corner_cannot_cross_either_wall() {
        let walls = [
            [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
            [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
            [[-10.0, 0.0, 0.5], [10.0, 10.0, 0.5], [-10.0, 10.0, 0.5]],
            [[-10.0, 0.0, 0.5], [10.0, 0.0, 0.5], [10.0, 10.0, 0.5]],
        ];
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(walls)).unwrap();
        let mut position = Vec3::ZERO;
        for _ in 0..10 {
            position = world.step(position, Vec3::new(1.0, 0.0, 1.0), 6.0);
            assert!(position.x < 0.1 && position.z < 0.1, "{position:?}");
        }
    }

    #[test]
    fn ground_query_includes_its_exact_lower_boundary_without_extending_the_budget() {
        let world = CollisionWorld::new(floor(0.0)).unwrap();
        assert!(world.ground(Vec3::Y * 2.0, 2.0, 2.0).is_some());
        assert!(world.ground(Vec3::Y * 2.0005, 2.0, 2.0).is_none());
    }

    #[test]
    fn one_unit_stair_can_be_climbed_but_not_through_a_low_ceiling() {
        let stair = [
            [[0.0, 1.0, -10.0], [10.0, 1.0, -10.0], [10.0, 1.0, 10.0]],
            [[0.0, 1.0, -10.0], [10.0, 1.0, 10.0], [0.0, 1.0, 10.0]],
            [[0.0, 0.0, -10.0], [0.0, 1.0, -10.0], [0.0, 1.0, 10.0]],
            [[0.0, 0.0, -10.0], [0.0, 1.0, 10.0], [0.0, 0.0, 10.0]],
        ];
        let start = Vec3::new(-0.8, 0.0, 0.0);
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(stair)).unwrap();
        let end = world.step(start, Vec3::X * 1.6, 6.0);
        assert!(end.distance(Vec3::new(0.8, 1.0, 0.0)) < 0.001, "{end:?}");
        let mut incremental = start;
        for _ in 0..16 {
            incremental = world.step(incremental, Vec3::X * 0.1, 6.0);
        }
        assert!(incremental.distance(end) < 0.001, "{incremental:?}");
        let covered =
            CollisionWorld::new(floor(0.0).into_iter().chain(stair).chain(floor(7.0))).unwrap();
        let blocked = covered.step(start, Vec3::X * 1.6, 6.0);
        assert!(blocked.x < 0.0, "{blocked:?}");
    }

    #[test]
    fn diagonal_motion_and_long_frames_do_not_increase_speed() {
        let straight = displacement(Vec3::X, 6.0, 0.05);
        let diagonal = displacement(Vec3::X + Vec3::Z, 6.0, 4.0);
        assert!((straight.length() - diagonal.length()).abs() < 0.0001);
        assert!(displacement(Vec3::X, f32::NAN, 1.0).length() < 0.0001);
    }
}

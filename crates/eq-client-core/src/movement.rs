//! Conservative ground locomotion and continuous collision queries.

mod airborne;
mod carrier_filter;
use carrier_filter::SweepPurpose;
#[cfg(test)]
mod carrier_walls;
mod choices;
mod falls;
mod flight;
mod instances;
mod jesus_mode;
pub use instances::CollisionInstance;
use instances::Surface;
mod liquids;
mod navigator;
mod observations;
pub use observations::{ContactReport, MotionObservation};
pub mod navmesh;
mod pace;
mod pad;
mod path;
mod route;
mod surface;
mod swimming;
pub use airborne::{AirborneController, Landing, MotionStep, PROVISIONAL_PHYSICS, VerticalPhysics};
pub use choices::{offered_options, options};
use eq_network_game::doors::titanium::Solidity;
pub use eq_network_game::movement::{
    CARRIER_REACH, CarrierSample, Carriers, MovementOptions, MovementPath, MovementPoint,
    PostureOptions, RunSpeedMultiplier, carrier_local, carrier_world,
};
pub use eq_network_game::movement::{
    SHORELINE_COOLDOWN, SHORELINE_FRAME_MILLIS, SHORELINE_FRAMES, ShorelineFrame, ShorelineStroke,
    ShorelineTrace, ShorelineVelocity,
};
pub use falls::{HARMLESS_DROP, fall_damage};
use liquids::LiquidGuard;
pub use liquids::{GuardView, Liquid, Liquids, SurfaceRegion};
pub use navigator::{
    Fallback, GridNavigator, Navigator, Planned, Refusals, RouteSearch, Scope, SearchRequest,
};
pub use pace::{ANIMATION_UNITS_PER_SECOND, record_speed, runs};
pub use pad::PadArrival;
pub use path::{PathProgress, PathSearch};
pub use route::{Background, Route, RouteStep, SearchTiming};
pub use surface::{SurfaceClock, SurfaceDecline, SurfacePlan, SurfaceStroke};
pub use swimming::{LiquidContact, ShorelineStep, SwimDepth};

#[derive(Clone, Copy)]
enum Support {
    /// Ground under every stride, within a riser up or down.
    Required,
    /// Standing on the ground as the airborne controller walks: a stride
    /// keeps the feet on a floor no more than `drop` below them, and past
    /// it moves across at their height, leaving the drop to the fall. It
    /// steps up onto a floor no more than `climb` above them, and the whole
    /// walk (one step's) rises no further. In the programs a step stands
    /// the feet on the first surface the floor sweep from the origin meets
    /// (read 6.344); what stops a move short of that is their horizontal
    /// collision, a sphere swept a lift height above the origin, which this
    /// client still makes with its own capsule, never letting the player
    /// past what the capsule meets (6.344 remainder: the capsule against the
    /// raised sphere, the engine's slide or stop, TAKP's twin of 0x48acef,
    /// the model's own step value).
    Optional { drop: f32, climb: f32 },
    /// In the air: no ground is kept, and a face whose top is no more than
    /// `clear` above the feet doesn't stop the move, which steps onto that
    /// top instead (the program's raised sweep, read R42; 6.484).
    Airborne { clear: f32 },
}

struct LavaContact;

pub use eq_network_game::movement::{
    Carried, EQMAC_FALL_SPEED, MAX_FALL_SPEED, MAX_GROUNDED_STEP, PROGRAM_STEP_SECONDS,
    SwimCalibration, ZBetween, between_reports, carry, climb_reach, program_steps, swim_speed,
    water,
};
use glam::Vec3;
use parry3d::{
    math::{Pose, Vector},
    query::{Ray, ShapeCastOptions, cast_shapes, contact, intersection_test},
    shape::{Capsule, TriMesh},
};
use std::sync::Arc;

/// How far the collision capsule's base floats above the feet, so floors, stairs
/// and uneven ground never register as walls.
const LIFT: f32 = 0.85;

/// The collision capsule's radius.
const CAPSULE_RADIUS: f32 = 0.4;
/// How far a sweep stops short of what it hits.
const SKIN: f32 = 0.03;
/// How far from the capsule's centre the ground under it is sampled.
const SUPPORT_SPREAD: f32 = 0.4;
const SUPPORT_OFFSETS: [(f32, f32); 9] = [
    (0.0, 0.0),
    (SUPPORT_SPREAD, 0.0),
    (-SUPPORT_SPREAD, 0.0),
    (0.0, SUPPORT_SPREAD),
    (0.0, -SUPPORT_SPREAD),
    (0.282_842_7, 0.282_842_7),
    (-0.282_842_7, 0.282_842_7),
    (0.282_842_7, -0.282_842_7),
    (-0.282_842_7, -0.282_842_7),
];
/// How far across the ground from the feet any one movement probe reaches,
/// beyond the distance moved: the capsule with its skin, or the ground
/// samples around it.
pub(super) const BODY_REACH: f32 = if CAPSULE_RADIUS + SKIN > SUPPORT_SPREAD {
    CAPSULE_RADIUS + SKIN
} else {
    SUPPORT_SPREAD
};

/// The character's collision capsule at `feet`, floating `LIFT` above them.
fn body(feet: Vec3, height: f32) -> (Pose, Capsule) {
    let radius = CAPSULE_RADIUS;
    let center = feet + Vec3::Y * (height * 0.5 + LIFT);
    (
        Pose::translation(center.x, center.y, center.z),
        Capsule::new_y(height * 0.5 - radius, radius),
    )
}

/// Maps the program's ducked room onto this client's capsule. The need,
/// margin and zero ducked lift are read on Titanium and Mac; using a capsule
/// with its existing radius and base lift is ours, stricter than their shape.
/// The top never falls below either that room or S plus the host's estimated
/// native sphere radius. That estimate is distinct from the unchanged capsule
/// width and is supplied from the same source as the standing-sweep estimate.
#[must_use]
pub fn ducked_height(
    stand_height: f32,
    room: f32,
    sphere_radius: f32,
    standing_height: f32,
) -> f32 {
    if !stand_height.is_finite()
        || !room.is_finite()
        || !sphere_radius.is_finite()
        || sphere_radius < 0.0
    {
        return standing_height;
    }
    (room.max(stand_height + sphere_radius) - LIFT).max(1.0_f32.max(2.0 * CAPSULE_RADIUS))
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

/// What a body sweep touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obstacle {
    /// The zone's own geometry.
    Terrain,
    /// A door, by its id.
    Door(u8),
    /// A placed obstacle that belongs to no door.
    Static,
    /// A carrier spawn's model, by its spawn id.
    Carrier(u16),
}

/// The first wall a body sweep meets, for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// What was met.
    pub obstacle: Obstacle,
    /// The contact normal on the body, in render axes.
    pub normal: Vec3,
    /// The fraction of the sweep reached before contact.
    pub time_of_impact: f32,
}

/// A placed obstacle: the door it belongs to, if any, its mesh, and how
/// solid it is.
type Obstruction = (Option<u8>, Arc<CollisionMesh>, Solidity);

/// Static zone geometry plus replaceable nearby obstacles, and the zone's
/// water and lava where they are known. A clone shares all of it, so a
/// snapshot for another thread costs a few reference counts.
#[derive(Clone)]
pub struct CollisionWorld {
    /// Optional, per-trial observations; never part of geometry or movement rules.
    observation: Option<MotionObservation>,
    terrain: Arc<CollisionMesh>,
    /// Each obstacle with the door it belongs to, if any, and whether it
    /// holds the player up and stops them walking.
    obstacles: Arc<[Obstruction]>,
    /// Actual model geometry of nearby received carrier spawns.
    spawn_obstacles: Arc<[(u16, CollisionInstance)]>,
    liquids: Option<Arc<dyn Liquids>>,
    /// The zone's liquids as the player's own moves meet them, where they
    /// differ: the Titanium program swims lava (R49-26), so its walks,
    /// falls and strokes enter lava as water, while routes this client
    /// chooses keep [`Self::liquids`] and refuse it.
    own_liquids: Option<Arc<dyn Liquids>>,
    liquid_support: bool,
    swim_origin: Option<f32>,
    /// A per-move shape override; liquid probes still receive their original
    /// body height and origin. Never part of the static navigation world.
    collision_height: Option<f32>,
    titanium_walking_origin: Option<f32>,
}

impl CollisionWorld {
    /// Builds the immutable part of a collision world.
    ///
    /// # Errors
    /// Rejects empty geometry, excessive vertex counts, and invalid meshes.
    pub fn new(triangles: impl IntoIterator<Item = [[f32; 3]; 3]>) -> Result<Self, String> {
        Ok(Self {
            terrain: Arc::new(CollisionMesh::new(triangles)?),
            obstacles: Arc::new([]),
            spawn_obstacles: Arc::new([]),
            liquids: None,
            own_liquids: None,
            liquid_support: false,
            swim_origin: None,
            collision_height: None,
            titanium_walking_origin: None,
            observation: None,
        })
    }

    /// Uses a posture's collision height without changing liquid/head probes,
    /// the origin, capsule radius or the base lift. Invalid heights leave the
    /// shape unchanged. The caller keeps this snapshot local to its move.
    #[must_use]
    pub fn with_collision_height(mut self, height: f32) -> Self {
        self.collision_height = (height.is_finite() && height >= 1.0).then_some(height);
        self
    }

    /// Uses Titanium's ordinary walking object filter for lateral strides.
    /// The caller keeps this view local to its dry walking move and supplies
    /// the actor origin above its feet for the separate ceiling query.
    /// An invalid offset keeps NPC carrier walls enabled.
    #[must_use]
    pub fn with_titanium_walking(mut self, feet_offset: f32) -> Self {
        self.titanium_walking_origin =
            (feet_offset.is_finite() && feet_offset >= 0.0).then_some(feet_offset);
        self
    }

    /// Replaces dynamic obstacles without rebuilding the zone's acceleration structure.
    pub fn set_obstacles(&mut self, obstacles: Vec<Arc<CollisionMesh>>) {
        self.obstacles = obstacles
            .into_iter()
            .map(|mesh| (None, mesh, Solidity::BOTH))
            .collect();
    }

    /// Replaces dynamic obstacles with doors' colliders, each with its door id,
    /// so a route search can tell which door stands in its way, and its
    /// solidity: a door the floor sweep skips holds no one up, and one the
    /// wall contact skips stops no one.
    pub fn set_door_obstacles(&mut self, doors: Vec<(u8, Arc<CollisionMesh>)>) {
        self.set_door_obstacles_with(
            doors
                .into_iter()
                .map(|(id, mesh)| (id, mesh, Solidity::BOTH))
                .collect(),
        );
    }

    /// Replaces the door colliders, each solid only where its rule says: on
    /// Titanium most doors are walls nobody stands on, and a few types are
    /// neither (see [`crate::doors::Solidity::of`]).
    pub fn set_door_obstacles_with(&mut self, doors: Vec<(u8, Arc<CollisionMesh>, Solidity)>) {
        self.obstacles = doors
            .into_iter()
            .map(|(id, mesh, solidity)| (Some(id), mesh, solidity))
            .collect();
    }

    /// Replaces carrier colliders independently of doors. Their floor sweep
    /// identifies the spawn underfoot; no generic NPC gains collision.
    pub fn set_spawn_obstacles(&mut self, spawns: Vec<(u16, Arc<CollisionMesh>)>) {
        self.spawn_obstacles = spawns
            .into_iter()
            .map(|(id, mesh)| (id, CollisionInstance::identity(mesh)))
            .collect();
    }

    /// Whether this carrier's collider is loaded.
    #[must_use]
    pub fn has_spawn(&self, id: u16) -> bool {
        self.spawn_obstacles.iter().any(|(spawn, _)| *spawn == id)
    }

    /// Replaces carrier placements while sharing their authored geometry and
    /// acceleration structures. No triangles are rebuilt to move an instance.
    pub fn set_spawn_instances(&mut self, spawns: Vec<(u16, CollisionInstance)>) {
        self.spawn_obstacles = spawns.into();
    }

    /// The same world with these doors' colliders left out, sharing its
    /// geometry, as if they stood open.
    #[must_use]
    pub fn without_doors(&self, doors: &[u8]) -> Self {
        Self {
            terrain: Arc::clone(&self.terrain),
            obstacles: self
                .obstacles
                .iter()
                .filter(|(door, ..)| door.is_none_or(|door| !doors.contains(&door)))
                .cloned()
                .collect(),
            spawn_obstacles: self.spawn_obstacles.clone(),
            liquids: self.liquids.clone(),
            own_liquids: self.own_liquids.clone(),
            liquid_support: self.liquid_support,
            swim_origin: self.swim_origin,
            collision_height: self.collision_height,
            titanium_walking_origin: self.titanium_walking_origin,
            observation: self.observation.clone(),
        }
    }

    /// The same world keeping only the obstacles that come within `reach` of
    /// `center` across the ground, sharing its geometry. Any movement that
    /// stays within `reach` of `center` meets exactly what it would in the
    /// whole world.
    #[must_use]
    pub fn near(&self, center: Vec3, reach: f32) -> Self {
        Self {
            terrain: Arc::clone(&self.terrain),
            obstacles: self
                .obstacles
                .iter()
                .filter(|(_, mesh, _)| {
                    let bounds = mesh.mesh.local_aabb();
                    center.x >= bounds.mins.x - reach
                        && center.x <= bounds.maxs.x + reach
                        && center.z >= bounds.mins.z - reach
                        && center.z <= bounds.maxs.z + reach
                })
                .cloned()
                .collect(),
            spawn_obstacles: self.spawn_obstacles.clone(),
            liquids: self.liquids.clone(),
            own_liquids: self.own_liquids.clone(),
            liquid_support: self.liquid_support,
            swim_origin: self.swim_origin,
            collision_height: self.collision_height,
            titanium_walking_origin: self.titanium_walking_origin,
            observation: self.observation.clone(),
        }
    }

    /// The zone's own triangles, without the obstacles, as a zone's mesh is
    /// built from them.
    #[cfg(test)]
    pub(crate) fn terrain_triangles(&self) -> Vec<[[f32; 3]; 3]> {
        let mesh = &self.terrain.mesh;
        let vertices = mesh.vertices();
        mesh.indices()
            .iter()
            .map(|triangle| triangle.map(|i| vertices[i as usize].into()))
            .collect()
    }

    /// Each door collider's id and bounds, low corner then high.
    #[must_use]
    pub fn door_bounds(&self) -> Vec<(u8, Vec3, Vec3)> {
        self.obstacles
            .iter()
            .filter_map(|(door, mesh, _)| {
                let bounds = mesh.mesh.local_aabb();
                Some((
                    (*door)?,
                    Vec3::from_array(bounds.mins.into()),
                    Vec3::from_array(bounds.maxs.into()),
                ))
            })
            .collect()
    }

    /// How many obstacles stand in the world, for diagnostics.
    #[must_use]
    pub fn obstacle_count(&self) -> usize {
        self.obstacles.len() + self.spawn_obstacles.len()
    }

    /// The first of these doors whose collider comes within `margin` of the box
    /// spanning `from` and `to`.
    #[must_use]
    pub fn door_between(&self, doors: &[u8], from: Vec3, to: Vec3, margin: f32) -> Option<u8> {
        let (low, high) = (
            from.min(to) - Vec3::splat(margin),
            from.max(to) + Vec3::splat(margin),
        );
        self.obstacles.iter().find_map(|(door, mesh, _)| {
            let door = (*door).filter(|door| doors.contains(door))?;
            let bounds = mesh.mesh.local_aabb();
            let (mins, maxs) = (
                Vec3::from_array(bounds.mins.into()),
                Vec3::from_array(bounds.maxs.into()),
            );
            (mins.cmple(high).all() && maxs.cmpge(low).all()).then_some(door)
        })
    }

    fn meshes(&self) -> impl Iterator<Item = Surface<'_>> {
        std::iter::once(Surface::identity(&self.terrain))
            .chain(
                self.obstacles
                    .iter()
                    .map(|(_, mesh, _)| Surface::identity(mesh)),
            )
            .chain(
                self.spawn_obstacles
                    .iter()
                    .map(|(_, instance)| instance.surface()),
            )
    }

    /// The zone and the obstacles that hold the player up.
    fn floor_meshes(&self) -> impl Iterator<Item = Surface<'_>> {
        std::iter::once(Surface::identity(&self.terrain))
            .chain(
                self.obstacles
                    .iter()
                    .filter(|(.., solidity)| solidity.floor)
                    .map(|(_, mesh, _)| Surface::identity(mesh)),
            )
            .chain(
                self.spawn_obstacles
                    .iter()
                    .map(|(_, instance)| instance.surface()),
            )
    }

    /// The zone and the obstacles that stop the player walking.
    /// This world without the walls of each door the body at `feet`
    /// overlaps, if any, so the player can walk out rather than be held in
    /// every direction. Its floor still holds them up: the capsule rides
    /// above the feet, so a floor it stands on does not count.
    ///
    /// Only a door or the world moving the body can make such an overlap,
    /// never the player walking: every sweep stops `SKIN` short of a wall,
    /// so pressing or sliding into a door never releases it (see the test
    /// `pressing_or_sliding_into_a_door_never_releases_it`). The ways the body
    /// is placed without a sweep are: a door that moves into it (a lift
    /// landing around a player beside it, a hinged door swinging shut on
    /// them); the server placing it (a summon, a `#goto`, a zone-in); and a
    /// change of size beside a door. A lift's carry moves the body only where
    /// it fits, by the same clearance, so it cannot carry it into another
    /// door. The airborne controller's falls and jumps are swept like a walk.
    /// What the program does with a body a door moves into is unread
    /// (recording checklist 6.439, inferred).
    #[must_use]
    pub fn releasing_overlapped_doors(&self, feet: Vec3, height: f32) -> Option<Self> {
        if !feet.is_finite() || !height.is_finite() || height < 1.0 {
            return None;
        }
        let (pose, capsule) = body(feet, self.collision_height.unwrap_or(height));
        // Inside a closed door, or crossing a thin one's faces (a
        // portcullis's bars, one plane, come down through the body).
        let inside = |mesh: &CollisionMesh| {
            contact(&pose, &capsule, &Pose::IDENTITY, &mesh.mesh, 0.0)
                .ok()
                .flatten()
                .is_some_and(|contact| contact.dist < 0.0)
                || intersection_test(&pose, &capsule, &Pose::IDENTITY, &mesh.mesh).unwrap_or(false)
        };
        let mut released = false;
        let obstacles = self
            .obstacles
            .iter()
            .map(|(door, mesh, solidity)| {
                let mut solidity = *solidity;
                if door.is_some() && solidity.wall && inside(mesh) {
                    solidity.wall = false;
                    released = true;
                }
                (*door, Arc::clone(mesh), solidity)
            })
            .collect();
        released.then(|| Self {
            obstacles,
            ..self.clone()
        })
    }

    fn wall_meshes(&self) -> impl Iterator<Item = Surface<'_>> {
        self.labelled_wall_meshes().map(|(_, surface)| surface)
    }

    /// The walls, each with what it belongs to.
    fn labelled_wall_meshes(&self) -> impl Iterator<Item = (Obstacle, Surface<'_>)> {
        self.labelled_wall_meshes_for(SweepPurpose::Body)
    }

    fn labelled_wall_meshes_for(
        &self,
        purpose: SweepPurpose,
    ) -> impl Iterator<Item = (Obstacle, Surface<'_>)> {
        std::iter::once((Obstacle::Terrain, Surface::identity(&self.terrain)))
            .chain(
                self.obstacles
                    .iter()
                    .filter(|(.., solidity)| solidity.wall)
                    .map(|(door, mesh, _)| {
                        (
                            door.map_or(Obstacle::Static, Obstacle::Door),
                            Surface::identity(mesh),
                        )
                    }),
            )
            .chain(
                self.spawn_obstacles
                    .iter()
                    .filter(move |(_, instance)| self.carrier_blocks(purpose, instance))
                    .map(|(id, instance)| (Obstacle::Carrier(*id), instance.surface())),
            )
    }

    /// What the body would touch first sweeping `delta` from `feet`, and the
    /// contact normal, for diagnostics only: movement never calls it. It
    /// sweeps the whole delta on one straight line, while the mover steps
    /// and slides in parts, so it names the first obstacle on that line,
    /// which is not always the one that stopped the move.
    #[must_use]
    pub fn first_contact(&self, feet: Vec3, delta: Vec3, height: f32) -> Option<Contact> {
        if !feet.is_finite() || !delta.is_finite() || delta.length_squared() < 1e-12 {
            return None;
        }
        self.labelled_sweep(feet, delta, height)
            .ok()
            .flatten()
            .map(|(obstacle, hit)| Contact {
                obstacle,
                normal: Vec3::from_array(hit.normal1.to_array()),
                time_of_impact: hit.time_of_impact,
            })
    }

    /// The door whose floor holds the player up at `position`, between
    /// `step_up` above it and `drop` below, if the floor found there is a
    /// door's: the floor [`Self::ground`] finds, when it is solid.
    #[must_use]
    pub fn ground_door(&self, position: Vec3, step_up: f32, drop: f32) -> Option<u8> {
        const EDGE: f32 = 0.000_1;
        let origin = Vector::from_array((position + Vec3::Y * (step_up + EDGE)).to_array());
        let ray = Ray::new(origin, -Vector::Y);
        let reach = step_up + drop + 2.0 * EDGE;
        let terrain = Surface::identity(&self.terrain).cast_floor(&ray, reach);
        let (door, time) = self
            .obstacles
            .iter()
            .filter(|(door, _, solidity)| door.is_some() && solidity.floor)
            .filter_map(|(door, mesh, _)| {
                Some(((*door)?, Surface::identity(mesh).cast_floor(&ray, reach)?))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        terrain.is_none_or(|ground| time <= ground).then_some(door)
    }

    /// The carrier whose actual model floor wins the ordinary downward
    /// floor probe. Terrain and door floors keep priority when higher.
    #[must_use]
    pub fn ground_spawn(&self, position: Vec3, step_up: f32, drop: f32) -> Option<u16> {
        self.spawn_floor(position, step_up, drop).map(|(id, _)| id)
    }

    /// The carrier [`Self::ground_spawn`] finds, with the height of the
    /// floor it holds the footprint up at.
    #[must_use]
    pub fn spawn_floor(&self, position: Vec3, step_up: f32, drop: f32) -> Option<(u16, f32)> {
        const EDGE: f32 = 0.000_1;
        let supported = self.support_height(position, step_up, drop)?;
        for (x, z) in SUPPORT_OFFSETS {
            let sample = position + Vec3::new(x, step_up + EDGE, z);
            let ray = Ray::new(Vector::from_array(sample.to_array()), -Vector::Y);
            for (id, instance) in self.spawn_obstacles.iter() {
                if let Some(time) = instance
                    .surface()
                    .cast_floor(&ray, step_up + drop + 2.0 * EDGE)
                    && (sample.y - time - supported).abs() <= EDGE
                {
                    return Some((*id, supported));
                }
            }
        }
        None
    }

    /// [`Self::spawn_floor`], keeping the carrier a rider is linked to: while
    /// the first floor straight down under any part of the footprint is
    /// still `current`'s, it stays the link, with the height of the highest
    /// such floor, though another carrier's floor stands higher or level
    /// under the rest. Only once no part of the footprint meets it does
    /// the link go to whichever floor [`Self::spawn_floor`] finds. So a
    /// ship passing alongside never takes the rider off the deck it stands
    /// on. The program's choice between two carriers is unread: this is
    /// inferred, and the stricter side (6.611).
    #[must_use]
    pub fn spawn_floor_keeping(
        &self,
        position: Vec3,
        step_up: f32,
        drop: f32,
        current: Option<u16>,
    ) -> Option<(u16, f32)> {
        const EDGE: f32 = 0.000_1;
        let kept = current.and_then(|current| {
            let (_, instance) = self.spawn_obstacles.iter().find(|(id, _)| *id == current)?;
            SUPPORT_OFFSETS
                .into_iter()
                .filter_map(|(x, z)| {
                    let sample = position + Vec3::new(x, step_up + EDGE, z);
                    let ray = Ray::new(Vector::from_array(sample.to_array()), -Vector::Y);
                    let reach = step_up + drop + 2.0 * EDGE;
                    let time = instance.surface().cast_floor(&ray, reach)?;
                    let first = self
                        .floor_meshes()
                        .filter_map(|mesh| mesh.cast_floor(&ray, reach))
                        .min_by(f32::total_cmp)?;
                    (time <= first + EDGE).then_some(sample.y - time)
                })
                .max_by(f32::total_cmp)
                .map(|height| (current, height))
        });
        kept.or_else(|| self.spawn_floor(position, step_up, drop))
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
            .filter_map(|mesh| mesh.cast_ray(&ray, max_distance, false))
            .min_by(f32::total_cmp)
    }

    /// Finds nearby ground, never a roof above the permitted step height: the
    /// first surface straight down from `step_up` above `position`, down to
    /// `drop` below it, at any angle. Both generations' programs find a floor
    /// so and their on-ground test compares
    /// heights only (Titanium 0x48a0fc and 0x48a2f4, `EQMac` 0x4b3a84 and
    /// 0x50ba07; read, Decompile R-s, 11-01), so a face too steep to have
    /// been floor before is floor at its height. The Titanium floor query
    /// skips downward-facing faces (read 0x10185790 / 0x10192400, 6.651);
    /// the same eligibility on Mac is inferred. They sweep a sphere down
    /// where this casts a ray (checklist 6.336).
    pub fn ground(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        self.solid_ground(position, step_up, drop)
            .into_iter()
            .chain(self.liquid_ground(position, step_up, drop))
            .max_by(f32::total_cmp)
    }

    fn solid_ground(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        // Both window edges are inclusive: the ray starts a hair above the top one,
        // since a ray starting on a surface can miss it, and runs a hair past the
        // bottom one, since mesh traversal can discard an endpoint contact.
        const EDGE: f32 = 0.000_1;
        let origin = Vector::from_array((position + Vec3::Y * (step_up + EDGE)).to_array());
        let time = self
            .floor_meshes()
            .filter_map(|mesh| {
                mesh.cast_floor(&Ray::new(origin, -Vector::Y), step_up + drop + 2.0 * EDGE)
            })
            .min_by(f32::total_cmp)?;
        let floor = origin.y - time;
        (floor >= position.y - drop - EDGE && floor <= position.y + step_up + EDGE)
            .then(|| floor.clamp(position.y - drop, position.y + step_up))
    }

    /// Finds the floor nearest `position`'s height between `step_up` above
    /// it and `drop` below, each surface straight down at any angle, as
    /// [`Self::ground`] takes one. Where floors are stacked in that window,
    /// as a bridge over the ground, the one closest to `position` wins, not
    /// simply the highest.
    pub fn nearest_ground(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        // The window's edges are inclusive, as in `ground`; each next ray
        // starts a hair under the surface the last one met.
        const EDGE: f32 = 0.000_1;
        const SURFACES: usize = 32;
        let bottom = position.y - drop - EDGE;
        let mut from = position.y + step_up + EDGE;
        let mut nearest: Option<f32> = None;
        for _ in 0..SURFACES {
            if from <= bottom {
                break;
            }
            let origin = Vector::from_array(Vec3::new(position.x, from, position.z).to_array());
            let Some(time) = self
                .floor_meshes()
                .filter_map(|mesh| {
                    mesh.cast_floor(&Ray::new(origin, -Vector::Y), from - bottom + EDGE)
                })
                .min_by(f32::total_cmp)
            else {
                break;
            };
            let met = from - time;
            if met < bottom {
                break;
            }
            let floor = met.clamp(position.y - drop, position.y + step_up);
            if nearest.is_none_or(|best| (floor - position.y).abs() < (best - position.y).abs()) {
                nearest = Some(floor);
            }
            from = met - EDGE;
        }
        nearest
    }

    /// Clearance for a manual duck-to-stand transition (Titanium 0x483556,
    /// Mac 0x50be3c): the new posture's strict room test, then ceiling minus
    /// origin at least S. These rays approximate the programs' probe shapes;
    /// this helper does not install a headroom gate on movement.
    #[must_use]
    pub fn posture_clearance(&self, origin: Vec3, stand_height: f32, room: f32) -> bool {
        const REACH: f32 = 32_767.0;
        if !origin.is_finite() || !stand_height.is_finite() || !room.is_finite() {
            return false;
        }
        let up = Ray::new(Vector::from_array(origin.to_array()), Vector::Y);
        let ceiling = self
            .labelled_wall_meshes()
            .filter(|(obstacle, _)| !matches!(obstacle, Obstacle::Carrier(_)))
            .filter_map(|(_, mesh)| mesh.cast_ray(&up, REACH, false))
            .min_by(f32::total_cmp)
            .map(|distance| origin.y + distance);
        let Some(ceiling) = ceiling else {
            return true;
        };
        let fits = self
            .solid_ground(origin, 0.0, REACH)
            .is_none_or(|floor| ceiling <= floor || ceiling - floor > room);
        fits && ceiling - origin.y >= stand_height
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
            self.observe_unknown();
            return None;
        }
        self.capsule_clearance(feet, self.collision_height.unwrap_or(height), range)
    }

    /// Contact at the exact swimming/flight capsule, with no walk lift or
    /// posture override. Unsupported queries remain visible to a trial token.
    #[must_use]
    pub fn swim_clearance(&self, feet: Vec3, height: f32) -> Option<f32> {
        if !feet.is_finite() || !height.is_finite() || height < 1.0 {
            self.observe_unknown();
            return None;
        }
        self.capsule_clearance(feet - Vec3::Y * LIFT, height, 0.0)
    }

    fn capsule_clearance(&self, feet: Vec3, height: f32, range: f32) -> Option<f32> {
        let (pose, capsule) = body(feet, height);
        self.wall_meshes()
            .filter_map(|mesh| {
                let result = contact(&pose, &capsule, &mesh.pose, mesh.mesh, range);
                self.observe_sweep(&result);
                result.ok().flatten()
            })
            .map(|contact| contact.dist)
            .min_by(f32::total_cmp)
    }

    /// Sweeps the character capsule and slides along walls without crossing cliffs or steep slopes.
    /// Movement stops rather than attempting automatic jumps, swimming, or falling, and
    /// before deep water or lava where the world knows the zone's liquids.
    pub fn step(&self, feet: Vec3, displacement: Vec3, height: f32) -> Vec3 {
        self.walk(feet, displacement, height, Support::Required)
    }

    /// Finds a nearby solid surface above a fresh placement's whole footprint
    /// when the placement probe has no floor below it. This is evidence that
    /// gravity cannot land the body in place. `probe_lift` is measured from
    /// the received origin and `body_size` bounds the upward diagnosis.
    #[must_use]
    pub fn floor_above_placement(
        &self,
        origin: Vec3,
        probe_lift: f32,
        body_size: f32,
    ) -> Option<f32> {
        if !origin.is_finite() || !body_size.is_finite() || !(0.0..body_size).contains(&probe_lift)
        {
            return None;
        }
        let probe = origin + Vec3::Y * probe_lift;
        let range = body_size - probe_lift;
        let bottom = self
            .meshes()
            .map(Surface::minimum_y)
            .fold(probe.y, f32::min);
        let drop = probe.y - bottom;
        let mut center = None;
        for (x, z) in SUPPORT_OFFSETS {
            let sample = probe + Vec3::new(x, 0.0, z);
            let ray = Ray::new(Vector::from_array(sample.to_array()), Vector::Y);
            let height = sample.y + Surface::identity(&self.terrain).cast_floor(&ray, range)?;
            // A lower floor can be a tunnel or the landing of an ordinary fall.
            if self.ground(sample, 0.0, drop).is_some() {
                return None;
            }
            // A ledge or uneven overhead obstruction is ambiguous. Require
            // one coherent surface across the same probes used for support.
            let floor = *center.get_or_insert(height);
            if (height - floor).abs() > 0.5 {
                return None;
            }
        }
        center
    }

    /// The floor the airborne controller stands feet here on, as its step
    /// finds it: under the footprint, from just above the feet to a little
    /// below them. None means the next step falls.
    #[must_use]
    pub fn support(&self, feet: Vec3) -> Option<f32> {
        self.support_height(feet, 0.01, 0.05)
    }

    /// Samples support under the capsule footprint, including contact before its center reaches a stair.
    fn support_height(&self, position: Vec3, step_up: f32, drop: f32) -> Option<f32> {
        let floor = SUPPORT_OFFSETS
            .into_iter()
            .filter_map(|(x, z)| self.ground(position + Vec3::new(x, 0.0, z), step_up, drop))
            .max_by(f32::total_cmp);
        self.observe_support(floor.is_some());
        floor
    }

    /// Walks a displacement in strides shorter than a stair tread, so a sample that
    /// crosses several steps climbs or descends them one at a time. The whole walk
    /// stays within one sample's grounded height budget, which the network guard
    /// enforces; a flight too steep for it continues on the next sample.
    fn walk(&self, feet: Vec3, displacement: Vec3, height: f32, support: Support) -> Vec3 {
        self.walk_with_guard(feet, displacement, height, support, LiquidGuard::All)
    }

    fn walk_with_guard(
        &self,
        feet: Vec3,
        displacement: Vec3,
        height: f32,
        support: Support,
        liquids: LiquidGuard,
    ) -> Vec3 {
        const STRIDE: f32 = 0.5;
        if !feet.is_finite() || !displacement.is_finite() || !height.is_finite() || height < 1.0 {
            return feet;
        }
        // Feet in deep water or lava now may go anywhere, so they can leave.
        let guarded = !self.inside_guarded_liquid(feet, height, liquids);
        if matches!(support, Support::Airborne { .. }) {
            // In the air, a move whose fall would end in deep water or lava is not made.
            return self
                .guarded_stride(feet, displacement, height, support, liquids, guarded)
                .unwrap_or(feet);
        }
        let horizontal = Vec3::new(displacement.x, 0.0, displacement.z);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped.
        let count = (horizontal.length() / STRIDE).ceil().clamp(1.0, 64.0) as u8;
        let part = horizontal / f32::from(count);
        let mut position = feet;
        for _ in 0..count {
            let next = self.guarded_stride(position, part, height, support, liquids, guarded);
            let wet = next.is_none();
            let next =
                next.unwrap_or_else(|| self.dry_part(position, part, height, support, liquids));
            // A walk on the ground keeps to a riser a sample up and down. One
            // the airborne controller makes (a step's) descends a stride at a
            // time within its drop, and past it the controller falls, so only
            // its climb is held, to the step's.
            let rise = next.y - feet.y;
            let (held, limit) = match support {
                Support::Optional { climb, .. } => (rise, climb),
                _ => (rise.abs(), MAX_GROUNDED_STEP),
            };
            if held > limit + 0.001 {
                break;
            }
            // Lateral NPC walls and overhead support are separate queries.
            // Keep the last good stride when the body's headroom is refused.
            if !self.carrier_headroom(next, height) {
                break;
            }
            let moved = next.distance_squared(position) > 1e-10;
            position = next;
            if !moved
                || wet
                || liquids == LiquidGuard::Lava && self.in_swimmable_water(position, height)
            {
                break;
            }
        }
        position
    }

    /// One stride: to the ground under its end, sliding along walls, stepping up
    /// a riser the capsule can clear, and stepping down across then down.
    #[allow(clippy::too_many_lines)]
    fn stride(
        &self,
        feet: Vec3,
        displacement: Vec3,
        height: f32,
        support: Support,
        liquids: LiquidGuard,
        check_lava: bool,
    ) -> Result<Vec3, LavaContact> {
        let entry = |from, to| {
            let water = if liquids == LiquidGuard::Lava {
                self.water_entry(from, to, height)
            } else {
                None
            };
            // Only the portion reached by the solid sweep and before any
            // transition to swimming is part of this movement leg.
            if check_lava && !self.clear(from, water.unwrap_or(to), height, LiquidGuard::Lava) {
                return Err(LavaContact);
            }
            Ok(water)
        };
        let mut position = feet;
        let mut remaining = Vec3::new(displacement.x, 0.0, displacement.z);
        let purpose = if matches!(support, Support::Airborne { .. }) {
            SweepPurpose::Body
        } else {
            SweepPurpose::Walking
        };
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
                Support::Optional { drop, climb } => {
                    match self.support_height(probe, climb, drop) {
                        Some(ground) => ground,
                        // Ground under the stride's end that rises past the
                        // step's climb stops it as a wall does: no stride walks
                        // the feet into a face (6.344).
                        None if self.ground(probe, height, 0.0).is_some() => break,
                        // Past the drop, across at the feet's height, the fall
                        // to follow.
                        None => position.y,
                    }
                }
                Support::Airborne { .. } => position.y,
            };
            let target = Vec3::new(position.x + remaining.x, ground, position.z + remaining.z);
            // Going down, move across at the current height and then down, so the
            // capsule never cuts the edge it steps off and the feet never stop at
            // a height between the two floors.
            let descending = !matches!(support, Support::Airborne { .. }) && target.y < position.y;
            let delta = if descending {
                Vec3::new(remaining.x, 0.0, remaining.z)
            } else {
                target - position
            };
            if delta.length_squared() < 0.000_001 {
                if descending {
                    let settled = self.settle(position, target.y, height);
                    position = entry(position, settled)?.unwrap_or(settled);
                }
                break;
            }
            match self.sweep_for(position, delta, height, purpose) {
                Ok(None) => {
                    let swept = position + delta;
                    if let Some(water) = entry(position, swept)? {
                        return Ok(water);
                    }
                    if descending {
                        let settled = self.settle(swept, target.y, height);
                        return Ok(entry(swept, settled)?.unwrap_or(settled));
                    }
                    return Ok(target);
                }
                Ok(Some(hit)) => {
                    // In the air, an edge whose top is within the clearance
                    // is passed over and stood on where the move ends on it,
                    // as the program's floor refresh under the end point
                    // finds it. A top the move passes beyond (a thin rail)
                    // lifts nothing: the face stops the body as before.
                    if let Support::Airborne { clear } = support
                        && clear > 0.0
                        && let Some(top) = self
                            .support_height(Vec3::new(target.x, position.y, target.z), clear, 0.0)
                            .filter(|top| *top > position.y + 0.001)
                    {
                        let onto = Vec3::new(target.x, top, target.z);
                        if self.can_step_up(position, onto, height, purpose) {
                            let lifted = Vec3::new(position.x, top, position.z);
                            if let Some(water) = entry(position, lifted)? {
                                return Ok(water);
                            }
                            return Ok(entry(lifted, onto)?.unwrap_or(onto));
                        }
                    }
                    if !matches!(support, Support::Airborne { .. })
                        && target.y > position.y
                        && self.can_step_up(position, target, height, purpose)
                    {
                        let lifted = Vec3::new(position.x, target.y, position.z);
                        if let Some(water) = entry(position, lifted)? {
                            return Ok(water);
                        }
                        return Ok(entry(lifted, target)?.unwrap_or(target));
                    }
                    let fraction = (hit.time_of_impact - 0.005 / delta.length()).clamp(0.0, 1.0);
                    let reached = position + delta * fraction;
                    if let Some(water) = entry(position, reached)? {
                        return Ok(water);
                    }
                    position = reached;
                    // A mostly vertical contact is a floor or ceiling edge, not a
                    // wall to slide along; normalizing its small horizontal part
                    // would cancel all forward motion.
                    if hit.normal1.y.abs() > 0.7 {
                        break;
                    }
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
        Ok(position)
    }

    /// Lowers the feet onto `floor`, or onto whatever the capsule meets first. A
    /// wall beside the drop (such as the riser just stepped off, within the sweep's
    /// contact margin) does not hold the character up.
    fn settle(&self, feet: Vec3, floor: f32, height: f32) -> Vec3 {
        let drop = Vec3::Y * (floor - feet.y);
        let landed = Vec3::new(feet.x, floor, feet.z);
        match self.sweep(feet, drop, height) {
            Ok(None) => landed,
            Ok(Some(hit)) => {
                let beside_a_wall = hit.normal1.y.abs() < 0.7
                    && self
                        .clearance(landed, height, 0.0)
                        .is_none_or(|distance| distance >= 0.0);
                if beside_a_wall || drop.length() == 0.0 {
                    landed
                } else {
                    let fraction = (hit.time_of_impact - 0.005 / drop.length()).clamp(0.0, 1.0);
                    feet + drop * fraction
                }
            }
            Err(_) => feet,
        }
    }

    /// Clears a stair riser by checking both the upward and elevated horizontal sweeps.
    fn can_step_up(&self, feet: Vec3, target: Vec3, height: f32, purpose: SweepPurpose) -> bool {
        let lift = Vec3::Y * (target.y - feet.y);
        matches!(self.sweep(feet, lift, height), Ok(None))
            && matches!(
                self.sweep_for(feet + lift, target - feet - lift, height, purpose),
                Ok(None)
            )
    }

    /// [`Self::sweep`] for a fall straight down, which a body whose upper
    /// part starts inside something over it, such as a swimmer surfaced
    /// under a ramp into the water, still makes: going down leaves what the
    /// head overlaps, and only what the lower body meets can stop it. The
    /// lower body leads, halved until its sweep isn't refused where it
    /// starts. Ours: the program's sphere differs (R40) and this case is
    /// unread.
    fn fall_sweep(
        &self,
        feet: Vec3,
        drop: f32,
        height: f32,
    ) -> Result<Option<parry3d::query::ShapeCastHit>, parry3d::query::Unsupported> {
        let delta = Vec3::Y * drop;
        let hit = self.sweep(feet, delta, height)?;
        if drop >= 0.0 || hit.as_ref().is_none_or(|hit| hit.time_of_impact > 0.0) {
            return Ok(hit);
        }
        let mut lower = height / 2.0;
        while lower >= 2.5 * CAPSULE_RADIUS {
            let below = self.sweep(feet, delta, lower)?;
            if below.as_ref().is_none_or(|hit| hit.time_of_impact > 0.0) {
                return Ok(below);
            }
            lower /= 2.0;
        }
        Ok(hit)
    }

    /// Sweeps one segment with a skin gap, keeping the collision query separate from sliding.
    fn sweep(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Option<parry3d::query::ShapeCastHit>, parry3d::query::Unsupported> {
        self.sweep_for(feet, delta, height, SweepPurpose::Body)
    }

    fn sweep_for(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
        purpose: SweepPurpose,
    ) -> Result<Option<parry3d::query::ShapeCastHit>, parry3d::query::Unsupported> {
        Ok(self
            .labelled_sweep_for(feet, delta, height, purpose)?
            .map(|(_, hit)| hit))
    }

    /// The nearest wall hit with what it belongs to.
    fn labelled_sweep(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
    ) -> Result<Option<(Obstacle, parry3d::query::ShapeCastHit)>, parry3d::query::Unsupported> {
        self.labelled_sweep_for(feet, delta, height, SweepPurpose::Body)
    }

    fn labelled_sweep_for(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
        purpose: SweepPurpose,
    ) -> Result<Option<(Obstacle, parry3d::query::ShapeCastHit)>, parry3d::query::Unsupported> {
        let result = self.labelled_sweep_inner(feet, delta, height, purpose);
        self.observe_sweep(&result);
        result
    }

    fn labelled_sweep_inner(
        &self,
        feet: Vec3,
        delta: Vec3,
        height: f32,
        purpose: SweepPurpose,
    ) -> Result<Option<(Obstacle, parry3d::query::ShapeCastHit)>, parry3d::query::Unsupported> {
        // A sweep that starts inside the margin (or overlapping) is refused only when it
        // goes deeper; one that stops there would pin the character in every direction.
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: SKIN,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        let (pose, capsule) = body(feet, self.collision_height.unwrap_or(height));
        let mut nearest: Option<(Obstacle, parry3d::query::ShapeCastHit)> = None;
        for (obstacle, mesh) in self.labelled_wall_meshes_for(purpose) {
            let hit = cast_shapes(
                &pose,
                Vector::from_array(delta.to_array()),
                &capsule,
                &mesh.pose,
                Vector::ZERO,
                mesh.mesh,
                options,
            )?
            .map(|mut hit| {
                // Keep the world-mesh witness/normal convention used before
                // placements: Parry reports shape 2 in its own local frame.
                hit.normal2 = mesh.pose.transform_vector(hit.normal2);
                hit.witness2 = mesh.pose.transform_point(hit.witness2);
                hit
            });
            if let Some(hit) = hit
                && nearest
                    .as_ref()
                    .is_none_or(|(_, previous)| hit.time_of_impact < previous.time_of_impact)
            {
                nearest = Some((obstacle, hit));
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
mod floor_support_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ducked_capsule_keeps_its_radius_and_bottom_and_never_undercuts_the_sphere() {
        let standing = 6.677_319_5;
        let height = ducked_height(3.75, 5.25, 0.65, standing);
        let (pose, capsule) = body(Vec3::ZERO, height);
        assert_eq!(capsule.radius.to_bits(), CAPSULE_RADIUS.to_bits());
        assert!((pose.translation.y - height * 0.5 - LIFT).abs() < 1e-6);
        assert!((height + LIFT - 5.25).abs() < 1e-6);
        assert!(ducked_height(3.75, 3.8, 0.65, standing) + LIFT >= 3.75 + 0.65);
        assert_eq!(
            ducked_height(f32::NAN, 5.25, 0.65, standing).to_bits(),
            standing.to_bits()
        );
    }

    #[test]
    fn a_ducked_sweep_refuses_the_exact_top_and_passes_just_above_it() {
        let standing = 6.677_319_5;
        let height = ducked_height(3.75, 5.25, 0.65, standing);
        let make = |ceiling| {
            let mut triangles = floor(0.0);
            triangles.extend([
                [
                    [0.0, ceiling, -20.0],
                    [20.0, ceiling, 20.0],
                    [20.0, ceiling, -20.0],
                ],
                [
                    [0.0, ceiling, -20.0],
                    [0.0, ceiling, 20.0],
                    [20.0, ceiling, 20.0],
                ],
            ]);
            CollisionWorld::new(triangles).unwrap()
        };
        let start = Vec3::new(-2.0, 0.0, 0.0);
        for (ceiling, passes) in [(5.25, false), (5.30, true)] {
            let world = make(ceiling);
            let ducked = world.clone().with_collision_height(height);
            let next = ducked.step(start, Vec3::X * 4.0, standing);
            assert_eq!(next.x > 1.5, passes, "{ceiling}: {next:?}");
            let unchanged = world.step(start, Vec3::X * 4.0, standing);
            assert!(unchanged.x < 0.0, "standing {ceiling}: {unchanged:?}");
        }
    }

    #[test]
    fn manual_posture_clearance_is_strict_and_also_checks_room_above_the_origin() {
        let make =
            |ceiling| CollisionWorld::new(floor(0.0).into_iter().chain(floor(ceiling))).unwrap();
        assert!(!make(8.0).posture_clearance(Vec3::Y * 3.75, 3.75, 8.0));
        assert!(make(8.001).posture_clearance(Vec3::Y * 3.75, 3.75, 8.0));
        // The total gap passes, but less than S remains above this origin.
        assert!(!make(10.0).posture_clearance(Vec3::Y * 8.0, 3.75, 8.0));
    }

    #[test]
    fn a_collision_height_override_preserves_liquid_probes_and_world_snapshots() {
        let standing = 6.677_319_5;
        let world = CollisionWorld::new(floor(0.0))
            .unwrap()
            .with_liquids(liquids::Boxes(vec![(
                Vec3::splat(-20.0),
                Vec3::new(20.0, 5.0, 20.0),
                Liquid::Water,
            )]));
        let ducked = world
            .clone()
            .with_collision_height(ducked_height(3.75, 5.25, 0.65, standing));
        assert_eq!(
            world.guard_view(Vec3::ZERO, standing),
            ducked.guard_view(Vec3::ZERO, standing)
        );
        assert_eq!(
            world.origin_in_liquid(Vec3::ZERO, standing),
            ducked.origin_in_liquid(Vec3::ZERO, standing)
        );
        assert_eq!(
            ducked.without_doors(&[]).collision_height,
            ducked.collision_height
        );
        assert_eq!(
            ducked.near(Vec3::ZERO, 20.0).collision_height,
            ducked.collision_height
        );
        assert!(world.collision_height.is_none());
    }

    fn floor(y: f32) -> Vec<[[f32; 3]; 3]> {
        vec![
            [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
            [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
        ]
    }
    /// A wall across x = `x`, two units thick, from the floor up.
    fn wall(x: f32) -> Arc<CollisionMesh> {
        let (near, far) = (x, x + 2.0);
        Arc::new(
            CollisionMesh::new([
                [[near, 0.0, -10.0], [near, 20.0, -10.0], [near, 20.0, 10.0]],
                [[near, 0.0, -10.0], [near, 20.0, 10.0], [near, 0.0, 10.0]],
                [[far, 0.0, -10.0], [far, 20.0, 10.0], [far, 20.0, -10.0]],
                [[far, 0.0, -10.0], [far, 0.0, 10.0], [far, 20.0, 10.0]],
            ])
            .unwrap(),
        )
    }

    #[test]
    fn a_contact_names_the_nearest_door_carrier_or_nothing() {
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        world.set_door_obstacles(vec![(7, wall(8.0))]);
        world.set_spawn_obstacles(vec![(50, wall(4.0))]);
        let feet = Vec3::new(0.0, 0.0, 0.0);
        let contact = world
            .first_contact(feet, Vec3::X * 12.0, 6.0)
            .expect("the carrier stands nearer");
        assert_eq!(contact.obstacle, Obstacle::Carrier(50));
        assert!(contact.normal.x > 0.9, "{:?}", contact.normal);
        assert!((0.0..1.0).contains(&contact.time_of_impact));
        world.set_spawn_obstacles(Vec::new());
        assert_eq!(
            world
                .first_contact(feet, Vec3::X * 12.0, 6.0)
                .map(|c| c.obstacle),
            Some(Obstacle::Door(7))
        );
        assert!(world.first_contact(feet, -Vec3::X * 5.0, 6.0).is_none());
        assert!(world.first_contact(feet, Vec3::ZERO, 6.0).is_none());
    }

    #[test]
    fn placement_checks_require_a_nearby_surface_and_no_lower_floor() {
        let level = CollisionWorld::new(floor(0.0)).unwrap();
        let below = Vec3::Y * -5.0;
        assert!(level.floor_above_placement(below, 1.0, 7.0).unwrap().abs() < 0.001);
        assert!(level.floor_above_placement(below, 1.0, 4.0).is_none());
        // A surface already in the native probe's reach stays on the
        // ordinary collision path.
        assert!(
            level
                .floor_above_placement(Vec3::Y * -0.5, 1.0, 7.0)
                .is_none()
        );
        assert!(
            level
                .floor_above_placement(Vec3::Y * 12.0, 1.0, 7.0)
                .is_none()
        );
        assert!(
            level
                .floor_above_placement(below + Vec3::X * 20.0, 1.0, 7.0)
                .is_none()
        );

        let layers = CollisionWorld::new(floor(-18.0).into_iter().chain(floor(0.0))).unwrap();
        assert!(layers.floor_above_placement(below, 1.0, 7.0).is_none());
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
    fn pressing_or_sliding_into_a_door_never_releases_it() {
        // A box of door walls 8 across; the player walks from its middle
        // straight at a wall, into a corner, sliding shallowly along a wall
        // and wobbling sideways, at strides from 0.01 to 3, and is never left
        // overlapping it, at their own height or a taller or shorter one.
        let mut triangles = Vec::new();
        for (a, b) in [
            ([-4.0, -4.0], [4.0, -4.0]),
            ([4.0, -4.0], [4.0, 4.0]),
            ([4.0, 4.0], [-4.0, 4.0]),
            ([-4.0, 4.0], [-4.0, -4.0]),
        ] {
            let low = |p: [f32; 2]| [p[0], 0.0, p[1]];
            let high = |p: [f32; 2]| [p[0], 12.0, p[1]];
            triangles.push([low(a), low(b), high(b)]);
            triangles.push([low(a), high(b), high(a)]);
        }
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        world.set_door_obstacles_with(vec![(
            1,
            Arc::new(CollisionMesh::new(triangles).unwrap()),
            Solidity::BOTH,
        )]);
        let directions = [
            Vec3::X,
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.05),
            Vec3::new(0.05, 0.0, -1.0),
            Vec3::new(-1.0, 0.0, 0.3),
        ];
        for direction in directions {
            // Each stride with at least 200 steps, and enough to cross the box.
            for (stride, steps) in [
                (0.01, 600),
                (0.1, 200),
                (0.5, 200),
                (1.0, 200),
                (2.0, 200),
                (3.0, 200),
            ] {
                let mut feet = Vec3::ZERO;
                for step in 0..steps {
                    // Every other step wobbles sideways.
                    let wobble = if step % 2 == 0 { 0.3 } else { -0.3 };
                    let toward = (direction + Vec3::new(-direction.z, 0.0, direction.x) * wobble)
                        .normalize()
                        * stride;
                    feet = world.step(feet, toward, 6.0);
                    for height in [3.0, 6.0, 6.5, 9.0] {
                        assert!(
                            world.releasing_overlapped_doors(feet, height).is_none(),
                            "{direction} by {stride}, step {step}, at {feet} for {height}"
                        );
                    }
                }
                // The walk did reach the walls: within a radius and the skin.
                assert!(
                    feet.x.abs().max(feet.z.abs()) > 3.5,
                    "{direction} by {stride}: {feet}"
                );
            }
        }
    }

    #[test]
    fn a_door_that_moved_into_the_body_lets_it_walk_out_but_still_holds_it_up() {
        // A box of walls 8 across, a door, dropped around a player at the
        // origin, who could not otherwise take a step.
        let walls = |half: f32| {
            let mut triangles = Vec::new();
            for (a, b) in [
                ([-half, -half], [half, -half]),
                ([half, -half], [half, half]),
                ([half, half], [-half, half]),
                ([-half, half], [-half, -half]),
            ] {
                let low = |p: [f32; 2]| [p[0], 0.0, p[1]];
                let high = |p: [f32; 2]| [p[0], 10.0, p[1]];
                triangles.push([low(a), low(b), high(b)]);
                triangles.push([low(a), high(b), high(a)]);
            }
            Arc::new(CollisionMesh::new(triangles).unwrap())
        };
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        let feet = Vec3::new(3.8, 0.0, 0.0);
        world.set_door_obstacles_with(vec![(1, walls(4.0), Solidity::BOTH)]);
        assert!(world.step(feet, Vec3::X, 6.0).x - feet.x < 0.01);
        let released = world.releasing_overlapped_doors(feet, 6.0).unwrap();
        assert!(released.step(feet, Vec3::X, 6.0).x - feet.x > 0.9);
        // Clear of it, or only standing on a door's floor, nothing changes.
        assert!(world.releasing_overlapped_doors(Vec3::ZERO, 6.0).is_none());
        let mut deck = CollisionWorld::new(floor(0.0)).unwrap();
        deck.set_door_obstacles_with(vec![(
            1,
            Arc::new(CollisionMesh::new(floor(1.0)).unwrap()),
            Solidity::BOTH,
        )]);
        assert!(deck.releasing_overlapped_doors(Vec3::Y, 6.0).is_none());
    }

    #[test]
    fn a_portcullis_coming_down_on_a_player_under_it_lets_them_walk_out_either_way() {
        // Bars of one plane across x = 0, every face a wall (a door's
        // passable faces too), come down around a player standing in the
        // gateway: they may step out on either side, and once clear the bars
        // stop them again (reviewer W2, recording checklist 6.439).
        let bars = Arc::new(
            CollisionMesh::new(vec![
                [[0.0, 0.0, -5.0], [0.0, 0.0, 5.0], [0.0, 10.0, 5.0]],
                [[0.0, 0.0, -5.0], [0.0, 10.0, 5.0], [0.0, 10.0, -5.0]],
            ])
            .unwrap(),
        );
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        world.set_door_obstacles_with(vec![(
            1,
            bars,
            Solidity {
                floor: false,
                wall: true,
            },
        )]);
        // Unreleased, the one-sided plane would hold them on its back side.
        assert!(world.step(Vec3::ZERO, Vec3::X * 3.0, 6.0).x < 0.01);
        let released = world.releasing_overlapped_doors(Vec3::ZERO, 6.0).unwrap();
        for direction in [Vec3::X, -Vec3::X] {
            let out = released.step(Vec3::ZERO, direction * 3.0, 6.0);
            assert!(out.distance(direction * 3.0) < 0.01, "stepped out to {out}");
        }
        let clear = Vec3::new(3.0, 0.0, 0.0);
        assert!(world.releasing_overlapped_doors(clear, 6.0).is_none());
        assert!(world.step(clear, -Vec3::X * 6.0, 6.0).x > 0.0);
    }

    #[test]
    fn a_door_holds_the_player_up_or_stops_them_only_as_its_solidity_says() {
        let wall = || {
            Arc::new(
                CollisionMesh::new([
                    [[0.5, 0.0, -10.0], [0.5, 10.0, -10.0], [0.5, 10.0, 10.0]],
                    [[0.5, 0.0, -10.0], [0.5, 10.0, 10.0], [0.5, 0.0, 10.0]],
                ])
                .unwrap(),
            )
        };
        let deck = || Arc::new(CollisionMesh::new(floor(1.0)).unwrap());
        let mut world = CollisionWorld::new(floor(0.0)).unwrap();
        let only = |floor, wall| Solidity { floor, wall };
        world.set_door_obstacles_with(vec![
            (1, deck(), only(true, false)),
            (2, wall(), only(false, true)),
        ]);
        assert!((world.ground(Vec3::Y * 1.5, 0.8, 1.5).unwrap() - 1.0).abs() < 0.001);
        assert_eq!(world.ground_door(Vec3::Y * 1.5, 0.8, 1.5), Some(1));
        assert!(world.step(Vec3::Y * 1.0, Vec3::X, 6.0).x < 0.5);
        // A deck that is no floor holds no one up, and a wall that is no
        // wall stops no one; the zone below still does.
        world.set_door_obstacles_with(vec![
            (1, deck(), only(false, true)),
            (2, wall(), only(true, false)),
        ]);
        assert!(world.ground(Vec3::Y * 1.5, 0.8, 1.0).is_none());
        assert_eq!(world.ground_door(Vec3::Y * 0.5, 0.8, 1.5), None);
        assert!(world.ground(Vec3::Y * 0.5, 0.8, 1.5).unwrap().abs() < 0.001);
        assert!(world.step(Vec3::Y * 1.0, Vec3::X, 6.0).x > 0.9);
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
        let request = eq_network_game::movement::MovementRequest::new(
            eq_network_game::movement::MovementMode::Forward,
            7,
            crate::world_position(position.to_array(), 0.0),
            now,
        );
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
    fn a_face_at_any_angle_under_the_feet_is_floor_at_its_height() {
        // Faces of 30, 55 and 80 degrees, rising along +x from x 0: each is
        // floor straight under the feet, as both programs' floor finders
        // take it, discarding the normal (Decompile R-s, 11-01).
        for degrees in [30.0_f32, 55.0, 80.0] {
            let rise = degrees.to_radians().tan() * 10.0;
            let world = CollisionWorld::new([
                [[0.0, 0.0, -20.0], [10.0, rise, 20.0], [10.0, rise, -20.0]],
                [[0.0, 0.0, -20.0], [0.0, 0.0, 20.0], [10.0, rise, 20.0]],
            ])
            .unwrap();
            let at = Vec3::new(5.0, rise * 0.5, 0.0);
            for floor in [
                world.ground(at, 0.1, 0.1),
                world.nearest_ground(at, 0.1, 0.1),
            ] {
                assert!(
                    floor.is_some_and(|y| (y - at.y).abs() < 0.001),
                    "{degrees}: {floor:?}"
                );
            }
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
            [[0.0, 1.0, -10.0], [10.0, 1.0, 10.0], [10.0, 1.0, -10.0]],
            [[0.0, 1.0, -10.0], [0.0, 1.0, 10.0], [10.0, 1.0, 10.0]],
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

    /// The walk a step on the ground makes, as the airborne controller
    /// calls it: a fixed 3.15-unit climb for these fixtures and the
    /// on-ground tolerance.
    const STANDING: Support = Support::Optional {
        drop: 0.501,
        climb: 3.15,
    };

    /// A face rising toward -x at `degrees` through the origin.
    fn steep_face(degrees: f32) -> CollisionWorld {
        let rise = 20.0 * degrees.to_radians().tan();
        CollisionWorld::new(quad(
            [20.0, -rise, -20.0],
            [-20.0, rise, -20.0],
            [-20.0, rise, 20.0],
            [20.0, -rise, 20.0],
        ))
        .unwrap()
    }

    #[test]
    fn a_step_climbs_a_steep_face_at_pace_until_its_reach_runs_out() {
        // No angle stops a step on a face: the programs take a floor at any
        // angle (R-z), and what one step climbs is its reach alone (6.344).
        // The capsule, lifted over the feet, never touches a face below
        // about 70°, so a run (1.6 a step) and a walk (0.6) go at full pace
        // while the rise they need is within the 3.15, and past it go only
        // as far as the reach allows. Feet left inside the face, as a
        // server's move can leave them, climb the same.
        for degrees in [45.0_f32, 50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 80.0, 85.0] {
            let world = steep_face(degrees);
            let slope = degrees.to_radians().tan();
            for (pace, sunk) in [(1.6_f32, 0.0_f32), (0.6, 0.0), (1.6, 1.0), (0.6, 1.0)] {
                // On the floor the footprint stands on: the face under its
                // uphill edge.
                let feet = Vec3::new(0.0, SUPPORT_SPREAD * slope - sunk, 0.0);
                let next = world.walk(feet, -Vec3::X * pace, 6.0, STANDING);
                let (moved, rise) = (feet.x - next.x, next.y - feet.y);
                let case = format!("{degrees}° at {pace}, {sunk} in: moved {moved}, rose {rise}");
                assert!(rise <= 3.15 + 0.001, "{case}");
                // Never under the face beneath the feet.
                assert!(next.y >= -next.x * slope - sunk - 0.001, "{case}");
                if (pace * slope + sunk) <= 3.15 {
                    assert!((moved - pace).abs() < 0.001, "{case}");
                } else {
                    assert!(moved < pace - 0.1, "{case}");
                }
            }
        }
        // 50° and 60° at a run: full pace, as the reach rule has them.
        for degrees in [50.0_f32, 60.0] {
            let world = steep_face(degrees);
            let feet = Vec3::new(0.0, SUPPORT_SPREAD * degrees.to_radians().tan(), 0.0);
            let next = world.walk(feet, -Vec3::X * 1.6, 6.0, STANDING);
            assert!((feet.x - next.x - 1.6).abs() < 0.001, "{degrees}°: {next}");
        }
    }

    #[test]
    fn a_step_up_a_face_that_steepens_slows_only_by_its_reach() {
        // 50° up to a crease, then a steeper face: the stride stops where
        // the rise passes the step's reach, not at the crease; at 85° a
        // 0.4 stride already rises past it, so the step stops there.
        for steep in [60.0_f32, 70.0, 80.0, 85.0] {
            let (lower, upper) = (50.0_f32.to_radians().tan(), steep.to_radians().tan());
            let top = 10.0 * upper;
            let mut triangles = quad(
                [0.0, 0.0, -20.0],
                [-10.0, top, -20.0],
                [-10.0, top, 20.0],
                [0.0, 0.0, 20.0],
            )
            .to_vec();
            triangles.extend(quad(
                [20.0, -20.0 * lower, -20.0],
                [0.0, 0.0, -20.0],
                [0.0, 0.0, 20.0],
                [20.0, -20.0 * lower, 20.0],
            ));
            let world = CollisionWorld::new(triangles).unwrap();
            let mut feet = Vec3::new(1.0, 0.0, 0.0);
            let mut climbed = Vec::new();
            for _ in 0..6 {
                let next = world.walk(feet, -Vec3::X * 1.6, 6.0, STANDING);
                assert!(
                    next.y - feet.y <= 3.15 + 0.001,
                    "{steep}°: {feet} to {next}"
                );
                climbed.push(feet.x - next.x);
                feet = next;
            }
            // Past the crease each step moves on, as far as the reach allows.
            if steep < 85.0 {
                assert!(feet.x < -0.2, "{steep}°: {climbed:?}");
            } else {
                assert!(feet.x > -0.2, "{steep}°: {climbed:?}");
            }
            let reach = 3.15 / upper;
            assert!(
                climbed[2..]
                    .iter()
                    .all(|moved| *moved <= reach.min(1.6) + 0.001),
                "{steep}°: {climbed:?} within {reach}"
            );
        }
    }

    #[test]
    fn a_step_stops_at_a_wall_whatever_its_reach() {
        // A vertical face taller than the reach is a wall: the step stops
        // at it, and moving on along it slides.
        let wall = quad(
            [-1.0, 0.0, -10.0],
            [-1.0, 20.0, -10.0],
            [-1.0, 20.0, 10.0],
            [-1.0, 0.0, 10.0],
        );
        let world = CollisionWorld::new(floor(0.0).into_iter().chain(wall)).unwrap();
        let blocked = world.walk(Vec3::ZERO, -Vec3::X * 1.6, 6.0, STANDING);
        assert!(
            blocked.x > -1.0 + CAPSULE_RADIUS - 0.001 && blocked.y.abs() < 0.001,
            "{blocked}"
        );
        let again = world.walk(blocked, -Vec3::X * 1.6, 6.0, STANDING);
        assert!(again.x > -1.0 + CAPSULE_RADIUS - 0.001, "{again}");
        let along = world.walk(blocked, Vec3::new(-1.6, 0.0, 1.6), 6.0, STANDING);
        assert!(
            along.x > -1.0 + CAPSULE_RADIUS - 0.001 && along.z > 1.0,
            "{along}"
        );
    }

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// Open stairs rising along +x from a floor at y 0: `steps` risers of `rise`
    /// on `tread`-deep treads, the last tread running on to x 60.
    fn stairs(rise: f32, tread: f32, steps: u8) -> CollisionWorld {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [-30.0, 0.0, 30.0],
            [0.0, 0.0, 30.0],
            [0.0, 0.0, -30.0],
        )
        .to_vec();
        for step in 0..steps {
            let x = f32::from(step) * tread;
            let (low, high) = (rise * f32::from(step), rise * f32::from(step + 1));
            let end = if step + 1 == steps { 60.0 } else { x + tread };
            triangles.extend(quad(
                [x, low, -30.0],
                [x, low, 30.0],
                [x, high, 30.0],
                [x, high, -30.0],
            ));
            triangles.extend(quad(
                [x, high, -30.0],
                [x, high, 30.0],
                [end, high, 30.0],
                [end, high, -30.0],
            ));
        }
        CollisionWorld::new(triangles).unwrap()
    }

    #[test]
    fn samples_of_any_length_climb_stairs_within_the_height_budget() {
        for (rise, tread) in [(1.0, 0.6), (1.5, 1.0), (1.3, 1.9), (2.0, 1.5)] {
            let world = stairs(rise, tread, 4);
            let top = 4.0 * rise;
            for sample in [0.3, 1.5, 3.0, 7.5] {
                for start in 0..10_u8 {
                    let mut feet = Vec3::new(-3.0 + f32::from(start) * 0.13, 0.0, 0.0);
                    for _ in 0..100 {
                        let next = world.step(feet, Vec3::X * sample, 6.0);
                        assert!(
                            (next.y - feet.y).abs() <= MAX_GROUNDED_STEP + 0.001,
                            "rise {rise} tread {tread} sample {sample}: {feet:?} -> {next:?}"
                        );
                        feet = next;
                    }
                    assert!(
                        feet.x > 15.0 && (feet.y - top).abs() < 0.01,
                        "rise {rise} tread {tread} sample {sample} start {start}: {feet:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn stepping_off_a_riser_lands_on_the_floor_below() {
        for riser in [0.5, 1.0, 1.5, 2.0] {
            let mut triangles = floor(0.0);
            triangles.extend(quad(
                [-20.0, riser, -10.0],
                [-20.0, riser, 10.0],
                [0.0, riser, 10.0],
                [0.0, riser, -10.0],
            ));
            triangles.extend(quad(
                [0.0, 0.0, -10.0],
                [0.0, riser, -10.0],
                [0.0, riser, 10.0],
                [0.0, 0.0, 10.0],
            ));
            let world = CollisionWorld::new(triangles).unwrap();
            for sample in [0.5, 1.5, 3.0, 4.5] {
                for start in 0..10_u8 {
                    let mut feet = Vec3::new(-3.0 + f32::from(start) * 0.137, riser, 0.0);
                    for _ in 0..30 {
                        feet = world.step(feet, Vec3::X * sample, 6.0);
                        // Never left between the two floors.
                        assert!(
                            feet.y.abs() < 0.001 || (feet.y - riser).abs() < 0.001,
                            "riser {riser} sample {sample}: {feet:?}"
                        );
                    }
                    assert!(
                        feet.x > 5.0 && feet.y.abs() < 0.001,
                        "riser {riser}: {feet:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn diagonal_motion_and_long_frames_do_not_increase_speed() {
        let straight = displacement(Vec3::X, 6.0, 0.05);
        let diagonal = displacement(Vec3::X + Vec3::Z, 6.0, 4.0);
        assert!((straight.length() - diagonal.length()).abs() < 0.0001);
        assert!(displacement(Vec3::X, f32::NAN, 1.0).length() < 0.0001);
    }
}

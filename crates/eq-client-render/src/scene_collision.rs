//! Static zone geometry and region adapters shared by movement and navigation.
use bevy::prelude::*;
use eq_client_assets::{ZoneAsset, ZonePrimitive};
use eq_client_core::movement::navmesh::{MainIsland, NavMesh};
#[derive(Resource)]
pub(super) struct TerrainSurface(pub(super) Vec<[Vec3; 3]>);

/// Where a zone object sits; the assets crate already placed it in the
/// renderer's frame.
pub(super) fn object_transform(object: &eq_client_assets::ZoneObject) -> Transform {
    Transform {
        translation: Vec3::from_array(object.translation),
        rotation: Quat::from_array(object.rotation),
        scale: Vec3::from_array(object.scale),
    }
}

/// The zone's water and lava as its regions mark them.
pub(super) struct ZoneLiquids(pub(super) eq_client_assets::regions::ZoneRegions);

impl eq_client_core::movement::Liquids for ZoneLiquids {
    fn cache_fingerprint(&self) -> Option<u64> {
        Some(self.0.liquid_fingerprint())
    }

    fn liquid_at(&self, point: Vec3) -> Option<eq_client_core::movement::Liquid> {
        self.0.liquid_at(point.to_array()).map(movement_liquid)
    }

    fn region_known(&self, point: Vec3) -> bool {
        self.0.medium_at(point.to_array()).is_some()
    }

    fn segment_known(&self, from: Vec3, to: Vec3) -> bool {
        self.0.segment_known(from.to_array(), to.to_array())
    }

    fn passes_through(
        &self,
        from: Vec3,
        to: Vec3,
        liquid: eq_client_core::movement::Liquid,
    ) -> bool {
        self.0
            .passes_through(from.to_array(), to.to_array(), asset_liquid(liquid))
    }

    fn first_outside(
        &self,
        from: Vec3,
        to: Vec3,
        liquid: eq_client_core::movement::Liquid,
    ) -> Option<std::ops::Range<f64>> {
        self.0
            .first_outside(from.to_array(), to.to_array(), asset_liquid(liquid))
    }

    fn water_surface_exit(&self, from: Vec3, to: Vec3) -> bool {
        self.0.water_surface_exit(from.to_array(), to.to_array())
    }

    fn liquid_surface(
        &self,
        top: Vec3,
        bottom: f32,
    ) -> Option<(f32, eq_client_core::movement::Liquid)> {
        self.0
            .liquid_surface(top.to_array(), bottom)
            .map(|(surface, liquid)| (surface, movement_liquid(liquid)))
    }

    fn within(&self, low: Vec3, high: Vec3) -> Option<Box<dyn eq_client_core::movement::Liquids>> {
        let regions = self.0.within(low.to_array(), high.to_array())?;
        Some(Box::new(Self(regions)))
    }
}

fn asset_liquid(liquid: eq_client_core::movement::Liquid) -> eq_client_assets::regions::Liquid {
    use eq_client_assets::regions::Liquid;
    match liquid {
        eq_client_core::movement::Liquid::Water => Liquid::Water,
        eq_client_core::movement::Liquid::Lava => Liquid::Lava,
    }
}

/// A liquid the zone's regions mark, as movement knows it.
fn movement_liquid(liquid: eq_client_assets::regions::Liquid) -> eq_client_core::movement::Liquid {
    use eq_client_assets::regions::Liquid;
    match liquid {
        Liquid::Water => eq_client_core::movement::Liquid::Water,
        Liquid::Lava => eq_client_core::movement::Liquid::Lava,
    }
}

/// Includes solid terrain and transformed object geometry, including hidden boundaries,
/// and the zone's water and lava.
/// A zone's solid geometry as movement and the camera meet it: its own
/// collision with its placed objects', and its water and lava; None where
/// it has none that can be built. For tools that weigh a zone's shape, such
/// as the character select spot proposer.
#[must_use]
pub fn zone_collision(zone: &ZoneAsset) -> Option<eq_client_core::movement::CollisionWorld> {
    build_collision(zone)
}

/// The zone's navigation mesh as zone entry builds it (the same triangles,
/// world and settings, so one cache serves both), read from or kept in the
/// cache under `settings_directory` when one is given, for `client`'s
/// headroom test. For the select spot tools; nothing opens a window.
///
/// # Errors
/// Says why no mesh could be built.
pub fn select_spot_mesh(
    settings_directory: Option<&std::path::Path>,
    zone_name: &str,
    zone: &ZoneAsset,
    client: eq_client_assets::ui::InstalledClient,
) -> Result<NavMesh, String> {
    let triangles = collision_triangles(zone);
    let world = collision_world(zone, triangles.clone(), None);
    eq_client_core::movement::navmesh::cache::load_or_build(
        settings_directory,
        zone_name,
        &triangles,
        world.as_ref(),
        &crate::zone_navmesh::settings(client),
        eq_client_core::movement::navmesh::build_threads(),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map(|loaded| loaded.mesh)
    .ok_or_else(|| "the navigation mesh build stopped".to_owned())
}

/// The zone's main walkable area on `mesh`, its dry floor weighed as the
/// zone's regions mark water and lava ([`NavMesh::main_island`]). Our rule
/// for "where players walk".
#[must_use]
pub fn select_spot_main_island(zone: &ZoneAsset, mesh: &NavMesh) -> Option<MainIsland> {
    mesh.main_island(|at| zone.regions.liquid_at(at.to_array()).is_none())
}

/// How large an island must be, as a share of the main island's dry floor,
/// to count as walked: the mesh splits floors a player walks as one at
/// doors, lifts and pads, and can split one floor into islands of like
/// size. Ours.
const WALKED_SHARE: f32 = 0.5;

/// The islands a player walks on `mesh`: the main island and those near
/// its size ([`NavMesh::walked_islands`]).
fn walked_islands(zone: &ZoneAsset, mesh: &NavMesh) -> Vec<u32> {
    mesh.walked_islands(
        |at| zone.regions.liquid_at(at.to_array()).is_none(),
        WALKED_SHARE,
    )
}

/// Whether a player can walk to the floor under `feet` (the renderer's
/// frame): it lies on a walked island, or a walked island's drops lead
/// down to it. A floor the mesh doesn't cover (no collision under it) can't
/// be walked to.
fn walked_to(mesh: &NavMesh, walked: &[u32], feet: Vec3) -> bool {
    mesh.locate(feet).is_some_and(|(polygon, _)| {
        let island = mesh.island_of(polygon);
        walked
            .iter()
            .any(|&from| mesh.island_reaches(from, island, true))
    })
}

/// What [`check_select_spot`] finds for one spot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpotVerdict {
    /// The preview stands on a floor a player can walk to.
    Reachable,
    /// The preview stands on a floor no walk from a walked island reaches.
    Unreachable {
        /// The floor's height, as the server's z.
        z: f32,
        /// Its island's floor area across, or None off the mesh.
        island_area: Option<f32>,
    },
    /// No floor under the spot: the preview uses the fallback.
    NoFloor,
    /// The mesh has no floor at all to weigh.
    NoMainIsland,
}

impl SpotVerdict {
    /// Whether the check flags the spot: an unreachable floor unless its
    /// row is kept, and no floor whatever the row says, since the preview
    /// then falls back to the zone's centre. A mesh with no floor is
    /// unknown, not flagged.
    #[must_use]
    pub const fn flagged(self, kept: bool) -> bool {
        match self {
            Self::Reachable | Self::NoMainIsland => false,
            Self::Unreachable { .. } => !kept,
            Self::NoFloor => true,
        }
    }

    /// Whether the check's line says "main island unclear": only on a
    /// flagged spot whose main island isn't clearly the main one. Most
    /// zones' meshes are split that way, so the words help only where a
    /// spot is in question.
    #[must_use]
    pub fn says_unclear(self, kept: bool, main: Option<MainIsland>) -> bool {
        self.flagged(kept) && main.is_some_and(|main| !main.clear())
    }
}

/// Whether the floor the preview stands a spot on can be walked to, and the
/// main island it was weighed against.
#[must_use]
pub fn check_select_spot(
    zone: &ZoneAsset,
    spot: &eq_client_core::select_spots::Spot,
    mesh: &NavMesh,
) -> (SpotVerdict, Option<MainIsland>) {
    let main = select_spot_main_island(zone, mesh);
    let walked = walked_islands(zone, mesh);
    let terrain = TerrainSurface::from_primitives(&zone.primitives);
    let verdict = match (terrain.spot_feet(spot), main) {
        (None, _) => SpotVerdict::NoFloor,
        (Some(_), None) => SpotVerdict::NoMainIsland,
        (Some(feet), Some(_)) if walked_to(mesh, &walked, feet) => SpotVerdict::Reachable,
        (Some(feet), Some(_)) => SpotVerdict::Unreachable {
            z: eq_client_core::axes::to_world(feet).z,
            island_area: mesh
                .locate(feet)
                .map(|(polygon, _)| mesh.island_area(mesh.island_of(polygon))),
        },
    };
    (verdict, main)
}

/// Proposes a selection spot using the same drawn terrain that stands the
/// preview, while retaining solid boundaries and objects for clearance.
/// Collision-only shells cannot stand a preview; passable liquid surfaces
/// can, and must therefore participate in the floor and liquid tests. Only
/// floors a player can walk to on `mesh` (the zone's, as
/// [`select_spot_mesh`] builds it) are weighed: on the main island or one
/// near its size, or reached from one by drops. None where the mesh has no
/// floor.
#[must_use]
pub fn propose_select_spot(
    zone: &ZoneAsset,
    mesh: &NavMesh,
) -> Option<eq_client_core::select_spots::Spot> {
    let walked = walked_islands(zone, mesh);
    if walked.is_empty() {
        return None;
    }
    let world = zone_collision(zone)?;
    let surfaces = eq_client_core::movement::CollisionWorld::new(
        TerrainSurface::from_primitives(&zone.primitives)
            .0
            .into_iter()
            .map(|triangle| triangle.map(|point| point.to_array())),
    )
    .ok()?;
    let floors =
        eq_client_core::movement::CollisionWorld::new(upward_triangles(&zone.primitives)).ok()?;
    let (min, max) = zone.bounds()?;
    eq_client_core::select_spots::propose(
        &world,
        &surfaces,
        &floors,
        (Vec3::from_array(min), Vec3::from_array(max)),
        |point| zone.regions.liquid_at(point.to_array()).is_some(),
        |feet| walked_to(mesh, &walked, feet),
    )
}

/// Faces whose transformed triangle winding points upward can stand a selection
/// character; a room's downward ceiling cannot, even though ordinary collision
/// rays hit both. Retain every face in `TerrainSurface` for placement and
/// enclosure tests.
fn upward_triangles(primitives: &[ZonePrimitive]) -> Vec<[[f32; 3]; 3]> {
    primitives
        .iter()
        .flat_map(|primitive| {
            primitive
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|indices| {
                    let [a, b, c] =
                        indices.map(|index| primitive.positions.get(index as usize).copied());
                    let triangle = [
                        Vec3::from_array(a?),
                        Vec3::from_array(b?),
                        Vec3::from_array(c?),
                    ];
                    // The asset loader has already transformed WLD positions
                    // and winding through eq-client-axes. Shading normals are
                    // not an authority for which side of a floor is visible.
                    let normal = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
                    (normal.y > 0.0).then(|| triangle.map(|point| point.to_array()))
                })
        })
        .collect()
}

#[cfg(test)]
#[path = "selection_geometry_tests.rs"]
mod selection_geometry_tests;

pub(super) fn build_collision(
    zone: &ZoneAsset,
) -> Option<eq_client_core::movement::CollisionWorld> {
    collision_world(zone, collision_triangles(zone), None)
}

/// The zone's collision triangles with every placed object's: the solids the
/// walker meets, doors aside.
pub(super) fn collision_triangles(zone: &ZoneAsset) -> Vec<[[f32; 3]; 3]> {
    let mut triangles = zone.collision.clone();
    for object in &zone.objects {
        let Some(model) = zone.models.get(object.model) else {
            continue;
        };
        let transform = object_transform(object).to_matrix();
        triangles.extend(model.collision.iter().map(|triangle| {
            triangle.map(|p| transform.transform_point3(Vec3::from_array(p)).to_array())
        }));
    }
    triangles
}

pub(super) fn collision_world(
    zone: &ZoneAsset,
    triangles: Vec<[[f32; 3]; 3]>,
    swim_origin: Option<f32>,
) -> Option<eq_client_core::movement::CollisionWorld> {
    match eq_client_core::movement::CollisionWorld::new(triangles) {
        Ok(world) if let Some(origin) = swim_origin => {
            let regions = zone.regions.clone().with_titanium_liquids();
            Some(if regions.liquid_region_count() == 0 {
                world
            } else {
                world
                    .with_liquids(ZoneLiquids(regions))
                    .with_swim_origin(origin)
            })
        }
        Ok(world) if zone.regions.liquid_region_count() == 0 => Some(world),
        Ok(world) => Some(world.with_liquids(ZoneLiquids(zone.regions.clone()))),
        Err(error) => {
            warn!("Collision unavailable: {error}");
            None
        }
    }
}

impl TerrainSurface {
    pub(super) fn from_primitives(primitives: &[ZonePrimitive]) -> Self {
        let triangles = primitives
            .iter()
            .flat_map(|primitive| {
                primitive
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter_map(|indices| {
                        Some([
                            Vec3::from_array(*primitive.positions.get(indices[0] as usize)?),
                            Vec3::from_array(*primitive.positions.get(indices[1] as usize)?),
                            Vec3::from_array(*primitive.positions.get(indices[2] as usize)?),
                        ])
                    })
            })
            .collect();
        Self(triangles)
    }

    /// Where the character select preview stands a spot's character, in
    /// the renderer's frame: on the first drawn floor at or below the
    /// spot's height and a little above it, or at the spot's own height for
    /// a spot [`afloat`](eq_client_core::select_spots::Spot::afloat). None
    /// with no floor there.
    pub(super) fn spot_feet(&self, spot: &eq_client_core::select_spots::Spot) -> Option<Vec3> {
        let point = Vec3::from_array(eq_client_core::render_position(spot.position()));
        if spot.afloat {
            return Some(point);
        }
        let floor = self.height_below(
            point.x,
            point.z,
            point.y + eq_client_core::select_spots::ABOVE_FLOOR,
        )?;
        Some(Vec3::new(point.x, floor, point.z))
    }

    pub(super) fn height_below(&self, x: f32, z: f32, ceiling: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .filter(|height| *height <= ceiling)
            .max_by(f32::total_cmp)
    }

    pub(super) fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.0
            .iter()
            .filter_map(|triangle| triangle_height_at(*triangle, x, z))
            .max_by(f32::total_cmp)
    }
}

pub(super) fn triangle_height_at(
    triangle: [Vec3; 3],
    position_x: f32,
    position_z: f32,
) -> Option<f32> {
    let [vertex_a, vertex_b, vertex_c] = triangle;
    let denominator = (vertex_b.z - vertex_c.z) * (vertex_a.x - vertex_c.x)
        + (vertex_c.x - vertex_b.x) * (vertex_a.z - vertex_c.z);
    if denominator.abs() < 0.001 {
        return None;
    }
    let first = ((vertex_b.z - vertex_c.z) * (position_x - vertex_c.x)
        + (vertex_c.x - vertex_b.x) * (position_z - vertex_c.z))
        / denominator;
    let second = ((vertex_c.z - vertex_a.z) * (position_x - vertex_c.x)
        + (vertex_a.x - vertex_c.x) * (position_z - vertex_c.z))
        / denominator;
    let third = 1.0 - first - second;
    if first < -0.001 || second < -0.001 || third < -0.001 {
        return None;
    }
    Some(first * vertex_a.y + second * vertex_b.y + third * vertex_c.y)
}

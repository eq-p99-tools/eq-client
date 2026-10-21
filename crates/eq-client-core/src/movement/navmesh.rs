//! A navigation mesh over the collision triangles, for the probe that decides
//! whether routes should plan on one (probe-only; not for merge).
//!
//! rerecast builds a polygon mesh per square tile; the tiles are joined where
//! their border edges meet, and a route is an A* search over the polygons
//! followed by the funnel through the edges they share. Every leg of a route
//! is then checked with the walker's own edge tests, which stay the judge of
//! what is walkable.

use super::{CollisionWorld, MAX_GROUNDED_STEP, PathProgress, PathSearch, path};
use glam::{UVec3, Vec2, Vec3, Vec3A};
use rerecast::{AreaType, HeightfieldBuilder, PolygonNavmesh, RegionId, TriMesh};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::time::{Duration, Instant};

/// How a mesh is built.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Voxel size across, in world units.
    pub cell_size: f32,
    /// Voxel size up.
    pub cell_height: f32,
    /// A tile's side, in world units.
    pub tile: f32,
    /// The walker's radius.
    pub radius: f32,
    /// The walker's height.
    pub height: f32,
    /// The highest step the walker climbs.
    pub climb: f32,
    /// The lowest floor normal's up component the walker stands on.
    pub floor_normal_y: f32,
    /// Threads the tiles are built on.
    pub threads: usize,
}

impl Settings {
    /// The walker's own limits at `cell_size`.
    #[must_use]
    pub fn walker(cell_size: f32, tile: f32, height: f32, threads: usize) -> Self {
        Self {
            cell_size,
            cell_height: (cell_size / 2.0).min(0.1),
            tile,
            radius: super::BODY_REACH,
            height,
            climb: MAX_GROUNDED_STEP,
            floor_normal_y: 0.7,
            threads,
        }
    }
}

/// Where a build spent its time, and what it made.
#[derive(Clone, Debug, Default)]
pub struct BuildStats {
    /// Tiles with any triangles.
    pub tiles: usize,
    /// Tiles that made polygons.
    pub tiles_with_polygons: usize,
    /// Tiles rerecast refused.
    pub failed_tiles: usize,
    /// Input triangles.
    pub triangles: usize,
    /// Sorting triangles into tiles.
    pub bucket: Duration,
    /// Building the tiles.
    pub tiles_time: Duration,
    /// Joining tiles, islands and the index.
    pub join: Duration,
    /// The whole build.
    pub total: Duration,
    /// The most spans any one tile's heightfield held.
    pub peak_tile_spans: usize,
}

struct Link {
    to: u32,
    /// The shared edge's ends.
    a: Vec3,
    b: Vec3,
}

struct Polygon {
    vertices: Vec<Vec3>,
    links: Vec<Link>,
    /// Tile border edges not yet joined: (side, a, b).
    open: Vec<(u8, Vec3, Vec3)>,
    tile: (i32, i32),
}

/// A built mesh.
pub struct NavMesh {
    polygons: Vec<Polygon>,
    island: Vec<u32>,
    islands: usize,
    /// Polygons by the index grid cells their bounds cover.
    index: HashMap<(i32, i32), Vec<u32>>,
    /// How the build went.
    pub stats: BuildStats,
}

const INDEX_CELL: f32 = 8.0;

/// An edge's two ends.
type Edge = (Vec3, Vec3);
/// A tile and one of its four sides.
type TileSide = ((i32, i32), u8);
/// A route's polygons, each with the edge crossed into it.
type Corridor = Vec<(u32, Option<Edge>)>;
/// A polygon's best cost, where it was entered, and the polygon and link it was entered by.
type Reached = (f32, Vec3, Option<(u32, usize)>);

/// A leg's checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegCheck {
    /// The walk test with clearance to both sides passes.
    pub clear: bool,
    /// The plain walk test passes.
    pub walk: bool,
    /// A short grid search repairs it, when neither passes.
    pub repaired: Option<bool>,
}

impl NavMesh {
    /// Builds a mesh over `triangles`.
    ///
    /// # Panics
    ///
    /// Never for finite triangles.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn build(triangles: &[[[f32; 3]; 3]], settings: Settings) -> Self {
        let started = Instant::now();
        let mut stats = BuildStats {
            triangles: triangles.len(),
            ..BuildStats::default()
        };
        let (low, high) = triangles.iter().flatten().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(low, high), p| {
                (
                    low.min(Vec3::from_array(*p)),
                    high.max(Vec3::from_array(*p)),
                )
            },
        );
        let cs = settings.cell_size;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let tile_cells = (settings.tile / cs).round().max(8.0) as u16;
        let tile = f32::from(tile_cells) * cs;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let walkable_radius = (settings.radius / cs).ceil() as u16;
        let border = walkable_radius + 3;
        let pad = f32::from(border) * cs;
        #[allow(clippy::cast_possible_truncation)]
        let (tiles_x, tiles_z) = (
            ((high.x - low.x) / tile).ceil().max(1.0) as i32,
            ((high.z - low.z) / tile).ceil().max(1.0) as i32,
        );
        // Each triangle goes to every tile its bounds, widened by the border, touch.
        let bucketing = Instant::now();
        let mut buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, triangle) in triangles.iter().enumerate() {
            let (min, max) = triangle.iter().fold(
                (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
                |(min, max), p| {
                    (
                        min.min(Vec2::new(p[0], p[2])),
                        max.max(Vec2::new(p[0], p[2])),
                    )
                },
            );
            #[allow(clippy::cast_possible_truncation)]
            let cell = |v: f32, base: f32| ((v - base) / tile).floor() as i32;
            for tx in cell(min.x - pad, low.x).max(0)..=cell(max.x + pad, low.x).min(tiles_x - 1) {
                for tz in
                    cell(min.y - pad, low.z).max(0)..=cell(max.y + pad, low.z).min(tiles_z - 1)
                {
                    #[allow(clippy::cast_possible_truncation)]
                    buckets.entry((tx, tz)).or_default().push(i as u32);
                }
            }
        }
        stats.bucket = bucketing.elapsed();
        stats.tiles = buckets.len();
        let jobs: Vec<((i32, i32), Vec<u32>)> = buckets.into_iter().collect();
        let tiling = Instant::now();
        let threads = settings.threads.max(1);
        let chunk = jobs.len().div_ceil(threads).max(1);
        let results: Vec<Option<(PolygonNavmesh, usize)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .chunks(chunk)
                .map(|jobs| {
                    scope.spawn(move || {
                        jobs.iter()
                            .map(|&((tx, tz), ref indices)| {
                                #[allow(clippy::cast_precision_loss)]
                                let min = Vec3::new(
                                    low.x + tx as f32 * tile - pad,
                                    low.y - 1.0,
                                    low.z + tz as f32 * tile - pad,
                                );
                                let max = Vec3::new(
                                    min.x + tile + 2.0 * pad,
                                    high.y + settings.height + 1.0,
                                    min.z + tile + 2.0 * pad,
                                );
                                build_tile(triangles, indices, min, max, border, settings)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .collect()
        });
        stats.tiles_time = tiling.elapsed();
        let joining = Instant::now();
        let mut polygons = Vec::new();
        for (((tx, tz), _), result) in jobs.iter().zip(results) {
            let Some((mesh, spans)) = result else {
                stats.failed_tiles += 1;
                continue;
            };
            stats.peak_tile_spans = stats.peak_tile_spans.max(spans);
            if mesh.polygon_count() > 0 {
                stats.tiles_with_polygons += 1;
            }
            append_tile(&mut polygons, &mesh, (*tx, *tz));
        }
        join_tiles(
            &mut polygons,
            settings.cell_height.max(0.05) * 4.0 + settings.climb,
        );
        let (island, islands) = islands(&polygons);
        let index = index(&polygons);
        stats.join = joining.elapsed();
        stats.total = started.elapsed();
        Self {
            polygons,
            island,
            islands,
            index,
            stats,
        }
    }

    /// Polygons in the mesh.
    #[must_use]
    pub fn polygon_count(&self) -> usize {
        self.polygons.len()
    }

    /// Islands: sets of polygons joined to each other and nothing else.
    #[must_use]
    pub const fn island_count(&self) -> usize {
        self.islands
    }

    /// Links between polygons, each counted from both sides.
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.polygons.iter().map(|p| p.links.len()).sum()
    }

    /// The polygons in the largest island.
    #[must_use]
    pub fn largest_island(&self) -> usize {
        let mut sizes = vec![0usize; self.islands];
        for &i in &self.island {
            sizes[i as usize] += 1;
        }
        sizes.into_iter().max().unwrap_or(0)
    }

    /// Bytes the mesh holds in memory, roughly.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        let polygons: usize = self
            .polygons
            .iter()
            .map(|p| {
                std::mem::size_of::<Polygon>()
                    + p.vertices.capacity() * 12
                    + p.links.capacity() * std::mem::size_of::<Link>()
            })
            .sum();
        let index: usize = self.index.values().map(|v| v.capacity() * 4 + 32).sum();
        polygons + self.island.capacity() * 4 + index
    }

    /// The mesh as bytes, as a cache would store it.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let u32s = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
        let vec3 = |out: &mut Vec<u8>, v: Vec3| {
            for f in v.to_array() {
                out.extend_from_slice(&f.to_le_bytes());
            }
        };
        #[allow(clippy::cast_possible_truncation)]
        u32s(&mut out, self.polygons.len() as u32);
        for polygon in &self.polygons {
            #[allow(clippy::cast_possible_truncation)]
            u32s(&mut out, polygon.vertices.len() as u32);
            for &v in &polygon.vertices {
                vec3(&mut out, v);
            }
            #[allow(clippy::cast_possible_truncation)]
            u32s(&mut out, polygon.links.len() as u32);
            for link in &polygon.links {
                u32s(&mut out, link.to);
                vec3(&mut out, link.a);
                vec3(&mut out, link.b);
            }
        }
        out
    }

    /// A mesh read back from [`Self::to_bytes`], with its islands and index rebuilt.
    ///
    /// # Panics
    ///
    /// On bytes [`Self::to_bytes`] did not write.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut at = 0;
        let mut u32s = || {
            let v = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            at += 4;
            v
        };
        let mut read = Vec::new();
        let count = u32s();
        for _ in 0..count {
            let vertices = (0..u32s())
                .map(|_| {
                    Vec3::new(
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                    )
                })
                .collect();
            let links = (0..u32s())
                .map(|_| Link {
                    to: u32s(),
                    a: Vec3::new(
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                    ),
                    b: Vec3::new(
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                        f32::from_bits(u32s()),
                    ),
                })
                .collect();
            read.push(Polygon {
                vertices,
                links,
                open: Vec::new(),
                tile: (0, 0),
            });
        }
        let (island, islands) = islands(&read);
        let index = index(&read);
        Self {
            polygons: read,
            island,
            islands,
            index,
            stats: BuildStats::default(),
        }
    }

    /// The polygon under `at`, on the floor nearest its height, and that height.
    #[must_use]
    pub fn locate(&self, at: Vec3) -> Option<(u32, f32)> {
        let key = cell_of(Vec2::new(at.x, at.z));
        self.index
            .get(&key)?
            .iter()
            .filter_map(|&i| {
                let height = height_in(&self.polygons[i as usize].vertices, at)?;
                ((height - at.y).abs() <= 3.0).then_some((i, height))
            })
            .min_by(|a, b| (a.1 - at.y).abs().total_cmp(&(b.1 - at.y).abs()))
    }

    /// Whether `a` and `b` lie on one island.
    #[must_use]
    pub fn connected(&self, a: u32, b: u32) -> bool {
        self.island[a as usize] == self.island[b as usize]
    }

    /// A polygon at each end of the longest stretch of the largest island,
    /// roughly: the farthest from its first polygon, then the farthest from that.
    #[must_use]
    pub fn far_pair(&self) -> Option<(Vec3, Vec3)> {
        let mut sizes = vec![0usize; self.islands];
        for &i in &self.island {
            sizes[i as usize] += 1;
        }
        let largest = (0..self.islands).max_by_key(|&i| sizes[i])?;
        #[allow(clippy::cast_possible_truncation)]
        let members: Vec<Vec3> = (0..self.polygons.len())
            .filter(|&i| self.island[i] as usize == largest)
            .map(|i| centroid(&self.polygons[i].vertices))
            .collect();
        let far = |from: Vec3| {
            members
                .iter()
                .copied()
                .max_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
        };
        let a = far(*members.first()?)?;
        Some((a, far(a)?))
    }

    /// The straight legs from `start` to `goal` over the mesh: an A* over the
    /// polygons, then the funnel through the edges they share. None when either
    /// end is off the mesh or they lie on different islands.
    #[must_use]
    pub fn route(&self, start: Vec3, goal: Vec3) -> Option<Vec<Vec3>> {
        let (from, _) = self.locate(start)?;
        let (to, _) = self.locate(goal)?;
        if !self.connected(from, to) {
            return None;
        }
        let corridor = self.corridor(from, to, start, goal)?;
        Some(self.funnel(&corridor, start, goal))
    }

    /// The polygons from `from` to `to`, and the edge crossed into each after the first.
    fn corridor(&self, from: u32, to: u32, start: Vec3, goal: Vec3) -> Option<Corridor> {
        struct Open(f32, u32);
        impl PartialEq for Open {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }
        impl Eq for Open {}
        impl PartialOrd for Open {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for Open {
            fn cmp(&self, other: &Self) -> Ordering {
                other.0.total_cmp(&self.0)
            }
        }
        // Each polygon's best cost, the point it was entered at, and the link it came by.
        let mut best: HashMap<u32, Reached> = HashMap::new();
        best.insert(from, (0.0, start, None));
        let mut open = BinaryHeap::from([Open(start.distance(goal), from)]);
        while let Some(Open(_, at)) = open.pop() {
            if at == to {
                break;
            }
            let (cost, point, _) = best[&at];
            for (k, link) in self.polygons[at as usize].links.iter().enumerate() {
                let entry = (link.a + link.b) / 2.0;
                let next = cost
                    + point.distance(entry)
                    + if link.to == to {
                        entry.distance(goal)
                    } else {
                        0.0
                    };
                if best.get(&link.to).is_none_or(|(old, _, _)| next < *old) {
                    best.insert(link.to, (next, entry, Some((at, k))));
                    open.push(Open(next + entry.distance(goal), link.to));
                }
            }
        }
        best.get(&to)?;
        let mut corridor = Vec::new();
        let mut at = to;
        loop {
            let (_, _, came) = best[&at];
            let Some((parent, k)) = came else {
                corridor.push((at, None));
                break;
            };
            let link = &self.polygons[parent as usize].links[k];
            corridor.push((at, Some((link.a, link.b))));
            at = parent;
        }
        corridor.reverse();
        Some(corridor)
    }

    /// The simple stupid funnel over the corridor's edges, across the ground plane.
    fn funnel(
        &self,
        corridor: &[(u32, Option<(Vec3, Vec3)>)],
        start: Vec3,
        goal: Vec3,
    ) -> Vec<Vec3> {
        let flat = |v: Vec3| Vec2::new(v.x, v.z);
        // Cross product of (b - a) and (c - a): positive when c lies left of a→b.
        let cross = |a: Vec3, b: Vec3, c: Vec3| (flat(b) - flat(a)).perp_dot(flat(c) - flat(a));
        // Each edge as (left, right) for the walker crossing it.
        let mut portals: Vec<(Vec3, Vec3)> = vec![(start, start)];
        for window in corridor.windows(2) {
            let (Some((a, b)), (from, _)) = (window[1].1, window[0]) else {
                continue;
            };
            let center = centroid(&self.polygons[from as usize].vertices);
            let middle = (a + b) / 2.0;
            if cross(center, middle, a) >= 0.0 {
                portals.push((a, b));
            } else {
                portals.push((b, a));
            }
        }
        portals.push((goal, goal));
        let mut points = Vec::new();
        let (mut apex, mut left, mut right) = (start, start, start);
        let (mut left_index, mut right_index) = (0, 0);
        let mut i = 1;
        while i < portals.len() {
            let (next_left, next_right) = portals[i];
            // Tighten the right side.
            if cross(apex, right, next_right) >= 0.0 {
                if apex == right || cross(apex, left, next_right) < 0.0 {
                    right = next_right;
                    right_index = i;
                } else {
                    apex = left;
                    let apex_index = left_index;
                    points.push(apex);
                    (left, right) = (apex, apex);
                    (left_index, right_index) = (apex_index, apex_index);
                    i = apex_index + 1;
                    continue;
                }
            }
            // Tighten the left side.
            if cross(apex, left, next_left) <= 0.0 {
                if apex == left || cross(apex, right, next_left) > 0.0 {
                    left = next_left;
                    left_index = i;
                } else {
                    apex = right;
                    let apex_index = right_index;
                    points.push(apex);
                    (left, right) = (apex, apex);
                    (left_index, right_index) = (apex_index, apex_index);
                    i = apex_index + 1;
                    continue;
                }
            }
            i += 1;
        }
        if points.last() != Some(&goal) {
            points.push(goal);
        }
        points
    }
}

/// Checks each leg of `legs` from `start` with the walker's edge tests: the
/// clearance walk, the plain walk, and for a leg that fails both, a short grid
/// search between its ends. A failed leg's next starts on the floor at its end.
#[must_use]
pub fn check_legs(
    world: &CollisionWorld,
    start: Vec3,
    legs: &[Vec3],
    height: f32,
    doors: &[u8],
) -> Vec<LegCheck> {
    let mut from = start;
    legs.iter()
        .map(|&to| {
            let clear = path::clear_leg_probe(world, from, to, height);
            let walked = path::walk(world, from, Vec2::new(to.x, to.z), height)
                .filter(|end| (end.y - to.y).abs() <= MAX_GROUNDED_STEP);
            let repaired = (!clear && walked.is_none()).then(|| {
                let mut search = PathSearch::new(from, to, 1.5, height)
                    .with_doors(doors.to_vec())
                    .with_max_expansions(3000);
                loop {
                    match search.advance(world, 3000) {
                        PathProgress::Searching => (),
                        PathProgress::Found(_) => break true,
                        PathProgress::Failed => break false,
                    }
                }
            });
            from = walked.unwrap_or_else(|| {
                world
                    .ground(to, 3.0, 6.0)
                    .map_or(to, |floor| Vec3::new(to.x, floor, to.z))
            });
            LegCheck {
                clear,
                walk: walked.is_some(),
                repaired,
            }
        })
        .collect()
}

fn build_tile(
    triangles: &[[[f32; 3]; 3]],
    indices: &[u32],
    min: Vec3,
    max: Vec3,
    border: u16,
    settings: Settings,
) -> Option<(PolygonNavmesh, usize)> {
    let cs = settings.cell_size;
    let ch = settings.cell_height;
    let mut trimesh = TriMesh::default();
    for &i in indices {
        #[allow(clippy::cast_possible_truncation)]
        let base = trimesh.vertices.len() as u32;
        trimesh
            .vertices
            .extend(triangles[i as usize].iter().map(|p| Vec3A::from_array(*p)));
        trimesh.indices.push(UVec3::new(base, base + 1, base + 2));
        trimesh.area_types.push(AreaType::NOT_WALKABLE);
    }
    trimesh.mark_walkable_triangles(settings.floor_normal_y.acos());
    let mut heightfield = HeightfieldBuilder {
        aabb: rerecast::Aabb3d { min, max },
        cell_size: cs,
        cell_height: ch,
    }
    .build()
    .ok()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (walkable_height, walkable_climb) = (
        (settings.height / ch).ceil() as u16,
        (settings.climb / ch).floor() as u16,
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let walkable_radius = (settings.radius / cs).ceil() as u16;
    heightfield
        .rasterize_triangles(&trimesh, walkable_climb)
        .ok()?;
    heightfield.filter_low_hanging_walkable_obstacles(walkable_climb);
    heightfield.filter_ledge_spans(walkable_height, walkable_climb);
    heightfield.filter_walkable_low_height_spans(walkable_height);
    let spans = heightfield.allocated_spans.len();
    let mut compact = heightfield
        .into_compact(walkable_height, walkable_climb)
        .ok()?;
    compact.erode_walkable_area(walkable_radius);
    compact.build_distance_field();
    compact.build_regions(border, 8 * 8, 20 * 20).ok()?;
    let contours = compact.build_contours(
        1.3,
        walkable_radius * 8,
        rerecast::BuildContoursFlags::default(),
    );
    let mesh = contours.into_polygon_mesh(6).ok()?;
    Some((mesh, spans))
}

fn append_tile(polygons: &mut Vec<Polygon>, mesh: &PolygonNavmesh, tile: (i32, i32)) {
    let nvp = mesh.max_vertices_per_polygon as usize;
    let world = |i: u16| {
        let v = mesh.vertices[i as usize];
        Vec3::new(
            mesh.aabb.min.x + f32::from(v.x) * mesh.cell_size,
            mesh.aabb.min.y + f32::from(v.y) * mesh.cell_height,
            mesh.aabb.min.z + f32::from(v.z) * mesh.cell_size,
        )
    };
    #[allow(clippy::cast_possible_truncation)]
    let base = polygons.len() as u32;
    for p in 0..mesh.polygon_count() {
        let indices: Vec<u16> = mesh.polygons[p * nvp..(p + 1) * nvp]
            .iter()
            .take_while(|&&i| i != PolygonNavmesh::NO_INDEX)
            .copied()
            .collect();
        let vertices: Vec<Vec3> = indices.iter().map(|&i| world(i)).collect();
        let mut links = Vec::new();
        let mut open = Vec::new();
        for j in 0..indices.len() {
            let (a, b) = (vertices[j], vertices[(j + 1) % indices.len()]);
            let neighbor = mesh.polygon_neighbors[p * nvp + j];
            if neighbor == PolygonNavmesh::NO_CONNECTION {
                continue;
            }
            if neighbor & RegionId::BORDER_REGION.bits() != 0 {
                #[allow(clippy::cast_possible_truncation)]
                open.push(((neighbor & 0x3) as u8, a, b));
            } else {
                links.push(Link {
                    to: base + u32::from(neighbor),
                    a,
                    b,
                });
            }
        }
        polygons.push(Polygon {
            vertices,
            links,
            open,
            tile,
        });
    }
}

/// Joins tiles where a border edge of one overlaps the facing border edge of
/// its neighbour at about the same height.
fn join_tiles(polygons: &mut [Polygon], tolerance: f32) {
    // Sides: 0 at low x, 1 at high z, 2 at high x, 3 at low z.
    let mut by_tile: HashMap<TileSide, Vec<(u32, Edge)>> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        for &(side, a, b) in &polygon.open {
            #[allow(clippy::cast_possible_truncation)]
            by_tile
                .entry((polygon.tile, side))
                .or_default()
                .push((i as u32, (a, b)));
        }
    }
    let mut links: Vec<(u32, Link)> = Vec::new();
    for (&((tx, tz), side), edges) in &by_tile {
        let (other, facing) = match side {
            0 => ((tx - 1, tz), 2),
            1 => ((tx, tz + 1), 3),
            2 => ((tx + 1, tz), 0),
            _ => ((tx, tz - 1), 1),
        };
        let Some(theirs) = by_tile.get(&(other, facing)) else {
            continue;
        };
        // Along the shared line: z for the x sides, x for the z sides.
        let along = |v: Vec3| if side % 2 == 0 { v.z } else { v.x };
        for &(mine, (a, b)) in edges {
            let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
            for &(their, (c, d)) in theirs {
                let (clo, chi) = (along(c).min(along(d)), along(c).max(along(d)));
                let (start, end) = (lo.max(clo), hi.min(chi));
                if end - start < 0.01 {
                    continue;
                }
                let at = |p: Vec3, q: Vec3, t: f32| {
                    let span = along(q) - along(p);
                    if span.abs() < f32::EPSILON {
                        p
                    } else {
                        p.lerp(q, (t - along(p)) / span)
                    }
                };
                let (m0, m1) = (at(a, b, start), at(a, b, end));
                let (t0, t1) = (at(c, d, start), at(c, d, end));
                if (m0.y - t0.y).abs() > tolerance || (m1.y - t1.y).abs() > tolerance {
                    continue;
                }
                links.push((
                    mine,
                    Link {
                        to: their,
                        a: m0,
                        b: m1,
                    },
                ));
            }
        }
    }
    for (from, link) in links {
        polygons[from as usize].links.push(link);
    }
    for polygon in polygons.iter_mut() {
        polygon.open = Vec::new();
    }
}

fn islands(polygons: &[Polygon]) -> (Vec<u32>, usize) {
    const NONE: u32 = u32::MAX;
    let mut island = vec![NONE; polygons.len()];
    let mut count = 0u32;
    for seed in 0..polygons.len() {
        if island[seed] != NONE {
            continue;
        }
        let mut stack = vec![seed];
        island[seed] = count;
        while let Some(at) = stack.pop() {
            for link in &polygons[at].links {
                if island[link.to as usize] == NONE {
                    island[link.to as usize] = count;
                    stack.push(link.to as usize);
                }
            }
        }
        count += 1;
    }
    (island, count as usize)
}

#[allow(clippy::cast_possible_truncation)]
fn cell_of(v: Vec2) -> (i32, i32) {
    (
        (v.x / INDEX_CELL).floor() as i32,
        (v.y / INDEX_CELL).floor() as i32,
    )
}

fn index(polygons: &[Polygon]) -> HashMap<(i32, i32), Vec<u32>> {
    let mut index: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (i, polygon) in polygons.iter().enumerate() {
        let (min, max) = polygon.vertices.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(min, max), v| (min.min(Vec2::new(v.x, v.z)), max.max(Vec2::new(v.x, v.z))),
        );
        let (low, high) = (cell_of(min), cell_of(max));
        for x in low.0..=high.0 {
            for z in low.1..=high.1 {
                #[allow(clippy::cast_possible_truncation)]
                index.entry((x, z)).or_default().push(i as u32);
            }
        }
    }
    index
}

fn centroid(vertices: &[Vec3]) -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let count = vertices.len().max(1) as f32;
    vertices.iter().copied().sum::<Vec3>() / count
}

/// The height of a convex polygon's surface under `at`, from the fan triangle
/// that holds it; None when `at` is outside it.
#[allow(clippy::many_single_char_names)]
fn height_in(vertices: &[Vec3], at: Vec3) -> Option<f32> {
    let p = Vec2::new(at.x, at.z);
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    for k in 1..vertices.len().saturating_sub(1) {
        let (a, b, c) = (vertices[0], vertices[k], vertices[k + 1]);
        let (fa, fb, fc) = (flat(a), flat(b), flat(c));
        let area = (fb - fa).perp_dot(fc - fa);
        if area.abs() < 1e-6 {
            continue;
        }
        let u = (fb - p).perp_dot(fc - p) / area;
        let v = (fc - p).perp_dot(fa - p) / area;
        let w = 1.0 - u - v;
        let eps = -1e-3;
        if u >= eps && v >= eps && w >= eps {
            return Some(u * a.y + v * b.y + w * c.y);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    fn floor(y: f32, low: f32, high: f32) -> Vec<[[f32; 3]; 3]> {
        quad(
            [low, y, low],
            [low, y, high],
            [high, y, high],
            [high, y, low],
        )
        .to_vec()
    }

    fn settings() -> Settings {
        Settings::walker(0.3, 16.0, 6.0, 2)
    }

    #[test]
    fn a_route_across_tiles_goes_straight_over_open_floor() {
        let triangles = floor(0.0, -40.0, 40.0);
        let mesh = NavMesh::build(&triangles, settings());
        assert!(mesh.stats.tiles > 4, "{:?}", mesh.stats);
        assert_eq!(mesh.island_count(), 1);
        let (start, goal) = (Vec3::new(-30.0, 0.0, -30.0), Vec3::new(30.0, 0.0, 30.0));
        let legs = mesh.route(start, goal).unwrap();
        // A corridor through polygon centres may bend a hair round a vertex.
        let length: f32 = std::iter::once(start)
            .chain(legs.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum();
        assert!(length <= start.distance(goal) * 1.01, "{legs:?}");
        assert_eq!(legs.last(), Some(&goal));
        let world = CollisionWorld::new(triangles).unwrap();
        let checks = check_legs(&world, start, &legs, 6.0, &[]);
        assert!(checks.iter().all(|c| c.walk), "{checks:?}");
    }

    #[test]
    fn a_route_turns_at_a_wall_s_end() {
        let mut triangles = floor(0.0, -40.0, 40.0);
        // A wall across z = 0 from x = -40 to x = 20, ten high, a unit thick.
        for (a, b, c, d) in [
            (
                [-40.0, 0.0, -0.5],
                [20.0, 0.0, -0.5],
                [20.0, 10.0, -0.5],
                [-40.0, 10.0, -0.5],
            ),
            (
                [-40.0, 0.0, 0.5],
                [-40.0, 10.0, 0.5],
                [20.0, 10.0, 0.5],
                [20.0, 0.0, 0.5],
            ),
            (
                [20.0, 0.0, -0.5],
                [20.0, 0.0, 0.5],
                [20.0, 10.0, 0.5],
                [20.0, 10.0, -0.5],
            ),
            (
                [-40.0, 10.0, -0.5],
                [20.0, 10.0, -0.5],
                [20.0, 10.0, 0.5],
                [-40.0, 10.0, 0.5],
            ),
        ] {
            triangles.extend(quad(a, b, c, d));
        }
        let mesh = NavMesh::build(&triangles, settings());
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 20.0));
        let legs = mesh.route(start, goal).unwrap();
        assert!(legs.len() >= 2, "{legs:?}");
        assert!(legs.iter().any(|p| p.x > 20.0), "{legs:?}");
        let world = CollisionWorld::new(triangles).unwrap();
        let checks = check_legs(&world, start, &legs, 6.0, &[]);
        assert!(checks.iter().all(|c| c.walk), "{legs:?} {checks:?}");
    }

    #[test]
    fn stacked_floors_are_told_apart() {
        let mut triangles = floor(0.0, -20.0, 20.0);
        triangles.extend(floor(12.0, -10.0, 10.0));
        let mesh = NavMesh::build(&triangles, settings());
        let (low, low_y) = mesh.locate(Vec3::new(0.0, 0.0, 0.0)).unwrap();
        let (high, high_y) = mesh.locate(Vec3::new(0.0, 12.0, 0.0)).unwrap();
        assert_ne!(low, high);
        assert!(
            low_y.abs() < 0.5 && (high_y - 12.0).abs() < 0.5,
            "{low_y} {high_y}"
        );
        assert!(!mesh.connected(low, high));
    }

    /// Solid box faces, outward-facing.
    fn block(low: [f32; 3], high: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let [x0, y0, z0] = low;
        let [x1, y1, z1] = high;
        [
            quad([x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]),
            quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]),
            quad([x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1]),
            quad([x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]),
            quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]),
        ]
        .concat()
    }

    #[test]
    fn a_route_climbs_stairs_a_riser_at_a_time() {
        let mut triangles = floor(0.0, -40.0, 40.0);
        // Six steps of one unit up along +x, then a landing five high.
        for step in 0..5 {
            #[allow(clippy::cast_precision_loss)]
            let (x, y) = (step as f32 * 2.0, (step + 1) as f32);
            triangles.extend(block([x, 0.0, -5.0], [x + 2.0, y, 5.0]));
        }
        triangles.extend(block([10.0, 0.0, -5.0], [30.0, 5.0, 5.0]));
        let mesh = NavMesh::build(&triangles, settings());
        let (start, goal) = (Vec3::new(-20.0, 0.0, 0.0), Vec3::new(25.0, 5.0, 0.0));
        let legs = mesh.route(start, goal).unwrap();
        let world = CollisionWorld::new(triangles).unwrap();
        let checks = check_legs(&world, start, &legs, 6.0, &[]);
        assert!(checks.iter().all(|c| c.walk), "{legs:?} {checks:?}");
    }

    #[test]
    fn a_mesh_reads_back_from_its_bytes() {
        let triangles = floor(0.0, -40.0, 40.0);
        let mesh = NavMesh::build(&triangles, settings());
        let back = NavMesh::from_bytes(&mesh.to_bytes());
        assert_eq!(back.polygon_count(), mesh.polygon_count());
        assert_eq!(back.link_count(), mesh.link_count());
        let (start, goal) = (Vec3::new(-30.0, 0.0, -30.0), Vec3::new(30.0, 0.0, 30.0));
        assert_eq!(back.route(start, goal), mesh.route(start, goal));
    }

    /// Build times on a synthetic town (a 1,000-unit floor of 4-unit quads,
    /// with boxes for buildings), for a feel before a real zone; not a zone.
    #[test]
    #[ignore = "timing only; run with --release --ignored --nocapture"]
    #[allow(clippy::cast_precision_loss)]
    fn inspect_synthetic_build_speed() {
        let mut triangles = Vec::new();
        for i in 0..250 {
            for j in 0..250 {
                #[allow(clippy::cast_precision_loss)]
                let (x, z) = (-500.0 + i as f32 * 4.0, -500.0 + j as f32 * 4.0);
                triangles.extend(quad(
                    [x, 0.0, z],
                    [x, 0.0, z + 4.0],
                    [x + 4.0, 0.0, z + 4.0],
                    [x + 4.0, 0.0, z],
                ));
            }
        }
        for i in 0..20 {
            for j in 0..20 {
                #[allow(clippy::cast_precision_loss)]
                let (x, z) = (-480.0 + i as f32 * 48.0, -480.0 + j as f32 * 48.0);
                triangles.extend(block([x, 0.0, z], [x + 20.0, 12.0, z + 20.0]));
            }
        }
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        for cell in [0.2, 0.3, 0.5] {
            let mesh = NavMesh::build(&triangles, Settings::walker(cell, 48.0, 6.0, threads));
            println!(
                "triangles={} threads={threads} cell={cell} seconds={:.2} tiles={} polygons={} islands={} memory_mb={:.1}",
                triangles.len(),
                mesh.stats.total.as_secs_f32(),
                mesh.stats.tiles,
                mesh.polygon_count(),
                mesh.island_count(),
                mesh.memory_bytes() as f64 / 1e6
            );
        }
    }
}

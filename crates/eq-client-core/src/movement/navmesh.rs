//! A navigation mesh over a zone's collision triangles: the walkable floor as
//! convex polygons, joined where the walker can step from one to the next.
//!
//! rerecast builds the polygons a square tile at a time ([`build`]); the
//! tiles are joined where their border edges meet. A built mesh is cached by
//! its input ([`cache`]). [`MeshNavigator`] plans routes over it: an A* over
//! the polygons, the funnel through their shared edges, then every leg
//! checked by the walker's own edge tests, which stay the judge of what is
//! walkable.

mod build;
pub mod cache;
mod drops;
mod navigator;
mod route;

use super::CollisionWorld;
pub use build::{BuildReport, RefusedTile};
use glam::{Vec2, Vec3};
pub use navigator::MeshNavigator;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How a mesh is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Voxel size across, in world units.
    pub cell_size: f32,
    /// Voxel size up.
    pub cell_height: f32,
    /// A tile's side, in world units.
    pub tile: f32,
    /// The walker's radius.
    pub radius: f32,
    /// The shortest walker's height: the floor it fits under is kept.
    pub height: f32,
    /// The heights of the walker sizes the floor is tagged for, shortest
    /// first: each polygon admits the sizes its headroom fits. The first is
    /// `height`.
    pub sizes: [f32; 3],
    /// The highest step the walker climbs.
    pub climb: f32,
    /// The lowest floor normal's up component the walker stands on.
    pub floor_normal_y: f32,
}

/// Voxel size across: the finest the probe measured, and the only one whose
/// legs all passed the walker's edge tests or a short repair on real zones.
const CELL_SIZE: f32 = 0.2;
/// A tile's side: a few hundred voxels, so a tile builds in milliseconds.
const TILE: f32 = 48.0;

/// Threads a build takes: half the machine, so the frame, the engine's own
/// pools and the network keep the rest, and at most 8.
#[must_use]
pub fn build_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get() / 2)
        .clamp(1, 8)
}

impl Settings {
    /// The walker's own limits, for a body `height` tall.
    #[must_use]
    pub fn walker(height: f32) -> Self {
        Self {
            cell_size: CELL_SIZE,
            cell_height: (CELL_SIZE / 2.0).min(0.1),
            tile: TILE,
            radius: super::BODY_REACH,
            height,
            sizes: [height; 3],
            climb: super::MAX_GROUNDED_STEP,
            // The collision's own floor limit.
            floor_normal_y: 0.7,
        }
    }

    /// The walker's own limits, with the floor tagged for walkers of the
    /// three `sizes` (heights, shortest first): one build serves them all.
    ///
    /// # Panics
    ///
    /// When the sizes aren't shortest first.
    #[must_use]
    pub fn sized(sizes: [f32; 3]) -> Self {
        assert!(
            sizes[0] <= sizes[1] && sizes[1] <= sizes[2],
            "sizes shortest first: {sizes:?}"
        );
        Self {
            sizes,
            ..Self::walker(sizes[0])
        }
    }
}

/// Where a polygon meets another: the shared stretch of their edges.
#[derive(Clone, Debug, PartialEq)]
struct Link {
    to: u32,
    a: Vec3,
    b: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
struct Polygon {
    /// Corners, counter-clockwise seen from above.
    vertices: Vec<Vec3>,
    links: Vec<Link>,
    /// The tallest of the build's sizes, 0 to 2, that fits under the lowest
    /// headroom over the polygon's floor; every shorter size fits too.
    size: u8,
}

impl Polygon {
    /// A point on the polygon's floor: the corners' average, or where that
    /// lies off a polygon that bends round it, the middle of its first
    /// corner triangle; None for a polygon with no area.
    fn point(&self) -> Option<Vec3> {
        let on_floor = |corners: &[Vec3]| {
            #[allow(clippy::cast_precision_loss)] // A polygon has a few corners.
            let middle = corners.iter().sum::<Vec3>() / corners.len() as f32;
            height_in(&self.vertices, middle).map(|y| Vec3::new(middle.x, y, middle.z))
        };
        on_floor(&self.vertices).or_else(|| on_floor(self.vertices.get(..3)?))
    }
}

/// A link [`NavMesh::link_faults`] finds wrong.
#[derive(Clone, Debug)]
pub struct LinkFault {
    /// The sizes of the polygon it leaves and the one it leads to.
    pub sizes: (u8, u8),
    /// The portal's one end.
    pub a: Vec3,
    /// Its other end.
    pub b: Vec3,
    /// How far the portal lies off the far polygon's edges, seen from
    /// above, at worst.
    pub across: f32,
    /// How far it lies above or below them there, at worst.
    pub up: f32,
    /// Whether the far polygon has no link back.
    pub one_way: bool,
}

/// How far `p` lies from the nearest edge of the polygon with these
/// corners: across, seen from above, and up or down from that edge there.
fn off_boundary(vertices: &[Vec3], p: Vec3) -> (f32, f32) {
    let flat = Vec2::new(p.x, p.z);
    (0..vertices.len())
        .map(|i| {
            let (a, b) = (vertices[i], vertices[(i + 1) % vertices.len()]);
            let (a2, b2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            let along = b2 - a2;
            let t = if along.length_squared() > 0.0 {
                ((flat - a2).dot(along) / along.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (flat.distance(a2 + along * t), (a.lerp(b, t).y - p.y).abs())
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .unwrap_or((f32::INFINITY, f32::INFINITY))
}

/// A built mesh.
#[derive(Debug)]
pub struct NavMesh {
    polygons: Vec<Polygon>,
    /// Each polygon's island: the set of polygons joined to it.
    island: Vec<u32>,
    islands: usize,
    /// Polygons by the index cells their bounds cover.
    index: HashMap<(i32, i32), Vec<u32>>,
    /// The mesh's outline: the stretches of polygon edges no link shares.
    outline: Vec<Outline>,
    /// The outline's stretches by the index cells their bounds cover.
    walls: HashMap<(i32, i32), Vec<u32>>,
    /// Ways off a ledge onto a lower polygon, by the polygon they leave.
    drops: Vec<DropLink>,
    drops_from: HashMap<u32, Vec<u32>>,
    /// The islands each island reaches by its drops, directly.
    drops_to: Vec<Vec<u32>>,
}

/// A stretch of the mesh's outline, and the polygon whose edge it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Outline {
    a: Vec3,
    b: Vec3,
    polygon: u32,
}

/// A way off a ledge: walking from `top` on polygon `from` over its edge
/// lands the walker at `landing` on polygon `to`, lower down. One way only.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DropLink {
    from: u32,
    to: u32,
    top: Vec3,
    landing: Vec3,
}

/// Items by the index cells their bounds cover.
type Index = HashMap<(i32, i32), Vec<u32>>;

/// The side of an index cell, in world units.
const INDEX_CELL: f32 = 8.0;

impl NavMesh {
    /// Builds a mesh over `triangles` on `threads` threads; None when `cancel`
    /// is set before it finishes.
    #[must_use]
    pub fn build(
        triangles: &[[[f32; 3]; 3]],
        settings: &Settings,
        threads: usize,
        cancel: &AtomicBool,
    ) -> Option<(Self, BuildReport)> {
        Self::build_in(triangles, None, settings, threads, cancel)
    }

    /// [`NavMesh::build`] in `world`, the zone's own world over the same
    /// `triangles`, when there is one: its ledges' drops are tried there, and
    /// floor where the tallest size would be under water or in lava there
    /// is left out.
    #[must_use]
    pub fn build_in(
        triangles: &[[[f32; 3]; 3]],
        world: Option<&CollisionWorld>,
        settings: &Settings,
        threads: usize,
        cancel: &AtomicBool,
    ) -> Option<(Self, BuildReport)> {
        Self::build_until(triangles, world, settings, threads, &|| {
            cancel.load(Ordering::Relaxed)
        })
    }

    fn build_until(
        triangles: &[[[f32; 3]; 3]],
        world: Option<&CollisionWorld>,
        settings: &Settings,
        threads: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<(Self, BuildReport)> {
        let (polygons, mut report) =
            build::polygons(triangles, world, settings, threads, cancelled)?;
        let mesh = Self::from_parts(polygons, Vec::new());
        let started = std::time::Instant::now();
        let (links, refused) =
            // Found for the tallest size: what it can drop off, every size
            // can, and each drop is run again from the real feet.
            drops::links(&mesh, triangles, world, settings.sizes[2], threads, cancelled)?;
        report.drop_links = links.len();
        report.refused_stretches = refused;
        report.drop_elapsed = started.elapsed();
        report.elapsed += report.drop_elapsed;
        Some((mesh.with_drops(links), report))
    }

    fn from_parts(polygons: Vec<Polygon>, drops: Vec<DropLink>) -> Self {
        let (island, islands) = islands(&polygons);
        let index = index(&polygons);
        let (outline, walls) = walls(&polygons);
        Self {
            polygons,
            island,
            islands,
            index,
            outline,
            walls,
            drops: Vec::new(),
            drops_from: HashMap::new(),
            drops_to: Vec::new(),
        }
        .with_drops(drops)
    }

    /// The mesh with `drops` as its ways off ledges.
    fn with_drops(mut self, drops: Vec<DropLink>) -> Self {
        let mut drops_from: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut drops_to = vec![Vec::new(); self.islands];
        for (i, link) in drops.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // Links are counted in u32.
            drops_from.entry(link.from).or_default().push(i as u32);
            let (from, to) = (
                self.island[link.from as usize],
                self.island[link.to as usize],
            );
            let reached: &mut Vec<u32> = &mut drops_to[from as usize];
            if from != to && !reached.contains(&to) {
                reached.push(to);
            }
        }
        self.drops = drops;
        self.drops_from = drops_from;
        self.drops_to = drops_to;
        self
    }

    /// Polygons in the mesh.
    #[must_use]
    pub fn polygon_count(&self) -> usize {
        self.polygons.len()
    }

    /// Islands: sets of polygons joined to each other and to nothing else.
    #[must_use]
    pub const fn island_count(&self) -> usize {
        self.islands
    }

    /// The polygon under `at` whose floor is nearest its height, within a few
    /// units, and that floor's height.
    #[must_use]
    pub fn locate(&self, at: Vec3) -> Option<(u32, f32)> {
        self.index
            .get(&cell_of(Vec2::new(at.x, at.z)))?
            .iter()
            .filter_map(|&i| {
                let height = height_in(&self.polygons[i as usize].vertices, at)?;
                ((height - at.y).abs() <= 3.0).then_some((i, height))
            })
            .min_by(|a, b| (a.1 - at.y).abs().total_cmp(&(b.1 - at.y).abs()))
    }

    /// The floor nearest `at`'s height among the polygons under it that
    /// `keep` takes, however far from that height.
    pub(super) fn floor_among(&self, at: Vec3, keep: impl Fn(u32) -> bool) -> Option<f32> {
        self.index
            .get(&cell_of(Vec2::new(at.x, at.z)))?
            .iter()
            .filter(|&&i| keep(i))
            .filter_map(|&i| height_in(&self.polygons[i as usize].vertices, at))
            .min_by(|a, b| (a - at.y).abs().total_cmp(&(b - at.y).abs()))
    }

    /// Whether `at` lies on the mesh's outline, where the walkable floor
    /// ends, rather than only where polygons meet.
    fn on_outline(&self, at: Vec3) -> bool {
        // Within a hair across, and a step up or down.
        const NEAR: f32 = 0.05;
        const HEIGHT: f32 = 1.0;
        let point = Vec2::new(at.x, at.z);
        self.walls
            .get(&cell_of(point))
            .into_iter()
            .flatten()
            .any(|&i| {
                let Outline { a, b, .. } = self.outline[i as usize];
                let (a2, b2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
                let along = b2 - a2;
                let t = ((point - a2).dot(along) / along.length_squared()).clamp(0.0, 1.0);
                point.distance(a2 + along * t) <= NEAR && (a.lerp(b, t).y - at.y).abs() <= HEIGHT
            })
    }

    /// The size polygon `i` is tagged for: 0 for the shortest alone, up to 2
    /// for all three.
    #[must_use]
    pub fn polygon_size(&self, i: u32) -> u8 {
        self.polygons[i as usize].size
    }

    /// Every link whose portal doesn't lie on an edge of the polygon it
    /// leads to, within `horizontal` across and `vertical` up or down, or
    /// that the polygon it leads to has no link back for: a check of the
    /// build's links, for probes.
    #[must_use]
    pub fn link_faults(&self, horizontal: f32, vertical: f32) -> (usize, Vec<LinkFault>) {
        let mut count = 0;
        let mut faults = Vec::new();
        for (from, polygon) in self.polygons.iter().enumerate() {
            for link in &polygon.links {
                count += 1;
                let to = &self.polygons[link.to as usize];
                let off = |p: Vec3| off_boundary(&to.vertices, p);
                let (across, up) = [link.a, link.b, (link.a + link.b) / 2.0]
                    .into_iter()
                    .map(off)
                    .fold((0.0_f32, 0.0_f32), |(x, y), (a, b)| (x.max(a), y.max(b)));
                #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
                let one_way = !to.links.iter().any(|back| back.to == from as u32);
                if across > horizontal || up > vertical || one_way {
                    faults.push(LinkFault {
                        sizes: (polygon.size, to.size),
                        a: link.a,
                        b: link.b,
                        across,
                        up,
                        one_way,
                    });
                }
            }
        }
        (count, faults)
    }

    /// The island polygon `i` lies on. For probes.
    #[must_use]
    pub fn island_of(&self, i: u32) -> u32 {
        self.island[i as usize]
    }

    /// A point on polygon `i`'s floor, as [`far_pair`](Self::far_pair)
    /// takes it. For probes.
    #[must_use]
    pub fn polygon_point(&self, i: u32) -> Option<Vec3> {
        self.polygons[i as usize].point()
    }

    /// Whether polygons `a` and `b` lie on one island, so a walk can join them.
    #[must_use]
    pub fn connected(&self, a: u32, b: u32) -> bool {
        self.island[a as usize] == self.island[b as usize]
    }

    /// Whether a walk from polygon `a` can reach polygon `b`: on one island,
    /// or with `drops`, through islands its drops lead down to.
    #[must_use]
    pub fn reaches(&self, a: u32, b: u32, drops: bool) -> bool {
        let (from, to) = (self.island[a as usize], self.island[b as usize]);
        if from == to || !drops {
            return from == to;
        }
        let mut seen = vec![false; self.islands];
        let mut stack = vec![from];
        seen[from as usize] = true;
        while let Some(at) = stack.pop() {
            for &next in &self.drops_to[at as usize] {
                if next == to {
                    return true;
                }
                if !std::mem::replace(&mut seen[next as usize], true) {
                    stack.push(next);
                }
            }
        }
        false
    }

    /// Two points far apart on the largest island, on its floor: from any of
    /// its polygons the farthest one, then the farthest from that. For
    /// probes, where no route is given.
    #[must_use]
    pub fn far_pair(&self) -> Option<(Vec3, Vec3)> {
        self.far_pair_where(|_| true)
    }

    /// [`far_pair`](Self::far_pair) over only the polygons whose floor
    /// centre `keep` takes, on the island most of them lie on.
    #[must_use]
    pub fn far_pair_where(&self, keep: impl Fn(Vec3) -> bool) -> Option<(Vec3, Vec3)> {
        let kept: Vec<(Vec3, u32)> = self
            .polygons
            .iter()
            .zip(&self.island)
            .filter_map(|(polygon, &island)| Some((polygon.point()?, island)))
            .filter(|&(at, _)| keep(at))
            .collect();
        let mut sizes = vec![0_usize; self.islands];
        for &(_, island) in &kept {
            sizes[island as usize] += 1;
        }
        let largest = (0..self.islands)
            .filter(|&i| sizes[i] > 0)
            .max_by_key(|&i| (sizes[i], std::cmp::Reverse(i)))?;
        let on_island: Vec<Vec3> = kept
            .into_iter()
            .filter(|&(_, island)| island as usize == largest)
            .map(|(at, _)| at)
            .collect();
        let farthest = |from: Vec3| {
            on_island
                .iter()
                .copied()
                .max_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
        };
        let a = farthest(*on_island.first()?)?;
        Some((a, farthest(a)?))
    }

    /// Ways off ledges, as where each leaves its ledge and where it lands.
    pub fn drop_links(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        self.drops.iter().map(|link| (link.top, link.landing))
    }

    /// The floor's area across, summed over the polygons a walker of `size`
    /// (0 to 2) may use. For probes comparing builds.
    #[must_use]
    pub fn floor_area(&self, size: u8) -> f32 {
        self.polygons
            .iter()
            .filter(|polygon| polygon.size >= size)
            .map(|polygon| {
                let flat = |v: Vec3| Vec2::new(v.x, v.z);
                let first = flat(polygon.vertices[0]);
                polygon
                    .vertices
                    .windows(2)
                    .skip(1)
                    .map(|pair| {
                        (flat(pair[0]) - first)
                            .perp_dot(flat(pair[1]) - first)
                            .abs()
                            / 2.0
                    })
                    .sum::<f32>()
            })
            .sum()
    }

    /// The mesh a walker of `size` (0 to 2) uses: only the polygons whose
    /// headroom fits it, so an edge to one too low for it is outline, and
    /// only the drops between them.
    #[must_use]
    pub fn for_size(&self, size: u8) -> Self {
        const GONE: u32 = u32::MAX;
        let mut kept = vec![GONE; self.polygons.len()];
        let mut polygons = Vec::new();
        for (i, polygon) in self.polygons.iter().enumerate() {
            if polygon.size >= size {
                #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
                let at = polygons.len() as u32;
                kept[i] = at;
                polygons.push(polygon.clone());
            }
        }
        for polygon in &mut polygons {
            polygon.links.retain_mut(|link| {
                link.to = kept[link.to as usize];
                link.to != GONE
            });
        }
        let drops = self
            .drops
            .iter()
            .filter_map(|link| {
                let (from, to) = (kept[link.from as usize], kept[link.to as usize]);
                (from != GONE && to != GONE).then_some(DropLink { from, to, ..*link })
            })
            .collect();
        Self::from_parts(polygons, drops)
    }

    /// The mesh split for the build's three walker `sizes` (heights, shortest
    /// first, as in [`Settings::sizes`]).
    #[must_use]
    pub fn split(&self, sizes: [f32; 3]) -> SizedMeshes {
        let short = Arc::new(self.for_size(0));
        // Sizes of one height share a mesh: a plain build tags every
        // polygon for all three.
        let medium = if sizes[1] > sizes[0] {
            Arc::new(self.for_size(1))
        } else {
            Arc::clone(&short)
        };
        let tall = if sizes[2] > sizes[1] {
            Arc::new(self.for_size(2))
        } else {
            Arc::clone(&medium)
        };
        SizedMeshes {
            sizes,
            meshes: [short, medium, tall],
        }
    }
}

/// One build's meshes for its three walker sizes.
#[derive(Debug, Clone)]
pub struct SizedMeshes {
    sizes: [f32; 3],
    meshes: [Arc<NavMesh>; 3],
}

impl SizedMeshes {
    /// The size, 0 to 2, a walker `height` tall uses: the smallest at least
    /// its height. None for a walker taller than the tallest size, which
    /// stays on the grid rather than take a mesh for a shorter body.
    #[must_use]
    pub fn size_for(&self, height: f32) -> Option<u8> {
        // A hair of slack for heights measured from the same model twice.
        const SLACK: f32 = 1e-3;
        (0..3_u8).find(|&size| height <= self.sizes[size as usize] + SLACK)
    }

    /// The mesh a walker `height` tall uses, as [`SizedMeshes::size_for`]
    /// picks it.
    #[must_use]
    pub fn for_height(&self, height: f32) -> Option<Arc<NavMesh>> {
        self.size_for(height)
            .map(|size| Arc::clone(&self.meshes[size as usize]))
    }

    /// The three sizes' heights, shortest first.
    #[must_use]
    pub const fn sizes(&self) -> [f32; 3] {
        self.sizes
    }
}

fn islands(polygons: &[Polygon]) -> (Vec<u32>, usize) {
    const NONE: u32 = u32::MAX;
    let mut island = vec![NONE; polygons.len()];
    let mut count = 0;
    for seed in 0..polygons.len() {
        if island[seed] != NONE {
            continue;
        }
        island[seed] = count;
        let mut stack = vec![seed];
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

#[allow(clippy::cast_possible_truncation)] // Zone coordinates are far inside i32 cells.
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
                #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
                index.entry((x, z)).or_default().push(i as u32);
            }
        }
    }
    index
}

/// The stretches of each polygon's edges that no link of its covers, and
/// them by the index cells their bounds cover.
fn walls(polygons: &[Polygon]) -> (Vec<Outline>, Index) {
    // How near a link's end lies to an edge to count as on it, and the
    // shortest stretch kept.
    const ON: f32 = 0.02;
    const SHORTEST: f32 = 0.01;
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let mut outline = Vec::new();
    let mut walls = Index::new();
    for (owner, polygon) in polygons.iter().enumerate() {
        let corners = polygon.vertices.len();
        for corner in 0..corners {
            let (a, b) = (
                polygon.vertices[corner],
                polygon.vertices[(corner + 1) % corners],
            );
            let along = flat(b) - flat(a);
            let span = along.length();
            if span < SHORTEST {
                continue;
            }
            // Where along the edge, 0 to 1, a point lies, when it's on it.
            let on = |v: Vec3| {
                let share = (flat(v) - flat(a)).dot(along) / (span * span);
                let off = (flat(v) - flat(a)).perp_dot(along).abs() / span;
                (off <= ON && (-0.01..=1.01).contains(&share)).then_some(share.clamp(0.0, 1.0))
            };
            let mut covered: Vec<(f32, f32)> = polygon
                .links
                .iter()
                .filter_map(|link| {
                    let (first, second) = (on(link.a)?, on(link.b)?);
                    Some((first.min(second), first.max(second)))
                })
                .collect();
            covered.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut from = 0.0_f32;
            let mut stretches = Vec::new();
            for (start, end) in covered.into_iter().chain([(1.0, 1.0)]) {
                if (start - from) * span >= SHORTEST {
                    stretches.push((a.lerp(b, from), a.lerp(b, start)));
                }
                from = from.max(end);
            }
            for (p, q) in stretches {
                let (low, high) = (cell_of(flat(p).min(flat(q))), cell_of(flat(p).max(flat(q))));
                #[allow(clippy::cast_possible_truncation)] // Counted in u32.
                let i = outline.len() as u32;
                for x in low.0..=high.0 {
                    for z in low.1..=high.1 {
                        walls.entry((x, z)).or_default().push(i);
                    }
                }
                #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
                outline.push(Outline {
                    a: p,
                    b: q,
                    polygon: owner as u32,
                });
            }
        }
    }
    (outline, walls)
}

/// The height of a convex polygon's surface under `at`, from the fan triangle
/// that holds it; None when `at` is outside it.
fn height_in(vertices: &[Vec3], at: Vec3) -> Option<f32> {
    // A hair outside an edge still counts, so a point on a shared edge finds
    // a polygon on either side.
    const EDGE: f32 = -1e-3;
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let point = flat(at);
    let first = *vertices.first()?;
    vertices.windows(2).skip(1).find_map(|pair| {
        let (second, third) = (pair[0], pair[1]);
        let area = (flat(second) - flat(first)).perp_dot(flat(third) - flat(first));
        if area.abs() < 1e-6 {
            return None;
        }
        let u = (flat(second) - point).perp_dot(flat(third) - point) / area;
        let v = (flat(third) - point).perp_dot(flat(first) - point) / area;
        let w = 1.0 - u - v;
        (u >= EDGE && v >= EDGE && w >= EDGE).then_some(u * first.y + v * second.y + w * third.y)
    })
}

#[cfg(test)]
mod tests;

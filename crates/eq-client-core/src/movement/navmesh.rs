//! A navigation mesh over a zone's collision triangles: the walkable floor as
//! convex polygons, joined where the walker can step from one to the next.
//!
//! rerecast builds the polygons a square tile at a time ([`build`]); the
//! tiles are joined where their border edges meet. A built mesh is cached by
//! its input ([`cache`]). Routes plan over it in a later change; until then
//! the grid search plans every route, and whatever plans over the mesh, the
//! walker's own edge tests stay the judge of what is walkable.

mod build;
pub mod cache;

pub use build::{BuildReport, RefusedTile};
use glam::{Vec2, Vec3};
use std::collections::HashMap;
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
    /// The walker's height.
    pub height: f32,
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
            climb: super::MAX_GROUNDED_STEP,
            // The collision's own floor limit.
            floor_normal_y: 0.7,
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
}

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
        Self::build_until(triangles, settings, threads, &|| {
            cancel.load(Ordering::Relaxed)
        })
    }

    fn build_until(
        triangles: &[[[f32; 3]; 3]],
        settings: &Settings,
        threads: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<(Self, BuildReport)> {
        let (polygons, report) = build::polygons(triangles, settings, threads, cancelled)?;
        Some((Self::from_polygons(polygons), report))
    }

    fn from_polygons(polygons: Vec<Polygon>) -> Self {
        let (island, islands) = islands(&polygons);
        let index = index(&polygons);
        Self {
            polygons,
            island,
            islands,
            index,
        }
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

    /// Whether polygons `a` and `b` lie on one island, so a walk can join them.
    #[must_use]
    pub fn connected(&self, a: u32, b: u32) -> bool {
        self.island[a as usize] == self.island[b as usize]
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

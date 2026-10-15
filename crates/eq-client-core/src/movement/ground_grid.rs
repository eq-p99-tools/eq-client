//! A faster answer to the same downward ground ray over a large mesh.
//!
//! The mesh's ground plan is cut into square columns, each listing the
//! triangles whose ground-plan bounds reach it. A downward ray tests only its
//! column's triangles, with the same per-triangle routine the mesh's BVH
//! traversal ends in, so it finds the same hit without walking the tree.

use parry3d::{
    bounding_volume::Aabb,
    math::Vector,
    query::{Ray, RayCast, RayIntersection},
    shape::TriMesh,
};
use std::time::{Duration, Instant};

/// About how many triangles one column should hold.
const PER_COLUMN: f32 = 12.0;
/// The narrowest and widest a column may be, in world units.
const NARROWEST: f32 = 1.0;
const WIDEST: f32 = 16.0;
/// A triangle reaching more columns than this is kept on a short list every
/// ray tests instead, so a few huge floor triangles don't flood the index.
const WIDE_TRIANGLE: u64 = 64;
/// The most columns a grid may have, about 16 MB of offsets at most.
const MAX_COLUMNS: u64 = 4 << 20;

/// What building a grid took, for the log.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundGridStats {
    /// How many columns the ground plan was cut into.
    pub columns: usize,
    /// How many triangles the mesh has.
    pub triangles: usize,
    /// How many triangles were too wide to list per column.
    pub wide: usize,
    /// The memory the index takes beside the mesh.
    pub bytes: usize,
    /// The side of one column, in world units.
    pub column_size: f32,
    /// How long building it took.
    pub build: Duration,
}

/// A ray whose answer the grid can't be sure matches the tree's; the caller
/// asks the tree.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TreeDecides;

pub(super) struct GroundGrid {
    /// The ground-plan corner of the first column, as (x, z).
    origin: [f32; 2],
    column_size: f32,
    columns: [u32; 2],
    /// Where each column's triangles start in `triangles`; one more entry than
    /// there are columns.
    offsets: Vec<u32>,
    triangles: Vec<u32>,
    /// Triangles too wide to list per column, which every ray tests.
    wide: Vec<u32>,
    /// Each triangle's bounding box, as the mesh's tree holds it.
    boxes: Vec<Aabb>,
    /// The ground-plan bounds of the whole mesh, as (min x, min z, max x, max z).
    bounds: [f32; 4],
    stats: GroundGridStats,
}

impl GroundGrid {
    /// Indexes every triangle of `mesh`; None for an empty mesh.
    pub(super) fn new(mesh: &TriMesh) -> Option<Self> {
        let started = Instant::now();
        let count = mesh.indices().len();
        let boxes = (0..count)
            .map(|index| Some(mesh.triangle(u32::try_from(index).ok()?).local_aabb()))
            .collect::<Option<Vec<Aabb>>>()?;
        let bounds = boxes.iter().fold(
            [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            |[x0, z0, x1, z1], bounds| {
                [
                    x0.min(bounds.mins.x),
                    z0.min(bounds.mins.z),
                    x1.max(bounds.maxs.x),
                    z1.max(bounds.maxs.z),
                ]
            },
        );
        if count == 0 || bounds.iter().any(|edge| !edge.is_finite()) {
            return None;
        }
        let (width, depth) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
        #[allow(clippy::cast_precision_loss)] // Only sets a size.
        let mut column_size = (width.max(1.0) * depth.max(1.0) * PER_COLUMN / count as f32)
            .sqrt()
            .clamp(NARROWEST, WIDEST);
        let across = |size: f32| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Bounded below.
            let span = |extent: f32| ((extent / size).floor() as u64 + 1).max(1);
            (span(width), span(depth))
        };
        let (mut cols, mut rows) = across(column_size);
        while cols * rows > MAX_COLUMNS {
            column_size *= 2.0;
            (cols, rows) = across(column_size);
        }
        let mut grid = Self {
            origin: [bounds[0], bounds[1]],
            column_size,
            columns: [u32::try_from(cols).ok()?, u32::try_from(rows).ok()?],
            offsets: Vec::new(),
            triangles: Vec::new(),
            wide: Vec::new(),
            boxes: Vec::new(),
            bounds,
            stats: GroundGridStats {
                columns: usize::try_from(cols * rows).ok()?,
                triangles: count,
                wide: 0,
                bytes: 0,
                column_size,
                build: Duration::ZERO,
            },
        };
        grid.place(&boxes)?;
        grid.boxes = boxes;
        grid.stats.wide = grid.wide.len();
        grid.stats.bytes = (grid.offsets.len() + grid.triangles.len() + grid.wide.len()) * 4
            + grid.boxes.len() * std::mem::size_of::<Aabb>();
        grid.stats.build = started.elapsed();
        Some(grid)
    }

    /// Lists each triangle in every column its box reaches, or on the wide
    /// list: a count of each column's triangles, then a second pass placing
    /// them in one flat list.
    fn place(&mut self, boxes: &[Aabb]) -> Option<()> {
        let mut counts = vec![0_u32; self.stats.columns + 1];
        let mut spans = Vec::with_capacity(boxes.len());
        for (index, bounds) in boxes.iter().enumerate() {
            let (c0, r0) = self.column(bounds.mins.x, bounds.mins.z);
            let (c1, r1) = self.column(bounds.maxs.x, bounds.maxs.z);
            let reach = u64::from(c1 - c0 + 1) * u64::from(r1 - r0 + 1);
            if reach > WIDE_TRIANGLE {
                self.wide.push(u32::try_from(index).ok()?);
                spans.push(None);
                continue;
            }
            for row in r0..=r1 {
                for col in c0..=c1 {
                    counts[self.slot(col, row)] += 1;
                }
            }
            spans.push(Some((c0, r0, c1, r1)));
        }
        let mut running = 0_u32;
        for count in &mut counts {
            let here = *count;
            *count = running;
            running += here;
        }
        let mut next = counts.clone();
        self.triangles = vec![0; running as usize];
        for (index, span) in spans.into_iter().enumerate() {
            let Some((c0, r0, c1, r1)) = span else {
                continue;
            };
            for row in r0..=r1 {
                for col in c0..=c1 {
                    let slot = self.slot(col, row);
                    self.triangles[next[slot] as usize] = u32::try_from(index).ok()?;
                    next[slot] += 1;
                }
            }
        }
        self.offsets = counts;
        Some(())
    }

    pub(super) const fn stats(&self) -> GroundGridStats {
        self.stats
    }

    /// The column holding ground-plan point (x, z), clamped into the grid. The
    /// same arithmetic places triangles and rays, and it never decreases as x
    /// or z grows, so a point within a triangle's bounds lies in a column the
    /// triangle is listed in, borders included.
    fn column(&self, x: f32, z: f32) -> (u32, u32) {
        let index = |value: f32, origin: f32, count: u32| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped.
            let index = ((value - origin) / self.column_size).floor().max(0.0) as u32;
            index.min(count - 1)
        };
        (
            index(x, self.origin[0], self.columns[0]),
            index(z, self.origin[1], self.columns[1]),
        )
    }

    fn slot(&self, col: u32, row: u32) -> usize {
        row as usize * self.columns[0] as usize + col as usize
    }

    /// The nearest hit of a straight-down `ray` within `max_time_of_impact`,
    /// as the mesh's own BVH ray cast finds it, or [`TreeDecides`] when the
    /// grid can't be sure of matching it: an exact tie between triangles with
    /// different normals, or a nearest hit the tree might pass over, either of which
    /// the tree's order decides, or a ray that isn't straight down from a
    /// finite point. The caller then asks the BVH. The hit's feature is the
    /// triangle's own face, not the mesh's numbering.
    pub(super) fn cast_down(
        &self,
        mesh: &TriMesh,
        ray: &Ray,
        max_time_of_impact: f32,
    ) -> Result<Option<RayIntersection>, TreeDecides> {
        let (x, z) = (ray.origin.x, ray.origin.z);
        if ray.dir != -Vector::Y || !ray.origin.is_finite() || !max_time_of_impact.is_finite() {
            return Err(TreeDecides);
        }
        if x < self.bounds[0] || x > self.bounds[2] || z < self.bounds[1] || z > self.bounds[3] {
            return Ok(None);
        }
        let (col, row) = self.column(x, z);
        let slot = self.slot(col, row);
        let listed = &self.triangles[self.offsets[slot] as usize..self.offsets[slot + 1] as usize];
        // The tree reaches a triangle only through its bounding box, tested
        // against the best time so far, and keeps the triangle's hit only if
        // it is strictly sooner than that best. So the tree takes a triangle
        // only while the best so far is later than both its box and its hit:
        // its key. When some triangle with the nearest hit has a key sooner
        // than every other hit and the ray's end, the tree takes it whatever
        // its order, and nothing after beats it. Otherwise (a hit reported
        // outside its own box, as a ray starting in a triangle's plane can
        // give, or a box met after a nearer hit) the order decides, and the
        // ray goes to the tree.
        let mut best: Option<RayIntersection> = None;
        let mut key = f32::INFINITY;
        let mut runner = max_time_of_impact;
        let mut tied = false;
        for &index in listed.iter().chain(&self.wide) {
            // The tree's box test on the two level axes, where the ray doesn't
            // move: a triangle whose box the ray passes beside is never
            // reached, whatever it would report.
            let bounds = &self.boxes[index as usize];
            if x < bounds.mins.x || x > bounds.maxs.x || z < bounds.mins.z || z > bounds.maxs.z {
                continue;
            }
            let Some(hit) =
                mesh.triangle(index)
                    .cast_local_ray_and_get_normal(ray, max_time_of_impact, false)
            else {
                continue;
            };
            let time = hit.time_of_impact;
            let reached = bounds
                .cast_local_ray(ray, max_time_of_impact, true)
                .map_or(f32::INFINITY, |reached| reached.max(time));
            match best {
                Some(best) if time.to_bits() == best.time_of_impact.to_bits() => {
                    // Faces at the same time that agree on the normal give the
                    // same answer whichever the tree takes.
                    tied |= hit.normal.to_array().map(f32::to_bits)
                        != best.normal.to_array().map(f32::to_bits);
                    key = key.min(reached);
                }
                Some(best) if time >= best.time_of_impact => runner = runner.min(time),
                _ if time < max_time_of_impact => {
                    if let Some(best) = best {
                        runner = runner.min(best.time_of_impact);
                    }
                    best = Some(hit);
                    key = reached;
                    tied = false;
                }
                _ => {}
            }
        }
        if tied || (best.is_some() && key >= runner) {
            return Err(TreeDecides);
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::{CollisionMesh, CollisionWorld};
    use glam::Vec3;

    type Triangle = [[f32; 3]; 3];

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [Triangle; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// A flat floor of `count` × `count` squares of side `size` at height `y`.
    fn floor(x: f32, z: f32, size: f32, count: u16, y: f32) -> Vec<Triangle> {
        let mut triangles = Vec::new();
        for i in 0..count {
            for j in 0..count {
                let (x0, z0) = (x + f32::from(i) * size, z + f32::from(j) * size);
                let (x1, z1) = (x0 + size, z0 + size);
                triangles.extend(quad([x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]));
            }
        }
        triangles
    }

    fn stairs() -> Vec<Triangle> {
        let mut triangles = floor(-4.0, -4.0, 1.0, 4, 0.0);
        for step in 0..8_u8 {
            let (z0, y) = (f32::from(step), f32::from(step + 1) * 0.5);
            triangles.extend(quad(
                [0.0, y - 0.5, z0],
                [4.0, y - 0.5, z0],
                [4.0, y, z0],
                [0.0, y, z0],
            ));
            triangles.extend(quad(
                [0.0, y, z0],
                [4.0, y, z0],
                [4.0, y, z0 + 1.0],
                [0.0, y, z0 + 1.0],
            ));
        }
        triangles
    }

    fn overlapping_floors() -> Vec<Triangle> {
        let mut triangles = floor(-6.0, -6.0, 1.5, 8, 0.0);
        triangles.extend(floor(-3.0, -3.0, 0.75, 8, 3.0));
        triangles.extend(floor(-2.0, -2.0, 1.0, 4, 3.0)); // Coplanar with the upper one.
        triangles
    }

    fn slope() -> Vec<Triangle> {
        let rise = 35_f32.to_radians().tan();
        let mut triangles = Vec::new();
        for i in 0..10_u8 {
            for j in 0..10_u8 {
                let (x0, z0) = (f32::from(i), f32::from(j));
                let (x1, z1) = (x0 + 1.0, z0 + 1.0);
                triangles.extend(quad(
                    [x0, x0 * rise, z0],
                    [x1, x1 * rise, z0],
                    [x1, x1 * rise, z1],
                    [x0, x0 * rise, z1],
                ));
            }
        }
        triangles
    }

    fn wall_with_lip() -> Vec<Triangle> {
        let mut triangles = floor(-5.0, -5.0, 1.0, 10, 0.0);
        triangles.extend(quad(
            [-5.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [5.0, 4.0, 0.0],
            [-5.0, 4.0, 0.0],
        ));
        triangles.extend(quad(
            [-5.0, 4.0, 0.0],
            [5.0, 4.0, 0.0],
            [5.0, 4.0, 0.5],
            [-5.0, 4.0, 0.5],
        ));
        triangles.extend(quad(
            [-5.0, 3.6, 0.5],
            [5.0, 3.6, 0.5],
            [5.0, 4.0, 0.5],
            [-5.0, 4.0, 0.5],
        ));
        triangles
    }

    /// Fine detail beside two triangles spanning far more columns than any
    /// list holds, so they land on the wide list.
    fn large_triangles() -> Vec<Triangle> {
        let mut triangles = floor(-2.0, -2.0, 0.25, 16, 0.5);
        triangles.extend(quad(
            [-200.0, 0.0, -200.0],
            [200.0, 0.0, -200.0],
            [200.0, 0.0, 200.0],
            [-200.0, 0.0, 200.0],
        ));
        triangles
    }

    /// A rough terrain from a fixed sequence, with sides from 0.3 to 2.
    fn rough() -> Vec<Triangle> {
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            #[allow(clippy::cast_precision_loss)]
            let unit = seed as f32 / u32::MAX as f32;
            unit
        };
        let mut xs = vec![0.0_f32];
        while xs.len() < 16 {
            let last = xs[xs.len() - 1];
            xs.push(last + 0.3 + 1.7 * next());
        }
        let heights: Vec<Vec<f32>> = (0..16)
            .map(|_| (0..16).map(|_| next() * 3.0).collect())
            .collect();
        let mut triangles = Vec::new();
        for i in 0..15 {
            for j in 0..15 {
                let point = |i: usize, j: usize| [xs[i], heights[i][j], xs[j]];
                triangles.extend(quad(
                    point(i, j),
                    point(i + 1, j),
                    point(i + 1, j + 1),
                    point(i, j + 1),
                ));
            }
        }
        triangles
    }

    fn bits(hit: Option<RayIntersection>) -> Option<(u32, [u32; 3])> {
        hit.map(|hit| {
            (
                hit.time_of_impact.to_bits(),
                hit.normal.to_array().map(f32::to_bits),
            )
        })
    }

    /// Every probe position over a mesh: a dense lattice through vertices and
    /// edges, every column border and corner, the outer bounds, and points
    /// from a fixed random sequence.
    fn probes(grid: &GroundGrid) -> Vec<[f32; 2]> {
        let [x0, z0, x1, z1] = grid.bounds;
        let mut points = Vec::new();
        // An eighth of a unit, or coarser over a mesh too wide for that,
        // always a power of two so the lattice passes through every vertex
        // and edge on whole or half units.
        let spacing = if x1 - x0 > 100.0 { 1.0 } else { 0.125 };
        let lattice = move |low: f32, high: f32| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Small meshes.
            let steps = ((high - low + 2.0) / spacing).ceil() as u16;
            (0..=steps).map(move |step| low - 1.0 + f32::from(step) * spacing)
        };
        for x in lattice(x0, x1) {
            for z in lattice(z0, z1) {
                points.push([x, z]);
            }
        }
        let border = |origin: f32, count: u32| {
            (0..=count).flat_map(move |index| {
                #[allow(clippy::cast_precision_loss)]
                let at = origin + index as f32 * grid.column_size;
                [at.next_down(), at, at.next_up()]
            })
        };
        let rows: Vec<f32> = border(grid.origin[1], grid.columns[1]).collect();
        for x in border(grid.origin[0], grid.columns[0]) {
            for &z in &rows {
                points.push([x, z]);
            }
        }
        for x in [x0, x1, x0.next_down(), x1.next_up()] {
            for z in [z0, z1, f32::midpoint(z0, z1), z0.next_down(), z1.next_up()] {
                points.push([x, z]);
                points.push([z.clamp(x0, x1), x.clamp(z0, z1)]);
            }
        }
        let mut seed = 0x9e37_79b9_u32;
        for _ in 0..20_000 {
            let mut unit = || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                #[allow(clippy::cast_precision_loss)]
                let unit = seed as f32 / u32::MAX as f32;
                unit
            };
            points.push([
                x0 - 1.0 + (x1 - x0 + 2.0) * unit(),
                z0 - 1.0 + (z1 - z0 + 2.0) * unit(),
            ]);
        }
        points
    }

    #[test]
    fn a_downward_ray_finds_exactly_what_the_tree_finds() {
        let meshes = [
            ("stairs", stairs()),
            ("overlapping floors", overlapping_floors()),
            ("slope", slope()),
            ("wall with a lip", wall_with_lip()),
            ("large triangles", large_triangles()),
            ("rough", rough()),
        ];
        for (name, triangles) in meshes {
            let mesh = CollisionMesh::new(triangles).expect("mesh").mesh;
            let grid = GroundGrid::new(&mesh).expect("grid");
            if name == "large triangles" {
                assert_eq!(grid.stats().wide, 2, "{name}");
            }
            let (mut compared, mut hits, mut ties) = (0, 0, 0);
            let in_a_plane = [0.0, 0.5, 3.0];
            for [x, z] in probes(&grid) {
                // Origins from below the floor to above the top step, and
                // windows from a hair to longer than the mesh is tall.
                for top in [-0.5, 0.0, 0.250_1, 0.5, 1.75, 3.0, 3.000_1, 4.2, 9.0] {
                    for length in [0.000_2, 0.5, 1.0, 2.5, 12.0] {
                        let ray = Ray::new(Vector::new(x, top, z), -Vector::Y);
                        let tree = mesh.cast_local_ray_and_get_normal(&ray, length, false);
                        compared += 1;
                        hits += usize::from(tree.is_some());
                        match grid.cast_down(&mesh, &ray, length) {
                            Ok(found) => assert_eq!(
                                bits(found),
                                bits(tree),
                                "{name} at ({x}, {top}, {z}) over {length}"
                            ),
                            // A ray starting exactly in a floor's plane may
                            // go to the tree; one starting off every plane,
                            // as the ground's rays do, should rarely.
                            Err(TreeDecides) => ties += usize::from(!in_a_plane.contains(&top)),
                        }
                    }
                }
            }
            assert!(hits * 20 > compared, "{name}: {hits} hits of {compared}");
            assert!(
                ties * 1000 < compared,
                "{name}: {ties} left to the tree of {compared}"
            );
        }
    }

    #[test]
    fn a_ray_that_is_not_straight_down_is_left_to_the_tree() {
        let mesh = CollisionMesh::new(slope()).expect("mesh").mesh;
        let grid = GroundGrid::new(&mesh).expect("grid");
        for dir in [
            Vector::Y,
            Vector::new(0.1, -0.99, 0.0).normalize(),
            Vector::X,
        ] {
            let ray = Ray::new(Vector::new(5.0, 9.0, 5.0), dir);
            assert!(grid.cast_down(&mesh, &ray, 20.0).is_err());
        }
        let ray = Ray::new(Vector::new(f32::NAN, 9.0, 5.0), -Vector::Y);
        assert!(grid.cast_down(&mesh, &ray, 20.0).is_err());
    }

    #[test]
    fn the_ground_is_the_same_with_and_without_the_grid() {
        for triangles in [
            stairs(),
            overlapping_floors(),
            slope(),
            wall_with_lip(),
            large_triangles(),
            rough(),
        ] {
            let world = CollisionWorld::new(triangles).expect("world");
            assert!(world.ground_grid_stats().is_some());
            let plain = world.without_ground_grid();
            assert!(plain.ground_grid_stats().is_none());
            let grid = world.ground_grid.as_ref().expect("grid");
            for [x, z] in probes(grid).into_iter().step_by(3) {
                for y in [-0.2, 0.0, 0.5, 1.25, 3.0, 4.0] {
                    let at = Vec3::new(x, y, z);
                    for (step_up, drop) in [(0.6, 0.6), (1.0, 4.0), (0.0, 0.0)] {
                        assert_eq!(
                            world.ground(at, step_up, drop).map(f32::to_bits),
                            plain.ground(at, step_up, drop).map(f32::to_bits),
                            "{at} {step_up} {drop}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod bench {
    use crate::movement::{CollisionWorld, PathProgress, PathSearch};
    use glam::Vec3;
    use std::time::Instant;

    type Triangle = [[f32; 3]; 3];

    fn cuboid(low: Vec3, high: Vec3) -> Vec<Triangle> {
        let corner = |i: u8| {
            [
                if i & 1 == 0 { low.x } else { high.x },
                if i & 2 == 0 { low.y } else { high.y },
                if i & 4 == 0 { low.z } else { high.z },
            ]
        };
        let faces = [
            [0, 1, 3, 2],
            [4, 6, 7, 5],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 2, 6, 4],
            [1, 5, 7, 3],
        ];
        faces
            .iter()
            .flat_map(|f| {
                [
                    [corner(f[0]), corner(f[1]), corner(f[2])],
                    [corner(f[0]), corner(f[2]), corner(f[3])],
                ]
            })
            .collect()
    }

    fn world() -> CollisionWorld {
        let mut triangles = Vec::new();
        for i in 0..150_u16 {
            for j in 0..150_u16 {
                let (x0, z0) = (-300.0 + f32::from(i) * 4.0, -300.0 + f32::from(j) * 4.0);
                let (x1, z1) = (x0 + 4.0, z0 + 4.0);
                triangles.push([[x0, 0.0, z0], [x1, 0.0, z0], [x1, 0.0, z1]]);
                triangles.push([[x0, 0.0, z0], [x1, 0.0, z1], [x0, 0.0, z1]]);
            }
        }
        triangles.extend(cuboid(
            Vec3::new(-40.0, 0.0, -1.0),
            Vec3::new(40.0, 10.0, 1.0),
        ));
        for post in 0..20_u8 {
            let x = -38.0 + f32::from(post) * 4.0;
            triangles.extend(cuboid(
                Vec3::new(x, 0.0, 20.0),
                Vec3::new(x + 0.5, 8.0, 20.5),
            ));
        }
        CollisionWorld::new(triangles).expect("world")
    }

    #[test]
    #[ignore = "bench"]
    fn bench_ground_grid() {
        let grid = world();
        let plain = grid.without_ground_grid();
        println!("{:?}", grid.ground_grid_stats());
        for (name, world) in [
            ("tree", &plain),
            ("grid", &grid),
            ("tree", &plain),
            ("grid", &grid),
        ] {
            let mut seed = 0x1234_5678_u32;
            let started = Instant::now();
            let mut found = 0_u32;
            let rays = 1_000_000_u32;
            for _ in 0..rays {
                let mut unit = || {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    #[allow(clippy::cast_precision_loss)]
                    let unit = seed as f32 / u32::MAX as f32;
                    unit
                };
                let at = Vec3::new(
                    -100.0 + 200.0 * unit(),
                    0.3 * unit(),
                    -100.0 + 200.0 * unit(),
                );
                found += u32::from(world.ground(at, 0.6, 0.6).is_some());
            }
            let ray_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(rays);
            let mut search = PathSearch::new(
                Vec3::new(0.0, 0.0, -60.0),
                Vec3::new(0.0, 0.0, 60.0),
                2.0,
                6.0,
            )
            .with_weight(1.25);
            let started = Instant::now();
            let result = loop {
                match search.advance(world, 5000) {
                    PathProgress::Searching => (),
                    done => break done,
                }
            };
            let took = started.elapsed();
            #[allow(clippy::cast_precision_loss)]
            let per = took.as_secs_f64() * 1e6 / search.expanded() as f64;
            println!(
                "{name}: {ray_us:.3} us per ray ({found} floors); search found={} expanded={} {:.1} ms, {per:.1} us per expansion",
                matches!(result, PathProgress::Found(_)),
                search.expanded(),
                took.as_secs_f64() * 1e3
            );
        }
    }
}

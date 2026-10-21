//! Building the polygons: the zone is cut into square tiles, rerecast builds
//! each tile's polygons on its own, and the tiles are joined where their
//! border edges meet.

use super::{Link, Polygon, Settings};
use glam::{UVec3, Vec2, Vec3, Vec3A};
use rerecast::{AreaType, HeightfieldBuilder, PolygonNavmesh, RegionId, TriMesh};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How a build went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildReport {
    /// Tiles with any triangles.
    pub tiles: usize,
    /// Tiles that made polygons.
    pub tiles_with_polygons: usize,
    /// Tiles rerecast refused; each leaves a hole in the mesh.
    pub refused: Vec<RefusedTile>,
    /// Each build thread's tiles and the time it spent building them.
    pub threads: Vec<(usize, Duration)>,
    /// The whole build.
    pub elapsed: Duration,
}

/// A tile rerecast refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedTile {
    /// The tile's column and row from the zone's low corner.
    pub tile: (i32, i32),
    /// The step that refused it: heightfield, rasterize, compact, regions or
    /// polygons.
    pub step: &'static str,
    /// rerecast's own error text.
    pub reason: String,
}

impl BuildReport {
    /// Refused tiles counted by step and reason, with the numbers in the
    /// reason masked so like refusals count together.
    #[must_use]
    pub fn refusals_by_reason(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for refused in &self.refused {
            let reason: String = refused
                .reason
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect();
            *counts
                .entry(format!("{}: {reason}", refused.step))
                .or_default() += 1;
        }
        counts
    }
}

/// A tile's polygon before the tiles are joined.
struct Tiled {
    polygon: Polygon,
    /// Tile border edges not yet joined: (side, a, b).
    open: Vec<(u8, Vec3, Vec3)>,
    tile: (i32, i32),
}

/// A tile's polygons, or the step that refused it and why.
type TileResult = Result<PolygonNavmesh, (&'static str, String)>;
/// An edge's two ends.
type Edge = (Vec3, Vec3);
/// A tile and one of its four sides.
type TileSide = ((i32, i32), u8);

/// Builds the polygons over `triangles` on `threads` threads, joined across
/// tiles; None when `cancelled` turns true before the last tile is built.
/// It is asked before each tile.
pub(super) fn polygons(
    triangles: &[[[f32; 3]; 3]],
    settings: &Settings,
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<Polygon>, BuildReport)> {
    let started = Instant::now();
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
    let border = walkable_radius(settings) + 3;
    let pad = f32::from(border) * cs;
    #[allow(clippy::cast_possible_truncation)]
    let (tiles_x, tiles_z) = (
        ((high.x - low.x) / tile).ceil().max(1.0) as i32,
        ((high.z - low.z) / tile).ceil().max(1.0) as i32,
    );
    let jobs = bucket(triangles, low, tile, pad, (tiles_x, tiles_z));
    let threads = threads.max(1);
    // Tiles differ widely in cost, so each thread takes the next tile from a
    // shared counter rather than a fixed share.
    let next = AtomicUsize::new(0);
    let mut built: Vec<Option<TileResult>> = (0..jobs.len()).map(|_| None).collect();
    let mut report = BuildReport {
        tiles: jobs.len(),
        ..BuildReport::default()
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut busy = Duration::ZERO;
                    let mut done = Vec::new();
                    while !cancelled() {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(((tx, tz), indices)) = jobs.get(i) else {
                            break;
                        };
                        let began = Instant::now();
                        #[allow(clippy::cast_precision_loss)] // Tile counts are small.
                        let min = Vec3::new(
                            low.x + *tx as f32 * tile - pad,
                            low.y - 1.0,
                            low.z + *tz as f32 * tile - pad,
                        );
                        let max = Vec3::new(
                            min.x + tile + 2.0 * pad,
                            high.y + settings.height + 1.0,
                            min.z + tile + 2.0 * pad,
                        );
                        done.push((
                            i,
                            build_tile(triangles, indices, min, max, border, settings),
                        ));
                        busy += began.elapsed();
                    }
                    (done, busy)
                })
            })
            .collect();
        for handle in handles {
            // A tile build that panicked is a bug in rerecast; pass it on.
            let (done, busy) = handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            report.threads.push((done.len(), busy));
            for (i, result) in done {
                built[i] = Some(result);
            }
        }
    });
    let mut tiled = Vec::new();
    for (((tx, tz), _), result) in jobs.iter().zip(built) {
        // A tile left unbuilt means the build was cancelled.
        match result? {
            Ok(mesh) => {
                if mesh.polygon_count() > 0 {
                    report.tiles_with_polygons += 1;
                }
                append_tile(&mut tiled, &mesh, (*tx, *tz));
            }
            Err((step, reason)) => report.refused.push(RefusedTile {
                tile: (*tx, *tz),
                step,
                reason,
            }),
        }
    }
    let polygons = join_tiles(tiled, settings.cell_height * 4.0 + settings.climb);
    report.elapsed = started.elapsed();
    Some((polygons, report))
}

/// Each triangle's index under every tile its bounds, widened by `pad`,
/// touch; ordered by tile so a build is the same on any thread count.
fn bucket(
    triangles: &[[[f32; 3]; 3]],
    low: Vec3,
    tile: f32,
    pad: f32,
    (tiles_x, tiles_z): (i32, i32),
) -> Vec<((i32, i32), Vec<u32>)> {
    let mut buckets: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
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
            for tz in cell(min.y - pad, low.z).max(0)..=cell(max.y + pad, low.z).min(tiles_z - 1) {
                #[allow(clippy::cast_possible_truncation)] // Triangles are counted in u32.
                buckets.entry((tx, tz)).or_default().push(i as u32);
            }
        }
    }
    buckets.into_iter().collect()
}

/// The walker's radius in voxels.
fn walkable_radius(settings: &Settings) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let radius = (settings.radius / settings.cell_size).ceil() as u16;
    radius
}

fn build_tile(
    triangles: &[[[f32; 3]; 3]],
    indices: &[u32],
    min: Vec3,
    max: Vec3,
    border: u16,
    settings: &Settings,
) -> TileResult {
    fn refused<E: std::fmt::Display>(step: &'static str) -> impl Fn(E) -> (&'static str, String) {
        move |error| (step, error.to_string())
    }
    let (cs, ch) = (settings.cell_size, settings.cell_height);
    let mut trimesh = TriMesh::default();
    for &i in indices {
        #[allow(clippy::cast_possible_truncation)] // Three per triangle, counted in u32.
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
    .map_err(refused("heightfield"))?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (walkable_height, walkable_climb) = (
        (settings.height / ch).ceil() as u16,
        (settings.climb / ch).floor() as u16,
    );
    let radius = walkable_radius(settings);
    heightfield
        .rasterize_triangles(&trimesh, walkable_climb)
        .map_err(refused("rasterize"))?;
    heightfield.filter_low_hanging_walkable_obstacles(walkable_climb);
    heightfield.filter_ledge_spans(walkable_height, walkable_climb);
    heightfield.filter_walkable_low_height_spans(walkable_height);
    let mut compact = heightfield
        .into_compact(walkable_height, walkable_climb)
        .map_err(refused("compact"))?;
    compact.erode_walkable_area(radius);
    compact.build_distance_field();
    compact
        .build_regions(border, 8 * 8, 20 * 20)
        .map_err(refused("regions"))?;
    let contours = compact.build_contours(1.3, radius * 8, rerecast::BuildContoursFlags::default());
    contours.into_polygon_mesh(6).map_err(refused("polygons"))
}

/// Adds a tile's polygons, in world units, with links inside the tile and
/// its border edges left open for [`join_tiles`].
fn append_tile(tiled: &mut Vec<Tiled>, mesh: &PolygonNavmesh, tile: (i32, i32)) {
    let nvp = mesh.max_vertices_per_polygon as usize;
    let world = |i: u16| {
        let v = mesh.vertices[i as usize];
        Vec3::new(
            mesh.aabb.min.x + f32::from(v.x) * mesh.cell_size,
            mesh.aabb.min.y + f32::from(v.y) * mesh.cell_height,
            mesh.aabb.min.z + f32::from(v.z) * mesh.cell_size,
        )
    };
    #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
    let base = tiled.len() as u32;
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
                // The low two bits name the tile side the edge lies on.
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
        tiled.push(Tiled {
            polygon: Polygon { vertices, links },
            open,
            tile,
        });
    }
}

/// Joins tiles where a border edge of one overlaps the facing border edge of
/// its neighbour within `tolerance` in height.
fn join_tiles(mut tiled: Vec<Tiled>, tolerance: f32) -> Vec<Polygon> {
    // Sides: 0 at low x, 1 at high z, 2 at high x, 3 at low z.
    let mut by_side: BTreeMap<TileSide, Vec<(u32, Edge)>> = BTreeMap::new();
    for (i, polygon) in tiled.iter().enumerate() {
        for &(side, a, b) in &polygon.open {
            #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
            by_side
                .entry((polygon.tile, side))
                .or_default()
                .push((i as u32, (a, b)));
        }
    }
    let mut links: Vec<(u32, Link)> = Vec::new();
    for (&((tx, tz), side), edges) in &by_side {
        let (other, facing) = match side {
            0 => ((tx - 1, tz), 2),
            1 => ((tx, tz + 1), 3),
            2 => ((tx + 1, tz), 0),
            _ => ((tx, tz - 1), 1),
        };
        let Some(theirs) = by_side.get(&(other, facing)) else {
            continue;
        };
        // Along the shared line: z for the x sides, x for the z sides.
        let along = |v: Vec3| if side % 2 == 0 { v.z } else { v.x };
        let at = |p: Vec3, q: Vec3, t: f32| {
            let span = along(q) - along(p);
            if span.abs() < f32::EPSILON {
                p
            } else {
                p.lerp(q, (t - along(p)) / span)
            }
        };
        for &(mine, (a, b)) in edges {
            let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
            for &(their, (c, d)) in theirs {
                let (start, end) = (
                    lo.max(along(c).min(along(d))),
                    hi.min(along(c).max(along(d))),
                );
                if end - start < 0.01 {
                    continue;
                }
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
        tiled[from as usize].polygon.links.push(link);
    }
    tiled.into_iter().map(|t| t.polygon).collect()
}

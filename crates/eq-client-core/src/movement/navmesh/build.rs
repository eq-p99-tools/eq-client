//! Building the polygons: the zone is cut into square tiles, rerecast builds
//! each tile's polygons on its own, and the tiles are joined where their
//! border edges meet.

use super::{Link, Polygon, Settings};
use glam::{UVec3, Vec2, Vec3, Vec3A};
use rerecast::{AreaType, HeightfieldBuilder, PolygonNavmesh, RegionId, TriMesh};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::AssertUnwindSafe;
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
    /// Triangles left out of at least one tile for lying wholly under the
    /// tallest heightfield rerecast can hold below the rest of the tile.
    /// The walker still collides with them; only the mesh leaves them out.
    pub deep_triangles: usize,
    /// Each build thread's tiles and the time it spent building them.
    pub threads: Vec<(usize, Duration)>,
    /// Ways off ledges found.
    pub drop_links: usize,
    /// Stretches of outline whose drops were refused for a panic.
    pub refused_stretches: usize,
    /// The search for ways off ledges.
    pub drop_elapsed: Duration,
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
pub(super) type TileResult = Result<PolygonNavmesh, (&'static str, String)>;
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
    polygons_with(triangles, settings, threads, cancelled, &build_tile)
}

/// Builds one tile: the triangles at `indices`, in the box from `min` to
/// `max` with a `border` of voxels.
pub(super) type TileBuilder<'a> =
    dyn Fn(&[[[f32; 3]; 3]], &[u32], Vec3, Vec3, u16, &Settings) -> TileResult + Sync + 'a;

/// [`polygons`], with each tile built by `tile_builder`.
pub(super) fn polygons_with(
    triangles: &[[[f32; 3]; 3]],
    settings: &Settings,
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
    tile_builder: &TileBuilder<'_>,
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
    let mut deep_triangles = BTreeSet::new();
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
                        let (bottom, top, kept, deep) =
                            tile_heights(triangles, indices, low.y - 1.0, settings);
                        #[allow(clippy::cast_precision_loss)] // Tile counts are small.
                        let min = Vec3::new(
                            low.x + *tx as f32 * tile - pad,
                            bottom,
                            low.z + *tz as f32 * tile - pad,
                        );
                        let max =
                            Vec3::new(min.x + tile + 2.0 * pad, top, min.z + tile + 2.0 * pad);
                        // A tile rerecast panics on is refused like one it
                        // reports an error for, and the build goes on.
                        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            tile_builder(triangles, &kept, min, max, border, settings)
                        }))
                        .unwrap_or_else(|panic| Err(("panic", panic_message(&*panic))));
                        done.push((i, result, deep));
                        busy += began.elapsed();
                    }
                    (done, busy)
                })
            })
            .collect();
        for handle in handles {
            // Tile panics are caught above, so a thread panics only on a
            // bug here; pass it on.
            let (done, busy) = handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            report.threads.push((done.len(), busy));
            for (i, result, deep) in done {
                built[i] = Some(result);
                deep_triangles.extend(deep);
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
    report.deep_triangles = deep_triangles.len();
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

/// A tile's heightfield bottom and top, the triangles it is built over and
/// those it leaves out. It spans its own triangles' heights, from a cell
/// under the lowest to the walker's height and a unit over the highest, so
/// a zone's height adds nothing to a tile's. Its bottom stays on the cell
/// grid that starts at `base`, so a tile's cells lie where a heightfield
/// spanning the whole zone puts them. A tile taller than rerecast's tallest
/// span is built over its top that tall, and leaves out the triangles
/// wholly under it.
fn tile_heights<'a>(
    triangles: &[[[f32; 3]; 3]],
    indices: &'a [u32],
    base: f32,
    settings: &Settings,
) -> (f32, f32, Cow<'a, [u32]>, Vec<u32>) {
    let ch = settings.cell_height;
    let (lowest, highest) = indices
        .iter()
        .flat_map(|&i| triangles[i as usize])
        .fold((f32::MAX, f32::MIN), |(low, high), p| {
            (low.min(p[1]), high.max(p[1]))
        });
    let top = highest + settings.height + 1.0;
    // Spans count cells in 16 bits; two short of the most keeps a span
    // started at the top inside them after rounding.
    let tallest = f32::from(u16::MAX - 2) * ch;
    let on_grid = |y: f32| base + ((y - base) / ch).floor() * ch;
    let bottom = on_grid(lowest - 1.0);
    if top - bottom <= tallest {
        return (bottom, top, Cow::Borrowed(indices), Vec::new());
    }
    let bottom = on_grid(top - tallest) + ch;
    let (kept, deep): (Vec<u32>, Vec<u32>) = indices
        .iter()
        .partition(|&&i| triangles[i as usize].iter().any(|p| p[1] >= bottom));
    (bottom, top, Cow::Owned(kept), deep)
}

/// A panic's message, as `panic!` gives it.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// The walker's radius in voxels.
fn walkable_radius(settings: &Settings) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let radius = (settings.radius / settings.cell_size).ceil() as u16;
    radius
}

pub(super) fn build_tile(
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
        let [a, b, c] = triangles[i as usize].map(Vec3A::from_array);
        // The walker stands on a triangle whichever way it was wound, so
        // each is turned to face up before rerecast judges its slope.
        let up = (b - a).cross(c - a).y >= 0.0;
        trimesh
            .vertices
            .extend(if up { [a, b, c] } else { [a, c, b] });
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
    tag_sizes(&mut compact, settings)?;
    keep_sizes_off_low_ceilings(&mut compact, radius);
    compact.erode_walkable_area(radius);
    compact.build_distance_field();
    compact
        .build_regions(border, 8 * 8, 20 * 20)
        .map_err(refused("regions"))?;
    let contours = compact.build_contours(1.3, radius * 8, rerecast::BuildContoursFlags::default());
    contours.into_polygon_mesh(6).map_err(refused("polygons"))
}

/// Tags each walkable span with the sizes its headroom fits: area 1 for the
/// shortest alone, 2 for the two shorter, 3 for all three. Regions, and so
/// polygons, never cross a change of tag.
fn tag_sizes(
    compact: &mut rerecast::CompactHeightfield,
    settings: &Settings,
) -> Result<(), (&'static str, String)> {
    let ch = settings.cell_height;
    // A span's headroom is kept in a byte of cell heights; open sky and any
    // gap above the byte read as its top.
    let cap = f32::from(u8::MAX) * ch;
    if settings.sizes[2] >= cap {
        return Err((
            "sizes",
            format!(
                "the tallest size {} is over the headroom cap {cap}",
                settings.sizes[2]
            ),
        ));
    }
    for (span, area) in compact.spans.iter().zip(compact.areas.iter_mut()) {
        if !area.is_walkable() {
            continue;
        }
        let headroom = f32::from(span.height()) * ch;
        #[allow(clippy::cast_possible_truncation)] // At most three sizes.
        let fits = settings
            .sizes
            .iter()
            .filter(|&&size| headroom >= size)
            .count() as u8;
        // The build's own height filter leaves only spans the shortest fits.
        *area = AreaType(fits.max(1));
    }
    Ok(())
}

/// Narrows each taller size's floor away from where a shorter size's alone
/// begins by the walker's `radius`, as a plain build at that size erodes it
/// from the low ceiling: a walker standing on a size's floor keeps its
/// whole width out from under a ceiling too low for it. For each size, the
/// spans of the sizes under it are treated as walls, rerecast's own erosion
/// runs, and a span it takes falls to the size below.
fn keep_sizes_off_low_ceilings(compact: &mut rerecast::CompactHeightfield, radius: u16) {
    let mut tags = compact.areas.clone();
    for size in [3_u8, 2] {
        for (area, tag) in compact.areas.iter_mut().zip(&tags) {
            *area = if tag.0 >= size && tag.is_walkable() {
                *tag
            } else {
                AreaType::NOT_WALKABLE
            };
        }
        compact.erode_walkable_area(radius);
        for (tag, area) in tags.iter_mut().zip(&compact.areas) {
            if tag.is_walkable() && tag.0 >= size && !area.is_walkable() {
                *tag = AreaType(size - 1);
            }
        }
    }
    compact.areas = tags;
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
            polygon: Polygon {
                vertices,
                links,
                // Areas 1 to 3, as `tag_sizes` sets them.
                size: (*mesh.areas[p]).clamp(1, 3) - 1,
            },
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

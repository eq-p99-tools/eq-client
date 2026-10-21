//! Ways off ledges: points along the mesh's outline where the walker's own
//! drop, a grid cell straight out over the edge, lands it on a lower polygon
//! within the drop limit.

use super::{DropLink, NavMesh, Outline};
use crate::movement::path::{CELL, MAX_DROP, drop};
use crate::movement::{CollisionWorld, MAX_GROUNDED_STEP};
use glam::{Vec2, Vec3};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How far apart along the outline drops are tried.
pub(super) const DROP_SPACING: f32 = 1.0;
/// How near along a ledge two drops between the same polygons may be.
pub(super) const THINNING: f32 = 4.0;
/// How far inside its polygon a drop starts, so it starts on the floor.
const INSIDE: f32 = 0.1;

/// The ways off the ledges of `mesh` for a walker `height` tall, tried in the
/// world over `triangles` on `threads` threads, and how many stretches of
/// outline were refused for a panic; None when `cancelled` turns true first.
/// It is asked before each stretch.
pub(super) fn links(
    mesh: &NavMesh,
    triangles: &[[[f32; 3]; 3]],
    height: f32,
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<DropLink>, usize)> {
    let Ok(world) = CollisionWorld::new(triangles.iter().copied()) else {
        // No world to drop in, so no drops; the walk finds none either.
        return Some((Vec::new(), 0));
    };
    links_with(mesh, &world, height, threads, cancelled, &stretch_links)
}

/// Finds one stretch's drops.
pub(super) type StretchLinks<'a> =
    dyn Fn(&NavMesh, &CollisionWorld, &Outline, f32) -> Vec<DropLink> + Sync + 'a;

/// [`links`] in `world`, each stretch's drops found by `stretch_links`.
pub(super) fn links_with(
    mesh: &NavMesh,
    world: &CollisionWorld,
    height: f32,
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
    stretch_links: &StretchLinks<'_>,
) -> Option<(Vec<DropLink>, usize)> {
    let next = AtomicUsize::new(0);
    let mut found: Vec<Option<Vec<DropLink>>> = vec![None; mesh.outline.len()];
    let mut refused = 0;
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while !cancelled() {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(stretch) = mesh.outline.get(i) else {
                            break;
                        };
                        // A stretch whose drops panic is refused, and the
                        // rest go on.
                        let links = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            stretch_links(mesh, world, stretch, height)
                        }));
                        done.push((i, links.ok()));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            // Panics are caught above, so a thread panics only on a bug here.
            let done = handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            for (i, links) in done {
                if links.is_none() {
                    refused += 1;
                }
                found[i] = Some(links.unwrap_or_default());
            }
        }
    });
    // A stretch left untried means the pass was cancelled.
    let found: Vec<Vec<DropLink>> = found.into_iter().collect::<Option<_>>()?;
    Some((thin(found.into_iter().flatten()), refused))
}

/// The drops along one stretch of outline, a [`DROP_SPACING`] apart.
pub(super) fn stretch_links(
    mesh: &NavMesh,
    world: &CollisionWorld,
    stretch: &Outline,
    height: f32,
) -> Vec<DropLink> {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let polygon = &mesh.polygons[stretch.polygon as usize];
    #[allow(clippy::cast_precision_loss)] // A polygon has a few corners.
    let centre = flat(polygon.vertices.iter().sum::<Vec3>()) / polygon.vertices.len() as f32;
    let along = flat(stretch.b) - flat(stretch.a);
    let span = along.length();
    if span < f32::EPSILON {
        return Vec::new();
    }
    // Out of the polygon, square to the stretch.
    let mut out = along.perp() / span;
    if out.dot(flat(stretch.a.lerp(stretch.b, 0.5)) - centre) < 0.0 {
        out = -out;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A stretch's samples.
    let samples = (span / DROP_SPACING).floor().max(1.0) as usize;
    (0..samples)
        .filter_map(|k| {
            #[allow(clippy::cast_precision_loss)] // A stretch's samples.
            let at = stretch.a.lerp(stretch.b, (k as f32 + 0.5) / samples as f32);
            drop_at(
                mesh,
                world,
                stretch.polygon,
                at - Vec3::new(out.x, 0.0, out.y) * INSIDE,
                out,
                height,
            )
        })
        .collect()
}

/// The drop from the mesh's point `at` on polygon `from`, a grid cell out
/// along `out`, as the walker's own drop makes it.
fn drop_at(
    mesh: &NavMesh,
    world: &CollisionWorld,
    from: u32,
    at: Vec3,
    out: Vec2,
    height: f32,
) -> Option<DropLink> {
    // The floor under the mesh's point: the mesh sits a voxel or so above it.
    let above = at + Vec3::Y * MAX_GROUNDED_STEP;
    let top = above - Vec3::Y * world.ray_distance(above, -Vec3::Y, 2.0 * MAX_GROUNDED_STEP)?;
    let target = Vec2::new(top.x, top.z) + out * CELL;
    // A floor a step or less below the far side is a walk, not a drop, and
    // none within the limit is no drop: one ray rules most points out
    // before the walker's drop is run.
    let probe = Vec3::new(target.x, top.y + MAX_GROUNDED_STEP, target.y);
    let below = world.ray_distance(probe, -Vec3::Y, 2.0 * MAX_GROUNDED_STEP + MAX_DROP)?;
    if below <= 2.0 * MAX_GROUNDED_STEP {
        return None;
    }
    let landing = drop(world, top, target, height)?;
    if top.y - landing.y <= MAX_GROUNDED_STEP {
        return None;
    }
    let (to, _) = mesh.locate(landing)?;
    Some(DropLink {
        from,
        to,
        top,
        landing,
    })
}

/// `links` with each kept only when no kept link between the same polygons
/// leaves its ledge within [`THINNING`] of it.
fn thin(links: impl Iterator<Item = DropLink>) -> Vec<DropLink> {
    let mut kept: Vec<DropLink> = Vec::new();
    let mut by_pair: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for link in links {
        let pair = by_pair.entry((link.from, link.to)).or_default();
        if pair
            .iter()
            .all(|&k| kept[k].top.distance(link.top) >= THINNING)
        {
            pair.push(kept.len());
            kept.push(link);
        }
    }
    kept
}

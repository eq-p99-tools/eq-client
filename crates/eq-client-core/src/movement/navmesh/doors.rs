//! Per-search connections across usable doors. The cached mesh stays static;
//! only the existing walk test may bridge its missing doorway floor.
use super::{NavMesh, cell_of};
use crate::movement::{
    CollisionWorld, MAX_GROUNDED_STEP,
    path::{CELL, walk},
};
use glam::{Vec2, Vec3};
use std::collections::{BTreeMap, HashSet};

// Two searched cells beyond a leaf cover erosion at either jamb. These are
// candidate limits, never permission to cross a wall, ledge or closed door.
const REACH: f32 = 2.0 * CELL;
const INSIDE: f32 = 0.05;
const PER_ISLAND: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(super) struct DoorLink {
    pub id: u8,
    pub from: u32,
    pub to: u32,
    pub start: Vec3,
    pub end: Vec3,
}

#[derive(Clone, Copy)]
struct Candidate {
    polygon: u32,
    at: Vec3,
}

/// Only listed, collidable doors can propose links. Each direction is
/// independently walked with that door alone removed; every other solid,
/// floor and liquid rule remains active. Actual following still stops and
/// uses the door interaction until the live collider permits the crossing.
pub(super) fn links(
    mesh: &NavMesh,
    world: &CollisionWorld,
    allowed: &[u8],
    height: f32,
) -> Vec<DoorLink> {
    let mut links = Vec::new();
    for (id, low, high) in world
        .door_bounds()
        .into_iter()
        .filter(|(id, ..)| allowed.contains(id))
    {
        let opened = world.without_doors(&[id]);
        let candidates = candidates(mesh, &opened, low, high, height);
        for (i, a) in candidates.iter().enumerate() {
            for b in candidates.iter().skip(i + 1) {
                if mesh.connected(a.polygon, b.polygon) || flat(a.at - b.at) > 2.0 * REACH {
                    continue;
                }
                let widened = Vec3::new(
                    crate::movement::BODY_REACH,
                    0.0,
                    crate::movement::BODY_REACH,
                );
                if super::route::entry(a.at, b.at, low - widened, high + widened).is_none() {
                    continue;
                }
                for (from, to) in [(a, b), (b, a)] {
                    if walk(&opened, from.at, Vec2::new(to.at.x, to.at.z), height)
                        .is_some_and(|end| (end.y - to.at.y).abs() <= MAX_GROUNDED_STEP)
                    {
                        links.push(DoorLink {
                            id,
                            from: from.polygon,
                            to: to.polygon,
                            start: from.at,
                            end: to.at,
                        });
                    }
                }
            }
        }
    }
    links
}

fn candidates(
    mesh: &NavMesh,
    world: &CollisionWorld,
    low: Vec3,
    high: Vec3,
    height: f32,
) -> Vec<Candidate> {
    let centre = (low + high) * 0.5;
    let widen = Vec3::new(REACH, 0.0, REACH);
    let (first, last) = (
        cell_of(Vec2::new((low - widen).x, (low - widen).z)),
        cell_of(Vec2::new((high + widen).x, (high + widen).z)),
    );
    let mut seen = HashSet::new();
    let mut islands: BTreeMap<u32, Vec<Candidate>> = BTreeMap::new();
    for x in first.0..=last.0 {
        for z in first.1..=last.1 {
            for &index in mesh.walls.get(&(x, z)).into_iter().flatten() {
                if !seen.insert(index) {
                    continue;
                }
                let edge = &mesh.outline[index as usize];
                let along = edge.b - edge.a;
                let delta = Vec2::new(along.x, along.z);
                let toward = Vec2::new(centre.x - edge.a.x, centre.z - edge.a.z);
                let t = if delta.length_squared() > 0.0 {
                    (toward.dot(delta) / delta.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut at = edge.a.lerp(edge.b, t);
                let Some(inside) = mesh.polygon_point(edge.polygon) else {
                    continue;
                };
                let inward = Vec3::new(inside.x - at.x, 0.0, inside.z - at.z).normalize_or_zero();
                at += inward * INSIDE;
                if at.x < low.x - REACH
                    || at.x > high.x + REACH
                    || at.z < low.z - REACH
                    || at.z > high.z + REACH
                {
                    continue;
                }
                let Some(y) =
                    world.nearest_ground(at, MAX_GROUNDED_STEP + 3.0, MAX_GROUNDED_STEP + 3.0)
                else {
                    continue;
                };
                at.y = y;
                if y < low.y - MAX_GROUNDED_STEP
                    || y > high.y
                    || world.support(at).is_none()
                    || world
                        .clearance(at, height, 0.0)
                        .is_some_and(|distance| distance < 0.0)
                {
                    continue;
                }
                let candidates = islands.entry(mesh.island_of(edge.polygon)).or_default();
                candidates.push(Candidate {
                    polygon: edge.polygon,
                    at,
                });
            }
        }
    }
    let mut result = Vec::new();
    for mut candidates in islands.into_values() {
        candidates.sort_by(|a, b| {
            flat(a.at - centre)
                .total_cmp(&flat(b.at - centre))
                .then(a.polygon.cmp(&b.polygon))
        });
        candidates.dedup_by(|a, b| a.polygon == b.polygon && flat(a.at - b.at) < INSIDE);
        result.extend(candidates.into_iter().take(PER_ISLAND));
    }
    result
}

fn flat(v: Vec3) -> f32 {
    Vec2::new(v.x, v.z).length()
}

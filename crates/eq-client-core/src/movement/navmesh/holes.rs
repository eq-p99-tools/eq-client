// Copyright (c) 2009-2010 Mikko Mononen memon@inside.org
//
// This software is provided 'as-is', without any express or implied
// warranty.  In no event will the authors be held liable for any damages
// arising from the use of this software.
// Permission is granted to anyone to use this software for any purpose,
// including commercial applications, and to alter it and redistribute it
// freely, subject to the following restrictions:
// 1. The origin of this software must not be misrepresented; you must not
//    claim that you wrote the original software. If you use this software
//    in a product, an acknowledgment in the product documentation would be
//    appreciated but is not required.
// 2. Altered source versions must be plainly marked as such, and must not be
//    misrepresented as being the original software.
// 3. This notice may not be removed or altered from any source distribution.
//
// Altered: ported to Rust from Recast's RecastContour.cpp (mergeRegionHoles
// and the helpers it uses), to run over rerecast's contours, which it builds
// without this step; holes are told from outlines by area within each region.

//! Holes merged into their regions' outlines. A region with a solid inside
//! it, such as a tree trunk, is outlined twice: once round its edge and once,
//! wound the other way, round the solid. rerecast hands both on as they are,
//! and it can't cut a hole wound that way into polygons, so it refuses the
//! whole tile. Recast first joins each hole to its outline by a bridge, which
//! leaves one outline that goes round the hole; this is that step.

use glam::U16Vec3;
use rerecast::ContourSet;
use std::collections::BTreeMap;

type Vertex = (U16Vec3, u32);

/// How the merge went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Merged {
    /// Holes joined to their outlines.
    pub(super) holes: usize,
    /// Holes no bridge could be found for; they are left as they were.
    pub(super) failed: usize,
}

/// Joins each region's holes in `contours` to the region's outline.
pub(super) fn merge_holes(contours: &mut ContourSet) -> Merged {
    let mut merged = Merged::default();
    let mut regions: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, contour) in contours.contours.iter().enumerate() {
        if contour.vertices.len() >= 3 {
            regions.entry(contour.region.bits()).or_default().push(i);
        }
    }
    let mut emptied = Vec::new();
    for members in regions.values().filter(|members| members.len() > 1) {
        let areas: Vec<i64> = members
            .iter()
            .map(|&i| twice_area(&contours.contours[i].vertices))
            .collect();
        // The outline is the region's largest contour; a hole is wound the
        // other way. Another contour wound like the outline is left alone.
        let (outline_at, &outline_area) = areas
            .iter()
            .enumerate()
            .max_by_key(|(_, area)| area.abs())
            .expect("a region with contours");
        let outline = members[outline_at];
        let mut holes: Vec<Hole> = members
            .iter()
            .zip(&areas)
            .filter(|&(_, &area)| area != 0 && area.signum() != outline_area.signum())
            .map(|(&i, _)| Hole::of(i, &contours.contours[i].vertices))
            .collect();
        // Left to right, as Recast merges them.
        holes.sort_by_key(|hole| (hole.min_x, hole.min_z));
        for h in 0..holes.len() {
            let found = bridge(contours, outline, &holes, h);
            let Some((at_outline, at_hole)) = found else {
                merged.failed += 1;
                continue;
            };
            let hole = std::mem::take(&mut contours.contours[holes[h].index].vertices);
            let joined = joined(
                &contours.contours[outline].vertices,
                at_outline,
                &hole,
                at_hole,
            );
            contours.contours[outline].vertices = joined;
            emptied.push(holes[h].index);
            merged.holes += 1;
        }
    }
    emptied.sort_unstable();
    for i in emptied.into_iter().rev() {
        contours.contours.remove(i);
    }
    merged
}

/// A hole and its lowest leftmost vertex.
struct Hole {
    index: usize,
    min_x: u16,
    min_z: u16,
    leftmost: usize,
}

impl Hole {
    fn of(index: usize, vertices: &[Vertex]) -> Self {
        let (leftmost, (corner, _)) = vertices
            .iter()
            .enumerate()
            .min_by_key(|(_, (v, _))| (v.x, v.z))
            .expect("a hole with vertices");
        Self {
            index,
            min_x: corner.x,
            min_z: corner.z,
            leftmost,
        }
    }
}

/// The outline vertex and the hole vertex the bridge for hole `h` joins:
/// from the hole's leftmost vertex on, the first that sees an outline vertex,
/// the nearest such, within that vertex's cone and crossing neither the
/// outline nor any hole not yet merged.
fn bridge(
    contours: &ContourSet,
    outline: usize,
    holes: &[Hole],
    h: usize,
) -> Option<(usize, usize)> {
    let outline_vertices = &contours.contours[outline].vertices;
    let hole_vertices = &contours.contours[holes[h].index].vertices;
    let mut corner_at = holes[h].leftmost;
    for _ in 0..hole_vertices.len() {
        let corner = point(&hole_vertices[corner_at]);
        let mut diagonals: Vec<(i64, usize)> = (0..outline_vertices.len())
            .filter(|&j| in_cone(j, outline_vertices, corner))
            .map(|j| {
                let p = point(&outline_vertices[j]);
                let (dx, dz) = (p[0] - corner[0], p[1] - corner[1]);
                (dx * dx + dz * dz, j)
            })
            .collect();
        // The shortest bridge first. Recast's own sort isn't stable; this
        // one breaks ties by vertex order.
        diagonals.sort_unstable();
        // Recast passes another diagonal's vertex as the one to pass over
        // here, a slip that changes nothing: edges ending at the bridge's own
        // ends are passed over anyway.
        let found = diagonals.iter().find(|&&(_, j)| {
            let p = point(&outline_vertices[j]);
            !crosses_contour(p, corner, Some(j), outline_vertices)
                && holes[h..].iter().all(|later| {
                    !crosses_contour(p, corner, None, &contours.contours[later.index].vertices)
                })
        });
        if let Some(&(_, j)) = found {
            return Some((j, corner_at));
        }
        corner_at = (corner_at + 1) % hole_vertices.len();
    }
    None
}

/// The outline from `at_outline` round to itself again, then the hole from
/// `at_hole` round to itself again: one contour through the bridge both
/// ways, which Recast leaves as two coincident edges.
fn joined(outline: &[Vertex], at_outline: usize, hole: &[Vertex], at_hole: usize) -> Vec<Vertex> {
    let from_outline = (0..=outline.len()).map(|i| outline[(at_outline + i) % outline.len()]);
    let from_hole = (0..=hole.len()).map(|i| hole[(at_hole + i) % hole.len()]);
    from_outline.chain(from_hole).collect()
}

/// Twice the area `vertices` enclose across, signed as Recast signs it:
/// an outline above zero and a hole below.
pub(super) fn twice_area(vertices: &[Vertex]) -> i64 {
    (0..vertices.len())
        .map(|i| {
            let (a, b) = (
                point(&vertices[i]),
                point(&vertices[prev(i, vertices.len())]),
            );
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

fn point((v, _): &Vertex) -> [i64; 2] {
    [i64::from(v.x), i64::from(v.z)]
}

fn prev(i: usize, n: usize) -> usize {
    if i == 0 { n - 1 } else { i - 1 }
}

fn next(i: usize, n: usize) -> usize {
    if i + 1 < n { i + 1 } else { 0 }
}

fn area2(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
}

/// Whether `c` is strictly left of the line from `a` to `b`.
fn left(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> bool {
    area2(a, b, c) < 0
}

fn left_on(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> bool {
    area2(a, b, c) <= 0
}

fn collinear(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> bool {
    area2(a, b, c) == 0
}

/// Whether `ab` and `cd` share a point inside both.
fn intersect_proper(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    if collinear(a, b, c) || collinear(a, b, d) || collinear(c, d, a) || collinear(c, d, b) {
        return false;
    }
    (left(a, b, c) ^ left(a, b, d)) && (left(c, d, a) ^ left(c, d, b))
}

/// Whether `c` lies on the closed segment `ab`.
fn between(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> bool {
    if !collinear(a, b, c) {
        return false;
    }
    if a[0] == b[0] {
        (a[1] <= c[1] && c[1] <= b[1]) || (a[1] >= c[1] && c[1] >= b[1])
    } else {
        (a[0] <= c[0] && c[0] <= b[0]) || (a[0] >= c[0] && c[0] >= b[0])
    }
}

fn intersect(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    intersect_proper(a, b, c, d)
        || between(a, b, c)
        || between(a, b, d)
        || between(c, d, a)
        || between(c, d, b)
}

/// Whether the segment from `d0` to `d1` crosses an edge of `vertices`,
/// leaving out the edges at vertex `skip` and those sharing an end with it.
fn crosses_contour(d0: [i64; 2], d1: [i64; 2], skip: Option<usize>, vertices: &[Vertex]) -> bool {
    let n = vertices.len();
    (0..n).any(|k| {
        let k1 = next(k, n);
        if skip == Some(k) || skip == Some(k1) {
            return false;
        }
        let (p0, p1) = (point(&vertices[k]), point(&vertices[k1]));
        if d0 == p0 || d1 == p0 || d0 == p1 || d1 == p1 {
            return false;
        }
        intersect(d0, d1, p0, p1)
    })
}

/// Whether `pj` lies within the cone of `vertices`' vertex `i`, between its
/// two edges on the contour's inside.
fn in_cone(i: usize, vertices: &[Vertex], pj: [i64; 2]) -> bool {
    let n = vertices.len();
    let pi = point(&vertices[i]);
    let after = point(&vertices[next(i, n)]);
    let before = point(&vertices[prev(i, n)]);
    // A convex corner: the next vertex is left of or on the edge into it.
    if left_on(before, pi, after) {
        return left(pi, pj, before) && left(pj, pi, after);
    }
    // A reflex corner, taken not to be a straight one.
    !(left_on(pi, pj, after) && left_on(pj, pi, before))
}

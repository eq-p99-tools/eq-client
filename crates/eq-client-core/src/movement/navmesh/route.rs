//! Planning over the mesh: an A* over the polygons and the drops off their
//! ledges, run a budget at a time, then the funnel through the edges they
//! share.

use super::NavMesh;
use glam::{Vec2, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// An edge crossed, its ends left then right for a walker crossing it, and
/// whether each end lies on the mesh's outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Portal {
    pub left: Vec3,
    pub right: Vec3,
    pub left_wall: bool,
    pub right_wall: bool,
}
/// A polygon's best cost, where it was entered, and the polygon and way it
/// was entered by.
type Reached = (f32, Vec3, Option<(u32, Via)>);

/// How a polygon was entered from the one before.
#[derive(Clone, Copy, Debug)]
enum Via {
    /// Across the shared edge of that polygon's link.
    Link(usize),
    /// Off a ledge by the mesh's drop.
    Drop(u32),
}

/// A step along a corridor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Crossing {
    /// Across an edge two polygons share.
    Edge(Portal),
    /// Off a ledge: walk to `top`, then step off and land at `landing`.
    Drop { top: Vec3, landing: Vec3 },
}

/// Boxes a request treats apart from the rest: doors. A move across the
/// floor that passes through one costs a toll, or can't be made. Only the
/// moves through a door's own box are marked, so the rooms on either side of
/// it stay as they are.
#[derive(Clone, Debug, Default)]
pub(super) struct Areas {
    /// Boxes, low corner then high, that cost `toll` more to pass through.
    pub tolled: Vec<(Vec3, Vec3)>,
    /// The toll.
    pub toll: f32,
    /// Boxes no move passes through.
    pub closed: Vec<(Vec3, Vec3)>,
    /// How far below a box's bottom a floor still meets it.
    pub below: f32,
}

impl Areas {
    /// What a straight move from `a` to `b` costs beyond its length; None
    /// when it can't be made.
    fn cost(&self, a: Vec3, b: Vec3) -> Option<f32> {
        let through = |&(low, high): &(Vec3, Vec3)| crosses(a, b, low, high, self.below);
        if self.closed.iter().any(through) {
            return None;
        }
        Some(if self.tolled.iter().any(through) {
            self.toll
        } else {
            0.0
        })
    }
}

/// Whether the segment from `a` to `b` passes through the box from `low` to
/// `high` seen from above, at a height between `below` under its bottom and
/// its top.
fn crosses(a: Vec3, b: Vec3, low: Vec3, high: Vec3, below: f32) -> bool {
    if a.y.max(b.y) < low.y - below || a.y.min(b.y) > high.y {
        return false;
    }
    entry(a, b, low, high).is_some()
}

/// How far along the segment from `a` to `b`, from 0 to 1, it first enters
/// the box from `low` to `high` seen from above; None when it misses it.
pub(super) fn entry(a: Vec3, b: Vec3, low: Vec3, high: Vec3) -> Option<f32> {
    // The slab test, across x and z.
    let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
    for (from, to, low, high) in [(a.x, b.x, low.x, high.x), (a.z, b.z, low.z, high.z)] {
        let delta = to - from;
        if delta.abs() < f32::EPSILON {
            if from < low || from > high {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((low - from) / delta, (high - from) / delta);
        enter = enter.max(t0.min(t1));
        leave = leave.min(t0.max(t1));
        if enter > leave {
            return None;
        }
    }
    Some(enter)
}

/// An A* from the polygon under the start to the polygon under the goal.
pub(super) struct PolygonSearch {
    goal: Vec3,
    to: u32,
    weight: f32,
    areas: Areas,
    /// Whether the search takes the mesh's drops.
    drops: bool,
    best: HashMap<u32, Reached>,
    done: HashSet<u32>,
    open: BinaryHeap<Open>,
    expanded: usize,
}

/// A polygon waiting to be expanded, cheapest estimate first.
struct Open(f32, u32);

impl PartialEq for Open {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
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
        // Reversed, so the heap pops the cheapest; ties by polygon so the
        // order never depends on insertion.
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// How an A* slice ended.
#[derive(Debug, PartialEq)]
pub(super) enum Corridor {
    /// More slices needed.
    Searching,
    /// The steps taken, in order, from the start polygon to the goal's.
    Found(Vec<Crossing>),
    /// No polygon path joins them.
    Failed,
}

impl PolygonSearch {
    /// A search from `start` on polygon `from` to `goal` on polygon `to`,
    /// with the distance still to go weighed `weight` times; with `drops`,
    /// it may step off ledges.
    pub fn new(
        (start, from): (Vec3, u32),
        (goal, to): (Vec3, u32),
        weight: f32,
        areas: Areas,
        drops: bool,
    ) -> Self {
        Self {
            goal,
            to,
            weight,
            areas,
            drops,
            best: HashMap::from([(from, (0.0, start, None))]),
            done: HashSet::new(),
            open: BinaryHeap::from([Open(weight * start.distance(goal), from)]),
            expanded: 0,
        }
    }

    /// Polygons expanded so far.
    pub const fn expanded(&self) -> usize {
        self.expanded
    }

    /// Expands up to `budget` polygons.
    pub fn advance(&mut self, mesh: &NavMesh, budget: usize) -> Corridor {
        for _ in 0..budget.max(1) {
            let Some(Open(_, at)) = self.open.pop() else {
                return Corridor::Failed;
            };
            if at == self.to {
                return Corridor::Found(self.edges(mesh));
            }
            if !self.done.insert(at) {
                continue;
            }
            self.expanded += 1;
            let (cost, point, _) = self.best[&at];
            // Entered at a shared edge's middle, at no extra cost.
            let links = mesh.polygons[at as usize]
                .links
                .iter()
                .enumerate()
                .map(|(k, link)| (link.to, (link.a + link.b) / 2.0, 0.0, Via::Link(k)));
            // Or off a ledge: walked to its top, then the drop's own way to
            // its landing, as the grid charges a drop step.
            let drops = self
                .drops
                .then(|| mesh.drops_from.get(&at))
                .flatten()
                .into_iter()
                .flatten()
                .map(|&d| {
                    let link = &mesh.drops[d as usize];
                    (
                        link.to,
                        link.landing,
                        link.top.distance(link.landing),
                        Via::Drop(d),
                    )
                });
            for (to, entry, extra, via) in links.chain(drops) {
                if self.done.contains(&to) {
                    continue;
                }
                let walked_to = match via {
                    Via::Link(_) => entry,
                    Via::Drop(d) => mesh.drops[d as usize].top,
                };
                // The last polygon adds the way on to the goal.
                let onward = if to == self.to {
                    entry.distance(self.goal)
                } else {
                    0.0
                };
                let Some(toll) = self.areas.cost(point, walked_to) else {
                    continue;
                };
                let next = cost + point.distance(walked_to) + extra + onward + toll;
                if self.best.get(&to).is_none_or(|(old, _, _)| next < *old) {
                    self.best.insert(to, (next, entry, Some((at, via))));
                    let estimate = next + self.weight * entry.distance(self.goal);
                    self.open.push(Open(estimate, to));
                }
            }
        }
        Corridor::Searching
    }

    /// The steps taken: the edges crossed, each set left then right by the
    /// polygon it leaves (its centre lies behind the edge, so the sides never
    /// depend on how the corridor bent before it), and the drops.
    fn edges(&self, mesh: &NavMesh) -> Vec<Crossing> {
        let mut edges = Vec::new();
        let mut at = self.to;
        while let Some(&(_, _, Some((parent, via)))) = self.best.get(&at) {
            at = parent;
            let k = match via {
                Via::Link(k) => k,
                Via::Drop(d) => {
                    let link = &mesh.drops[d as usize];
                    edges.push(Crossing::Drop {
                        top: link.top,
                        landing: link.landing,
                    });
                    continue;
                }
            };
            let polygon = &mesh.polygons[parent as usize];
            let link = &polygon.links[k];
            #[allow(clippy::cast_precision_loss)] // A polygon has a few corners.
            let centre = polygon.vertices.iter().sum::<Vec3>() / polygon.vertices.len() as f32;
            let (left, right) = if left_of(centre, (link.a + link.b) / 2.0, link.a) {
                (link.a, link.b)
            } else {
                (link.b, link.a)
            };
            edges.push(Crossing::Edge(Portal {
                left,
                right,
                left_wall: mesh.on_outline(left),
                right_wall: mesh.on_outline(right),
            }));
        }
        edges.reverse();
        edges
    }
}

/// Whether `c` lies left of the line from `a` to `b`, seen from above.
fn left_of(a: Vec3, b: Vec3, c: Vec3) -> bool {
    cross(a, b, c) >= 0.0
}

/// Positive when `c` lies left of `a`→`b`, seen from above.
fn cross(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    (flat(b) - flat(a)).perp_dot(flat(c) - flat(a))
}

/// The straight legs from `start` to `goal` through `edges`: the funnel
/// (string pulling) across the ground plane, each edge first narrowed by
/// `clearance` from each end on the mesh's outline, so the legs keep that
/// much room from the corners they turn at; an end where polygons only meet
/// is open floor and keeps its place. An edge too narrow for its insets is
/// crossed between them, at its middle when both ends are walls. Each leg
/// ends at the next corner, then at `goal`.
pub(super) fn funnel(start: Vec3, goal: Vec3, edges: &[Portal], clearance: f32) -> Vec<Vec3> {
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    // Each edge narrowed.
    let mut portals: Vec<(Vec3, Vec3)> = vec![(start, start)];
    for edge in edges {
        let (left, right) = (edge.left, edge.right);
        let width = flat(left).distance(flat(right));
        let inset = |wall: bool| if wall { clearance } else { 0.0 };
        let (from_left, from_right) = (inset(edge.left_wall), inset(edge.right_wall));
        portals.push(if width > from_left + from_right {
            (
                left.lerp(right, from_left / width),
                right.lerp(left, from_right / width),
            )
        } else {
            let share = if from_left + from_right > 0.0 {
                from_left / (from_left + from_right)
            } else {
                0.5
            };
            let at = left.lerp(right, share);
            (at, at)
        });
    }
    portals.push((goal, goal));
    let mut points = Vec::new();
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_index, mut right_index) = (0, 0);
    let mut i = 1;
    while i < portals.len() {
        let (next_left, next_right) = portals[i];
        // Tighten the right side, or turn at the left corner it crosses.
        if cross(apex, right, next_right) >= 0.0 {
            // On the left side's line counts as inside, so a corner the
            // straight line only touches is no turn.
            if apex == right || cross(apex, left, next_right) <= 0.0 {
                right = next_right;
                right_index = i;
            } else {
                let turn = left_index;
                apex = left;
                points.push(apex);
                (left, right) = (apex, apex);
                (left_index, right_index) = (turn, turn);
                i = turn + 1;
                continue;
            }
        }
        // Tighten the left side, or turn at the right corner it crosses.
        if cross(apex, left, next_left) <= 0.0 {
            if apex == left || cross(apex, right, next_left) >= 0.0 {
                left = next_left;
                left_index = i;
            } else {
                let turn = right_index;
                apex = right;
                points.push(apex);
                (left, right) = (apex, apex);
                (left_index, right_index) = (turn, turn);
                i = turn + 1;
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

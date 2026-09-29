//! Bounded grid search over walkable ground using the player's own collision steps.
//!
//! Every edge is walked with [`CollisionWorld::step`], so a found path only uses
//! slopes, stairs and gaps that local movement can actually traverse. Where a
//! walk stops at a ledge, the edge may instead drop to the floor below, simulated
//! with the same airborne controller online falls use.

use super::{AirborneController, CollisionWorld, MotionStep, PROVISIONAL_PHYSICS};
use glam::{Vec2, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Horizontal spacing between searched positions.
const CELL: f32 = 2.0;
/// Longest single collision step, keeping slopes within the grounded step limit.
const SUBSTEP: f32 = 1.0;
/// Floors closer together than this share a grid level.
const LEVEL: f32 = 4.0;
/// Expansions after which a search gives up.
const MAX_EXPANSIONS: usize = 60_000;
/// Deepest ledge a path drops off, in world units, keeping falls short.
const MAX_DROP: f32 = 20.0;
/// Airborne slice length, matching the controller's integration limit.
const SLICE: f32 = 0.05;
const NEIGHBORS: [(i16, i16); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

type Key = (i16, i16, i16);

/// Progress of an incremental path search.
#[derive(Debug, PartialEq)]
pub enum PathProgress {
    /// The search needs more calls.
    Searching,
    /// Feet positions after the start, ending within reach of the goal.
    Found(Vec<Vec3>),
    /// No walkable path exists within the search limits.
    Failed,
}

struct Node {
    feet: Vec3,
    cost: f32,
    parent: Option<Key>,
    closed: bool,
}

struct Open {
    estimate: f32,
    key: Key,
}

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
    // Reversed so the binary heap pops the lowest estimate first.
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.total_cmp(&self.estimate)
    }
}

/// A* search that expands a bounded number of positions per call, so callers can
/// spread it across frames.
pub struct PathSearch {
    origin: Vec2,
    goal: Vec3,
    reach: f32,
    height: f32,
    nodes: HashMap<Key, Node>,
    open: BinaryHeap<Open>,
    expanded: usize,
}

impl PathSearch {
    /// Starts a search from the feet position `start` to within `reach` of `goal`
    /// (horizontally) for a character of the given collision height.
    #[must_use]
    pub fn new(start: Vec3, goal: Vec3, reach: f32, height: f32) -> Self {
        let key = (0, 0, level(start.y));
        let mut nodes = HashMap::new();
        nodes.insert(
            key,
            Node {
                feet: start,
                cost: 0.0,
                parent: None,
                closed: false,
            },
        );
        let mut open = BinaryHeap::new();
        open.push(Open {
            estimate: start.distance(goal),
            key,
        });
        Self {
            origin: Vec2::new(start.x, start.z),
            goal,
            reach: reach.max(0.5),
            height,
            nodes,
            open,
            expanded: 0,
        }
    }

    /// Expands up to `budget` positions.
    pub fn advance(&mut self, world: &CollisionWorld, budget: usize) -> PathProgress {
        for _ in 0..budget {
            let Some(Open { key, .. }) = self.open.pop() else {
                return PathProgress::Failed;
            };
            let (feet, cost) = match self.nodes.get_mut(&key) {
                Some(node) if !node.closed => {
                    node.closed = true;
                    (node.feet, node.cost)
                }
                _ => continue,
            };
            if self.arrived(feet) {
                return PathProgress::Found(self.trace(key));
            }
            self.expanded += 1;
            if self.expanded > MAX_EXPANSIONS {
                return PathProgress::Failed;
            }
            for (di, dj) in NEIGHBORS {
                let (Some(i), Some(j)) = (key.0.checked_add(di), key.1.checked_add(dj)) else {
                    continue;
                };
                let target = self.origin + Vec2::new(f32::from(i), f32::from(j)) * CELL;
                let Some(next_feet) = walk(world, feet, target, self.height)
                    .or_else(|| drop(world, feet, target, self.height))
                else {
                    continue;
                };
                let next = (i, j, level(next_feet.y));
                let next_cost = cost + feet.distance(next_feet);
                if self
                    .nodes
                    .get(&next)
                    .is_some_and(|node| node.closed || node.cost <= next_cost)
                {
                    continue;
                }
                self.nodes.insert(
                    next,
                    Node {
                        feet: next_feet,
                        cost: next_cost,
                        parent: Some(key),
                        closed: false,
                    },
                );
                self.open.push(Open {
                    estimate: next_cost + next_feet.distance(self.goal),
                    key: next,
                });
            }
        }
        PathProgress::Searching
    }

    /// The path to the reached position nearest the goal, for when no full path
    /// exists; None when nothing beyond the start was reachable.
    #[must_use]
    pub fn closest(&self) -> Option<Vec<Vec3>> {
        let flat = |feet: Vec3| Vec2::new(feet.x - self.goal.x, feet.z - self.goal.z).length();
        let (key, _) = self
            .nodes
            .iter()
            .filter(|(_, node)| node.parent.is_some())
            .min_by(|(_, a), (_, b)| flat(a.feet).total_cmp(&flat(b.feet)))?;
        Some(self.trace(*key))
    }

    /// How many positions the search has reached.
    #[must_use]
    pub fn reached(&self) -> usize {
        self.nodes.len()
    }

    fn arrived(&self, feet: Vec3) -> bool {
        Vec2::new(feet.x - self.goal.x, feet.z - self.goal.z).length() <= self.reach
            && (feet.y - self.goal.y).abs() <= self.reach.max(10.0)
    }

    fn trace(&self, mut key: Key) -> Vec<Vec3> {
        let mut path = Vec::new();
        while let Some(node) = self.nodes.get(&key) {
            let Some(parent) = node.parent else {
                break;
            };
            path.push(node.feet);
            key = parent;
        }
        path.reverse();
        path
    }
}

#[allow(clippy::cast_possible_truncation)] // Zone heights are far inside i16 levels.
fn level(y: f32) -> i16 {
    (y / LEVEL).round() as i16
}

/// Walks toward a horizontal position in short collision steps; None when a wall,
/// ledge or steep slope stops the character short of it.
fn walk(world: &CollisionWorld, from: Vec3, to: Vec2, height: f32) -> Option<Vec3> {
    let delta = Vec3::new(to.x - from.x, 0.0, to.y - from.z);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few grid steps.
    let steps = (delta.length() / SUBSTEP).ceil().max(1.0) as u8;
    let part = delta / f32::from(steps);
    let mut feet = from;
    for _ in 0..steps {
        feet = world.step(feet, part, height);
    }
    (Vec2::new(feet.x - to.x, feet.z - to.y).length() < 0.25).then_some(feet)
}

/// Steps off a ledge toward a horizontal position and falls to the floor below;
/// None when a wall blocks the way or the drop exceeds [`MAX_DROP`].
fn drop(world: &CollisionWorld, from: Vec3, to: Vec2, height: f32) -> Option<Vec3> {
    let mut airborne = AirborneController::default();
    let step = |airborne: &mut AirborneController, feet, horizontal| {
        airborne.step(
            world,
            feet,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal,
                jump: false,
                seconds: SLICE,
                height,
            },
        )
    };
    let delta = Vec3::new(to.x - from.x, 0.0, to.y - from.z);
    let mut feet = from;
    for _ in 0..4 {
        feet = step(&mut airborne, feet, delta / 4.0);
    }
    if Vec2::new(feet.x - to.x, feet.z - to.y).length() >= 0.25 {
        return None;
    }
    // Two seconds of falling reaches terminal speed well past the drop limit.
    for _ in 0..40 {
        if let Some(landing) = airborne.take_landing() {
            return (landing.fall_distance <= MAX_DROP).then_some(feet);
        }
        if from.y - feet.y > MAX_DROP {
            return None;
        }
        feet = step(&mut airborne, feet, Vec3::ZERO);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    fn run(world: &CollisionWorld, start: Vec3, goal: Vec3) -> PathProgress {
        let mut search = PathSearch::new(start, goal, 2.0, 6.0);
        loop {
            match search.advance(world, 64) {
                PathProgress::Searching => (),
                done => return done,
            }
        }
    }

    #[test]
    fn paths_drop_off_ledges_but_never_climb_them() {
        // A platform five units above the floor, ending in a wall at x = 0.
        let mut triangles = quad(
            [-30.0, 5.0, -30.0],
            [0.0, 5.0, -30.0],
            [0.0, 5.0, 30.0],
            [-30.0, 5.0, 30.0],
        )
        .to_vec();
        triangles.extend(quad(
            [0.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [0.0, 0.0, 30.0],
        ));
        triangles.extend(quad(
            [0.0, 0.0, -30.0],
            [0.0, 0.0, 30.0],
            [0.0, 5.0, 30.0],
            [0.0, 5.0, -30.0],
        ));
        let world = CollisionWorld::new(triangles).unwrap();
        let (high, low) = (Vec3::new(-10.0, 5.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
        let PathProgress::Found(path) = run(&world, high, low) else {
            panic!("expected a drop off the ledge");
        };
        assert!(path.last().unwrap().y.abs() < 0.01, "{path:?}");
        assert!(path.iter().any(|feet| feet.x > 0.0 && feet.y < 0.01));
        assert_eq!(run(&world, low, high), PathProgress::Failed);
    }

    #[test]
    fn paths_walk_around_walls_and_fail_when_enclosed() {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        // A wall across z = 0 that leaves a gap for x > 10.
        triangles.extend(quad(
            [-30.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [-30.0, 10.0, 0.0],
        ));
        let world = CollisionWorld::new(triangles.clone()).unwrap();
        let start = Vec3::new(0.0, 0.0, -10.0);
        let goal = Vec3::new(0.0, 0.0, 10.0);
        let PathProgress::Found(path) = run(&world, start, goal) else {
            panic!("expected a path around the wall");
        };
        assert!(path.iter().any(|feet| feet.x > 10.0), "{path:?}");
        let end = path.last().copied().unwrap();
        assert!(Vec2::new(end.x - goal.x, end.z - goal.z).length() <= 2.0);
        // Each waypoint is reachable from the previous one with ordinary steps.
        let mut previous = start;
        for feet in &path {
            assert!(walk(&world, previous, Vec2::new(feet.x, feet.z), 6.0).is_some());
            previous = *feet;
        }

        // Closing the gap leaves no route.
        triangles.extend(quad(
            [10.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [30.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ));
        let world = CollisionWorld::new(triangles).unwrap();
        assert_eq!(run(&world, start, goal), PathProgress::Failed);
        // The nearest reachable position is at the wall, on the start's side.
        let mut search = PathSearch::new(start, goal, 2.0, 6.0);
        while search.advance(&world, 64) == PathProgress::Searching {}
        let end = *search.closest().unwrap().last().unwrap();
        assert!(end.z < 0.0 && end.z > -3.0, "{end:?}");
        assert!(search.reached() > 100);
    }
}

//! Where a route's searches come from: the interface a route plans through,
//! and the grid search behind it.
//!
//! A [`Navigator`] turns a [`SearchRequest`] into an incremental
//! [`RouteSearch`]; [`super::Route`] follows whatever it finds. Each
//! implementation is free in how it searches, but every path it returns must
//! be walkable by the walker's own collision steps.

use super::{CollisionWorld, PathProgress, PathSearch};
use glam::Vec3;

/// How much a search of the whole way weighs the distance still to go. On
/// two long city routes (9,554 and 13,849 expansions unweighted) it expanded
/// 76% and 62% fewer positions (2,321 and 5,260), for paths 2.7% and 3.5%
/// longer; every weight from 1.25 to 2 kept both within 5%. A path is never
/// more than this many times the cost of the shortest. A local search stays
/// unweighted: it is short already.
pub(super) const SEARCH_WEIGHT: f32 = 2.0;
/// Expansions a local search may take: a few rooms' worth, so it stays local
/// and never stands the walker still for long.
pub(super) const LOCAL_EXPANSIONS: usize = 3000;

/// How far a search looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The whole way to the goal.
    Whole,
    /// A short way round something near the walker, such as an opened door;
    /// it gives up rather than wander far.
    Local,
}

/// What a route asks a search for.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchRequest {
    /// The walker's feet.
    pub start: Vec3,
    /// Where to end, within `reach` of it.
    pub goal: Vec3,
    /// How near the goal counts as there.
    pub reach: f32,
    /// The walker's collision height.
    pub height: f32,
    /// Closed doors the path may pass by opening them.
    pub doors: Vec<u8>,
    /// Whether the path may step off ledges.
    pub drops: bool,
    /// How far the search looks.
    pub scope: Scope,
}

/// A search in progress, advanced a budget at a time, on the caller's thread
/// or another.
pub trait RouteSearch: Send + Sync {
    /// Advances by up to `budget` units of work in `world`.
    fn advance(&mut self, world: &CollisionWorld, budget: usize) -> PathProgress;
    /// The path to the reached position nearest the goal, for a route that
    /// ends as close as it can.
    fn closest(&self) -> Option<Vec<Vec3>>;
    /// For each position of the found path, or else of the closest one, the
    /// closed door the step to it passes through.
    fn doors_along(&self) -> Vec<Option<u8>>;
    /// Work done so far, and whether any remained when it ended, for
    /// diagnostics.
    fn searched(&self) -> (usize, bool);
    /// What planned the search, for diagnostics.
    fn planned(&self) -> Planned {
        Planned::Grid
    }
}

/// What planned a search.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Planned {
    /// The grid search, throughout.
    #[default]
    Grid,
    /// The navigation mesh, its legs checked by the walker's edge tests.
    Mesh {
        /// Legs the mesh gave.
        legs: usize,
        /// Legs that also kept the clearance from walls to either side.
        clear: usize,
        /// Drops off ledges the walker's own drop made.
        drops: usize,
        /// The legs a short grid search replaced, each as where it began
        /// and where the mesh had it end.
        repaired: Vec<(Vec3, Vec3)>,
        /// Why the grid took over the whole search, if it did.
        fallback: Option<Fallback>,
    },
}

/// Why a mesh search handed the whole way to the grid.
#[derive(Clone, Debug, PartialEq)]
pub enum Fallback {
    /// The start or the goal is off the mesh.
    OffMesh,
    /// The start and the goal lie on islands the mesh doesn't join.
    OtherIsland,
    /// No polygon path joins them past the doors that can't be opened.
    NoCorridor,
    /// A leg failed the walker's edge tests and a short grid search found no
    /// way between its ends.
    Leg {
        /// Where the leg began.
        from: Vec3,
        /// Where it ended.
        to: Vec3,
    },
}

/// Plans a route's searches.
pub trait Navigator: Send + Sync {
    /// Starts a search for `request`.
    fn search(&self, request: SearchRequest) -> Box<dyn RouteSearch>;
}

/// Searches a grid of positions over walkable ground, walking every edge with
/// the walker's own collision steps: [`PathSearch`].
#[derive(Clone, Copy, Debug, Default)]
pub struct GridNavigator;

impl Navigator for GridNavigator {
    fn search(&self, request: SearchRequest) -> Box<dyn RouteSearch> {
        let search = PathSearch::new(request.start, request.goal, request.reach, request.height)
            .with_drops(request.drops)
            .with_doors(request.doors);
        Box::new(match request.scope {
            Scope::Whole => search.with_weight(SEARCH_WEIGHT),
            Scope::Local => search.with_max_expansions(LOCAL_EXPANSIONS),
        })
    }
}

impl RouteSearch for PathSearch {
    fn advance(&mut self, world: &CollisionWorld, budget: usize) -> PathProgress {
        Self::advance(self, world, budget)
    }

    fn closest(&self) -> Option<Vec<Vec3>> {
        Self::closest(self)
    }

    fn doors_along(&self) -> Vec<Option<u8>> {
        Self::doors_along(self)
    }

    fn searched(&self) -> (usize, bool) {
        (self.expanded(), self.has_open())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// A floor with a wall across it, open at one end or shut along its length.
    fn walled(gap: bool) -> CollisionWorld {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        let end = if gap { 10.0 } else { 30.0 };
        triangles.extend(quad(
            [-30.0, 0.0, 0.0],
            [end, 0.0, 0.0],
            [end, 10.0, 0.0],
            [-30.0, 10.0, 0.0],
        ));
        CollisionWorld::new(triangles).unwrap()
    }

    fn run(search: &mut dyn RouteSearch, world: &CollisionWorld) -> PathProgress {
        loop {
            match search.advance(world, 64) {
                PathProgress::Searching => (),
                done => break done,
            }
        }
    }

    #[test]
    fn the_grid_navigator_finds_what_the_route_searched_before_it() {
        let (start, goal) = (Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0));
        for gap in [true, false] {
            let world = walled(gap);
            for scope in [Scope::Whole, Scope::Local] {
                // The search a route made before it planned through a navigator.
                let before = PathSearch::new(start, goal, 3.0, 6.0)
                    .with_drops(true)
                    .with_doors(vec![7]);
                let mut before = match scope {
                    Scope::Whole => before.with_weight(2.0),
                    Scope::Local => before.with_max_expansions(3000),
                };
                let mut now = GridNavigator.search(SearchRequest {
                    start,
                    goal,
                    reach: 3.0,
                    height: 6.0,
                    doors: vec![7],
                    drops: true,
                    scope,
                });
                let found = run(&mut before, &world);
                assert_eq!(run(&mut *now, &world), found, "gap {gap}, {scope:?}");
                assert_eq!(matches!(found, PathProgress::Found(_)), gap);
                assert_eq!(now.closest(), before.closest());
                assert_eq!(now.doors_along(), before.doors_along());
                assert_eq!(now.searched(), (before.expanded(), before.has_open()));
            }
        }
    }
}

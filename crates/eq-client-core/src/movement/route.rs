//! Follows a searched path from wherever the character currently stands.
//!
//! The route owns the search and the waypoints still ahead; callers only report
//! the character's feet and the goal each tick and steer toward what comes back.

use super::{CollisionWorld, PathProgress, PathSearch};
use glam::{Vec2, Vec3};
use std::collections::VecDeque;

/// A waypoint counts as reached within this flat distance, about one online
/// movement sample, so a step never overshoots a corner and turns back.
const WAYPOINT_REACH: f32 = 3.5;
/// The last waypoint needs half a movement sample, so a route ends where it was
/// found to end without one step overshooting past it.
const FINAL_REACH: f32 = 1.5;

/// What a follower should do next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteStep {
    /// The search needs more ticks; stay put.
    Searching,
    /// Head for this feet position.
    Toward(Vec3),
    /// Within reach of the goal, or at the end of the nearest route found.
    Arrived,
    /// Nothing beyond the start is reachable.
    Unreachable,
}

/// An incremental search, then the waypoints still ahead.
pub struct Route {
    search: Option<PathSearch>,
    waypoints: VecDeque<Vec3>,
    reach: f32,
    partial: bool,
}

impl Route {
    /// Starts a search from `feet` to within `reach` of `goal` for a character of
    /// the given collision height.
    #[must_use]
    pub fn new(feet: Vec3, goal: Vec3, reach: f32, height: f32) -> Self {
        Self {
            search: Some(PathSearch::new(feet, goal, reach, height)),
            waypoints: VecDeque::new(),
            reach,
            partial: false,
        }
    }

    /// Whether the search is still running.
    #[must_use]
    pub const fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    /// Whether no full path existed and the route ends at the nearest reachable point.
    #[must_use]
    pub const fn partial(&self) -> bool {
        self.partial
    }

    /// Waypoints still ahead.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.waypoints.len()
    }

    /// Advances the search by up to `budget` positions, then says where to head
    /// from `feet`. `goal` is the target's current position; the route ends as
    /// soon as it is within reach, even before the last waypoint.
    pub fn next(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        goal: Vec3,
        budget: usize,
    ) -> RouteStep {
        let flat = |v: Vec3| Vec2::new(v.x, v.z).length();
        if flat(goal - feet) <= self.reach {
            return RouteStep::Arrived;
        }
        if let Some(search) = &mut self.search {
            let path = match search.advance(world, budget) {
                PathProgress::Searching => return RouteStep::Searching,
                PathProgress::Found(path) => path,
                PathProgress::Failed => {
                    let Some(path) = search.closest() else {
                        return RouteStep::Unreachable;
                    };
                    self.partial = true;
                    path
                }
            };
            self.waypoints = path.into();
            self.search = None;
        }
        while let Some(waypoint) = self.waypoints.front() {
            let reach = if self.waypoints.len() > 1 {
                WAYPOINT_REACH
            } else {
                FINAL_REACH
            };
            if flat(*waypoint - feet) > reach {
                break;
            }
            self.waypoints.pop_front();
        }
        match self.waypoints.front() {
            Some(next) => RouteStep::Toward(*next),
            // A full route's last waypoint lies within reach of the goal, so the
            // rest of the way is a short straight walk.
            None if !self.partial => RouteStep::Toward(goal),
            None => RouteStep::Arrived,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// Walks straight to each waypoint the route names, as a follower would.
    fn follow(route: &mut Route, world: &CollisionWorld, mut feet: Vec3, goal: Vec3) -> Vec3 {
        for _ in 0..500 {
            match route.next(world, feet, goal, 256) {
                RouteStep::Searching => (),
                RouteStep::Toward(next) => {
                    let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z);
                    feet = world.step(feet, to.clamp_length_max(3.0), 6.0);
                }
                RouteStep::Arrived => return feet,
                RouteStep::Unreachable => panic!("route found nothing reachable"),
            }
        }
        panic!("route did not finish");
    }

    #[test]
    fn a_follower_reaches_the_goal_around_a_wall_or_stops_as_close_as_it_can() {
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
        let (start, goal) = (Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 3.0, 6.0);
        let end = follow(&mut route, &world, start, goal);
        assert!(
            Vec2::new(end.x - goal.x, end.z - goal.z).length() <= 3.0,
            "{end:?}"
        );
        assert!(!route.partial());

        // Closing the gap leaves only the nearest point on this side of the wall.
        triangles.extend(quad(
            [10.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [30.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ));
        let world = CollisionWorld::new(triangles).unwrap();
        let mut route = Route::new(start, goal, 3.0, 6.0);
        let end = follow(&mut route, &world, start, goal);
        assert!(route.partial());
        // At the nearest point on the start's side of the wall.
        assert!(end.z < 0.0 && end.z > -2.0 - FINAL_REACH - 0.1, "{end:?}");
    }

    #[test]
    fn a_start_with_no_way_out_is_unreachable() {
        // A floor too small for any step, with nothing below to drop onto.
        let world = CollisionWorld::new(quad(
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 0.0, 1.0],
        ))
        .unwrap();
        let mut route = Route::new(Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), 2.0, 6.0);
        let step = loop {
            match route.next(&world, Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), 64) {
                RouteStep::Searching => (),
                done => break done,
            }
        };
        assert_eq!(step, RouteStep::Unreachable);
    }
}

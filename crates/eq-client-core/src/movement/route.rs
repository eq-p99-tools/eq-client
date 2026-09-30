//! Follows a searched path from wherever the character currently stands.
//!
//! The route owns the search and the waypoints still ahead; callers only report
//! the character's feet and the goal each tick and steer toward what comes back.

use super::{CollisionWorld, PathProgress, PathSearch};
use glam::{Vec2, Vec3};
use std::{collections::VecDeque, time::Duration};

/// A waypoint counts as reached within this flat distance, about one online
/// movement sample, so a step never overshoots a corner and turns back.
const WAYPOINT_REACH: f32 = 3.5;
/// The last waypoint needs half a movement sample, so a route ends where it was
/// found to end without one step overshooting past it.
const FINAL_REACH: f32 = 1.5;
/// Following that shortens the remaining route by less than this...
const PROGRESS: f32 = 1.0;
/// ...over this much steering time has stalled, whatever stopped it: geometry
/// the search misjudged, a refused movement sample, or something in the way.
const STALL: Duration = Duration::from_secs(3);
/// The most steering time one call can add, so a pause or a long frame between
/// calls is never mistaken for a stall.
const MAX_TICK: Duration = Duration::from_millis(250);

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
    /// Following stopped making progress, and again after a fresh search from
    /// where the character stood.
    Stalled,
}

/// An incremental search, then the waypoints still ahead.
pub struct Route {
    search: Option<PathSearch>,
    waypoints: VecDeque<Vec3>,
    reach: f32,
    height: f32,
    partial: bool,
    /// Steering time so far, counted only while heading somewhere.
    steering: Duration,
    last_call: Option<Duration>,
    /// The shortest remaining route distance so far, and the steering time when
    /// it last shrank by a whole `PROGRESS`.
    best: Option<(f32, Duration)>,
    replanned: bool,
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
            height,
            partial: false,
            steering: Duration::ZERO,
            last_call: None,
            best: None,
            replanned: false,
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
    /// soon as it is within reach, even before the last waypoint. `now` is any
    /// monotonic clock. Following that stops shortening the remaining route
    /// searches again once from where the character stands, then reports
    /// [`RouteStep::Stalled`].
    pub fn next(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        goal: Vec3,
        budget: usize,
        now: Duration,
    ) -> RouteStep {
        let tick = self.last_call.map_or(Duration::ZERO, |last| {
            now.saturating_sub(last).min(MAX_TICK)
        });
        self.last_call = Some(now);
        let step = self.steer(world, feet, goal, budget);
        if !matches!(step, RouteStep::Toward(_)) {
            return step;
        }
        self.steering += tick;
        let left = self.left(feet, goal);
        match self.best {
            Some((best, since)) if left > best - PROGRESS => {
                if self.steering.saturating_sub(since) < STALL {
                    step
                } else if std::mem::replace(&mut self.replanned, true) {
                    RouteStep::Stalled
                } else {
                    self.search = Some(PathSearch::new(feet, goal, self.reach, self.height));
                    self.waypoints.clear();
                    self.partial = false;
                    self.best = None;
                    RouteStep::Searching
                }
            }
            _ => {
                self.best = Some((left, self.steering));
                step
            }
        }
    }

    /// Flat distance still to cover along the waypoints, then on to the goal
    /// unless the route ends short of it.
    fn left(&self, feet: Vec3, goal: Vec3) -> f32 {
        let (mut total, mut at) = (0.0, feet);
        for waypoint in &self.waypoints {
            total += flat(*waypoint - at);
            at = *waypoint;
        }
        if self.partial {
            total
        } else {
            total + flat(goal - at)
        }
    }

    fn steer(
        &mut self,
        world: &CollisionWorld,
        feet: Vec3,
        goal: Vec3,
        budget: usize,
    ) -> RouteStep {
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

/// Distance ignoring height.
fn flat(v: Vec3) -> f32 {
    Vec2::new(v.x, v.z).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// Walks straight to each waypoint the route names, as a follower would.
    fn follow(route: &mut Route, world: &CollisionWorld, mut feet: Vec3, goal: Vec3) -> Vec3 {
        for tick in 0..500_u64 {
            match route.next(world, feet, goal, 256, Duration::from_millis(tick * 150)) {
                RouteStep::Searching => (),
                RouteStep::Toward(next) => {
                    let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z);
                    feet = world.step(feet, to.clamp_length_max(3.0), 6.0);
                }
                RouteStep::Arrived => return feet,
                RouteStep::Unreachable | RouteStep::Stalled => panic!("route did not get there"),
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
            match route.next(
                &world,
                Vec3::ZERO,
                Vec3::new(20.0, 0.0, 0.0),
                64,
                Duration::ZERO,
            ) {
                RouteStep::Searching => (),
                done => break done,
            }
        };
        assert_eq!(step, RouteStep::Unreachable);
    }

    fn open_field() -> CollisionWorld {
        CollisionWorld::new(quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        ))
        .unwrap()
    }

    /// Ticks every 100 ms with feet placed by `feet_at`, returning each step.
    fn ticks(
        route: &mut Route,
        world: &CollisionWorld,
        goal: Vec3,
        count: u16,
        feet_at: impl Fn(u16) -> Vec3,
    ) -> Vec<RouteStep> {
        (0..count)
            .map(|tick| {
                let now = Duration::from_millis(u64::from(tick) * 100);
                route.next(world, feet_at(tick), goal, 4096, now)
            })
            .collect()
    }

    #[test]
    fn a_follower_that_stops_moving_searches_again_once_then_stalls() {
        let world = open_field();
        let goal = Vec3::new(20.0, 0.0, 0.0);
        let mut route = Route::new(Vec3::ZERO, goal, 2.0, 6.0);
        // Something keeps the feet where they are while the route says to go.
        let steps = ticks(&mut route, &world, goal, 100, |_| Vec3::ZERO);
        let stalled = steps.iter().position(|step| *step == RouteStep::Stalled);
        // Three seconds, a fresh search, three more seconds.
        assert!(
            stalled.is_some_and(|tick| (60..=64).contains(&tick)),
            "{stalled:?}"
        );
        let searches = steps[1..stalled.unwrap()]
            .iter()
            .filter(|step| **step == RouteStep::Searching)
            .count();
        assert_eq!(searches, 1);
    }

    #[test]
    fn sliding_back_and_forth_is_not_progress_but_slow_steady_walking_is() {
        let world = open_field();
        let goal = Vec3::new(25.0, 0.0, 0.0);
        // Across the route and back, without getting any closer.
        let mut route = Route::new(Vec3::ZERO, goal, 2.0, 6.0);
        let sliding = ticks(&mut route, &world, goal, 100, |tick| {
            Vec3::new(0.0, 0.0, if tick % 20 < 10 { 1.5 } else { -1.5 })
        });
        assert!(sliding.contains(&RouteStep::Stalled));
        // Half a unit per second: a whole unit every two seconds.
        let mut route = Route::new(Vec3::ZERO, goal, 2.0, 6.0);
        let walking = ticks(&mut route, &world, goal, 150, |tick| {
            Vec3::new(f32::from(tick) * 0.05, 0.0, 0.0)
        });
        assert!(
            walking[1..]
                .iter()
                .all(|step| matches!(step, RouteStep::Toward(_))),
            "{walking:?}"
        );
    }

    #[test]
    fn time_between_calls_is_not_steering_time() {
        let world = open_field();
        let goal = Vec3::new(20.0, 0.0, 0.0);
        let mut route = Route::new(Vec3::ZERO, goal, 2.0, 6.0);
        // Paused for a minute between calls, as when the window loses focus.
        for minute in 0..5_u64 {
            let step = loop {
                let now = Duration::from_secs(minute * 60);
                match route.next(&world, Vec3::ZERO, goal, 4096, now) {
                    RouteStep::Searching => (),
                    step => break step,
                }
            };
            assert!(matches!(step, RouteStep::Toward(_)), "{step:?}");
        }
    }
}

//! Follows a searched path from wherever the character currently stands.
//!
//! The route owns the search and the waypoints still ahead; callers only report
//! the character's feet and the goal each tick and steer toward what comes back.

use super::{CollisionWorld, MAX_GROUNDED_STEP, PathProgress, PathSearch, path};
use glam::{Vec2, Vec3};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Within this flat distance of a waypoint, about one online movement sample,
/// a follower may head straight for the one after it if that is walkable from
/// here, so a step never overshoots a corner and turns back.
const WAYPOINT_REACH: f32 = 3.5;
/// A waypoint is reached within half a movement sample, so a route ends where
/// it was found to end without one step overshooting past it.
const FINAL_REACH: f32 = 1.5;
/// Following that shortens the remaining route by less than this...
const PROGRESS: f32 = 1.0;
/// ...over this much steering time has stalled, whatever stopped it: geometry
/// the search misjudged, a refused movement sample, or something in the way.
const STALL: Duration = Duration::from_secs(3);
/// The most steering time one call can add, so a pause or a long frame between
/// calls is never mistaken for a stall.
const MAX_TICK: Duration = Duration::from_millis(250);

/// How far ahead a follower looks for a listed door shut across its way:
/// about one online movement sample.
const DOOR_PROBE: f32 = 4.0;
/// How near the way a door's collider must come to be the one in the way.
const DOOR_MARGIN: f32 = 2.0;
/// A step round an opened door rejoins the route at its first waypoint at
/// least this far from the feet, beyond the door's leaf.
const DETOUR_REJOIN: f32 = 8.0;
/// Expansions a step round an opened door may take: a few rooms' worth, so
/// it stays local and never stands the walker still for long.
const DETOUR_EXPANSIONS: usize = 3000;

/// How much a search of the whole way weighs the distance still to go. On
/// two long city routes (9,554 and 13,849 expansions unweighted) it expanded
/// 33% and 26% fewer positions, for paths at most 1.91 units (0.3%) longer;
/// a path is never more than this many times the cost of the shortest. A
/// step round a door stays unweighted: it is short already.
const SEARCH_WEIGHT: f32 = 1.25;
/// Expansions a search off the caller's thread makes between checks that its
/// route still wants it.
const BACKGROUND_CHUNK: usize = 500;

/// Runs a route's searches away from the caller, so a long search never holds
/// up whoever steers the walker.
pub trait Background: Send + Sync {
    /// Runs `job` to completion somewhere other than the calling thread.
    fn spawn(&self, job: Box<dyn FnOnce() + Send>);
}

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
    /// The next leg passes through this closed door: open it, then ask again.
    OpenDoor(u8),
}

/// The one fresh search a route may make, after a stall or a refused door.
#[derive(Clone, Copy, PartialEq)]
enum Replan {
    Ready,
    Spent,
    /// A door was refused with the search already spent: the route is over.
    Stalled,
}

/// Where a search handed off spent its time, for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchTiming {
    /// Taking the world's snapshot, on the caller's thread.
    pub snapshot: Duration,
    /// Handing the search to the background runner, on the caller's thread.
    pub handoff: Duration,
    /// Waiting for a background thread to start it.
    pub queued: Duration,
    /// Searching and straightening, on the background thread.
    pub ran: Duration,
    /// Obstacles in the snapshot.
    pub obstacles: usize,
}

/// A finished search, straightened from where it began.
struct Finished {
    /// The waypoints to follow, each with the closed door the leg to it
    /// passes; None when the search gives the route nothing to follow.
    legs: Option<Vec<(Vec3, Option<u8>)>>,
    /// Whether the search reached its goal.
    found: bool,
    /// Expansions, and whether positions remained.
    searched: (usize, bool),
    /// For a search handed off, where its time went.
    timing: Option<SearchTiming>,
}

impl Finished {
    /// Straightens what `search` found from `feet`; `partial` takes the nearest
    /// reached point when it found no way to the goal.
    fn new(
        search: &PathSearch,
        progress: PathProgress,
        world: &CollisionWorld,
        feet: Vec3,
        partial: bool,
    ) -> Self {
        let found = matches!(progress, PathProgress::Found(_));
        let path = match progress {
            PathProgress::Found(path) => Some(path),
            _ if partial => search.closest(),
            _ => None,
        };
        let legs = path.map(|path| {
            let doors = search.doors_along();
            path::straightened(world, feet, &path, search.height())
                .into_iter()
                .map(|index| (path[index], doors.get(index).copied().flatten()))
                .collect()
        });
        Self {
            legs,
            found,
            searched: (search.expanded(), search.has_open()),
            timing: None,
        }
    }
}

/// A search started off the caller's thread, and the slot it reports into.
struct Running {
    result: Arc<Mutex<Option<Finished>>>,
    cancel: Arc<AtomicBool>,
    /// The caller's share of the handoff: snapshot, handoff and obstacles.
    timing: SearchTiming,
}

impl Running {
    /// Hands `search` and a snapshot of `world` to `background`.
    fn spawn(
        background: &dyn Background,
        mut search: PathSearch,
        world: &CollisionWorld,
        feet: Vec3,
        partial: bool,
    ) -> Self {
        let taken = Instant::now();
        let world = world.clone();
        let mut timing = SearchTiming {
            snapshot: taken.elapsed(),
            obstacles: world.obstacle_count(),
            ..SearchTiming::default()
        };
        let result: Arc<Mutex<Option<Finished>>> = Arc::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let (slot, stop) = (Arc::clone(&result), Arc::clone(&cancel));
        let spawned = Instant::now();
        background.spawn(Box::new(move || {
            let began = Instant::now();
            let progress = loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match search.advance(&world, BACKGROUND_CHUNK) {
                    PathProgress::Searching => (),
                    done => break done,
                }
            };
            let mut finished = Finished::new(&search, progress, &world, feet, partial);
            finished.timing = Some(SearchTiming {
                queued: began.duration_since(spawned),
                ran: began.elapsed(),
                ..SearchTiming::default()
            });
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(finished);
            }
        }));
        timing.handoff = spawned.elapsed();
        Self {
            result,
            cancel,
            timing,
        }
    }

    /// The search's result once it has one. A job that ended without one
    /// (it panicked) reports nothing found.
    fn poll(&self) -> Option<Finished> {
        let finished = self.result.lock().ok().and_then(|mut slot| slot.take());
        let finished = finished.map(|mut finished| {
            finished.timing = finished.timing.map(|ran| SearchTiming {
                queued: ran.queued,
                ran: ran.ran,
                ..self.timing
            });
            finished
        });
        finished.or_else(|| {
            (Arc::strong_count(&self.result) == 1).then_some(Finished {
                legs: None,
                found: false,
                searched: (0, false),
                timing: None,
            })
        })
    }
}

impl Drop for Running {
    /// A search its route no longer wants stops at its next check.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A search in progress: run a budget at a time by the caller, or handed off.
enum Job {
    Inline(Box<PathSearch>),
    Running(Running),
}

/// A route's search under way.
struct Search {
    job: Job,
    /// For a step round an opened door, how many of the waypoints ahead its
    /// path replaces; None for a search of the whole way.
    replaces: Option<usize>,
}

impl Search {
    fn new(search: PathSearch, replaces: Option<usize>) -> Self {
        Self {
            job: Job::Inline(Box::new(search)),
            replaces,
        }
    }

    /// Advances the search by `budget` positions, or hands it to `background`
    /// with a snapshot of `world` on its first call; the finished search, or
    /// the search still under way.
    fn poll(
        self,
        world: &CollisionWorld,
        feet: Vec3,
        budget: usize,
        background: Option<&dyn Background>,
    ) -> Result<(Finished, Option<usize>), Self> {
        let Self { job, replaces } = self;
        let partial = replaces.is_none();
        match (job, background) {
            (Job::Inline(search), Some(background)) => Err(Self {
                job: Job::Running(Running::spawn(background, *search, world, feet, partial)),
                replaces,
            }),
            (Job::Inline(mut search), None) => match search.advance(world, budget) {
                PathProgress::Searching => Err(Self {
                    job: Job::Inline(search),
                    replaces,
                }),
                done => Ok((Finished::new(&search, done, world, feet, partial), replaces)),
            },
            (Job::Running(running), _) => match running.poll() {
                Some(finished) => Ok((finished, replaces)),
                None => Err(Self {
                    job: Job::Running(running),
                    replaces,
                }),
            },
        }
    }
}

/// An incremental search, then the waypoints still ahead.
pub struct Route {
    search: Option<Search>,
    waypoints: VecDeque<Vec3>,
    /// For each waypoint, the closed door the leg to it passes through.
    crossings: VecDeque<Option<u8>>,
    /// Closed doors the route may pass by opening them.
    doors: Vec<u8>,
    /// Where searches run when not a budget at a time in [`Route::next`].
    background: Option<Arc<dyn Background>>,
    /// Whether the last search of the whole way reached nothing beyond the start.
    unreachable: bool,
    /// Whether the last finished search was a step round a door, and whether
    /// it found one; None after a search of the whole way.
    detoured: Option<bool>,
    reach: f32,
    height: f32,
    partial: bool,
    /// Steering time so far, counted only while heading somewhere.
    steering: Duration,
    last_call: Option<Duration>,
    /// The shortest remaining route distance so far, and the steering time when
    /// it last shrank by a whole `PROGRESS`.
    best: Option<(f32, Duration)>,
    /// Whether the one fresh search a route may make has been spent, and
    /// whether a refused door came after that.
    replan: Replan,
    /// Whether the route may step off ledges; see [`PathSearch::with_drops`].
    drops: bool,
    /// The last finished search's expansions, and whether positions remained.
    searched: Option<(usize, bool)>,
    /// How long the last finished search handed off waited to start, and ran.
    timing: Option<SearchTiming>,
}

impl Route {
    /// Starts a search from `feet` to within `reach` of `goal` for a character of
    /// the given collision height.
    #[must_use]
    pub fn new(feet: Vec3, goal: Vec3, reach: f32, height: f32) -> Self {
        Self {
            search: Some(Search::new(
                PathSearch::new(feet, goal, reach, height).with_weight(SEARCH_WEIGHT),
                None,
            )),
            drops: false,
            waypoints: VecDeque::new(),
            crossings: VecDeque::new(),
            doors: Vec::new(),
            background: None,
            unreachable: false,
            detoured: None,
            reach,
            height,
            partial: false,
            steering: Duration::ZERO,
            last_call: None,
            best: None,
            replan: Replan::Ready,
            searched: None,
            timing: None,
        }
    }

    /// Lets the route step off ledges, for sessions that simulate falls.
    #[must_use]
    pub fn with_drops(mut self, drops: bool) -> Self {
        self.drops = drops;
        self.restart(|search| search.with_drops(drops));
        self
    }

    /// Lets the route pass these closed doors, for a follower that opens each
    /// one as [`RouteStep::OpenDoor`] asks.
    #[must_use]
    pub fn with_doors(mut self, doors: Vec<u8>) -> Self {
        self.restart(|search| search.with_doors(doors.clone()));
        self.doors = doors;
        self
    }

    /// Runs the route's searches through `background`, each on a snapshot of
    /// the world as it stood when the search began, so a long one never holds
    /// up [`Route::next`]; the route reports [`RouteStep::Searching`] until
    /// the result arrives.
    #[must_use]
    pub fn with_background(mut self, background: Arc<dyn Background>) -> Self {
        self.background = Some(background);
        self
    }

    /// Reconfigures a search that has not started yet.
    fn restart(&mut self, change: impl FnOnce(PathSearch) -> PathSearch) {
        self.search = self.search.take().map(|search| match search {
            Search {
                job: Job::Inline(inner),
                replaces,
            } => Search {
                job: Job::Inline(Box::new(change(*inner))),
                replaces,
            },
            running => running,
        });
    }

    /// A door the follower could not open: it is a wall from now on, and the
    /// route searches again once from `feet`, sharing the stall's one replan;
    /// with that already spent, the route reports [`RouteStep::Stalled`].
    pub fn refused(&mut self, door: u8, feet: Vec3, goal: Vec3) {
        self.doors.retain(|open| *open != door);
        if self.spend_replan() {
            self.replan(feet, goal);
        } else {
            self.replan = Replan::Stalled;
        }
    }

    /// Takes the one fresh search; false when it was already spent.
    fn spend_replan(&mut self) -> bool {
        let ready = self.replan == Replan::Ready;
        if ready {
            self.replan = Replan::Spent;
        }
        ready
    }

    /// A door the follower opened stands in the way as drawn: searches from
    /// `feet`, in the world as it stands with the door still passable, for a
    /// short way round to a waypoint beyond it, keeping the rest of the route,
    /// as a player steps round a door that swung toward them. Outside the
    /// stall's one replan. If no short way round exists the route is left as
    /// it was, so the same door is asked for again.
    pub fn reroute(&mut self, feet: Vec3, goal: Vec3) {
        let Some(last) = self.waypoints.len().checked_sub(1) else {
            self.replan(feet, goal);
            return;
        };
        let rejoin = self
            .waypoints
            .iter()
            .position(|waypoint| flat(*waypoint - feet) >= DETOUR_REJOIN)
            .unwrap_or(last);
        let search = PathSearch::new(feet, self.waypoints[rejoin], FINAL_REACH, self.height)
            .with_drops(self.drops)
            .with_doors(self.doors.clone())
            .with_max_expansions(DETOUR_EXPANSIONS);
        self.search = Some(Search::new(search, Some(rejoin + 1)));
        self.best = None;
    }

    fn replan(&mut self, feet: Vec3, goal: Vec3) {
        let search = PathSearch::new(feet, goal, self.reach, self.height)
            .with_weight(SEARCH_WEIGHT)
            .with_drops(self.drops)
            .with_doors(self.doors.clone());
        self.search = Some(Search::new(search, None));
        self.waypoints.clear();
        self.crossings.clear();
        self.unreachable = false;
        self.partial = false;
        self.best = None;
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

    /// The last finished search's expansions and whether positions remained
    /// unexpanded, for diagnostics; None until a search finishes.
    #[must_use]
    pub const fn searched(&self) -> Option<(usize, bool)> {
        self.searched
    }

    /// Where the last finished search handed off spent its time; None for a
    /// search run in [`Route::next`].
    #[must_use]
    pub const fn search_timing(&self) -> Option<SearchTiming> {
        self.timing
    }

    /// How near the goal the route counts as arrived.
    #[must_use]
    pub const fn reach(&self) -> f32 {
        self.reach
    }

    /// The collision height the route was searched for.
    #[must_use]
    pub const fn height(&self) -> f32 {
        self.height
    }

    /// Whether the last finished search was a step round an opened door, and
    /// whether it found a way; None when it searched the whole way.
    #[must_use]
    pub const fn detoured(&self) -> Option<bool> {
        self.detoured
    }

    /// Where the waypoints still ahead end.
    #[must_use]
    pub fn end(&self) -> Option<Vec3> {
        self.waypoints.back().copied()
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
        if self.replan == Replan::Stalled {
            return RouteStep::Stalled;
        }
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
                } else if self.spend_replan() {
                    self.replan(feet, goal);
                    RouteStep::Searching
                } else {
                    RouteStep::Stalled
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
        if super::path::within_reach(feet, goal, self.reach) {
            return RouteStep::Arrived;
        }
        if let Some(search) = self.search.take() {
            let background = self.background.as_deref();
            let (finished, replaces) = match search.poll(world, feet, budget, background) {
                Ok(done) => done,
                Err(search) => {
                    self.search = Some(search);
                    return RouteStep::Searching;
                }
            };
            self.searched = Some(finished.searched);
            self.timing = finished.timing;
            match (replaces, finished.legs) {
                (None, None) => {
                    self.detoured = None;
                    self.unreachable = true;
                }
                (None, Some(legs)) => {
                    self.detoured = None;
                    self.partial = !finished.found;
                    (self.waypoints, self.crossings) = legs.into_iter().unzip();
                }
                (Some(spliced), legs) => {
                    self.detoured = Some(finished.found);
                    // No short way round: the route stays, and asks for the door again.
                    if let Some(legs) = legs {
                        self.waypoints.drain(..spliced.min(self.waypoints.len()));
                        self.crossings.drain(..spliced.min(self.crossings.len()));
                        for (waypoint, door) in legs.into_iter().rev() {
                            self.waypoints.push_front(waypoint);
                            self.crossings.push_front(door);
                        }
                    }
                }
            }
        }
        if self.unreachable {
            return RouteStep::Unreachable;
        }
        while let Some(&waypoint) = self.waypoints.front() {
            let near = |reach: f32| {
                flat(waypoint - feet) <= reach && (waypoint.y - feet.y).abs() <= MAX_GROUNDED_STEP
            };
            // A waypoint behind a wall, or a level up a stair beside the feet, can
            // be close without being reachable; cutting to the next one is only
            // safe where the search's own edge test can walk straight there.
            let shortcut = || {
                self.waypoints.get(1).is_some_and(|&next| {
                    path::walk(world, feet, Vec2::new(next.x, next.z), self.height)
                        .is_some_and(|end| (end.y - next.y).abs() <= MAX_GROUNDED_STEP)
                })
            };
            if !(near(FINAL_REACH) || near(WAYPOINT_REACH) && shortcut()) {
                break;
            }
            self.waypoints.pop_front();
            self.crossings.pop_front();
        }
        let toward = match (self.waypoints.front(), self.crossings.front()) {
            // A door still shut across the next leg is opened before going on.
            (Some(next), Some(Some(door)))
                if path::walk(world, feet, Vec2::new(next.x, next.z), self.height)
                    .is_none_or(|end| (end.y - next.y).abs() > MAX_GROUNDED_STEP) =>
            {
                return RouteStep::OpenDoor(*door);
            }
            (Some(next), _) => *next,
            // A full route's last waypoint lies within reach of the goal, so the
            // rest of the way is a short straight walk.
            (None, _) if !self.partial => goal,
            (None, _) => return RouteStep::Arrived,
        };
        match self.door_ahead(world, feet, toward) {
            Some(door) => RouteStep::OpenDoor(door),
            None => RouteStep::Toward(toward),
        }
    }

    /// A listed door shut across the next few units of the way, which the
    /// search could not see: one too far away to be loaded when the route was
    /// planned, or one standing open then. Clicked where it blocks, so the
    /// walker never pushes against it until the route stalls.
    fn door_ahead(&self, world: &CollisionWorld, feet: Vec3, toward: Vec3) -> Option<u8> {
        if self.doors.is_empty() {
            return None;
        }
        let along =
            Vec3::new(toward.x - feet.x, 0.0, toward.z - feet.z).clamp_length_max(DOOR_PROBE);
        let ahead = feet + along;
        if path::walk(world, feet, Vec2::new(ahead.x, ahead.z), self.height).is_some() {
            return None;
        }
        world.door_between(&self.doors, feet, ahead, DOOR_MARGIN)
    }
}

/// Distance ignoring height.
fn flat(v: Vec3) -> f32 {
    Vec2::new(v.x, v.z).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// Walks straight to each waypoint the route names, as a follower would.
    fn follow(route: &mut Route, world: &CollisionWorld, mut feet: Vec3, goal: Vec3) -> Vec3 {
        for tick in 0..500_u64 {
            match route.next(world, feet, goal, 256, Duration::from_millis(tick * 150)) {
                RouteStep::Searching => (),
                RouteStep::Toward(next) => {
                    // One online sample, stepped in slices as the live stepper does.
                    let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z).clamp_length_max(3.0);
                    for _ in 0..3 {
                        feet = world.step(feet, to / 3.0, 6.0);
                    }
                }
                RouteStep::Arrived => return feet,
                end @ (RouteStep::Unreachable | RouteStep::Stalled | RouteStep::OpenDoor(_)) => {
                    panic!("{end:?} at {feet:?}")
                }
            }
        }
        panic!("route did not finish");
    }

    /// A floor split by a wall across z = 0, with a doorway at 10 < x < 14 shut
    /// by door 7, and an open gap at 16 < x < 20 when `gap` is set.
    fn doorway(gap: bool) -> (CollisionWorld, Arc<crate::movement::CollisionMesh>) {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        let mut wall = |from: f32, to: f32| {
            triangles.extend(quad(
                [from, 0.0, 0.0],
                [to, 0.0, 0.0],
                [to, 10.0, 0.0],
                [from, 10.0, 0.0],
            ));
        };
        wall(-30.0, 10.0);
        if gap {
            wall(14.0, 16.0);
            wall(20.0, 30.0);
        } else {
            wall(14.0, 30.0);
        }
        let door = Arc::new(
            crate::movement::CollisionMesh::new(quad(
                [10.0, 0.0, 0.0],
                [14.0, 0.0, 0.0],
                [14.0, 10.0, 0.0],
                [10.0, 10.0, 0.0],
            ))
            .unwrap(),
        );
        let mut world = CollisionWorld::new(triangles).unwrap();
        world.set_door_obstacles(vec![(7, Arc::clone(&door))]);
        (world, door)
    }

    /// Follows until the route asks for a door or ends.
    fn follow_to_door(
        route: &mut Route,
        world: &CollisionWorld,
        mut feet: Vec3,
        goal: Vec3,
        from: u64,
    ) -> (RouteStep, Vec3) {
        for tick in from..from + 500 {
            match route.next(world, feet, goal, 256, Duration::from_millis(tick * 150)) {
                RouteStep::Searching => (),
                RouteStep::Toward(next) => {
                    let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z).clamp_length_max(3.0);
                    for _ in 0..3 {
                        feet = world.step(feet, to / 3.0, 6.0);
                    }
                }
                end => return (end, feet),
            }
        }
        panic!("route did not finish");
    }

    /// Runs each search on a thread of its own.
    struct Threads;

    impl Background for Threads {
        fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
            std::thread::spawn(job);
        }
    }

    /// Keeps each search it is handed until the test runs it.
    #[derive(Default)]
    struct Held(std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>>);

    impl Background for Held {
        fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
            self.0.lock().unwrap().push(job);
        }
    }

    impl Held {
        fn run(&self) -> usize {
            let jobs = std::mem::take(&mut *self.0.lock().unwrap());
            let count = jobs.len();
            for job in jobs {
                job();
            }
            count
        }
    }

    #[test]
    fn a_route_searched_off_the_caller_follows_the_same_waypoints_and_doors() {
        let (world, _) = doorway(true);
        let (start, goal) = (Vec3::new(-20.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        // However busy the machine, a thread of its own finishes this search.
        let first_step = |route: &mut Route| {
            let deadline = Instant::now() + Duration::from_secs(60);
            for tick in 0_u64.. {
                let now = Duration::from_millis(tick);
                match route.next(&world, start, goal, 256, now) {
                    RouteStep::Searching if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    RouteStep::Searching => panic!("the search never finished"),
                    step => return step,
                }
            }
            unreachable!("the clock ran out first");
        };
        let mut inline = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let mut handed = Route::new(start, goal, 1.0, 6.0)
            .with_doors(vec![7])
            .with_background(Arc::new(Threads));
        assert_eq!(first_step(&mut handed), first_step(&mut inline));
        assert_eq!(handed.waypoints, inline.waypoints);
        assert_eq!(handed.crossings, inline.crossings);
        assert_eq!(handed.searched(), inline.searched());
        // Only the handed-off search reports where its time went.
        assert_eq!(inline.search_timing(), None);
        assert_eq!(
            handed.search_timing().map(|timing| timing.obstacles),
            Some(1)
        );
        assert!(inline.waypoints.len() > 1, "{:?}", inline.waypoints);
    }

    #[test]
    fn a_handed_off_search_holds_the_route_at_searching_until_its_result_arrives() {
        let world = open_field();
        let (start, goal) = (Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0));
        let held = Arc::new(Held::default());
        let mut route = Route::new(start, goal, 2.0, 6.0).with_background(held.clone());
        // Every call while the search is away returns at once, whatever the budget.
        for tick in 0..20 {
            let now = Duration::from_millis(tick * 16);
            assert_eq!(
                route.next(&world, start, goal, 0, now),
                RouteStep::Searching
            );
        }
        assert_eq!(held.run(), 1);
        assert!(matches!(
            route.next(&world, start, goal, 0, Duration::from_secs(1)),
            RouteStep::Toward(_)
        ));
        assert!(!route.is_searching());

        // A search whose route is gone is told to stop, and runs safely without it.
        let mut route = Route::new(start, goal, 2.0, 6.0).with_background(held.clone());
        assert_eq!(
            route.next(&world, start, goal, 0, Duration::ZERO),
            RouteStep::Searching
        );
        drop(route);
        assert_eq!(held.run(), 1);
    }

    #[test]
    fn a_route_opens_a_closed_door_from_beside_it_and_walks_through_once_it_is_open() {
        let (mut world, _) = doorway(false);
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::OpenDoor(7));
        // Asked from beside the door, not from across the room.
        assert!(feet.z > -3.0 && feet.z < 0.0, "{feet:?}");
        // Waiting at the door is not a stall, however long the door takes.
        for tick in 0..100 {
            let now = Duration::from_millis(100_000 + tick * 150);
            assert_eq!(
                route.next(&world, feet, goal, 256, now),
                RouteStep::OpenDoor(7)
            );
        }
        // The door swings open: its collider leaves the doorway.
        world.set_door_obstacles(Vec::new());
        let (step, feet) = follow_to_door(&mut route, &world, feet, goal, 1000);
        assert_eq!(step, RouteStep::Arrived);
        assert!(feet.z > 8.0, "{feet:?}");
    }

    #[test]
    fn a_far_door_unseen_when_planned_and_shut_on_arrival_is_opened_without_a_stall() {
        let (mut world, door) = doorway(false);
        world.set_door_obstacles(Vec::new());
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let mut feet = start;
        // Without a collider when planned: open, or too far away to be loaded.
        let mut shut = None;
        let mut asked = None;
        for tick in 0..500_u64 {
            // Shut as the walker nears the doorway.
            if shut.is_none() && feet.z > -6.0 {
                world.set_door_obstacles(vec![(7, Arc::clone(&door))]);
                shut = Some(tick);
            }
            match route.next(&world, feet, goal, 256, Duration::from_millis(tick * 150)) {
                RouteStep::Searching => (),
                RouteStep::Toward(next) => {
                    let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z).clamp_length_max(3.0);
                    for _ in 0..3 {
                        feet = world.step(feet, to / 3.0, 6.0);
                    }
                }
                RouteStep::OpenDoor(id) => {
                    asked = Some((id, tick));
                    break;
                }
                end => panic!("{end:?} at {feet:?}"),
            }
        }
        let (id, tick) = asked.expect("the shut door is asked for");
        assert_eq!(id, 7);
        assert!(feet.z < 0.0, "{feet:?}");
        // Asked for as the walker meets it, well before the 3 s stall (20 ticks).
        let shut = shut.unwrap();
        assert!(tick - shut < 8, "asked {} ticks after it shut", tick - shut);
        // The walker opens it and goes on through.
        world.set_door_obstacles(Vec::new());
        let (step, feet) = follow_to_door(&mut route, &world, feet, goal, tick + 1);
        assert_eq!(step, RouteStep::Arrived);
        assert!(feet.z > 8.0, "{feet:?}");
    }

    /// A floor split by a wall across z = 0 with a wide doorway, 6 < x < 14,
    /// shut by door 7; and that door standing open with its leaf swung toward
    /// the walker, across `leaf` (the x span it covers) at z = -1.5.
    fn wide_doorway(leaf: (f32, f32)) -> (CollisionWorld, Vec<[[f32; 3]; 3]>) {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        for (from, to) in [(-30.0, 6.0), (14.0, 30.0)] {
            triangles.extend(quad(
                [from, 0.0, 0.0],
                [to, 0.0, 0.0],
                [to, 10.0, 0.0],
                [from, 10.0, 0.0],
            ));
        }
        let shut = crate::movement::CollisionMesh::new(quad(
            [6.0, 0.0, 0.0],
            [14.0, 0.0, 0.0],
            [14.0, 10.0, 0.0],
            [6.0, 10.0, 0.0],
        ))
        .unwrap();
        let mut world = CollisionWorld::new(triangles).unwrap();
        world.set_door_obstacles(vec![(7, Arc::new(shut))]);
        let open = quad(
            [leaf.0, 0.0, -1.5],
            [leaf.1, 0.0, -1.5],
            [leaf.1, 10.0, -1.5],
            [leaf.0, 10.0, -1.5],
        )
        .to_vec();
        (world, open)
    }

    #[test]
    fn a_door_that_swings_toward_the_walker_is_passed_around_its_open_leaf() {
        let (mut world, leaf) = wide_doorway((10.0, 14.0));
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::OpenDoor(7));
        // Open: the doorway is clear but the leaf now stands in the straight way.
        let leaf = crate::movement::CollisionMesh::new(leaf).unwrap();
        world.set_door_obstacles(vec![(7, Arc::new(leaf))]);
        assert_eq!(
            route.next(&world, feet, goal, 256, Duration::from_secs(100)),
            RouteStep::OpenDoor(7)
        );
        route.reroute(feet, goal);
        let (step, feet) = follow_to_door(&mut route, &world, feet, goal, 1000);
        assert_eq!(step, RouteStep::Arrived);
        assert!(feet.z > 8.0, "{feet:?}");
        // Only a short way round was searched, not the whole route again.
        let (expanded, _) = route.searched().unwrap();
        assert!(expanded < 500, "{expanded}");
    }

    #[test]
    fn a_detour_keeps_a_later_door_on_the_kept_route_aligned_with_its_waypoint() {
        let (_, leaf) = wide_doorway((10.0, 14.0));
        // A second wall at z = 10, its doorway, 10 < x < 14, shut by door 8.
        let second = |z: f32| {
            crate::movement::CollisionMesh::new(quad(
                [10.0, 0.0, z],
                [14.0, 0.0, z],
                [14.0, 10.0, z],
                [10.0, 10.0, z],
            ))
            .unwrap()
        };
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        for (from, to) in [(-30.0, 6.0), (14.0, 30.0)] {
            triangles.extend(quad(
                [from, 0.0, 0.0],
                [to, 0.0, 0.0],
                [to, 10.0, 0.0],
                [from, 10.0, 0.0],
            ));
        }
        for (from, to) in [(-30.0, 10.0), (14.0, 30.0)] {
            triangles.extend(quad(
                [from, 0.0, 10.0],
                [to, 0.0, 10.0],
                [to, 10.0, 10.0],
                [from, 10.0, 10.0],
            ));
        }
        let mut world = CollisionWorld::new(triangles).unwrap();
        let shut = crate::movement::CollisionMesh::new(quad(
            [6.0, 0.0, 0.0],
            [14.0, 0.0, 0.0],
            [14.0, 10.0, 0.0],
            [6.0, 10.0, 0.0],
        ))
        .unwrap();
        let door8 = Arc::new(second(10.0));
        world.set_door_obstacles(vec![(7, Arc::new(shut)), (8, Arc::clone(&door8))]);
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 20.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7, 8]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::OpenDoor(7));
        let leaf = Arc::new(crate::movement::CollisionMesh::new(leaf).unwrap());
        world.set_door_obstacles(vec![(7, leaf), (8, door8)]);
        route.reroute(feet, goal);
        for tick in 0..50 {
            if route.next(
                &world,
                feet,
                goal,
                256,
                Duration::from_millis(200_000 + tick),
            ) != RouteStep::Searching
            {
                break;
            }
        }
        assert_eq!(route.detoured(), Some(true));
        // Door 8's crossing is still on the waypoint just past its wall.
        let crossing = route
            .crossings
            .iter()
            .position(|door| *door == Some(8))
            .unwrap();
        assert!(route.waypoints[crossing].z >= 10.0, "{:?}", route.waypoints);
        assert!(
            route.waypoints[crossing - 1].z < 10.0,
            "{:?}",
            route.waypoints
        );
        let (step, _) = follow_to_door(&mut route, &world, feet, goal, 1000);
        assert_eq!(step, RouteStep::OpenDoor(8));
    }

    #[test]
    fn a_door_whose_open_leaf_still_blocks_the_doorway_is_asked_for_again() {
        let (mut world, leaf) = wide_doorway((5.0, 15.0));
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::OpenDoor(7));
        let leaf = crate::movement::CollisionMesh::new(leaf).unwrap();
        world.set_door_obstacles(vec![(7, Arc::new(leaf))]);
        route.reroute(feet, goal);
        // The search around it still has to pass the same door, so the walk
        // refuses it ("it opened but still blocks the way").
        let (step, _) = follow_to_door(&mut route, &world, feet, goal, 1000);
        assert_eq!(step, RouteStep::OpenDoor(7));
    }

    #[test]
    fn a_door_the_route_may_not_open_is_a_wall() {
        let (world, _) = doorway(false);
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::Arrived);
        assert!(route.partial());
        assert!(feet.z < 0.0, "{feet:?}");
    }

    #[test]
    fn a_door_that_does_not_open_turns_the_route_back_once() {
        let (world, _) = doorway(false);
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::OpenDoor(7));
        route.refused(7, feet, goal);
        // Searched again without the door: only a partial route is left.
        let (step, _) = follow_to_door(&mut route, &world, feet, goal, 1000);
        assert_eq!(step, RouteStep::Arrived);
        assert!(route.partial());
        // A second refusal has no replan left.
        route.refused(7, feet, goal);
        assert_eq!(
            route.next(&world, feet, goal, 256, Duration::from_mins(15)),
            RouteStep::Stalled
        );
    }

    #[test]
    fn a_short_way_round_beats_opening_a_door() {
        let (world, _) = doorway(true);
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut route = Route::new(start, goal, 1.0, 6.0).with_doors(vec![7]);
        let (step, feet) = follow_to_door(&mut route, &world, start, goal, 0);
        assert_eq!(step, RouteStep::Arrived);
        assert!(!route.partial());
        assert!(feet.z > 8.0, "{feet:?}");
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

    /// A lower floor at y = 0 for z < 0 and a raised floor at y = 4 for z > 0,
    /// split by a ledge too tall to step up, with an optional ramp between them
    /// for x > 20.
    fn ledge(ramp: bool) -> CollisionWorld {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, -10.0],
            [-30.0, 0.0, -10.0],
        )
        .to_vec();
        triangles.extend(quad(
            [-30.0, 0.0, -10.0],
            [20.0, 0.0, -10.0],
            [20.0, 0.0, 0.0],
            [-30.0, 0.0, 0.0],
        ));
        triangles.extend(quad(
            [-30.0, 4.0, 0.0],
            [30.0, 4.0, 0.0],
            [30.0, 4.0, 30.0],
            [-30.0, 4.0, 30.0],
        ));
        triangles.extend(quad(
            [-30.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 4.0, 0.0],
            [-30.0, 4.0, 0.0],
        ));
        if ramp {
            // Rises 4 over 10, well within a walkable slope.
            triangles.extend(quad(
                [20.0, 0.0, -10.0],
                [30.0, 0.0, -10.0],
                [30.0, 4.0, 0.0],
                [20.0, 4.0, 0.0],
            ));
            // The ramp's side above the lower floor.
            triangles.push([[20.0, 0.0, -10.0], [20.0, 0.0, 0.0], [20.0, 4.0, 0.0]]);
        } else {
            triangles.extend(quad(
                [20.0, 0.0, -10.0],
                [30.0, 0.0, -10.0],
                [30.0, 0.0, 0.0],
                [20.0, 0.0, 0.0],
            ));
            triangles.extend(quad(
                [20.0, 0.0, 0.0],
                [30.0, 0.0, 0.0],
                [30.0, 4.0, 0.0],
                [20.0, 4.0, 0.0],
            ));
        }
        CollisionWorld::new(triangles).unwrap()
    }

    #[test]
    fn a_follower_climbs_to_a_raised_floor_by_its_ramp_or_stops_at_the_ledge() {
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 4.0, 15.0));
        let world = ledge(true);
        let mut route = Route::new(start, goal, 3.0, 6.0);
        let end = follow(&mut route, &world, start, goal);
        assert!(!route.partial());
        assert!(
            Vec2::new(end.x - goal.x, end.z - goal.z).length() <= 3.0 && end.y > 3.9,
            "{end:?}"
        );

        // Without the ramp the ledge is too tall to step up, so the route ends
        // as close as it can get on the lower floor.
        let world = ledge(false);
        let mut route = Route::new(start, goal, 3.0, 6.0);
        let end = follow(&mut route, &world, start, goal);
        assert!(route.partial());
        assert!(
            end.y.abs() < 0.01 && end.z < 0.0 && end.z > -2.0 - FINAL_REACH - 0.1,
            "{end:?}"
        );
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
    fn a_goal_on_another_floor_is_not_reached_from_below_it() {
        let world = open_field();
        let goal = Vec3::Y * 20.0;
        let mut route = Route::new(Vec3::X, goal, 2.0, 6.0);
        let step = route.next(&world, Vec3::X, goal, 4096, Duration::ZERO);
        assert_ne!(step, RouteStep::Arrived);
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

    /// A box from `low` to `high`, closed on every side.
    fn solid(low: [f32; 3], high: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let [x0, y0, z0] = low;
        let [x1, y1, z1] = high;
        [
            quad([x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]),
            quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]),
            quad([x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]),
            quad([x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]),
            quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    #[test]
    fn a_follower_climbs_the_stairs_the_route_takes_instead_of_walking_beside_them() {
        let mut triangles = quad(
            [-30.0, 0.0, -30.0],
            [30.0, 0.0, -30.0],
            [30.0, 0.0, 30.0],
            [-30.0, 0.0, 30.0],
        )
        .to_vec();
        // Stairs rising 1.5 per unit along +x to a platform 6 high.
        for step in 0..4_u8 {
            let x = f32::from(step);
            triangles.extend(solid([x, 0.0, 0.0], [x + 1.0, 1.5 * (x + 1.0), 3.0]));
        }
        triangles.extend(solid([4.0, 0.0, 0.0], [14.0, 6.0, 3.0]));
        // A side wall: the stairs can only be climbed from their foot.
        triangles.extend(solid([0.0, 0.0, -0.2], [14.0, 8.0, 0.0]));
        let world = CollisionWorld::new(triangles).unwrap();
        // Approaching from beside the stairs, where the first step's waypoint is
        // within flat reach long before the feet are on it.
        let (start, goal) = (Vec3::new(-1.0, 0.0, -12.0), Vec3::new(12.0, 6.0, 1.5));
        let mut route = Route::new(start, goal, 2.0, 6.0);
        let end = follow(&mut route, &world, start, goal);
        assert!((end.y - 6.0).abs() < 0.01, "{end:?}");
        assert!(
            Vec2::new(end.x - goal.x, end.z - goal.z).length() <= 2.0,
            "{end:?}"
        );
    }
}

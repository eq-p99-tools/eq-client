//! Routes planned over the mesh: [`MeshNavigator`]. The mesh proposes the
//! legs; every leg is then walked by the walker's own edge tests in the world
//! as it stands, and a leg they refuse is repaired by a short grid search, or
//! the whole way handed to the grid.

use super::NavMesh;
use super::route::{Areas, Corridor, Crossing, PolygonSearch, entry, funnel};
use crate::movement::BODY_REACH;
use crate::movement::navigator::LOCAL_EXPANSIONS;
use crate::movement::path::{CELL, CLEARANCE, DOOR_REACH, DOOR_TOLL, clear_leg, drop, walk};
use crate::movement::{
    CollisionWorld, Fallback, GridNavigator, MAX_GROUNDED_STEP, Navigator, PathProgress,
    PathSearch, Planned, RouteSearch, Scope, SearchRequest,
};
use glam::{Vec2, Vec3};
use std::sync::Arc;

/// How the polygon search weighs the distance still to go: not at all beyond
/// it, so the search finds the cheapest corridor. The grid weighs it twice
/// ([`crate::movement::navigator`]) to keep its many cells' search short; a
/// zone's mesh has few enough polygons to search them in full, and a weighted
/// search would go through a tolled door rather than a slightly longer way
/// round.
const MESH_WEIGHT: f32 = 1.0;

/// How near a repaired leg's end its grid search must come: within a grid
/// cell's half-diagonal, so it ends on the cell nearest the leg's end.
const REPAIR_REACH: f32 = CELL * std::f32::consts::FRAC_1_SQRT_2 + 0.1;

/// Plans searches of the whole way over a zone's mesh; a short way round
/// stays with the grid, whose expansion cap keeps it short.
#[derive(Clone, Debug)]
pub struct MeshNavigator {
    mesh: Arc<NavMesh>,
}

impl MeshNavigator {
    /// Plans over `mesh`, built for the walker's own height.
    #[must_use]
    pub const fn new(mesh: Arc<NavMesh>) -> Self {
        Self { mesh }
    }

    #[cfg(test)]
    pub(super) fn mesh(&self) -> &NavMesh {
        &self.mesh
    }
}

impl Navigator for MeshNavigator {
    fn search(&self, request: SearchRequest) -> Box<dyn RouteSearch> {
        match request.scope {
            Scope::Local => GridNavigator.search(request),
            Scope::Whole => Box::new(MeshSearch {
                mesh: Arc::clone(&self.mesh),
                request,
                stage: Stage::Start,
                walked: Walked::default(),
                legs: 0,
                clear: 0,
                drops: 0,
                repaired: Vec::new(),
                fallback: None,
                expanded: 0,
            }),
        }
    }
}

/// A search of the whole way over the mesh.
struct MeshSearch {
    mesh: Arc<NavMesh>,
    request: SearchRequest,
    stage: Stage,
    /// The legs that stood so far.
    walked: Walked,
    legs: usize,
    clear: usize,
    /// Drops off ledges taken.
    drops: usize,
    /// Each repaired leg's ends, as the mesh gave them.
    repaired: Vec<(Vec3, Vec3)>,
    fallback: Option<Fallback>,
    /// Polygons, leg checks and repairs, in grid expansions' worth.
    expanded: usize,
}

enum Stage {
    Start,
    Polygons(PolygonSearch),
    Legs {
        legs: Vec<Leg>,
        next: usize,
        repair: Option<Box<PathSearch>>,
    },
    Grid(Box<dyn RouteSearch>),
    Done(bool),
}

/// A leg the mesh gives: a straight walk to `to`, or with `drop`, a step off
/// a ledge that lands at `to`.
#[derive(Clone, Copy, Debug)]
struct Leg {
    to: Vec3,
    drop: bool,
}

/// The legs through `corridor` from `start` to `goal`: each stretch between
/// drops funnelled on its own, its last leg ending at the drop's top, then
/// the drop.
fn legs(start: Vec3, goal: Vec3, corridor: &[Crossing]) -> Vec<Leg> {
    let stretch = |from: Vec3, to: Vec3, portals: &[_]| {
        funnel(from, to, portals, CLEARANCE)
            .into_iter()
            .map(|to| Leg { to, drop: false })
    };
    let mut legs = Vec::new();
    let mut from = start;
    let mut portals = Vec::new();
    for crossing in corridor {
        match *crossing {
            Crossing::Edge(portal) => portals.push(portal),
            Crossing::Drop { top, landing } => {
                legs.extend(stretch(from, top, &portals));
                legs.push(Leg {
                    to: landing,
                    drop: true,
                });
                portals.clear();
                from = landing;
            }
        }
    }
    legs.extend(stretch(from, goal, &portals));
    legs
}

/// The positions walked so far, each with the closed door the step to it
/// passes.
#[derive(Default)]
struct Walked {
    path: Vec<Vec3>,
    doors: Vec<Option<u8>>,
}

impl Walked {
    fn feet(&self, start: Vec3) -> Vec3 {
        self.path.last().copied().unwrap_or(start)
    }

    fn push(&mut self, feet: Vec3, door: Option<u8>) {
        self.path.push(feet);
        self.doors.push(door);
    }
}

impl MeshSearch {
    fn fall_back(&mut self, why: Fallback) -> Stage {
        self.fallback = Some(why);
        self.walked = Walked::default();
        Stage::Grid(GridNavigator.search(self.request.clone()))
    }

    /// The doors' boxes: a listed door's widened by the walker's reach and
    /// costing a toll to pass, one that can't be opened closed.
    fn areas(&self, world: &CollisionWorld) -> Areas {
        let mut areas = Areas {
            toll: DOOR_TOLL,
            below: MAX_GROUNDED_STEP,
            ..Areas::default()
        };
        for (door, low, high) in world.door_bounds() {
            if self.request.doors.contains(&door) {
                let widen = Vec3::new(BODY_REACH, 0.0, BODY_REACH);
                areas.tolled.push((low - widen, high + widen));
            } else {
                areas.closed.push((low, high));
            }
        }
        areas
    }

    fn start(&mut self, world: &CollisionWorld) -> Stage {
        let SearchRequest { start, goal, .. } = self.request;
        let (Some((from, _)), Some((to, _))) = (self.mesh.locate(start), self.mesh.locate(goal))
        else {
            return self.fall_back(Fallback::OffMesh);
        };
        if !self.mesh.reaches(from, to, self.request.drops) {
            return self.fall_back(Fallback::OtherIsland);
        }
        Stage::Polygons(PolygonSearch::new(
            (start, from),
            (goal, to),
            MESH_WEIGHT,
            self.areas(world),
            self.request.drops,
        ))
    }

    /// Checks the next leg, from where the last one ended; a leg the edge
    /// tests refuse starts a repair.
    fn check(&mut self, world: &CollisionWorld, leg: Leg) -> Result<(), Box<PathSearch>> {
        let (height, to) = (self.request.height, leg.to);
        let from = self.walked.feet(self.request.start);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A leg's length.
        let cost = (flat(to - from) / CELL).ceil() as usize + 1;
        self.expanded += cost;
        if leg.drop {
            // The walker's own drop from where it stands, landing where the
            // mesh's drop did.
            if let Some(landing) = drop(world, from, Vec2::new(to.x, to.z), height)
                && flat(landing - to) <= REPAIR_REACH
                && (landing.y - to.y).abs() <= MAX_GROUNDED_STEP
            {
                self.drops += 1;
                self.walked.push(landing, None);
                return Ok(());
            }
            return Err(self.repair(from, to));
        }
        let reach = |world: &CollisionWorld| {
            walk(world, from, Vec2::new(to.x, to.z), height)
                .filter(|end| (end.y - to.y).abs() <= MAX_GROUNDED_STEP)
        };
        if let Some(end) = reach(world) {
            // The plain walk is the bar, as for the grid's own hops; legs
            // that also keep the clearance are counted, for the looks.
            if clear_leg(world, from, to, height) {
                self.clear += 1;
            }
            self.walked.push(end, None);
            return Ok(());
        }
        if let Some(door) = world.door_between(&self.request.doors, from, to, DOOR_REACH)
            && self.through_door(world, door, to)
        {
            return Ok(());
        }
        Err(self.repair(from, to))
    }

    /// A short grid search from `from` to `to`, with the route's doors and
    /// drops, to stand for a leg the edge tests refused.
    fn repair(&self, from: Vec3, to: Vec3) -> Box<PathSearch> {
        // The last leg ends anywhere within the route's reach, as the grid's
        // search does.
        let last = to == self.request.goal;
        let reach = if last {
            self.request.reach
        } else {
            REPAIR_REACH
        };
        Box::new(
            PathSearch::new(from, to, reach, self.request.height)
                .with_drops(self.request.drops)
                .with_doors(self.request.doors.clone())
                .with_max_expansions(LOCAL_EXPANSIONS),
        )
    }

    /// Walks the leg to `to` through `door`, as the grid keeps a door's step:
    /// a position a cell short of the door, then the step through it, so the
    /// door is asked for from there. A leg that starts nearer the door than
    /// that moves its start back along its own line. False when the walk
    /// fails even with the door open.
    fn through_door(&mut self, world: &CollisionWorld, door: u8, to: Vec3) -> bool {
        let height = self.request.height;
        let from = self.walked.feet(self.request.start);
        let Some((_, low, high)) = world.door_bounds().into_iter().find(|(id, ..)| *id == door)
        else {
            return false;
        };
        let way = Vec3::new(to.x - from.x, 0.0, to.z - from.z).normalize_or_zero();
        let met = entry(from, to, low, high).unwrap_or(0.0) * flat(to - from);
        let short = from + way * (met - CELL);
        // Where the step through the door starts: the leg's own start, or a
        // cell short of the door, reached from the position before.
        let before = self
            .walked
            .path
            .len()
            .checked_sub(2)
            .map_or(self.request.start, |i| self.walked.path[i]);
        // A start that is itself a door's step stays: moving it back would
        // undo that door's step.
        let movable = self.walked.doors.last().is_some_and(Option::is_none);
        let stop = if met > CELL {
            walk(world, from, Vec2::new(short.x, short.z), height).map(|stop| (stop, false))
        } else if movable {
            walk(world, before, Vec2::new(short.x, short.z), height).map(|stop| (stop, true))
        } else {
            None
        };
        let opened = world.without_doors(&[door]);
        let start = stop.map_or(from, |(stop, _)| stop);
        let Some(end) = walk(&opened, start, Vec2::new(to.x, to.z), height)
            .filter(|end| (end.y - to.y).abs() <= MAX_GROUNDED_STEP)
        else {
            return false;
        };
        match stop {
            Some((stop, true)) => {
                if let Some(last) = self.walked.path.last_mut() {
                    *last = stop;
                }
            }
            Some((stop, _)) => self.walked.push(stop, None),
            None => (),
        }
        // A leg that starts in the doorway the last step went through is
        // still that one crossing.
        let again = self.walked.doors.last() == Some(&Some(door));
        self.walked.push(end, (!again).then_some(door));
        true
    }

    /// Advances the leg checks by up to `budget`; the stage that follows.
    fn legs(
        &mut self,
        world: &CollisionWorld,
        budget: usize,
        (legs, mut next, mut repair): (Vec<Leg>, usize, Option<Box<PathSearch>>),
    ) -> Stage {
        let spent = self.expanded + budget;
        while self.expanded < spent {
            if let Some(mut search) = repair.take() {
                let from = self.walked.feet(self.request.start);
                let before = search.expanded();
                let progress = search.advance(world, spent - self.expanded);
                self.expanded += search.expanded() - before;
                match progress {
                    PathProgress::Searching => repair = Some(search),
                    PathProgress::Found(path) => {
                        for (feet, door) in path.into_iter().zip(search.doors_along()) {
                            self.walked.push(feet, door);
                        }
                        self.repaired.push((from, legs[next].to));
                        next += 1;
                    }
                    PathProgress::Failed => {
                        return self.fall_back(Fallback::Leg {
                            from,
                            to: legs[next].to,
                        });
                    }
                }
                continue;
            }
            let Some(&leg) = legs.get(next) else {
                return Stage::Done(true);
            };
            match self.check(world, leg) {
                Ok(()) => next += 1,
                Err(search) => repair = Some(search),
            }
        }
        Stage::Legs { legs, next, repair }
    }
}

fn flat(v: Vec3) -> f32 {
    Vec2::new(v.x, v.z).length()
}

impl RouteSearch for MeshSearch {
    fn advance(&mut self, world: &CollisionWorld, budget: usize) -> PathProgress {
        // A stage that ends inside the call hands on what is left of it.
        let (began, budget) = (self.expanded, budget.max(1));
        loop {
            let left = budget.saturating_sub(self.expanded - began);
            let working = matches!(
                self.stage,
                Stage::Polygons(_) | Stage::Legs { .. } | Stage::Grid(_)
            );
            if working && left == 0 {
                return PathProgress::Searching;
            }
            self.stage = match std::mem::replace(&mut self.stage, Stage::Done(false)) {
                Stage::Start => self.start(world),
                Stage::Polygons(mut search) => {
                    let before = search.expanded();
                    let progress = search.advance(&self.mesh, left);
                    self.expanded += search.expanded() - before;
                    match progress {
                        Corridor::Searching => {
                            self.stage = Stage::Polygons(search);
                            return PathProgress::Searching;
                        }
                        Corridor::Failed => self.fall_back(Fallback::NoCorridor),
                        Corridor::Found(corridor) => {
                            let legs = legs(self.request.start, self.request.goal, &corridor);
                            self.legs = legs.len();
                            Stage::Legs {
                                legs,
                                next: 0,
                                repair: None,
                            }
                        }
                    }
                }
                Stage::Legs { legs, next, repair } => {
                    match self.legs(world, left, (legs, next, repair)) {
                        stage @ Stage::Legs { .. } => {
                            self.stage = stage;
                            return PathProgress::Searching;
                        }
                        stage => stage,
                    }
                }
                Stage::Grid(mut search) => {
                    let progress = search.advance(world, left);
                    self.stage = Stage::Grid(search);
                    return progress;
                }
                Stage::Done(true) => {
                    self.stage = Stage::Done(true);
                    return PathProgress::Found(self.walked.path.clone());
                }
                Stage::Done(false) => {
                    self.stage = Stage::Done(false);
                    return PathProgress::Failed;
                }
            };
        }
    }

    fn closest(&self) -> Option<Vec<Vec3>> {
        match &self.stage {
            Stage::Grid(search) => search.closest(),
            _ => (!self.walked.path.is_empty()).then(|| self.walked.path.clone()),
        }
    }

    fn doors_along(&self) -> Vec<Option<u8>> {
        match &self.stage {
            Stage::Grid(search) => search.doors_along(),
            _ => self.walked.doors.clone(),
        }
    }

    fn searched(&self) -> (usize, bool) {
        match &self.stage {
            Stage::Grid(search) => {
                let (expanded, open) = search.searched();
                (self.expanded + expanded, open)
            }
            _ => (self.expanded, false),
        }
    }

    fn planned(&self) -> Planned {
        Planned::Mesh {
            legs: self.legs,
            clear: self.clear,
            drops: self.drops,
            repaired: self.repaired.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

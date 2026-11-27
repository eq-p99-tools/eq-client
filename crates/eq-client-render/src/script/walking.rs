//! Current-zone destinations and waiting on the existing mesh build.
use super::{PlaceDestination, Script, Step, openable_doors, walk_ends};
use crate::{
    PlayerBody,
    online::OnlineState,
    zone_navmesh::{GridReason, ZoneNavMesh},
};
use bevy::prelude::Vec3;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use eq_client_assets::{regions::ZoneLine, ui::InstalledClient};
use eq_client_core::{
    WorldPosition,
    movement::{
        CollisionWorld, GridNavigator, Navigator, PathProgress, Route, Scope as SearchScope,
        SearchRequest,
        navmesh::{MeshNavigator, NavMesh},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    admission: u64,
    zone: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Generation {
    mesh: Option<u64>,
    exit_zones: Option<u64>,
}

impl Scope {
    fn capture(online: &OnlineState) -> Result<Self, String> {
        let world = online.world();
        let admission = world
            .session_id()
            .filter(|_| world.connected() && !world.is_dead())
            .ok_or("walk needs a current admission")?;
        Ok(Self {
            admission,
            zone: world.zone().to_owned(),
        })
    }
}

pub(super) struct PlaceGoal {
    scope: Scope,
    pub feet: Vec3,
}
impl PlaceGoal {
    pub(super) fn current(&self, online: &OnlineState) -> Option<Vec3> {
        (Scope::capture(online).ok().as_ref() == Some(&self.scope)).then_some(self.feet)
    }
}

pub(super) struct FindingExit {
    cancelled: Arc<AtomicBool>,
    task: Task<Result<Vec3, String>>,
}
impl Drop for FindingExit {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn wait(script: &mut Script, reason: &'static str) {
    if script.walk_waiting != Some(reason) {
        script.progress.event(format!("Walk waiting: {reason}"));
        script.walk_waiting = Some(reason);
    }
}

pub(super) fn scene_ready(script: &mut Script, online: &OnlineState) -> Result<bool, String> {
    let scope = Scope::capture(online)?;
    if script.walk_scope.as_ref().is_some_and(|old| *old != scope) {
        return Err("walk cancelled by a zone or admission change".into());
    }
    script.walk_scope = Some(scope);
    if online.scene_loading() {
        wait(script, "current-zone geometry is loading");
        Ok(false)
    } else {
        Ok(true)
    }
}

/// A queued request is not yet a route. Building waits without injecting
/// Forward; failure stays distinct from Disabled's explicit grid fallback.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one current-zone route preparation transition"
)]
pub(super) fn prepare(
    script: &mut Script,
    step: &Step,
    online: &OnlineState,
    client: InstalledClient,
    body: PlayerBody,
    world: &CollisionWorld,
    meshes: Option<&ZoneNavMesh>,
    falls: bool,
    range: f32,
) -> Result<bool, String> {
    let scope = Scope::capture(online)?;
    if script.walk_scope.as_ref().is_some_and(|old| *old != scope) {
        return Err("walk cancelled by a zone or admission change".into());
    }
    script.walk_scope = Some(scope.clone());
    let generation = Generation {
        mesh: meshes.map(ZoneNavMesh::generation),
        exit_zones: matches!(
            step,
            Step::WalkPlace {
                destination: PlaceDestination::ExitZone(_),
                ..
            }
        )
        .then(|| online.world().zone_line_zones_revision()),
    };
    // With neither a mesh nor an exit map there is nothing to invalidate a
    // route against, as before the exit map joined this key.
    let generation =
        (generation.mesh.is_some() || generation.exit_zones.is_some()).then_some(generation);
    if script.walk_generation != generation {
        script.route = None;
        script.place_lookup = None;
        if matches!(
            step,
            Step::WalkPlace {
                destination: PlaceDestination::ExitReference(_) | PlaceDestination::ExitZone(_),
                ..
            }
        ) {
            script.place = None;
        }
        script.walk_generation = generation;
    }
    if script.route.is_some() {
        return Ok(true);
    }
    let (mesh, grid) = match meshes
        .map(|meshes| meshes.for_zone_walker(&scope.zone, meshes.walker_size(body, client)))
    {
        Some(Ok(mesh)) => (Some(mesh), None),
        Some(Err(GridReason::Building)) => {
            wait(script, "current-zone navigation mesh is building");
            return Ok(false);
        }
        Some(Err(
            reason @ (GridReason::Failed
            | GridReason::Missing
            | GridReason::NoFloor
            | GridReason::TooTall),
        )) => {
            return Err(reason.describe().into());
        }
        Some(Err(reason)) => (None, Some(reason)),
        None => (None, Some(GridReason::Missing)),
    };
    if let Step::WalkPlace { destination, .. } = step
        && script.place.is_none()
    {
        let feet = match *destination {
            PlaceDestination::Coordinates([x, y, z]) => {
                Vec3::from_array(eq_client_core::render_position(WorldPosition {
                    x,
                    y,
                    z,
                    heading: 0.0,
                }))
            }
            exit => {
                if script.place_lookup.is_none() {
                    let player = online.world().player().ok_or("walk needs a player")?;
                    let from = Vec3::from_array(eq_client_core::render_position(player.position))
                        - Vec3::Y * body.feet_offset;
                    let floors = meshes
                        .and_then(|mesh| mesh.floors(&scope.zone))
                        .unwrap_or_else(|| Arc::new(world.terrain_triangles()));
                    let regions = online.regions.clone();
                    let exit_zones = online.world().zone_line_zones().clone();
                    let snapshot = world.clone();
                    let mesh = mesh.clone();
                    let doors = openable_doors(online);
                    let cancelled = Arc::new(AtomicBool::new(false));
                    let cancellation = Arc::clone(&cancelled);
                    let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                        let points = regions.zone_line_floor_points(
                            &floors,
                            body.feet_offset,
                            |line| matches_exit(exit, line, &exit_zones),
                            &|| cancellation.load(Ordering::Relaxed),
                        );
                        choose_exit(
                            points,
                            &snapshot,
                            mesh,
                            from,
                            body.height,
                            range,
                            &doors,
                            falls,
                            &cancellation,
                        )
                    });
                    script.place_lookup = Some(FindingExit { cancelled, task });
                }
                let finding = script.place_lookup.as_mut().expect("started above");
                let Some(result) = block_on(poll_once(&mut finding.task)) else {
                    wait(script, "finding a reachable authored zone line");
                    return Ok(false);
                };
                script.place_lookup = None;
                result?
            }
        };
        script.place = Some(PlaceGoal {
            scope: scope.clone(),
            feet,
        });
    }
    let (feet, goal) = walk_ends(
        online,
        client,
        body,
        script.approached,
        script.place.as_ref(),
        Some(world),
    )
    .ok_or("walk needs its current destination and collision")?;
    let mut route = Route::new(feet, goal, range, body.height)
        .with_drops(falls)
        .with_doors(openable_doors(online))
        .with_background(Arc::new(crate::background::ComputePool));
    if script.place.is_some() {
        route = route.with_exact_height();
    }
    if let Some(mesh) = mesh {
        if mesh.locate(feet).is_none() {
            script.progress.event(
                "Walk start is off the mesh; this is a projection issue, not a door connection"
                    .to_owned(),
            );
        }
        if mesh.locate(goal).is_none() {
            script
                .progress
                .event("Walk goal is off the mesh".to_owned());
        }
        if mesh.incomplete() {
            script.progress.event(
                "Walk uses a partial navigation mesh; each leg still needs collision validation"
                    .to_owned(),
            );
        }
        route = route.with_navigator(Box::new(MeshNavigator::new(mesh)));
    } else if let Some(reason) = grid {
        script
            .progress
            .event(format!("Walk uses grid search: {}", reason.describe()));
    }
    if script.walk_waiting.take().is_some() {
        script
            .progress
            .event("Walk destination and current-zone mesh resolved; planning route".to_owned());
    }
    script.route = Some(route);
    script.door_wait = None;
    script.following = None;
    script.quiet = None;
    Ok(true)
}

fn matches_exit(
    wanted: PlaceDestination,
    line: ZoneLine,
    zones: &std::collections::BTreeMap<u32, u16>,
) -> bool {
    match (wanted, line) {
        (PlaceDestination::ExitReference(wanted), ZoneLine::Reference(actual)) => wanted == actual,
        (PlaceDestination::ExitZone(wanted), ZoneLine::Reference(reference)) => {
            zones.get(&reference) == Some(&wanted)
        }
        (PlaceDestination::ExitZone(wanted), ZoneLine::Absolute { zone_id, .. }) => {
            wanted == zone_id
        }
        _ => false,
    }
}

pub(super) fn end(
    feet: Vec3,
    goal: Vec3,
    range: f32,
    arrived: bool,
    out_of_time: bool,
) -> Result<bool, String> {
    let flat = bevy::prelude::Vec2::new(feet.x - goal.x, feet.z - goal.z).length();
    let right_floor =
        (feet.y - goal.y).abs() <= range.min(eq_client_core::movement::MAX_GROUNDED_STEP);
    if flat <= range && right_floor {
        Ok(true)
    } else if arrived {
        Err("no walkable route reaches the requested place and floor".into())
    } else if out_of_time {
        Err("walk ran out of time before reaching the requested place".into())
    } else {
        Ok(false)
    }
}

/// Candidate selection is background route work, not a second mesh bake.
/// A bounded set is searched rather than guessing that a nearby point behind
/// a wall is reachable. The actual route is re-evaluated from current feet.
#[allow(
    clippy::too_many_arguments,
    reason = "one bounded exit search snapshot"
)]
fn choose_exit(
    points: Vec<[f32; 3]>,
    world: &CollisionWorld,
    mesh: Option<Arc<NavMesh>>,
    from: Vec3,
    height: f32,
    range: f32,
    doors: &[u8],
    falls: bool,
    cancelled: &AtomicBool,
) -> Result<Vec3, String> {
    let opened = world.without_doors(doors);
    let mut points: Vec<_> = points
        .into_iter()
        .map(Vec3::from_array)
        .filter(|point| {
            opened.support(*point).is_some()
                && opened
                    .clearance(*point, height, 0.0)
                    .is_none_or(|distance| distance >= 0.0)
        })
        .collect();
    points.sort_by(|a, b| {
        a.distance_squared(from)
            .total_cmp(&b.distance_squared(from))
    });
    points.dedup_by(|a, b| a.distance_squared(*b) < 0.01);
    let planner: Box<dyn Navigator> = mesh.map_or_else(
        || Box::new(GridNavigator) as Box<dyn Navigator>,
        |mesh| Box::new(MeshNavigator::new(mesh)),
    );
    for goal in points.into_iter().take(64) {
        if cancelled.load(Ordering::Relaxed) {
            return Err("exit lookup cancelled".into());
        }
        let mut search = planner.search(SearchRequest {
            start: from,
            goal,
            reach: range,
            height,
            doors: doors.to_vec(),
            drops: falls,
            scope: SearchScope::Whole,
            goal_height: Some(range.min(eq_client_core::movement::MAX_GROUNDED_STEP)),
        });
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("exit lookup cancelled".into());
            }
            match search.advance(world, 500) {
                PathProgress::Searching => (),
                PathProgress::Found(_) => return Ok(goal),
                PathProgress::Failed => break,
            }
        }
    }
    Err("no reachable floor in the requested authored zone line".into())
}

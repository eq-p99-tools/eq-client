//! The shown zone's navigation mesh, read from the cache or built off the
//! frame at zone entry while the `navmesh` switch is on. A scripted walk
//! plans over it once it is ready.
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use eq_client_core::movement::CollisionWorld;
use eq_client_core::movement::navmesh::{self, Settings, SizedMeshes, cache};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

/// More refused tiles than this are summed up by reason, not listed.
const LISTED_REFUSALS: usize = 10;

/// Starts the shown zone's mesh, empty; its poll runs each frame.
pub(super) fn install(app: &mut App) {
    app.init_resource::<ZoneNavMesh>().add_systems(Update, poll);
}

/// The shown zone's mesh once it is ready, and the build or read making it.
#[derive(Resource, Default)]
pub(super) struct ZoneNavMesh {
    /// The mesh, split for its walker sizes.
    mesh: Option<SizedMeshes>,
    making: Option<Making>,
}

struct Making {
    zone: String,
    threads: usize,
    started: Instant,
    /// Set on leaving the zone; the build's threads stop at their next tile.
    cancel: Arc<AtomicBool>,
    task: Task<Option<(cache::Loaded, [f32; 3])>>,
}

impl ZoneNavMesh {
    /// Reads or builds `zone`'s mesh over `triangles`, its drops tried in
    /// the zone's `world` when there is one, keeping it under
    /// `settings_directory` when there is one. It is tagged for the three
    /// walker sizes measured from the models in `installation`, or without
    /// one built for a walker `height` tall alone.
    pub(super) fn start(
        &mut self,
        zone: &str,
        triangles: Vec<[[f32; 3]; 3]>,
        world: Option<CollisionWorld>,
        (height, installation): (f32, Option<PathBuf>),
        settings_directory: Option<PathBuf>,
    ) {
        self.leave();
        let cancel = Arc::new(AtomicBool::new(false));
        let threads = navmesh::build_threads();
        let task = {
            let (zone, cancel) = (zone.to_owned(), Arc::clone(&cancel));
            // One task that runs its own threads, so the tiles don't crowd
            // out the pool's other work.
            AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                let sizes = installation.as_deref().map_or([height; 3], sizes);
                cache::load_or_build(
                    settings_directory.as_deref(),
                    &zone,
                    &triangles,
                    world.as_ref(),
                    &Settings::sized(sizes),
                    threads,
                    &cancel,
                )
                .map(|loaded| (loaded, sizes))
            })
        };
        self.making = Some(Making {
            zone: zone.to_owned(),
            threads,
            started: Instant::now(),
            cancel,
            task,
        });
    }

    /// Drops the mesh, and stops a build in progress: it keeps nothing.
    pub(super) fn leave(&mut self) {
        if let Some(making) = self.making.take() {
            making.cancel.store(true, Ordering::Relaxed);
        }
        self.mesh = None;
    }

    /// The shown zone's mesh for a walker `height` tall, once it is ready:
    /// the smallest of its sizes that fits the walker. None for a walker
    /// taller than its tallest size, which stays on the grid.
    pub(super) fn mesh_for(&self, height: f32) -> Option<Arc<navmesh::NavMesh>> {
        self.mesh.as_ref()?.for_height(height)
    }
}

/// The heights of the three walker sizes in `installation`: in each of
/// [`races::SIZE_GROUPS`], the tallest model its global archives draw the
/// group's races with. A group with no model there takes the next shorter
/// group's height, or 6. Measured once per installation.
///
/// [`races::SIZE_GROUPS`]: eq_client_core::races::SIZE_GROUPS
pub(super) fn sizes(installation: &Path) -> [f32; 3] {
    static MEASURED: OnceLock<Mutex<HashMap<PathBuf, [f32; 3]>>> = OnceLock::new();
    let measured = MEASURED.get_or_init(Mutex::default);
    if let Some(sizes) = measured
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(installation)
    {
        return *sizes;
    }
    let donors = eq_client_assets::characters::DonorArchives::default();
    let tallest = |races: &[u32]| {
        races
            .iter()
            .flat_map(|&race| [0, 1].map(|gender| eq_client_core::races::model(race, gender)))
            .flatten()
            .filter_map(|model| {
                eq_client_assets::characters::load_global_character(installation, model, &donors)
                    .ok()
            })
            .map(|character| character.height())
            .reduce(f32::max)
    };
    let sizes = sizes_from(eq_client_core::races::SIZE_GROUPS.map(tallest));
    measured
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(installation.to_owned(), sizes);
    sizes
}

/// The three sizes from each group's tallest model, where it has one: a
/// group with none takes the size below it, or 6, and no size is shorter
/// than the one below it.
fn sizes_from(tallest: [Option<f32>; 3]) -> [f32; 3] {
    let mut sizes = [6.0_f32; 3];
    let mut shorter = None;
    for (size, tallest) in tallest.into_iter().enumerate() {
        let height = tallest
            .or(shorter)
            .unwrap_or(6.0)
            .max(shorter.unwrap_or(0.0));
        sizes[size] = height;
        shorter = Some(height);
    }
    sizes
}

/// Takes the mesh once it is ready, and says so in the log.
fn poll(mut navmesh: ResMut<ZoneNavMesh>) {
    let Some(making) = navmesh.making.as_mut() else {
        return;
    };
    let Some(loaded) = block_on(poll_once(&mut making.task)) else {
        return;
    };
    let making = navmesh.making.take().expect("polled above");
    // None only for a cancelled build, whose zone is gone.
    let Some((loaded, sizes)) = loaded else {
        return;
    };
    report(&making, &loaded, sizes);
    navmesh.mesh = Some(loaded.mesh.split(sizes));
}

fn report(making: &Making, loaded: &cache::Loaded, sizes: [f32; 3]) {
    let (source, tiles, refused, drop_seconds) = match &loaded.source {
        cache::Source::Cache => ("cache", None, &[][..], None),
        cache::Source::Built(report) => (
            "built",
            Some(report.tiles),
            &report.refused[..],
            Some(format!("{:.2}", report.drop_elapsed.as_secs_f32())),
        ),
    };
    let bytes = loaded.cached.as_ref().ok().copied();
    info!(
        zone = making.zone.as_str(),
        source,
        seconds = %format!("{:.2}", making.started.elapsed().as_secs_f32()),
        threads = making.threads,
        tiles,
        polygons = loaded.mesh.polygon_count(),
        islands = loaded.mesh.island_count(),
        failed_tiles = refused.len(),
        drop_links = loaded.mesh.drop_links().count(),
        drop_seconds,
        bytes,
        sizes = ?sizes,
        "Navmesh ready"
    );
    if let Err(error) = &loaded.cached {
        warn!(zone = making.zone.as_str(), %error, "Navmesh not kept");
    }
    if let cache::Source::Built(report) = &loaded.source {
        if report.refused_stretches > 0 {
            warn!(
                zone = making.zone.as_str(),
                stretches = report.refused_stretches,
                "Navmesh ledge drops refused"
            );
        }
        if refused.len() > LISTED_REFUSALS {
            for (reason, count) in report.refusals_by_reason() {
                warn!(
                    zone = making.zone.as_str(),
                    count, reason, "Navmesh tiles refused"
                );
            }
        } else {
            for tile in refused {
                warn!(
                    zone = making.zone.as_str(),
                    tile = ?tile.tile,
                    step = tile.step,
                    reason = tile.reason.as_str(),
                    "Navmesh tile refused"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> Vec<[[f32; 3]; 3]> {
        let (a, b, c, d) = (
            [-20.0, 0.0, -20.0],
            [-20.0, 0.0, 20.0],
            [20.0, 0.0, 20.0],
            [20.0, 0.0, -20.0],
        );
        vec![[a, b, c], [a, c, d]]
    }

    fn app() -> App {
        let mut app = App::new();
        install(&mut app);
        app
    }

    #[test]
    fn the_mesh_is_taken_once_ready_and_dropped_on_leaving() {
        let mut app = app();
        app.world_mut().resource_mut::<ZoneNavMesh>().start(
            "zone",
            floor(),
            None,
            (6.0, None),
            None,
        );
        let started = Instant::now();
        while app
            .world()
            .resource::<ZoneNavMesh>()
            .mesh_for(6.0)
            .is_none()
        {
            assert!(started.elapsed().as_secs() < 60, "never ready");
            app.update();
        }
        let navmesh = app.world().resource::<ZoneNavMesh>();
        assert!(navmesh.making.is_none());
        assert_eq!(navmesh.mesh_for(6.0).unwrap().island_count(), 1);
        // Built for one height, it plans for no taller walker.
        assert!(navmesh.mesh_for(5.0).is_some());
        assert!(navmesh.mesh_for(7.0).is_none());
        app.world_mut().resource_mut::<ZoneNavMesh>().leave();
        assert!(
            app.world()
                .resource::<ZoneNavMesh>()
                .mesh_for(6.0)
                .is_none()
        );
    }

    #[test]
    fn sizes_come_from_each_groups_tallest_model_and_never_shrink() {
        assert_eq!(
            sizes_from([Some(3.5), Some(5.8), Some(8.9)]),
            [3.5, 5.8, 8.9]
        );
        // A group with no model takes the size below it, or 6.
        assert_eq!(sizes_from([None, Some(5.8), None]), [6.0, 6.0, 6.0]);
        assert_eq!(sizes_from([Some(3.5), None, Some(8.9)]), [3.5, 3.5, 8.9]);
        // No size is shorter than the one below it.
        assert_eq!(
            sizes_from([Some(4.0), Some(3.0), Some(9.0)]),
            [4.0, 4.0, 9.0]
        );
    }

    #[test]
    fn leaving_stops_the_build() {
        let mut app = app();
        let mut navmesh = app.world_mut().resource_mut::<ZoneNavMesh>();
        navmesh.start("zone", floor(), None, (6.0, None), None);
        let cancel = Arc::clone(&navmesh.making.as_ref().unwrap().cancel);
        navmesh.leave();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(navmesh.making.is_none() && navmesh.mesh_for(6.0).is_none());
        app.update();
        assert!(
            app.world()
                .resource::<ZoneNavMesh>()
                .mesh_for(6.0)
                .is_none()
        );
    }
}

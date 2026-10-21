//! The shown zone's navigation mesh, read from the cache or built off the
//! frame at zone entry while the `navmesh` switch is on. A scripted walk
//! plans over it once it is ready.
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use eq_client_core::movement::navmesh::{self, NavMesh, Settings, cache};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The mesh, and the walker height it was built for.
    mesh: Option<(Arc<NavMesh>, f32)>,
    making: Option<Making>,
}

struct Making {
    zone: String,
    height: f32,
    threads: usize,
    started: Instant,
    /// Set on leaving the zone; the build's threads stop at their next tile.
    cancel: Arc<AtomicBool>,
    task: Task<Option<cache::Loaded>>,
}

impl ZoneNavMesh {
    /// Reads or builds `zone`'s mesh over `triangles` for a walker `height`
    /// tall, keeping it under `settings_directory` when there is one.
    pub(super) fn start(
        &mut self,
        zone: &str,
        triangles: Vec<[[f32; 3]; 3]>,
        height: f32,
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
                cache::load_or_build(
                    settings_directory.as_deref(),
                    &zone,
                    &triangles,
                    &Settings::walker(height),
                    threads,
                    &cancel,
                )
            })
        };
        self.making = Some(Making {
            zone: zone.to_owned(),
            height,
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

    /// The shown zone's mesh, once it is ready, when it was built for a
    /// walker `height` tall.
    pub(super) fn mesh_for(&self, height: f32) -> Option<Arc<NavMesh>> {
        let (mesh, built) = self.mesh.as_ref()?;
        ((built - height).abs() < 1e-3).then(|| Arc::clone(mesh))
    }
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
    let Some(loaded) = loaded else {
        return;
    };
    report(&making, &loaded);
    navmesh.mesh = Some((Arc::new(loaded.mesh), making.height));
}

fn report(making: &Making, loaded: &cache::Loaded) {
    let (source, tiles, refused) = match &loaded.source {
        cache::Source::Cache => ("cache", None, &[][..]),
        cache::Source::Built(report) => ("built", Some(report.tiles), &report.refused[..]),
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
        bytes,
        "Navmesh ready"
    );
    if let Err(error) = &loaded.cached {
        warn!(zone = making.zone.as_str(), %error, "Navmesh not kept");
    }
    if let cache::Source::Built(report) = &loaded.source {
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
        app.world_mut()
            .resource_mut::<ZoneNavMesh>()
            .start("zone", floor(), 6.0, None);
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
        // Built for one height, it plans for no other.
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
    fn leaving_stops_the_build() {
        let mut app = app();
        let mut navmesh = app.world_mut().resource_mut::<ZoneNavMesh>();
        navmesh.start("zone", floor(), 6.0, None);
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

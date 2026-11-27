//! The shown zone's navigation mesh, read from the cache or built off the
//! frame at zone entry while the `navmesh` switch is on (the default). A
//! scripted walk plans over it once it is ready, and says why when it plans
//! on the grid instead.
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use eq_client_core::movement::CollisionWorld;
use eq_client_core::movement::navmesh::{self, Settings, SizedMeshes, cache};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// More refused tiles than this are summed up by reason, not listed.
const LISTED_REFUSALS: usize = 10;

#[cfg(test)]
mod lifecycle;

/// Starts the shown zone's mesh, empty; its poll runs each frame. With the
/// switch off, startup says so, since every scripted walk then plans on the
/// grid.
pub(super) fn install(app: &mut App, enabled: bool) {
    app.insert_resource(ZoneNavMesh {
        enabled,
        ..ZoneNavMesh::default()
    })
    .add_systems(
        Update,
        (refresh, poll).chain().in_set(crate::systems::Stage::Scene),
    );
    if !enabled {
        app.add_systems(Startup, || {
            warn!(
                "Navigation meshes are off (--no-navmesh): scripted walks plan with grid search only"
            );
        });
    }
}

/// The shown zone's mesh once it is ready, and the build or read making it.
#[derive(Resource, Default)]
pub(super) struct ZoneNavMesh {
    /// Whether zone entry reads or builds a mesh at all.
    enabled: bool,
    /// The mesh, split for its walker sizes.
    mesh: Option<SizedMeshes>,
    making: Option<Making>,
    source: Option<Source>,
    generation: u64,
    failure: Option<&'static str>,
    summary: Option<Summary>,
}

struct Summary {
    source: &'static str,
    polygons: usize,
    islands: usize,
    incomplete: bool,
}

/// Retain shared build inputs so a late zone-header scale changes both the
/// mesh and its cache identity, rather than keeping an under-filtered mesh.
#[derive(Clone)]
struct Source {
    zone: String,
    triangles: Arc<Vec<[[f32; 3]; 3]>>,
    world: Option<CollisionWorld>,
    directory: Option<PathBuf>,
    settings: Settings,
}

/// Why a walk plans on the grid rather than the zone's mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GridReason {
    /// The client was started with `--no-navmesh`.
    Disabled,
    /// The zone's mesh is still being read or built.
    Building,
    /// No mesh was started for this zone, or its build was stopped.
    Missing,
    /// The walker is taller than the tallest mesh size.
    TooTall,
    /// The current build failed or produced no walkable polygons.
    Failed,
    /// The selected size has no walkable polygons in this mesh.
    NoFloor,
}

impl GridReason {
    /// The reason as the log and the script's events name it.
    pub(super) const fn describe(self) -> &'static str {
        match self {
            Self::Disabled => "navigation meshes are off (--no-navmesh)",
            Self::Building => "the zone's navigation mesh is still being built",
            Self::Missing => "this zone has no navigation mesh",
            Self::TooTall => "the character is taller than the tallest navigation mesh size",
            Self::Failed => "the current zone navigation mesh failed to load or build",
            Self::NoFloor => "the navigation mesh has no floor for this character size",
        }
    }
}

struct Making {
    zone: String,
    generation: u64,
    threads: usize,
    started: Instant,
    /// Set on leaving the zone; the build's threads stop at their next tile.
    cancel: Arc<AtomicBool>,
    task: Task<Option<(cache::Loaded, [f32; 3])>>,
}

impl ZoneNavMesh {
    #[cfg(test)]
    pub(super) fn set_test_settings(&mut self, settings: Settings) {
        self.source.as_mut().unwrap().settings = settings;
    }
    #[cfg(test)]
    pub(super) fn hold_for_test(&mut self, zone: &str, triangles: Vec<[[f32; 3]; 3]>) {
        self.start(zone, triangles, None, None, Settings::sized(walker_sizes()));
        let making = self.making.as_mut().unwrap();
        making.cancel.store(true, Ordering::Relaxed);
        making.cancel = Arc::new(AtomicBool::new(false));
        making.task =
            AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(std::future::pending());
    }

    #[cfg(test)]
    pub(super) fn complete_for_test(&mut self, mesh: Option<navmesh::NavMesh>) {
        let sizes = self.source.as_ref().unwrap().settings.sizes;
        self.making.as_mut().unwrap().task = AsyncComputeTaskPool::get_or_init(TaskPool::new)
            .spawn(async move {
                mesh.map(|mesh| {
                    (
                        cache::Loaded {
                            mesh,
                            source: cache::Source::Cache,
                            cached: Ok(0),
                        },
                        sizes,
                    )
                })
            });
    }

    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn floors(&self, zone: &str) -> Option<Arc<Vec<[[f32; 3]; 3]>>> {
        self.source
            .as_ref()
            .filter(|source| source.zone == zone)
            .map(|source| Arc::clone(&source.triangles))
    }

    /// Reads or builds `zone`'s mesh over `triangles`, its drops tried in
    /// the zone's `world` when there is one, keeping it under
    /// `settings_directory` when there is one. It is tagged for the three
    /// walker sizes.
    pub(super) fn start(
        &mut self,
        zone: &str,
        triangles: Vec<[[f32; 3]; 3]>,
        world: Option<CollisionWorld>,
        settings_directory: Option<PathBuf>,
        settings: Settings,
    ) {
        self.leave();
        if !self.enabled {
            return;
        }
        self.source = Some(Source {
            zone: zone.to_owned(),
            triangles: Arc::new(triangles),
            world,
            directory: settings_directory,
            settings,
        });
        self.build();
    }

    fn build(&mut self) {
        let Some(source) = self.source.clone() else {
            return;
        };
        if let Some(making) = self.making.take() {
            making.cancel.store(true, Ordering::Relaxed);
        }
        self.mesh = None;
        self.summary = None;
        self.failure = None;
        self.generation = self.generation.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let threads = navmesh::build_threads();
        let task = {
            let cancel = Arc::clone(&cancel);
            let source = source.clone();
            // One task that runs its own threads, so the tiles don't crowd
            // out the pool's other work.
            AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cache::load_or_build(
                        source.directory.as_deref(),
                        &source.zone,
                        &source.triangles,
                        source.world.as_ref(),
                        &source.settings,
                        threads,
                        &cancel,
                    )
                }))
                .ok()
                .flatten()
                .map(|loaded| (loaded, source.settings.sizes))
            })
        };
        self.making = Some(Making {
            zone: source.zone,
            generation: self.generation,
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
        self.source = None;
        self.summary = None;
        self.failure = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// The shown zone's mesh for a walker drawn `size` tall, once it is
    /// ready: the smallest of its sizes that fits the walker. Otherwise why
    /// the walker plans on the grid, including a walker taller than the
    /// tallest size.
    pub(super) fn for_walker(&self, size: f32) -> Result<Arc<navmesh::NavMesh>, GridReason> {
        match &self.mesh {
            Some(mesh) => mesh
                .for_height(size)
                .ok_or(GridReason::TooTall)
                .and_then(|mesh| {
                    if mesh.polygon_count() == 0 {
                        Err(GridReason::NoFloor)
                    } else {
                        Ok(mesh)
                    }
                }),
            None if !self.enabled => Err(GridReason::Disabled),
            None if self.making.is_some() => Err(GridReason::Building),
            None if self.failure.is_some() => Err(GridReason::Failed),
            None => Err(GridReason::Missing),
        }
    }

    pub(super) fn for_zone_walker(
        &self,
        zone: &str,
        size: f32,
    ) -> Result<Arc<navmesh::NavMesh>, GridReason> {
        if self.enabled
            && self
                .source
                .as_ref()
                .is_none_or(|source| source.zone != zone)
        {
            return Err(GridReason::Missing);
        }
        self.for_walker(size)
    }

    pub(super) fn status(&self, zone: &str, size: Option<f32>) -> serde_json::Value {
        let current = self
            .source
            .as_ref()
            .is_some_and(|source| source.zone == zone);
        let summary = current.then_some(self.summary.as_ref()).flatten();
        let phase = if !self.enabled {
            "disabled"
        } else if !current {
            "missing"
        } else if self.making.is_some() {
            "building"
        } else if self.failure.is_some() {
            "failed"
        } else if summary.is_some_and(|summary| summary.incomplete) {
            "partial"
        } else if summary.is_some() {
            "ready"
        } else {
            "missing"
        };
        let walker = size.map(|size| {
            self.for_zone_walker(zone, size)
                .map(|_| ())
                .map_err(GridReason::describe)
        });
        serde_json::json!({"zone":zone,"phase":phase,"ready":phase=="ready",
            "generation":self.generation,"source":summary.map(|s|s.source),
            "polygons":summary.map(|s|s.polygons),"islands":summary.map(|s|s.islands),
            "failure":current.then_some(self.failure).flatten(),
            "walker_ready":walker.as_ref().map(Result::is_ok),"walker_error":walker.and_then(Result::err),
            "elapsed_ms":current.then(|| self.making.as_ref().map(|m|m.started.elapsed().as_millis())).flatten()})
    }
}

/// Unknown headers use the unreduced, conservative settings. A later
/// header replaces a running build as well as an already loaded mesh.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameter wrappers.
fn refresh(
    mut navmesh: ResMut<ZoneNavMesh>,
    online: Option<Res<crate::online::OnlineState>>,
    viewer: Option<Res<crate::ViewerSettings>>,
) {
    let (Some(online), Some(viewer), Some(source)) = (online, viewer, navmesh.source.as_mut())
    else {
        return;
    };
    let wanted = settings(
        viewer.0.installed_client,
        eq_client_assets::scale::ZoneScale {
            classic_reduced: online
                .world()
                .classic_model_scale_flag()
                .is_some_and(|flag| flag != 0),
        },
    );
    if source.settings != wanted {
        source.settings = wanted;
        navmesh.build();
    } else if navmesh.mesh.is_none() && navmesh.making.is_none() && navmesh.failure.is_none() {
        navmesh.build();
    }
}

/// The heights of the three walker sizes: in each of
/// [`races::SIZE_GROUPS`], the tallest size its races are drawn at, by
/// `EQEmu`'s default sizes (dwarf 4, human 6, ogre 9). A walker's own drawn
/// size picks its mesh.
///
/// [`races::SIZE_GROUPS`]: eq_client_core::races::SIZE_GROUPS
pub(super) fn walker_sizes() -> [f32; 3] {
    eq_client_core::races::walker_sizes(|race| eq_client_core::size_or_default(race, 0.0))
}

/// What a zone's mesh is built with for `client`: the three walker sizes,
/// each needing the room the generation's headroom test asks of it
/// standing, with S the stand height a size is drawn at (Titanium
/// 0x48bb83, reads R1d and R49-9, with its 0.5 margin; checklist 6.504).
/// Ducking asks less than the capsule already does, and walkers don't
/// mount, so neither is asked. The mesh asks one cell height more than the
/// test: a span's headroom is counted in whole cells, so a span the voxel
/// rounding makes look up to a cell taller still never leads a walker to a
/// refused move (reviewer N1). In a zone whose header sets the classic model
/// scale flag, Titanium's S is 1/1.3 smaller, as the test's is (read R49-9f a);
/// Mac follows its own unshrunk model-scale policy. A mesh
/// built before the header is known (a prebuild, a select spot) asks the
/// larger room, which never leads a walker to a refused move either.
pub(crate) fn settings(
    client: eq_client_assets::ui::InstalledClient,
    zone: eq_client_assets::scale::ZoneScale,
) -> Settings {
    let sized = Settings::sized(walker_sizes());
    match client.headroom() {
        Some(rule) => {
            let guard = sized.cell_height;
            let stands = eq_client_core::races::walker_sizes(|race| {
                [0, 1]
                    .into_iter()
                    .map(|gender| {
                        eq_client_core::drawn_feet_offset(client.model_scale().drawn_scale(
                            race,
                            gender,
                            eq_client_core::size_or_default(race, 0.0),
                            zone,
                        ))
                    })
                    .fold(0.0_f32, f32::max)
            });
            Settings {
                standing_room: stands.map(|stand| rule.room(stand, false, false) + guard),
                ..sized
            }
        }
        None => sized,
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
    if making.generation != navmesh.generation
        || navmesh
            .source
            .as_ref()
            .is_none_or(|source| source.zone != making.zone)
    {
        return;
    }
    let Some((loaded, sizes)) = loaded else {
        navmesh.failure = Some("build failed or was cancelled");
        warn!(zone = making.zone, "Navmesh failed");
        return;
    };
    if loaded.mesh.polygon_count() == 0 {
        navmesh.failure = Some("no walkable polygons");
        warn!(zone = making.zone, "Navmesh failed: no walkable polygons");
        return;
    };
    report(
        &making.zone,
        (making.started, making.threads),
        &loaded,
        sizes,
    );
    navmesh.summary = Some(Summary {
        source: if matches!(&loaded.source, cache::Source::Cache) {
            "cache"
        } else {
            "built"
        },
        polygons: loaded.mesh.polygon_count(),
        islands: loaded.mesh.island_count(),
        incomplete: loaded.mesh.incomplete(),
    });
    navmesh.mesh = Some(loaded.mesh.split(sizes));
}

/// Logs a mesh ready for `zone`, read or built since `started` on
/// `threads` threads, with what its build refused.
pub(super) fn report(
    zone: &str,
    (started, threads): (Instant, usize),
    loaded: &cache::Loaded,
    sizes: [f32; 3],
) {
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
        zone = zone,
        source,
        seconds = %format!("{:.2}", started.elapsed().as_secs_f32()),
        threads,
        tiles,
        polygons = loaded.mesh.polygon_count(),
        islands = loaded.mesh.island_count(),
        failed_tiles = tiles.map(|_| refused.len()),
        drop_links = loaded.mesh.drop_links().count(),
        drop_seconds,
        bytes,
        sizes = ?sizes,
        complete = !loaded.mesh.incomplete(),
        "Navmesh loaded"
    );
    if loaded.mesh.incomplete() {
        warn!(zone, "Navmesh partial: some build coverage was refused");
    } else {
        info!(
            zone,
            polygons = loaded.mesh.polygon_count(),
            islands = loaded.mesh.island_count(),
            "Navmesh ready"
        );
    }
    if let Err(error) = &loaded.cached {
        warn!(zone = zone, %error, "Navmesh not kept");
    }
    if let cache::Source::Built(report) = &loaded.source {
        if report.refused_stretches > 0 {
            warn!(
                zone = zone,
                stretches = report.refused_stretches,
                "Navmesh ledge drops refused"
            );
        }
        if report.failed_hole_merges > 0 {
            warn!(
                zone = zone,
                holes = report.failed_hole_merges,
                merged = report.merged_holes,
                "Navmesh holes that couldn't be joined to their outlines"
            );
        }
        if report.deep_triangles > 0 {
            warn!(
                zone = zone,
                triangles = report.deep_triangles,
                "Navmesh triangles left out below the span limit"
            );
        }
        if refused.len() > LISTED_REFUSALS {
            for (reason, count) in report.refusals_by_reason() {
                warn!(zone = zone, count, reason, "Navmesh tiles refused");
            }
        } else {
            for tile in refused {
                warn!(
                    zone = zone,
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
    #[test]
    fn a_titanium_mesh_asks_each_size_for_twice_its_stand_height_and_the_margin() {
        use eq_client_assets::{scale::ZoneScale, ui::InstalledClient};
        let titanium = settings(InstalledClient::Titanium, ZoneScale::default());
        let eqmac = settings(InstalledClient::EqMac, ZoneScale::default());
        // A zone with the classic model scale flag asks 1/1.3 of S (R49-9f).
        let reduced = settings(
            InstalledClient::Titanium,
            ZoneScale {
                classic_reduced: true,
            },
        );
        let lift = eqmac.lift;
        assert_eq!(
            eqmac,
            settings(
                InstalledClient::EqMac,
                ZoneScale {
                    classic_reduced: true
                }
            )
        );
        assert_eq!(titanium.sizes, walker_sizes());
        assert_eq!(eqmac, titanium);
        for (index, size) in walker_sizes().into_iter().enumerate() {
            let room =
                2.0 * eq_client_core::drawn_feet_offset(size * 0.2) + 0.5 + titanium.cell_height;
            assert!((titanium.clearance(index) - room.max(size + lift)).abs() < 1e-5);
            assert!((eqmac.clearance(index) - room.max(size + lift)).abs() < 1e-5);
            let small = 2.0 * eq_client_core::drawn_feet_offset(size * 0.2 / 1.3)
                + 0.5
                + titanium.cell_height;
            assert!((reduced.clearance(index) - small.max(size + lift)).abs() < 1e-5);
        }
    }

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
        install(&mut app, true);
        app
    }

    #[test]
    fn a_late_header_replaces_titanium_build_settings_and_key_but_not_macs() {
        use crate::online::{OnlineState, testing};
        use eq_client_assets::{scale::ZoneScale, ui::InstalledClient};
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let mut app = app();
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, 11, testing::player(7));
            app.insert_resource(online)
                .init_resource::<crate::ViewerSettings>();
            app.world_mut()
                .resource_mut::<crate::ViewerSettings>()
                .0
                .installed_client = client;
            app.world_mut().resource_mut::<ZoneNavMesh>().start(
                "zone",
                floor(),
                None,
                None,
                settings(client, ZoneScale::default()),
            );
            let navmesh = app.world().resource::<ZoneNavMesh>();
            let source = navmesh.source.as_ref().unwrap();
            let old_key = cache::key(&source.triangles, source.world.as_ref(), &source.settings);
            let cancelled = Arc::clone(&navmesh.making.as_ref().unwrap().cancel);
            let mut header = [0; 700];
            header[592] = 1;
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [eq_client_core::WorldEvent::ZoneDescription(
                    eq_client_core::zones::titanium_description(&header).unwrap(),
                )],
            );
            app.update();
            let navmesh = app.world().resource::<ZoneNavMesh>();
            let source = navmesh.source.as_ref().unwrap();
            let new_key = cache::key(&source.triangles, source.world.as_ref(), &source.settings);
            assert_eq!(old_key != new_key, client == InstalledClient::Titanium);
            assert_eq!(
                cancelled.load(Ordering::Relaxed),
                client == InstalledClient::Titanium
            );
            assert_eq!(
                source.settings,
                settings(
                    client,
                    ZoneScale {
                        classic_reduced: true
                    }
                )
            );
        }
    }

    #[test]
    fn the_mesh_is_taken_once_ready_and_dropped_on_leaving() {
        let mut app = app();
        app.world_mut().resource_mut::<ZoneNavMesh>().start(
            "zone",
            floor(),
            None,
            None,
            Settings::sized(walker_sizes()),
        );
        let started = Instant::now();
        while app
            .world()
            .resource::<ZoneNavMesh>()
            .for_walker(6.0)
            .ok()
            .is_none()
        {
            assert!(started.elapsed().as_secs() < 60, "never ready");
            app.update();
        }
        let navmesh = app.world().resource::<ZoneNavMesh>();
        assert!(navmesh.making.is_none());
        assert_eq!(navmesh.for_walker(6.0).ok().unwrap().island_count(), 1);
        // It plans for walkers up to the tallest size, an ogre's 9, and no
        // taller.
        assert!(navmesh.for_walker(3.0).ok().is_some() && navmesh.for_walker(9.0).ok().is_some());
        assert!(navmesh.for_walker(9.5).ok().is_none());
        app.world_mut().resource_mut::<ZoneNavMesh>().leave();
        assert!(
            app.world()
                .resource::<ZoneNavMesh>()
                .for_walker(6.0)
                .ok()
                .is_none()
        );
    }

    #[test]
    fn leaving_stops_the_build() {
        let mut app = app();
        let mut navmesh = app.world_mut().resource_mut::<ZoneNavMesh>();
        navmesh.start("zone", floor(), None, None, Settings::sized(walker_sizes()));
        let cancel = Arc::clone(&navmesh.making.as_ref().unwrap().cancel);
        navmesh.leave();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(navmesh.making.is_none() && navmesh.for_walker(6.0).ok().is_none());
        app.update();
        assert!(
            app.world()
                .resource::<ZoneNavMesh>()
                .for_walker(6.0)
                .ok()
                .is_none()
        );
    }

    #[test]
    fn a_walk_without_a_mesh_names_why() {
        let mut off = App::new();
        install(&mut off, false);
        let navmesh = off.world().resource::<ZoneNavMesh>();
        assert_eq!(navmesh.for_walker(6.0).err(), Some(GridReason::Disabled));

        let mut app = app();
        assert_eq!(
            app.world().resource::<ZoneNavMesh>().for_walker(6.0).err(),
            Some(GridReason::Missing)
        );
        app.world_mut().resource_mut::<ZoneNavMesh>().start(
            "floor",
            floor(),
            None,
            None,
            Settings::sized(walker_sizes()),
        );
        assert_eq!(
            app.world().resource::<ZoneNavMesh>().for_walker(6.0).err(),
            Some(GridReason::Building)
        );
        let started = Instant::now();
        while app.world().resource::<ZoneNavMesh>().mesh.is_none() {
            assert!(started.elapsed().as_secs() < 60, "never ready");
            app.update();
        }
        let navmesh = app.world().resource::<ZoneNavMesh>();
        assert!(navmesh.for_walker(6.0).is_ok());
        assert_eq!(navmesh.for_walker(9.5).err(), Some(GridReason::TooTall));
    }
}

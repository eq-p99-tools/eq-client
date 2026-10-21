//! Building every installed zone's navigation mesh from the login screen,
//! while the `navmesh` switch is on: one button of ours, **Build navmeshes**,
//! that builds the zones whose meshes aren't kept yet, one at a time in
//! zone-number order, into the cache zone entry reads, and a line counting
//! them. Pressing it again stops the batch, as does a world connection, so
//! zone entry's own build never competes with it; every zone finished is
//! kept, and the next press carries on from there.
//!
//! Every mesh is built on the player's own PC from their own installed
//! files; nothing is sent to any server.
use crate::{
    login::{FrontEnd, Screen},
    online::OnlineState,
};
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use eq_client_core::movement::navmesh::{self, Settings, cache};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// The highest zone number the zone table names.
const HIGHEST_ZONE: u32 = 999;

/// The login screen's button of ours, which starts or stops the batch.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Toggle;

/// The zones a batch builds, and how it builds one.
pub(crate) trait Zones: Send + Sync + 'static {
    /// The zones there are to build, in the order they are built.
    fn list(&self) -> Vec<String>;
    /// Whether `zone`'s mesh is kept already.
    fn present(&self, zone: &str) -> bool;
    /// Builds and keeps `zone`'s mesh, unless `cancel` is set first.
    fn build(&self, zone: &str, cancel: &AtomicBool) -> Built;
}

/// How building one zone ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Built {
    /// Its mesh is kept.
    Kept,
    /// It couldn't be loaded, built or kept, for this reason.
    Failed(String),
    /// The batch was stopped first; nothing of it is kept.
    Stopped,
}

/// The batch, offered while the `navmesh` switch is on.
#[derive(Resource)]
pub(crate) struct NavmeshPrebuild {
    zones: Arc<dyn Zones>,
    state: State,
    /// The zones the last batch couldn't build.
    failed: usize,
}

enum State {
    /// Not counted yet: the count is first read when the login screen
    /// shows.
    Uncounted,
    Counting(Task<Counted>),
    Idle(Counted),
    Building(Batch),
}

/// The zones there are, and those whose meshes aren't kept.
struct Counted {
    total: usize,
    missing: Vec<String>,
}

struct Batch {
    total: usize,
    progress: Arc<Progress>,
    /// Set by Stop or a world connection; the build stops at its next tile.
    cancel: Arc<AtomicBool>,
    started: Instant,
    /// The zones still not kept once it ends.
    task: Task<Vec<String>>,
}

/// How far a batch has got, as its task goes.
#[derive(Default)]
struct Progress {
    done: AtomicUsize,
    failed: AtomicUsize,
    current: Mutex<Option<String>>,
}

/// Offers the batch on the login screen while the switch is on, where
/// there is an installation to read and a settings folder to keep the
/// meshes in.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn offer(mut commands: Commands, settings: Res<crate::ViewerSettings>) {
    let config = &settings.0;
    if let (true, Some(installation), Some(directory)) = (
        config.navmesh,
        config.eq_directory.clone(),
        config.settings_directory.clone(),
    ) {
        commands.insert_resource(NavmeshPrebuild::installed(installation, directory));
    }
}

/// The batch's presses, then its tasks, while it is offered.
pub(super) fn run_batch() -> ScheduleConfigs<ScheduleSystem> {
    (press, poll)
        .chain()
        .run_if(resource_exists::<NavmeshPrebuild>)
}

impl NavmeshPrebuild {
    /// The batch over the zones in `installation`, kept under
    /// `settings_directory`.
    pub(crate) fn installed(installation: PathBuf, settings_directory: PathBuf) -> Self {
        Self::over(Arc::new(Installed {
            installation,
            directory: settings_directory,
        }))
    }

    fn over(zones: Arc<dyn Zones>) -> Self {
        Self {
            zones,
            state: State::Uncounted,
            failed: 0,
        }
    }

    /// The button's words.
    pub(crate) fn label(&self) -> &'static str {
        if self.building() {
            "Stop"
        } else {
            "Build navmeshes"
        }
    }

    /// Whether pressing the button does anything: it stops a batch, or
    /// starts one with zones still to build.
    pub(crate) fn usable(&self) -> bool {
        match &self.state {
            State::Building(_) => true,
            State::Idle(counted) => !counted.missing.is_empty(),
            State::Uncounted | State::Counting(_) => false,
        }
    }

    /// The line under the button, such as `Navmeshes: 38 / 211 zones
    /// (building ecommons), 2 failed`.
    pub(crate) fn line(&self) -> String {
        let (done, total, building, failed) = match &self.state {
            State::Uncounted | State::Counting(_) => return "Navmeshes: counting zones".to_owned(),
            State::Idle(counted) => (
                counted.total - counted.missing.len(),
                counted.total,
                None,
                self.failed,
            ),
            State::Building(batch) => (
                batch.progress.done.load(Ordering::Relaxed),
                batch.total,
                batch
                    .progress
                    .current
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
                batch.progress.failed.load(Ordering::Relaxed),
            ),
        };
        let building = building.map(|zone| format!(" (building {zone})"));
        let failed = (failed > 0).then(|| format!(", {failed} failed"));
        format!(
            "Navmeshes: {done} / {total} zones{}{}",
            building.unwrap_or_default(),
            failed.unwrap_or_default()
        )
    }

    /// What the button and the line show, which draws the login screen
    /// again when it changes.
    pub(crate) fn appearance(&self) -> String {
        format!("{}:{}:{}", self.label(), self.usable(), self.line())
    }

    fn building(&self) -> bool {
        matches!(self.state, State::Building(_))
    }

    /// Starts counting the kept zones, off the frame, if they aren't yet.
    fn count(&mut self) {
        if matches!(self.state, State::Uncounted) {
            let zones = Arc::clone(&self.zones);
            self.state = State::Counting(spawn(move || {
                let list = zones.list();
                let total = list.len();
                let missing = list
                    .into_iter()
                    .filter(|zone| !zones.present(zone))
                    .collect();
                Counted { total, missing }
            }));
        }
    }

    /// Starts building the zones not kept yet, one at a time.
    fn start(&mut self) {
        let State::Idle(counted) = std::mem::replace(&mut self.state, State::Uncounted) else {
            return;
        };
        let missing = counted.missing.len();
        let progress = Arc::new(Progress {
            done: AtomicUsize::new(counted.total - counted.missing.len()),
            ..default()
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let task = {
            let (zones, progress, cancel) = (
                Arc::clone(&self.zones),
                Arc::clone(&progress),
                Arc::clone(&cancel),
            );
            spawn(move || run(&*zones, counted.missing, &progress, &cancel))
        };
        info!(zones = counted.total, missing, "Navmesh prebuild started");
        self.state = State::Building(Batch {
            total: counted.total,
            progress,
            cancel,
            started: Instant::now(),
            task,
        });
    }

    /// Stops a batch under way at its next tile; it keeps every zone
    /// finished.
    fn stop(&self) {
        if let State::Building(batch) = &self.state {
            batch.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// One task that runs its own threads, so the tiles don't crowd out the
/// pool's other work.
fn spawn<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Task<T> {
    AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move { work() })
}

/// Builds `missing` in turn, counting each into `progress`, until `cancel`
/// is set; the zones still not kept.
fn run(
    zones: &dyn Zones,
    missing: Vec<String>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Vec<String> {
    let mut left = Vec::new();
    let mut zones_left = missing.into_iter();
    for zone in zones_left.by_ref() {
        if cancel.load(Ordering::Relaxed) {
            left.push(zone);
            break;
        }
        *progress
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(zone.clone());
        // A zone that panics on the way is one that failed, not the end of
        // the batch.
        let built = catch_unwind(AssertUnwindSafe(|| zones.build(&zone, cancel)))
            .unwrap_or_else(|_| Built::Failed("panicked".to_owned()));
        match built {
            Built::Kept => {
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            Built::Failed(reason) => {
                warn!(zone = zone.as_str(), reason, "Navmesh prebuild failed");
                progress.failed.fetch_add(1, Ordering::Relaxed);
                left.push(zone);
            }
            Built::Stopped => {
                left.push(zone);
                break;
            }
        }
    }
    *progress
        .current
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
    left.extend(zones_left);
    left
}

/// Starts or stops the batch as its button is pressed.
fn press(
    mut prebuild: ResMut<NavmeshPrebuild>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<Toggle>)>,
) {
    if !prebuild.usable()
        || !buttons
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }
    if prebuild.building() {
        prebuild.stop();
    } else {
        prebuild.start();
    }
}

/// Counts the kept zones when the login screen first shows, stops the
/// batch when a world connection starts, and takes a count or a batch once
/// it ends.
fn poll(mut prebuild: ResMut<NavmeshPrebuild>, (front, online): (Res<FrontEnd>, Res<OnlineState>)) {
    match front.screen(&online) {
        Screen::Login => prebuild.count(),
        Screen::Servers => (),
        Screen::Characters | Screen::World => prebuild.stop(),
    }
    let prebuild = &mut *prebuild;
    match &mut prebuild.state {
        State::Counting(task) => {
            if let Some(counted) = block_on(poll_once(task)) {
                prebuild.state = State::Idle(counted);
            }
        }
        State::Building(batch) => {
            if let Some(missing) = block_on(poll_once(&mut batch.task)) {
                let failed = batch.progress.failed.load(Ordering::Relaxed);
                info!(
                    built = batch.progress.done.load(Ordering::Relaxed),
                    zones = batch.total,
                    failed,
                    stopped = batch.cancel.load(Ordering::Relaxed),
                    seconds = %format!("{:.1}", batch.started.elapsed().as_secs_f32()),
                    "Navmesh prebuild done"
                );
                prebuild.failed = failed;
                prebuild.state = State::Idle(Counted {
                    total: batch.total,
                    missing,
                });
            }
        }
        State::Uncounted | State::Idle(_) => (),
    }
}

/// The zones of an installation: those of the zone table whose archive it
/// holds, kept under the settings directory.
struct Installed {
    installation: PathBuf,
    directory: PathBuf,
}

impl Installed {
    /// The settings zone entry builds with.
    fn settings() -> Settings {
        Settings::sized(crate::zone_navmesh::walker_sizes())
    }
}

impl Zones for Installed {
    fn list(&self) -> Vec<String> {
        installed_zones(&self.installation)
    }

    fn present(&self, zone: &str) -> bool {
        cache::present(&self.directory, zone, &Self::settings())
    }

    fn build(&self, zone: &str, cancel: &AtomicBool) -> Built {
        let started = Instant::now();
        let asset = match eq_client_assets::load_zone_collision(&self.installation, zone) {
            Ok(asset) => asset,
            Err(error) => return Built::Failed(error.to_string()),
        };
        let triangles = crate::collision_triangles(&asset);
        let world = crate::collision_world(&asset, triangles.clone());
        let settings = Self::settings();
        let threads = navmesh::build_threads();
        let Some(loaded) = cache::load_or_build(
            Some(&self.directory),
            zone,
            &triangles,
            world.as_ref(),
            &settings,
            threads,
            cancel,
        ) else {
            return Built::Stopped;
        };
        crate::zone_navmesh::report(zone, (started, threads), &loaded, settings.sizes);
        match &loaded.cached {
            Ok(_) => Built::Kept,
            Err(error) => Built::Failed(error.to_string()),
        }
    }
}

/// The short names of the zone table's zones whose archive, `<name>.s3d`,
/// is in `installation`, in zone-number order.
pub(crate) fn installed_zones(installation: &Path) -> Vec<String> {
    (0..=HIGHEST_ZONE)
        .filter_map(eq_client_core::zones::short_name)
        .filter(|name| installation.join(format!("{name}.s3d")).is_file())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Zones that build at once, save those named failing; `held` holds
    /// each build until the test lets it go.
    struct Fake {
        zones: Vec<&'static str>,
        kept: Mutex<HashSet<String>>,
        failing: HashSet<&'static str>,
        held: Mutex<bool>,
        tried: AtomicUsize,
    }

    impl Fake {
        fn new(zones: &[&'static str], kept: &[&str], failing: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                zones: zones.to_vec(),
                kept: Mutex::new(kept.iter().map(|zone| (*zone).to_owned()).collect()),
                failing: failing.iter().copied().collect(),
                held: Mutex::new(false),
                tried: AtomicUsize::new(0),
            })
        }
    }

    impl Zones for Fake {
        fn list(&self) -> Vec<String> {
            self.zones.iter().map(|zone| (*zone).to_owned()).collect()
        }

        fn present(&self, zone: &str) -> bool {
            self.kept.lock().unwrap().contains(zone)
        }

        fn build(&self, zone: &str, cancel: &AtomicBool) -> Built {
            self.tried.fetch_add(1, Ordering::Relaxed);
            while *self.held.lock().unwrap() {
                if cancel.load(Ordering::Relaxed) {
                    return Built::Stopped;
                }
                std::thread::yield_now();
            }
            if self.failing.contains(zone) {
                return Built::Failed("unreadable".to_owned());
            }
            self.kept.lock().unwrap().insert(zone.to_owned());
            Built::Kept
        }
    }

    fn app(zones: Arc<Fake>) -> App {
        let mut app = App::new();
        app.insert_resource(FrontEnd::default())
            .insert_resource(OnlineState::new(true))
            .insert_resource(NavmeshPrebuild::over(zones))
            .add_systems(Update, run_batch());
        app
    }

    fn line(app: &App) -> String {
        app.world().resource::<NavmeshPrebuild>().line()
    }

    /// Runs frames until `done` holds of the batch.
    fn until(app: &mut App, done: impl Fn(&NavmeshPrebuild) -> bool) {
        let started = Instant::now();
        while !done(app.world().resource::<NavmeshPrebuild>()) {
            assert!(started.elapsed().as_secs() < 30, "{}", line(app));
            app.update();
        }
    }

    fn idle(prebuild: &NavmeshPrebuild) -> bool {
        matches!(prebuild.state, State::Idle(_))
    }

    fn press(app: &mut App) {
        let button = app.world_mut().spawn((Toggle, Interaction::Pressed)).id();
        app.update();
        app.world_mut().entity_mut(button).despawn();
    }

    #[test]
    fn the_batch_is_offered_only_with_the_switch_on_and_somewhere_to_keep_meshes() {
        let offered = |navmesh: bool, folders: bool| {
            let mut app = App::new();
            let folder = folders.then(|| std::env::temp_dir().join("eq-prebuild-offer"));
            app.insert_resource(crate::ViewerSettings(crate::ViewerConfig {
                navmesh,
                eq_directory: folder.clone(),
                settings_directory: folder,
                ..crate::ViewerConfig::default()
            }))
            .add_systems(Startup, offer);
            app.update();
            app.world().contains_resource::<NavmeshPrebuild>()
        };
        assert!(offered(true, true));
        assert!(!offered(false, true));
        assert!(!offered(true, false));
    }

    #[test]
    fn the_login_screen_counts_the_kept_zones_then_builds_the_rest_in_turn() {
        let zones = Fake::new(&["qeynos", "ecommons", "thurgadina"], &["qeynos"], &[]);
        let mut app = app(Arc::clone(&zones));
        assert_eq!(line(&app), "Navmeshes: counting zones");
        until(&mut app, idle);
        assert_eq!(line(&app), "Navmeshes: 1 / 3 zones");
        let prebuild = app.world().resource::<NavmeshPrebuild>();
        assert_eq!(
            (prebuild.label(), prebuild.usable()),
            ("Build navmeshes", true)
        );
        *zones.held.lock().unwrap() = true;
        press(&mut app);
        assert_eq!(app.world().resource::<NavmeshPrebuild>().label(), "Stop");
        *zones.held.lock().unwrap() = false;
        until(&mut app, idle);
        assert_eq!(line(&app), "Navmeshes: 3 / 3 zones");
        // Only the missing zones were built, and none is left to press for.
        assert_eq!(zones.tried.load(Ordering::Relaxed), 2);
        assert!(!app.world().resource::<NavmeshPrebuild>().usable());
    }

    #[test]
    fn a_zone_that_fails_is_counted_and_the_rest_go_on() {
        let zones = Fake::new(&["a", "b", "c"], &[], &["b"]);
        let mut app = app(Arc::clone(&zones));
        until(&mut app, idle);
        press(&mut app);
        until(&mut app, idle);
        assert_eq!(line(&app), "Navmeshes: 2 / 3 zones, 1 failed");
        // The failed zone is tried again on the next press.
        assert!(app.world().resource::<NavmeshPrebuild>().usable());
    }

    #[test]
    fn stop_keeps_the_finished_zones_and_the_next_press_carries_on() {
        let zones = Fake::new(&["a", "b", "c"], &[], &[]);
        let mut app = app(Arc::clone(&zones));
        until(&mut app, idle);
        *zones.held.lock().unwrap() = true;
        press(&mut app);
        until(&mut app, |prebuild| {
            prebuild.line().contains("(building a)")
        });
        assert_eq!(line(&app), "Navmeshes: 0 / 3 zones (building a)");
        press(&mut app);
        until(&mut app, idle);
        // Stopped mid-zone, it kept nothing of it and tried no other.
        assert_eq!(line(&app), "Navmeshes: 0 / 3 zones");
        assert_eq!(zones.tried.load(Ordering::Relaxed), 1);
        *zones.held.lock().unwrap() = false;
        press(&mut app);
        until(&mut app, idle);
        assert_eq!(line(&app), "Navmeshes: 3 / 3 zones");
    }

    #[test]
    fn a_world_connection_stops_the_batch() {
        let zones = Fake::new(&["a", "b"], &[], &[]);
        let mut app = app(Arc::clone(&zones));
        until(&mut app, idle);
        *zones.held.lock().unwrap() = true;
        press(&mut app);
        until(&mut app, |prebuild| prebuild.line().contains("building"));
        // The offline viewer stands for a zone being shown.
        app.insert_resource(OnlineState::new(false));
        until(&mut app, idle);
        assert_eq!(line(&app), "Navmeshes: 0 / 2 zones");
    }

    #[test]
    fn the_installed_zones_are_the_tables_with_an_archive_in_zone_order() {
        let folder =
            std::env::temp_dir().join(format!("eq-client-installed-zones-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for file in [
            "ecommons.s3d",
            "qeynos.s3d",
            "ecommons_obj.s3d",
            "nowhere.s3d",
        ] {
            std::fs::write(folder.join(file), b"").unwrap();
        }
        // Qeynos is zone 1, East Commons 22; an archive the table doesn't
        // name is no zone.
        assert_eq!(installed_zones(&folder), ["qeynos", "ecommons"]);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}

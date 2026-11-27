use super::*;
use crate::hud::hotbar::carry::pointer_tests::{self, Fixture};
use bevy::ecs::system::RunSystemOnce;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "eq-hotbar-script-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("Scripts")).unwrap();
        Self(path)
    }

    fn script(&self, text: &str) -> PathBuf {
        let path = self.0.join("Scripts").join("A saved test.eqscript");
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_registry_and_all_page_bindings_round_trip_together() {
    let path = PathBuf::from("C:/A game/Scripts/名前 = \"test\".eqscript");
    let mut bindings = Bindings::default();
    let binding = bindings.script_binding(path.clone()).unwrap();
    assert_eq!(bindings.script_binding(path.clone()).unwrap(), binding);
    let other = bindings
        .script_binding(PathBuf::from("C:/B/Scripts/test.eqscript"))
        .unwrap();
    assert_ne!(binding, other);
    bindings.0[0] = Some(binding);
    bindings.select(9);
    bindings.0[4] = Some(other);
    let text = bindings.text();
    let mut restored = Bindings::default();
    restored.load(&eq_client_core::hotbar::Pages::read(
        &text,
        eq_client_core::hotbar::Pages::default(),
    ));
    restored.2 = Files::read(&text);
    assert_eq!(restored.0[0], Some(binding));
    let Action::Script(id) = binding else {
        panic!("script")
    };
    assert_eq!(restored.script_path(id), Some(path.as_path()));
    restored.select(9);
    assert_eq!(restored.0[4], Some(other));
    assert!(
        Files::read("script_path_0 = \"bad.eqscript\"\nscript_path_4 = 123\n")
            .0
            .is_empty()
    );
}

fn setup(fixture: &mut Fixture, directory: &Directory, source: &str) -> Action {
    Program::parse(source, Path::new(".")).expect("valid lifecycle fixture");
    let app = &mut fixture.app;
    app.insert_resource(Time::<Real>::new(Instant::now()))
        .init_resource::<crate::windows::Moves>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_message::<bevy::input::keyboard::KeyboardFocusLost>()
        .add_message::<bevy::input::mouse::MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<bevy::input::mouse::MouseMotion>()
        .add_message::<bevy::window::WindowFocused>()
        .add_message::<AppExit>();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .eq_directory = Some(directory.0.clone());
    app.world_mut()
        .run_system_once(crate::profile_files::follow)
        .unwrap();
    crate::install_script(
        app,
        (None, None),
        true,
        false,
        crate::script::InputMode::Attended,
    );
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(bevy::window::WindowFocused {
        window,
        focused: true,
    });
    app.add_systems(
        Update,
        (input, super::super::presentation, presentation)
            .chain()
            .after(super::super::carry::route),
    );
    let mut bindings = app.world_mut().resource_mut::<Bindings>();
    let binding = bindings.script_binding(directory.script(source)).unwrap();
    bindings.0[0] = Some(binding);
    binding
}

fn tick(app: &mut App, ms: u64) {
    let mut time = app.world_mut().resource_mut::<Time<Real>>();
    let startup = time.startup();
    time.update_with_instant(startup + Duration::from_millis(ms));
    app.update();
}

fn start(fixture: &mut Fixture) {
    fixture.press(0);
    fixture.release(0);
    let until = Instant::now() + Duration::from_secs(5);
    while !fixture.app.world().contains_resource::<Script>() {
        assert!(Instant::now() < until, "saved file did not start");
        fixture.app.update();
        std::thread::yield_now();
    }
}

fn right(fixture: &mut Fixture) {
    fixture.pointer(Vec2::new(30.0, 60.0), None);
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    fixture.app.update();
    let mut mouse = fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>();
    mouse.release(MouseButton::Right);
    mouse.clear();
}

fn view(app: &App, binding: Action) -> View {
    let Action::Script(id) = binding else {
        panic!("script")
    };
    app.world().resource::<Runs>().view(
        id,
        app.world().resource::<Profile>(),
        app.world().get_resource::<Script>(),
    )
}

/// Both normal and skin factories route real clicks to the actual executor.
#[allow(
    clippy::too_many_lines,
    reason = "One lifecycle contract shared by both real hotbar factories"
)]
pub(crate) fn run_controls(mut fixture: Fixture) {
    let directory = Directory::new();
    let binding = setup(
        &mut fixture,
        &directory,
        "key_down(\"W\")\nwait(100)\nkey_up(\"W\")\nwait(500)\n",
    );
    let button = fixture
        .app
        .world_mut()
        .query::<(Entity, &Slot)>()
        .iter(fixture.app.world())
        .find_map(|(entity, slot)| (slot.0 == 0).then_some(entity))
        .unwrap();
    let previous_outline = Outline::new(px(3), px(1), Color::srgb_u8(200, 30, 40));
    fixture
        .app
        .world_mut()
        .entity_mut(button)
        .insert(previous_outline);
    start(&mut fixture);
    tick(&mut fixture.app, 10);
    tick(&mut fixture.app, 20);
    assert!(
        fixture
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    let identity = fixture
        .app
        .world()
        .resource::<Script>()
        .status()
        .run()
        .clone();
    assert_eq!(view(&fixture.app, binding), View::Running);
    assert_eq!(
        fixture.app.world().get::<Outline>(button).unwrap().color,
        Color::srgb(0.3, 0.85, 0.45)
    );
    fixture.press(0);
    fixture.release(0);
    tick(&mut fixture.app, 200);
    assert_eq!(view(&fixture.app, binding), View::Paused);
    assert_eq!(
        fixture.app.world().get::<Outline>(button).unwrap().color,
        Color::srgb(1.0, 0.72, 0.25)
    );
    assert!(
        !fixture
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert!(
        fixture
            .app
            .world_mut()
            .query::<&Text>()
            .iter(fixture.app.world())
            .any(|text| text.0.starts_with("Paused\n"))
    );
    let progress = fixture.app.world().resource::<Script>().status().progress();
    crate::keys::testing::bind(
        &mut fixture.app,
        crate::keys::Act::Slot(0),
        crate::keys::Chord::key(KeyCode::F6),
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F6);
    fixture.app.update();
    fixture
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset(KeyCode::F6);
    tick(&mut fixture.app, 210);
    let script = fixture.app.world().resource::<Script>();
    assert!(Arc::ptr_eq(&identity, script.status().run()));
    assert!(script.status().progress().0 >= progress.0);
    assert_eq!(view(&fixture.app, binding), View::Running);
    right(&mut fixture);
    tick(&mut fixture.app, 220);
    assert_eq!(view(&fixture.app, binding), View::Idle);
    assert_eq!(
        fixture.app.world().get::<Outline>(button),
        Some(&previous_outline)
    );
    assert_eq!(fixture.binding(0), Some(binding));
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<crate::script::RunStatus>()
            .phase(),
        Phase::Stopped
    );
    directory.script("wait(1)\n");
    start(&mut fixture);
    assert!(!Arc::ptr_eq(
        &identity,
        fixture.app.world().resource::<Script>().status().run()
    ));
    assert_eq!(
        fixture.app.world().resource::<Script>().status().source(),
        "wait(1)\n"
    );
}

#[test]
fn ordinary_hotbuttons_control_the_runner_and_keep_the_binding() {
    run_controls(pointer_tests::ordinary());
}

#[test]
fn completion_and_runtime_failure_clear_the_latch() {
    for (source, phase) in [
        ("wait(1)\n", Phase::Completed),
        ("wait(10)\nvalue: int = 1 / 0\n", Phase::Failed),
    ] {
        let directory = Directory::new();
        let mut fixture = pointer_tests::ordinary();
        let binding = setup(&mut fixture, &directory, source);
        start(&mut fixture);
        for ms in (10..=500).step_by(10) {
            tick(&mut fixture.app, ms);
            if !fixture.app.world().contains_resource::<Script>() {
                break;
            }
        }
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<crate::script::RunStatus>()
                .phase(),
            phase
        );
        assert_eq!(view(&fixture.app, binding), View::Idle);
        assert_eq!(fixture.binding(0), Some(binding));
    }
}

#[test]
fn a_stale_right_click_cannot_stop_a_replacement_run() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    let binding = setup(&mut fixture, &directory, "wait(5000)\n");
    start(&mut fixture);
    right(&mut fixture);
    let foreign = Script::for_viewer(
        Program::parse("wait(5000)\n", Path::new(".")).unwrap(),
        &crate::ViewerConfig::default(),
        true,
    );
    let identity = foreign.status().run().clone();
    fixture.app.insert_resource(foreign);
    tick(&mut fixture.app, 10);
    assert!(Arc::ptr_eq(
        &identity,
        fixture.app.world().resource::<Script>().status().run()
    ));
    assert_eq!(view(&fixture.app, binding), View::Idle);
    assert!(
        fixture
            .app
            .world()
            .resource::<Script>()
            .status()
            .phase()
            .active()
    );
    right(&mut fixture);
    fixture.press(0);
    fixture.release(0);
    assert_eq!(fixture.app.world().resource::<RunControl>().pending(), None);
    assert!(Arc::ptr_eq(
        &identity,
        fixture.app.world().resource::<Script>().status().run()
    ));
}

#[test]
fn script_shortcuts_keep_the_normal_pickup_swap_and_page_rules() {
    let mut fixture = pointer_tests::ordinary();
    let binding = fixture
        .app
        .world_mut()
        .resource_mut::<Bindings>()
        .script_binding(PathBuf::from("C:/game/Scripts/example.eqscript"))
        .unwrap();
    fixture.app.world_mut().resource_mut::<Bindings>().0[0] = Some(binding);
    fixture.press(0);
    fixture.hold();
    assert_eq!(fixture.carry(), Some(binding));
    fixture.release(1);
    assert_eq!(fixture.binding(1), Some(binding));
    assert_eq!(fixture.carry(), Some(Action::Gem(1)));
    fixture.app.world_mut().resource_mut::<Bindings>().select(1);
    fixture.app.update();
    assert_eq!(fixture.carry(), None);
    fixture.app.world_mut().resource_mut::<Bindings>().select(0);
    assert_eq!(fixture.binding(1), Some(binding));
}

fn enter(app: &mut App, name: &str) {
    let mut online = crate::online::OnlineState::new(true);
    let mut player = crate::online::testing::player(1);
    player.name = name.into();
    crate::online::testing::admit(&mut online, 1, player);
    crate::online::testing::news(
        &mut online,
        [eq_client_core::WorldEvent::WorldName {
            short_name: "FixtureWorld".into(),
        }],
    );
    app.insert_resource(online);
}

#[test]
fn saved_paths_stay_with_the_character_that_bound_them() {
    let directory = Directory::new();
    let path = directory.script("wait(1)\n");
    let mut app = crate::testing::app();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .settings_directory = Some(directory.0.clone());
    app.add_systems(
        Update,
        (
            crate::profile_files::follow,
            super::super::prepare,
            super::super::persist,
        )
            .chain(),
    );
    enter(&mut app, "First");
    app.update();
    let binding = app
        .world_mut()
        .resource_mut::<Bindings>()
        .script_binding(path.clone())
        .unwrap();
    app.world_mut().resource_mut::<Bindings>().0[0] = Some(binding);
    app.update();
    enter(&mut app, "Second");
    app.update();
    assert!(app.world().resource::<Bindings>().2.0.is_empty());
    enter(&mut app, "First");
    app.update();
    let bindings = app.world().resource::<Bindings>();
    assert_eq!(bindings.0[0], Some(binding));
    let Action::Script(id) = binding else {
        panic!("script")
    };
    assert_eq!(bindings.script_path(id), Some(path.as_path()));
}

#[test]
fn external_stop_releases_held_keys_and_clears_the_button() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    let binding = setup(&mut fixture, &directory, "key_down(\"W\")\nwait(5000)\n");
    start(&mut fixture);
    tick(&mut fixture.app, 10);
    tick(&mut fixture.app, 20);
    assert!(
        fixture
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<RunControl>()
        .request(Request::Stop);
    tick(&mut fixture.app, 30);
    assert!(
        !fixture
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert_eq!(view(&fixture.app, binding), View::Idle);
    assert_eq!(fixture.binding(0), Some(binding));
}

#[test]
fn a_file_load_cannot_replace_a_run_started_elsewhere() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    setup(&mut fixture, &directory, "wait(5000)\n");
    fixture.press(0);
    fixture.release(0);
    assert!(fixture.app.world().resource::<Runs>().loading.is_some());
    let foreign = Script::for_viewer(
        Program::parse("wait(5000)\n", Path::new(".")).unwrap(),
        &crate::ViewerConfig::default(),
        true,
    );
    let identity = foreign.status().run().clone();
    fixture.app.insert_resource(foreign);
    let until = Instant::now() + Duration::from_secs(5);
    while fixture.app.world().resource::<Runs>().loading.is_some() {
        assert!(Instant::now() < until);
        fixture.app.update();
        std::thread::yield_now();
    }
    assert!(Arc::ptr_eq(
        &identity,
        fixture.app.world().resource::<Script>().status().run()
    ));
    assert!(fixture.app.world().resource::<Runs>().owner.is_none());
}

#[test]
fn right_click_stops_a_paused_script_without_unbinding() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    let binding = setup(&mut fixture, &directory, "wait(100)\nwait(10000)\n");
    start(&mut fixture);
    tick(&mut fixture.app, 10);
    fixture.press(0);
    fixture.release(0);
    tick(&mut fixture.app, 200);
    assert_eq!(view(&fixture.app, binding), View::Paused);
    right(&mut fixture);
    tick(&mut fixture.app, 210);
    assert_eq!(view(&fixture.app, binding), View::Idle);
    assert_eq!(fixture.binding(0), Some(binding));
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<crate::script::RunStatus>()
            .phase(),
        Phase::Stopped
    );
}

#[test]
fn profile_change_cancels_a_pending_file_start() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    setup(&mut fixture, &directory, "wait(5000)\n");
    fixture.press(0);
    fixture.release(0);
    let mut runs = fixture.app.world_mut().resource_mut::<Runs>();
    let generation = runs.generation;
    runs.job.keep_pending(generation);
    enter(&mut fixture.app, "DifferentCharacter");
    fixture.app.update();
    assert!(fixture.app.world().resource::<Runs>().loading.is_none());
    assert!(!fixture.app.world().contains_resource::<Script>());
}

#[test]
fn a_second_press_while_loading_pauses_that_run_at_its_first_boundary() {
    let directory = Directory::new();
    let mut fixture = pointer_tests::ordinary();
    let source = "wait(10)\nwait(1000)\n";
    let binding = setup(&mut fixture, &directory, source);
    fixture.press(0);
    fixture.release(0);
    let mut runs = fixture.app.world_mut().resource_mut::<Runs>();
    let generation = runs.generation;
    runs.job.keep_pending(generation);
    fixture.press(0);
    fixture.release(0);
    let program = Program::parse(source, Path::new(".")).unwrap();
    fixture
        .app
        .world_mut()
        .resource_mut::<Runs>()
        .job
        .start(generation, move || Ok(program));
    let until = Instant::now() + Duration::from_secs(5);
    while !fixture.app.world().contains_resource::<Script>() {
        assert!(Instant::now() < until);
        fixture.app.update();
        std::thread::yield_now();
    }
    for ms in (10..=200).step_by(10) {
        tick(&mut fixture.app, ms);
        if view(&fixture.app, binding) == View::Paused {
            break;
        }
    }
    assert_eq!(view(&fixture.app, binding), View::Paused);
}

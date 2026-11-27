//! Real chat editing and walk reduction, with no window backend or network.
use super::*;
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};
use bevy::window::PrimaryWindow;

fn app() -> (App, Entity) {
    let mut app = pending_walk();
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.init_resource::<crate::navigation::NavigationKeys>();
    app.add_systems(
        Update,
        (
            crate::chat::input,
            crate::chat::capture,
            crate::navigation::update,
            crate::script::interrupt_walk,
        )
            .chain(),
    );
    frame_at(&mut app, 0);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Running
    );
    (app, window)
}

fn key(app: &mut App, window: Entity, code: KeyCode, logical: Key, text: Option<&str>, down: bool) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: logical,
        state: if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: text.map(Into::into),
        repeat: false,
        window,
    });
}
fn tap(app: &mut App, window: Entity, code: KeyCode, logical: Key, text: Option<&str>) {
    key(app, window, code, logical.clone(), text, true);
    key(app, window, code, logical, None, false);
}
fn active(app: &App) {
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Running
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one ordered native chat, UI and route completion scenario"
)]
fn script_walk_6721_typed_who_ui_click_and_camera_activity_continue_to_reported_arrival() {
    let (mut app, window) = app();
    let mesh = NavMesh::build(
        &floor(),
        &eq_client_core::movement::navmesh::Settings::walker(6.),
        1,
        &AtomicBool::new(false),
    )
    .unwrap()
    .0;
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(Some(mesh));
    let started = Instant::now();
    let mut now = 1;
    loop {
        frame_at(&mut app, now);
        active(&app);
        if app.world().resource::<Script>().acts.contains(&FORWARD) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        now += 1;
        std::thread::yield_now();
    }

    tap(&mut app, window, KeyCode::Enter, Key::Enter, None);
    frame_at(&mut app, now + 10);
    active(&app);
    assert!(app.world().resource::<crate::keys::Typing>().composing);
    // W is a movement binding, but this native W belongs to the chat draft.
    for (code, text) in [
        (KeyCode::Slash, "/"),
        (KeyCode::KeyW, "w"),
        (KeyCode::KeyH, "h"),
        (KeyCode::KeyO, "o"),
    ] {
        tap(
            &mut app,
            window,
            code,
            Key::Character(text.into()),
            Some(text),
        );
    }
    frame_at(&mut app, now + 20);
    active(&app);
    assert_eq!(
        app.world().resource::<crate::chat::ChatState>().draft(),
        "/who"
    );
    tap(&mut app, window, KeyCode::Enter, Key::Enter, None);
    frame_at(&mut app, now + 30);
    active(&app);
    assert!(!app.world().resource::<crate::keys::Typing>().composing);
    assert!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .zone_who
            .is_some()
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert!(!app.world().resource::<Script>().background_owns_input());
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
    app.world_mut()
        .write_message(bevy::input::mouse::MouseMotion {
            delta: Vec2::new(5., 3.),
        });
    frame_at(&mut app, now + 40);
    active(&app);
    // A text arrow remains consumed even when Enter closes chat in its batch.
    tap(&mut app, window, KeyCode::Enter, Key::Enter, None);
    tap(&mut app, window, KeyCode::ArrowUp, Key::ArrowUp, None);
    tap(&mut app, window, KeyCode::Enter, Key::Enter, None);
    frame_at(&mut app, now + 50);
    active(&app);
    let goal = app
        .world()
        .resource::<Script>()
        .place
        .as_ref()
        .unwrap()
        .feet;
    testing::place(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        eq_client_core::world_position((goal + Vec3::Y * 3.75).to_array(), 0.),
    );
    let returned = Instant::now();
    let mut frame = now + 60;
    while app.world().contains_resource::<Script>() {
        frame_at(&mut app, frame);
        assert!(
            returned.elapsed() < Duration::from_secs(30),
            "walk remained active: {:?}",
            app.world()
                .get_resource::<Script>()
                .map(|s| s.status().events())
        );
        frame += 1;
        std::thread::yield_now();
    }
    assert_eq!(
        app.world().resource::<crate::script::RunStatus>().phase(),
        Phase::Completed,
        "{:?}",
        app.world().resource::<crate::script::RunStatus>().events()
    );
}

#[test]
fn script_walk_6721_forward_and_remapped_movement_interrupt_without_eating_native_hold() {
    for code in [KeyCode::KeyW, KeyCode::F12] {
        let (mut app, window) = app();
        crate::keys::testing::bind(
            &mut app,
            crate::keys::Act::Forward,
            crate::keys::Chord::key(code),
        );
        key(
            &mut app,
            window,
            code,
            Key::Character("synthetic movement".into()),
            None,
            true,
        );
        frame_at(&mut app, 10);
        assert!(!app.world().contains_resource::<Script>());
        assert_eq!(
            app.world().resource::<crate::script::RunStatus>().phase(),
            Phase::Failed
        );
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(code));
    }
}

#[test]
fn script_walk_6721_captured_character_steering_interrupts_but_ui_buttons_do_not() {
    let (mut app, window) = app();
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Right,
            state: ButtonState::Pressed,
            window,
        });
    frame_at(&mut app, 10);
    active(&app);
    // The camera owns this classification only after the world hit test; raw
    // buttons over UI never acquire the character-steering state.
    app.world_mut()
        .resource_mut::<crate::camera::MouseLook>()
        .held = true;
    frame_at(&mut app, 20);
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(
        app.world().resource::<crate::script::RunStatus>().phase(),
        Phase::Failed
    );
}

fn install_detachable_camera(app: &mut App) -> Entity {
    let camera = app
        .world_mut()
        .spawn((
            crate::OrbitCamera {
                focus: Vec3::ZERO,
                radius: 30.,
                yaw: 0.,
                pitch: 0.,
                pulled: None,
                view: default(),
                look: 0.,
            },
            Transform::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::FreeCamera, true);
    crate::keys::testing::bind(
        app,
        crate::keys::Act::FreeCamera,
        crate::keys::Chord::key(KeyCode::F12),
    );
    app.add_systems(
        Update,
        crate::camera::toggle_free
            .after(crate::chat::capture)
            .before(crate::script::interrupt_walk),
    );
    camera
}

#[test]
fn script_walk_6721_detached_camera_releases_input_keeps_route_and_replans_on_return() {
    let (mut app, window) = app();
    let scope = Scope::capture(app.world().resource::<OnlineState>()).unwrap();
    {
        let mut script = app.world_mut().resource_mut::<Script>();
        script.place = Some(PlaceGoal {
            scope,
            feet: Vec3::new(0., 0., 20.),
        });
        script.route = Some(Route::new(
            Vec3::ZERO,
            Vec3::new(0., 0., 20.),
            1.,
            6.677_319_5,
        ));
        script.acts = vec![FORWARD];
    }
    let camera = install_detachable_camera(&mut app);
    tap(
        &mut app,
        window,
        KeyCode::F12,
        Key::Character("camera".into()),
        None,
    );
    frame_at(&mut app, 10);
    active(&app);
    assert!(
        app.world()
            .get::<crate::camera::FreeCamera>(camera)
            .is_some()
    );
    frame_at(&mut app, 20);
    active(&app);
    let script = app.world().resource::<Script>();
    assert!(script.route.is_some(), "detaching retains route ownership");
    assert_eq!(script.acts, [] as [crate::keys::Act; 0]);
    assert_eq!(script.facing, None);
    let paused_since = script.walk_camera_pause.unwrap();
    for ms in [100, 500, 25_000] {
        frame_at(&mut app, ms);
        active(&app);
        let script = app.world().resource::<Script>();
        assert_eq!(script.walk_camera_pause, Some(paused_since));
        assert_eq!(script.acts, [] as [crate::keys::Act; 0]);
        assert!(script.route.is_some());
    }
    let current = eq_client_core::world_position([5., 3.75, 7.], 64.);
    testing::place(&mut app.world_mut().resource_mut::<OnlineState>(), current);
    tap(
        &mut app,
        window,
        KeyCode::F12,
        Key::Character("camera".into()),
        None,
    );
    frame_at(&mut app, 25_010);
    active(&app);
    assert!(
        app.world()
            .get::<crate::camera::FreeCamera>(camera)
            .is_none()
    );
    frame_at(&mut app, 25_020);
    active(&app);
    let script = app.world().resource::<Script>();
    assert_eq!(script.walk_camera_pause, None);
    assert!(
        script.route.is_none(),
        "pending mesh holds the fresh plan, not the old route"
    );
    assert_eq!(script.acts, [] as [crate::keys::Act; 0]);
    let since = script.current.as_ref().unwrap().1;
    assert!(Duration::from_millis(25_020).saturating_sub(since) < Duration::from_secs(1));
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .player()
            .unwrap()
            .position,
        current
    );
}

//! Production script ownership, hit testing and ordinary attached camera path.
use super::*;
use crate::camera::{MouseAim, MouseLook};
use bevy::input::mouse::{MouseButtonInput, MouseMotion};

fn fixture(steps: Vec<Step>) -> (App, Entity, Entity) {
    let (mut app, window) = background(steps);
    app.world_mut().resource_mut::<Script>().offline = true;
    crate::systems::configure_stages(&mut app);
    app.init_resource::<crate::navigation::NavigationKeys>()
        .add_systems(
            PreUpdate,
            crate::camera::look
                .after(input::observe)
                .before(bevy::ui::UiSystems::Focus),
        )
        .add_systems(
            Update,
            crate::windows::pointer::wheel.in_set(crate::Stage::Typing),
        )
        .add_systems(Update, crate::camera::turn.in_set(crate::Stage::Input))
        .add_systems(
            Update,
            (crate::camera::look_keys, crate::orbit_camera)
                .chain()
                .in_set(crate::Stage::Present),
        );
    app.world_mut().spawn((
        crate::Player,
        Transform::default(),
        crate::PlayerBody {
            feet_offset: 3.0,
            height: 6.0,
            size: 6.0,
        },
    ));
    let camera = app
        .world_mut()
        .spawn((
            crate::OrbitCamera {
                focus: Vec3::ZERO,
                radius: 30.0,
                yaw: 0.0,
                pitch: crate::camera::CHASE_PITCH,
                pulled: None,
                view: default(),
                look: 0.0,
            },
            Transform::default(),
        ))
        .id();
    (app, window, camera)
}

fn look(duration: u64) -> Step {
    Step::WorldLook {
        delta: Vec2::new(120.0, -80.0),
        duration: Duration::from_millis(duration),
    }
}

fn frame(app: &mut App, ms: u64) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(10));
    frame_at(app, ms);
}

fn released(app: &App) {
    assert!(!app.world().resource::<MouseLook>().held);
    assert!(!app.world().resource::<WorldInput>().active);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
    assert!(app.world().resource::<MouseAim>().horizontal.is_none());
}

#[test]
fn explicit_key_hold_spans_wait_and_world_rmb_look_until_key_up() {
    let (mut app, _, camera) = fixture(vec![
        Step::KeyDown(vec![KeyCode::KeyA]),
        Step::Wait(Duration::from_millis(30)),
        look(100),
        Step::Wait(Duration::from_millis(30)),
        Step::KeyUp(vec![KeyCode::KeyA]),
        Step::Wait(Duration::from_millis(50)),
    ]);
    let mut saw_look = false;
    let mut saw_wait_after_look = false;
    for ms in (0..=400).step_by(10) {
        frame(&mut app, ms);
        if app.world().resource::<WorldInput>().held {
            saw_look = true;
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::KeyA)
            );
        }
        if saw_look
            && let Some(script) = app.world().get_resource::<Script>()
            && matches!(script.current, Some((Step::Wait(_), _)))
            && script.latched.contains(&KeyCode::KeyA)
        {
            saw_wait_after_look = true;
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::KeyA)
            );
        }
    }
    assert!(saw_look && saw_wait_after_look);
    assert!(
        app.world()
            .get::<crate::OrbitCamera>(camera)
            .unwrap()
            .yaw
            .abs()
            > 0.0
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    released(&app);
}

#[test]
fn explicit_holds_release_on_completion_stop_pause_and_native_takeover_error() {
    for ending in 0..4 {
        let mut steps = vec![
            Step::KeyDown(vec![KeyCode::KeyA]),
            look(if matches!(ending, 0 | 3) { 30 } else { 1000 }),
        ];
        if ending == 3 {
            // Pause at the next action; the last action instead completes.
            steps.push(Step::Wait(Duration::from_secs(1)));
        }
        let (mut app, window, _) = fixture(steps);
        for ms in [0, 10, 20] {
            frame(&mut app, ms);
        }
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        if ending == 1 {
            app.world_mut()
                .resource_mut::<RunControl>()
                .request(Request::Stop);
        } else if ending == 3 {
            app.world_mut()
                .resource_mut::<RunControl>()
                .request(Request::Pause);
        } else if ending == 2 {
            set_focus(&mut app, window, true);
            app.world_mut().write_message(MouseMotion {
                delta: Vec2::new(50.0, 0.0),
            });
        }
        for ms in (30..=200).step_by(10) {
            frame(&mut app, ms);
        }
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        released(&app);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            match ending {
                0 => Phase::Completed,
                1 => Phase::Stopped,
                3 => Phase::Paused,
                _ => Phase::Failed,
            }
        );
    }
}

#[test]
fn explicit_hold_cleanup_preserves_a_native_hold_of_the_same_key() {
    let (mut app, window, _) = fixture(vec![
        Step::KeyDown(vec![KeyCode::KeyA]),
        Step::Wait(Duration::from_secs(1)),
    ]);
    frame(&mut app, 0);
    set_focus(&mut app, window, true);
    native_key(&mut app, window, KeyCode::KeyA, true);
    frame(&mut app, 10);
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    assert!(
        app.world()
            .resource::<SeenKeys>()
            .0
            .iter()
            .all(|event| event.state.is_pressed())
    );
}

#[test]
fn explicit_hold_survives_same_key_transient_steps_and_repeated_key_down() {
    let (mut app, _, _) = fixture(vec![
        Step::KeyDown(vec![KeyCode::KeyA]),
        Step::KeyDown(vec![KeyCode::KeyA]),
        Step::Press(vec![KeyCode::KeyA]),
        Step::Hold(vec![KeyCode::KeyA], Duration::from_millis(30)),
        Step::Wait(Duration::from_millis(30)),
        Step::KeyUp(vec![KeyCode::KeyA]),
        Step::Wait(Duration::from_millis(30)),
    ]);
    for ms in (0..=250).step_by(10) {
        frame(&mut app, ms);
        if app
            .world()
            .get_resource::<Script>()
            .is_some_and(|script| script.latched.contains(&KeyCode::KeyA))
        {
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::KeyA)
            );
        }
    }
    let events = &app.world().resource::<SeenKeys>().0;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.key_code == KeyCode::KeyA && event.state.is_pressed())
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.key_code == KeyCode::KeyA && !event.state.is_pressed())
            .count(),
        1
    );
}

#[test]
fn wheel_enters_first_person_and_right_look_holds_off_spring_until_release() {
    let (mut app, _, camera) = fixture(vec![
        Step::WorldWheel(64.0),
        look(100),
        Step::Wait(Duration::from_secs(1)),
    ]);
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .look_spring = true;
    frame(&mut app, 0);
    assert_eq!(
        app.world()
            .get::<crate::OrbitCamera>(camera)
            .unwrap()
            .view_name(),
        "first_person"
    );
    frame(&mut app, 10);
    frame(&mut app, 60);
    let halfway = app
        .world()
        .get::<crate::OrbitCamera>(camera)
        .unwrap()
        .view_pitch();
    frame(&mut app, 110);
    let camera_state = app.world().get::<crate::OrbitCamera>(camera).unwrap();
    assert!(halfway > 0.0 && camera_state.view_pitch() > halfway);
    assert!(camera_state.yaw.abs() > 0.0);
    assert!(app.world().resource::<MouseLook>().held);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
    frame(&mut app, 120);
    released(&app);
    for ms in (130..=500).step_by(10) {
        frame(&mut app, ms);
    }
    assert!(
        app.world()
            .get::<crate::OrbitCamera>(camera)
            .unwrap()
            .view_pitch()
            .abs()
            < 0.0001
    );
}

#[test]
fn wheel_respects_zoom_option_and_outward_wheel_returns_to_chase() {
    for enabled in [false, true] {
        let (mut app, _, camera) = fixture(vec![
            Step::WorldWheel(64.0),
            Step::WorldWheel(-1.0),
            Step::Wait(Duration::from_secs(1)),
        ]);
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .wheel_zoom = enabled;
        frame(&mut app, 0);
        assert_eq!(
            app.world()
                .get::<crate::OrbitCamera>(camera)
                .unwrap()
                .view_name(),
            if enabled { "first_person" } else { "chase" }
        );
        frame(&mut app, 10);
        assert_eq!(
            app.world()
                .get::<crate::OrbitCamera>(camera)
                .unwrap()
                .view_name(),
            "chase"
        );
        frame(&mut app, 20);
        released(&app);
    }
}

#[test]
fn native_motion_reclaims_only_a_focused_client_world_gesture() {
    for focused in [false, true] {
        let (mut app, window, _) = fixture(vec![look(1000)]);
        frame(&mut app, 0);
        set_focus(&mut app, window, focused);
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(500.0, 100.0),
        });
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(200.0, 50.0),
        });
        frame(&mut app, 10);
        if focused {
            assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
            released(&app);
        } else {
            assert!(app.world().resource::<MouseLook>().held);
            assert!((app.world().resource::<WorldInput>().delta.x - 1.2).abs() < 0.0001);
            set_focus(&mut app, window, true);
            frame(&mut app, 20);
            assert!(app.world().resource::<MouseLook>().held);
            assert!(app.world().contains_resource::<Script>());
        }
    }
}

#[test]
fn native_right_press_survives_script_look_cancellation() {
    let (mut app, window, _) = fixture(vec![look(1000)]);
    frame(&mut app, 0);
    set_focus(&mut app, window, true);
    app.world_mut().write_message(MouseButtonInput {
        window,
        button: MouseButton::Right,
        state: bevy::input::ButtonState::Pressed,
    });
    frame(&mut app, 10);
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    assert!(!app.world().resource::<WorldInput>().active);
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
    assert!(app.world().resource::<MouseLook>().held);
}

#[test]
fn stop_and_pause_boundaries_release_world_look() {
    for request in [Request::Stop, Request::Pause] {
        let (mut app, _, _) = fixture(vec![look(100), Step::Press(vec![KeyCode::KeyA])]);
        frame(&mut app, 0);
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(request);
        frame(&mut app, 100);
        frame(&mut app, 110);
        released(&app);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        assert!(matches!(
            app.world().resource::<RunStatus>().phase(),
            Phase::Stopped | Phase::Paused
        ));
    }
}

#[test]
fn a_gesture_binds_its_own_admission_and_releases_on_transfer_or_session_end() {
    use crate::online::testing as session;
    use eq_client_core::world::Link;
    for link in [Link::Zoning, Link::Ended] {
        let (mut app, _, _) = fixture(vec![look(1000)]);
        {
            let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
            online.enabled = true;
            session::admit(&mut online, 7, session::player(9));
        }
        frame(&mut app, 0);
        assert!(app.world().resource::<MouseLook>().held);
        session::link(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            link,
        );
        frame(&mut app, 10);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        released(&app);
    }
}

#[test]
fn fully_covered_world_and_attended_gestures_are_refused() {
    for covered in [false, true] {
        let (mut app, _, _) = fixture(vec![look(1000)]);
        if covered {
            app.world_mut().spawn((
                crate::windows::Frame::default(),
                ComputedNode {
                    size: Vec2::splat(100_000.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ));
        } else {
            let mut script = app.world_mut().resource_mut::<Script>();
            script.input_mode = InputMode::Attended;
        }
        frame(&mut app, 0);
        frame(&mut app, 10);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        released(&app);
    }
}

#[test]
fn a_stale_expected_world_refuses_before_gesture_activation() {
    let (mut app, _, _) = fixture(vec![look(1000)]);
    let expected = {
        let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
        online.enabled = true;
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(9));
        let mut expected = ExpectedWorld::current(&online).unwrap();
        expected.admission_id += 1;
        expected
    };
    app.world_mut()
        .resource_mut::<Script>()
        .progress
        .expected_world = Some(expected);
    frame(&mut app, 0);
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    released(&app);
}

#[test]
fn received_admission_change_withdraws_a_same_frame_gesture_before_camera_input() {
    let (mut app, _, camera) = fixture(vec![Step::WorldWheel(64.0)]);
    {
        let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
        online.enabled = true;
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(9));
    }
    app.add_systems(
        Update,
        (|mut online: ResMut<crate::online::OnlineState>| {
            crate::online::testing::admit(&mut online, 8, crate::online::testing::player(9));
        })
        .in_set(crate::Stage::Receive)
        .before(guard_scope),
    );
    frame(&mut app, 0);
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    assert_eq!(
        app.world()
            .get::<crate::OrbitCamera>(camera)
            .unwrap()
            .view_name(),
        "chase"
    );
    released(&app);
}

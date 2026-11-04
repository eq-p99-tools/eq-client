//! Synthetic control requests through the production receiver and driver.
use super::*;
use crate::online::testing as session;
use std::{fs, time::Instant};

fn app() -> App {
    let mut app = crate::testing::app();
    app.insert_resource(Time::<Real>::new(Instant::now()))
        .init_resource::<crate::windows::Moves>()
        .init_resource::<RunStatus>()
        .init_resource::<RunControl>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_message::<bevy::window::WindowFocused>()
        .add_message::<AppExit>()
        .insert_resource(Bridge::new(
            PathBuf::from("synthetic-control"),
            "test-1".into(),
        ))
        .add_systems(
            PreUpdate,
            (receive, super::super::drive, observe)
                .chain()
                .before(crate::clicks::track),
        );
    app
}

fn frame(app: &mut App, ms: u64) {
    let mut clock = app.world_mut().resource_mut::<Time<Real>>();
    let startup = clock.startup();
    clock.update_with_instant(startup + Duration::from_millis(ms));
    app.update();
}

/// Public-session input permissions with the normal backend message systems,
/// but no real window or network connection. Control starts without a CLI run.
fn background_app() -> (App, Entity) {
    let mut app = app();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .script_input_mode = super::super::InputMode::Background;
    app.world_mut()
        .resource_mut::<crate::online::OnlineState>()
        .enabled = true;
    app.add_message::<bevy::input::keyboard::KeyboardFocusLost>()
        .add_message::<bevy::input::mouse::MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(
            PreUpdate,
            (
                bevy::input::keyboard::keyboard_input_system,
                bevy::input::mouse::mouse_button_input_system,
            )
                .in_set(bevy::input::InputSystems),
        );
    super::super::install_input(&mut app, super::super::InputMode::Background);
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    assert!(!app.world().contains_resource::<Script>());
    (app, window)
}

#[test]
fn background_control_reports_native_ownership_wait_before_taking_input() {
    let (mut app, window) = background_app();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: bevy::input::keyboard::Key::Character("w".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("w".into()),
            repeat: false,
            window,
        });
    assert_eq!(start(&mut app, 1, "press A\n").outcome, "accepted");
    let status: Value =
        serde_json::from_slice(&app.world().resource::<Bridge>().snapshot()).unwrap();
    assert_eq!(status["run"]["phase"], "waiting_for_input");
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    assert_eq!(
        request(&mut app, 2, json!({"op":"stop","run_id":1})).outcome,
        "stop_requested"
    );
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(
        *app.world().resource::<super::super::InputMode>(),
        super::super::InputMode::Background
    );
}

#[test]
fn background_control_can_be_passively_watched_through_selection_and_report() {
    for (focused, pointer) in [(true, false), (false, true), (true, true)] {
        let (mut app, window) = background_app();
        {
            let mut window = app.world_mut().get_mut::<Window>(window).unwrap();
            window.focused = focused;
            if pointer {
                window.set_cursor_position(Some(Vec2::new(10.0, 10.0)));
            }
        }
        assert_eq!(
            start(&mut app, 1, "wait_select\nreport observed\n").outcome,
            "accepted"
        );
        let status: Value =
            serde_json::from_slice(&app.world().resource::<Bridge>().snapshot()).unwrap();
        assert_eq!(status["run"]["phase"], "running");
        assert!(matches!(
            app.world().resource::<Script>().current,
            Some((super::super::Step::WaitSelect, _))
        ));
        app.world_mut()
            .resource_mut::<crate::online::OnlineState>()
            .selection = Some(crate::character_select::Selection::new(1, Vec::new()));
        for ms in [10, 20, 30, 40, 50] {
            frame(&mut app, ms);
        }
        assert!(!app.world().contains_resource::<Script>());
        assert_eq!(
            app.world()
                .resource::<Bridge>()
                .history
                .back()
                .unwrap()
                .phase,
            Phase::Completed
        );
    }
}

#[test]
fn explicit_control_recovery_preserves_background_mode_without_replaying_failed_input() {
    let (mut app, _) = background_app();
    assert_eq!(
        *app.world().resource::<super::super::InputMode>(),
        super::super::InputMode::Background
    );
    assert_eq!(start(&mut app, 1, "hold W 1000\npress B\n").run_id, Some(1));
    let failed_identity = app.world().resource::<Script>().status().identity.clone();
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    frame(&mut app, 900_001);
    assert!(!app.world().contains_resource::<Script>());
    let failed = app.world().resource::<Bridge>().history.back().unwrap();
    assert_eq!((failed.id, failed.phase), (1, Phase::Failed));
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyB)
    );

    assert_eq!(start(&mut app, 2, "press A\n").run_id, Some(2));
    let recovered = app.world().resource::<Script>();
    assert_eq!(recovered.input_mode, super::super::InputMode::Background);
    assert!(!recovered.local && !recovered.offline);
    assert!(!Arc::ptr_eq(&failed_identity, &recovered.status().identity));
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    frame(&mut app, 900_011);
    frame(&mut app, 900_021);
    assert!(!app.world().contains_resource::<Script>());
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyB)
    );
    let bridge = app.world().resource::<Bridge>();
    assert!(
        bridge
            .history
            .iter()
            .any(|run| run.id == 1 && run.phase == Phase::Failed)
    );
    assert_eq!(bridge.history.back().unwrap().phase, Phase::Completed);
    assert_eq!(
        *app.world().resource::<super::super::InputMode>(),
        super::super::InputMode::Background
    );
}

fn request(app: &mut App, id: u64, fields: Value) -> Receipt {
    let Value::Object(fields) = fields else {
        panic!("control request fields must be an object");
    };
    let mut value = json!({"version":1,"instance":"test-1","id":id});
    value.as_object_mut().unwrap().extend(fields);
    let bytes = serde_json::to_vec(&value).unwrap();
    {
        let mut bridge = app.world_mut().resource_mut::<Bridge>();
        assert!(!bridge.job.pending(Some(0)));
        bridge.receipt = None;
        bridge.job.start(0, move || Ok(Some(bytes)));
    }
    let ms = u64::try_from(app.world().resource::<Time<Real>>().elapsed().as_millis()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        frame(app, ms);
        if let Some(receipt) = &app.world().resource::<Bridge>().receipt {
            return receipt.clone();
        }
        assert!(Instant::now() < deadline, "control request did not settle");
        std::thread::yield_now();
    }
}

fn start(app: &mut App, id: u64, source: &str) -> Receipt {
    request(app, id, json!({"op":"start","source":source}))
}

#[test]
fn control_is_absent_by_default_and_requires_exclusive_new_directory() {
    let mut app = crate::testing::app();
    install(&mut app).unwrap();
    assert!(!app.world().contains_resource::<Bridge>());
    let root = temporary();
    fs::create_dir(&root).unwrap();
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .script_control = Some(root.clone());
    assert_eq!(
        install(&mut app).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert!(!root.join("status.json").exists());
    clean(&root);
}

#[test]
fn duplicate_and_rejected_requests_cannot_start_another_run_after_retirement() {
    let mut app = app();
    let first = start(&mut app, 1, "wait 100\n");
    assert_eq!((first.outcome, first.run_id), ("accepted", Some(1)));
    let duplicate = start(&mut app, 1, "wait 100\n");
    assert_eq!(duplicate, first);
    assert_eq!(start(&mut app, 1, "press A\n").code, "id_conflict");
    assert_eq!(start(&mut app, 2, "press A\n").code, "busy");
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    frame(&mut app, 100);
    frame(&mut app, 101);
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 2, "press A\n").code, "busy");
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 1, "wait 100\n").code, "stale_id");
    assert_eq!(start(&mut app, 3, "wait 1\n").run_id, Some(2));
}

#[test]
fn invalid_source_keeps_its_line_and_does_not_consume_a_run_identity() {
    let mut app = app();
    let receipt = start(&mut app, 1, "# source\n\nno_such_command\n");
    assert_eq!(receipt.code, "invalid_program");
    assert_eq!(receipt.line, Some(3));
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 2, "# empty\n").code, "invalid_program");
    assert_eq!(start(&mut app, 3, "wait 1\n").run_id, Some(1));
}

#[test]
fn wrong_instance_requests_cannot_consume_ids_or_stop_the_current_run() {
    let mut app = app();
    start(&mut app, 1, "wait 100\n");
    let response = request(
        &mut app,
        99,
        json!({"instance":"old-client","op":"stop","run_id":1}),
    );
    assert_eq!(response.code, "wrong_instance");
    assert_eq!(
        app.world()
            .resource::<Bridge>()
            .processed
            .as_ref()
            .unwrap()
            .0
            .id,
        1
    );
    assert!(app.world().contains_resource::<Script>());
    assert_eq!(
        request(&mut app, 2, json!({"op":"status"})).outcome,
        "observed"
    );
    assert_eq!(
        app.world()
            .resource::<Bridge>()
            .current
            .as_ref()
            .unwrap()
            .snapshot
            .id,
        1
    );
}

#[test]
fn an_explicitly_paused_run_still_owns_the_executor_until_stop() {
    let mut app = app();
    start(&mut app, 1, "wait 1\nwait 100\n");
    app.world_mut()
        .resource_mut::<RunControl>()
        .request(Request::Pause);
    frame(&mut app, 1);
    frame(&mut app, 2);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Paused
    );
    assert_eq!(start(&mut app, 2, "press A\n").code, "busy");
    request(&mut app, 3, json!({"op":"stop","run_id":1}));
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Stopped);
}

#[test]
fn the_existing_local_command_and_program_limits_are_preserved() {
    let mut app = app();
    for (id, source) in [(1, "gm level 1\n"), (2, "chat say synthetic\n")] {
        let receipt = start(&mut app, id, source);
        assert_eq!(receipt.code, "invalid_program");
        assert_eq!(receipt.line, Some(1));
        assert!(!app.world().contains_resource::<Script>());
    }
    assert_eq!(
        start(&mut app, 3, &"wait 1\n".repeat(501)).code,
        "invalid_program"
    );
    assert_eq!(
        start(
            &mut app,
            4,
            &format!("#{}", "x".repeat(super::super::MAX_SOURCE_BYTES))
        )
        .code,
        "invalid_program"
    );
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .local_session = true;
    let program = prepare("gm level 1\n", std::path::Path::new("."), true).unwrap();
    let runner = Script::for_viewer(
        program,
        &app.world().resource::<crate::ViewerSettings>().0,
        true,
    );
    assert!(runner.local && !runner.offline);
}

#[test]
fn explicit_stop_releases_held_input_cancels_the_click_and_never_starts_rejected_gameplay() {
    let mut app = app();
    let first = start(&mut app, 1, "hold W 1000\npress A\n");
    assert_eq!(first.run_id, Some(1));
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    let button = app
        .world_mut()
        .spawn((
            Node::default(),
            Interaction::Pressed,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    {
        let mut script = app.world_mut().resource_mut::<Script>();
        script.clicked = Some(MouseButton::Left);
        script.pressed = Some(button);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    frame(&mut app, 1);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert_eq!(start(&mut app, 2, "press A\n").code, "busy");
    let stop = request(&mut app, 3, json!({"op":"stop","run_id":1}));
    assert_eq!(stop.outcome, "stop_requested");
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Stopped);
    assert!(!app.world().contains_resource::<Script>());
    let keys = app.world().resource::<ButtonInput<KeyCode>>();
    assert!(!keys.pressed(KeyCode::KeyW) && !keys.pressed(KeyCode::KeyA));
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert_eq!(
        app.world().get::<Interaction>(button),
        Some(&Interaction::None)
    );
    assert!(
        !app.world()
            .resource::<crate::clicks::LeftButton>()
            .clicked(button)
    );
    frame(&mut app, 1001);
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 4, "wait 100\n").run_id, Some(2));
    assert_eq!(
        request(&mut app, 5, json!({"op":"stop","run_id":1})).code,
        "run_changed"
    );
    assert!(app.world().contains_resource::<Script>());
}

#[test]
fn runtime_failure_retires_and_an_explicit_recovery_preserves_the_failed_source_line() {
    let mut app = app();
    assert_eq!(
        start(&mut app, 1, "# requires target\n\nface\n").outcome,
        "accepted"
    );
    frame(&mut app, 1);
    let bridge = app.world().resource::<Bridge>();
    let failed = bridge.history.back().unwrap();
    assert_eq!(
        (failed.id, failed.phase, failed.line),
        (1, Phase::Failed, Some(3))
    );
    assert!(failed.message.as_ref().unwrap().contains("target"));
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 2, "wait 10\n").run_id, Some(2));
    assert_eq!(
        app.world().resource::<Bridge>().history.back().unwrap().id,
        1
    );
}

#[test]
fn runtime_limit_and_session_end_require_distinct_new_runs_without_client_exit() {
    for ended in [false, true] {
        let mut app = app();
        if ended {
            let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
            online.enabled = true;
            session::admit(&mut online, 1, session::player(7));
            app.world_mut()
                .resource_mut::<crate::ViewerSettings>()
                .0
                .local_session = true;
        }
        start(&mut app, 1, "wait 1000\n");
        if ended {
            session::link(
                &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                eq_client_core::world::Link::Ended,
            );
            frame(&mut app, 1);
        } else {
            frame(&mut app, 900_001);
        }
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        assert!(!app.world().contains_resource::<Script>());
        let bridge = app.world().resource::<Bridge>();
        let reason = bridge.history.back().unwrap().message.as_deref().unwrap();
        assert!(reason.contains(if ended {
            "Session ended"
        } else {
            "maximum runtime"
        }));
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        assert_eq!(start(&mut app, 2, "wait 100\n").run_id, Some(2));
    }
}

#[test]
fn a_failed_camp_wait_remains_an_error_and_never_reaches_quit() {
    let mut app = app();
    // No character list arrives. The existing wait must fail before Quit.
    start(&mut app, 1, "wait_select\nquit\n");
    frame(&mut app, 180_001);
    frame(&mut app, 180_002);
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
    assert_eq!(app.world().resource::<RunStatus>().line(), Some(1));
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    assert!(!app.world().contains_resource::<Script>());
    assert_eq!(start(&mut app, 2, "wait 100\n").outcome, "accepted");
}

#[test]
fn a_followed_run_still_accepts_complete_appended_lines_but_rejects_replacement() {
    let root = temporary();
    fs::create_dir(&root).unwrap();
    let source = root.join("follow.eqscript");
    fs::write(&source, b"wait 1\n").unwrap();
    let mut app = app();
    let program = Program::parse("wait 1\n", &root).unwrap();
    app.insert_resource(
        Script::from_program(program)
            .offline_preview(true)
            .with_follow(source.clone(), 7),
    );
    frame(&mut app, 0);
    frame(&mut app, 1);
    frame(&mut app, 2);
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::Following
    );
    let followed_id = app
        .world()
        .resource::<Bridge>()
        .current
        .as_ref()
        .unwrap()
        .snapshot
        .id;
    assert_eq!(start(&mut app, 1, "press A\n").code, "busy");
    fs::write(&source, b"wait 1\nwait 100\nwait ").unwrap();
    frame(&mut app, 252);
    assert_eq!(
        app.world()
            .resource::<Bridge>()
            .current
            .as_ref()
            .unwrap()
            .snapshot
            .id,
        followed_id
    );
    assert!(app.world().resource::<Bridge>().history.is_empty());
    assert_eq!(app.world().resource::<Script>().status().progress().1, 2);
    assert_eq!(
        app.world()
            .resource::<Bridge>()
            .current
            .as_ref()
            .unwrap()
            .snapshot
            .origin,
        "other"
    );
    let run_id = app
        .world()
        .resource::<Bridge>()
        .current
        .as_ref()
        .unwrap()
        .snapshot
        .id;
    assert_eq!(
        request(&mut app, 2, json!({"op":"stop","run_id":run_id})).outcome,
        "stop_requested"
    );
    assert_eq!(start(&mut app, 3, "wait 1\n").outcome, "accepted");
    clean(&root);
}

#[test]
fn public_control_runs_use_the_existing_focus_gate_and_stop_without_focus() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<crate::online::OnlineState>()
        .enabled = true;
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    start(&mut app, 1, "press W\n");
    assert_eq!(
        app.world().resource::<Script>().status().phase(),
        Phase::WaitingForFocus
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    request(&mut app, 2, json!({"op":"stop","run_id":1}));
    assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Stopped);
}

#[test]
fn status_history_is_bounded_and_does_not_serialize_script_source() {
    let mut bridge = Bridge::new("synthetic".into(), "instance".into());
    for _ in 0..20 {
        let mut status = Script::from_program(
            Program::parse("wait 1\n# private source marker", std::path::Path::new(".")).unwrap(),
        )
        .progress;
        bridge.bind(&status, "control").unwrap();
        status.finish(Phase::Failed, "synthetic failure");
        bridge.observe(&status, false);
    }
    let bytes = bridge.snapshot();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["recent_runs"].as_array().unwrap().len(), HISTORY);
    assert_eq!(value["recent_runs"][0]["id"], 5);
    assert_eq!(value["run"]["id"], 20);
    assert_eq!(value["run"]["phase"], "failed");
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("private source marker")
    );
}

#[test]
fn file_control_publishes_receipts_and_shutdown_without_claiming_run_completion() {
    let root = temporary();
    files::prepare(&root).unwrap();
    let mut app = app();
    app.insert_resource(Bridge::new(root.clone(), "test-1".into()))
        .add_systems(Last, publish);
    fs::write(
        root.join("request.pending"),
        serde_json::to_vec(&json!({
            "version":1,"instance":"test-1","id":1,"op":"start","source":"wait 120000\n"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::rename(root.join("request.pending"), root.join("request.json")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ms = 0;
    loop {
        frame(&mut app, ms);
        ms += 1;
        if fs::read(root.join("status.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| {
                value["last_request"]["id"] == 1 && value["last_request"]["outcome"] == "accepted"
            })
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    drop(app);
    let value: Value =
        serde_json::from_slice(&fs::read(root.join("status.json")).unwrap()).unwrap();
    assert_eq!(value["client_state"], "closed");
    assert_eq!(value["run"]["phase"], "failed");
    assert_eq!(
        value["run"]["message"],
        "Client closed before run completed"
    );
    assert_eq!(value["last_request"]["outcome"], "accepted");
    clean(&root);
}

fn temporary() -> PathBuf {
    std::env::temp_dir().join(format!(
        "eq-control-scenario-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn repeated_program_snapshots_have_distinct_run_identity_and_terminal_outcomes() {
    let program = Program::parse("wait 1\n", std::path::Path::new(".")).unwrap();
    let mut first = Script::from_program(program.clone());
    let second = Script::from_program(program);
    assert!(Arc::ptr_eq(&first.progress.source, &second.progress.source));
    assert!(!Arc::ptr_eq(
        &first.progress.identity,
        &second.progress.identity
    ));
    let mut bridge = Bridge::new("synthetic".into(), "instance".into());
    bridge.observe(first.status(), true);
    first.progress.finish(Phase::Completed, "Script finished");
    bridge.observe(first.status(), false);
    bridge.observe(second.status(), true);
    assert_eq!(bridge.current.as_ref().unwrap().snapshot.id, 2);
    assert_eq!(
        (bridge.history[0].id, bridge.history[0].phase),
        (1, Phase::Completed)
    );
}

fn clean(path: &std::path::Path) {
    assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
    assert!(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("eq-control-scenario-")
    );
    fs::remove_dir_all(path).unwrap();
}

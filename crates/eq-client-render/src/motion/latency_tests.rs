//! Offline frame-loop timing. Uses the production Receive/Input/Present stage edges.

use super::*;
use std::sync::{Mutex, mpsc};

pub(super) fn key(app: &mut App, code: KeyCode, pressed: bool) {
    let world = app.world_mut();
    let window = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .unwrap();
    world.write_message(bevy::input::keyboard::KeyboardInput {
        key_code: code,
        logical_key: bevy::input::keyboard::Key::Character("movement-probe".into()),
        text: None,
        state: if pressed {
            bevy::input::ButtonState::Pressed
        } else {
            bevy::input::ButtonState::Released
        },
        repeat: false,
        window,
    });
}

pub(super) fn timing_app() -> (
    App,
    mpsc::Receiver<ClientCommand>,
    mpsc::Sender<eq_client_core::WorldUpdate>,
) {
    let (mut app, requests) = super::tests::fixture();
    app.add_plugins(bevy::input::InputPlugin).add_systems(
        PreUpdate,
        crate::keys::presses::capture.after(bevy::input::InputSystems),
    );
    crate::systems::init_presentation(&mut app);
    // This fixture begins with the already-applied default policy.
    app.world_mut()
        .remove_resource::<crate::options::MovementSync>();
    for (act, key) in [
        (Act::CameraForward, KeyCode::KeyW),
        (Act::CameraBack, KeyCode::KeyS),
        (Act::CameraLeft, KeyCode::KeyA),
        (Act::CameraRight, KeyCode::KeyD),
    ] {
        crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
    }
    let (news, updates) = mpsc::channel();
    app.insert_resource(online::Updates(Mutex::new(Some(updates))))
        .add_message::<bevy::input::mouse::MouseMotion>()
        .init_resource::<crate::spellbook::SpellNames>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<Time>();
    crate::systems::configure_stages(&mut app);
    app.add_systems(Update, crate::systems::receive_world())
        .add_systems(Update, input.in_set(crate::systems::Stage::Input))
        .add_systems(
            Update,
            (interpolate, crate::camera::orbit_camera)
                .chain()
                .in_set(crate::systems::Stage::Present),
        );
    let world = app.world_mut();
    let root = world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap()
        .translation;
    let camera = world
        .query_filtered::<Entity, With<OrbitCamera>>()
        .single(world)
        .unwrap();
    world.entity_mut(camera).insert(Transform::default());
    world
        .entity_mut(camera)
        .get_mut::<OrbitCamera>()
        .unwrap()
        .focus = root;
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    (app, requests, news)
}

pub(super) fn shown(app: &mut App) -> (Vec3, Vec3) {
    let world = app.world_mut();
    let root = world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap()
        .translation;
    let focus = world.query::<&OrbitCamera>().single(world).unwrap().focus;
    (root, focus)
}

fn frame(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
}

fn next_move(requests: &mpsc::Receiver<ClientCommand>) -> MovementRequest {
    std::iter::from_fn(|| requests.try_recv().ok())
        .find_map(|command| {
            if let ClientCommand::Move(request) = command {
                Some(request)
            } else {
                None
            }
        })
        .expect("one movement request")
}

#[test]
fn a_cold_key_moves_the_root_and_follow_camera_before_the_worker_answers() {
    for code in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
        let (mut app, requests, news) = timing_app();
        frame(&mut app);
        let before = shown(&mut app).0;
        let basis = app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position;
        key(&mut app, code, true);
        frame(&mut app);
        let request = next_move(&requests);
        let (root, focus) = shown(&mut app);
        let endpoint = Vec3::from_array(eq_client_core::render_position(request.position));
        assert!(
            root.distance(before) > 0.001,
            "{code:?} has no first-frame step"
        );
        assert!(root.distance(before) <= endpoint.distance(before) + 0.001);
        assert_eq!(root, focus, "camera did not follow the first step");
        assert_eq!(
            app.world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position,
            basis
        );
        assert!(app.world().resource::<Controls>().waiting());
        news.send(eq_client_core::WorldUpdate::Game(
            eq_client_core::WorldEvent::MotionSent {
                session_id: request.session_id,
                created: request.created,
                position: request.position,
                refused: None,
            },
        ))
        .unwrap();
        frame(&mut app);
        let controls = app.world().resource::<Controls>();
        assert!(!controls.waiting());
        assert!(controls.preview.is_none());
        assert!(
            controls.visual.as_ref().unwrap().queue.is_empty(),
            "acceptance replayed preview distance"
        );
        assert!(
            requests.try_recv().is_err(),
            "acceptance bypassed the ordinary sample gate"
        );
    }
}

#[test]
fn the_accepted_only_negative_control_waits_for_the_worker() {
    let (mut app, requests, news) = timing_app();
    app.world_mut()
        .resource_mut::<Controls>()
        .suppress_submitted_preview = true;
    frame(&mut app);
    let before = shown(&mut app).0;
    key(&mut app, KeyCode::KeyW, true);
    frame(&mut app);
    let request = next_move(&requests);
    assert_eq!(shown(&mut app).0, before);
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: request.position,
            refused: None,
        },
    ))
    .unwrap();
    frame(&mut app);
    assert!(shown(&mut app).0.distance(before) > 0.001);
}

#[test]
fn a_refused_preview_returns_to_the_observed_pose_in_the_receive_frame() {
    let (mut app, requests, news) = timing_app();
    frame(&mut app);
    let basis = app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let before = shown(&mut app).0;
    key(&mut app, KeyCode::KeyW, true);
    frame(&mut app);
    let request = next_move(&requests);
    assert!(shown(&mut app).0.distance(before) > 0.001);
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: basis,
            refused: Some("local guard refusal".into()),
        },
    ))
    .unwrap();
    frame(&mut app);
    assert_eq!(shown(&mut app), (before, before));
    assert!(app.world().resource::<Controls>().preview.is_none());
}

#[test]
fn a_full_command_queue_never_previews_the_unsent_endpoint() {
    let (mut app, _requests, _news) = timing_app();
    let (sender, _requests) = mpsc::sync_channel(0);
    app.insert_resource(crate::outbox::Outbox::new(Some(sender)));
    frame(&mut app);
    let before = shown(&mut app).0;
    key(&mut app, KeyCode::KeyW, true);
    frame(&mut app);
    assert_eq!(shown(&mut app).0, before);
    assert!(app.world().resource::<Controls>().preview.is_none());
}

#[test]
fn the_first_dry_step_after_disembark_waits_until_the_display_frame_is_world_space() {
    let (mut app, requests, news) = timing_app();
    frame(&mut app);
    let before = shown(&mut app).0;
    let mut carriers = eq_client_core::movement::Carriers::default();
    carriers.observe(
        &eq_client_core::WorldEvent::Spawns(vec![eq_client_core::SpawnState {
            race: 72,
            gender: 2,
            ..online::testing::pet(8, 0)
        }]),
        eq_client_core::GameDialect::Titanium,
        Instant::now(),
    );
    app.world_mut().resource_mut::<Controls>().visual_boat = Some(boats::DisplayFrame {
        sample: carriers.latest(8).unwrap().sample(),
        pose: eq_client_core::WorldPosition {
            x: 100.0,
            heading: 128.0,
            ..default()
        },
    });
    key(&mut app, KeyCode::KeyW, true);
    frame(&mut app);
    let request = next_move(&requests);
    assert!(app.world().resource::<Controls>().preview.is_none());
    assert_eq!(shown(&mut app).0, before);
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: request.position,
            refused: None,
        },
    ))
    .unwrap();
    frame(&mut app);
    let endpoint = Vec3::from_array(eq_client_core::render_position(request.position));
    let drawn = shown(&mut app).0;
    assert!(drawn.distance(before) > 0.001);
    assert!(drawn.distance(before) <= endpoint.distance(before) + 0.001);
    assert!(app.world().resource::<Controls>().visual_boat.is_none());
}

/// Real wall-clock pacing, no manual aging of pending requests or acceptance clocks.
/// The transport double returns each local worker result 50 ms after submission.
#[test]
#[ignore = "bounded offline timing probe; writes its rows to stdout"]
fn frame_loop_movement_latency_probe() {
    for preview in [false, true] {
        for (trial, release_ms, repress_ms, rerelease_ms) in [
            ("ordinary", 320, u64::MAX, u64::MAX),
            ("rapid_restart", 40, 120, 200),
        ] {
            for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
                let (mut app, requests, news) = timing_app();
                // Keep this historical #701 comparison independent of T1.
                app.world_mut()
                    .resource_mut::<Controls>()
                    .suppress_release_catchup = true;
                app.world_mut()
                    .resource_mut::<Controls>()
                    .suppress_submitted_preview = !preview;
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_micros(16_667));
                app.update();
                let idle = shown(&mut app);
                std::thread::sleep(Duration::from_micros(16_667));
                let start = Instant::now();
                let mut previous = start.checked_sub(Duration::from_micros(16_667)).unwrap();
                let mut due = Vec::<(Instant, MovementRequest)>::new();
                let mut submitted = Vec::new();
                let mut rows = Vec::new();
                let mut released = false;
                let mut repressed = false;
                let mut rereleased = false;
                self::key(&mut app, key, true);
                loop {
                    let now = Instant::now();
                    if now.duration_since(start) >= Duration::from_millis(release_ms) && !released {
                        self::key(&mut app, key, false);
                        released = true;
                    }
                    if now.duration_since(start) >= Duration::from_millis(repress_ms) && !repressed
                    {
                        self::key(&mut app, key, true);
                        repressed = true;
                    }
                    if now.duration_since(start) >= Duration::from_millis(rerelease_ms)
                        && !rereleased
                    {
                        self::key(&mut app, key, false);
                        rereleased = true;
                    }
                    for (_, request) in due.extract_if(.., |(at, _)| *at <= now) {
                        news.send(eq_client_core::WorldUpdate::Game(
                            eq_client_core::WorldEvent::MotionSent {
                                session_id: request.session_id,
                                created: request.created,
                                position: request.position,
                                refused: None,
                            },
                        ))
                        .unwrap();
                    }
                    app.world_mut()
                        .resource_mut::<Time>()
                        .advance_by(now.duration_since(previous).max(Duration::from_micros(1)));
                    previous = now;
                    app.update();
                    while let Ok(command) = requests.try_recv() {
                        if let ClientCommand::Move(request) = command {
                            submitted
                                .push(request.created.duration_since(start).as_secs_f64() * 1000.0);
                            due.push((Instant::now() + Duration::from_millis(50), request));
                        }
                    }
                    let (root, focus) = shown(&mut app);
                    rows.push(serde_json::json!({"frame":rows.len(),"input_ms":now.duration_since(start).as_secs_f64()*1000.0,"observed_ms":Instant::now().duration_since(start).as_secs_f64()*1000.0,"released":released,"repressed":repressed,"rereleased":rereleased,"root":root.to_array(),"focus":focus.to_array(),"waiting":app.world().resource::<Controls>().waiting()}));
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .clear();
                    if now.duration_since(start) >= Duration::from_millis(700) {
                        break;
                    }
                    std::thread::sleep(Duration::from_micros(16_667));
                }
                println!(
                    "LATENCY {}",
                    serde_json::json!({"preview":preview,"trial":trial,"key":format!("{key:?}"),"idle_root":idle.0.to_array(),"idle_focus":idle.1.to_array(),"submitted_ms":submitted,"rows":rows})
                );
            }
        }
    }
}

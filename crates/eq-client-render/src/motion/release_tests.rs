//! Deterministic production-stage replay: only the display policy differs.
use super::{
    latency_tests::{key, shown, timing_app},
    *,
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug)]
enum Input {
    Key(KeyCode),
    Autorun,
    MouseForward,
    Turn,
    Opposite,
    Reverse,
    Restart(usize),
}

struct Replay {
    commands: Vec<String>,
    move_positions: Vec<eq_client_core::WorldPosition>,
    positions: Vec<Vec3>,
    rotations: Vec<Quat>,
    camera_angles: Vec<(f32, f32)>,
    stop: usize,
    used_playhead: bool,
}

fn clock(app: &mut App, now: Instant) {
    app.world_mut().resource_mut::<Controls>().test_now = Some(now);
    app.world_mut()
        .resource_mut::<crate::outbox::Outbox>()
        .set_test_time(now);
}

fn tick(app: &mut App, base: Instant, frame: u64) {
    clock(app, base + Duration::from_micros(frame * 16_667));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
}

fn mouse(app: &mut App, held: bool) {
    app.insert_resource(MouseLook { held, left: held });
}

fn action(app: &mut App, input: Input, frame: usize) {
    match input {
        Input::Key(code) => {
            if frame == 5 {
                key(app, code, true);
            }
            if frame == 24 {
                key(app, code, false);
            }
        }
        Input::Autorun => {
            // Distinct press edges switch the same ordinary autorun latch.
            for start in [5, 24] {
                if frame == start {
                    key(app, KeyCode::KeyR, true);
                }
                if frame == start + 1 {
                    key(app, KeyCode::KeyR, false);
                }
            }
        }
        Input::MouseForward => {
            if frame == 5 {
                mouse(app, true);
            }
            if frame == 24 {
                mouse(app, false);
            }
        }
        Input::Turn => {
            for code in [KeyCode::KeyW, KeyCode::KeyE] {
                if frame == 5 {
                    key(app, code, true);
                }
                if frame == 24 {
                    key(app, code, false);
                }
            }
        }
        Input::Opposite => {
            for code in [KeyCode::KeyW, KeyCode::KeyS] {
                if frame == 5 {
                    key(app, code, true);
                }
                if frame == 24 {
                    key(app, code, false);
                }
            }
        }
        Input::Reverse => {
            if frame == 5 {
                key(app, KeyCode::KeyW, true);
            }
            if frame == 24 {
                key(app, KeyCode::KeyW, false);
                key(app, KeyCode::KeyS, true);
            }
            if frame == 44 {
                key(app, KeyCode::KeyS, false);
            }
        }
        Input::Restart(gap) => {
            if frame == 5 {
                key(app, KeyCode::KeyW, true);
            }
            if frame == 24 {
                key(app, KeyCode::KeyW, false);
            }
            // gap0 deliberately releases and re-presses within one input frame.
            if frame == 24 + gap {
                key(app, KeyCode::KeyW, true);
            }
            if frame == 44 {
                key(app, KeyCode::KeyW, false);
            }
        }
    }
}

fn replay(input: Input, reply_delay: usize, base: Instant, catchup: bool) -> Replay {
    let (mut app, requests, news) = timing_app();
    app.world_mut()
        .resource_mut::<Controls>()
        .suppress_release_catchup = !catchup;
    crate::keys::testing::bind(
        &mut app,
        Act::Autorun,
        crate::keys::Chord::key(KeyCode::KeyR),
    );
    let mut result = Replay {
        commands: vec![],
        move_positions: vec![],
        positions: vec![],
        rotations: vec![],
        camera_angles: vec![],
        stop: if matches!(input, Input::Restart(_) | Input::Reverse) {
            44
        } else {
            24
        },
        used_playhead: false,
    };
    let mut replies: VecDeque<(usize, MovementRequest)> = VecDeque::new();
    for frame in 0..120 {
        let now = base + Duration::from_micros(16_667 * u64::try_from(frame).unwrap());
        clock(&mut app, now);
        while replies.front().is_some_and(|(due, _)| *due <= frame) {
            let (_, request) = replies.pop_front().unwrap();
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
        action(&mut app, input, frame);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_micros(16_667));
        app.update();
        for command in requests.try_iter() {
            // Includes exact stamps, modes, positions, support, vertical speed,
            // climb/posture messages and the final zero-input stop, in order.
            result.commands.push(format!("{command:?}"));
            if let ClientCommand::Move(request) = command {
                result.move_positions.push(request.position);
                replies.push_back((frame + 1 + reply_delay, request));
            }
        }
        result.positions.push(shown(&mut app).0);
        let world = app.world_mut();
        result.rotations.push(
            world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap()
                .rotation,
        );
        let camera = world.query::<&OrbitCamera>().single(world).unwrap();
        result.camera_angles.push((camera.yaw, camera.pitch));
        result.used_playhead |= world.resource::<Controls>().has_translation_playhead();
    }
    assert!(replies.is_empty(), "replay left a reply outstanding");
    if result.move_positions.len() >= 2 {
        let final_moves = &result.move_positions[result.move_positions.len() - 2..];
        assert_eq!(final_moves[0], final_moves[1], "missing zero-input stop");
    }
    result
}

#[test]
fn release_catchup_preserves_the_complete_command_and_heading_timelines() {
    let base = Instant::now() + Duration::from_secs(60);
    for input in [
        Input::Key(KeyCode::KeyW),
        Input::Key(KeyCode::KeyA),
        Input::Key(KeyCode::KeyS),
        Input::Key(KeyCode::KeyD),
        Input::Key(KeyCode::ArrowUp),
        Input::Key(KeyCode::ArrowDown),
        Input::Autorun,
        Input::MouseForward,
        Input::Turn,
        Input::Opposite,
        Input::Reverse,
        Input::Restart(0),
        Input::Restart(1),
        Input::Restart(2),
        Input::Restart(4),
    ] {
        for delay in [0, 1, 2, 6] {
            let before = replay(input, delay, base, false);
            let after = replay(input, delay, base, true);
            assert_eq!(
                before.commands, after.commands,
                "{input:?}, delay{delay}: commands changed"
            );
            assert_eq!(
                before.rotations, after.rotations,
                "{input:?}, delay{delay}: rotation clock changed"
            );
            assert_eq!(
                before.camera_angles, after.camera_angles,
                "{input:?}, delay{delay}: camera heading changed"
            );
            assert_eq!(
                before.positions.last(),
                after.positions.last(),
                "{input:?}, delay{delay}: final endpoint changed"
            );
            if matches!(
                input,
                Input::Key(KeyCode::KeyW) | Input::Autorun | Input::MouseForward
            ) {
                assert!(
                    after.used_playhead,
                    "{input:?}, delay{delay}: catch-up was never exercised"
                );
                let finish_frames = usize::try_from(
                    Duration::from_secs_f32(translation::RELEASE_FINISH_SECONDS)
                        .as_micros()
                        .div_ceil(16_667),
                )
                .unwrap();
                let finished_at = after.stop + finish_frames - 1;
                let end = after.positions[finished_at];
                assert!(
                    after.positions[finished_at..]
                        .iter()
                        .all(|position| position.distance(end) < 0.000_1),
                    "{input:?}, delay{delay}: release restarted or exceeded the finish window"
                );
            }
        }
    }
}

#[test]
fn a_delayed_refusal_rolls_back_a_finished_release_to_the_observed_pose() {
    let base = Instant::now() + Duration::from_secs(60);
    let (mut app, requests, news) = timing_app();
    let original = shown(&mut app).0;
    let position = app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    clock(&mut app, base);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
    key(&mut app, KeyCode::KeyW, true);
    clock(&mut app, base + Duration::from_micros(16_667));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
    let request = requests
        .try_iter()
        .find_map(|command| match command {
            ClientCommand::Move(request) => Some(request),
            _ => None,
        })
        .unwrap();
    key(&mut app, KeyCode::KeyW, false);
    for frame in 2..=5 {
        clock(&mut app, base + Duration::from_micros(16_667 * frame));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_micros(16_667));
        app.update();
    }
    assert!(
        app.world()
            .resource::<Controls>()
            .has_translation_playhead()
    );
    assert!(shown(&mut app).0.distance(original) > 0.001);
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position,
            refused: Some("delayed guard refusal".into()),
        },
    ))
    .unwrap();
    clock(&mut app, base + Duration::from_micros(16_667 * 6));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_micros(16_667));
    app.update();
    assert_eq!(shown(&mut app), (original, original));
    assert!(
        !app.world()
            .resource::<Controls>()
            .has_translation_playhead()
    );
}

#[test]
fn accepted_release_rollback_restores_translation_without_changing_the_rotation_clock() {
    let base = Instant::now() + Duration::from_secs(60);
    for interruption in ["stun", "withdrawal", "placement", "admission"] {
        let mut results = Vec::new();
        for enabled in [false, true] {
            let (mut app, requests, news) = timing_app();
            app.world_mut()
                .resource_mut::<Controls>()
                .suppress_release_catchup = !enabled;
            let mut request = None;
            for frame in 0..=3 {
                clock(&mut app, base + Duration::from_micros(frame * 16_667));
                if frame == 1 {
                    key(&mut app, KeyCode::KeyW, true);
                }
                if frame == 2 {
                    key(&mut app, KeyCode::KeyW, false);
                    let sent: MovementRequest = request.take().unwrap();
                    news.send(eq_client_core::WorldUpdate::Game(
                        eq_client_core::WorldEvent::MotionSent {
                            session_id: sent.session_id,
                            created: sent.created,
                            position: sent.position,
                            refused: None,
                        },
                    ))
                    .unwrap();
                }
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_micros(16_667));
                app.update();
                for command in requests.try_iter() {
                    if let ClientCommand::Move(sent) = command {
                        request = Some(sent);
                    }
                }
            }
            assert!(
                app.world().resource::<Controls>().preview.is_none(),
                "worker acceptance was not received"
            );
            assert_eq!(
                app.world()
                    .resource::<Controls>()
                    .has_translation_playhead(),
                enabled
            );
            clock(&mut app, base + Duration::from_micros(4 * 16_667));
            match interruption {
                "stun" => {
                    news.send(eq_client_core::WorldUpdate::Game(
                        eq_client_core::WorldEvent::OwnStun {
                            session_id: 11,
                            revision: 1,
                            state: eq_client_core::control::StunState::Stunned,
                        },
                    ))
                    .unwrap();
                }
                "withdrawal" => app.world_mut().resource_mut::<Controls>().reset(None),
                "placement" => {
                    news.send(eq_client_core::WorldUpdate::Game(
                        eq_client_core::WorldEvent::Position {
                            spawn_id: 7,
                            position: eq_client_core::world_position([4.0, 3.0, -2.0], 64.0),
                            velocity: [0.0; 3],
                            forward: 0.0,
                        },
                    ))
                    .unwrap();
                }
                "admission" => {
                    let mut player = app
                        .world()
                        .resource::<online::OnlineState>()
                        .world()
                        .player()
                        .unwrap()
                        .clone();
                    player.position = eq_client_core::world_position([7.0, 3.0, 5.0], 0.0);
                    crate::online::testing::admit(
                        &mut app.world_mut().resource_mut::<online::OnlineState>(),
                        12,
                        player,
                    );
                    app.world_mut().resource_mut::<Controls>().reset(None);
                }
                _ => unreachable!(),
            }
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_micros(16_667));
            app.update();
            let world = app.world_mut();
            let transform = *world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .unwrap();
            if enabled {
                let observed = world
                    .resource::<online::OnlineState>()
                    .world()
                    .player()
                    .unwrap()
                    .position;
                assert_eq!(
                    transform.translation,
                    Vec3::from_array(eq_client_core::render_position(observed)),
                    "{interruption}: translation did not roll back exactly"
                );
            }
            let camera = world.query::<&OrbitCamera>().single(world).unwrap();
            results.push((transform.rotation, camera.yaw, camera.pitch));
        }
        assert_eq!(
            results[0], results[1],
            "{interruption}: rollback changed the rotation/camera clock"
        );
    }
}

#[test]
fn a_timed_out_release_restores_authority_and_a_late_answer_is_not_replayed() {
    let base = Instant::now() + Duration::from_secs(60);
    let (mut app, requests, news) = timing_app();
    tick(&mut app, base, 0);
    let original = shown(&mut app).0;
    key(&mut app, KeyCode::KeyW, true);
    tick(&mut app, base, 1);
    let request = requests
        .try_iter()
        .find_map(|command| match command {
            ClientCommand::Move(request) => Some(request),
            _ => None,
        })
        .unwrap();
    let endpoint = Vec3::from_array(eq_client_core::render_position(request.position));
    key(&mut app, KeyCode::KeyW, false);
    for frame in 2..=17 {
        tick(&mut app, base, frame);
    }
    assert_eq!(shown(&mut app), (original, original));
    assert!(
        app.world().resource::<Controls>().waiting(),
        "timeout did not retain command ownership"
    );
    assert!(
        !app.world()
            .resource::<Controls>()
            .has_translation_playhead()
    );
    assert!(
        requests.try_recv().is_err(),
        "release emitted an early stop"
    );
    news.send(eq_client_core::WorldUpdate::Game(
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: request.position,
            refused: None,
        },
    ))
    .unwrap();
    for frame in 18..=40 {
        tick(&mut app, base, frame);
    }
    assert_eq!(shown(&mut app), (endpoint, endpoint));
    assert!(
        requests.try_recv().is_err(),
        "late acceptance duplicated a move"
    );
}

#[test]
fn release_catchup_requires_owned_ground_input_without_a_lift_or_jump() {
    let base = Instant::now() + Duration::from_secs(60);
    for interruption in ["blur", "lift", "jump"] {
        let (mut app, requests, _news) = timing_app();
        tick(&mut app, base, 0);
        key(&mut app, KeyCode::KeyW, true);
        tick(&mut app, base, 1);
        assert!(
            requests
                .try_iter()
                .any(|command| matches!(command, ClientCommand::Move(_)))
        );
        key(&mut app, KeyCode::KeyW, false);
        match interruption {
            "blur" => {
                let world = app.world_mut();
                world
                    .query::<&mut Window>()
                    .single_mut(world)
                    .unwrap()
                    .focused = false;
            }
            "lift" => app.world_mut().resource_mut::<Controls>().riding = Some((1, base, 3.0)),
            "jump" => app.world_mut().resource_mut::<Controls>().jump = Some(base),
            _ => unreachable!(),
        }
        tick(&mut app, base, 2);
        assert!(
            !app.world()
                .resource::<Controls>()
                .has_translation_playhead(),
            "{interruption}: release retimed an ineligible path"
        );
    }
}

/// CPU-side timing only. The unchanged sample gate uses its real wall clock;
/// the worker double answers after50ms. No timing sleeps enter default tests.
#[test]
#[ignore = "bounded private normal-cadence release/repress timing probe"]
fn release_playhead_timing_probe() {
    for enabled in [false, true] {
        for (trial, release, repress, stop) in [
            ("ordinary", 320, u64::MAX, u64::MAX),
            ("restart", 40, 120, 200),
        ] {
            for code in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
                let (mut app, requests, news) = timing_app();
                app.world_mut()
                    .resource_mut::<Controls>()
                    .suppress_release_catchup = !enabled;
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_micros(16_667));
                app.update();
                let idle = shown(&mut app);
                std::thread::sleep(Duration::from_micros(16_667));
                let start = Instant::now();
                let mut prior = start - Duration::from_micros(16_667);
                let mut replies: Vec<(Instant, MovementRequest)> = Vec::new();
                let mut rows = Vec::new();
                let mut commands = Vec::new();
                let (mut released, mut restarted, mut stopped) = (false, false, false);
                key(&mut app, code, true);
                loop {
                    let at = Instant::now();
                    let elapsed = at.duration_since(start);
                    if elapsed >= Duration::from_millis(release) && !released {
                        key(&mut app, code, false);
                        released = true;
                    }
                    if elapsed >= Duration::from_millis(repress) && !restarted {
                        key(&mut app, code, true);
                        restarted = true;
                    }
                    if elapsed >= Duration::from_millis(stop) && !stopped {
                        key(&mut app, code, false);
                        stopped = true;
                    }
                    for (_, request) in replies.extract_if(.., |(due, _)| *due <= at) {
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
                        .advance_by(at.duration_since(prior));
                    prior = at;
                    app.update();
                    let observed = Instant::now();
                    for command in requests.try_iter() {
                        commands.push(serde_json::json!({"observed_ms":observed.duration_since(start).as_secs_f64()*1000.0,"command":format!("{command:?}")}));
                        if let ClientCommand::Move(request) = command {
                            replies.push((observed + Duration::from_millis(50), request));
                        }
                    }
                    let (root, focus) = shown(&mut app);
                    rows.push(serde_json::json!({"frame":rows.len(),"input_ms":elapsed.as_secs_f64()*1000.0,"observed_ms":observed.duration_since(start).as_secs_f64()*1000.0,"released":released,"repressed":restarted,"rereleased":stopped,"root":root.to_array(),"focus":focus.to_array(),"waiting":app.world().resource::<Controls>().waiting()}));
                    if elapsed >= Duration::from_millis(700) {
                        break;
                    }
                    std::thread::sleep(Duration::from_micros(16_667));
                }
                println!(
                    "RELEASE {}",
                    serde_json::json!({"enabled":enabled,"trial":trial,"key":format!("{code:?}"),"idle_root":idle.0.to_array(),"idle_focus":idle.1.to_array(),"commands":commands,"rows":rows})
                );
            }
        }
    }
}

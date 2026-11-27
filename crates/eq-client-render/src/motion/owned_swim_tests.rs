//! Controlled input through production Receive/Input/Present; no gameplay.
use super::*;
use eq_network_game::movement::MotionSession;

#[path = "delayed_reset_tests.rs"]
mod delayed_reset_tests;

fn calibration() -> eq_network_game::movement::MotionCalibration {
    titanium_speeds(&SpeedState {
        run: 0.7,
        walk: 0.46,
        level: 50,
        hit_points: Some((500, 500)),
        rate: 0,
        weight: 0,
        capacity: 100,
        sneaking: false,
        ducking: false,
        zone_id: 1,
        swimming_skill: Some(100),
    })
    .motion
    .unwrap()
}

fn news(f: &Native, event: eq_client_core::WorldEvent) {
    f.news
        .send(eq_client_core::WorldUpdate::Game(event))
        .unwrap();
}

fn frame(f: &mut Native, owner: &mut MotionSession) -> Vec<ClientCommand> {
    let step = Duration::from_micros(16_667);
    f.now += step;
    f.app.world_mut().resource_mut::<Time>().advance_by(step);
    f.app.world_mut().resource_mut::<Controls>().test_now = Some(f.now);
    f.app
        .world_mut()
        .resource_mut::<crate::outbox::Outbox>()
        // Dispatch finishes later than the physical slice. Qualified swim
        // must retain the physical end, without padding its native calls.
        .set_test_time(f.now + Duration::from_millis(5));
    f.app.update();
    f.requests
        .try_iter()
        .filter_map(|command| match command {
            // A real correction also re-observes body climb before its first
            // movement frame. Preserve that ordinary owner input in this replay.
            ClientCommand::ClimbReach { per_step, .. } => {
                owner.set_climb(per_step);
                None
            }
            other => Some(other),
        })
        .collect()
}

fn anchor(f: &Native, owner: &MotionSession) {
    news(
        f,
        eq_client_core::WorldEvent::Position {
            spawn_id: 7,
            position: owner.position(),
            velocity: [0.; 3],
            forward: 0.,
        },
    );
    news(
        f,
        eq_client_core::WorldEvent::SwimState {
            session_id: 11,
            position: owner.position(),
            created: None,
            basis: owner.swim_basis(),
        },
    );
}

fn owner(f: &Native) -> MotionSession {
    let position = f
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let mut owner = MotionSession::new(11, 7, position, f.now)
        .unwrap()
        .with_swimming(true)
        .with_heading_snapshots(true)
        .with_falls(true);
    owner.calibrate(calibration(), f.now).unwrap();
    owner
}

fn settle(f: &Native, owner: &mut MotionSession, command: ClientCommand) -> MovementRequest {
    let ClientCommand::SwimFrame { request, controls } = command else {
        panic!("expected actual ordinary input ledger: {command:?}");
    };
    assert_eq!(request.created, f.now);
    owner
        .inspect_swim_replay(&request, &controls)
        .unwrap_or_else(|error| {
            panic!("native replay: {error}; request={request:?}; controls={controls:?}")
        });
    owner
        .send_swim_sample(&request, Some(&controls), f.now, |_| Ok(()))
        .unwrap();
    assert!(owner.swim_basis().is_some());
    news(
        f,
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: owner.position(),
            refused: None,
        },
    );
    news(
        f,
        eq_client_core::WorldEvent::SwimState {
            session_id: request.session_id,
            created: Some(request.created),
            position: owner.position(),
            basis: owner.swim_basis(),
        },
    );
    request
}

#[test]
fn turn_only_and_no_input_never_create_horizontal_travel() {
    for turn in [false, true] {
        let mut f = Native::new(false, -15.);
        let mut owner = owner(&f);
        anchor(&f, &owner);
        if turn {
            latency_tests::key(&mut f.app, KeyCode::KeyA, true);
        }
        let origin = owner.position();
        let mut count = 0;
        let mut path_xyz = 0_f64;
        for _ in 0..600 {
            for command in frame(&mut f, &mut owner) {
                let before = owner.position();
                let request = settle(&f, &mut owner, command);
                path_xyz += f64::from(
                    (request.position.x - before.x)
                        .hypot(request.position.y - before.y)
                        .hypot(request.position.z - before.z),
                );
                assert!(request.position.x.hypot(request.position.y) < 0.001);
                count += 1;
            }
        }
        assert!(count > 50);
        let net_xyz = f64::from(owner.position().z - origin.z).abs();
        assert!(net_xyz <= path_xyz + 0.001);
        eprintln!(
            "controlled native turn_only={turn}: accepted={count} refused=0 accepted_path_xy=0 net_xy=0 accepted_path_xyz={path_xyz} net_xyz={net_xyz}"
        );
        if turn {
            assert!(owner.position().heading != 0.);
        }
    }
}

#[test]
fn releasing_strafe_preserves_earned_carry_and_correction_retires_pending_trial() {
    let mut f = Native::new(false, -15.);
    let mut owner = owner(&f);
    anchor(&f, &owner);
    crate::keys::testing::bind(
        &mut f.app,
        Act::StrafeLeft,
        crate::keys::Chord::key(KeyCode::KeyZ),
    );
    latency_tests::key(&mut f.app, KeyCode::KeyZ, true);
    for _ in 0..60 {
        for command in frame(&mut f, &mut owner) {
            settle(&f, &mut owner, command);
        }
    }
    let before = owner.position();
    latency_tests::key(&mut f.app, KeyCode::KeyZ, false);
    // Turning continues while input is released; it must not reset real q.
    latency_tests::key(&mut f.app, KeyCode::KeyA, true);
    let mut carry = false;
    for _ in 0..30 {
        for command in frame(&mut f, &mut owner) {
            let request = settle(&f, &mut owner, command);
            carry |= (request.position.x - before.x).hypot(request.position.y - before.y) > 0.001;
        }
    }
    assert!(carry);
    let stale = loop {
        if let Some(command) = frame(&mut f, &mut owner).pop() {
            break command;
        }
    };
    let ClientCommand::SwimFrame { request, controls } = stale else {
        panic!("missing queued owned step");
    };
    f.now += Duration::from_millis(100);
    let position = owner.position();
    owner.correct(position, f.now).unwrap();
    owner.calibrate(calibration(), f.now).unwrap();
    let reset = owner.swim_basis();
    assert!(
        owner
            .send_swim_sample(&request, Some(&controls), f.now, |_| panic!(
                "retired step submitted"
            ))
            .is_err()
    );
    assert_eq!(owner.swim_basis(), reset);
    anchor(&f, &owner);
    // A late result from that prior request must not restore its trial.
    news(
        &f,
        eq_client_core::WorldEvent::MotionSent {
            session_id: 11,
            created: request.created,
            position,
            refused: Some("retired by correction".into()),
        },
    );
    news(
        &f,
        eq_client_core::WorldEvent::SwimState {
            session_id: 11,
            created: Some(request.created),
            position,
            basis: reset,
        },
    );
    for _ in 0..60 {
        for command in frame(&mut f, &mut owner) {
            let request = settle(&f, &mut owner, command);
            assert_eq!(
                (request.position.x, request.position.y),
                (position.x, position.y)
            );
        }
    }
}

#[test]
fn held_strafe_and_turn_both_sides_keep_native_carry_and_report_path_and_net() {
    for side in [Act::StrafeLeft, Act::StrafeRight] {
        for turning in [None, Some(Act::TurnLeft), Some(Act::TurnRight)] {
            let mut f = Native::new(false, -15.);
            let mut owner = owner(&f);
            anchor(&f, &owner);
            // These are actual held keyboard bindings read by normal input.
            crate::keys::testing::bind(&mut f.app, side, crate::keys::Chord::key(KeyCode::KeyZ));
            latency_tests::key(&mut f.app, KeyCode::KeyZ, true);
            if let Some(turning) = turning {
                crate::keys::testing::bind(
                    &mut f.app,
                    turning,
                    crate::keys::Chord::key(KeyCode::KeyX),
                );
                latency_tests::key(&mut f.app, KeyCode::KeyX, true);
            }
            let origin = owner.position();
            let mut path = 0_f64;
            let mut path_xyz = 0_f64;
            let mut accepted = 0_u16;
            let mut refused = 0_u16;
            let start = f.now;
            while f.now.duration_since(start) < Duration::from_secs(10) {
                for command in frame(&mut f, &mut owner) {
                    let ClientCommand::SwimFrame { request, controls } = command else {
                        panic!("matched ordinary swim lost owned controls: {command:?}");
                    };
                    let before = owner.position();
                    owner
                        .inspect_swim_replay(&request, &controls)
                        .unwrap_or_else(|error| {
                            panic!(
                                "native replay: {error}; request={request:?}; controls={controls:?}"
                            )
                        });
                    let result =
                        owner.send_swim_sample(&request, Some(&controls), f.now, |_| Ok(()));
                    if result.is_ok() {
                        accepted += 1;
                    } else {
                        refused += 1;
                    }
                    let position = owner.position();
                    path += f64::from((position.x - before.x).hypot(position.y - before.y));
                    path_xyz += f64::from(
                        (position.x - before.x)
                            .hypot(position.y - before.y)
                            .hypot(position.z - before.z),
                    );
                    news(
                        &f,
                        eq_client_core::WorldEvent::MotionSent {
                            session_id: 11,
                            created: request.created,
                            position,
                            refused: result.err().map(|error| error.to_string()),
                        },
                    );
                    news(
                        &f,
                        eq_client_core::WorldEvent::SwimState {
                            session_id: 11,
                            position,
                            created: Some(request.created),
                            basis: owner.swim_basis(),
                        },
                    );
                    assert!(owner.swim_basis().is_some(), "native controls lost proof");
                }
            }
            let end = owner.position();
            let net = f64::from((end.x - origin.x).hypot(end.y - origin.y));
            let net_xyz = net.hypot(f64::from(end.z - origin.z));
            assert_eq!(refused, 0);
            assert!(
                accepted > 50 && path > 1.,
                "held controls must actually move"
            );
            assert!(net <= path + 0.001);
            assert!(net_xyz <= path_xyz + 0.001);
            if turning.is_some() {
                assert!(net < path * 0.8);
            } else {
                assert!(
                    path > 50.,
                    "straight strafe must retain its calibrated native travel"
                );
            }
            eprintln!(
                "controlled native {side:?}+{turning:?}: accepted={accepted} refused={refused} accepted_path_xy={path} net_xy={net} accepted_path_xyz={path_xyz} net_xyz={net_xyz}"
            );
        }
    }
}

#[test]
fn reset_receipt_requires_the_actual_uninterrupted_matching_reset() {
    let mut f = Native::new(false, -15.);
    let mut owner = owner(&f);
    let basis = owner.swim_basis().unwrap();
    let position = owner.position();
    let mut local = owned_swim::Owner::default();
    let mut wet = eq_client_core::movement::water::WetVelocity::default();
    local.receipt(11, position, None, Some(basis), &mut wet);
    assert!(
        local
            .begin(
                Some(11),
                position,
                Some((eq_client_core::movement::SwimInput::Idle, 0.))
            )
            .is_none()
    );
    local.reset_observed(11, position);
    local.physical_frame(true);
    wet.advance(
        eq_client_core::movement::water::LIQUID_5,
        eq_client_core::movement::water::WetInput::default(),
        eq_client_core::movement::water::WET_FRAME_SECONDS,
    );
    local.receipt(11, position, None, Some(basis), &mut wet);
    assert!(
        local
            .begin(
                Some(11),
                position,
                Some((eq_client_core::movement::SwimInput::Idle, 0.))
            )
            .is_none()
    );
    wet = eq_client_core::movement::water::WetVelocity::default();
    local.reset_observed(11, position);
    local.receipt(11, position, None, Some(basis), &mut wet);
    assert!(
        local
            .begin(
                Some(11),
                position,
                Some((eq_client_core::movement::SwimInput::Idle, 0.))
            )
            .is_some()
    );
    // Repeated same-position corrections still mint different opaque epochs.
    f.now += Duration::from_millis(100);
    owner.correct(position, f.now).unwrap();
    owner.calibrate(calibration(), f.now).unwrap();
    assert_ne!(owner.swim_basis(), Some(basis));
}

#[test]
fn repeated_ordinary_backward_control_keeps_the_native_zero_backward_rate() {
    let mut f = Native::new(false, -15.);
    let mut owner = owner(&f);
    anchor(&f, &owner);
    crate::keys::testing::bind(
        &mut f.app,
        Act::Back,
        crate::keys::Chord::key(KeyCode::KeyZ),
    );
    latency_tests::key(&mut f.app, KeyCode::KeyZ, true);
    assert_eq!(calibration().swim.unwrap().backward, 0.);
    assert_eq!(
        owned_swim::observed(MovementMode::Backward, Vec3::ZERO, 0.),
        Some((eq_client_core::movement::SwimInput::Backward, 0.))
    );
    let mut accepted = 0;
    for _ in 0..120 {
        for command in frame(&mut f, &mut owner) {
            let request = settle(&f, &mut owner, command);
            assert_eq!((request.position.x, request.position.y), (0., 0.));
            accepted += 1;
        }
    }
    assert!(accepted > 5);
}

#[test]
fn discarded_owned_request_restores_native_trial_without_a_position_publication() {
    let mut f = Native::new(false, -15.);
    let mut owner = owner(&f);
    let origin = owner.position();
    anchor(&f, &owner);
    crate::keys::testing::bind(
        &mut f.app,
        Act::StrafeLeft,
        crate::keys::Chord::key(KeyCode::KeyZ),
    );
    latency_tests::key(&mut f.app, KeyCode::KeyZ, true);
    let command = loop {
        if let Some(command) = frame(&mut f, &mut owner).pop() {
            break command;
        }
    };
    let ClientCommand::SwimFrame { request, .. } = command else {
        panic!("missing owned trial");
    };
    let rest = eq_client_core::movement::water::WetVelocity::default();
    assert_ne!(f.app.world().resource::<Controls>().wet, rest);
    news(
        &f,
        eq_client_core::WorldEvent::MotionRefused {
            session_id: 11,
            created: request.created,
            reason: "fixture discarded before motion owner".into(),
        },
    );
    assert_eq!(frame(&mut f, &mut owner).len(), 0);
    assert_eq!(f.app.world().resource::<Controls>().wet, rest);
    assert_eq!(
        f.app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position,
        origin
    );
    assert_eq!(owner.position(), origin);
    // This result did not accept the trial or establish a fresh reset marker.
    news(
        &f,
        eq_client_core::WorldEvent::SwimState {
            session_id: 11,
            created: Some(request.created),
            position: origin,
            basis: owner.swim_basis(),
        },
    );
    let next = loop {
        if let Some(command) = frame(&mut f, &mut owner).pop() {
            break command;
        }
    };
    assert!(matches!(next, ClientCommand::Move(_)));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one matched native mouse input/control sweep"
)]
fn held_mouse_strafe_and_turn_use_actual_slow_and_fast_inputs_in_both_views() {
    for first_person in [false, true] {
        for side in [Some(Act::StrafeLeft), Some(Act::StrafeRight), None] {
            for mouse in [-40_f32, -7.5, 7.5, 40.] {
                let mut f = Native::new(first_person, -15.);
                let mut owner = owner(&f);
                anchor(&f, &owner);
                crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
                if let Some(side) = side {
                    // Native turn bindings become sidesteps while right
                    // look is held: exercise the real A/D+mouse path.
                    let key = if side == Act::StrafeLeft {
                        KeyCode::KeyA
                    } else {
                        KeyCode::KeyD
                    };
                    latency_tests::key(&mut f.app, key, true);
                }
                let origin = owner.position();
                let mut path_xy = 0_f64;
                let mut path_xyz = 0_f64;
                let mut accepted = 0_u16;
                let mut snapshots = 0_u16;
                for _ in 0..600 {
                    // Actual owned mouse events continue while a worker result
                    // is pending. Only the input really consumed by each
                    // physical slice is captured; no interpolated arc is used.
                    f.app
                        .world_mut()
                        .write_message(bevy::input::mouse::MouseMotion {
                            delta: Vec2::new(mouse, 0.),
                        });
                    for command in frame(&mut f, &mut owner) {
                        let before = owner.position();
                        let request = settle(&f, &mut owner, command);
                        snapshots += u16::from(request.heading == HeadingUpdate::Snapshot);
                        let delta = [
                            request.position.x - before.x,
                            request.position.y - before.y,
                            request.position.z - before.z,
                        ];
                        path_xy += f64::from(delta[0].hypot(delta[1]));
                        path_xyz += f64::from(delta[0].hypot(delta[1]).hypot(delta[2]));
                        accepted += 1;
                    }
                }
                let end = owner.position();
                let net_xy = f64::from((end.x - origin.x).hypot(end.y - origin.y));
                let net_xyz = net_xy.hypot(f64::from(end.z - origin.z));
                assert!(accepted > 50 && snapshots > 0);
                assert!(net_xy <= path_xy + 0.001 && net_xyz <= path_xyz + 0.001);
                if side.is_some() {
                    assert!(path_xy > 1.);
                } else {
                    assert!(path_xy < 0.001 && net_xy < 0.001);
                }
                eprintln!(
                    "controlled native mouse first_person={first_person} side={side:?} dx_per_render_frame={mouse}: accepted={accepted} refused=0 accepted_path_xy={path_xy} net_xy={net_xy} accepted_path_xyz={path_xyz} net_xyz={net_xyz}"
                );
            }
        }
    }
}

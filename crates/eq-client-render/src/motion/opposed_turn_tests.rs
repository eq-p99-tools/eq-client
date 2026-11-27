//! Ordinary input, coordinate-frame and conservative clipping controls.
use super::*;
use eq_client_assets::regions::{RegionKind, ZoneRegions};
use eq_network_game::movement::{SwimControl, SwimControls, SwimInput, water};

#[derive(Default, Debug)]
struct Outcome {
    accepted: usize,
    refused: usize,
    replay_mismatches: usize,
    first_mismatch: Option<String>,
    backing_refusals: usize,
    path_xy: f64,
    net_xy: f32,
    dz: f32,
    start_heading: f32,
    end_heading: f32,
}

fn scene(slope: bool, pitch: f32) -> (Native, MotionSession) {
    let origin_y = if slope { -13.7 } else { -15. };
    let mut f = Native::new(false, origin_y);
    let position = world_position([0., origin_y, 0.], 128.);
    online::testing::place(
        &mut f.app.world_mut().resource_mut::<online::OnlineState>(),
        position,
    );
    f.app
        .world_mut()
        .get_mut::<Transform>(f.player)
        .unwrap()
        .rotation = Quat::from_rotation_y(eq_client_core::render_heading(128.));
    {
        let mut camera = f.app.world_mut().get_mut::<OrbitCamera>(f.camera).unwrap();
        camera.yaw = crate::camera::behind(eq_client_core::render_heading(128.));
        camera.look = pitch;
    }
    if slope {
        let at = |x: f32, z: f32| [x, -18.78 - 0.08 * x + 0.04 * z, z];
        let world = CollisionWorld::new([
            [at(-200., -200.), at(200., 200.), at(200., -200.)],
            [at(-200., -200.), at(-200., 200.), at(200., 200.)],
        ])
        .unwrap()
        .with_liquids(crate::scene_collision::ZoneLiquids(
            ZoneRegions::test_kind_surface(0., RegionKind::WATER).with_titanium_liquids(),
        ))
        .with_own_liquids(crate::scene_collision::ZoneLiquids(
            ZoneRegions::test_kind_surface(0., RegionKind::WATER).with_titanium_own_liquids(),
        ))
        .with_swim_origin(3.75);
        f.app.insert_resource(Collision(Some(world)));
    }
    let grant = f.app.world().resource::<Controls>().speeds();
    news(
        &f,
        eq_client_core::WorldEvent::MotionState {
            session_id: 11,
            units_per_second: grant.units_per_second,
            backward_units_per_second: grant.backward_units_per_second,
            walk_units_per_second: grant.walk_units_per_second,
            strafe_units_per_second: grant.strafe_units_per_second,
            strafe_walk_units_per_second: grant.strafe_walk_units_per_second,
            swim_backward_units_per_second: grant.swim_backward_units_per_second,
            swim: grant.swim,
            diagonal: grant.diagonal,
            falls: true,
            jump_factor: grant.jump_factor,
        },
    );
    let session = owner(&f);
    anchor(&f, &session);
    (f, session)
}

fn answer(f: &Native, session: &mut MotionSession, command: ClientCommand, out: &mut Outcome) {
    let before = session.position();
    // The worker handles even a fallback stamped at dispatch, after the
    // physical caller slice. No extra consumption is inserted into the tape.
    let now = f.now + Duration::from_millis(10);
    let (request, result) = match command {
        ClientCommand::SwimFrame { request, controls } => {
            if let Err(reason) = session.inspect_swim_replay(&request, &controls) {
                out.replay_mismatches += 1;
                out.first_mismatch.get_or_insert_with(|| reason.to_string());
            }
            let result = session.send_swim_sample(&request, Some(&controls), now, |_| Ok(()));
            (request, result)
        }
        ClientCommand::Move(request) => {
            let result = session.send_move_sample(&request, now, |_| Ok(()));
            (request, result)
        }
        _ => panic!("unexpected ordinary swim command"),
    };
    let refused = result.err().map(|reason| reason.to_string());
    if let Some(reason) = refused.as_ref() {
        out.refused += 1;
        out.backing_refusals += usize::from(reason.contains("backing up in water"));
        assert_eq!(session.position(), before, "refusal cannot commit XYZ");
    } else {
        out.accepted += 1;
        let after = session.position();
        out.path_xy += f64::from((after.x - before.x).hypot(after.y - before.y));
    }
    news(
        f,
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: session.position(),
            refused,
        },
    );
    news(
        f,
        eq_client_core::WorldEvent::SwimState {
            session_id: request.session_id,
            created: Some(request.created),
            position: session.position(),
            basis: session.swim_basis(),
        },
    );
}

fn run(slope: bool, pitch: f32, side: Option<KeyCode>, turn: f32) -> Outcome {
    let (mut f, mut session) = scene(slope, pitch);
    if slope {
        // Earn the actual falling wet state before meeting the floor. A
        // supported zero-displacement start is not a witnessed native fall.
        let mut settling = Outcome::default();
        for _ in 0..180 {
            for command in frame(&mut f, &mut session) {
                answer(&f, &mut session, command, &mut settling);
            }
        }
    }
    let origin = session.position();
    assert!(
        arc(128., origin.heading).abs() < 0.001,
        "fixture must retain the actual EQ basis"
    );
    // RMB acquisition faces along the current camera. Match its controlled
    // initial view to the accepted body without resetting earned wet state.
    f.app
        .world_mut()
        .get_mut::<OrbitCamera>(f.camera)
        .unwrap()
        .yaw = crate::camera::behind(eq_client_core::render_heading(origin.heading));
    let mut out = Outcome {
        start_heading: origin.heading,
        ..Outcome::default()
    };
    crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
    if let Some(key) = side {
        if key == KeyCode::KeyS {
            crate::keys::testing::bind(&mut f.app, Act::Back, crate::keys::Chord::key(key));
        }
        latency_tests::key(&mut f.app, key, true);
    }
    for _ in 0..600 {
        if turn != 0. {
            f.app
                .world_mut()
                .write_message(bevy::input::mouse::MouseMotion {
                    delta: Vec2::new(turn, 0.),
                });
        }
        for command in frame(&mut f, &mut session) {
            answer(&f, &mut session, command, &mut out);
        }
    }
    let end = session.position();
    out.net_xy = (end.x - origin.x).hypot(end.y - origin.y);
    out.dz = end.z - origin.z;
    out.end_heading = end.heading;
    out
}

#[test]
fn clear_water_both_strafe_turn_signs_preserve_pitch_independent_depth() {
    for pitch in [-0.45, 0., 0.45] {
        let idle = run(false, pitch, None, 0.);
        for side in [KeyCode::KeyA, KeyCode::KeyD] {
            for turn in [-40., 0., 40.] {
                let row = run(false, pitch, Some(side), turn);
                assert_eq!(
                    row.refused, 0,
                    "pitch={pitch} side={side:?} turn={turn}: {row:?}"
                );
                assert_eq!(row.replay_mismatches, 0, "{row:?}");
                assert!(row.path_xy > 10. && f64::from(row.net_xy) <= row.path_xy + 0.001);
                assert!(
                    (row.dz - idle.dz).abs() < 0.002,
                    "a pure strafe adds no pitch stroke: {row:?}"
                );
            }
        }
    }
}

#[test]
fn a_sloped_bottom_can_change_depth_without_any_pitch_stroke() {
    let idle = run(true, 0., None, 0.);
    let left = run(true, 0., Some(KeyCode::KeyA), 0.);
    let right = run(true, 0., Some(KeyCode::KeyD), 0.);
    eprintln!("synthetic slope idle={idle:?} left={left:?} right={right:?}");
    assert!(arc(left.start_heading, left.end_heading).abs() < 0.001);
    assert!(arc(right.start_heading, right.end_heading).abs() < 0.001);
    assert!(left.dz > 0.1 && right.dz < -0.1);
    assert!(idle.net_xy > 0.01, "gravity can slide along a sloped solid");
    assert!(
        left.replay_mismatches + right.replay_mismatches > 0,
        "resolved collision travel is not the unmodified native endpoint"
    );
}

#[test]
fn unverified_floor_clipping_cannot_silently_gain_owned_replay_credit() {
    let mut refused = 0;
    for side in [KeyCode::KeyA, KeyCode::KeyD] {
        for turn in [-40., 40.] {
            let row = run(true, 0., Some(side), turn);
            eprintln!("synthetic floor side={side:?} turn={turn}: {row:?}");
            refused += row.backing_refusals;
            assert!(f64::from(row.net_xy) <= row.path_xy + 0.001);
        }
    }
    assert!(
        refused > 0,
        "this is the conservative fallback reproduction, not a repair"
    );
}

#[test]
fn genuine_backing_cannot_create_fresh_horizontal_credit() {
    let row = run(false, 0.45, Some(KeyCode::KeyS), 0.);
    assert_eq!(row.refused, 0, "{row:?}");
    assert!(
        row.path_xy < 0.001,
        "native backward input is zero here: {row:?}"
    );
    let (f, mut session) = scene(false, 0.);
    let position = session.position();
    let created = f.now + Duration::from_millis(100);
    let controls = SwimControls::new(
        session.swim_basis().unwrap(),
        vec![
            SwimControl::new(
                Duration::from_millis(100),
                128.,
                water::WetKind::Normal,
                SwimInput::Backward,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut request = MovementRequest::new(MovementMode::Swim, 11, position, created);
    request.position.x -= 0.5;
    assert!(session.inspect_swim_replay(&request, &controls).is_err());
    assert!(
        session
            .send_swim_sample(&request, Some(&controls), created, |_| Ok(()))
            .is_err()
    );
    assert_eq!(session.position(), position);
}

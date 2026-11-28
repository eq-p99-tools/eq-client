//! Ordinary input, coordinate-frame and conservative clipping controls.
use super::*;
use eq_client_assets::regions::{RegionKind, ZoneRegions};
use eq_network_game::movement::{SwimControl, SwimControls, SwimInput, water};

#[derive(Default, Debug)]
struct Outcome {
    accepted: usize,
    refused: usize,
    replay_mismatches: usize,
    owned_proposals: usize,
    physical_proposals: usize,
    first_mismatch: Option<String>,
    backing_refusals: usize,
    path_xy: f64,
    path_xyz: f64,
    net_xy: f32,
    net_xyz: f32,
    dz: f32,
    start_heading: f32,
    end_heading: f32,
}

fn scene(slope: bool, pitch: f32) -> (Native, MotionSession) {
    scene_with_receipt(slope, pitch, true)
}

fn scene_with_receipt(slope: bool, pitch: f32, receipt: bool) -> (Native, MotionSession) {
    let origin_y = if slope { -13.7 } else { -15. };
    let mut f = Native::new(false, origin_y);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_floor_swim = true;
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
    if receipt {
        anchor(&f, &session);
    } else {
        // Observe the same actual reset, but do not invent its missing
        // qualified receipt. Unknown ownership keeps the ordinary checks.
        news(
            &f,
            eq_client_core::WorldEvent::Position {
                spawn_id: 7,
                position: session.position(),
                velocity: [0.; 3],
                forward: 0.,
            },
        );
    }
    (f, session)
}

fn answer(
    f: &Native,
    session: &mut MotionSession,
    command: ClientCommand,
    out: &mut Outcome,
) -> [eq_client_core::WorldEvent; 2] {
    let before = session.position();
    // The worker handles even a fallback stamped at dispatch, after the
    // physical caller slice. No extra consumption is inserted into the tape.
    let now = f.now + Duration::from_millis(10);
    let (request, result) = match command {
        ClientCommand::SwimFrame { request, controls } => {
            out.owned_proposals += 1;
            out.physical_proposals += usize::from(controls.uses_scene());
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
        let horizontal = (after.x - before.x).hypot(after.y - before.y);
        out.path_xy += f64::from(horizontal);
        out.path_xyz += f64::from(horizontal.hypot(after.z - before.z));
    }
    [
        eq_client_core::WorldEvent::MotionSent {
            session_id: request.session_id,
            created: request.created,
            position: session.position(),
            refused,
        },
        eq_client_core::WorldEvent::SwimState {
            session_id: request.session_id,
            created: Some(request.created),
            position: session.position(),
            basis: session.swim_basis(),
        },
    ]
}

type Replies = std::collections::VecDeque<(usize, [eq_client_core::WorldEvent; 2])>;

fn drive(
    f: &mut Native,
    session: &mut MotionSession,
    out: &mut Outcome,
    replies: &mut Replies,
    tick: usize,
    turn: f32,
) {
    while replies.front().is_some_and(|(due, _)| *due <= tick) {
        for event in replies.pop_front().unwrap().1 {
            news(f, event);
        }
    }
    if turn != 0. {
        f.app
            .world_mut()
            .write_message(bevy::input::mouse::MouseMotion {
                delta: Vec2::new(turn, 0.),
            });
    }
    for command in frame(f, session) {
        let events = answer(f, session, command, out);
        // Three real rendered frames separate publication and delivery.
        // This yields the ordinary ~150ms sample cadence in retained rows.
        replies.push_back((tick + 3, events));
    }
}

fn run(slope: bool, pitch: f32, side: Option<KeyCode>, turn: f32) -> Outcome {
    run_with_receipt(slope, pitch, side, turn, true)
}

#[test]
fn scripted_face_preserves_earned_floor_swim_for_the_next_rmb_strafe() {
    let (mut f, mut session) = scene(true, 0.);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_floor_swim = false;
    let mut replies = Replies::new();
    let mut settling = Outcome::default();
    for tick in 0..180 {
        drive(&mut f, &mut session, &mut settling, &mut replies, tick, 0.);
    }
    assert!(settling.physical_proposals > 0);
    let start = session.position().heading;
    let goal = (start + 64.).rem_euclid(512.);
    f.app.insert_resource(
        crate::script::Script::new(Vec::new()).holding_for_test(vec![], Some(goal)),
    );
    let mut turning = Outcome::default();
    drive(&mut f, &mut session, &mut turning, &mut replies, 180, 0.);
    let heading = f
        .app
        .world()
        .resource::<Controls>()
        .floor_swim
        .heading()
        .expect("script facing keeps the active wet controller");
    assert!(arc(start, heading).abs() < 64.);
    for tick in 181..300 {
        drive(&mut f, &mut session, &mut turning, &mut replies, tick, 0.);
    }
    assert!(turning.physical_proposals > 0);
    assert!(arc(session.position().heading, goal).abs() < 1.);
    f.app.world_mut().remove_resource::<crate::script::Script>();
    crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
    latency_tests::key(&mut f.app, KeyCode::KeyA, true);
    let mut strafing = Outcome::default();
    for tick in 300..480 {
        drive(&mut f, &mut session, &mut strafing, &mut replies, tick, 1.);
    }
    assert!(
        strafing.physical_proposals > 0 && strafing.path_xy > 0.,
        "{strafing:?}"
    );
    assert_eq!(
        strafing.refused + strafing.replay_mismatches,
        0,
        "{strafing:?}"
    );
}

fn run_with_receipt(
    slope: bool,
    pitch: f32,
    side: Option<KeyCode>,
    turn: f32,
    receipt: bool,
) -> Outcome {
    run_selected(slope, pitch, side, turn, receipt, false)
}

fn run_selected(
    slope: bool,
    pitch: f32,
    side: Option<KeyCode>,
    turn: f32,
    receipt: bool,
    physical: bool,
) -> Outcome {
    let (mut f, mut session) = scene_with_receipt(slope, pitch, receipt);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_floor_swim = !physical;
    let mut replies = Replies::new();
    if slope {
        // Earn the actual falling wet state before meeting the floor. A
        // supported zero-displacement start is not a witnessed native fall.
        let mut settling = Outcome::default();
        for tick in 0..180 {
            drive(&mut f, &mut session, &mut settling, &mut replies, tick, 0.);
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
    for tick in 180..780 {
        drive(&mut f, &mut session, &mut out, &mut replies, tick, turn);
    }
    let end = session.position();
    out.net_xy = (end.x - origin.x).hypot(end.y - origin.y);
    out.dz = end.z - origin.z;
    out.net_xyz = out.net_xy.hypot(out.dz);
    out.end_heading = end.heading;
    out
}

#[test]
fn clear_water_both_strafe_turn_signs_preserve_pitch_independent_depth() {
    let calibrated = calibration().swim.unwrap();
    let maximum_path =
        f64::from(water::WetCaps::new(calibrated.set, calibrated.strafe, 0.).horizontal) * 10.
            + 0.01;
    for pitch in [-0.45, 0., 0.45] {
        let idle = run(false, pitch, None, 0.);
        for turn in [-40., -7.5, 0., 7.5, 40.] {
            let left = run(false, pitch, Some(KeyCode::KeyA), turn);
            let right = run(false, pitch, Some(KeyCode::KeyD), -turn);
            for row in [&left, &right] {
                assert_eq!(row.refused, 0, "pitch={pitch} turn={turn}: {row:?}");
                assert_eq!(
                    row.replay_mismatches, 0,
                    "every native endpoint must match: {row:?}"
                );
                assert!(row.accepted > 50 && row.path_xy > idle.path_xy + 1.);
                assert!(
                    row.path_xy <= maximum_path,
                    "only the calibrated strafe earns travel: {row:?}"
                );
                assert!(f64::from(row.net_xy) <= row.path_xy + 0.001);
                assert!(
                    (row.dz - idle.dz).abs() < 0.002,
                    "a pure strafe adds no pitch stroke: {row:?}"
                );
            }
            assert!(
                (left.path_xy - right.path_xy).abs() < 0.01,
                "mirrored physical input must retain the same native travel: {left:?} / {right:?}"
            );
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
            let opposed =
                (side == KeyCode::KeyA && turn > 0.) || (side == KeyCode::KeyD && turn < 0.);
            if opposed {
                assert!(
                    row.replay_mismatches > 0 && row.backing_refusals > 0,
                    "both opposed signs must exercise the fallback: {row:?}"
                );
            } else {
                assert_eq!(row.refused, 0, "co-direction control: {row:?}");
            }
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

#[test]
fn idle_floor_clipping_can_retire_replay_before_strafe() {
    for (slope, milliseconds, retains_replay) in [
        (true, 151, true),
        (true, 250, false),
        (false, 151, true),
        (false, 250, true),
    ] {
        let (mut f, mut session) = scene(slope, 0.);
        let mut replies = Replies::new();
        let mut settling = Outcome::default();
        for tick in 0..360 {
            drive(&mut f, &mut session, &mut settling, &mut replies, tick, 0.);
            if tick >= 180
                && replies.is_empty()
                && f.app.world().resource::<Controls>().pending.is_none()
            {
                break;
            }
        }
        // Settle through the real Receive pass before measuring. Delivering
        // an answer on the measured frame would restart its ordinary gate.
        assert!(replies.is_empty());
        assert!(f.app.world().resource::<Controls>().pending.is_none());
        let basis = session.swim_basis().unwrap();
        let target = basis.started_at() + Duration::from_millis(milliseconds);
        let step = target.checked_duration_since(f.now).unwrap();
        // Three 16,667us reply frames use 50,001us. The short control is
        // 151ms so its remaining time exceeds the ordinary 100ms gate.
        assert!(step >= START_CYCLE_DURATION);
        let before = session.position();
        let commands = frame_for(&mut f, &mut session, step);
        assert_eq!(commands.len(), 1, "one measured ordinary sample");
        assert!(matches!(
            commands.as_slice(),
            [ClientCommand::SwimFrame { .. }]
        ));
        let mut out = Outcome::default();
        for command in commands {
            for event in answer(&f, &mut session, command, &mut out) {
                news(&f, event);
            }
        }
        assert_eq!(
            out.accepted, 1,
            "ordinary fallback stays available: {out:?}"
        );
        assert_eq!(out.refused, 0, "idle is not backing: {out:?}");
        assert_eq!(
            session.position().heading.to_bits(),
            before.heading.to_bits()
        );
        assert_eq!(session.swim_basis().is_some(), retains_replay, "{out:?}");
        if retains_replay {
            assert_eq!(out.replay_mismatches, 0, "{out:?}");
        } else {
            assert_eq!(out.replay_mismatches, 1, "{out:?}");
            assert!(out.first_mismatch.as_ref().is_some_and(|reason| {
                reason.contains("native swim horizontal endpoint differs")
            }));
        }
        if slope {
            // The native Idle input has no earned XY velocity. These are
            // collision-resolved sampled lengths, not input or travel credit.
            assert!(out.path_xy > 0.);
            assert_eq!(out.path_xy <= 0.01, retains_replay, "{out:?}");
        } else {
            assert!(out.path_xy < 0.001, "clear water adds no idle XY: {out:?}");
        }
    }
}

#[test]
fn clear_water_unknown_basis_can_refuse_an_opposed_curve_without_a_ledger_error() {
    for (side, turn) in [(KeyCode::KeyA, 40.), (KeyCode::KeyD, -40.)] {
        let qualified = run_with_receipt(false, 0., Some(side), turn, true);
        let unknown = run_with_receipt(false, 0., Some(side), turn, false);
        let co_direction = run_with_receipt(false, 0., Some(side), -turn, false);
        eprintln!(
            "controlled clear water side={side:?} turn={turn}: qualified={qualified:?} unknown={unknown:?} co_direction={co_direction:?}"
        );
        assert!(qualified.owned_proposals > 0);
        assert_eq!(
            qualified.owned_proposals, qualified.accepted,
            "{qualified:?}"
        );
        assert_eq!(qualified.refused, 0, "{qualified:?}");
        assert_eq!(qualified.replay_mismatches, 0, "{qualified:?}");
        assert_eq!(unknown.owned_proposals, 0, "no receipt is not a new anchor");
        assert_eq!(
            unknown.replay_mismatches, 0,
            "no heading ledger was submitted"
        );
        assert!(unknown.backing_refusals > 0, "{unknown:?}");
        assert_eq!(co_direction.owned_proposals, 0);
        assert_eq!(
            co_direction.refused, 0,
            "ordinary control: {co_direction:?}"
        );
        // This characterises the conservative fallback, not a translation
        // repair or an exact replay of any captured live input chronology.
        assert!(unknown.path_xy < qualified.path_xy);
    }
}

#[test]
fn physical_continuous_rmb_strafe_keeps_swim_replay_after_unfocused_idle() {
    for (side, dx) in [(KeyCode::KeyA, 4.), (KeyCode::KeyD, -4.)] {
        let (mut f, mut session) = scene(false, 0.);
        f.app
            .world_mut()
            .resource_mut::<Controls>()
            .suppress_floor_swim = false;
        let mut replies = Replies::new();
        let mut idle = Outcome::default();
        for tick in 0..60 {
            drive(&mut f, &mut session, &mut idle, &mut replies, tick, 0.);
        }
        assert!(idle.physical_proposals > 0);
        let set_focus = |app: &mut App, focused| {
            let world = app.world_mut();
            let (window, mut state) = world
                .query_filtered::<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>()
                .single_mut(world)
                .unwrap();
            state.focused = focused;
            world.write_message(bevy::window::WindowFocused { window, focused });
        };
        set_focus(&mut f.app, false);
        let mut coast = Outcome::default();
        for tick in 60..90 {
            drive(&mut f, &mut session, &mut coast, &mut replies, tick, 0.);
        }
        assert!(
            coast.physical_proposals > 0 && coast.path_xy < 0.001,
            "{coast:?}"
        );
        set_focus(&mut f.app, true);
        crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
        latency_tests::key(&mut f.app, side, true);
        let mut travel = Outcome::default();
        let mut signed_turn = 0.;
        for tick in 90..690 {
            let previous = session.position().heading;
            drive(&mut f, &mut session, &mut travel, &mut replies, tick, dx);
            // EQ headings wrap at 512. Compare each accepted turn to its
            // predecessor instead of interpreting the final wrapped chord.
            let turn = arc(previous, session.position().heading);
            assert!(
                turn * dx <= 0.001,
                "reversed while strafing: {turn} at {tick}"
            );
            signed_turn += turn;
        }
        assert!(signed_turn.abs() > 128., "expected a continuous turn");
        assert!(
            travel.physical_proposals > 50 && travel.path_xy > 40.,
            "{travel:?}"
        );
        assert_eq!(travel.refused + travel.replay_mismatches, 0, "{travel:?}");
    }
}

#[test]
fn a_short_underwater_rmb_drag_turns_the_accepted_body_toward_that_side_of_the_view() {
    for dx in [-4.0_f32, 4.0] {
        let (mut f, mut session) = scene(true, 0.);
        f.app
            .world_mut()
            .resource_mut::<Controls>()
            .suppress_floor_swim = false;
        let mut replies = Replies::new();
        let mut out = Outcome::default();
        for tick in 0..180 {
            drive(&mut f, &mut session, &mut out, &mut replies, tick, 0.);
        }
        let ahead =
            |heading| Quat::from_rotation_y(eq_client_core::render_heading(heading)) * Vec3::Z;
        let start = ahead(session.position().heading);
        let screen_right = start.cross(Vec3::Y);
        crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
        for tick in 180..240 {
            let drag = if tick == 185 { dx } else { 0. };
            drive(&mut f, &mut session, &mut out, &mut replies, tick, drag);
        }
        let end = ahead(session.position().heading);
        assert!(end.dot(start) > 0.99, "a short drag must not wrap around");
        assert!(
            end.dot(screen_right) * dx > 0.,
            "wrong side for dx={dx}: {out:?}"
        );
        assert_eq!(out.refused, 0);
    }
}

#[test]
fn bottom_strafe_resumes_after_one_long_render_frame() {
    let (mut f, mut session) = scene(true, 0.);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_floor_swim = false;
    let mut replies = Replies::new();
    let mut before = Outcome::default();
    for tick in 0..180 {
        drive(&mut f, &mut session, &mut before, &mut replies, tick, 0.);
    }
    crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
    latency_tests::key(&mut f.app, KeyCode::KeyD, true);
    for tick in 180..300 {
        drive(&mut f, &mut session, &mut before, &mut replies, tick, -2.);
    }
    assert!(before.physical_proposals > 0 && before.path_xy > 1.);
    assert_eq!(before.refused, 0);

    // One observed stall with no intervening input or physics updates.
    // Continue from the same admission, geometry, body and held key.
    let mut stalled = Outcome::default();
    for command in frame_for(&mut f, &mut session, Duration::from_micros(231_838)) {
        let events = answer(&f, &mut session, command, &mut stalled);
        replies.push_back((303, events));
    }
    let mut after = Outcome::default();
    for tick in 301..901 {
        drive(&mut f, &mut session, &mut after, &mut replies, tick, -2.);
    }
    assert_eq!(
        stalled.refused + after.refused,
        0,
        "{stalled:?} / {after:?}"
    );
    assert_eq!(after.replay_mismatches, 0);
    assert!(after.path_xy > 40., "{after:?}");
}

#[test]
fn physical_floor_strafe_and_turn_preserve_all_four_signs_and_pitch_controls() {
    for side in [KeyCode::KeyA, KeyCode::KeyD] {
        for turn in [-40.0_f32, -4.0, 0.0, 4.0, 40.0] {
            for pitch in [-0.55_f32, 0.0, 0.55] {
                let row = run_selected(true, pitch, Some(side), turn, true, true);
                eprintln!(
                    "controlled physical floor side={side:?} turn={turn} pitch={pitch}: {row:?}"
                );
                assert_eq!(row.refused, 0, "{row:?}");
                assert_eq!(row.replay_mismatches, 0, "{row:?}");
                // Rapid rotation legitimately reduces native travel through
                // directional damping; compare path, never its endpoint chord.
                let minimum = if turn.abs() < 10.0 { 40.0 } else { 5.0 };
                assert!(
                    row.physical_proposals > 50 && row.path_xy > minimum,
                    "{row:?}"
                );
                assert!(f64::from(row.net_xy) <= row.path_xy + 0.001);
                assert!(row.path_xyz + 0.001 >= row.path_xy);
                assert!(f64::from(row.net_xyz) <= row.path_xyz + 0.001);
            }
        }
    }
}

#[test]
fn physical_floor_idle_has_no_projection_travel_and_backing_stays_zero() {
    for (key, turn) in [
        (None, -4.0),
        (None, 0.0),
        (None, 4.0),
        (Some(KeyCode::KeyS), 0.0),
    ] {
        let row = run_selected(true, 0.0, key, turn, true, true);
        assert!(row.physical_proposals > 50, "{row:?}");
        assert_eq!(row.refused, 0, "{row:?}");
        assert_eq!(row.replay_mismatches, 0, "{row:?}");
        assert!(row.path_xy < 0.001, "{row:?}");
        if turn != 0.0 {
            assert!(
                arc(row.start_heading, row.end_heading).abs() > 1.0,
                "turn-only input was ignored: {row:?}"
            );
        }
    }
}

#[test]
fn physical_clear_water_retains_normal_sink_and_unknown_basis_uses_old_guard() {
    let idle = run_selected(false, 0.0, None, 0.0, true, true);
    assert!(idle.dz < -6.4 && idle.dz > -6.9, "{idle:?}");
    for (key, turn) in [(KeyCode::KeyA, 4.0_f32), (KeyCode::KeyD, -4.0)] {
        let row = run_selected(false, 0.4, Some(key), turn, true, true);
        assert_eq!(row.refused, 0, "{row:?}");
        assert_eq!(row.replay_mismatches, 0, "{row:?}");
        assert!((row.dz - idle.dz).abs() < 0.1, "{row:?}");
        let unknown = run_selected(false, 0.0, Some(key), turn, false, true);
        assert_eq!(unknown.owned_proposals, 0, "{unknown:?}");
        assert!(unknown.backing_refusals > 0, "{unknown:?}");
    }
}

#[test]
fn physical_pending_and_refused_frames_never_enter_visible_xyz() {
    let (mut f, mut session) = scene_with_receipt(false, 0.0, true);
    f.app
        .world_mut()
        .resource_mut::<Controls>()
        .suppress_floor_swim = false;
    let before = f
        .app
        .world()
        .get::<Transform>(f.player)
        .unwrap()
        .translation;
    latency_tests::key(&mut f.app, KeyCode::KeyW, true);
    let command = loop {
        if let Some(command) = frame(&mut f, &mut session).pop() {
            break command;
        }
    };
    let ClientCommand::SwimFrame { request, controls } = command else {
        panic!("physical swim command missing");
    };
    assert!(controls.uses_scene());
    for _ in 0..3 {
        assert_eq!(frame(&mut f, &mut session).len(), 0);
        let shown = f
            .app
            .world()
            .get::<Transform>(f.player)
            .unwrap()
            .translation;
        assert!(
            shown.distance(before) < 0.000_001,
            "pending motion became visible"
        );
    }
    let retained = session.position();
    session.withdraw();
    assert!(
        session
            .send_swim_sample(&request, Some(&controls), f.now, |_| panic!(
                "withdrawn input sent"
            ))
            .is_err()
    );
    news(
        &f,
        eq_client_core::WorldEvent::MotionSent {
            session_id: 11,
            created: request.created,
            position: retained,
            refused: Some("test withdrawn grant".into()),
        },
    );
    news(
        &f,
        eq_client_core::WorldEvent::SwimState {
            session_id: 11,
            created: Some(request.created),
            position: retained,
            basis: None,
        },
    );
    frame(&mut f, &mut session);
    assert!(
        f.app
            .world()
            .get::<Transform>(f.player)
            .unwrap()
            .translation
            .distance(before)
            < 0.000_001
    );
}

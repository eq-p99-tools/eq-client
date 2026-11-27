//! Queued reset/grant/receipt chronology, with real no-physics and moved controls.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use eq_client_core::{
    WorldEvent,
    movement::{SwimInput, water},
};

fn receive(f: &mut Native, events: impl IntoIterator<Item = WorldEvent>) {
    for event in events {
        news(f, event);
    }
    f.app.world_mut().run_system_once(online::receive).unwrap();
}

fn motion_state(session: u64, grant: eq_client_core::world::MotionGrant) -> WorldEvent {
    WorldEvent::MotionState {
        session_id: session,
        units_per_second: grant.units_per_second,
        backward_units_per_second: grant.backward_units_per_second,
        walk_units_per_second: grant.walk_units_per_second,
        strafe_units_per_second: grant.strafe_units_per_second,
        strafe_walk_units_per_second: grant.strafe_walk_units_per_second,
        swim_backward_units_per_second: grant.swim_backward_units_per_second,
        swim: grant.swim,
        diagonal: grant.diagonal,
        falls: grant.falls,
        jump_factor: grant.jump_factor,
    }
}

fn placed(f: &mut Native, admission: bool) -> (u64, eq_client_core::WorldPosition) {
    let player = f
        .app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .clone();
    let position = player.position;
    let session = if admission { 12 } else { 11 };
    let event = if admission {
        WorldEvent::Entered {
            capabilities: eq_client_core::Capability::ALL.to_vec(),
            choices: Vec::new(),
            session_id: session,
            zone: "qeytoqrg".into(),
            player: Box::new(player),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 1,
        }
    } else {
        WorldEvent::Position {
            spawn_id: 7,
            position,
            velocity: [0.; 3],
            forward: 0.,
        }
    };
    receive(f, [event]);
    (session, position)
}

fn delayed(admission: bool, delay: usize, unloaded: bool) -> (Native, MotionSession) {
    let mut f = Native::new(false, -15.);
    let ready = f.app.world().resource::<Controls>().speeds();
    let (session, position) = placed(&mut f, admission);
    let mut owner = MotionSession::new(session, 7, position, f.now)
        .unwrap()
        .with_swimming(true)
        .with_heading_snapshots(true)
        .with_falls(true);
    // The real admission asks for calibration after creating its reset.
    owner.withdraw();
    let missing = eq_client_core::world::MotionGrant {
        units_per_second: None,
        backward_units_per_second: None,
        walk_units_per_second: None,
        strafe_units_per_second: None,
        strafe_walk_units_per_second: None,
        swim_backward_units_per_second: None,
        swim: None,
        diagonal: None,
        falls: true,
        jump_factor: 1.,
    };
    receive(&mut f, [motion_state(session, missing)]);
    let collision = f.app.world().resource::<Collision>().0.clone();
    if unloaded {
        f.app.world_mut().resource_mut::<Collision>().0 = None;
    }
    for _ in 0..delay {
        let proposals = frame(&mut f, &mut owner);
        assert_eq!(
            proposals.len(),
            0,
            "no physical proposal while awaiting the grant"
        );
        assert_eq!(
            f.app.world().resource::<Controls>().wet,
            water::WetVelocity::default()
        );
        assert_eq!(
            f.app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position,
            position
        );
    }
    f.app.world_mut().resource_mut::<Collision>().0 = collision;
    owner.calibrate(calibration(), f.now).unwrap();
    let basis = owner
        .swim_basis()
        .expect("unused actual reset survives ungranted time");
    receive(
        &mut f,
        [
            motion_state(session, ready),
            WorldEvent::SwimState {
                session_id: session,
                position,
                created: None,
                basis: Some(basis),
            },
        ],
    );
    (f, owner)
}

#[test]
fn observed_reset_survives_idle_input_passes_before_its_receipt() {
    let f = Native::new(false, -15.);
    let owner = owner(&f);
    for swimming in [false, true] {
        for delay in [1, 8, 40] {
            let position = owner.position();
            let mut local = owned_swim::Owner::default();
            let mut wet = water::WetVelocity::default();
            local.reset_observed(11, position);
            for _ in 0..delay {
                local.physical_frame(swimming);
            }
            local.receipt(11, position, None, owner.swim_basis(), &mut wet);
            assert!(
                local
                    .begin(Some(11), position, Some((SwimInput::Idle, 0.)))
                    .is_some()
            );
        }
    }
}

#[test]
fn delayed_correction_and_zone_in_receipts_engage_the_ordinary_controller() {
    for admission in [false, true] {
        for delay in [1, 8, 40] {
            for unloaded in [false, true] {
                let (mut f, mut owner) = delayed(admission, delay, unloaded);
                let mut accepted = 0;
                for _ in 0..24 {
                    for command in frame(&mut f, &mut owner) {
                        settle(&f, &mut owner, command);
                        accepted += 1;
                    }
                }
                assert!(accepted > 0);
            }
        }
    }
}

#[test]
fn delayed_real_reset_keeps_both_strafe_turn_sides_and_turn_only_validated() {
    for admission in [false, true] {
        for side in [Some(KeyCode::KeyA), Some(KeyCode::KeyD), None] {
            let (mut f, mut owner) = delayed(admission, 40, admission);
            crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
            if let Some(side) = side {
                latency_tests::key(&mut f.app, side, true);
            }
            let origin = owner.position();
            let mut path = 0_f64;
            let mut accepted = 0;
            for _ in 0..600 {
                f.app
                    .world_mut()
                    .write_message(bevy::input::mouse::MouseMotion {
                        delta: Vec2::new(40., 0.),
                    });
                for command in frame(&mut f, &mut owner) {
                    let before = owner.position();
                    let request = settle(&f, &mut owner, command);
                    path += f64::from(
                        (request.position.x - before.x).hypot(request.position.y - before.y),
                    );
                    accepted += 1;
                }
            }
            let end = owner.position();
            let net = f64::from((end.x - origin.x).hypot(end.y - origin.y));
            assert!(accepted > 50 && net <= path + 0.001);
            assert_eq!(path > 1., side.is_some());
            eprintln!(
                "controlled delayed reset admission={admission} side={side:?}: accepted={accepted} refused=0 path_xy={path} net_xy={net}"
            );
        }
    }
}

#[test]
fn a_moved_controller_cannot_adopt_a_late_reset_receipt() {
    for admission in [false, true] {
        let mut f = Native::new(false, -15.);
        let ready = f.app.world().resource::<Controls>().speeds();
        let (session, position) = placed(&mut f, admission);
        receive(&mut f, [motion_state(session, ready)]);
        let mut owner = MotionSession::new(session, 7, position, f.now)
            .unwrap()
            .with_swimming(true)
            .with_heading_snapshots(true)
            .with_falls(true);
        owner.calibrate(calibration(), f.now).unwrap();
        let old = owner.swim_basis();
        latency_tests::key(&mut f.app, KeyCode::KeyW, true);
        let request = (0..120)
            .find_map(|_| {
                let mut proposals = frame(&mut f, &mut owner);
                if let Some(command) = proposals.pop() {
                    let ClientCommand::Move(request) = command else {
                        panic!("no reset receipt supplied yet");
                    };
                    Some(request)
                } else {
                    None
                }
            })
            .expect("ordinary input must actually move before the late receipt");
        assert_ne!(request.position, position);
        let before = f.app.world().resource::<Controls>().wet;
        assert_ne!(before, water::WetVelocity::default());
        receive(
            &mut f,
            [WorldEvent::SwimState {
                session_id: session,
                position,
                created: None,
                basis: old,
            }],
        );
        let mut controls = f.app.world_mut().resource_mut::<Controls>();
        assert_eq!(
            controls.wet, before,
            "late receipt must not reset consumed physics"
        );
        assert!(
            controls
                .swim_owner
                .begin(Some(session), position, Some((SwimInput::Idle, 0.)))
                .is_none()
        );
    }
}

#[test]
fn even_a_submitted_zero_move_consumes_the_real_reset_witness() {
    let f = Native::new(false, -15.);
    let owner = owner(&f);
    let position = owner.position();
    let mut controls = Controls::default();
    controls.swim_reset_observed(11, position);
    controls.wait_for(Some(11), f.now, PendingKind::Move);
    controls.swim_receipt(11, position, None, owner.swim_basis());
    assert!(
        controls
            .swim_owner
            .begin(Some(11), position, Some((SwimInput::Idle, 0.)))
            .is_none()
    );
}

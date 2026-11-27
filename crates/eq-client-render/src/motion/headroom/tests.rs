use super::refused;
use crate::{
    Collision, Player, PlayerBody,
    motion::{Controls, input},
    online::{self, testing},
    outbox::Outbox,
};
use bevy::prelude::*;
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{
    ClientCommand, GameDialect, MovementMode, Posture, PostureState, WorldEvent,
    movement::{AirborneController, CollisionWorld},
    world_position,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const HUMAN: PlayerBody = PlayerBody {
    feet_offset: 3.75,
    height: 6.677_319_5,
    size: 6.0,
};

const OGRE: PlayerBody = PlayerBody {
    feet_offset: 5.625,
    height: 10.015_979,
    size: 9.0,
};

fn thin_lintel() -> CollisionWorld {
    let mut faces = plane(0.0, true);
    // The 10-unit Door62 opening, with a synthetic .1-unit underside.
    // Both endpoints of a 50/150 ms stride are outside its ray shadow.
    faces.extend([
        [
            [-20.0, 10.0, -0.05],
            [20.0, 10.0, 0.05],
            [-20.0, 10.0, 0.05],
        ],
        [
            [-20.0, 10.0, -0.05],
            [20.0, 10.0, -0.05],
            [20.0, 10.0, 0.05],
        ],
    ]);
    CollisionWorld::new(faces).unwrap()
}

fn thin_lintel_case(direction: f32, span: f32) {
    let from = Vec3::Z * -direction * 60.0 * span * 0.5;
    let to = -from;
    assert!(
        refused(
            InstalledClient::Titanium,
            &thin_lintel(),
            (&OGRE, 5.625),
            (from, to),
            (false, false)
        )
        .is_some(),
        "standing direction={direction} span={span}"
    );
    assert!(
        refused(
            InstalledClient::Titanium,
            &thin_lintel(),
            (&OGRE, 5.625),
            (from, to),
            (true, false)
        )
        .is_none(),
        "ducked direction={direction} span={span}"
    );
}

#[test]
fn headroom_thin_lintel_north_50ms() {
    thin_lintel_case(1.0, 0.05);
}
#[test]
fn headroom_thin_lintel_south_50ms() {
    thin_lintel_case(-1.0, 0.05);
}
#[test]
fn headroom_thin_lintel_north_150ms() {
    thin_lintel_case(1.0, 0.15);
}
#[test]
fn headroom_thin_lintel_south_150ms() {
    thin_lintel_case(-1.0, 0.15);
}

#[test]
fn headroom_held_input_retries_lintel_and_crosses_only_after_observed_duck() {
    for direction in [-1.0, 1.0] {
        for span in [0.05, 0.15] {
            for enabled in [false, true] {
                let (mut app, receiver) = app(InstalledClient::Titanium, 30.0);
                app.insert_resource(Collision(Some(thin_lintel())));
                let start = Vec3::new(0.0, 5.625, -direction * 60.0 * span * 0.5);
                {
                    let mut online = app.world_mut().resource_mut::<online::OnlineState>();
                    let mut player = online.world().player().unwrap().clone();
                    player.race = 10;
                    player.size = 9.0;
                    player.posture = Some(PostureState::Standing);
                    player.position = world_position(
                        start.to_array(),
                        eq_client_core::world_heading(if direction > 0.0 {
                            0.0
                        } else {
                            std::f32::consts::PI
                        }),
                    );
                    testing::admit(&mut online, 11, player);
                }
                {
                    let world = app.world_mut();
                    let mut player = world.query::<(&mut PlayerBody, &mut Transform)>();
                    let (mut body, mut transform) = player.single_mut(world).unwrap();
                    *body = OGRE;
                    transform.translation = start;
                    world
                        .query::<&mut crate::OrbitCamera>()
                        .single_mut(world)
                        .unwrap()
                        .yaw = if direction > 0.0 {
                        std::f32::consts::PI
                    } else {
                        0.0
                    };
                    let mut controls = world.resource_mut::<Controls>();
                    controls.test_span = Some(span);
                    controls.speed = Some(60.0);
                    controls.airborne = Some(AirborneController::default());
                    controls.climb_sent = None;
                }
                app.world_mut()
                    .resource_mut::<crate::options::OptionsState>()
                    .options
                    .allow_auto_stand = enabled;
                let mut crossed = false;
                for frame in 0..16 {
                    app.update();
                    let at = app
                        .world()
                        .resource::<online::OnlineState>()
                        .world()
                        .player()
                        .unwrap()
                        .position;
                    let z = eq_client_core::render_position(at)[2];
                    let ducked = app
                        .world()
                        .resource::<online::OnlineState>()
                        .world()
                        .posture(7)
                        == Some(PostureState::Ducking);
                    if z * direction > 0.05 {
                        assert!(
                            ducked && enabled,
                            "standing crossed: {direction} {span} {enabled} frame={frame}"
                        );
                        crossed = true;
                    }
                    let requests = accepted_commands(&mut app, &receiver);
                    if requests.contains(&Posture::Ducking) {
                        assert!(enabled);
                        posture(&mut app, PostureState::Ducking);
                    }
                }
                assert_eq!(crossed, enabled, "direction={direction} span={span}");
                if !enabled {
                    assert!(app.world().resource::<Controls>().headroom_refused);
                }
            }
        }
    }
}

#[test]
fn headroom_gallery_held_forward_stays_ducked_after_the_deadline() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
    app.insert_resource(Collision(Some(
        CollisionWorld::new(plane(4.0, true).into_iter().chain(plane(12.0, false))).unwrap(),
    )));
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position([0.0, 7.75, -10.0], 0.0),
    );
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now,
            );
            controls.test_span = Some(0.05);
            controls.speed = Some(10.0);
        });
    posture(&mut app, PostureState::Ducking);
    for millis in [2_001, 2_100, 2_200, 2_500, 3_000] {
        app.world_mut().resource_mut::<Controls>().test_now =
            Some(now + Duration::from_millis(millis));
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            Vec::<Posture>::new()
        );
    }
    assert_eq!(
        app.world()
            .resource::<online::OnlineState>()
            .world()
            .posture(7),
        Some(PostureState::Ducking)
    );
}

fn plane(y: f32, up: bool) -> Vec<[[f32; 3]; 3]> {
    let mut faces = vec![
        [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
        [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
    ];
    if !up {
        for face in &mut faces {
            face.swap(1, 2);
        }
    }
    faces
}

fn room(ceiling: f32) -> CollisionWorld {
    CollisionWorld::new(plane(0.0, true).into_iter().chain(plane(ceiling, false))).unwrap()
}

fn app(client: InstalledClient, ceiling: f32) -> (App, mpsc::Receiver<ClientCommand>) {
    let (mut app, _) = crate::motion::tests::fixture();
    let (sender, receiver) = mpsc::sync_channel(16);
    app.insert_resource(Outbox::new(Some(sender)))
        .insert_resource(Collision(Some(room(ceiling))))
        .add_systems(Update, (crate::posture::refresh, input).chain());
    app.world_mut()
        .resource_mut::<crate::ViewerSettings>()
        .0
        .installed_client = client;
    {
        let world = app.world_mut();
        let mut player = world.query::<(&mut PlayerBody, &mut Transform)>();
        let (mut body, mut transform) = player.single_mut(world).unwrap();
        *body = HUMAN;
        transform.translation = Vec3::Y * HUMAN.feet_offset;
    }
    let mut online = app.world_mut().resource_mut::<online::OnlineState>();
    testing::place(&mut online, world_position([0.0, 3.75, 0.0], 0.0));
    testing::generation(
        &mut online,
        if client == InstalledClient::EqMac {
            GameDialect::EqMac
        } else {
            GameDialect::Titanium
        },
    );
    testing::news(
        &mut online,
        [WorldEvent::Posture {
            spawn_id: 7,
            posture: PostureState::Standing,
        }],
    );
    (app, receiver)
}

fn posture(app: &mut App, posture: PostureState) {
    testing::news(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        [WorldEvent::Posture {
            spawn_id: 7,
            posture,
        }],
    );
}

fn commands(receiver: &mpsc::Receiver<ClientCommand>) -> Vec<Posture> {
    receiver
        .try_iter()
        .filter_map(|command| match command {
            ClientCommand::SetPosture { posture, .. } => Some(posture),
            _ => None,
        })
        .collect()
}

// Return is evaluated on a later own move. Feed the actual queued request
// back as a local worker submission so the next input pass can propose it.
fn accepted_commands(app: &mut App, receiver: &mpsc::Receiver<ClientCommand>) -> Vec<Posture> {
    let mut postures = Vec::new();
    for command in receiver.try_iter() {
        match command {
            ClientCommand::SetPosture { posture, .. } => postures.push(posture),
            ClientCommand::Move(request) => {
                testing::news(
                    &mut app.world_mut().resource_mut::<online::OnlineState>(),
                    [WorldEvent::MotionSent {
                        session_id: request.session_id,
                        created: request.created,
                        position: request.position,
                        refused: None,
                    }],
                );
                let mut controls = app.world_mut().resource_mut::<Controls>();
                assert!(controls.moved(Some(request.session_id), request.created));
                controls.last_accepted = controls
                    .now()
                    .checked_sub(Duration::from_millis(100))
                    .unwrap();
            }
            _ => (),
        }
    }
    postures
}

#[test]
fn actual_input_refuses_then_ducks_once_on_the_next_pass_for_keys_and_scripts() {
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        for enabled in [false, true] {
            for scripted in [false, true] {
                let (mut app, receiver) = app(client, 7.9);
                app.world_mut()
                    .resource_mut::<crate::options::OptionsState>()
                    .options
                    .allow_auto_stand = enabled;
                if scripted {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .reset_all();
                    crate::script::Script::press_keys_for_test(&mut app, &[KeyCode::KeyW]);
                }
                app.update();
                assert!(app.world().resource::<Controls>().headroom_refused);
                let first: Vec<_> = receiver.try_iter().collect();
                assert!(
                    !first
                        .iter()
                        .any(|command| matches!(command, ClientCommand::SetPosture { .. }))
                );
                assert!(first.iter().any(|command| matches!(command, ClientCommand::Move(request)
                    if request.position.x == 0.0 && request.position.y == 0.0 && request.position.z == 3.75)), "{first:?}");
                app.update();
                let expected = if enabled || client == InstalledClient::EqMac {
                    vec![Posture::Ducking]
                } else {
                    vec![]
                };
                assert_eq!(
                    commands(&receiver),
                    expected,
                    "{client:?} option={enabled} script={scripted}"
                );
                app.update();
                assert!(
                    commands(&receiver).is_empty(),
                    "no duplicate before an observed posture"
                );
                assert_eq!(
                    app.world()
                        .resource::<online::OnlineState>()
                        .world()
                        .posture(7),
                    Some(PostureState::Standing)
                );
            }
        }
    }
}

#[test]
fn automatic_return_checks_the_next_floor_before_standing_at_the_bank_boundary() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
    let mut triangles = plane(3.8, true);
    triangles.extend(plane(12.0, false));
    // The last two tenths of the approach rise onto the floor4 counter.
    // At the retained origin the gap8.2 passes return's 2S+r=8.15;
    // at the next candidate the gap8 does not. No threshold is changed.
    triangles.extend([
        [[-20.0, 4.0, 0.0], [-20.0, 4.0, 20.0], [20.0, 4.0, 20.0]],
        [[-20.0, 4.0, 0.0], [20.0, 4.0, 20.0], [20.0, 4.0, 0.0]],
    ]);
    app.insert_resource(Collision(Some(CollisionWorld::new(triangles).unwrap())));
    let origin = Vec3::new(0.0, 7.55, -0.5);
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position(origin.to_array(), 0.0),
    );
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now,
            );
            controls.test_now = Some(now + Duration::from_millis(2_001));
            controls.test_span = Some(0.05);
            controls.airborne = Some(AirborneController::default());
            controls.climb_sent = None;
            controls.speed = Some(120.0);
        });
    posture(&mut app, PostureState::Ducking);
    app.update();
    let sent: Vec<_> = receiver.try_iter().collect();
    assert!(
        !sent.iter().any(|command| matches!(
            command,
            ClientCommand::SetPosture {
                posture: Posture::Standing,
                ..
            }
        )),
        "a passing old origin must not stand before the low candidate: {sent:?}"
    );
    assert!(
        sent.iter().any(|command| matches!(command,
        ClientCommand::Move(request) if request.position.z > 7.7)),
        "{sent:?}"
    );
    assert!(!app.world().resource::<Controls>().headroom_refused);
}

#[test]
fn automatic_return_tests_the_manual_origin_bound_at_the_candidate_too() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 10.0);
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now.checked_sub(Duration::from_millis(2_001)).unwrap(),
            );
        });
    posture(&mut app, PostureState::Ducking);
    // DX9 returns .001 before the authored plane (0x1018f674), so the
    // compared ceiling is9.999. The origin-bound equality still permits.
    let bound = 10.0 - 0.001 - HUMAN.feet_offset;
    for (height, expected) in [(bound + 0.001, false), (bound, true)] {
        // Both pass the independent floor-to-ceiling return requirement.
        app.world_mut()
            .resource_scope(|world, mut controls: Mut<Controls>| {
                assert_eq!(
                    controls
                        .headroom
                        .drive(
                            InstalledClient::Titanium,
                            world.resource::<online::OnlineState>().world(),
                            world.resource::<Collision>().0.as_ref().unwrap(),
                            &HUMAN,
                            false,
                            world.resource::<Outbox>(),
                            now,
                            Some(Vec3::new(0.0, height, 1.0)),
                        )
                        .unwrap(),
                    expected
                );
            });
        assert_eq!(
            commands(&receiver),
            if expected {
                vec![Posture::Standing]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn automatic_return_preserves_the_existing_idle_origin_test() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now,
            );
            controls.test_now = Some(now + Duration::from_millis(2_001));
        });
    posture(&mut app, PostureState::Ducking);
    app.update();
    assert_eq!(commands(&receiver), vec![Posture::Standing]);
}

#[test]
fn automatic_return_waits_past_two_seconds_and_uses_the_native_return_predicate() {
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        let (mut app, receiver) = app(client, 7.9);
        let now = Instant::now();
        app.world_mut().resource_mut::<Controls>().test_now = Some(now);
        app.update();
        let _ = accepted_commands(&mut app, &receiver);
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            vec![Posture::Ducking]
        );
        posture(&mut app, PostureState::Ducking);
        app.insert_resource(Collision(Some(room(8.1))));
        for millis in [1_999, 2_000] {
            app.world_mut().resource_mut::<Controls>().test_now =
                Some(now + Duration::from_millis(millis));
            app.update();
            assert_eq!(
                accepted_commands(&mut app, &receiver),
                Vec::<Posture>::new()
            );
        }
        app.world_mut().resource_mut::<Controls>().test_now =
            Some(now + Duration::from_millis(2_001));
        app.update();
        if client == InstalledClient::EqMac {
            assert_eq!(
                accepted_commands(&mut app, &receiver),
                vec![Posture::Standing]
            );
        } else {
            // Manual standing fits at >8.0, but Titanium return needs >2S+r=8.15.
            assert_eq!(
                accepted_commands(&mut app, &receiver),
                Vec::<Posture>::new()
            );
            app.insert_resource(Collision(Some(room(8.2))));
            app.update();
            assert_eq!(
                accepted_commands(&mut app, &receiver),
                vec![Posture::Standing]
            );
        }
        app.update();
        assert_eq!(commands(&receiver), Vec::<Posture>::new());
        posture(&mut app, PostureState::Standing);
        app.update();
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
    }
}

#[test]
fn an_impossible_titanium_duck_clears_the_flag_and_sends_nothing() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.update();
    let _ = commands(&receiver);
    // Geometry changes before the next pass; Duck's own room test fails.
    app.insert_resource(Collision(Some(room(5.25))));
    app.update();
    assert_eq!(commands(&receiver), Vec::<Posture>::new());
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_none()
    );
}

#[test]
fn gallery_jumps_launch_after_windup_even_when_the_optional_stand_is_refused() {
    for (floor, expected) in [
        (0.0, eq_client_core::JumpPosture::StandIfDucked),
        (4.0, eq_client_core::JumpPosture::Keep),
    ] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
        app.insert_resource(Collision(Some(
            CollisionWorld::new(plane(floor, true).into_iter().chain(plane(12.0, false))).unwrap(),
        )));
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            world_position([0.0, floor + 3.75, 0.0], 0.0),
        );
        // An expired automatic cycle must not stand from this origin before
        // the finished windup reaches its independent grounded-launch test.
        let now = Instant::now();
        app.world_mut()
            .resource_scope(|world, mut controls: Mut<Controls>| {
                controls.headroom.refused(
                    InstalledClient::Titanium,
                    world.resource::<online::OnlineState>().world(),
                    Some(PostureState::Standing),
                    true,
                    now.checked_sub(Duration::from_millis(2_001)).unwrap(),
                );
                controls.test_now = Some(now);
            });
        posture(&mut app, PostureState::Ducking);
        {
            let mut controls = app.world_mut().resource_mut::<Controls>();
            controls.crouch = Some(crate::motion::Crouch::Until(now));
            controls.airborne = Some(AirborneController::default());
            controls.climb_sent = None;
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Space);
        }
        app.update();
        let sent: Vec<_> = receiver.try_iter().collect();
        assert!(
            sent.iter().any(|command| matches!(command,
            ClientCommand::Jump { posture, .. } if *posture == expected)),
            "floor={floor}, {sent:?}"
        );
        assert!(!app.world().resource::<Controls>().headroom_refused);
    }
}

#[test]
fn a_jump_without_starting_floor_is_silent_without_a_headroom_refusal() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position([0.0, 4.75, 0.0], 0.0),
    );
    {
        let now = Instant::now();
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.test_now = Some(now);
        controls.crouch = Some(crate::motion::Crouch::Until(now));
        controls.airborne = Some(AirborneController::default());
        controls.climb_sent = None;
    }
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Space);
    }
    app.update();
    let sent: Vec<_> = receiver.try_iter().collect();
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, ClientCommand::Jump { .. })),
        "{sent:?}"
    );
    assert!(!app.world().resource::<Controls>().headroom_refused);
}

#[test]
fn a_refused_jump_endpoint_keeps_the_eligible_launch_command() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.world_mut().resource_mut::<Controls>().airborne = Some(AirborneController::default());
    app.world_mut().resource_mut::<Controls>().climb_sent = None;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    let sent: Vec<_> = receiver.try_iter().collect();
    assert!(
        sent.iter()
            .any(|command| matches!(command, ClientCommand::Jump { .. })),
        "{sent:?}"
    );
    assert!(app.world().resource::<Controls>().headroom_refused);
    assert!(
        sent.iter()
            .any(|command| matches!(command, ClientCommand::Move(request)
        if request.position.x == 0.0 && request.position.y == 0.0 && request.position.z == 3.75)),
        "{sent:?}"
    );
}

#[test]
fn a_new_admission_or_lost_geometry_cannot_drive_an_old_automatic_cycle() {
    for lost_geometry in [false, true] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.update();
        let _ = commands(&receiver);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        if lost_geometry {
            app.insert_resource(Collision(None));
        } else {
            let mut online = app.world_mut().resource_mut::<online::OnlineState>();
            let player = online.world().player().unwrap().clone();
            testing::admit(&mut online, 12, player);
        }
        app.update();
        assert_eq!(commands(&receiver), Vec::<Posture>::new());
        let automatic = &app.world().resource::<Controls>().headroom.automatic;
        if lost_geometry {
            assert!(automatic.is_none());
        } else {
            // An unchanged stop proposal may itself refuse and arm a NEW
            // admission's cycle. It must not retain or drive session11's.
            assert!(
                automatic
                    .as_ref()
                    .is_none_or(|automatic| automatic.session == 12)
            );
        }
    }
}

#[test]
fn an_intervening_posture_does_not_leave_a_stale_wait_or_queue_another_action() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.update();
    let _ = commands(&receiver);
    app.update();
    assert_eq!(commands(&receiver), vec![Posture::Ducking]);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    posture(&mut app, PostureState::Sitting);
    app.update();
    assert_eq!(commands(&receiver), Vec::<Posture>::new());
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_none()
    );
}

#[test]
fn a_ducked_refusal_with_the_option_off_arms_only_macs_return() {
    // Exercise the refusal-to-posture contract separately from the capsule:
    // a failed ducked strict-room query arms Mac even though already ducked.
    for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
        let (mut app, receiver) = app(client, 8.1);
        posture(&mut app, PostureState::Ducking);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .allow_auto_stand = false;
        let now = Instant::now();
        assert!(
            refused(
                client,
                &room(4.25),
                (&HUMAN, 3.75),
                (Vec3::ZERO, Vec3::X),
                (true, false)
            )
            .is_some()
        );
        app.world_mut()
            .resource_scope(|world, mut controls: Mut<Controls>| {
                controls.headroom.refused(
                    client,
                    world.resource::<online::OnlineState>().world(),
                    Some(PostureState::Ducking),
                    false,
                    now,
                );
                controls.test_now = Some(now + Duration::from_millis(2_001));
            });
        app.update();
        assert_eq!(
            commands(&receiver),
            if client == InstalledClient::EqMac {
                vec![Posture::Standing]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn a_launch_stand_report_precedes_automatic_duck() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    posture(&mut app, PostureState::Ducking);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let now = Instant::now();
    app.world_mut()
        .resource_scope(|world, mut controls: Mut<Controls>| {
            controls.headroom.refused(
                InstalledClient::Titanium,
                world.resource::<online::OnlineState>().world(),
                Some(PostureState::Standing),
                true,
                now,
            );
        });
    app.update();
    assert_eq!(commands(&receiver), Vec::<Posture>::new());
    posture(&mut app, PostureState::Standing);
    app.update();
    assert_eq!(commands(&receiver), vec![Posture::Ducking]);
}

#[test]
fn the_bank_sill_is_tested_after_stepping_and_restores_the_airborne_snapshot() {
    // Synthetic boundary from the measured floor4 / underside12 counter.
    // 12.1 is an invented just-over control, not another measured doorway
    // or an official-client traversal. The fast grant isolates gate order.
    for span in [0.01, 1.0 / 60.0, 0.05] {
        for ceiling in [12.0, 12.1] {
            let (mut app, receiver) = app(InstalledClient::Titanium, ceiling);
            let mut triangles = plane(0.0, true);
            triangles.extend(plane(ceiling, false));
            // A four-unit sill and its leading face. Standing's unchanged
            // raised sweep clears it; the candidate floor is then four up.
            triangles.extend([
                [[-20.0, 4.0, 0.0], [-20.0, 4.0, 20.0], [20.0, 4.0, 20.0]],
                [[-20.0, 4.0, 0.0], [20.0, 4.0, 20.0], [20.0, 4.0, 0.0]],
                [[-20.0, 0.0, 0.0], [-20.0, 4.0, 0.0], [20.0, 4.0, 0.0]],
                [[-20.0, 0.0, 0.0], [20.0, 4.0, 0.0], [20.0, 0.0, 0.0]],
            ]);
            app.insert_resource(Collision(Some(CollisionWorld::new(triangles).unwrap())));
            // A synthetic fast grant crosses the whole face within even
            // the .01-second sample, starting outside capsule contact.
            // This isolates post-step gating from initial wall overlap;
            // it does not calibrate a human's ordinary walking speed.
            let speed = 120.0;
            let start = Vec3::new(0.0, 3.75, -speed * span * 0.5);
            testing::place(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                world_position(start.to_array(), 0.0),
            );
            {
                let world = app.world_mut();
                world
                    .query_filtered::<&mut Transform, With<Player>>()
                    .single_mut(world)
                    .unwrap()
                    .translation = start;
                let mut controls = world.resource_mut::<Controls>();
                controls.test_span = Some(span);
                controls.speed = Some(speed);
                controls.airborne = Some(AirborneController::default());
                controls.climb_sent = None;
            }
            app.update();
            let moved: Vec<_> = receiver
                .try_iter()
                .filter_map(|command| match command {
                    ClientCommand::Move(request) => Some(Vec3::from_array(
                        eq_client_core::render_position(request.position),
                    )),
                    _ => None,
                })
                .collect();
            assert_eq!(moved.len(), 1, "{span} {ceiling}: {moved:?}");
            let controls = app.world().resource::<Controls>();
            if ceiling == 12.0 {
                assert!(controls.headroom_refused, "{span}: {moved:?}");
                assert!(moved[0].abs_diff_eq(start, 1e-5), "{moved:?}");
                assert_eq!(controls.airborne.as_ref().unwrap().velocity(), 0.0);
                assert!(controls.airborne.as_ref().unwrap().peak().is_none());
            } else {
                assert!(!controls.headroom_refused);
                assert!(
                    moved[0].z > start.z && (moved[0].y - 7.75).abs() < 0.01,
                    "{span}: {moved:?}"
                );
            }
        }
    }
}

#[test]
fn ducked_room_and_mount_precedence_remain_per_generation() {
    let from = Vec3::ZERO;
    let to = Vec3::X;
    for (client, threshold) in [
        (InstalledClient::Titanium, 5.25),
        (InstalledClient::EqMac, 4.25),
    ] {
        assert!(
            refused(
                client,
                &room(threshold),
                (&HUMAN, 3.75),
                (from, to),
                (true, true)
            )
            .is_some()
        );
        assert!(
            refused(
                client,
                // Titanium's actual floor-probe return includes +.001.
                // Keep the positive control beyond that read bias too.
                &room(threshold + 0.01),
                (&HUMAN, 3.75),
                (from, to),
                (true, true)
            )
            .is_none()
        );
    }
    assert!(
        refused(
            InstalledClient::Titanium,
            &room(11.75),
            (&HUMAN, 3.75),
            (from, to),
            (false, true)
        )
        .is_some()
    );
    assert!(
        refused(
            InstalledClient::Titanium,
            &room(5.0),
            (&HUMAN, 3.75),
            (from, from),
            (false, false)
        )
        .is_some()
    );
}

#[derive(Clone)]
struct Submerged;

impl eq_client_core::movement::Liquids for Submerged {
    fn liquid_at(&self, _: Vec3) -> Option<eq_client_core::movement::Liquid> {
        Some(eq_client_core::movement::Liquid::Water)
    }
    fn passes_through(&self, _: Vec3, _: Vec3, liquid: eq_client_core::movement::Liquid) -> bool {
        liquid == eq_client_core::movement::Liquid::Water
    }
    fn first_outside(
        &self,
        _: Vec3,
        _: Vec3,
        _: eq_client_core::movement::Liquid,
    ) -> Option<std::ops::Range<f64>> {
        None
    }
    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn eq_client_core::movement::Liquids>> {
        Some(Box::new(self.clone()))
    }
}

#[test]
fn underwater_forward_and_retreat_keep_the_existing_kedge_exemption() {
    // A synthetic control for the retained exemption, not a Kedge asset or
    // full-passage result. The room clears the capsule but fails standing.
    for key in [KeyCode::KeyW, KeyCode::KeyS] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.insert_resource(Collision(Some(room(7.9).with_liquids(Submerged))));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        let sent: Vec<_> = receiver.try_iter().collect();
        assert!(
            sent.iter()
                .any(|command| matches!(command, ClientCommand::Move(request)
            if request.mode == MovementMode::Swim && request.position.x.abs() > 0.01)),
            "{key:?}: {sent:?}"
        );
        assert!(!app.world().resource::<Controls>().headroom_refused);
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
    }
}

#[test]
fn headroom_pending_duck_is_withdrawn_before_water_input() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    app.update();
    assert!(app.world().resource::<Controls>().headroom.automatic.is_some());
    accepted_commands(&mut app, &receiver);
    app.insert_resource(Collision(Some(room(7.9).with_liquids(Submerged))));
    app.update();
    assert_eq!(accepted_commands(&mut app, &receiver), Vec::<Posture>::new());
    let controls = app.world().resource::<Controls>();
    assert!(controls.headroom.automatic.is_none());
    assert!(!controls.headroom_retry);
    assert!(!controls.headroom_refused);
}

#[test]
fn headroom_server_placement_withdraws_pending_and_ducked_cycles_even_at_same_xyz() {
    for ducked in [false, true] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.update();
        assert!(app.world().resource::<Controls>().headroom.automatic.is_some());
        accepted_commands(&mut app, &receiver);
        if ducked { posture(&mut app, PostureState::Ducking); }
        let position = app.world().resource::<online::OnlineState>().world().player().unwrap().position;
        testing::place(&mut app.world_mut().resource_mut::<online::OnlineState>(), position);
        app.insert_resource(Collision(Some(room(30.0))));
        app.update();
        assert_eq!(accepted_commands(&mut app, &receiver), Vec::<Posture>::new());
        let controls = app.world().resource::<Controls>();
        assert!(controls.headroom.automatic.is_none(), "ducked={ducked}");
        assert!(!controls.headroom_retry);
    }
}

#[test]
fn headroom_ordinary_ledge_above_a_distant_floor_still_falls() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 30.0);
    let ledge = [
        [[-20.0, 0.0, -20.0], [-20.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        [[-20.0, 0.0, -20.0], [20.0, 0.0, 0.0], [20.0, 0.0, -20.0]],
    ];
    app.insert_resource(Collision(Some(CollisionWorld::new(
        ledge.into_iter().chain(plane(-1000.0, true)),
    ).unwrap())));
    testing::place(&mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position([0.0, 3.75, -1.0], 0.0));
    app.world_mut().query::<&mut crate::OrbitCamera>().single_mut(app.world_mut()).unwrap().yaw = std::f32::consts::PI;
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.test_span = Some(0.05);
        controls.speed = Some(60.0);
        controls.airborne = Some(AirborneController::default());
    }
    for _ in 0..8 {
        app.update();
        assert_eq!(accepted_commands(&mut app, &receiver), Vec::<Posture>::new());
        assert!(!app.world().resource::<Controls>().headroom_refused);
    }
    let at = app.world().resource::<online::OnlineState>().world().player().unwrap().position;
    assert!(at.z < 3.75, "{at:?}");
    assert!(eq_client_core::render_position(at)[2] > 0.0, "{at:?}");
}

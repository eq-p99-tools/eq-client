use super::*;
use crate::{
    motion,
    online::{OnlineState, testing},
};
use eq_client_core::{ClientCommand, SpawnState, WorldEvent, qol::Fix};

mod cadence;

fn spawn(distance: f32) -> SpawnState {
    SpawnState {
        name: "MeleeFixture".into(),
        kind: SpawnKind::Npc,
        race: 1,
        size: 6.,
        pet_owner: None,
        position: WorldPosition {
            y: distance,
            z: 3.,
            ..WorldPosition::default()
        },
        ..testing::pet(5, 0)
    }
}

fn select(online: &mut OnlineState, revision: u64) {
    let request_id = online.world().next_target_request() - 1;
    testing::news(
        online,
        [WorldEvent::TargetChanged {
            session_id: 11,
            request_id,
            state: eq_network_game::targets::TargetState {
                revision,
                spawn_id: Some(5),
                available: true,
            },
        }],
    );
}

fn world(distance: f32) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.race = 1;
    player.size = 6.;
    player.position = WorldPosition {
        z: 3.,
        ..WorldPosition::default()
    };
    testing::admit(&mut online, 11, player);
    testing::spawn_entry(&mut online, 5, spawn(distance));
    select(&mut online, 1);
    assert!(online.world().target_intent().is_some());
    online
}

fn plane() -> eq_client_core::movement::CollisionWorld {
    eq_client_core::movement::CollisionWorld::new([
        [[-300., 0., -300.], [300., 0., 300.], [300., 0., -300.]],
        [[-300., 0., -300.], [-300., 0., 300.], [300., 0., 300.]],
    ])
    .unwrap()
}

fn body(online: &OnlineState) -> Body {
    Body {
        position: online.world().player().unwrap().position,
        height: 6.,
        offset: 3.,
        seconds: 0.1,
        speeds: (6., 3.),
        walking: false,
    }
}

fn move_target(online: &mut OnlineState, distance: f32) {
    testing::news(
        online,
        [WorldEvent::Position {
            spawn_id: 5,
            position: WorldPosition {
                y: distance,
                z: 3.,
                ..WorldPosition::default()
            },
            velocity: [0.; 3],
            forward: 0.,
        }],
    );
}

#[test]
fn melee_range_reuses_sizes_and_turns_without_advancing_inside_reach() {
    let online = world(10.);
    let mut body = body(&online);
    body.position.heading = 128.;
    let step = Assist::default()
        .step(online.world(), &plane(), &body, Instant::now())
        .unwrap();
    assert!(!step.forward);
    assert!(
        super::super::arc(body.position.heading, step.heading).abs()
            <= motion::TURN_RATE * body.seconds + 1e-4
    );
    assert!(step.heading < body.position.heading);
    assert!(step.beyond.abs() < 1e-5);
    let small = abilities::Body {
        race: 1,
        size: 6.,
        x: 0.,
        y: 0.,
    };
    let large = abilities::Body { size: 10., ..small };
    assert!((reach(small, small) - 12.).abs() < 0.004);
    assert!((reach(small, large) - 20.).abs() < 0.004);
}

#[test]
fn melee_approach_stops_inside_reach_and_hysteresis_avoids_edge_jitter() {
    let mut online = world(30.);
    let body = body(&online);
    let floor = plane();
    let mut assist = Assist::default();
    let now = Instant::now();
    assert!(
        assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 10.5);
    assert!(
        !assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 11.8);
    assert!(
        !assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 12.2);
    assert!(
        assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
    assert!(
        !Assist::default()
            .step(
                online.world(),
                &floor,
                &Body {
                    walking: true,
                    ..body
                },
                now
            )
            .unwrap()
            .run
    );
}

#[test]
fn melee_identity_loss_corpse_and_pending_selection_never_drive() {
    let mut online = world(30.);
    let body = body(&online);
    let mut assist = Assist::default();
    let now = Instant::now();
    assert!(assist.step(online.world(), &plane(), &body, now).is_some());
    online.select_target(Some(5));
    assert!(assist.step(online.world(), &plane(), &body, now).is_none());
    select(&mut online, 2);
    assert!(assist.step(online.world(), &plane(), &body, now).is_some());
    testing::spawn_entry(&mut online, 5, spawn(30.));
    assert!(assist.step(online.world(), &plane(), &body, now).is_none());
    let mut corpse = spawn(30.);
    corpse.kind = SpawnKind::NpcCorpse;
    testing::spawn_entry(&mut online, 5, corpse);
    select(&mut online, 3);
    assert!(assist.step(online.world(), &plane(), &body, now).is_none());
}

#[test]
fn melee_target_change_pauses_until_the_new_current_target_is_authoritative() {
    let mut online = world(30.);
    let body = body(&online);
    let floor = plane();
    let now = Instant::now();
    let mut assist = Assist::default();
    assert!(
        assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );

    let mut next = spawn(0.);
    next.spawn_id = 6;
    next.position.x = 30.;
    testing::spawn_entry(&mut online, 6, next);
    online.select_target(Some(6));
    assert!(assist.step(online.world(), &floor, &body, now).is_none());
    assert!(!assist.driving());
    let request_id = online.world().next_target_request() - 1;
    testing::news(
        &mut online,
        [WorldEvent::TargetChanged {
            session_id: 11,
            request_id,
            state: eq_network_game::targets::TargetState {
                revision: 2,
                spawn_id: Some(6),
                available: true,
            },
        }],
    );
    let turning = assist.step(online.world(), &floor, &body, now).unwrap();
    assert_eq!(assist.scope.unwrap().target_id, 6);
    assert!(
        !turning.forward,
        "turn toward the new target before approaching"
    );
    assert!(motion::arc(body.position.heading, turning.heading).abs() > 0.);
    online.select_target(None);
    assert!(assist.step(online.world(), &floor, &body, now).is_none());
}

#[test]
fn melee_distance_limit_stops_above_200_and_can_resume_within_range() {
    let mut online = world(200.);
    let body = body(&online);
    let floor = plane();
    let now = Instant::now();
    let mut assist = Assist::default();
    assert!(
        assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
    move_target(&mut online, 200.01);
    assert!(assist.step(online.world(), &floor, &body, now).is_none());
    assert!(!assist.driving());
    move_target(&mut online, 190.);
    assert!(
        assist
            .step(online.world(), &floor, &body, now)
            .unwrap()
            .forward
    );
}

#[test]
fn melee_obstruction_stops_and_manual_input_keeps_the_release_gap() {
    let online = world(30.);
    let body = body(&online);
    let floor = plane();
    let wall = eq_client_core::movement::CollisionWorld::new([
        [[-300., 0., -300.], [300., 0., 300.], [300., 0., -300.]],
        [[-300., 0., -300.], [-300., 0., 300.], [300., 0., 300.]],
        [[5., 0., -20.], [5., 20., 20.], [5., 0., 20.]],
        [[5., 0., -20.], [5., 20., -20.], [5., 20., 20.]],
    ])
    .unwrap();
    let now = Instant::now();
    let mut assist = Assist::default();
    assert!(
        !assist
            .step(online.world(), &wall, &body, now)
            .unwrap()
            .forward
    );
    assist.yield_input(now);
    assert!(
        assist
            .step(
                online.world(),
                &floor,
                &body,
                now + Duration::from_millis(499)
            )
            .is_none()
    );
    assert!(
        assist
            .step(online.world(), &floor, &body, now + MANUAL_PAUSE)
            .unwrap()
            .forward
    );
}

fn runtime() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (mut app, rx) = motion::tests::fixture();
    app.insert_resource(world(30.))
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::hud::hotbar::Bindings>()
        .init_resource::<Time>()
        .add_systems(
            Update,
            (crate::options::toggle, crate::combat::input, motion::input).chain(),
        );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    crate::clicks::testing::install(&mut app);
    let now = Instant::now();
    {
        let mut controls = app.world_mut().resource_mut::<motion::Controls>();
        controls.test_now = Some(now);
        controls.test_span = Some(0.1);
        controls.walk_speed = Some(3.);
        controls.last_accepted = now.checked_sub(Duration::from_millis(100)).unwrap();
    }
    app.world_mut()
        .resource_mut::<crate::outbox::Outbox>()
        .set_test_time(now);
    (app, rx)
}

fn moves(rx: &std::sync::mpsc::Receiver<ClientCommand>) -> Vec<eq_client_core::MovementRequest> {
    rx.try_iter()
        .filter_map(|command| match command {
            ClientCommand::Move(request) => Some(request),
            ClientCommand::AutoAttack { .. } => None,
            other => panic!("melee positioning generated a non-movement request: {other:?}"),
        })
        .collect()
}

#[test]
fn melee_ui_option_and_normal_attack_button_drive_one_bounded_move() {
    let (mut app, rx) = runtime();
    let option = app
        .world_mut()
        .spawn((
            Interaction::None,
            crate::options::OptionCheckbox(eq_client_core::options::Toggle::Qol(
                Fix::MeleePositioning,
            )),
        ))
        .id();
    assert!(
        !app.world()
            .resource::<crate::options::OptionsState>()
            .options
            .qol
            .on(Fix::MeleePositioning)
    );
    crate::clicks::testing::click(&mut app, option);
    assert!(
        app.world()
            .resource::<crate::options::OptionsState>()
            .options
            .qol
            .on(Fix::MeleePositioning)
    );
    assert_eq!(moves(&rx), Vec::new());
    let attack = app
        .world_mut()
        .spawn((Interaction::None, crate::skinned::AttackButton))
        .id();
    crate::clicks::testing::press(&mut app, Some(attack));
    assert_eq!(moves(&rx), Vec::new());
    crate::clicks::testing::release(&mut app, Some(attack));
    let commands: Vec<_> = rx.try_iter().collect();
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, ClientCommand::AutoAttack { enabled: true, .. }))
            .count(),
        1
    );
    let moving: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            ClientCommand::Move(request) => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(moving.len(), 1);
    let endpoint = moving[0].position;
    assert!(endpoint.y > 0. && endpoint.y <= 0.601);
    assert!(endpoint.x.abs() < 1e-4);
    assert!(matches!(
        moving[0].mode,
        eq_client_core::MovementMode::Forward
    ));
    app.update();
    assert_eq!(
        moves(&rx),
        Vec::new(),
        "one actual proposal stays in flight"
    );
}

#[test]
fn melee_manual_movement_rmb_scripts_and_capability_loss_keep_normal_input() {
    for reason in 0..5 {
        let (mut app, rx) = runtime();
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::MeleePositioning, true);
        match reason {
            0 => app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW),
            1 => {
                app.insert_resource(crate::camera::MouseLook {
                    held: true,
                    left: false,
                });
            }
            2 => {
                app.insert_resource(crate::script::Script::new(Vec::new()).paused());
            }
            3 => {
                let mut online = world(30.);
                let player = online.world().player().unwrap().clone();
                testing::enter_with(
                    &mut online,
                    11,
                    player,
                    None,
                    vec![
                        eq_client_core::Capability::Moving,
                        eq_client_core::Capability::Combat,
                    ],
                );
                testing::spawn_entry(&mut online, 5, spawn(30.));
                select(&mut online, 1);
                app.insert_resource(online);
            }
            _ => {
                app.world_mut()
                    .resource_mut::<crate::options::OptionsState>()
                    .options
                    .qol
                    .set(Fix::MeleePositioning, false);
            }
        }
        let attack = app
            .world_mut()
            .spawn((Interaction::None, crate::skinned::AttackButton))
            .id();
        crate::clicks::testing::click(&mut app, attack);
        let sent = moves(&rx);
        if reason == 0 {
            assert_eq!(sent.len(), 1);
            assert!(
                sent[0].position.x < 0.,
                "the player's camera-relative W keeps its own direction"
            );
        } else {
            assert_eq!(
                sent,
                Vec::new(),
                "automatic input leaked past owner/gate {reason}"
            );
        }
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one Receive/Input/Present lifecycle matrix"
)]
fn melee_received_target_loss_both_deaths_zone_and_off_stop_new_samples() {
    use crate::motion::latency_tests;
    for reason in 0..6 {
        let (mut app, rx, news) = latency_tests::timing_app();
        app.insert_resource(world(30.))
            .init_resource::<crate::combat::CombatState>()
            .init_resource::<crate::hud::hotbar::Bindings>()
            .add_systems(
                Update,
                crate::combat::input
                    .before(motion::input)
                    .in_set(crate::systems::Stage::Input),
            );
        crate::keys::testing::bind(
            &mut app,
            crate::keys::Act::Attack,
            crate::keys::Chord::key(KeyCode::F10),
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .qol
            .set(Fix::MeleePositioning, true);
        let mut now = Instant::now();
        {
            let mut controls = app.world_mut().resource_mut::<motion::Controls>();
            controls.last_accepted = now.checked_sub(Duration::from_millis(100)).unwrap();
            controls.last_sent = None;
            controls.test_span = Some(0.1);
            controls.walk_speed = Some(3.);
        }
        let frame = |app: &mut App, now| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(100));
            app.world_mut().resource_mut::<motion::Controls>().test_now = Some(now);
            app.world_mut()
                .resource_mut::<crate::outbox::Outbox>()
                .set_test_time(now);
            app.update();
        };
        latency_tests::key(&mut app, KeyCode::F10, true);
        frame(&mut app, now);
        let first = moves(&rx)
            .pop()
            .expect("normal attack press starts bounded positioning");
        news.send(eq_client_core::WorldUpdate::Game(WorldEvent::MotionSent {
            session_id: 11,
            created: first.created,
            position: first.position,
            refused: None,
        }))
        .unwrap();
        latency_tests::key(&mut app, KeyCode::F10, false);
        match reason {
            0 => news
                .send(eq_client_core::WorldUpdate::Game(
                    WorldEvent::TargetChanged {
                        session_id: 11,
                        request_id: 0,
                        state: eq_network_game::targets::TargetState {
                            revision: 2,
                            spawn_id: None,
                            available: true,
                        },
                    },
                ))
                .unwrap(),
            1 => latency_tests::key(&mut app, KeyCode::F10, true),
            2 => news
                .send(eq_client_core::WorldUpdate::Game(WorldEvent::Death(
                    eq_client_core::Death {
                        spawn_id: 7,
                        killer_id: 5,
                        corpse_id: 77,
                        bind_zone_id: 1,
                        corpse_name: None,
                    },
                )))
                .unwrap(),
            3 => {
                let player = app
                    .world()
                    .resource::<OnlineState>()
                    .world()
                    .player()
                    .unwrap()
                    .clone();
                news.send(eq_client_core::WorldUpdate::Game(WorldEvent::Entered {
                    capabilities: eq_client_core::Capability::ALL.to_vec(),
                    choices: Vec::new(),
                    session_id: 12,
                    zone: "qeytoqrg".into(),
                    player: Box::new(player),
                    far_clip: None,
                    spell_landing_flag: None,
                    zone_id: 1,
                }))
                .unwrap();
            }
            4 => app
                .world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(Fix::MeleePositioning, false),
            _ => news
                .send(eq_client_core::WorldUpdate::Game(WorldEvent::Death(
                    eq_client_core::Death {
                        spawn_id: 5,
                        killer_id: 7,
                        corpse_id: 5,
                        bind_zone_id: 0,
                        corpse_name: None,
                    },
                )))
                .unwrap(),
        }
        for _ in 0..3 {
            now += Duration::from_millis(100);
            frame(&mut app, now);
            for stop in moves(&rx) {
                let delta = Vec3::from_array(eq_client_core::render_position(stop.position))
                    - Vec3::from_array(eq_client_core::render_position(first.position));
                assert!(
                    delta.length() < 1e-5,
                    "positioning outlived lifecycle boundary {reason}: {stop:?}"
                );
            }
        }
    }
}

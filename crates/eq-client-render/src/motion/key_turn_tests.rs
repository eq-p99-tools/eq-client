//! Physical default bindings through Receive/Input/Present and ordinary admission.
use super::*;
use eq_client_core::{GameDialect, WorldEvent, WorldUpdate};
use eq_network_game::movement::{MotionSession, TITANIUM_FLAGS, UpdateGate};
use std::{collections::VecDeque, sync::mpsc};

#[derive(Clone, Copy)]
struct Row {
    heading: f32,
    yaw: f32,
    position: Vec3,
    pitch: f32,
    radius: f32,
}

struct Rig {
    app: App,
    requests: mpsc::Receiver<ClientCommand>,
    news: mpsc::Sender<WorldUpdate>,
    owner: MotionSession,
    replies: VecDeque<(
        usize,
        MovementRequest,
        eq_client_core::WorldPosition,
        Option<String>,
    )>,
    packets: Vec<(usize, eq_client_core::WorldPosition)>,
    frame: usize,
    base: Instant,
    delay: usize,
    carriers: eq_client_core::movement::Carriers,
    withdraw_next: bool,
    refused_results: usize,
    diagonal_requests: usize,
}

fn plane(y: f32) -> [[[f32; 3]; 3]; 2] {
    [
        [[-1000., y, -1000.], [-1000., y, 1000.], [1000., y, -1000.]],
        [[1000., y, -1000.], [-1000., y, 1000.], [1000., y, 1000.]],
    ]
}

fn calibration() -> eq_client_core::MotionCalibration {
    eq_client_core::MotionCalibration {
        units_per_second: 6.,
        velocity_scale: 1.,
        animation: 12,
        swim: None,
        swim_backward_units_per_second: None,
        diagonal: DiagonalRates::default(),
        strafe: None,
        walk: None,
        backward: Some(eq_client_core::BackwardCalibration {
            units_per_second: 4.,
            velocity_scale: 1.,
            animation: -8,
        }),
    }
}

impl Rig {
    #[allow(
        clippy::too_many_lines,
        reason = "one ordinary-session fixture with explicit static carrier setup"
    )]
    fn new(boat: bool, delay: usize, negative: bool) -> Self {
        let (mut app, requests, news) = latency_tests::timing_app();
        app.insert_resource(KeyMap::default())
            .add_message::<bevy::window::WindowFocused>()
            .add_systems(
                PreUpdate,
                crate::camera::look.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                crate::camera::turn
                    .before(input)
                    .in_set(crate::systems::Stage::Input),
            );
        let origin = world_position([0., 3., 0.], 511.5);
        let base = Instant::now() + Duration::from_secs(60);
        let start = base.checked_sub(Duration::from_secs(1)).unwrap();
        {
            let world = app.world_mut();
            online::testing::place(&mut world.resource_mut::<online::OnlineState>(), origin);
            world
                .query_filtered::<&mut Transform, With<Player>>()
                .single_mut(world)
                .unwrap()
                .rotation = Quat::from_rotation_y(eq_client_core::render_heading(origin.heading));
            world
                .query::<&mut OrbitCamera>()
                .single_mut(world)
                .unwrap()
                .yaw = crate::camera::behind(eq_client_core::render_heading(origin.heading));
            let mut controls = world.resource_mut::<Controls>();
            controls.backward_speed = Some(4.);
            controls.last_accepted = start;
            controls.suppress_key_facing = negative;
        }
        let mut carriers = eq_client_core::movement::Carriers::default();
        if boat {
            let mut ship = online::testing::pet(172, 7);
            ship.race = 72;
            ship.pet_owner = None;
            ship.position = eq_client_core::WorldPosition::default();
            let spawned = WorldEvent::Spawns(vec![ship]);
            let changes = carriers.observe(&spawned, GameDialect::Titanium, start);
            let sample = carriers.latest(172).unwrap().sample();
            online::testing::news(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                [spawned, WorldEvent::Carriers(changes)],
            );
            app.world_mut().resource_mut::<Controls>().boat = Some(boats::Riding {
                sample,
                pose: eq_client_core::WorldPosition::default(),
                position: origin,
            });
            let mut collision = CollisionWorld::new(plane(-30.)).unwrap();
            collision.set_spawn_obstacles(vec![(
                172,
                std::sync::Arc::new(
                    eq_client_core::movement::CollisionMesh::new(plane(0.)).unwrap(),
                ),
            )]);
            app.insert_resource(Collision(Some(collision)));
        }
        let mut owner = MotionSession::new(11, 7, origin, start)
            .unwrap()
            .with_heading_snapshots(true)
            .with_gate(Some(UpdateGate::Flagged {
                pack: |packet| packet.packet(),
                rule: TITANIUM_FLAGS,
            }));
        owner.calibrate(calibration(), start).unwrap();
        owner.set_climb(app.world().resource::<Controls>().climb_sent.unwrap().1);
        let mut rig = Self {
            app,
            requests,
            news,
            owner,
            replies: VecDeque::new(),
            packets: Vec::new(),
            frame: 0,
            base,
            delay,
            carriers,
            withdraw_next: false,
            refused_results: 0,
            diagonal_requests: 0,
        };
        for _ in 0..5 {
            rig.tick();
        }
        rig
    }

    fn key(&mut self, code: KeyCode, held: bool) {
        latency_tests::key(&mut self.app, code, held);
    }

    fn tick(&mut self) -> Row {
        let now = self.base + Duration::from_nanos(16_666_667) * u32::try_from(self.frame).unwrap();
        while self
            .replies
            .front()
            .is_some_and(|(due, _, _, _)| *due <= self.frame)
        {
            let (_, request, position, refused) = self.replies.pop_front().unwrap();
            self.refused_results += usize::from(refused.is_some());
            self.news
                .send(WorldUpdate::Game(WorldEvent::MotionSent {
                    session_id: request.session_id,
                    created: request.created,
                    position,
                    refused,
                }))
                .unwrap();
        }
        self.app.world_mut().resource_mut::<Controls>().test_now = Some(now);
        self.app
            .world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(now);
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_nanos(16_666_667));
        self.app.update();
        for command in self.requests.try_iter() {
            if let ClientCommand::Move(request) = command {
                self.diagonal_requests +=
                    usize::from(matches!(request.mode, MovementMode::GroundedDiagonal(_)));
                let carrier = match request.support {
                    Some(Support::Spawn(sample)) => self
                        .app
                        .world()
                        .resource::<online::OnlineState>()
                        .world()
                        .carriers()
                        .pose(sample, request.created)
                        .map(|pose| (sample, pose)),
                    _ => None,
                };
                self.owner.allow_carrier(carrier);
                let withdrawn = std::mem::take(&mut self.withdraw_next);
                if withdrawn {
                    self.owner.correct(self.owner.position(), now).unwrap();
                }
                let packets = &mut self.packets;
                let frame = self.frame;
                let result = self.owner.send_move_sample(&request, now, |packet| {
                    packets.push((frame, packet.position));
                    Ok(())
                });
                let refused = if withdrawn {
                    let reason = result.unwrap_err().to_string();
                    // correct() revokes calibration. The deliberate one-request
                    // denial is over; restore the identical ordinary grant,
                    // without granting another movement interval.
                    self.owner.calibrate(calibration(), now).unwrap();
                    Some(reason)
                } else {
                    result.unwrap();
                    None
                };
                self.replies.push_back((
                    self.frame + self.delay + 1,
                    request,
                    self.owner.position(),
                    refused,
                ));
            }
        }
        let packets = &mut self.packets;
        let frame = self.frame;
        self.owner
            .tick_sample(now, |packet| {
                packets.push((frame, packet.position));
                Ok(())
            })
            .unwrap();
        let world = self.app.world_mut();
        let player = *world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .unwrap();
        let camera = world.query::<&OrbitCamera>().single(world).unwrap();
        let forward = player.rotation * Vec3::Z;
        let row = Row {
            heading: eq_client_core::world_heading(forward.x.atan2(forward.z)),
            yaw: camera.yaw,
            position: player.translation,
            pitch: camera.pitch,
            radius: camera.radius,
        };
        self.frame += 1;
        row
    }
}

fn still(before: Row, after: Row) {
    assert!(
        arc(before.heading, after.heading).abs() < 0.000_1,
        "actor continued turning"
    );
    let yaw = after.yaw - before.yaw;
    assert!(
        yaw.sin().atan2(yaw.cos()).abs() < 0.000_1,
        "chase continued turning"
    );
    assert_eq!(before.pitch.to_bits(), after.pitch.to_bits());
    assert_eq!(before.radius.to_bits(), after.radius.to_bits());
}

#[test]
fn turn_release_stops_the_actor_and_chase_at_every_sample_phase_on_foot_and_boat() {
    for boat in [false, true] {
        for delay in [0, 6] {
            for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight] {
                for travel in [None, Some(KeyCode::ArrowUp), Some(KeyCode::ArrowDown)] {
                    for held in [29, 32, 35] {
                        let mut rig = Rig::new(boat, delay, false);
                        rig.key(code, true);
                        if let Some(key) = travel {
                            rig.key(key, true);
                        }
                        let mut before = rig.tick();
                        for _ in 1..held {
                            before = rig.tick();
                        }
                        if boat {
                            assert!(
                                rig.app
                                    .world()
                                    .resource::<Controls>()
                                    .boat
                                    .is_some_and(|boat| boat.sample.spawn_id == 172)
                            );
                        }
                        rig.key(code, false);
                        for _ in 0..60 {
                            still(before, rig.tick());
                        }
                        assert!(arc(before.heading, rig.owner.position().heading).abs() < 0.001);
                        for pair in rig.packets.windows(2) {
                            assert!(pair[1].0 - pair[0].0 >= 15, "packet cadence increased");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn turn_release_negative_control_has_both_the_tail_and_extra_accepted_frame() {
    for boat in [false, true] {
        let mut endings = Vec::new();
        for negative in [true, false] {
            let mut rig = Rig::new(boat, 6, negative);
            rig.key(KeyCode::ArrowLeft, true);
            let mut before = rig.tick();
            for _ in 1..32 {
                before = rig.tick();
            }
            rig.key(KeyCode::ArrowLeft, false);
            let mut maximum = 0_f32;
            for _ in 0..60 {
                maximum = maximum.max(arc(before.heading, rig.tick().heading).abs());
            }
            if negative {
                assert!(maximum > 24.);
            } else {
                assert!(maximum < 0.000_1);
            }
            endings.push(rig.owner.position().heading);
        }
        let expected = (511.5 + ramped_turn(0., 32. / 60., 12.)).rem_euclid(512.);
        assert!(arc(expected, endings[1]).abs() < 0.001);
        let excess = arc(endings[1], endings[0]);
        // The old sample-window partition changes how much of the inclusive
        // release frame is charged. The independent held-frame endpoint is
        // fixed; excess is positive and at most one capped60Hz frame.
        assert!(
            excess > 0.001 && excess <= TURN_RATE / 60. + 0.001,
            "legacy released-slice excess: {endings:?}"
        );
    }
}

#[test]
fn turn_release_repress_resets_the_ramp_without_playing_an_old_heading() {
    for boat in [false, true] {
        let mut rig = Rig::new(boat, 6, false);
        rig.key(KeyCode::ArrowRight, true);
        let mut before = rig.tick();
        for _ in 1..32 {
            before = rig.tick();
        }
        rig.key(KeyCode::ArrowRight, false);
        for _ in 0..3 {
            still(before, rig.tick());
        }
        rig.key(KeyCode::ArrowLeft, true);
        let pressed = rig.tick();
        assert!((arc(before.heading, pressed.heading) - 1. / 3.).abs() < 0.000_1);
        rig.key(KeyCode::ArrowLeft, false);
        for _ in 0..60 {
            still(pressed, rig.tick());
        }
        assert!(arc(pressed.heading, rig.owner.position().heading).abs() < 0.001);
    }
}

#[test]
fn turn_release_plain_forward_and_back_translation_and_reporting_are_unchanged() {
    for code in [KeyCode::ArrowUp, KeyCode::ArrowDown] {
        let mut controls = Vec::new();
        for negative in [true, false] {
            let mut rig = Rig::new(false, 6, negative);
            rig.key(code, true);
            let mut positions = Vec::new();
            for _ in 0..32 {
                positions.push(rig.tick().position);
            }
            rig.key(code, false);
            for _ in 0..60 {
                positions.push(rig.tick().position);
            }
            controls.push((positions, rig.packets));
        }
        assert_eq!(controls[0], controls[1]);
    }
}

#[test]
fn turn_release_a_server_correction_replaces_the_frozen_keyboard_facing() {
    for boat in [false, true] {
        let mut rig = Rig::new(boat, 6, false);
        rig.key(KeyCode::ArrowLeft, true);
        for _ in 0..32 {
            rig.tick();
        }
        rig.key(KeyCode::ArrowLeft, false);
        for _ in 0..60 {
            rig.tick();
        }
        // These ordinary results were already admitted; the fixture delays
        // delivery only. Deliver them before the later authoritative placement.
        while let Some((_, request, position, refused)) = rig.replies.pop_front() {
            rig.news
                .send(WorldUpdate::Game(WorldEvent::MotionSent {
                    session_id: request.session_id,
                    created: request.created,
                    position,
                    refused,
                }))
                .unwrap();
        }
        let correction = world_position([20., 3., -40.], 73.25);
        rig.owner
            .correct(
                correction,
                rig.base + Duration::from_nanos(16_666_667) * u32::try_from(rig.frame).unwrap(),
            )
            .unwrap();
        rig.owner
            .calibrate(
                calibration(),
                rig.base + Duration::from_nanos(16_666_667) * u32::try_from(rig.frame).unwrap(),
            )
            .unwrap();
        rig.news
            .send(WorldUpdate::Game(WorldEvent::Position {
                spawn_id: 7,
                position: correction,
                velocity: [0.; 3],
                forward: 0.,
            }))
            .unwrap();
        let row = rig.tick();
        assert!(arc(row.heading, correction.heading).abs() < 0.000_1);
        assert_eq!(row.position, Vec3::new(20., 3., -40.));
        assert!(rig.app.world().resource::<Controls>().key_facing.is_none());
    }
}

#[test]
fn turn_release_natural_carrier_yaw_is_not_a_continued_key_turn() {
    let mut rig = Rig::new(true, 6, false);
    rig.key(KeyCode::ArrowLeft, true);
    let mut before = rig.tick();
    for _ in 1..32 {
        before = rig.tick();
    }
    rig.key(KeyCode::ArrowLeft, false);
    for _ in 0..60 {
        still(before, rig.tick());
    }
    let event = WorldEvent::Position {
        spawn_id: 172,
        position: eq_client_core::WorldPosition {
            heading: 16.,
            ..Default::default()
        },
        velocity: [0.; 3],
        forward: 0.,
    };
    let changes = rig
        .carriers
        .observe(&event, GameDialect::Titanium, Instant::now());
    rig.news.send(WorldUpdate::Game(event)).unwrap();
    rig.news
        .send(WorldUpdate::Game(WorldEvent::Carriers(changes)))
        .unwrap();
    let moved = rig.tick();
    assert!((arc(before.heading, moved.heading) - 16.).abs() < 0.000_1);
    for _ in 0..30 {
        still(moved, rig.tick());
    }
}

#[test]
fn turn_release_a_worker_result_refusal_retires_the_held_facing_on_foot_and_boat() {
    for boat in [false, true] {
        let mut rig = Rig::new(boat, 6, false);
        rig.withdraw_next = true;
        rig.key(KeyCode::ArrowLeft, true);
        rig.tick();
        rig.key(KeyCode::ArrowLeft, false);
        let mut row = rig.tick();
        for _ in 0..60 {
            if rig.refused_results > 0 {
                break;
            }
            row = rig.tick();
        }
        assert_eq!(
            rig.refused_results, 1,
            "fixture must receive the actual denied proposal's result"
        );
        assert!(rig.app.world().resource::<Controls>().key_facing.is_none());
        assert!(arc(row.heading, rig.owner.position().heading).abs() < 0.000_1);
        assert!(
            rig.packets
                .iter()
                .all(|(_, position)| arc(position.heading, 511.5).abs() < 0.000_1)
        );
        for _ in 0..60 {
            still(row, rig.tick());
        }
    }
}

#[test]
fn turn_release_keeps_the_facing_when_forward_and_strafe_resume_diagonal() {
    for boat in [false, true] {
        let mut rig = Rig::new(boat, 6, false);
        crate::keys::testing::bind(
            &mut rig.app,
            Act::StrafeRight,
            crate::keys::Chord::key(KeyCode::KeyL),
        );
        let mut rates = calibration();
        rates.diagonal = DiagonalRates {
            run: Some(3.),
            walk: None,
        };
        rates.strafe = Some(eq_client_core::StrafeCalibration {
            units_per_second: 3.,
            velocity_scale: 1.,
            animation: 8,
            walk_units_per_second: None,
        });
        let now = rig.base + Duration::from_nanos(16_666_667) * u32::try_from(rig.frame).unwrap();
        rig.owner.calibrate(rates, now).unwrap();
        {
            let mut controls = rig.app.world_mut().resource_mut::<Controls>();
            controls.diagonal_rates = rates.diagonal;
            controls.strafe_speed = Some(3.);
        }
        rig.key(KeyCode::ArrowLeft, true);
        rig.key(KeyCode::ArrowUp, true);
        rig.key(KeyCode::KeyL, true);
        let mut held = rig.tick();
        for _ in 1..32 {
            held = rig.tick();
        }
        rig.key(KeyCode::ArrowLeft, false);
        for _ in 0..60 {
            still(held, rig.tick());
        }
        assert!(
            rig.diagonal_requests > 0,
            "fixture must resume actual diagonal proposals"
        );
        assert!(arc(held.heading, rig.owner.position().heading).abs() < 0.001);
    }
}

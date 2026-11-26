//! Actual input/receive/present with the unchanged ordinary session guard.
use super::*;
use eq_client_core::movement::{Liquid, Liquids};
use eq_network_game::movement::{GroundSampleResult, MotionSession, TITANIUM_FLAGS, UpdateGate};
use std::{collections::VecDeque, sync::mpsc};

struct Dry;
impl Liquids for Dry {
    fn liquid_at(&self, _: Vec3) -> Option<Liquid> {
        None
    }
    fn passes_through(&self, _: Vec3, _: Vec3, _: Liquid) -> bool {
        false
    }
    fn segment_known(&self, _: Vec3, _: Vec3) -> bool {
        true
    }
    fn within(&self, _: Vec3, _: Vec3) -> Option<Box<dyn Liquids>> {
        Some(Box::new(Self))
    }
}

#[derive(Clone, Copy, Debug)]
struct Row {
    shown: Vec3,
    predicted: Vec3,
    focus: Vec3,
    camera: Vec3,
    owned: Vec3,
    forward: bool,
    accepted: eq_client_core::WorldPosition,
}

struct ResultNews {
    due: u32,
    created: Instant,
    position: eq_client_core::WorldPosition,
    refused: Option<String>,
}

struct Scene {
    app: App,
    commands: mpsc::Receiver<ClientCommand>,
    news: mpsc::Sender<eq_client_core::WorldUpdate>,
    owner: MotionSession,
    base: Instant,
    frame: u32,
    delay: u32,
    replies: VecDeque<ResultNews>,
    packets: Vec<(Instant, eq_client_core::WorldPosition, i16)>,
    observations: usize,
    admissions: usize,
    refusals: usize,
    early_refuse: bool,
}

impl Scene {
    fn new(delay: u32, known: bool) -> Self {
        let (mut app, commands, news) = latency_tests::timing_app();
        // Physical Forward deliberately differs from the fixture's camera-W.
        crate::keys::testing::bind(
            &mut app,
            Act::Forward,
            crate::keys::Chord::key(KeyCode::ArrowUp),
        );
        let plane = vec![
            [
                [-2000., 0., -2000.],
                [-2000., 0., 2000.],
                [2000., 0., -2000.],
            ],
            [[2000., 0., -2000.], [-2000., 0., 2000.], [2000., 0., 2000.]],
        ];
        let collision = CollisionWorld::new(plane).unwrap();
        let collision = if known {
            collision.with_liquids(Dry)
        } else {
            collision
        };
        app.insert_resource(Collision(Some(collision)));
        let base = Instant::now();
        let start = base.checked_sub(Duration::from_secs(1)).unwrap();
        app.world_mut().resource_mut::<Controls>().last_accepted = start;
        let origin = app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position;
        let mut owner = MotionSession::new(11, 7, origin, start)
            .unwrap()
            .with_heading_snapshots(true)
            .with_gate(Some(UpdateGate::Flagged {
                pack: |packet| packet.packet(),
                rule: TITANIUM_FLAGS,
            }));
        owner
            .calibrate(
                eq_client_core::MotionCalibration {
                    units_per_second: 6.,
                    velocity_scale: 1.,
                    animation: 12,
                    swim: None,
                    swim_backward_units_per_second: None,
                    diagonal: DiagonalRates::default(),
                    strafe: None,
                    walk: None,
                    backward: None,
                },
                start,
            )
            .unwrap();
        let mut scene = Self {
            app,
            commands,
            news,
            owner,
            base,
            frame: 0,
            delay,
            replies: VecDeque::new(),
            packets: Vec::new(),
            observations: 0,
            admissions: 0,
            refusals: 0,
            early_refuse: false,
        };
        scene.step(false);
        scene
    }

    fn now(&self) -> Instant {
        self.base + Duration::from_nanos(u64::from(self.frame) * 16_666_667)
    }

    fn reply(&mut self, created: Instant, refused: Option<String>) {
        self.replies.push_back(ResultNews {
            due: self.frame + self.delay + 1,
            created,
            position: self.owner.position(),
            refused,
        });
    }

    fn publish(&mut self, command: ClientCommand, now: Instant) {
        match command {
            ClientCommand::GroundFrame { request, state } => {
                if self.early_refuse {
                    self.news
                        .send(eq_client_core::WorldUpdate::Game(
                            eq_client_core::WorldEvent::MotionRefused {
                                session_id: 11,
                                created: request.created,
                                reason: "outside action admission".into(),
                            },
                        ))
                        .unwrap();
                    return;
                }
                self.observations += 1;
                let packets = &mut self.packets;
                let result = self
                    .owner
                    .try_send_ground_sample(&request, state, now, |packet| {
                        packets.push((now, packet.position, packet.animation));
                        Ok(())
                    });
                if matches!(result, Ok(GroundSampleResult::WaitingForInterval)) {
                    self.owner
                        .observe_ground_input(
                            request.session_id,
                            request.created,
                            state.forward(),
                            now,
                        )
                        .unwrap();
                } else if let Err(error) = result {
                    self.refusals += 1;
                    self.reply(request.created, Some(error.to_string()));
                } else {
                    assert!(matches!(result, Ok(GroundSampleResult::Accepted)));
                    self.admissions += 1;
                    self.reply(request.created, None);
                }
            }
            ClientCommand::Move(request) => {
                let packets = &mut self.packets;
                self.owner
                    .send_move_sample(&request, now, |packet| {
                        packets.push((now, packet.position, packet.animation));
                        Ok(())
                    })
                    .unwrap();
                self.reply(request.created, None);
            }
            _ => (),
        }
    }

    fn step(&mut self, held: bool) -> Row {
        self.frame += 1;
        let now = self.now();
        while self
            .replies
            .front()
            .is_some_and(|reply| reply.due <= self.frame)
        {
            let reply = self.replies.pop_front().unwrap();
            self.news
                .send(eq_client_core::WorldUpdate::Game(
                    eq_client_core::WorldEvent::MotionSent {
                        session_id: 11,
                        created: reply.created,
                        position: reply.position,
                        refused: reply.refused,
                    },
                ))
                .unwrap();
        }
        let was_held = self
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ArrowUp);
        if held != was_held {
            latency_tests::key(&mut self.app, KeyCode::ArrowUp, held);
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
        while let Ok(command) = self.commands.try_recv() {
            self.publish(command, now);
        }
        let packets = &mut self.packets;
        self.owner
            .tick_sample(now, |packet| {
                packets.push((now, packet.position, packet.animation));
                Ok(())
            })
            .unwrap();
        let (shown, focus) = latency_tests::shown(&mut self.app);
        let current = self.app.world().resource::<Controls>().ground.current;
        let world = self.app.world_mut();
        let camera = world
            .query_filtered::<&Transform, With<OrbitCamera>>()
            .single(world)
            .unwrap()
            .translation;
        let row = Row {
            shown,
            predicted: current.map_or(shown, |frame| frame.drawn.translation),
            focus,
            camera,
            owned: current.map_or(Vec3::ZERO, |frame| frame.motion.velocity()),
            forward: current.is_some_and(|frame| frame.state.forward()),
            accepted: self.owner.position(),
        };
        if std::env::var_os("EQ_LAND_TRACE").is_some() {
            println!("land-frame frame={} held={held} {row:?}", self.frame);
        }
        row
    }
}

#[test]
fn native_ground_release_uses_the_release_frame_and_receipts_never_replay_distance() {
    for delay in [0, 2, 6] {
        for held in [1_u16, 2, 5, 7, 13] {
            let mut scene = Scene::new(delay, true);
            let before = latency_tests::shown(&mut scene.app).0;
            let mut prior = scene.step(true);
            assert!(prior.shown.distance(before) > 0.001, "cold first frame");
            for _ in 1..held {
                prior = scene.step(true);
            }
            let release_pose = prior.shown;
            let mut prior_step = f32::INFINITY;
            for _ in 0..40 {
                let row = scene.step(false);
                assert!(!row.forward, "release did not remove actual input");
                assert!(row.owned.length() <= prior.owned.length() + 0.000_001);
                let step = row.shown.distance(prior.shown);
                assert!(
                    step <= prior_step + 0.000_01,
                    "release/receipt accelerated the display: {row:?}"
                );
                assert_eq!(row.shown, row.focus);
                assert_eq!(row.shown, row.predicted);
                assert!(
                    ((row.camera - prior.camera) - (row.shown - prior.shown)).length() < 0.000_01,
                    "camera introduced a separate release step"
                );
                prior_step = step;
                prior = row;
            }
            assert_eq!(prior.owned, Vec3::ZERO);
            let stopped = prior.shown;
            for _ in 0..30 {
                assert_eq!(scene.step(false).shown, stopped);
            }
            assert!(stopped.distance(before) <= 6. * (f32::from(held) / 60.) + 0.001);
            assert_eq!(scene.refusals, 0);
            let accepted = Vec3::from_array(eq_client_core::render_position(prior.accepted));
            assert!(
                stopped.distance(accepted) < 0.001,
                "final stop never admitted"
            );
            println!(
                "delay={delay} held={held} release={release_pose:?} stop={stopped:?} coast={} observations={} admitted={}",
                stopped.distance(release_pose),
                scene.observations,
                scene.admissions
            );
        }
    }
}

#[test]
fn native_ground_repress_retains_only_owned_velocity_and_new_actual_input() {
    for delay in [0, 6] {
        for gap in [1, 3, 12, 60] {
            let mut scene = Scene::new(delay, true);
            let before = latency_tests::shown(&mut scene.app).0;
            for _ in 0..2 {
                scene.step(true);
            }
            for _ in 0..gap {
                scene.step(false);
            }
            if gap == 60 {
                let controls = scene.app.world().resource::<Controls>();
                assert!(
                    controls.ground.old_timeout(scene.now()),
                    "negative control lacks stale waiting identity"
                );
                assert!(
                    !controls.ground.timed_out(scene.now()),
                    "an admitted rest became a new unresolved prefix"
                );
            }
            for _ in 0..11 {
                scene.step(true);
                assert!(
                    scene
                        .app
                        .world()
                        .resource::<Controls>()
                        .ground
                        .current
                        .is_some(),
                    "re-press left the native controller"
                );
            }
            let mut last = scene.step(false);
            for _ in 0..50 {
                last = scene.step(false);
            }
            assert!(last.shown.distance(before) <= 6. * (13. / 60.) + 0.001);
            assert_eq!(last.owned, Vec3::ZERO);
            assert_eq!(scene.step(false).shown, last.shown);
            assert_eq!(scene.refusals, 0);
        }
    }
}

#[test]
fn native_ground_a_subframe_tap_never_invents_held_duration() {
    let mut scene = Scene::new(0, true);
    let before = latency_tests::shown(&mut scene.app).0;
    latency_tests::key(&mut scene.app, KeyCode::ArrowUp, true);
    latency_tests::key(&mut scene.app, KeyCode::ArrowUp, false);
    let row = scene.step(false);
    assert_eq!(row.shown, before);
    assert_eq!(row.owned, Vec3::ZERO);
    assert!(!row.forward);
    assert_eq!(scene.observations, 0);
}

#[test]
fn native_ground_paused_translation_keeps_the_independent_mouse_heading_preview() {
    let mut scene = Scene::new(6, true);
    for _ in 0..8 {
        scene.step(true);
    }
    let before = latency_tests::shown(&mut scene.app).0;
    {
        let mut controls = scene.app.world_mut().resource_mut::<Controls>();
        controls.walking = true;
        controls.walk_speed = Some(3.);
        controls.mouse_heading = Some(37.125);
    }
    let row = scene.step(true);
    assert_eq!(
        row.shown, before,
        "unsupported Walk advanced native translation"
    );
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .ground
            .current
            .is_some()
    );
    let world = scene.app.world_mut();
    let drawn = world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap();
    let expected = Quat::from_rotation_y(eq_client_core::render_heading(37.125));
    assert!(drawn.rotation.angle_between(expected) < 0.000_01);
    let camera = world.query::<&OrbitCamera>().single(world).unwrap();
    assert!(
        (camera.yaw - crate::camera::behind(eq_client_core::render_heading(37.125))).abs()
            < 0.000_01
    );
}

#[test]
fn native_ground_ten_seconds_keeps_held_travel_and_ordinary_report_cadence() {
    let mut scene = Scene::new(0, true);
    let before = latency_tests::shown(&mut scene.app).0;
    let mut last = scene.step(true);
    for _ in 1..600 {
        last = scene.step(true);
    }
    let distance = last.shown.distance(before);
    assert!(
        (distance - 60.).abs() / 60. < 0.04,
        "ten-second native acceleration: {distance}"
    );
    for pair in scene.packets.windows(2) {
        assert!(pair[1].0.duration_since(pair[0].0) > TITANIUM_FLAGS.spacing);
    }
    assert!(
        scene.packets.len() <= 35,
        "per-frame observations increased wire cadence"
    );
    assert!(scene.observations > scene.admissions && scene.admissions > scene.packets.len());
    assert_eq!(scene.refusals, 0);
    println!(
        "10s held distance={distance} packets={} observations={} admissions={}",
        scene.packets.len(),
        scene.observations,
        scene.admissions
    );
}

#[test]
fn unknown_geometry_negative_control_retains_the_old_projected_tap_tail() {
    let mut scene = Scene::new(0, false);
    let before = latency_tests::shown(&mut scene.app).0;
    let release = scene.step(true).shown;
    let mut stop = release;
    for _ in 0..60 {
        stop = scene.step(false).shown;
    }
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .ground
            .current
            .is_none()
    );
    assert!(
        stop.distance(release) > 0.4,
        "negative control did not reproduce projected tail"
    );
    assert!((stop.distance(before) - 0.6).abs() < 0.01);
}

#[test]
fn native_ground_real_correction_rolls_back_in_receive_and_retires_the_simulation() {
    let mut scene = Scene::new(6, true);
    for _ in 0..8 {
        scene.step(true);
    }
    let corrected = world_position([0., 3., -2.], 80.);
    scene.owner.correct(corrected, scene.now()).unwrap();
    scene
        .news
        .send(eq_client_core::WorldUpdate::Game(
            eq_client_core::WorldEvent::Position {
                spawn_id: 7,
                position: corrected,
                velocity: [0.; 3],
                forward: 0.,
            },
        ))
        .unwrap();
    // Preserve the real worker's FIFO: already accepted results precede the
    // correction, never an invented late acceptance of its old coordinates.
    scene.replies.clear();
    let row = scene.step(false);
    let actual = Vec3::from_array(eq_client_core::render_position(corrected));
    assert_eq!(row.shown, actual);
    assert_eq!(row.focus, actual);
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .ground
            .current
            .is_none_or(|frame| {
                frame.motion.velocity() == Vec3::ZERO && frame.position == corrected
            })
    );
    for _ in 0..10 {
        assert_eq!(scene.step(false).shown, actual);
    }
}

#[test]
fn native_ground_a_full_command_queue_restores_the_genuinely_accepted_pose() {
    let mut scene = Scene::new(0, true);
    for _ in 0..8 {
        scene.step(true);
    }
    let (sender, _receiver) = mpsc::sync_channel(0);
    scene
        .app
        .insert_resource(crate::outbox::Outbox::new(Some(sender)));
    let row = scene.step(false);
    let actual = Vec3::from_array(eq_client_core::render_position(
        scene
            .app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position,
    ));
    assert_eq!(row.shown, actual);
    assert_eq!(row.focus, actual);
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .ground
            .current
            .is_none()
    );
    assert!(scene.app.world().resource::<Controls>().preview.is_none());
}

#[test]
fn native_ground_guard_and_pre_admission_refusals_restore_observed_state() {
    for early in [false, true] {
        let mut scene = Scene::new(0, true);
        let before = latency_tests::shown(&mut scene.app).0;
        scene
            .app
            .world_mut()
            .resource_mut::<Controls>()
            .mouse_heading = Some(37.125);
        scene.step(true);
        if early {
            scene.early_refuse = true;
        } else {
            scene
                .owner
                .set_hold(eq_network_game::movement::Hold::Stunned);
        }
        scene.step(false);
        let row = scene.step(false);
        assert_eq!(row.shown, before);
        assert_eq!(row.focus, before);
        assert!(
            scene
                .app
                .world()
                .resource::<Controls>()
                .ground
                .current
                .is_none()
        );
        assert!(scene.app.world().resource::<Controls>().refused.is_some());
        assert_eq!(
            scene.app.world().resource::<Controls>().mouse_heading,
            Some(37.125)
        );
        if !early {
            assert_eq!(scene.refusals, 1);
        }
    }
}

#[test]
fn native_ground_unknown_contact_hands_back_only_after_the_observed_prefix_is_admitted() {
    for delay in [0, 6] {
        let mut scene = Scene::new(delay, true);
        for _ in 0..8 {
            scene.step(true);
        }
        let held_pose = latency_tests::shown(&mut scene.app).0;
        scene.app.insert_resource(Collision(Some(
            CollisionWorld::new([
                [
                    [-2000., 0., -2000.],
                    [-2000., 0., 2000.],
                    [2000., 0., -2000.],
                ],
                [[2000., 0., -2000.], [-2000., 0., 2000.], [2000., 0., 2000.]],
            ])
            .unwrap(),
        )));
        let mut handed_back = None;
        for frame in 1..=30 {
            let row = scene.step(true);
            if scene
                .app
                .world()
                .resource::<Controls>()
                .ground
                .current
                .is_some()
            {
                assert_eq!(
                    row.shown, held_pose,
                    "unknown contact advanced native simulation"
                );
            } else if row.shown.distance(held_pose) > 0.001 {
                handed_back = Some(frame);
                break;
            }
        }
        let frames = handed_back.expect("bounded handback to existing ordinary path");
        assert!(frames <= 20);
        let mut previous = latency_tests::shown(&mut scene.app).0;
        for _ in 0..30 {
            let row = scene.step(true);
            assert!(
                row.shown.x >= previous.x - 0.000_01,
                "late native receipt rewound legacy playback"
            );
            assert!(
                row.shown.distance(previous) <= 6. / 60. + 0.000_01,
                "handback caught up a frozen interval"
            );
            previous = row.shown;
        }
        assert_eq!(scene.refusals, 0);
        println!(
            "unknown-contact delay={delay} handback_frames={frames} pause_ms={}",
            f64::from(frames) * 1000. / 60.
        );
    }
}

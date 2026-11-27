//! One virtual monotonic clock for input, request creation and local-result delivery.
use super::*;
use std::collections::VecDeque;

const RUN: f32 = 29.866_667;
const WALK: f32 = 12.800_001;

#[derive(Resource, Default)]
struct ReceivedAt(Option<Instant>);

fn receive_clock(mut controls: ResMut<motion::Controls>, mut received: ResMut<ReceivedAt>) {
    if let Some(at) = received.0.take() {
        // Receive normally uses Instant::now. Align only its test observation
        // timestamp with this fixture's injected request/frame clock.
        controls.last_accepted = at;
    }
}

#[derive(Debug)]
struct Measurement {
    path: f32,
    net: f32,
    samples: usize,
    writes: usize,
    first_ms: u128,
    minimum_spacing_ms: u128,
    maximum_spacing_ms: u128,
    maximum_arrival_rate: f32,
}

#[allow(
    clippy::too_many_lines,
    reason = "one ordinary frame/request/local-result clock fixture"
)]
fn ten_seconds(assisted: bool, delays: &[u64]) -> Measurement {
    use crate::motion::latency_tests;
    use eq_network_game::movement::{MotionSession, TITANIUM_FLAGS, UpdateGate};
    let (mut app, requests, news) = latency_tests::timing_app();
    app.insert_resource(world(100.))
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::hud::hotbar::Bindings>()
        .init_resource::<ReceivedAt>()
        .add_systems(
            Update,
            (
                crate::combat::input.before(motion::input),
                receive_clock.before(motion::input),
            )
                .in_set(crate::systems::Stage::Input),
        );
    crate::keys::testing::bind(
        &mut app,
        crate::keys::Act::Attack,
        crate::keys::Chord::key(KeyCode::F10),
    );
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, assisted);
    let start = Instant::now();
    {
        let mut controls = app.world_mut().resource_mut::<motion::Controls>();
        controls.test_now = Some(start);
        controls.test_span = None;
        controls.last_accepted = start;
        controls.last_sent = None;
        controls.speed = Some(RUN);
        controls.walk_speed = Some(WALK);
    }
    {
        let world = app.world_mut();
        let mut cameras = world.query::<&mut crate::camera::OrbitCamera>();
        cameras.single_mut(world).unwrap().yaw =
            crate::camera::behind(eq_client_core::render_heading(0.));
    }
    let origin = app
        .world()
        .resource::<OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    let mut session = MotionSession::new(11, 7, origin, start)
        .unwrap()
        .with_heading_snapshots(true)
        .with_gate(Some(UpdateGate::Flagged {
            pack: |sample| sample.packet(),
            rule: TITANIUM_FLAGS,
        }));
    session
        .calibrate(
            eq_client_core::MotionCalibration {
                units_per_second: RUN,
                velocity_scale: 1.,
                animation: 1,
                backward: None,
                walk: None,
                strafe: None,
                diagonal: eq_client_core::DiagonalRates::default(),
                swim: None,
                swim_backward_units_per_second: None,
            },
            start,
        )
        .unwrap();
    latency_tests::key(&mut app, KeyCode::F10, true);
    if !assisted {
        latency_tests::key(&mut app, KeyCode::KeyW, true);
    }
    let mut elapsed = Duration::ZERO;
    let mut pending = VecDeque::new();
    let mut samples = Vec::new();
    let mut path = 0.;
    let mut total_writes = 0;
    let mut last_created = start;
    let mut last_arrival = start;
    let mut maximum_arrival_rate = 0_f32;
    while elapsed < Duration::from_secs(10) {
        let frame = Duration::from_millis([8, 12, 16, 33][samples.len() % 4]);
        let step = frame.min(Duration::from_secs(10).checked_sub(elapsed).unwrap());
        elapsed += step;
        let now = start + elapsed;
        while pending.front().is_some_and(|(due, _, _)| *due <= now) {
            let (_, request, distance): (Instant, eq_client_core::MovementRequest, f32) =
                pending.pop_front().unwrap();
            maximum_arrival_rate =
                maximum_arrival_rate.max(distance / now.duration_since(last_arrival).as_secs_f32());
            last_arrival = now;
            news.send(eq_client_core::WorldUpdate::Game(WorldEvent::MotionSent {
                session_id: 11,
                created: request.created,
                position: request.position,
                refused: None,
            }))
            .unwrap();
            app.world_mut().resource_mut::<ReceivedAt>().0 = Some(now);
        }
        // A current target moving ahead keeps both matched runs out of the
        // stopping margin for the entire ten seconds; it is not an input list.
        news.send(eq_client_core::WorldUpdate::Game(WorldEvent::Position {
            spawn_id: 5,
            position: WorldPosition {
                y: 100. + RUN * elapsed.as_secs_f32(),
                z: 3.,
                ..WorldPosition::default()
            },
            velocity: [0.; 3],
            forward: 0.,
        }))
        .unwrap();
        app.world_mut().resource_mut::<Time>().advance_by(step);
        app.world_mut().resource_mut::<motion::Controls>().test_now = Some(now);
        app.world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(now);
        app.update();
        for command in requests.try_iter() {
            let request = match command {
                ClientCommand::Move(request) => request,
                ClientCommand::AutoAttack { .. } => continue,
                other => panic!("unexpected clock-fixture request: {other:?}"),
            };
            assert!(pending.is_empty(), "one actual request may be in flight");
            let before = session.position();
            let distance = (request.position.x - before.x).hypot(request.position.y - before.y);
            let seconds = request.created.duration_since(last_created).as_secs_f32();
            assert!(
                distance <= RUN * seconds + 0.001,
                "{distance} exceeds {RUN} * {seconds}"
            );
            let mut writes = 0;
            session
                .send_move_sample(&request, now, |_| {
                    writes += 1;
                    Ok(())
                })
                .unwrap();
            total_writes += writes;
            assert_eq!(session.position(), request.position);
            path += distance;
            last_created = request.created;
            samples.push(request.created);
            let delay = Duration::from_millis(delays[(samples.len() - 1) % delays.len()]);
            pending.push_back((now + delay, request, distance));
        }
    }
    let end = session.position();
    let spacing: Vec<_> = samples
        .windows(2)
        .map(|pair| pair[1].duration_since(pair[0]).as_millis())
        .collect();
    assert!(path <= RUN * 10. + 0.01);
    Measurement {
        path,
        net: (end.x - origin.x).hypot(end.y - origin.y),
        samples: samples.len(),
        writes: total_writes,
        first_ms: samples[0].duration_since(start).as_millis(),
        minimum_spacing_ms: *spacing.iter().min().unwrap(),
        maximum_spacing_ms: *spacing.iter().max().unwrap(),
        maximum_arrival_rate,
    }
}

#[test]
fn melee_ten_second_request_path_and_cap_match_manual_run_with_result_jitter() {
    for delays in [&[0][..], &[40][..], &[120][..], &[80, 0][..]] {
        let manual = ten_seconds(false, delays);
        let assisted = ten_seconds(true, delays);
        println!(
            "ten simulated seconds delays_ms={delays:?} manual={manual:?} assisted={assisted:?}"
        );
        assert_eq!(manual.samples, assisted.samples);
        assert!(assisted.writes > 0 && assisted.writes <= assisted.samples);
        assert_eq!(manual.first_ms, assisted.first_ms);
        assert_eq!(manual.minimum_spacing_ms, assisted.minimum_spacing_ms);
        assert_eq!(manual.maximum_spacing_ms, assisted.maximum_spacing_ms);
        assert!((manual.path - assisted.path).abs() < 0.03);
        assert!((assisted.net - assisted.path).abs() < 0.01);
        if delays.len() == 2 {
            assert!(
                assisted.maximum_arrival_rate > RUN,
                "arrival spacing can overstate a correctly bounded request rate"
            );
        }
    }
}

#[test]
fn melee_walk_selection_cannot_exceed_the_ordinary_run_grant() {
    let (mut app, requests) = runtime();
    app.insert_resource(world(18.));
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, true);
    app.world_mut()
        .resource_mut::<motion::Controls>()
        .walk_speed = Some(12.);
    let attack = app
        .world_mut()
        .spawn((Interaction::None, crate::skinned::AttackButton))
        .id();
    crate::clicks::testing::click(&mut app, attack);
    let request = moves(&requests).pop().unwrap();
    assert!(
        request.position.x.hypot(request.position.y) <= 0.601,
        "the six-unit ordinary run grant also caps assisted walking: {request:?}"
    );
}

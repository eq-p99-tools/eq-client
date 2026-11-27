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
        super::refused_probe(
            InstalledClient::Titanium,
            &thin_lintel(),
            (&OGRE, 5.625),
            (from, to),
            (false, false),
            (
                crate::posture::headroom_dimensions(InstalledClient::Titanium, 5.625),
                0.0,
                span
            ),
        )
        .is_some(),
        "standing direction={direction} span={span}"
    );
    assert!(
        super::refused_probe(
            InstalledClient::Titanium,
            &thin_lintel(),
            (&OGRE, 5.625),
            (from, to),
            (true, false),
            (
                crate::posture::headroom_dimensions(InstalledClient::Titanium, 5.625),
                0.0,
                span
            ),
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
                    controls.sampling.span = Some(span);
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
            controls.sampling.span = Some(0.05);
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
            controls.sampling.span = Some(0.05);
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
                            None,
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
fn headroom_vertical_wall_with_open_sky_never_arms_a_pointless_duck() {
    for slant in [0.0, 0.01] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 100.0);
        let mut faces = plane(0.0, true);
        // A wall, including a slight inward lean. It is not a low opening.
        // The larger ceiling sphere can contact its side at a body-resolved
        // endpoint; ducking cannot create a passable route through it.
        faces.extend([
            [
                [-20.0, 0.0, 0.0],
                [-20.0, 100.0, -100.0 * slant],
                [20.0, 0.0, 0.0],
            ],
            [
                [20.0, 0.0, 0.0],
                [-20.0, 100.0, -100.0 * slant],
                [20.0, 100.0, -100.0 * slant],
            ],
        ]);
        app.insert_resource(Collision(Some(CollisionWorld::new(faces).unwrap())));
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            world_position(
                [0.0, HUMAN.feet_offset, -1.0],
                eq_client_core::world_heading(0.0),
            ),
        );
        let begin = Instant::now();
        let mut flips = Vec::new();
        for frame in 0..100 {
            {
                let mut controls = app.world_mut().resource_mut::<Controls>();
                controls.test_now = Some(begin + Duration::from_millis(frame * 100));
                controls.sampling.span = Some(0.1);
                controls.speed = Some(14.0);
            }
            app.update();
            for requested in accepted_commands(&mut app, &receiver) {
                flips.push(requested);
                posture(
                    &mut app,
                    match requested {
                        Posture::Standing => PostureState::Standing,
                        Posture::Ducking => PostureState::Ducking,
                        Posture::Sitting => panic!("unexpected sit"),
                    },
                );
            }
        }
        assert!(flips.is_empty(), "wall slant={slant}: {flips:?}");
    }
}

#[test]
fn automatic_return_checks_the_present_lifted_standing_probe() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 12.0);
    // A one-sided higher shelf is below the lifted standing origin, but
    // above the ducked origin. Return must use the ordinary standing probe
    // too, rather than assuming the ducked floor remains its standing floor.
    let faces = plane(0.0, true)
        .into_iter()
        .chain(plane(4.0, true))
        .chain(plane(12.0, false));
    app.insert_resource(Collision(Some(CollisionWorld::new(faces).unwrap())));
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
                now.checked_sub(Duration::from_millis(2_001)).unwrap(),
            );
            controls.test_now = Some(now);
        });
    posture(&mut app, PostureState::Ducking);
    app.update();
    assert!(
        commands(&receiver).is_empty(),
        "the next standing probe refuses"
    );
}

#[test]
fn automatic_return_checks_the_next_standing_step_along_held_input() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 100.0);
    let mut faces = plane(0.0, true);
    faces.extend([
        [[-20.0, 7.9, 0.0], [20.0, 7.9, 0.0], [20.0, 7.9, 3.0]],
        [[-20.0, 7.9, 0.0], [20.0, 7.9, 3.0], [-20.0, 7.9, 3.0]],
    ]);
    app.insert_resource(Collision(Some(CollisionWorld::new(faces).unwrap())));
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position(
            [0.0, HUMAN.feet_offset, -0.9],
            eq_client_core::world_heading(0.0),
        ),
    );
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
            controls.sampling.span = Some(0.1);
            controls.speed = Some(3.0);
        });
    posture(&mut app, PostureState::Ducking);
    app.update();
    let sent = accepted_commands(&mut app, &receiver);
    assert!(
        sent.is_empty(),
        "return at the edge must not precede a refused standing step: {sent:?}"
    );
    assert!(!app.world().resource::<Controls>().headroom_refused);
}

fn slower_duck_return(suppress_rate: bool) -> (App, mpsc::Receiver<ClientCommand>, Instant) {
    let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
    let begin = Instant::now();
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.test_now = Some(begin);
        controls.sampling.span = Some(0.1);
        controls.speed = Some(14.0);
        controls.walk_speed = Some(8.0);
        controls.backward_speed = Some(8.0);
        controls.strafe_speed = Some(7.0);
        controls.strafe_walk_speed = Some(4.0);
        controls.headroom.suppress_standing_rate = suppress_rate;
    }
    app.update();
    assert_eq!(
        accepted_commands(&mut app, &receiver),
        Vec::<Posture>::new()
    );
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .as_ref()
            .unwrap()
            .standing_rate
            >= 14.0
    );
    app.update();
    assert_eq!(
        accepted_commands(&mut app, &receiver),
        vec![Posture::Ducking]
    );
    posture(&mut app, PostureState::Ducking);
    let mut faces = plane(0.0, true);
    faces.extend([
        [[-20.0, 7.9, 0.0], [20.0, 7.9, 0.0], [20.0, 7.9, 3.0]],
        [[-20.0, 7.9, 0.0], [20.0, 7.9, 3.0], [-20.0, 7.9, 3.0]],
    ]);
    app.insert_resource(Collision(Some(CollisionWorld::new(faces).unwrap())));
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position(
            [0.0, HUMAN.feet_offset, -1.2],
            eq_client_core::world_heading(0.0),
        ),
    );
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.test_now = Some(begin + Duration::from_millis(2_001));
        controls.speed = Some(3.0);
        controls.backward_speed = Some(3.0);
    }
    (app, receiver, begin)
}

#[test]
fn automatic_return_uses_observed_standing_rates_not_the_slower_ducked_grant() {
    for suppress in [true, false] {
        let (mut app, receiver, begin) = slower_duck_return(suppress);
        app.update();
        let postures = accepted_commands(&mut app, &receiver);
        assert_eq!(
            postures,
            if suppress {
                vec![Posture::Standing]
            } else {
                vec![]
            }
        );
        assert!(!app.world().resource::<Controls>().headroom_refused);
        if suppress {
            // Same warmed fixture, only the test-only rate horizon suppressed:
            // returning on the ducked distance immediately refuses the next
            // real standing step when its normal faster rate is restored.
            posture(&mut app, PostureState::Standing);
            let before = app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position;
            {
                let mut controls = app.world_mut().resource_mut::<Controls>();
                controls.test_now = Some(begin + Duration::from_millis(2_101));
                controls.speed = Some(14.0);
            }
            app.update();
            assert!(app.world().resource::<Controls>().headroom_refused);
            let _ = accepted_commands(&mut app, &receiver);
            let after = app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position;
            assert_eq!([before.x, before.y, before.z], [after.x, after.y, after.z]);
        }
    }
}

#[test]
fn automatic_return_rate_horizon_uses_current_direction_and_release_stays_still() {
    for release in [false, true] {
        let (mut app, receiver, _) = slower_duck_return(false);
        let before = app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            if !release {
                keys.press(KeyCode::KeyS);
            }
        }
        app.update();
        let sent: Vec<_> = receiver.try_iter().collect();
        assert!(
            sent.iter().any(|command| matches!(
                command,
                ClientCommand::SetPosture {
                    posture: Posture::Standing,
                    ..
                }
            )),
            "release={release}: {sent:?}"
        );
        for command in sent {
            if let ClientCommand::Move(request) = command {
                if release {
                    assert_eq!(
                        [request.position.x, request.position.y, request.position.z],
                        [before.x, before.y, before.z]
                    );
                } else {
                    assert!(
                        eq_client_core::render_position(request.position)[2] < -1.2,
                        "current backward input must probe/move away from the opening"
                    );
                }
            }
        }
    }
}

#[test]
fn automatic_return_does_not_authorize_a_step_through_a_later_obstruction() {
    let (mut app, receiver, begin) = slower_duck_return(false);
    app.insert_resource(Collision(Some(room(100.0))));
    app.update();
    assert_eq!(
        accepted_commands(&mut app, &receiver),
        vec![Posture::Standing]
    );
    posture(&mut app, PostureState::Standing);
    let before = app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    // A new underside appears after Stand was queued/observed. Its actual
    // movement still uses ordinary collision; the earlier query grants none.
    app.insert_resource(Collision(Some(room(7.9))));
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.test_now = Some(begin + Duration::from_millis(2_101));
        controls.speed = Some(14.0);
    }
    app.update();
    assert!(app.world().resource::<Controls>().headroom_refused);
    let _ = accepted_commands(&mut app, &receiver);
    let after = app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    assert_eq!([before.x, before.y, before.z], [after.x, after.y, after.z]);
}

#[test]
fn headroom_single_marginal_opening_ducks_and_stands_at_most_once() {
    for millis in [50_u16, 100, 150] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 100.0);
        let mut faces = plane(0.0, true);
        faces.extend([
            [[-20.0, 7.9, 0.0], [20.0, 7.9, 0.0], [20.0, 7.9, 3.0]],
            [[-20.0, 7.9, 0.0], [20.0, 7.9, 3.0], [-20.0, 7.9, 3.0]],
        ]);
        app.insert_resource(Collision(Some(CollisionWorld::new(faces).unwrap())));
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            world_position(
                [0.0, HUMAN.feet_offset, -2.0],
                eq_client_core::world_heading(0.0),
            ),
        );
        let begin = Instant::now();
        let mut duck = 0;
        let mut stand = 0;
        for frame in 0..(10_000 / millis) {
            {
                let mut controls = app.world_mut().resource_mut::<Controls>();
                controls.test_now = Some(begin + Duration::from_millis(u64::from(frame * millis)));
                controls.sampling.span = Some(f32::from(millis) / 1_000.0);
                controls.speed = Some(6.0);
            }
            app.update();
            for requested in accepted_commands(&mut app, &receiver) {
                posture(
                    &mut app,
                    match requested {
                        Posture::Standing => {
                            stand += 1;
                            PostureState::Standing
                        }
                        Posture::Ducking => {
                            duck += 1;
                            PostureState::Ducking
                        }
                        Posture::Sitting => panic!("unexpected sit"),
                    },
                );
            }
        }
        assert_eq!(duck, 1, "{millis}ms");
        assert_eq!(stand, 1, "{millis}ms");
        let player = app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap();
        assert!(
            eq_client_core::render_position(player.position)[2] > 6.0,
            "{millis}ms: {:?}",
            player.position
        );
    }
}

/// Reconstruct the reported approach from installed geometry. The retained
/// report supplies pose/heading, not its exact frame span or granted speed.
#[test]
#[ignore = "reads an EverQuest installation named by EQ_DIR; no window or session"]
fn installed_felwithe_side_wall_does_not_cycle_posture() {
    let directory = std::path::PathBuf::from(std::env::var_os("EQ_DIR").expect("EQ_DIR"));
    let zone = eq_client_assets::load_zone_collision(&directory, "gfaydark").unwrap();
    let collision = crate::scene_collision::collision_world(
        &zone,
        crate::scene_collision::collision_triangles(&zone),
        Some(HUMAN.feet_offset),
    )
    .unwrap();
    for millis in [50_u16, 100, 150] {
        for speed in [14.0, 30.0, 43.0] {
            let (mut app, receiver) = app(InstalledClient::Titanium, 100.0);
            app.insert_resource(Collision(Some(collision.clone())));
            testing::place(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                eq_client_core::WorldPosition {
                    x: -2369.4993,
                    y: -2012.1583,
                    z: 28.256_168,
                    heading: 397.0,
                },
            );
            {
                let origin = app
                    .world()
                    .resource::<online::OnlineState>()
                    .world()
                    .player()
                    .unwrap()
                    .position;
                let world = app.world_mut();
                let mut players = world.query_filtered::<&mut Transform, With<Player>>();
                let mut transform = players.single_mut(world).unwrap();
                transform.translation = Vec3::from_array(eq_client_core::render_position(origin));
                transform.rotation =
                    Quat::from_rotation_y(eq_client_core::render_heading(origin.heading));
            }
            let begin = Instant::now();
            let mut flips = Vec::new();
            let mut attempted = false;
            let mut refused = false;
            for frame in 0..(10_000 / millis) {
                {
                    let mut controls = app.world_mut().resource_mut::<Controls>();
                    controls.test_now =
                        Some(begin + Duration::from_millis(u64::from(frame * millis)));
                    controls.sampling.span = Some(f32::from(millis) / 1_000.0);
                    controls.speed = Some(speed);
                    controls
                        .airborne
                        .get_or_insert_with(AirborneController::default);
                }
                app.update();
                {
                    let controls = app.world().resource::<Controls>();
                    attempted |= controls.pending.is_some();
                    refused |= controls.headroom_refused;
                }
                for requested in accepted_commands(&mut app, &receiver) {
                    flips.push(requested);
                    posture(
                        &mut app,
                        match requested {
                            Posture::Standing => PostureState::Standing,
                            Posture::Ducking => PostureState::Ducking,
                            Posture::Sitting => panic!("unexpected sit"),
                        },
                    );
                }
            }
            assert!(
                attempted && refused,
                "{millis}ms speed={speed}: the logged wall must actually be exercised"
            );
            assert!(flips.is_empty(), "{millis}ms speed={speed}: {flips:?}");
        }
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
                controls.sampling.span = Some(span);
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
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_some()
    );
    accepted_commands(&mut app, &receiver);
    app.insert_resource(Collision(Some(room(7.9).with_liquids(Submerged))));
    app.update();
    assert_eq!(
        accepted_commands(&mut app, &receiver),
        Vec::<Posture>::new()
    );
    let controls = app.world().resource::<Controls>();
    assert!(controls.headroom.automatic.is_none());
    assert!(!controls.headroom_retry);
    assert!(!controls.headroom_refused);
}

#[test]
fn headroom_server_placement_withdraws_unobserved_pending_cycles_even_at_same_xyz() {
    {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        app.update();
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_some()
        );
        accepted_commands(&mut app, &receiver);
        let position = app
            .world()
            .resource::<online::OnlineState>()
            .world()
            .player()
            .unwrap()
            .position;
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            position,
        );
        app.insert_resource(Collision(Some(room(30.0))));
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            Vec::<Posture>::new()
        );
        let controls = app.world().resource::<Controls>();
        assert!(controls.headroom.automatic.is_none());
        assert!(!controls.headroom_retry);
    }
}

fn observed_automatic_duck(app: &mut App, receiver: &mpsc::Receiver<ClientCommand>, now: Instant) {
    app.world_mut().resource_mut::<Controls>().test_now = Some(now);
    app.update();
    assert!(
        app.world()
            .resource::<Controls>()
            .headroom
            .automatic
            .is_some()
    );
    accepted_commands(app, receiver);
    app.update();
    assert_eq!(accepted_commands(app, receiver), vec![Posture::Ducking]);
    posture(app, PostureState::Ducking);
}

#[test]
fn headroom_observed_automatic_duck_survives_placement_and_motion_rebase() {
    for settle_before_placement in [false, true] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        let now = Instant::now();
        observed_automatic_duck(&mut app, &receiver, now);
        if settle_before_placement {
            app.update();
            assert_eq!(
                accepted_commands(&mut app, &receiver),
                Vec::<Posture>::new()
            );
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            world_position([2.0, 3.75, 0.0], 0.0),
        );
        app.insert_resource(Collision(Some(room(30.0))));
        app.world_mut().resource_mut::<Controls>().rebase();
        for millis in [500, 1_999, 2_000] {
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
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            vec![Posture::Standing],
            "settled={settle_before_placement}"
        );
    }
}

#[test]
fn headroom_automatic_duck_in_water_keeps_the_delay_then_returns_idle_or_held() {
    for (held, placed) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 7.9);
        let now = Instant::now();
        observed_automatic_duck(&mut app, &receiver, now);
        if !held {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
        if placed {
            testing::place(
                &mut app.world_mut().resource_mut::<online::OnlineState>(),
                world_position([2.0, 3.75, 0.0], 0.0),
            );
            app.world_mut().resource_mut::<Controls>().rebase();
        }
        app.insert_resource(Collision(Some(room(30.0).with_liquids(Submerged))));
        for millis in [500, 1_999, 2_000] {
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
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            vec![Posture::Standing],
            "held={held} placed={placed}"
        );
    }
}

#[test]
fn headroom_manual_duck_survives_land_and_water_placement_without_a_return_cycle() {
    for water in [false, true] {
        let (mut app, receiver) = app(InstalledClient::Titanium, 30.0);
        posture(&mut app, PostureState::Ducking);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        accepted_commands(&mut app, &receiver);
        testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            world_position([2.0, 3.75, 0.0], 0.0),
        );
        if water {
            app.insert_resource(Collision(Some(room(30.0).with_liquids(Submerged))));
        }
        app.world_mut().resource_mut::<Controls>().rebase();
        app.world_mut().resource_mut::<Controls>().test_now =
            Some(Instant::now() + Duration::from_secs(5));
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            Vec::<Posture>::new()
        );
        assert!(
            app.world()
                .resource::<Controls>()
                .headroom
                .automatic
                .is_none()
        );
        assert_eq!(
            app.world()
                .resource::<online::OnlineState>()
                .world()
                .posture(7),
            Some(PostureState::Ducking)
        );
    }
}

#[test]
fn headroom_ordinary_ledge_above_a_distant_floor_still_falls() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 30.0);
    let ledge = [
        [[-20.0, 0.0, -20.0], [-20.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        [[-20.0, 0.0, -20.0], [20.0, 0.0, 0.0], [20.0, 0.0, -20.0]],
    ];
    app.insert_resource(Collision(Some(
        CollisionWorld::new(ledge.into_iter().chain(plane(-1000.0, true))).unwrap(),
    )));
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position([0.0, 3.75, -1.0], 0.0),
    );
    app.world_mut()
        .query::<&mut crate::OrbitCamera>()
        .single_mut(app.world_mut())
        .unwrap()
        .yaw = std::f32::consts::PI;
    {
        let mut controls = app.world_mut().resource_mut::<Controls>();
        controls.sampling.span = Some(0.05);
        controls.speed = Some(60.0);
        controls.airborne = Some(AirborneController::default());
    }
    for _ in 0..8 {
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            Vec::<Posture>::new()
        );
        assert!(!app.world().resource::<Controls>().headroom_refused);
    }
    let at = app
        .world()
        .resource::<online::OnlineState>()
        .world()
        .player()
        .unwrap()
        .position;
    assert!(at.z < 3.75, "{at:?}");
    assert!(eq_client_core::render_position(at)[2] > 0.0, "{at:?}");
}

#[test]
fn headroom_zero_displacement_keeps_the_native_candidate_test() {
    // 0x48af13 normalizes a tiny delta, then still flows through the
    // ordinary local-player headroom guards at 0x48b162..0x48b1b7.
    let feet = Vec3::Y * 4.0;
    for (ceiling, refuses) in [(12.0, true), (20.0, false)] {
        let world =
            CollisionWorld::new(plane(4.0, true).into_iter().chain(plane(ceiling, false))).unwrap();
        assert_eq!(
            refused(
                InstalledClient::Titanium,
                &world,
                (&HUMAN, 3.75),
                (feet, feet),
                (false, false)
            )
            .is_some(),
            refuses
        );
    }
}

#[test]
fn headroom_held_input_at_a_tall_wall_never_arms_duck() {
    let (mut app, receiver) = app(InstalledClient::Titanium, 30.0);
    let wall = [
        [[-20.0, 0.0, 0.0], [-20.0, 30.0, 0.0], [20.0, 30.0, 0.0]],
        [[-20.0, 0.0, 0.0], [20.0, 30.0, 0.0], [20.0, 0.0, 0.0]],
    ];
    app.insert_resource(Collision(Some(
        CollisionWorld::new(plane(0.0, true).into_iter().chain(wall)).unwrap(),
    )));
    // An initially grazed wall must not become a ceiling, even without a
    // speculative controller push-out. Hold the real forward input.
    testing::place(
        &mut app.world_mut().resource_mut::<online::OnlineState>(),
        world_position([0.0, 3.75, -0.2], 0.0),
    );
    {
        let world = app.world_mut();
        world
            .query::<&mut crate::OrbitCamera>()
            .single_mut(world)
            .unwrap()
            .yaw = std::f32::consts::PI;
        let mut controls = world.resource_mut::<Controls>();
        controls.sampling.span = Some(0.05);
        controls.airborne = Some(AirborneController::default());
    }
    for _ in 0..6 {
        app.update();
        assert_eq!(
            accepted_commands(&mut app, &receiver),
            Vec::<Posture>::new()
        );
        let controls = app.world().resource::<Controls>();
        assert!(!controls.headroom_refused);
        assert!(controls.headroom.automatic.is_none());
    }
}

#[test]
fn headroom_ducked_low_opening_can_leave_a_standing_slide_path() {
    // Synthetic geometry control, not an installed asset or a native replay.
    // A hanging wall blocks the standing capsule, but its lower opening
    // admits the ducked shape. A different path is not itself a refusal.
    let faces = plane(0.0, true)
        .into_iter()
        .chain(plane(8.0, false))
        .chain([
            [[-10.0, 6.0, 0.0], [-10.0, 8.0, 0.0], [10.0, 8.0, 0.0]],
            [[-10.0, 6.0, 0.0], [10.0, 8.0, 0.0], [10.0, 6.0, 0.0]],
        ]);
    let world = CollisionWorld::new(faces).unwrap();
    let from = Vec3::new(0.0, 0.0, -1.0);
    let displacement = Vec3::Z * 2.0;
    let standing = world.step(from, displacement, HUMAN.height);
    let height = eq_client_core::movement::ducked_height(3.75, 4.75, 0.65, HUMAN.height);
    let ducked = world
        .clone()
        .with_collision_height(height)
        .step(from, displacement, HUMAN.height);
    assert!(standing.z < 0.0, "{standing:?}");
    assert!(ducked.z > 0.9, "{ducked:?}");
    assert!(
        refused(
            InstalledClient::Titanium,
            &world,
            (&HUMAN, 3.75),
            (from, ducked),
            (false, false)
        )
        .is_some()
    );
    assert!(
        refused(
            InstalledClient::Titanium,
            &world,
            (&HUMAN, 3.75),
            (from, ducked),
            (true, false)
        )
        .is_none()
    );
}

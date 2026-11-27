//! Native keyboard/mouse and production Receive/Input/Present regressions.
use super::*;
use eq_client_assets::regions::{RegionKind, ZoneRegions};
use eq_network_game::movement::{SpeedState, titanium_speeds};
use std::sync::mpsc;

#[path = "owned_swim_tests.rs"]
mod owned_swim_tests;

struct Native {
    app: App,
    requests: mpsc::Receiver<ClientCommand>,
    news: mpsc::Sender<eq_client_core::WorldUpdate>,
    camera: Entity,
    player: Entity,
    now: Instant,
}

impl Native {
    #[allow(clippy::too_many_lines, reason = "one production-stage water fixture")]
    fn new(first_person: bool, origin_y: f32) -> Self {
        let (mut app, requests, news) = latency_tests::timing_app();
        app.add_message::<bevy::window::WindowFocused>()
            .add_systems(
                PreUpdate,
                crate::camera::look.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                crate::camera::turn
                    .before(input)
                    .in_set(crate::systems::Stage::Input),
            )
            .add_systems(
                Update,
                crate::camera::look_keys
                    .after(interpolate)
                    .before(crate::camera::orbit_camera)
                    .in_set(crate::systems::Stage::Present),
            );
        for (act, key) in [
            (Act::Forward, KeyCode::KeyW),
            (Act::TurnLeft, KeyCode::KeyA),
            (Act::TurnRight, KeyCode::KeyD),
        ] {
            crate::keys::testing::bind(&mut app, act, crate::keys::Chord::key(key));
        }
        let pool = CollisionWorld::new([
            [
                [-200., -60., -200.],
                [200., -60., 200.],
                [200., -60., -200.],
            ],
            [
                [-200., -60., -200.],
                [-200., -60., 200.],
                [200., -60., 200.],
            ],
        ])
        .unwrap()
        .with_liquids(crate::scene_collision::ZoneLiquids(
            ZoneRegions::test_kind_surface(0., RegionKind::WATER).with_titanium_liquids(),
        ))
        .with_own_liquids(crate::scene_collision::ZoneLiquids(
            ZoneRegions::test_kind_surface(0., RegionKind::WATER).with_titanium_own_liquids(),
        ))
        .with_swim_origin(3.75);
        app.insert_resource(Collision(Some(pool)));
        let placement = world_position([0., origin_y, 0.], 0.);
        online::testing::place(
            &mut app.world_mut().resource_mut::<online::OnlineState>(),
            placement,
        );
        let grant = titanium_speeds(&SpeedState {
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
        .unwrap();
        let now = Instant::now();
        let start = now.checked_sub(Duration::from_millis(100)).unwrap();
        let (camera, player) = {
            let world = app.world_mut();
            let camera = world
                .query::<(Entity, &OrbitCamera)>()
                .single(world)
                .unwrap()
                .0;
            let player = world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap();
            world.get_mut::<Transform>(player).unwrap().translation = Vec3::new(0., origin_y, 0.);
            let mut body = world.get_mut::<PlayerBody>(player).unwrap();
            body.height = 6.677_319_5;
            body.feet_offset = 3.75;
            let mut orbit = world.get_mut::<OrbitCamera>(camera).unwrap();
            orbit.focus = Vec3::new(0., origin_y, 0.);
            orbit.yaw = crate::camera::behind(eq_client_core::render_heading(0.));
            orbit.look = if origin_y < -1. { 0.17 } else { 0. };
            orbit.pitch = -0.23;
            if first_person {
                orbit.look_first_person_for_test();
            }
            world
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(Fix::JesusMode, false);
            let mut c = world.resource_mut::<Controls>();
            c.speed = Some(grant.units_per_second);
            c.strafe_speed = grant.strafe.map(|s| s.units_per_second);
            c.strafe_walk_speed = grant.strafe.and_then(|s| s.walk_units_per_second);
            c.walk_speed = grant.walk.map(|s| s.units_per_second);
            c.backward_speed = grant.backward.map(|s| s.units_per_second);
            c.diagonal_rates = grant.diagonal;
            c.swim = grant.swim;
            c.airborne = Some(AirborneController::default());
            c.test_now = Some(now);
            c.last_sent = Some(start);
            c.last_accepted = start;
            (camera, player)
        };
        Self {
            app,
            requests,
            news,
            camera,
            player,
            now,
        }
    }

    fn frame(&mut self, dx: f32) -> Vec<MovementRequest> {
        let step = Duration::from_micros(16_667);
        self.now += step;
        self.app.world_mut().resource_mut::<Time>().advance_by(step);
        self.app.world_mut().resource_mut::<Controls>().test_now = Some(self.now);
        self.app
            .world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(self.now);
        if dx != 0. {
            self.app
                .world_mut()
                .write_message(bevy::input::mouse::MouseMotion {
                    delta: Vec2::new(dx, 0.),
                });
        }
        self.app.update();
        self.requests
            .try_iter()
            .filter_map(|command| match command {
                ClientCommand::Move(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    fn reply(&self, request: &MovementRequest, refused: bool) {
        let position = if refused {
            self.app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position
        } else {
            request.position
        };
        self.news
            .send(eq_client_core::WorldUpdate::Game(
                eq_client_core::WorldEvent::MotionSent {
                    session_id: request.session_id,
                    created: request.created,
                    position,
                    refused: refused.then(|| "fixture translation refusal".into()),
                },
            ))
            .unwrap();
    }

    fn view(&self) -> (f32, f32) {
        let orbit = self.app.world().get::<OrbitCamera>(self.camera).unwrap();
        (orbit.yaw, orbit.view_pitch())
    }
}

#[test]
fn refused_water_translation_retains_mouse_view_and_restores_observed_position() {
    for first_person in [false, true] {
        for (y, forward, submitted_preview) in [
            (-15., false, true),
            (-0.1, false, true),
            (-0.1, true, true),
            (-15., false, false),
            (-0.1, true, false),
        ] {
            let mut f = Native::new(first_person, y);
            // The accepted-only path also serves surface continuation: its
            // worker result falls back to display_in_frame, without a preview.
            f.app
                .world_mut()
                .resource_mut::<Controls>()
                .suppress_submitted_preview = !submitted_preview;
            crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
            latency_tests::key(&mut f.app, KeyCode::KeyD, true);
            if forward {
                latency_tests::key(&mut f.app, KeyCode::KeyW, true);
            }
            let request = f.frame(4.).pop().expect("first native strafe");
            assert_eq!(request.mode, MovementMode::Swim);
            for _ in 0..3 {
                assert_eq!(
                    f.frame(4.),
                    Vec::<MovementRequest>::new(),
                    "one proposal stays in flight"
                );
            }
            let before = f.view();
            let placement = f
                .app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position;
            f.reply(&request, true);
            // Real devices deliver frames with no mouse event even during a drag.
            assert_eq!(f.frame(0.), Vec::<MovementRequest>::new());
            let after = f.view();
            assert!(
                Quat::from_rotation_y(before.0).angle_between(Quat::from_rotation_y(after.0))
                    < 0.001,
                "refusal changed yaw: {before:?} -> {after:?}"
            );
            assert_eq!(before.1.to_bits(), after.1.to_bits());
            assert!(
                f.app
                    .world()
                    .get::<Transform>(f.player)
                    .unwrap()
                    .translation
                    .abs_diff_eq(
                        Vec3::from_array(eq_client_core::render_position(placement)),
                        1e-5
                    ),
                "refusal must restore the observed position within render-coordinate rounding"
            );
            assert_eq!(
                f.app
                    .world()
                    .resource::<online::OnlineState>()
                    .world()
                    .player()
                    .unwrap()
                    .position,
                placement
            );
        }
    }
}

#[test]
fn newer_mouse_view_survives_an_older_acceptance_but_not_a_server_correction() {
    let mut f = Native::new(false, -15.);
    crate::camera::testing::button(&mut f.app, MouseButton::Right, true);
    latency_tests::key(&mut f.app, KeyCode::KeyD, true);
    let request = f.frame(4.).pop().expect("first strafe");
    for _ in 0..3 {
        assert_eq!(f.frame(4.), Vec::<MovementRequest>::new());
    }
    let before = f.view();
    f.reply(&request, false);
    assert_eq!(f.frame(0.), Vec::<MovementRequest>::new());
    assert!(
        Quat::from_rotation_y(before.0).angle_between(Quat::from_rotation_y(f.view().0)) < 0.001
    );
    // A real server placement still withdraws local prediction and owns yaw.
    let position = world_position([3., -12., 2.], 64.);
    f.news
        .send(eq_client_core::WorldUpdate::Game(
            eq_client_core::WorldEvent::Position {
                spawn_id: 7,
                position,
                velocity: [0.; 3],
                forward: 0.,
            },
        ))
        .unwrap();
    assert_eq!(f.frame(0.), Vec::<MovementRequest>::new());
    assert!(f.app.world().resource::<Controls>().mouse_heading.is_none());
    let heading = eq_client_core::world_heading((-f.view().0.sin()).atan2(-f.view().0.cos()));
    assert!(arc(heading, 64.).abs() < 0.001);
    assert_eq!(
        f.app
            .world()
            .get::<Transform>(f.player)
            .unwrap()
            .translation,
        Vec3::from_array(eq_client_core::render_position(position))
    );
}

//! Received server placements through normal Receive/Input/Present stages.
use super::{
    latency_tests::{key, timing_app},
    *,
};
use eq_client_core::{WorldEvent, WorldUpdate};
use std::sync::mpsc;

struct Rig {
    app: App,
    requests: mpsc::Receiver<ClientCommand>,
    news: mpsc::Sender<WorldUpdate>,
    player: Entity,
    camera: Entity,
    base: Instant,
    frame: u32,
}

impl Rig {
    fn new(accepted: f32, drawn: f32, offset: f32) -> Self {
        let (mut app, requests, news) = timing_app();
        app.add_systems(
            Update,
            crate::camera::toggle_free
                .before(input)
                .in_set(crate::systems::Stage::Input),
        )
        .add_systems(
            Update,
            crate::camera::fly_camera
                .before(interpolate)
                .in_set(crate::systems::Stage::Present),
        );
        crate::keys::testing::bind(
            &mut app,
            Act::Forward,
            crate::keys::Chord::key(KeyCode::ArrowUp),
        );
        crate::keys::testing::bind(
            &mut app,
            Act::FreeCamera,
            crate::keys::Chord::key(KeyCode::F9),
        );
        let base = Instant::now() + Duration::from_secs(10);
        let (player, camera) = {
            let world = app.world_mut();
            online::testing::place(
                &mut world.resource_mut::<online::OnlineState>(),
                world_position([0., 3., 0.], accepted),
            );
            world.resource_mut::<Controls>().last_accepted =
                base.checked_sub(Duration::from_secs(1)).unwrap();
            world
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(eq_client_core::qol::Fix::FreeCamera, true);
            let player = world
                .query_filtered::<Entity, With<Player>>()
                .single(world)
                .unwrap();
            let camera = world
                .query_filtered::<Entity, With<OrbitCamera>>()
                .single(world)
                .unwrap();
            world.get_mut::<Transform>(player).unwrap().rotation =
                Quat::from_rotation_y(eq_client_core::render_heading(drawn));
            let mut orbit = world.get_mut::<OrbitCamera>(camera).unwrap();
            orbit.yaw = crate::camera::behind(eq_client_core::render_heading(drawn)) + offset;
            orbit.pitch = -0.33;
            orbit.radius = 12.;
            orbit.look = 0.27;
            (player, camera)
        };
        let mut rig = Self {
            app,
            requests,
            news,
            player,
            camera,
            base,
            frame: 0,
        };
        rig.tick();
        rig.tick();
        rig
    }

    fn tick(&mut self) {
        let now = self.base + Duration::from_micros(u64::from(self.frame) * 16_667);
        self.frame += 1;
        self.app.world_mut().resource_mut::<Controls>().test_now = Some(now);
        self.app
            .world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(now);
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_micros(16_667));
        self.app.update();
    }

    fn orbit(&self) -> &OrbitCamera {
        self.app.world().get::<OrbitCamera>(self.camera).unwrap()
    }
    fn root(&self) -> Transform {
        *self.app.world().get::<Transform>(self.player).unwrap()
    }
    fn view(&self) -> Transform {
        *self.app.world().get::<Transform>(self.camera).unwrap()
    }
    fn tell(&self, event: WorldEvent) {
        self.news.send(WorldUpdate::Game(event)).unwrap();
    }

    fn placement(&self, heading: f32, relocation: bool) {
        let position = world_position([20., 3., -40.], heading);
        self.tell(if relocation {
            WorldEvent::SameZoneRelocated(eq_client_core::SameZoneRelocation {
                session_id: 11,
                spawn_id: 7,
                position,
            })
        } else {
            WorldEvent::Position {
                spawn_id: 7,
                position,
                velocity: [0.; 3],
                forward: 0.,
            }
        });
    }

    fn assert_chase(&self, heading: f32, offset: f32) {
        let body = self.root();
        let expected_facing =
            Quat::from_rotation_y(eq_client_core::render_heading(heading)) * Vec3::Z;
        assert!((body.rotation * Vec3::Z).dot(expected_facing) > 1. - 0.000_01);
        let wanted = Quat::from_rotation_y(offset) * -expected_facing;
        let actual = (self.view().translation - body.translation)
            .with_y(0.)
            .normalize();
        assert!(
            actual.dot(wanted) > 1. - 0.000_01,
            "drawn chase offset changed: {actual:?} vs {wanted:?}"
        );
        assert!(
            angle(
                self.orbit().yaw
                    - crate::camera::behind(eq_client_core::render_heading(heading))
                    - offset
            )
            .abs()
                < 0.000_01
        );
        assert_eq!(self.orbit().pitch.to_bits(), (-0.33_f32).to_bits());
        assert_eq!(self.orbit().radius.to_bits(), 12_f32.to_bits());
        assert_eq!(self.orbit().look.to_bits(), 0.27_f32.to_bits());
        assert_eq!(self.orbit().view_name(), "chase");
        assert_eq!(self.orbit().focus, body.translation);
    }
}

fn angle(value: f32) -> f32 {
    value.sin().atan2(value.cos())
}

#[test]
fn server_placement_transports_chase_orbit_from_the_last_drawn_facing() {
    for relocation in [false, true] {
        for (accepted, drawn, new, offset) in [
            (128., 128., 0., 0.),
            (37.25, 37.25, 379.5, 125_f32.to_radians()),
            (511.5, 511.5, 0.5, -0.8),
            (0.5, 0.5, 511.5, 2.1),
            // An in-flight display can disagree with the last accepted yaw.
            (0., 128.75, 0., 0.6),
        ] {
            let mut rig = Rig::new(accepted, drawn, offset);
            rig.placement(new, relocation);
            rig.tick();
            assert_eq!(rig.root().translation, Vec3::new(20., 3., -40.));
            rig.assert_chase(new, offset);
        }
    }
}

#[test]
fn batched_server_placements_keep_one_relative_orbit_offset() {
    let mut rig = Rig::new(24., 24., -1.2);
    rig.placement(210., true);
    rig.placement(487., true);
    rig.tick();
    rig.assert_chase(487., -1.2);
}

#[test]
fn stale_mouse_display_cannot_erase_the_transported_orbit() {
    // Test-only negative control reproduces the old presenter's stale follow
    // after the yaw transport; the positive row retires that old basis.
    fn forget_placement(mut controls: ResMut<Controls>) {
        controls.server_chase_placed = false;
    }
    for negative in [true, false] {
        let mut rig = Rig::new(0., 128., 0.);
        rig.app.world_mut().resource_mut::<Controls>().mouse_heading = Some(128.);
        rig.tick();
        {
            let world = rig.app.world_mut();
            let pose = {
                let mut camera = world.get_mut::<OrbitCamera>(rig.camera).unwrap();
                camera.yaw += 0.8;
                crate::camera::orbit_transform(&camera)
            };
            *world.get_mut::<Transform>(rig.camera).unwrap() = pose;
        }
        if negative {
            rig.app.add_systems(
                Update,
                forget_placement
                    .before(interpolate)
                    .in_set(crate::systems::Stage::Present),
            );
        }
        rig.placement(0., true);
        rig.tick();
        rig.assert_chase(0., if negative { 0. } else { 0.8 });
    }
}

#[test]
fn unchanged_server_heading_and_remote_spawn_do_not_rotate_chase() {
    let mut rig = Rig::new(83., 83., 0.7);
    let before = rig.orbit().yaw;
    rig.placement(83., true);
    rig.tick();
    assert!(angle(rig.orbit().yaw - before).abs() < 0.000_01);
    rig.tell(WorldEvent::Position {
        spawn_id: 8,
        position: world_position([-20., 3., 40.], 300.),
        velocity: [0.; 3],
        forward: 0.,
    });
    rig.tick();
    assert!(angle(rig.orbit().yaw - before).abs() < 0.000_01);
}

#[test]
fn an_ordinary_local_receipt_keeps_deliberate_world_yaw() {
    let mut rig = Rig::new(0., 0., 0.7);
    let before = rig.orbit().yaw;
    key(&mut rig.app, KeyCode::ArrowUp, true);
    rig.tick();
    let request = rig
        .requests
        .try_iter()
        .find_map(|command| {
            if let ClientCommand::Move(request) = command {
                Some(request)
            } else {
                None
            }
        })
        .unwrap();
    key(&mut rig.app, KeyCode::ArrowUp, false);
    rig.tell(WorldEvent::MotionSent {
        session_id: 11,
        created: request.created,
        position: request.position,
        refused: None,
    });
    rig.tick();
    assert!(angle(rig.orbit().yaw - before).abs() < 0.000_01);
}

#[test]
fn detached_view_stays_put_and_returns_to_the_transported_chase_orbit() {
    let mut rig = Rig::new(128., 128., 0.9);
    key(&mut rig.app, KeyCode::F9, true);
    rig.tick();
    assert!(
        rig.app
            .world()
            .get::<crate::camera::FreeCamera>(rig.camera)
            .is_some()
    );
    key(&mut rig.app, KeyCode::F9, false);
    rig.tick();
    let before = rig.view();
    rig.placement(0., true);
    rig.tick();
    assert!(rig.view().translation.distance(before.translation) < 0.000_01);
    assert!(rig.view().rotation.dot(before.rotation).abs() > 1. - 0.000_01);
    assert!(
        angle(rig.orbit().yaw - crate::camera::behind(eq_client_core::render_heading(0.)) - 0.9)
            .abs()
            < 0.000_01
    );
    key(&mut rig.app, KeyCode::F9, true);
    rig.tick();
    assert!(
        rig.app
            .world()
            .get::<crate::camera::FreeCamera>(rig.camera)
            .is_none()
    );
    rig.assert_chase(0., 0.9);
}

#[test]
fn first_person_and_orthographic_placements_keep_their_existing_yaw_policy() {
    for first_person in [true, false] {
        let mut rig = Rig::new(128., 128., 0.4);
        if first_person {
            let mut orbit = rig
                .app
                .world_mut()
                .get_mut::<OrbitCamera>(rig.camera)
                .unwrap();
            orbit.look_first_person_for_test();
            orbit.look = 0.27;
        } else {
            rig.app
                .world_mut()
                .entity_mut(rig.camera)
                .insert(Projection::Orthographic(
                    OrthographicProjection::default_3d(),
                ));
        }
        rig.tick();
        let before = rig.orbit().yaw;
        rig.placement(0., true);
        rig.tick();
        assert!(angle(rig.orbit().yaw - before).abs() < 0.000_01);
        assert_eq!(rig.orbit().is_first_person(), first_person);
    }
}

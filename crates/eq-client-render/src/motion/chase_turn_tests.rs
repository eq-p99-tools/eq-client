//! Native mapped input through Receive/Input/Present, including the actual drawn chase.
use super::{
    latency_tests::{key, timing_app},
    *,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::mpsc};

struct Rig {
    app: App,
    requests: mpsc::Receiver<ClientCommand>,
    news: mpsc::Sender<eq_client_core::WorldUpdate>,
    replies: VecDeque<(usize, MovementRequest)>,
    base: Instant,
    frame: usize,
    delay: usize,
    rows: Vec<Value>,
}

impl Rig {
    fn new(heading: f32, delay: usize) -> Self {
        let (mut app, requests, news) = timing_app();
        // Restore Titanium's arrow/numpad defaults after timing_app's camera-key assignments.
        app.insert_resource(crate::keys::KeyMap::default())
            .add_message::<bevy::window::WindowFocused>()
            .add_systems(
                PreUpdate,
                crate::camera::look.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                (crate::camera::toggle_free, crate::camera::turn)
                    .chain()
                    .before(input)
                    .in_set(crate::systems::Stage::Input),
            )
            .add_systems(
                Update,
                crate::camera::fly_camera
                    .before(interpolate)
                    .in_set(crate::systems::Stage::Present),
            );
        // The detached-view extension has no native fallback binding.
        crate::keys::testing::bind(
            &mut app,
            Act::FreeCamera,
            crate::keys::Chord::key(KeyCode::F9),
        );
        {
            let world = app.world_mut();
            online::testing::place(
                &mut world.resource_mut::<online::OnlineState>(),
                world_position([0., 3., 0.], heading),
            );
            world
                .query_filtered::<&mut Transform, With<Player>>()
                .single_mut(world)
                .unwrap()
                .rotation = Quat::from_rotation_y(eq_client_core::render_heading(heading));
            world
                .query::<&mut OrbitCamera>()
                .single_mut(world)
                .unwrap()
                .yaw = crate::camera::behind(eq_client_core::render_heading(heading));
            world.resource_mut::<Controls>().strafe_speed = Some(4.);
            world
                .resource_mut::<crate::options::OptionsState>()
                .options
                .qol
                .set(eq_client_core::qol::Fix::FreeCamera, true);
        }
        let mut rig = Self {
            app,
            requests,
            news,
            replies: VecDeque::new(),
            base: Instant::now() + Duration::from_secs(60),
            frame: 0,
            delay,
            rows: Vec::new(),
        };
        for _ in 0..5 {
            rig.tick();
        }
        rig
    }

    fn key(&mut self, code: KeyCode, down: bool) {
        key(&mut self.app, code, down);
    }

    fn tick(&mut self) -> Value {
        let now = self.base + Duration::from_micros(u64::try_from(self.frame).unwrap() * 16_667);
        self.app.world_mut().resource_mut::<Controls>().test_now = Some(now);
        self.app
            .world_mut()
            .resource_mut::<crate::outbox::Outbox>()
            .set_test_time(now);
        while self
            .replies
            .front()
            .is_some_and(|(due, _)| *due <= self.frame)
        {
            let (_, request) = self.replies.pop_front().unwrap();
            self.news
                .send(eq_client_core::WorldUpdate::Game(
                    eq_client_core::WorldEvent::MotionSent {
                        session_id: request.session_id,
                        created: request.created,
                        position: request.position,
                        refused: None,
                    },
                ))
                .unwrap();
        }
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_micros(16_667));
        self.app.update();
        let mut commands = Vec::new();
        for command in self.requests.try_iter() {
            if let ClientCommand::Move(request) = command {
                commands.push(json!({"position":eq_client_core::render_position(request.position),"heading":request.position.heading,
                    "session":request.session_id,"created_ns":request.created.duration_since(self.base).as_nanos(),
                    "mode":format!("{:?}",request.mode),"update":format!("{:?}",request.heading),
                    "support":format!("{:?}",request.support),"vertical":request.vertical_speed}));
                self.replies
                    .push_back((self.frame + 1 + self.delay, request));
            }
        }
        let world = self.app.world_mut();
        let player = *world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .unwrap();
        let (camera, drawn, free) = world
            .query::<(&OrbitCamera, &Transform, Has<crate::camera::FreeCamera>)>()
            .single(world)
            .unwrap();
        let forward = (player.rotation * Vec3::Z).with_y(0.).normalize();
        let view = Quat::from_rotation_y(camera.yaw) * Vec3::NEG_Z;
        let behind = (drawn.translation - player.translation)
            .with_y(0.)
            .normalize_or_zero();
        let row = json!({"frame":self.frame,"root":player.translation.to_array(),"rotation":player.rotation.to_array(),
            "camera":drawn.translation.to_array(),"camera_rotation":drawn.rotation.to_array(),"yaw":camera.yaw,"pitch":camera.pitch,"radius":camera.radius,"free":free,
            "actor_forward":forward.to_array(),
            "facing_error":forward.distance(view),"behind_error":behind.distance(-forward),"commands":commands});
        self.rows.push(row.clone());
        self.frame += 1;
        row
    }
}

fn turn_case(heading: f32, code: KeyCode, moving: bool, delay: usize, old_tag: bool) -> Vec<Value> {
    let mut rig = Rig::new(heading, delay);
    if old_tag {
        // Exact pre-fix presentation identity. Set it after submission, before
        // Present and the following Receive; turn math and commands stay intact.
        rig.app.add_systems(
            Update,
            (|mut controls: ResMut<Controls>| {
                controls.sent_yaw = SentYaw::Ordinary;
                // Input may already have created the submitted dry preview.
                if let Some(visual) = controls.visual.as_mut() {
                    visual.follow_heading = false;
                    for segment in &mut visual.queue {
                        segment.follow_heading = false;
                    }
                }
            })
            .after(input)
            .before(interpolate)
            .in_set(crate::systems::Stage::Input),
        );
    }
    let initial = rig.rows.last().unwrap().clone();
    rig.key(code, true);
    if moving {
        rig.key(KeyCode::ArrowUp, true);
    }
    for frame in 5..85 {
        if frame == 35 {
            rig.key(code, false);
        }
        if frame == 50 {
            rig.key(KeyCode::ArrowUp, false);
        }
        let row = rig.tick();
        assert_eq!(row["pitch"], initial["pitch"]);
        assert_eq!(row["radius"], initial["radius"]);
    }
    assert!(
        rig.rows.iter().skip(5).any(|row| {
            row["actor_forward"]
                .as_array()
                .unwrap()
                .iter()
                .zip(initial["actor_forward"].as_array().unwrap())
                .map(|(a, b)| (a.as_f64().unwrap() - b.as_f64().unwrap()).powi(2))
                .sum::<f64>()
                > 0.000_1
        }),
        "mapped key failed to turn the actor"
    );
    if moving {
        assert_ne!(rig.rows.last().unwrap()["root"], initial["root"]);
    }
    println!(
        "CHASE_TURN {}",
        json!({"heading":heading,"key":format!("{code:?}"),"moving":moving,"delay":delay,"old_tag":old_tag,"rows":rig.rows})
    );
    rig.rows
}

#[test]
fn chase_follows_mapped_turn_keys_while_standing_moving_and_releasing() {
    let mut failures = Vec::new();
    for heading in [0., 128., 511.5] {
        for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight] {
            for moving in [false, true] {
                for delay in [0, 2] {
                    let rows = turn_case(heading, code, moving, delay, false);
                    let old = turn_case(heading, code, moving, delay, true);
                    assert!(
                        old.iter()
                            .any(|row| row["facing_error"].as_f64().unwrap() > 0.01)
                    );
                    for (row, prior) in rows.iter().zip(old) {
                        for field in ["frame", "root", "rotation", "actor_forward", "commands"] {
                            assert_eq!(row[field], prior[field], "presentation changed {field}");
                        }
                        if row["facing_error"].as_f64().unwrap() > 0.000_1
                            || row["behind_error"].as_f64().unwrap() > 0.001
                        {
                            failures.push((heading, code, moving, delay, row["frame"].clone()));
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "chase left the drawn actor's facing: {failures:?}"
    );
}

#[test]
fn chase_keeps_deliberate_orbit_for_forward_translation_without_turning() {
    let mut rig = Rig::new(128., 0);
    rig.app
        .world_mut()
        .query::<&mut OrbitCamera>()
        .single_mut(rig.app.world_mut())
        .unwrap()
        .yaw += 0.63;
    let before = rig.tick();
    rig.key(KeyCode::ArrowUp, true);
    for _ in 0..30 {
        let row = rig.tick();
        assert_eq!(row["yaw"], before["yaw"]);
        assert_eq!(row["pitch"], before["pitch"]);
        assert_eq!(row["radius"], before["radius"]);
    }
    assert_ne!(rig.rows.last().unwrap()["root"], before["root"]);
}

#[test]
fn chase_keeps_orbit_when_camera_relative_direction_owns_the_facing() {
    let mut rig = Rig::new(128., 0);
    crate::keys::testing::bind(
        &mut rig.app,
        Act::CameraForward,
        crate::keys::Chord::key(KeyCode::KeyT),
    );
    rig.app
        .world_mut()
        .query::<&mut OrbitCamera>()
        .single_mut(rig.app.world_mut())
        .unwrap()
        .yaw += 0.13;
    let before = rig.tick();
    rig.key(KeyCode::KeyT, true);
    rig.key(KeyCode::ArrowLeft, true);
    for _ in 0..30 {
        let row = rig.tick();
        assert_eq!(row["yaw"], before["yaw"]);
        assert_eq!(row["pitch"], before["pitch"]);
        assert_eq!(row["radius"], before["radius"]);
    }
}

#[test]
fn chase_rmb_acquisition_preserves_existing_view_and_turn_keys_only_strafe() {
    for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight] {
        let mut rig = Rig::new(128., 0);
        rig.app
            .world_mut()
            .query::<&mut OrbitCamera>()
            .single_mut(rig.app.world_mut())
            .unwrap()
            .yaw += 0.63;
        let before = rig.tick();
        crate::camera::testing::button(&mut rig.app, MouseButton::Right, true);
        rig.key(code, true);
        for _ in 0..25 {
            let row = rig.tick();
            let difference = row["yaw"].as_f64().unwrap() - before["yaw"].as_f64().unwrap();
            assert!(difference.sin().abs() < 0.000_1 && difference.cos() > 0.9999);
            assert!(row["facing_error"].as_f64().unwrap() < 0.000_1);
            assert_eq!(row["pitch"], before["pitch"]);
            assert_eq!(row["radius"], before["radius"]);
            for command in row["commands"].as_array().unwrap() {
                assert!(command["mode"].as_str().unwrap().starts_with("Strafe"));
            }
        }
    }
}

#[test]
fn chase_leaves_detached_camera_under_its_own_turn_controls() {
    let mut rig = Rig::new(128., 0);
    rig.key(KeyCode::F9, true);
    let before = rig.tick();
    assert_eq!(before["free"], true);
    rig.key(KeyCode::F9, false);
    rig.key(KeyCode::ArrowLeft, true);
    for _ in 0..12 {
        let row = rig.tick();
        assert_eq!(row["free"], true);
        assert_eq!(row["yaw"], before["yaw"]);
        assert_eq!(row["rotation"], before["rotation"]);
        assert_eq!(row["commands"].as_array().unwrap(), &Vec::<Value>::new());
    }
    assert_ne!(
        rig.rows.last().unwrap()["camera_rotation"],
        before["camera_rotation"],
        "detached view did not turn"
    );
}

//! RMB camera ownership through real button/motion events, normal stage edges,
//! and the existing dry-ground fixture's unchanged local movement guard.
//! No renderer, OS input, transport, or gameplay is exercised here.
use super::*;
use crate::camera::{self, MouseAim, MouseLook};
use bevy::{ecs::system::RunSystemOnce, input::mouse::MouseMotion, window::WindowFocused};
use eq_client_core::{SpawnKind, WorldEvent, WorldPosition, qol::Fix};

#[derive(Clone, Copy)]
struct View {
    yaw: f32,
    pitch: f32,
    camera: Transform,
    actor: Transform,
}

fn view(scene: &mut Scene) -> View {
    let world = scene.app.world_mut();
    let (orbit, camera) = world
        .query::<(&OrbitCamera, &Transform)>()
        .single(world)
        .unwrap();
    let (yaw, pitch, camera) = (orbit.yaw, orbit.view_pitch(), *camera);
    let actor = *world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap();
    View {
        yaw,
        pitch,
        camera,
        actor,
    }
}

fn point_view(scene: &mut Scene, yaw: f32, first_person: bool) {
    let world = scene.app.world_mut();
    let mut cameras = world.query::<&mut OrbitCamera>();
    let mut orbit = cameras.single_mut(world).unwrap();
    orbit.yaw = yaw;
    orbit.pitch = -0.3;
    if first_person {
        orbit.look_first_person_for_test();
    }
}

fn camera_locked(scene: &Scene) -> bool {
    let world = scene.app.world();
    world
        .resource::<Controls>()
        .melee_camera_locked(world.resource::<online::OnlineState>().world())
}

fn motion(scene: &mut Scene, delta: Vec2) {
    scene.app.world_mut().write_message(MouseMotion { delta });
}

fn turn_per_pixel(scene: &mut Scene) -> f32 {
    let world = scene.app.world_mut();
    let window = world.query::<&Window>().single(world).unwrap();
    world
        .resource::<crate::options::OptionsState>()
        .options
        .turn_per_pixel(camera::screen_width(Some(window)))
}

pub(super) fn scene(locked: bool, delay: u32) -> Scene {
    let mut scene = Scene::with_rates(delay, true, 6., true);
    scene
        .app
        .init_resource::<crate::combat::CombatState>()
        .init_resource::<crate::hud::hotbar::Bindings>()
        .init_resource::<MouseLook>()
        .init_resource::<MouseAim>()
        .init_resource::<crate::items::ItemState>()
        .init_resource::<crate::inventory::InventoryState>()
        .add_message::<WindowFocused>()
        .add_systems(PreUpdate, camera::look.after(bevy::input::InputSystems))
        .add_systems(
            Update,
            (crate::combat::input, camera::turn)
                .chain()
                .before(input)
                .in_set(crate::systems::Stage::Input),
        );
    crate::keys::testing::bind(
        &mut scene.app,
        Act::Attack,
        crate::keys::Chord::key(KeyCode::F10),
    );
    {
        let mut online = scene.app.world_mut().resource_mut::<online::OnlineState>();
        let mut target = crate::online::testing::pet(5, 0);
        target.kind = SpawnKind::Npc;
        target.pet_owner = None;
        target.race = 1;
        target.size = 6.;
        target.position = WorldPosition {
            y: 10.,
            z: 3.,
            ..WorldPosition::default()
        };
        crate::online::testing::spawn_entry(&mut online, 5, target);
        let request_id = online.world().next_target_request() - 1;
        crate::online::testing::news(
            &mut online,
            [WorldEvent::TargetChanged {
                session_id: 11,
                request_id,
                state: eq_network_game::targets::TargetState {
                    revision: 1,
                    spawn_id: Some(5),
                    available: true,
                },
            }],
        );
    }
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(Fix::MeleePositioning, locked);
    for _ in 0..30 {
        scene.step(false);
    }
    latency_tests::key(&mut scene.app, KeyCode::F10, true);
    scene.step(false);
    latency_tests::key(&mut scene.app, KeyCode::F10, false);
    scene.step(false);
    assert!(
        scene
            .app
            .world()
            .resource::<crate::combat::CombatState>()
            .requested_in(Some(11))
    );
    assert_eq!(
        camera_locked(&scene),
        locked,
        "the actual attack/target must establish the requested camera owner"
    );
    point_view(&mut scene, 0.83, false);
    scene.step(false);
    scene
}

fn assert_camera_only(scene: &mut Scene, before: View, yaw: f32) {
    let after = view(scene);
    assert!(
        (after.yaw - yaw).abs() < 1e-5,
        "camera yaw {} != {yaw}",
        after.yaw
    );
    assert!(
        after
            .actor
            .translation
            .abs_diff_eq(before.actor.translation, 1e-5)
    );
    assert!(after.actor.rotation.angle_between(before.actor.rotation) < 0.001);
    let aim = scene.app.world().resource::<MouseAim>();
    assert!(aim.horizontal.is_none() && aim.heading.is_none());
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .mouse_heading
            .is_none()
    );
    assert!(camera_locked(scene));
}

#[test]
fn locked_rmb_idle_drag_and_release_keep_the_actor_and_view_continuous() {
    for delay in [0, 6] {
        for first_person in [false, true] {
            let mut scene = scene(true, delay);
            point_view(&mut scene, 0.83, first_person);
            scene.step(false);
            let before = view(&mut scene);
            camera::testing::button(&mut scene.app, MouseButton::Right, true);
            scene.step(false);
            assert!(scene.app.world().resource::<MouseLook>().held);
            assert_camera_only(&mut scene, before, before.yaw);
            assert!(
                view(&mut scene)
                    .camera
                    .rotation
                    .abs_diff_eq(before.camera.rotation, 1e-5)
            );
            let yaw = before.yaw - 73. * turn_per_pixel(&mut scene);
            motion(&mut scene, Vec2::new(73., 20.));
            scene.step(false);
            assert_camera_only(&mut scene, before, yaw);
            let dragged = view(&mut scene);
            assert!(
                dragged
                    .camera
                    .rotation
                    .angle_between(before.camera.rotation)
                    > 0.01
            );
            // Motion arriving on release is discarded, rather than replayed
            // into body facing or the next capture.
            camera::testing::button(&mut scene.app, MouseButton::Right, false);
            motion(&mut scene, Vec2::splat(1000.));
            for _ in 0..12 {
                scene.step(false);
                assert!(!scene.app.world().resource::<MouseLook>().held);
                assert_camera_only(&mut scene, before, yaw);
                let after = view(&mut scene);
                assert!((after.pitch - dragged.pitch).abs() < 1e-5);
                assert!(after.camera.rotation.angle_between(dragged.camera.rotation) < 0.001);
            }
            assert_eq!(scene.refusals, 0);
        }
    }
}

#[test]
fn locked_rmb_drag_preserves_concurrent_target_relative_orbit_direction() {
    for (code, side) in [(KeyCode::KeyA, -1.), (KeyCode::KeyD, 1.)] {
        for drag in [-73., 73.] {
            let mut scene = scene(true, 6);
            let before = view(&mut scene);
            let yaw = before.yaw - drag * turn_per_pixel(&mut scene);
            camera::testing::button(&mut scene.app, MouseButton::Right, true);
            latency_tests::key(&mut scene.app, code, true);
            motion(&mut scene, Vec2::new(drag, 0.));
            for _ in 0..30 {
                let row = scene.step(false);
                let after = view(&mut scene);
                assert!((after.yaw - yaw).abs() < 1e-5);
                assert!(camera_locked(&scene));
                assert!(scene.app.world().resource::<MouseAim>().heading.is_none());
                assert!(row.focus.abs_diff_eq(row.shown, 1e-5));
            }
            let after = view(&mut scene);
            assert!(
                (after.actor.translation - before.actor.translation)
                    .dot((before.actor.rotation * Vec3::Z).cross(Vec3::Y))
                    * side
                    > 0.1,
                "RMB drag must not reverse/redefine the held orbit side"
            );
            assert!(
                scene
                    .ordinary_modes
                    .iter()
                    .any(|mode| matches!(mode, MovementMode::Strafe(_)))
            );
            assert_eq!(scene.refusals, 0);
        }
    }
}

#[test]
fn unlocked_rmb_still_faces_the_actor_from_the_existing_view() {
    let mut scene = scene(false, 0);
    let before = view(&mut scene);
    let yaw = before.yaw - 73. * turn_per_pixel(&mut scene);
    camera::testing::button(&mut scene.app, MouseButton::Right, true);
    motion(&mut scene, Vec2::new(73., 0.));
    scene.step(false);
    let after = view(&mut scene);
    assert!(!camera_locked(&scene));
    assert!(scene.app.world().resource::<MouseAim>().heading.is_some());
    assert!(after.actor.rotation.angle_between(before.actor.rotation) > 0.1);
    let actor_forward = after.actor.rotation * Vec3::Z;
    let camera_forward = after.camera.forward().with_y(0.).normalize();
    assert!(actor_forward.abs_diff_eq(camera_forward, 1e-4));
    assert!((after.yaw - yaw).abs() < 1e-4);
    assert!(
        after
            .actor
            .translation
            .abs_diff_eq(before.actor.translation, 1e-5)
    );
    assert_eq!(scene.refusals, 0);
}

#[test]
fn retained_native_ground_facing_cannot_recenter_a_locked_camera() {
    for locked in [false, true] {
        let mut scene = scene(locked, 0);
        let before = view(&mut scene);
        // Retain a valid observed ground frame from the real fixture. Its
        // follow flag can outlive the turn/preview owner that produced it.
        let mut controls = scene.app.world_mut().resource_mut::<Controls>();
        let frame = controls
            .ground
            .current
            .as_mut()
            .expect("native idle ground frame");
        frame.follows = true;
        frame.drawn.rotation = Quat::from_rotation_y(eq_client_core::render_heading(64.));
        scene.app.world_mut().run_system_once(interpolate).unwrap();
        scene
            .app
            .world_mut()
            .run_system_once(camera::orbit_camera)
            .unwrap();
        let after = view(&mut scene);
        if locked {
            // Lock keeps the actor facing its target even if an older ground
            // frame still asks the camera to follow another heading.
            let target_bearing = Vec3::from_array(eq_client_core::render_vector([0., 1., 0.]));
            assert!((after.actor.rotation * Vec3::Z).abs_diff_eq(target_bearing, 1e-4));
            assert!((after.yaw - before.yaw).abs() < 1e-5);
            assert!(after.camera.rotation.angle_between(before.camera.rotation) < 0.001);
        } else {
            assert!(after.actor.rotation.angle_between(before.actor.rotation) > 0.1);
            let actor_forward = after.actor.rotation * Vec3::Z;
            assert!(actor_forward.abs_diff_eq(after.camera.forward().with_y(0.).normalize(), 1e-4));
        }
    }
}

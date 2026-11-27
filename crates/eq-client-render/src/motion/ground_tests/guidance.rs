//! Normal guidance requests through Input/Receive/Present and the unchanged
//! local session guard. These are CPU-side regressions, not gameplay evidence.
use super::*;
use crate::{camera, script::Script};
use bevy::{input::mouse::MouseMotion, window::WindowFocused};

fn actor(scene: &mut Scene) -> Transform {
    let world = scene.app.world_mut();
    *world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap()
}

fn heading(transform: Transform) -> f32 {
    let forward = transform.rotation * Vec3::Z;
    eq_client_core::world_heading(forward.x.atan2(forward.z))
}

fn request(scene: &mut Scene, goal: f32, moving: bool) {
    scene.app.insert_resource(
        Script::new(Vec::new())
            .holding_for_test(
                if moving {
                    vec![Act::Forward]
                } else {
                    Vec::new()
                },
                None,
            )
            .guidance_for_test(goal),
    );
}

/// The same desired waypoint bearing the route publishes; every orientation
/// and position change is still produced by the actual input/frame systems.
fn waypoint(scene: &mut Scene, target: Vec3) {
    let toward = target - actor(scene).translation;
    let goal = eq_client_core::world_heading(toward.x.atan2(toward.z));
    scene
        .app
        .world_mut()
        .resource_mut::<Script>()
        .guide_for_test(goal);
}

#[test]
fn guidance_ninety_and_half_turn_waypoints_keep_normal_rates_and_arrive() {
    for delay in [0, 6] {
        for native in [false, true] {
            for last in [Vec3::new(20., 3., 20.), Vec3::new(0., 3., -20.)] {
                let mut scene = Scene::new(delay, true);
                scene
                    .app
                    .world_mut()
                    .resource_mut::<Controls>()
                    .suppress_ground = !native;
                request(&mut scene, 0., true);
                let points = [Vec3::new(0., 3., 20.), last];
                let mut index = 0;
                let mut corner = None;
                let mut travelled_during_turn = 0.;
                for _ in 0..1800 {
                    let before = actor(&mut scene);
                    waypoint(&mut scene, points[index]);
                    let row = scene.step(false);
                    let after = actor(&mut scene);
                    let turned = arc(heading(before), heading(after)).abs();
                    assert!(
                        turned <= TURN_RATE / TURN_STEPS_PER_SECOND + 0.001,
                        "one frame snapped from {} to {}",
                        heading(before),
                        heading(after)
                    );
                    if index == 1 && corner.is_none() {
                        assert!(
                            turned > 0. && turned <= 1. / 3. + 0.001,
                            "corner acquisition must start the ordinary ramp"
                        );
                        corner = Some(heading(before));
                    }
                    if corner.is_some() && turned > 0.001 {
                        travelled_during_turn += after.translation.distance(before.translation);
                    }
                    let accepted = Vec3::from_array(eq_client_core::render_position(row.accepted));
                    if after.translation.distance(points[index]) < 1.
                        && accepted.distance(points[index]) < 1.
                    {
                        index += 1;
                        if index == points.len() {
                            break;
                        }
                    }
                }
                assert_eq!(
                    index,
                    points.len(),
                    "unchanged one-unit arrival, native={native} delay={delay} goal={last:?}"
                );
                assert!(
                    travelled_during_turn > 0.05,
                    "Forward keeps travelling during the turn"
                );
                assert_eq!(scene.refusals, 0);
                scene.app.world_mut().remove_resource::<Script>();
                for _ in 0..30 {
                    scene.step(false);
                }
            }
        }
    }
}

#[test]
fn guidance_new_run_restarts_the_ramp_from_the_drawn_facing() {
    let mut scene = Scene::new(6, true);
    request(&mut scene, 128., false);
    for _ in 0..20 {
        scene.step(false);
    }
    let before = heading(actor(&mut scene));
    let old = scene
        .app
        .world()
        .resource::<Script>()
        .status()
        .run()
        .clone();
    request(&mut scene, 0., false);
    assert!(!std::sync::Arc::ptr_eq(
        &old,
        scene.app.world().resource::<Script>().status().run()
    ));
    scene.step(false);
    let after = heading(actor(&mut scene));
    assert!((arc(before, after) + 1. / 3.).abs() < 0.001);
    assert_eq!(scene.refusals, 0);
}

#[test]
fn guidance_native_refusal_restores_the_admitted_facing() {
    let mut scene = Scene::new(0, true);
    request(&mut scene, 128., false);
    for _ in 0..20 {
        scene.step(true);
    }
    assert!(scene.app.world().resource::<Controls>().ground.active());
    scene.early_refuse = true;
    for _ in 0..12 {
        scene.step(true);
        if scene.app.world().resource::<Controls>().refused.is_some() {
            let shown = heading(actor(&mut scene));
            let admitted = scene
                .app
                .world()
                .resource::<online::OnlineState>()
                .world()
                .player()
                .unwrap()
                .position
                .heading;
            assert!(arc(shown, admitted).abs() < 0.001);
            return;
        }
    }
    panic!("fixture did not deliver its actual native refusal");
}

fn install_native_look(scene: &mut Scene) {
    scene
        .app
        .add_message::<WindowFocused>()
        .add_systems(PreUpdate, camera::look.after(bevy::input::InputSystems))
        .add_systems(
            Update,
            camera::turn
                .before(input)
                .in_set(crate::systems::Stage::Input),
        );
}

#[test]
fn guidance_yields_presentation_to_unlocked_native_rmb_without_hiding_the_press() {
    let mut scene = Scene::new(0, true);
    install_native_look(&mut scene);
    request(&mut scene, 128., false);
    for _ in 0..6 {
        scene.step(false);
    }
    camera::testing::button(&mut scene.app, MouseButton::Right, true);
    scene.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(50., 0.),
    });
    scene.step(false);
    assert!(scene.app.world().resource::<camera::MouseLook>().held);
    assert!(
        scene
            .app
            .world()
            .resource::<camera::MouseAim>()
            .heading
            .is_some()
    );
    assert!(
        scene
            .app
            .world()
            .resource::<Controls>()
            .key_facing
            .is_none()
    );
    // The movement-only input worker owns route interruption/resumption.
    // Guidance does not consume the native event or invent that policy here.
    assert_eq!(scene.refusals, 0);
}

#[test]
fn guidance_does_not_enable_detached_camera_character_travel() {
    let mut scene = Scene::new(0, true);
    scene.app.add_systems(
        Update,
        camera::toggle_free
            .before(input)
            .in_set(crate::systems::Stage::Input),
    );
    crate::keys::testing::bind(
        &mut scene.app,
        Act::FreeCamera,
        crate::keys::Chord::key(KeyCode::F9),
    );
    scene
        .app
        .world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::FreeCamera, true);
    let before = actor(&mut scene);
    request(&mut scene, 128., true);
    latency_tests::key(&mut scene.app, KeyCode::F9, true);
    scene.step(false);
    latency_tests::key(&mut scene.app, KeyCode::F9, false);
    for _ in 0..30 {
        scene.step(false);
        let after = actor(&mut scene);
        assert!(after.translation.abs_diff_eq(before.translation, 1e-5));
        assert!(after.rotation.angle_between(before.rotation) < 0.001);
        let world = scene.app.world_mut();
        assert_eq!(world.query::<&camera::FreeCamera>().iter(world).count(), 1);
        assert!(world.resource::<Controls>().key_facing.is_none());
    }
    assert_eq!(scene.refusals, 0);
}

#[test]
fn melee_and_orbit_facing_use_the_shared_ramp_while_locked_rmb_stays_independent() {
    for orbit in [false, true] {
        let mut scene = super::camera_locked::scene(true, 6);
        if orbit {
            latency_tests::key(&mut scene.app, KeyCode::KeyD, true);
        } else {
            let mut online = scene.app.world_mut().resource_mut::<online::OnlineState>();
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::Position {
                    spawn_id: 5,
                    position: world_position([30., 3., 0.], 0.),
                    velocity: [0.; 3],
                    forward: 0.,
                }],
            );
        }
        camera::testing::button(&mut scene.app, MouseButton::Right, true);
        scene.app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(73., 0.),
        });
        let mut prior = actor(&mut scene);
        let mut independent_yaw: Option<f32> = None;
        for _ in 0..60 {
            scene.step(false);
            let after = actor(&mut scene);
            assert!(
                arc(heading(prior), heading(after)).abs()
                    <= TURN_RATE / TURN_STEPS_PER_SECOND + 0.001
            );
            let world = scene.app.world_mut();
            let yaw = world.query::<&OrbitCamera>().single(world).unwrap().yaw;
            if let Some(expected) = independent_yaw {
                assert!(
                    (yaw - expected).abs() < 1e-5,
                    "guidance must not pull locked RMB view back toward the route"
                );
            }
            independent_yaw = Some(yaw);
            prior = after;
        }
        if orbit {
            assert!(
                scene
                    .app
                    .world()
                    .resource::<camera::MouseAim>()
                    .heading
                    .is_none()
            );
            assert!(
                scene
                    .ordinary_modes
                    .iter()
                    .any(|mode| matches!(mode, MovementMode::Strafe(_)))
            );
        }
        assert_eq!(scene.refusals, 0);
    }
}

//! Matched physical turn-release controls across ordinary and native owners.
use super::*;
use serde_json::json;

fn plane(y: f32) -> [[[f32; 3]; 3]; 2] {
    [
        [[-2000., y, -2000.], [-2000., y, 2000.], [2000., y, -2000.]],
        [[2000., y, -2000.], [-2000., y, 2000.], [2000., y, 2000.]],
    ]
}

fn deck(native: bool, heading: f32) -> CarrierFixture {
    use eq_client_core::movement::CollisionMesh;
    let mut ship = crate::online::testing::pet(172, 7);
    ship.race = 72;
    ship.gender = 2;
    ship.position = eq_client_core::WorldPosition::default();
    ship.pet_owner = None;
    let mut collision = CollisionWorld::new(plane(-30.)).unwrap().with_liquids(Dry);
    collision.set_spawn_obstacles(vec![(
        172,
        std::sync::Arc::new(CollisionMesh::new(plane(0.)).unwrap()),
    )]);
    CarrierFixture {
        collision,
        origin: world_position([0., 3., 0.], heading),
        body: PlayerBody {
            feet_offset: 3.,
            height: 6.,
            size: 6.,
        },
        ship,
        native,
        clock: None,
    }
}

fn snapshot(scene: &mut Scene) -> serde_json::Value {
    let world = scene.app.world_mut();
    let transform = *world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap();
    let (camera, drawn) = world
        .query::<(&OrbitCamera, &Transform)>()
        .single(world)
        .unwrap();
    let forward = transform.rotation * Vec3::Z;
    let shown_heading = eq_client_core::world_heading(forward.x.atan2(forward.z));
    let camera_yaw = camera.yaw;
    let camera_position = drawn.translation;
    let camera_rotation = drawn.rotation;
    let controls = world.resource::<Controls>();
    json!({
        "frame":scene.frame,
        "shown":transform.translation.to_array(),
        "shown_heading":shown_heading,
        "camera_yaw":camera_yaw,
        "camera_position":camera_position.to_array(),
        "camera_rotation":camera_rotation.to_array(),
        "accepted_heading":scene.owner.position().heading,
        "world_heading":world.resource::<online::OnlineState>().world().player().unwrap().position.heading,
        "ground_active":controls.ground.active(),
        "ground_facing":controls.ground.current.map(|frame| frame.facing),
        "ground_heading":controls.ground.current.map(|frame| frame.position.heading),
        "boat":controls.boat.map(|boat| boat.sample.spawn_id),
        "drawn_boat":controls.visual_boat.map(|boat| boat.sample.spawn_id),
        "pending":controls.pending.is_some(),
        "packets":scene.packets.len(),
        "observations":scene.observations,
        "ordinary_requests":scene.ordinary_modes.len(),
    })
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "matched per-frame ownership and release diagnostic"
)]
fn turn_release_matched_legacy_ground_and_retained_boat() {
    let mut failures = Vec::new();
    for native in [false, true] {
        for boat in [false, true] {
            for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight] {
                for moving in [false, true] {
                    for delay in [0, 6] {
                        for held_frames in [29, 32, 35] {
                            // Seed owners through normal construction. A genuine
                            // correction deliberately withdraws player input and
                            // must not be used as fixture placement. Right turns
                            // wrap from0; the boat's left turns wrap from511.5.
                            let heading = if boat { 511.5 } else { 0. };
                            let mut scene = if boat {
                                Scene::with_carrier_fixture(
                                    delay,
                                    true,
                                    6.,
                                    false,
                                    Some(deck(native, heading)),
                                )
                            } else {
                                let mut scene = Scene::with_speed(delay, true, 6.);
                                scene
                                    .app
                                    .world_mut()
                                    .resource_mut::<Controls>()
                                    .suppress_ground = !native;
                                scene
                            };
                            scene.app.insert_resource(crate::keys::KeyMap::default());
                            {
                                let world = scene.app.world_mut();
                                world
                                    .query_filtered::<&mut Transform, With<Player>>()
                                    .single_mut(world)
                                    .unwrap()
                                    .rotation =
                                    Quat::from_rotation_y(eq_client_core::render_heading(heading));
                                world
                                    .query::<&mut OrbitCamera>()
                                    .single_mut(world)
                                    .unwrap()
                                    .yaw =
                                    crate::camera::behind(eq_client_core::render_heading(heading));
                                if boat {
                                    let sample = world
                                        .resource::<online::OnlineState>()
                                        .world()
                                        .carriers()
                                        .latest(172)
                                        .unwrap()
                                        .sample();
                                    world.resource_mut::<Controls>().boat = Some(boats::Riding {
                                        sample,
                                        pose: eq_client_core::WorldPosition::default(),
                                        position: world_position([0., 3., 0.], heading),
                                    });
                                }
                            }
                            for _ in 0..8 {
                                scene.step(false);
                            }
                            latency_tests::key(&mut scene.app, code, true);
                            let mut rows = Vec::new();
                            for _ in 0..held_frames {
                                scene.step(moving);
                                rows.push(snapshot(&mut scene));
                            }
                            let before_release = rows.last().unwrap().clone();
                            if boat {
                                assert_eq!(before_release["boat"], 172, "fixture lost its carrier");
                                assert!(scene.ordinary_supports.iter().any(|support| {
                                    matches!(support, Some(Support::Spawn(sample)) if sample.spawn_id == 172)
                                }), "fixture never submitted supported ordinary movement");
                            } else if native {
                                assert_eq!(
                                    before_release["ground_active"], true,
                                    "fixture never owns native frames"
                                );
                                assert!(scene.observations > 0);
                            } else {
                                assert_eq!(scene.observations, 0);
                            }
                            latency_tests::key(&mut scene.app, code, false);
                            scene.step(moving);
                            let released = snapshot(&mut scene);
                            let release_heading = released["shown_heading"].as_f64().unwrap();
                            let previous_heading =
                                before_release["shown_heading"].as_f64().unwrap();
                            assert!(
                                ((release_heading - previous_heading + 256.).rem_euclid(512.)
                                    - 256.)
                                    .abs()
                                    < 0.000_1,
                                "the key-up frame consumed another turn step"
                            );
                            rows.push(released.clone());
                            let release_yaw = released["camera_yaw"].as_f64().unwrap();
                            let mut maximum_heading_tail = 0_f64;
                            let mut maximum_camera_tail = 0_f64;
                            for _ in 0..24 {
                                scene.step(moving);
                                let row = snapshot(&mut scene);
                                let delta = (row["shown_heading"].as_f64().unwrap()
                                    - release_heading
                                    + 256.)
                                    .rem_euclid(512.)
                                    - 256.;
                                maximum_heading_tail = maximum_heading_tail.max(delta.abs());
                                let yaw = row["camera_yaw"].as_f64().unwrap() - release_yaw;
                                maximum_camera_tail =
                                    maximum_camera_tail.max(yaw.sin().atan2(yaw.cos()).abs());
                                rows.push(row);
                            }
                            let packets = scene.packets.iter().map(|(at, position, animation)| {
                                json!({"at_ns":at.duration_since(scene.base).as_nanos(),
                                    "heading":position.heading,"position":[position.x, position.y, position.z],
                                    "animation":animation})
                            }).collect::<Vec<_>>();
                            println!(
                                "TURN_RELEASE {}",
                                json!({"native":native,"boat":boat,
                                "key":format!("{code:?}"),"moving":moving,"delay":delay,"held_frames":held_frames,
                                "maximum_heading_tail":maximum_heading_tail,"maximum_camera_tail":maximum_camera_tail,
                                "rows":rows,"packets":packets})
                            );
                            assert_eq!(scene.refusals, 0, "ordinary guard refused fixture");
                            if maximum_heading_tail > 0.000_1 || maximum_camera_tail > 0.000_1 {
                                failures.push((
                                    native,
                                    boat,
                                    code,
                                    moving,
                                    delay,
                                    held_frames,
                                    maximum_heading_tail,
                                    maximum_camera_tail,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "turn continued after physical release: {failures:?}"
    );
}

#[test]
fn native_refusal_retires_keyboard_facing_before_ordinary_handover() {
    let mut scene = Scene::with_speed(0, true, 6.);
    scene.app.insert_resource(crate::keys::KeyMap::default());
    for _ in 0..30 {
        scene.step(true);
    }
    latency_tests::key(&mut scene.app, KeyCode::ArrowLeft, true);
    for _ in 0..20 {
        scene.step(true);
    }
    assert!(scene.app.world().resource::<Controls>().ground.active());
    scene.early_refuse = true;
    latency_tests::key(&mut scene.app, KeyCode::ArrowLeft, false);
    for _ in 0..12 {
        scene.step(true);
        if scene.app.world().resource::<Controls>().refused.is_some() {
            let row = snapshot(&mut scene);
            println!("NATIVE_REFUSAL {row}");
            let shown = row["shown_heading"].as_f64().unwrap();
            let accepted = row["accepted_heading"].as_f64().unwrap();
            assert!(
                ((shown - accepted + 256.).rem_euclid(512.) - 256.).abs() < 0.000_1,
                "native refusal must restore the accepted facing: {row}"
            );
            return;
        }
    }
    panic!("fixture never delivered its actual native refusal");
}

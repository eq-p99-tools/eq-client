//! Offline production script entry, with no OS input or network connection.
use super::*;
use crate::online::testing;
use crate::script::{
    FORWARD, InputMode, Phase,
    tests::{background, frame_at},
};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use std::time::{Duration, Instant};

fn floor() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[-30.0, 0.0, -30.0], [-30.0, 0.0, 30.0], [30.0, 0.0, 30.0]],
        [[-30.0, 0.0, -30.0], [30.0, 0.0, 30.0], [30.0, 0.0, -30.0]],
    ]
}

fn pending_walk() -> App {
    place_walk(true)
}

fn place_walk(mesh_pending: bool) -> App {
    let (mut app, _) = background(Vec::new());
    let body = PlayerBody {
        feet_offset: 3.75,
        height: 6.677_319_5,
        size: 6.0,
    };
    app.insert_resource(crate::Collision(Some(
        CollisionWorld::new(floor()).unwrap(),
    )));
    app.world_mut()
        .spawn((crate::Player, body, Transform::from_xyz(0.0, 3.75, 0.0)));
    let zone = {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        let mut player = testing::player(7);
        player.position = eq_client_core::world_position([0.0, 3.75, 0.0], 0.0);
        testing::admit(&mut online, 1, player);
        assert!(online.world().target().selected.is_none());
        online.world().zone().to_owned()
    };
    crate::zone_navmesh::install(&mut app, mesh_pending);
    if mesh_pending {
        app.world_mut()
            .resource_mut::<ZoneNavMesh>()
            .hold_for_test(&zone, floor());
    }
    // Use the same header-dependent settings as production refresh so it
    // does not replace our deliberately pending task on the first frame.
    let viewer = app.world().resource::<crate::ViewerSettings>();
    let settings = crate::zone_navmesh::settings(
        viewer.0.installed_client,
        eq_client_assets::scale::ZoneScale {
            classic_reduced: false,
        },
    );
    // The helper accepts a production-shaped input without executing a bake.
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .set_test_settings(settings);
    app.insert_resource(
        Script::new(vec![Step::WalkPlace {
            destination: PlaceDestination::Coordinates([0.0, -20.0, 0.0]),
            range: 1.0,
            duration: Duration::from_secs(20),
        }])
        .offline_preview(true)
        .input_mode(InputMode::Background),
    );
    app
}

fn authored_camera(app: &mut App) -> Entity {
    app.world_mut()
        .run_system_once(|mut commands: Commands| {
            crate::camera::spawn_camera(
                &mut commands,
                Transform::from_xyz(0., 3.75, 0.),
                30.,
                crate::ProjectionStyle::Perspective,
            );
        })
        .unwrap();
    let world = app.world_mut();
    let entity = world
        .query_filtered::<Entity, With<crate::OrbitCamera>>()
        .single(world)
        .unwrap();
    world.get_mut::<crate::OrbitCamera>(entity).unwrap().yaw = 0.83;
    entity
}

#[test]
fn grid_and_navmesh_walk_entry_publish_guidance_without_writing_camera_yaw() {
    for mesh in [false, true] {
        let mut app = place_walk(mesh);
        let camera = authored_camera(&mut app);
        frame_at(&mut app, 0);
        if mesh {
            let ready = NavMesh::build(
                &floor(),
                &eq_client_core::movement::navmesh::Settings::walker(6.),
                1,
                &AtomicBool::new(false),
            )
            .unwrap()
            .0;
            app.world_mut()
                .resource_mut::<ZoneNavMesh>()
                .complete_for_test(Some(ready));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut ms = 50;
        loop {
            frame_at(&mut app, ms);
            assert!(
                Instant::now() < deadline,
                "normal route did not publish guidance"
            );
            let script = app.world().resource::<Script>();
            assert_eq!(script.status().phase(), Phase::Running);
            if let Some(goal) = script.guidance {
                assert!((goal.rem_euclid(512.) - 256.).abs() < 0.001);
                assert_eq!(script.acts(), &[crate::keys::Act::Forward]);
                assert!(
                    (app.world().get::<crate::OrbitCamera>(camera).unwrap().yaw - 0.83).abs()
                        < 1e-5
                );
                break;
            }
            ms += 1;
            std::thread::yield_now();
        }
    }
}

#[test]
fn explicit_camera_only_face_keeps_its_existing_instant_view_semantics() {
    let mut app = place_walk(false);
    let camera = authored_camera(&mut app);
    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        let mut target = testing::pet(9, 0);
        target.position = eq_client_core::world_position([20., 3.75, 0.], 0.);
        testing::spawn_entry(&mut online, 9, target);
        online.select_target(Some(9));
        let request_id = online.world().next_target_request() - 1;
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::TargetChanged {
                session_id: 1,
                request_id,
                state: eq_network_game::targets::TargetState {
                    revision: 1,
                    spawn_id: Some(9),
                    available: true,
                },
            }],
        );
    }
    app.insert_resource(Script::new(vec![Step::Face]).input_mode(InputMode::Background));
    frame_at(&mut app, 0);
    let script = app.world().resource::<Script>();
    assert!(script.guidance().is_none());
    assert!((app.world().get::<crate::OrbitCamera>(camera).unwrap().yaw - 0.83).abs() > 0.1);
    let world = app.world_mut();
    let player = world
        .query_filtered::<&Transform, With<crate::Player>>()
        .single(world)
        .unwrap();
    assert!(player.rotation.angle_between(Quat::IDENTITY) < 0.001);
}

#[test]
fn a_place_walk_waits_for_the_current_build_then_plans_without_a_target() {
    let mut app = pending_walk();
    for ms in [0, 50, 100, 500] {
        frame_at(&mut app, ms);
        let script = app.world().resource::<Script>();
        assert_eq!(script.status().phase(), Phase::Running);
        assert!(script.route.is_none());
        assert!(!script.acts.contains(&FORWARD));
        assert_eq!(
            script.walk_waiting,
            Some("current-zone navigation mesh is building")
        );
    }
    let mesh = NavMesh::build(
        &floor(),
        &eq_client_core::movement::navmesh::Settings::walker(6.0),
        1,
        &AtomicBool::new(false),
    )
    .unwrap()
    .0;
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(Some(mesh));
    let start = Instant::now();
    let mut ms = 550;
    loop {
        frame_at(&mut app, ms);
        let script = app.world().resource::<Script>();
        assert_eq!(script.status().phase(), Phase::Running);
        if script.route.is_some() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        ms += 1;
        std::thread::yield_now();
    }
    assert!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected
            .is_none()
    );
    assert!(app.world().resource::<Script>().place.is_some());
}

#[test]
fn a_pending_walk_cannot_reuse_a_route_after_an_admission_change() {
    let mut app = pending_walk();
    frame_at(&mut app, 0);
    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        2,
        testing::player(7),
    );
    frame_at(&mut app, 50);
    let script = app.world().resource::<Script>();
    assert_eq!(script.status().phase(), Phase::Failed);
    assert!(script.route.is_none());
    assert!(!script.acts.contains(&FORWARD));
}

#[test]
fn a_failed_current_zone_build_does_not_silently_start_grid_search() {
    let mut app = pending_walk();
    frame_at(&mut app, 0);
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(None);
    let start = Instant::now();
    let mut ms = 50;
    while app.world().resource::<Script>().status().phase() != Phase::Failed {
        frame_at(&mut app, ms);
        assert!(start.elapsed() < Duration::from_secs(30));
        ms += 1;
        std::thread::yield_now();
    }
    assert!(app.world().resource::<Script>().route.is_none());
}

#[test]
fn place_completion_needs_the_requested_floor_and_reports_partial_or_timeout() {
    let goal = Vec3::new(0.0, 8.0, 0.0);
    assert_eq!(end(Vec3::ZERO, goal, 1.0, false, false), Ok(false));
    assert!(end(Vec3::ZERO, goal, 1.0, true, false).is_err());
    assert!(end(Vec3::ZERO, goal, 1.0, false, true).is_err());
    assert_eq!(end(goal, goal, 1.0, true, true), Ok(true));
}

#[test]
fn exit_selection_skips_an_unreachable_nearby_floor() {
    let world = CollisionWorld::new(floor()).unwrap();
    let stop = AtomicBool::new(false);
    let goal = choose_exit(
        vec![[0.0, 8.0, 0.0], [12.0, 0.0, 0.0]],
        &world,
        None,
        Vec3::ZERO,
        6.0,
        1.0,
        &[],
        false,
        &stop,
    )
    .unwrap();
    assert_eq!(goal, Vec3::new(12.0, 0.0, 0.0));
    stop.store(true, Ordering::Relaxed);
    assert!(
        choose_exit(
            vec![[12.0, 0.0, 0.0]],
            &world,
            None,
            Vec3::ZERO,
            6.0,
            1.0,
            &[],
            false,
            &stop
        )
        .is_err()
    );
}

#[test]
fn zone_selection_resolves_received_references_and_preserves_absolute_and_explicit_exits() {
    use PlaceDestination::{ExitReference, ExitZone};
    let zones = [(3, 54), (54, 61)].into_iter().collect();
    assert!(matches_exit(ExitZone(54), ZoneLine::Reference(3), &zones));
    assert!(!matches_exit(ExitZone(54), ZoneLine::Reference(54), &zones));
    assert!(!matches_exit(
        ExitZone(54),
        ZoneLine::Reference(3),
        &std::collections::BTreeMap::new()
    ));
    assert!(matches_exit(
        ExitReference(3),
        ZoneLine::Reference(3),
        &std::collections::BTreeMap::new()
    ));
    assert!(matches_exit(
        ExitZone(54),
        ZoneLine::Absolute {
            zone_id: 54,
            position: [700., 800., 900.],
            heading: 999.
        },
        &std::collections::BTreeMap::new()
    ));
}

fn exit_app(destination: PlaceDestination, mapped: bool) -> App {
    let mut app = pending_walk();
    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        online.regions = eq_client_assets::regions::ZoneRegions::test_zone_line_slab(
            8.0,
            12.0,
            ZoneLine::Reference(3),
        );
        if mapped {
            testing::news(
                &mut online,
                [eq_client_core::WorldEvent::ZoneLineZones {
                    session_id: 1,
                    zones: [(3, 54)].into_iter().collect(),
                }],
            );
        }
    }
    app.insert_resource(
        Script::new(vec![Step::WalkPlace {
            destination,
            range: 1.,
            duration: Duration::from_secs(20),
        }])
        .offline_preview(true)
        .input_mode(InputMode::Background),
    );
    let mesh = NavMesh::build(
        &floor(),
        &eq_client_core::movement::navmesh::Settings::walker(6.0),
        1,
        &AtomicBool::new(false),
    )
    .unwrap()
    .0;
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(Some(mesh));
    app
}

fn finish_lookup(app: &mut App, failed: bool) {
    let start = Instant::now();
    let first =
        u64::try_from(app.world().resource::<Time<Real>>().elapsed().as_millis()).unwrap() + 1;
    for ms in first.. {
        frame_at(app, ms);
        let script = app.world().resource::<Script>();
        if failed {
            if script.status().phase() == Phase::Failed {
                assert!(!script.acts.contains(&FORWARD));
                assert!(script.place.is_none());
                return;
            }
        } else {
            assert_eq!(script.status().phase(), Phase::Running);
            if let Some(place) = &script.place {
                assert!((8.0..=12.0).contains(&place.feet.x));
                assert!(place.feet.y.abs() < 0.01);
                assert!(script.route.is_some());
                assert!(
                    app.world()
                        .resource::<OnlineState>()
                        .world()
                        .target()
                        .selected
                        .is_none()
                );
                return;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::yield_now();
    }
}

#[test]
fn a_zone_exit_uses_local_authored_floor_then_replans_on_map_invalidation() {
    let mut app = exit_app(PlaceDestination::ExitZone(54), true);
    finish_lookup(&mut app, false);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [eq_client_core::WorldEvent::ZoneLineZones {
            session_id: 1,
            zones: std::collections::BTreeMap::new(),
        }],
    );
    finish_lookup(&mut app, true);
}

#[test]
fn a_missing_exit_zone_map_does_not_move_but_explicit_reference_still_works() {
    finish_lookup(&mut exit_app(PlaceDestination::ExitZone(54), false), true);
    finish_lookup(
        &mut exit_app(PlaceDestination::ExitReference(3), false),
        false,
    );
}

fn a_walk_with_body(body: PlayerBody) -> App {
    let mut app = pending_walk();
    let world = app.world_mut();
    *world.query::<&mut PlayerBody>().single_mut(world).unwrap() = body;
    world
        .query_filtered::<&mut Transform, With<crate::Player>>()
        .single_mut(world)
        .unwrap()
        .translation
        .y = body.feet_offset;
    let mut player = testing::player(7);
    player.position = eq_client_core::world_position([0.0, body.feet_offset, 0.0], 0.0);
    testing::admit(&mut world.resource_mut::<OnlineState>(), 1, player);
    app
}

#[test]
fn a_script_refuses_a_body_without_a_fitting_mesh_profile_before_any_movement() {
    let ordinary = PlayerBody {
        size: 6.0,
        height: 6.677_319_5,
        feet_offset: 3.75,
    };
    for body in [
        PlayerBody {
            size: 10.0,
            ..ordinary
        },
        PlayerBody {
            height: 12.0,
            ..ordinary
        },
        PlayerBody {
            feet_offset: 6.0,
            ..ordinary
        },
    ] {
        let mut app = a_walk_with_body(body);
        let mesh = NavMesh::build(
            &floor(),
            &eq_client_core::movement::navmesh::Settings::walker(6.0),
            1,
            &AtomicBool::new(false),
        )
        .unwrap()
        .0;
        app.world_mut()
            .resource_mut::<ZoneNavMesh>()
            .complete_for_test(Some(mesh));
        let start = Instant::now();
        for ms in 0.. {
            frame_at(&mut app, ms);
            let script = app.world().resource::<Script>();
            assert!(!script.acts.contains(&FORWARD));
            assert!(script.route.is_none());
            if script.status().phase() == Phase::Failed {
                assert_eq!(
                    script.status().events().back().unwrap().message,
                    "no navigation mesh profile fits this body's size and headroom"
                );
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::yield_now();
        }
    }
}

#[test]
fn explicit_grid_only_walk_still_runs_for_a_body_outside_mesh_profiles() {
    let mut app = a_walk_with_body(PlayerBody {
        size: 6.0,
        height: 12.0,
        feet_offset: 3.75,
    });
    // The disabled resource is exactly the production --no-navmesh state;
    // the existing scheduled systems remain, but no mesh task is retained.
    app.insert_resource(ZoneNavMesh::default());
    let start = Instant::now();
    for ms in 0.. {
        frame_at(&mut app, ms);
        let script = app.world().resource::<Script>();
        assert_eq!(script.status().phase(), Phase::Running);
        if script.acts.contains(&FORWARD) {
            assert!(script.route.is_some());
            assert!(script.status().events().iter().any(|event| event.message
                == "Walk uses grid search: navigation meshes are off (--no-navmesh)"));
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::yield_now();
    }
}

mod movement_input;

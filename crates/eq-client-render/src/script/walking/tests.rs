//! Offline production script entry, with no OS input or network connection.
use super::*;
use crate::online::testing;
use crate::script::{
    FORWARD, InputMode, Phase,
    tests::{background, frame_at},
};
use bevy::prelude::*;
use std::time::{Duration, Instant};

fn floor() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[-30.0, 0.0, -30.0], [-30.0, 0.0, 30.0], [30.0, 0.0, 30.0]],
        [[-30.0, 0.0, -30.0], [30.0, 0.0, 30.0], [30.0, 0.0, -30.0]],
    ]
}

fn pending_walk() -> App {
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
    crate::zone_navmesh::install(&mut app, true);
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .hold_for_test(&zone, floor());
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
        Vec::new(),
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
            Vec::new(),
            false,
            &stop
        )
        .is_err()
    );
}

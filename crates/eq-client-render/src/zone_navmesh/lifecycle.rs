//! Synthetic lifecycle fixtures named for successive zones, not installed assets.
use super::*;

fn floor() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[-20.0, 0.0, -20.0], [-20.0, 0.0, 20.0], [20.0, 0.0, 20.0]],
        [[-20.0, 0.0, -20.0], [20.0, 0.0, 20.0], [20.0, 0.0, -20.0]],
    ]
}
fn mesh(triangles: &[[[f32; 3]; 3]]) -> navmesh::NavMesh {
    navmesh::NavMesh::build(
        triangles,
        &Settings::sized(walker_sizes()),
        1,
        &AtomicBool::new(false),
    )
    .unwrap()
    .0
}
fn finish(app: &mut App) {
    let start = Instant::now();
    while app.world().resource::<ZoneNavMesh>().making.is_some() {
        assert!(start.elapsed().as_secs() < 30, "completion did not arrive");
        app.update();
        std::thread::yield_now();
    }
}

#[test]
fn current_zone_failure_never_reports_ready_and_later_zone_recovers() {
    let mut app = App::new();
    install(&mut app, true);
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .hold_for_test("felwithea", floor());
    assert_eq!(
        app.world()
            .resource::<ZoneNavMesh>()
            .status("felwithea", Some(6.0))["phase"],
        "building"
    );
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(Some(mesh(&[])));
    finish(&mut app);
    let state = app.world().resource::<ZoneNavMesh>();
    assert_eq!(
        state.for_zone_walker("felwithea", 6.0).err(),
        Some(GridReason::Failed)
    );
    assert_eq!(state.status("felwithea", Some(6.0))["ready"], false);
    assert_eq!(state.status("felwithea", Some(6.0))["phase"], "failed");
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .hold_for_test("gfaydark", floor());
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(Some(mesh(&floor())));
    finish(&mut app);
    let state = app.world().resource::<ZoneNavMesh>();
    assert_eq!(state.status("gfaydark", Some(6.0))["phase"], "ready");
    assert_eq!(state.status("felwithea", Some(6.0))["ready"], false);
    assert_eq!(
        state.for_zone_walker("felwithea", 6.0).err(),
        Some(GridReason::Missing)
    );
}

#[test]
fn a_stale_zone_completion_cannot_install_into_the_current_zone() {
    let mut app = App::new();
    install(&mut app, true);
    let old = {
        let mut state = app.world_mut().resource_mut::<ZoneNavMesh>();
        state.hold_for_test("felwithea", floor());
        state.generation
    };
    {
        let mut state = app.world_mut().resource_mut::<ZoneNavMesh>();
        state.hold_for_test("gfaydark", floor());
        state.complete_for_test(Some(mesh(&floor())));
        let making = state.making.as_mut().unwrap();
        making.zone = "felwithea".into();
        making.generation = old;
    }
    finish(&mut app);
    let state = app.world().resource::<ZoneNavMesh>();
    assert!(state.mesh.is_none());
    assert_eq!(state.status("gfaydark", Some(6.0))["ready"], false);
}

#[test]
fn failed_task_and_disabled_feature_are_distinct() {
    let mut app = App::new();
    install(&mut app, true);
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .hold_for_test("zone", floor());
    app.world_mut()
        .resource_mut::<ZoneNavMesh>()
        .complete_for_test(None);
    finish(&mut app);
    assert_eq!(
        app.world().resource::<ZoneNavMesh>().status("zone", None)["phase"],
        "failed"
    );
    let mut disabled = App::new();
    install(&mut disabled, false);
    let mut state = disabled.world_mut().resource_mut::<ZoneNavMesh>();
    state.start("zone", floor(), None, None, Settings::sized(walker_sizes()));
    assert!(state.making.is_none());
    assert_eq!(state.status("zone", Some(6.0))["phase"], "disabled");
}

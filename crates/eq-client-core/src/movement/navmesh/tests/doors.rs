//! Synthetic door-separated mesh islands, with the real collision step as judge.
use super::*;
use crate::movement::{
    CollisionMesh, Navigator, PathProgress, Planned, Route, RouteSearch, RouteStep, Scope,
    SearchRequest,
};
use glam::Vec2;
use std::sync::Arc;
use std::time::Duration;

fn rectangle(x0: f32, x1: f32) -> Vec<[[f32; 3]; 3]> {
    quad(
        [x0, 0.0, -10.0],
        [x0, 0.0, 10.0],
        [x1, 0.0, 10.0],
        [x1, 0.0, -10.0],
    )
    .to_vec()
}

fn rooms(solid_wall: bool) -> (CollisionWorld, MeshNavigator) {
    // The mesh omits the narrow opening, as voxel erosion can do. This is
    // a deliberate synthetic mesh gap, not copied Felwithe geometry.
    let triangles: Vec<_> = rectangle(-30.0, -0.75)
        .into_iter()
        .chain(rectangle(0.75, 30.0))
        .collect();
    let (mesh, _) = build(&triangles);
    let mut triangles = rectangle(-30.0, 30.0);
    let spans = if solid_wall {
        vec![(-10.0, 10.0)]
    } else {
        vec![(-10.0, 0.5), (1.5, 10.0)]
    };
    for (low, high) in spans {
        triangles.extend(block([-0.05, 0.0, low], [0.05, 10.0, high]));
    }
    let mut world = CollisionWorld::new(triangles).unwrap();
    let door = CollisionMesh::new(block([-0.05, 0.0, 0.5], [0.05, 10.0, 1.5])).unwrap();
    world.set_door_obstacles(vec![(7, Arc::new(door))]);
    (world, MeshNavigator::new(Arc::new(mesh)))
}

fn search(
    mesh: &MeshNavigator,
    world: &CollisionWorld,
    start: Vec3,
    goal: Vec3,
    doors: Vec<u8>,
) -> (PathProgress, Box<dyn RouteSearch>) {
    let mut search = mesh.search(SearchRequest {
        start,
        goal,
        reach: 0.5,
        height: 6.0,
        doors,
        drops: false,
        scope: Scope::Whole,
    });
    for _ in 0..1000 {
        match search.advance(world, 256) {
            PathProgress::Searching => (),
            progress => return (progress, search),
        }
    }
    panic!("door route did not finish");
}

#[test]
fn usable_door_joins_mesh_islands_in_both_directions() {
    let (world, mesh) = rooms(false);
    for direction in [-1.0, 1.0] {
        let (start, goal) = (
            Vec3::new(-20.0 * direction, 0.0, 0.0),
            Vec3::new(20.0 * direction, 0.0, 1.0),
        );
        assert!(!mesh.mesh().connected(
            mesh.mesh().locate(start).unwrap().0,
            mesh.mesh().locate(goal).unwrap().0
        ));
        let (progress, search) = search(&mesh, &world, start, goal, vec![7]);
        assert!(matches!(progress, PathProgress::Found(_)), "{progress:?}");
        assert!(
            matches!(search.planned(), Planned::Mesh { fallback: None, .. }),
            "{:?}",
            search.planned()
        );
        assert_eq!(
            search
                .doors_along()
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            vec![7]
        );
    }
}

#[test]
fn usable_door_connection_never_replaces_a_solid_wall() {
    let (world, mesh) = rooms(true);
    let (progress, _) = search(
        &mesh,
        &world,
        Vec3::new(-20.0, 0.0, 1.0),
        Vec3::new(20.0, 0.0, 1.0),
        vec![7],
    );
    assert_eq!(progress, PathProgress::Failed);
}

#[test]
fn an_unusable_door_cannot_join_mesh_islands() {
    let (world, mesh) = rooms(false);
    let (progress, _) = search(
        &mesh,
        &world,
        Vec3::new(-20.0, 0.0, 1.0),
        Vec3::new(20.0, 0.0, 1.0),
        Vec::new(),
    );
    assert_eq!(progress, PathProgress::Failed);
}

#[test]
fn ordinary_route_approaches_waits_and_crosses_only_after_the_door_is_passable() {
    for direction in [-1.0, 1.0] {
        let (mut world, mesh) = rooms(false);
        let (start, goal) = (
            Vec3::new(-20.0 * direction, 0.0, 0.0),
            Vec3::new(20.0 * direction, 0.0, 1.0),
        );
        let mut route = Route::new(start, goal, 0.5, 6.0)
            .with_doors(vec![7])
            .with_navigator(Box::new(mesh));
        let mut feet = start;
        let mut opened = false;
        let mut arrived = false;
        for tick in 0..1000 {
            match route.next(&world, feet, goal, 256, Duration::from_millis(tick * 50)) {
                RouteStep::Searching => (),
                RouteStep::Toward(to) => {
                    let delta = (to - feet) * Vec3::new(1.0, 0.0, 1.0);
                    feet = world.step(feet, delta.clamp_length_max(0.5), 6.0);
                }
                RouteStep::OpenDoor(7) => {
                    assert!(!opened);
                    assert!(feet.x.abs() < 4.0 && feet.x * direction < 0.0, "{feet:?}");
                    // A request alone does not open it or authorize a crossing.
                    for wait in 1..=3 {
                        assert_eq!(
                            route.next(
                                &world,
                                feet,
                                goal,
                                256,
                                Duration::from_millis(tick * 50 + wait)
                            ),
                            RouteStep::OpenDoor(7)
                        );
                    }
                    assert!(world.step(feet, Vec3::X * direction * 4.0, 6.0).x * direction < 0.0);
                    world.set_door_obstacles(Vec::new());
                    opened = true;
                }
                RouteStep::Arrived => {
                    assert!(opened);
                    assert!(
                        Vec2::new(feet.x - goal.x, feet.z - goal.z).length() <= 1.5,
                        "{feet:?}"
                    );
                    arrived = true;
                    break;
                }
                other => panic!("{other:?} at {feet:?}"),
            }
        }
        assert!(arrived, "door route did not arrive");
    }
}

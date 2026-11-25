use super::*;
use crate::movement::CollisionWorld;

fn room() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[-10.0, 0.0, -10.0], [10.0, 0.0, 10.0], [10.0, 0.0, -10.0]],
        [[-10.0, 0.0, -10.0], [-10.0, 0.0, 10.0], [10.0, 0.0, 10.0]],
        [[4.0, 0.0, -10.0], [4.0, 8.0, -10.0], [4.0, 8.0, 10.0]],
        [[4.0, 0.0, -10.0], [4.0, 8.0, 10.0], [4.0, 0.0, 10.0]],
    ]
}

fn world() -> CollisionWorld {
    CollisionWorld::new([[
        [-1000.0, -100.0, -1000.0],
        [0.0, -100.0, 1000.0],
        [1000.0, -100.0, -1000.0],
    ]])
    .unwrap()
}

fn close(left: Option<f32>, right: Option<f32>) {
    match (left, right) {
        (Some(left), Some(right)) => assert!((left - right).abs() < 0.001, "{left} vs {right}"),
        (None, None) => (),
        other => panic!("query disagreement: {other:?}"),
    }
}

#[test]
fn posed_queries_and_normals_match_baked_geometry() {
    let triangles = room();
    let translation = Vec3::new(20.0, 5.0, 30.0);
    let rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_z(0.15);
    let transform = |point: [f32; 3]| (rotation * Vec3::from_array(point) + translation).to_array();
    let shared = Arc::new(CollisionMesh::new(triangles.clone()).unwrap());
    let mut posed = world();
    posed.set_spawn_instances(vec![(
        8,
        CollisionInstance::new(shared, translation, rotation).unwrap(),
    )]);
    let mut baked = world();
    baked.set_spawn_obstacles(vec![(
        8,
        Arc::new(
            CollisionMesh::new(triangles.iter().map(|triangle| triangle.map(transform))).unwrap(),
        ),
    )]);
    close(
        posed.ray_distance(translation + Vec3::Y * 20.0, -Vec3::Y, 40.0),
        baked.ray_distance(translation + Vec3::Y * 20.0, -Vec3::Y, 40.0),
    );
    assert!(posed.ground(translation, 0.1, 0.1).is_some());
    close(
        posed.ground(translation, 0.1, 0.1),
        baked.ground(translation, 0.1, 0.1),
    );
    close(
        posed.nearest_ground(translation, 0.5, 0.5),
        baked.nearest_ground(translation, 0.5, 0.5),
    );
    close(posed.support(translation), baked.support(translation));
    assert_eq!(posed.ground_spawn(translation, 0.01, 0.05), Some(8));
    assert_eq!(
        posed.ground_spawn(translation, 0.01, 0.05),
        baked.ground_spawn(translation, 0.01, 0.05)
    );
    let beside_wall = translation + rotation * Vec3::X * 3.7;
    close(
        posed.clearance(beside_wall, 3.0, 2.0),
        baked.clearance(beside_wall, 3.0, 2.0),
    );
    let delta = rotation * Vec3::X * 8.0;
    assert!(
        posed
            .step(translation, delta, 3.0)
            .distance(baked.step(translation, delta, 3.0))
            < 0.001
    );
    let flying = translation + Vec3::Y * 2.0;
    let left = posed.flight_sweep(flying, delta, 2.0).unwrap().unwrap();
    let right = baked.flight_sweep(flying, delta, 2.0).unwrap().unwrap();
    assert!(left.time_of_impact < 0.8);
    assert!((left.time_of_impact - right.time_of_impact).abs() < 0.001);
    assert!(left.normal1.distance(right.normal1) < 0.005);
    assert!(left.normal2.distance(right.normal2) < 0.005);
    let left = posed.sweep(flying, delta, 2.0).unwrap().unwrap();
    let right = baked.sweep(flying, delta, 2.0).unwrap().unwrap();
    assert!((left.time_of_impact - right.time_of_impact).abs() < 0.001);
    assert!(left.normal1.distance(right.normal1) < 0.005);
    assert!(left.normal2.distance(right.normal2) < 0.005);
    assert!(left.witness2.distance(right.witness2) < 0.01);
    assert!(
        posed
            .fly_step(flying, delta, 2.0)
            .distance(baked.fly_step(flying, delta, 2.0))
            < 0.001
    );
}

#[test]
fn moving_instances_share_geometry_and_snapshots_keep_their_old_pose() {
    let mesh = Arc::new(CollisionMesh::new(room()).unwrap());
    let first = CollisionInstance::new(Arc::clone(&mesh), Vec3::ZERO, Quat::IDENTITY).unwrap();
    let second =
        CollisionInstance::new(mesh, Vec3::new(100.0, 5.0, 0.0), Quat::from_rotation_y(0.5))
            .unwrap();
    assert!(Arc::ptr_eq(&first.mesh, &second.mesh));
    let mut moved = world();
    moved.set_spawn_instances(vec![(8, first)]);
    let snapshot = moved.clone();
    moved.set_spawn_instances(vec![(8, second)]);
    assert_eq!(snapshot.ground_spawn(Vec3::ZERO, 0.01, 0.05), Some(8));
    assert!(moved.ground_spawn(Vec3::ZERO, 0.01, 0.05).is_none());
    assert_eq!(
        moved.ground_spawn(Vec3::new(100.0, 5.0, 0.0), 0.01, 0.05),
        Some(8)
    );
    moved.set_spawn_instances(Vec::new());
    assert!(
        moved
            .ground_spawn(Vec3::new(100.0, 5.0, 0.0), 0.01, 0.05)
            .is_none()
    );
}

#[test]
fn posed_footprint_edges_keep_floor_identity_and_reject_invalid_poses() {
    let deck = vec![
        [[-5.0, 0.0, -5.0], [0.0, 0.0, 5.0], [0.0, 0.0, -5.0]],
        [[-5.0, 0.0, -5.0], [-5.0, 0.0, 5.0], [0.0, 0.0, 5.0]],
    ];
    let mesh = Arc::new(CollisionMesh::new(deck).unwrap());
    assert!(CollisionInstance::new(Arc::clone(&mesh), Vec3::NAN, Quat::IDENTITY).is_err());
    assert!(
        CollisionInstance::new(Arc::clone(&mesh), Vec3::ZERO, Quat::from_array([0.0; 4])).is_err()
    );
    let at = Vec3::new(20.0, 5.0, 30.0);
    let rotation = Quat::from_rotation_y(0.7);
    let mut world = world();
    world.set_spawn_instances(vec![(
        8,
        CollisionInstance::new(mesh, at, rotation).unwrap(),
    )]);
    let edge = at + rotation * Vec3::X * 0.2;
    assert!(world.support(edge).is_some());
    assert_eq!(world.ground_spawn(edge, 0.01, 0.05), Some(8));
    assert!(
        world
            .ground_spawn(at + rotation * Vec3::X, 0.01, 0.05)
            .is_none()
    );
}

#[test]
fn tilted_placement_extent_uses_real_vertices_instead_of_empty_aabb_corners() {
    let triangles = [[[0.0, 0.0, 0.0], [10.0, 0.0, 1.0], [10.0, 10.0, 0.0]]];
    let rotation = Quat::from_rotation_z(-0.5);
    let translation = Vec3::new(0.0, 5.0, 0.0);
    let baked = CollisionMesh::new(triangles.iter().map(|triangle| {
        triangle.map(|point| (rotation * Vec3::from_array(point) + translation).to_array())
    }))
    .unwrap();
    let placed = CollisionInstance::new(
        Arc::new(CollisionMesh::new(triangles).unwrap()),
        translation,
        rotation,
    )
    .unwrap();
    assert!((placed.surface().minimum_y() - baked.mesh.local_aabb().mins.y).abs() < 0.001);
}

#[test]
fn a_ship_passing_alongside_never_takes_the_link_off_the_deck_still_underfoot() {
    // The #631 rerun (6.611): with two ships alongside, part of a rider's
    // footprint met the passing ship's side, level with or a little above
    // its own deck, and the link went to the passing ship. The deck it
    // stands on keeps the link while any part of the footprint is over it.
    let slab = |from: f32, to: f32, height: f32| {
        Arc::new(
            CollisionMesh::new([
                [
                    [from, height, -10.0],
                    [to, height, 10.0],
                    [to, height, -10.0],
                ],
                [
                    [from, height, -10.0],
                    [from, height, 10.0],
                    [to, height, 10.0],
                ],
            ])
            .unwrap(),
        )
    };
    for passing in [5.0, 5.45] {
        let mut world = world();
        // The passing ship is listed first, so the plain sweep meets it first.
        world.set_spawn_obstacles(vec![
            (9, slab(-0.1, 20.0, passing)),
            (8, slab(-20.0, 0.2, 5.0)),
        ]);
        // Feet on the higher of the two, where the walk stood them.
        let feet = Vec3::new(0.0, passing, 0.0);
        let depth = 200.0;
        assert_eq!(
            world.spawn_floor(feet, 0.01, depth).map(|(id, _)| id),
            Some(9)
        );
        let kept = world.spawn_floor_keeping(feet, 0.01, depth, Some(8));
        assert_eq!(kept.map(|(id, _)| id), Some(8), "{passing}");
        close(kept.map(|(_, height)| height), Some(5.0));
        // With no link, or a link to the other ship, the plain sweep's.
        assert_eq!(
            world.spawn_floor_keeping(feet, 0.01, depth, None),
            world.spawn_floor(feet, 0.01, depth)
        );
        assert_eq!(
            world
                .spawn_floor_keeping(feet, 0.01, depth, Some(9))
                .map(|(id, _)| id),
            Some(9)
        );
        // Once no part of the footprint is over its deck, the link moves.
        let across = Vec3::new(1.0, passing, 0.0);
        assert_eq!(
            world
                .spawn_floor_keeping(across, 0.01, depth, Some(8))
                .map(|(id, _)| id),
            Some(9)
        );
    }
    // A deck under another ship's floor is not the first floor there, so
    // it keeps nothing.
    let mut stacked = world();
    stacked.set_spawn_obstacles(vec![
        (9, slab(-20.0, 20.0, 6.0)),
        (8, slab(-20.0, 20.0, 5.0)),
    ]);
    let above = Vec3::new(0.0, 6.0, 0.0);
    assert_eq!(
        stacked
            .spawn_floor_keeping(above, 0.01, 200.0, Some(8))
            .map(|(id, _)| id),
        Some(9)
    );
}

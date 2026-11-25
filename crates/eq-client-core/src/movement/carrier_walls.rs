//! Walking object filters must not discard support or other body queries.
use super::*;
use eq_network_game::world::SpawnKind;
use glam::Quat;

fn deck(y: f32) -> Vec<[[f32; 3]; 3]> {
    vec![
        [[-10.0, y, -10.0], [10.0, y, 10.0], [10.0, y, -10.0]],
        [[-10.0, y, -10.0], [-10.0, y, 10.0], [10.0, y, 10.0]],
    ]
}

fn partition() -> Vec<[[f32; 3]; 3]> {
    vec![
        [[0.0, 0.0, -8.0], [0.0, 12.0, -8.0], [0.0, 12.0, 8.0]],
        [[0.0, 0.0, -8.0], [0.0, 12.0, 8.0], [0.0, 0.0, 8.0]],
    ]
}

fn carrier(kind: Option<SpawnKind>, origin: Vec3, rotation: Quat) -> CollisionWorld {
    let mut world = CollisionWorld::new(deck(-100.0)).unwrap();
    let triangles = deck(0.0).into_iter().chain(partition());
    let mut instance = CollisionInstance::new(
        Arc::new(CollisionMesh::new(triangles).unwrap()),
        origin,
        rotation,
    )
    .unwrap();
    if let Some(kind) = kind {
        instance = instance.with_spawn_kind(kind);
    }
    world.set_spawn_instances(vec![(8, instance)]);
    world
}

#[test]
fn carrier_walls_npc_walk_crosses_still_and_moved_partition_with_deck_support() {
    for (origin, rotation) in [
        (Vec3::ZERO, Quat::IDENTITY),
        (Vec3::new(30.0, 5.0, -20.0), Quat::from_rotation_y(0.7)),
    ] {
        let world = carrier(Some(SpawnKind::Npc), origin, rotation).with_titanium_walking();
        let start = origin + rotation * Vec3::new(-3.0, 0.0, 0.0);
        let end = world.step(start, rotation * Vec3::X * 6.0, 3.0);
        let local = rotation.inverse() * (end - origin);
        assert!(local.x > 2.99, "walking was blocked at {local:?}");
        assert!((local.y).abs() < 0.001);
        assert_eq!(world.ground_spawn(end, 0.01, 0.05), Some(8));
        // A fresh press from inside the rigging must also get out.
        let overlap = origin - rotation * Vec3::X * 0.1;
        let escaped = world.step(overlap, rotation * Vec3::X, 3.0);
        assert!(
            escaped.distance(overlap) > 0.99,
            "{origin:?}: {overlap:?} -> {escaped:?}"
        );
    }
}

#[test]
fn carrier_walls_preserve_player_unknown_unclassified_and_default_queries() {
    for kind in [Some(SpawnKind::Player), Some(SpawnKind::Unknown(9)), None] {
        let world = carrier(kind, Vec3::ZERO, Quat::IDENTITY).with_titanium_walking();
        let stopped = world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0);
        assert!(stopped.x < 0.0, "{kind:?} changed: {stopped:?}");
    }
    let world = carrier(Some(SpawnKind::Npc), Vec3::ZERO, Quat::IDENTITY);
    assert!(world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0).x < 0.0);
    let walking = world.with_titanium_walking();
    assert!(
        walking
            .flight_sweep(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0)
            .unwrap()
            .is_some()
    );
    assert!(
        walking
            .ray_distance(-Vec3::X * 3.0 + Vec3::Y * 2.0, Vec3::X, 6.0)
            .is_some()
    );
    let airborne = walking.walk(
        -Vec3::X * 3.0,
        Vec3::X * 6.0,
        3.0,
        Support::Airborne { clear: 0.0 },
    );
    assert!(airborne.x < 0.0);
}

#[test]
fn carrier_walls_keep_boom_headroom_and_vertical_landing() {
    let mut world = CollisionWorld::new(deck(-100.0)).unwrap();
    let mut triangles = deck(0.0);
    // An underside over the right half of the deck, within the standing body.
    triangles.extend([
        [[0.0, 3.0, -8.0], [8.0, 3.0, -8.0], [8.0, 3.0, 8.0]],
        [[0.0, 3.0, -8.0], [8.0, 3.0, 8.0], [0.0, 3.0, 8.0]],
    ]);
    let instance = CollisionInstance::new(
        Arc::new(CollisionMesh::new(triangles).unwrap()),
        Vec3::ZERO,
        Quat::IDENTITY,
    )
    .unwrap()
    .with_spawn_kind(SpawnKind::Npc);
    world.set_spawn_instances(vec![(8, instance)]);
    let baseline = world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0);
    assert!(baseline.x < 0.0);
    let world = world.with_titanium_walking();
    assert!(world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0).x < 0.0);
    let under = Vec3::new(4.0, 0.0, 0.0);
    assert!(world.fall_sweep(under, 4.0, 1.0).unwrap().is_some());
    let above_deck = Vec3::new(-4.0, 5.0, 0.0);
    assert!(world.fall_sweep(above_deck, -8.0, 3.0).unwrap().is_some());
    assert_eq!(
        world.ground_spawn(Vec3::new(-4.0, 0.0, 0.0), 0.01, 0.05),
        Some(8)
    );
}

#[test]
fn carrier_walls_keep_zone_placement_and_door_walls() {
    for kind in 0..3 {
        let mut world = CollisionWorld::new(if kind == 0 {
            deck(0.0).into_iter().chain(partition()).collect()
        } else {
            deck(0.0)
        })
        .unwrap();
        let mesh = Arc::new(CollisionMesh::new(partition()).unwrap());
        match kind {
            0 => (),
            1 => world.set_obstacles(vec![mesh]),
            _ => world.set_door_obstacles_with(vec![(80, mesh, Solidity::BOTH)]),
        }
        let start = -Vec3::X * 3.0;
        let baseline = world.step(start, Vec3::X * 6.0, 3.0);
        let changed = world
            .with_titanium_walking()
            .step(start, Vec3::X * 6.0, 3.0);
        assert!(baseline.x < 0.0);
        assert!(baseline.abs_diff_eq(changed, 0.0001));
    }
}

#[test]
fn carrier_walls_do_not_contain_a_walker_at_the_deck_edge() {
    let world = carrier(Some(SpawnKind::Npc), Vec3::ZERO, Quat::IDENTITY).with_titanium_walking();
    let end = world.walk(
        -Vec3::X * 3.0,
        Vec3::X * 16.0,
        3.0,
        Support::Optional {
            drop: 0.5,
            climb: 1.0,
        },
    );
    assert!(
        end.x > 12.9,
        "partition or edge contained the walker: {end:?}"
    );
    assert!(world.ground_spawn(end, 0.01, 0.5).is_none());
}

#[test]
fn carrier_walls_diagonal_step_keeps_floor_reach_without_reinstating_the_partition() {
    let mut world = CollisionWorld::new(deck(-100.0)).unwrap();
    let mut triangles = partition();
    triangles.extend([
        [[-10.0, 0.0, -10.0], [0.0, 0.0, 10.0], [0.0, 0.0, -10.0]],
        [[-10.0, 0.0, -10.0], [-10.0, 0.0, 10.0], [0.0, 0.0, 10.0]],
        [[0.0, 1.0, -10.0], [10.0, 1.0, 10.0], [10.0, 1.0, -10.0]],
        [[0.0, 1.0, -10.0], [0.0, 1.0, 10.0], [10.0, 1.0, 10.0]],
    ]);
    let instance = CollisionInstance::new(
        Arc::new(CollisionMesh::new(triangles).unwrap()),
        Vec3::ZERO,
        Quat::IDENTITY,
    )
    .unwrap()
    .with_spawn_kind(SpawnKind::Npc);
    world.set_spawn_instances(vec![(8, instance)]);
    let end = world.with_titanium_walking().walk(
        -Vec3::X * 3.0,
        Vec3::X * 6.0,
        3.0,
        Support::Optional {
            drop: 0.5,
            climb: 1.1,
        },
    );
    assert!(end.abs_diff_eq(Vec3::new(3.0, 1.0, 0.0), 0.001), "{end:?}");
}

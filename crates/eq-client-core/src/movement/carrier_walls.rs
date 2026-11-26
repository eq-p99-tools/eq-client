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
        let world = carrier(Some(SpawnKind::Npc), origin, rotation).with_titanium_walking(1.5);
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
        let world = carrier(kind, Vec3::ZERO, Quat::IDENTITY).with_titanium_walking(1.5);
        let stopped = world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0);
        assert!(stopped.x < 0.0, "{kind:?} changed: {stopped:?}");
    }
    let world = carrier(Some(SpawnKind::Npc), Vec3::ZERO, Quat::IDENTITY);
    assert!(world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0).x < 0.0);
    let walking = world.with_titanium_walking(1.5);
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
    let world = world.with_titanium_walking(1.5);
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
fn carrier_walls_ceiling_starts_at_actor_origin_and_keeps_the_boom_above_it() {
    for (origin, rotation) in [
        (Vec3::ZERO, Quat::IDENTITY),
        (Vec3::new(30.0, 5.0, -20.0), Quat::from_rotation_y(0.7)),
    ] {
        for (ceiling, blocked) in [(2.5, false), (6.5, true), (9.0, false)] {
            let mut triangles = deck(0.0);
            triangles.extend([
                [
                    [0.0, ceiling, -8.0],
                    [8.0, ceiling, -8.0],
                    [8.0, ceiling, 8.0],
                ],
                [
                    [0.0, ceiling, -8.0],
                    [8.0, ceiling, 8.0],
                    [0.0, ceiling, 8.0],
                ],
            ]);
            let mut world = CollisionWorld::new(deck(-100.0)).unwrap();
            world.set_spawn_instances(vec![(
                8,
                CollisionInstance::new(
                    Arc::new(CollisionMesh::new(triangles).unwrap()),
                    origin,
                    rotation,
                )
                .unwrap()
                .with_spawn_kind(SpawnKind::Npc),
            )]);
            let start = origin - rotation * Vec3::X * 3.0;
            let delta = rotation * Vec3::X * 6.0;
            let baseline = rotation.inverse() * (world.step(start, delta, 6.677_319_5) - origin);
            assert_eq!(baseline.x < 0.0, ceiling < 9.0, "{baseline:?}");
            // S is the actor origin above its feet. At S=3.75 the upward
            // sphere starts wholly above the low face at 2.5 (0x43a0a0).
            let world = world.with_titanium_walking(3.75);
            let end = world.step(start, delta, 6.677_319_5);
            let local = rotation.inverse() * (end - origin);
            assert_eq!(local.x < 0.0, blocked, "ceiling={ceiling}: {local:?}");
            if !blocked {
                assert!(local.abs_diff_eq(Vec3::X * 3.0, 0.001), "{local:?}");
            }
            assert_eq!(world.ground_spawn(end, 0.01, 0.05), Some(8));
        }
    }
}

#[test]
fn carrier_walls_headroom_keeps_approaching_edges_and_authored_facing() {
    for (overhead, blocked) in [
        // A vertical panel's lower edge is approached by the upward probe,
        // unlike an overlap whose projection already lies inside its face.
        (
            vec![
                [[0.0, 2.0, -8.0], [0.0, 8.0, -8.0], [0.0, 8.0, 8.0]],
                [[0.0, 2.0, -8.0], [0.0, 8.0, 8.0], [0.0, 2.0, 8.0]],
            ],
            true,
        ),
        // Only the downward side is a ceiling; a single upward-facing
        // floor above the walking reach must not acquire a solid underside.
        (deck(3.0), false),
        (
            deck(3.0).into_iter().map(|[a, b, c]| [a, c, b]).collect(),
            true,
        ),
    ] {
        let mut world = CollisionWorld::new(deck(-100.0))
            .unwrap()
            .with_titanium_walking(1.5);
        let mesh = CollisionMesh::new(deck(0.0).into_iter().chain(overhead)).unwrap();
        world.set_spawn_instances(vec![(
            8,
            CollisionInstance::new(Arc::new(mesh), Vec3::ZERO, Quat::IDENTITY)
                .unwrap()
                .with_spawn_kind(SpawnKind::Npc),
        )]);
        let end = world.step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0);
        assert_eq!(end.x < 0.0, blocked, "{end:?}");
        if !blocked {
            assert!(end.abs_diff_eq(Vec3::X * 3.0, 0.001), "{end:?}");
        }
    }
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
            .with_titanium_walking(1.5)
            .step(start, Vec3::X * 6.0, 3.0);
        assert!(baseline.x < 0.0);
        assert!(baseline.abs_diff_eq(changed, 0.0001));
    }
}

#[test]
fn carrier_walls_do_not_contain_a_walker_at_the_deck_edge() {
    let world =
        carrier(Some(SpawnKind::Npc), Vec3::ZERO, Quat::IDENTITY).with_titanium_walking(1.5);
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
    let end = world.with_titanium_walking(1.5).walk(
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

#[test]
fn carrier_walls_wide_translated_partitions_do_not_pin_their_centre_line() {
    for (scale, origin, rotation) in [
        (
            1.0,
            Vec3::new(0.0, 5.0, 10.0),
            Quat::from_rotation_y(crate::static_yaw(128.0)),
        ),
        (
            0.64,
            Vec3::new(30.0, 5.0, -20.0),
            Quat::from_rotation_y(0.7),
        ),
    ] {
        let faces = [
            [
                [-100.0, 0.0, -100.0],
                [100.0, 0.0, 100.0],
                [100.0, 0.0, -100.0],
            ],
            [
                [-100.0, 0.0, -100.0],
                [-100.0, 0.0, 100.0],
                [100.0, 0.0, 100.0],
            ],
            [
                [0.0, 0.0, -100.0],
                [0.0, 100.0, -100.0],
                [0.0, 100.0, 100.0],
            ],
            [[0.0, 0.0, -100.0], [0.0, 100.0, 100.0], [0.0, 0.0, 100.0]],
        ];
        let mesh = CollisionMesh::new(
            faces.map(|face| face.map(|point| (Vec3::from_array(point) * scale).to_array())),
        )
        .unwrap();
        let instance = CollisionInstance::new(Arc::new(mesh), origin, rotation)
            .unwrap()
            .with_spawn_kind(SpawnKind::Npc);
        let mut world = CollisionWorld::new(deck(-100.0))
            .unwrap()
            .with_titanium_walking(1.5 * scale);
        world.set_spawn_instances(vec![(8, instance)]);
        let start = origin - rotation * Vec3::X * (20.0 * scale);
        let end = world.step(
            start,
            rotation * Vec3::X * (40.0 * scale),
            (3.0 * scale).max(1.0),
        );
        let local = rotation.inverse() * (end - origin) / scale;
        assert!(
            local.x > 19.99,
            "scale={scale}, origin={origin:?}: {local:?}"
        );
        let escaped = world.step(origin, rotation * Vec3::X, (3.0 * scale).max(1.0));
        assert!(
            escaped.distance(origin) > 0.99,
            "centre-line escape: {origin:?} -> {escaped:?}"
        );
    }
}

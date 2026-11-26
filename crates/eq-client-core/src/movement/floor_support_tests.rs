use super::*;
use glam::Quat;

fn plane(y: f32, up: bool) -> Vec<[[f32; 3]; 3]> {
    let mut faces = vec![
        [[-20.0, y, -20.0], [-20.0, y, 20.0], [20.0, y, 20.0]],
        [[-20.0, y, -20.0], [20.0, y, 20.0], [20.0, y, -20.0]],
    ];
    if !up {
        for face in &mut faces {
            face.swap(1, 2);
        }
    }
    faces
}

#[test]
fn floor_support_culls_down_faces_without_losing_walls_or_ceilings() {
    let world = CollisionWorld::new(plane(0.0, true).into_iter().chain(plane(1.0, false))).unwrap();
    assert_eq!(world.ground(Vec3::Y, 0.1, 2.0), Some(0.0));
    assert_eq!(world.nearest_ground(Vec3::Y, 0.1, 2.0), Some(0.0));
    assert_eq!(world.support(Vec3::Y), None);
    assert_eq!(world.ray_distance(Vec3::Y * 2.0, -Vec3::Y, 3.0), Some(1.0));
    assert_eq!(world.ray_distance(Vec3::Y * 0.5, Vec3::Y, 3.0), Some(0.5));
    assert!(world.clearance(Vec3::ZERO, 2.0, 0.0).is_some());
}

#[test]
fn floor_support_is_shared_by_posture_clearance_but_not_its_ceiling() {
    let world = CollisionWorld::new(
        plane(0.0, true)
            .into_iter()
            .chain(plane(1.0, false))
            .chain(plane(4.0, false)),
    )
    .unwrap();
    // The down face below the origin cannot shorten the room's floor side.
    assert!(world.posture_clearance(Vec3::Y * 2.0, 1.0, 3.5));
    // The down face above the origin remains a ceiling.
    assert!(!world.posture_clearance(Vec3::Y * 2.0, 3.0, 3.5));
}

#[test]
fn floor_support_keeps_zero_ray_contact_and_both_window_edges() {
    // This keeps the current ray approximation's t=0 and window boundaries.
    // The read sphere's start-overlap height (origin - r + .001) remains the
    // separate 6.336 shape gap; eligibility must not add an above-origin lift.
    for up in [false, true] {
        let mesh = CollisionMesh::new(plane(0.0, up)).unwrap();
        let face = Surface::identity(&mesh);
        assert_eq!(
            face.cast_floor(&Ray::new(Vector::ZERO, -Vector::Y), 1.0),
            up.then_some(0.0)
        );
        let world = CollisionWorld::new(plane(0.0, up)).unwrap();
        assert_eq!(world.ground(Vec3::Y, 0.0, 1.0).is_some(), up);
        assert_eq!(world.ground(-Vec3::Y, 1.0, 0.0).is_some(), up);
        assert!(world.ground(Vec3::Y * 1.001, 0.0, 1.0).is_none());
        assert!(world.ground(-Vec3::Y * 0.001, 0.0, 1.0).is_none());
    }
}

#[test]
fn movement_room_is_strict_uses_floor_fronts_and_only_ceiling_fronts() {
    let make = |ceiling| {
        CollisionWorld::new(
            plane(0.0, true)
                .into_iter()
                .chain(plane(1.0, false)) // an underside below the origin isn't support
                .chain(plane(5.0, true)) // a back face above isn't a ceiling
                .chain(plane(ceiling, false)),
        )
        .unwrap()
    };
    let origin = Vec3::Y * 3.75;
    let (floor, ceiling) = make(8.0).headroom_bounds(origin).unwrap();
    // The support ray's endpoint guard can leave a sub-micro-unit residue.
    assert!(floor.abs() < 1e-6, "{floor}");
    assert_eq!(ceiling, 8.0);
    assert!(!make(8.0).headroom(origin, 8.0));
    assert!(make(8.001).headroom(origin, 8.0));
    assert!(
        CollisionWorld::new(plane(0.0, true))
            .unwrap()
            .headroom(origin, 8.0)
    );
    assert!(
        CollisionWorld::new(plane(8.0, false))
            .unwrap()
            .headroom(origin, 8.0)
    );
}

#[test]
fn movement_ceilings_keep_rotated_carriers_and_exclude_doors() {
    let ceiling = Arc::new(CollisionMesh::new(plane(0.0, true)).unwrap());
    let mut world = CollisionWorld::new(plane(0.0, true)).unwrap();
    let instance = CollisionInstance::new(
        Arc::clone(&ceiling),
        Vec3::Y * 8.0,
        Quat::from_rotation_x(std::f32::consts::PI),
    )
    .unwrap()
    .with_spawn_kind(crate::SpawnKind::Npc);
    world.set_spawn_instances(vec![(50, instance)]);
    // Walking skips NPC lateral walls, but its ceiling query still keeps them.
    let world = world.with_titanium_walking(3.75);
    assert!(!world.headroom(Vec3::Y * 3.75, 8.001));
    assert!(!world.posture_clearance(Vec3::Y * 6.0, 3.75, 7.9));
    let mut doors = CollisionWorld::new(plane(0.0, true)).unwrap();
    doors.set_door_obstacles(vec![(
        7,
        Arc::new(CollisionMesh::new(plane(8.0, false)).unwrap()),
    )]);
    assert!(doors.headroom(Vec3::Y * 3.75, 8.0));
}

#[test]
fn floor_support_has_one_rule_for_terrain_objects_doors_and_rotated_carriers() {
    for up in [false, true] {
        let mesh = Arc::new(CollisionMesh::new(plane(0.0, up)).unwrap());
        let mut placed = CollisionWorld::new(plane(-10.0, true)).unwrap();
        placed.set_obstacles(vec![Arc::clone(&mesh)]);
        assert_eq!(placed.ground(Vec3::ZERO, 0.1, 0.1).is_some(), up);
        placed.set_door_obstacles(vec![(7, Arc::clone(&mesh))]);
        assert_eq!(placed.ground_door(Vec3::ZERO, 0.1, 0.1), up.then_some(7));
        for (rotation, expected) in [
            (Quat::IDENTITY, up),
            (Quat::from_rotation_x(std::f32::consts::PI), !up),
        ] {
            placed.set_obstacles(Vec::new());
            placed.set_spawn_instances(vec![(
                8,
                CollisionInstance::new(Arc::clone(&mesh), Vec3::ZERO, rotation).unwrap(),
            )]);
            assert_eq!(placed.support(Vec3::ZERO).is_some(), expected);
            assert_eq!(placed.spawn_floor(Vec3::ZERO, 0.1, 0.1).is_some(), expected);
            assert_eq!(
                placed
                    .spawn_floor_keeping(Vec3::ZERO, 0.1, 0.1, Some(8))
                    .is_some(),
                expected
            );
            assert!(placed.ray_distance(Vec3::Y, -Vec3::Y, 2.0).is_some());
        }
    }
}

fn bank(bevel: Option<bool>) -> CollisionWorld {
    let mut faces = plane(0.0, true);
    // A vertical four-unit sill and its upward top.
    faces.extend([
        [[0.0, 0.0, -10.0], [0.0, 4.0, 10.0], [0.0, 0.0, 10.0]],
        [[0.0, 0.0, -10.0], [0.0, 4.0, -10.0], [0.0, 4.0, 10.0]],
        [[0.0, 4.0, -10.0], [0.0, 4.0, 10.0], [10.0, 4.0, 10.0]],
        [[0.0, 4.0, -10.0], [10.0, 4.0, 10.0], [10.0, 4.0, -10.0]],
    ]);
    if let Some(up) = bevel {
        let mut lower = [
            [[0.0, 3.281_25, -10.0], [2.0, 2.0, -10.0], [2.0, 2.0, 10.0]],
            [
                [0.0, 3.281_25, -10.0],
                [2.0, 2.0, 10.0],
                [0.0, 3.281_25, 10.0],
            ],
        ];
        if up {
            for face in &mut lower {
                face.swap(1, 2);
            }
        }
        faces.extend(lower);
    }
    CollisionWorld::new(faces).unwrap()
}

#[test]
fn floor_support_removes_only_the_downward_bevel_route_to_a_four_unit_sill() {
    for seconds in [0.01, 1.0 / 60.0, 0.05] {
        for speed in [6.3, 12.6] {
            for bevel in [None, Some(false), Some(true)] {
                for (edge, standing_reach) in [(3.375, false), (5.0, true)] {
                    let world = bank(bevel);
                    let mut controller = AirborneController::default();
                    let mut feet = Vec3::new(-0.45, 0.0, 0.0);
                    let physics = VerticalPhysics {
                        climb: 3.375,
                        edge_climb: Some(edge),
                        ..PROVISIONAL_PHYSICS
                    };
                    let mut top = false;
                    for _ in 0..240 {
                        feet = controller.step(
                            &world,
                            feet,
                            physics,
                            MotionStep {
                                horizontal: Vec3::X * speed * seconds,
                                jump: false,
                                seconds,
                                height: 6.677_319_5,
                            },
                        );
                        if feet.y > 3.99 {
                            top = true;
                            break;
                        }
                    }
                    assert_eq!(
                        top,
                        standing_reach || bevel == Some(true),
                        "dt{seconds} speed{speed} bevel{bevel:?} edge{edge}: {feet:?}"
                    );
                    if !top {
                        assert!(feet.x < 0.0 && feet.y.abs() < 0.001);
                    }
                }
            }
        }
    }
}

//! Opt-in local geometry probes; require user-owned assets and explicit coordinates.
use super::*;

fn probe_point(name: &str) -> Vec3 {
    let values: Vec<f32> = std::env::var(name)
        .unwrap()
        .split(',')
        .map(|number| number.parse().unwrap())
        .collect();
    assert_eq!(values.len(), 3, "expected renderer X,Y,Z in {name}");
    Vec3::new(values[0], values[1], values[2])
}

#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_FEET and EQ_PROBE_GOAL"]
fn inspect_path() {
    use eq_client_core::movement::{PathProgress, PathSearch};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let world = build_collision(&zone).unwrap();
    let (feet, goal) = (probe_point("EQ_PROBE_FEET"), probe_point("EQ_PROBE_GOAL"));
    let mut search = PathSearch::new(feet, goal, 10.0, 6.0);
    let result = loop {
        match search.advance(&world, 5000) {
            PathProgress::Searching => (),
            done => break done,
        }
    };
    let closest = search.closest().and_then(|path| path.last().copied());
    println!(
        "found={} reached={} closest={closest:?} closest_flat_distance={:?}",
        matches!(result, PathProgress::Found(_)),
        search.reached(),
        closest.map(|end| Vec2::new(end.x - goal.x, end.z - goal.z).length()),
    );
}

/// Checks whether a server's spawn positions sit in this client's zone geometry:
/// each named point should have a floor a little below it. Points in empty space
/// mean the server's zone data was laid out for different zone files.
/// `EQ_PROBE_POINTS` lists `name=x,y,z` renderer points separated by `;`; the
/// swapped column exchanges the two horizontal axes as a mapping cross-check.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE and EQ_PROBE_POINTS"]
fn inspect_points() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let points: Vec<(String, Vec3)> = std::env::var("EQ_PROBE_POINTS")
        .unwrap()
        .split(';')
        .map(|entry| {
            let (name, at) = entry.split_once('=').unwrap();
            let v: Vec<f32> = at.split(',').map(|n| n.parse().unwrap()).collect();
            (name.to_owned(), Vec3::new(v[0], v[1], v[2]))
        })
        .collect();
    let (low, high) = zone.collision.iter().flatten().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| {
            let p = Vec3::from_array(*p);
            (low.min(p), high.max(p))
        },
    );
    println!("terrain_bounds={low:?}..{high:?}");
    let everything = build_collision(&zone).unwrap();
    for (name, at) in &points {
        let swapped = Vec3::new(at.z, at.y, at.x);
        let around = |p: Vec3| {
            (
                everything.ray_distance(p + Vec3::Y, -Vec3::Y, 50.0),
                everything.ray_distance(p + Vec3::Y, Vec3::Y, 50.0),
            )
        };
        println!(
            "point={name} standard(below,above)={:?} swapped(below,above)={:?}",
            around(*at),
            around(swapped)
        );
    }
}

/// The terrain's collision, then each object's within 120 units of `near`, by model
/// name; prints each object's placement and bounds.
fn collision_parts(
    zone: &ZoneAsset,
    near: Vec3,
) -> Vec<(String, eq_client_core::movement::CollisionWorld)> {
    use eq_client_core::movement::CollisionWorld;
    let mut parts = Vec::new();
    if let Ok(terrain) = CollisionWorld::new(zone.collision.clone()) {
        parts.push(("terrain".into(), terrain));
    }
    for object in &zone.objects {
        let Some(model) = zone.models.get(object.model) else {
            continue;
        };
        if Vec3::from_array(object.translation).distance(near) > 120.0 {
            continue;
        }
        let rotation = object.rotation_degrees.map(f32::to_radians);
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::from_array(object.scale),
            Quat::from_euler(EulerRot::XYZ, rotation[0], rotation[1], rotation[2]),
            Vec3::from_array(object.translation),
        );
        let triangles: Vec<[[f32; 3]; 3]> = model
            .collision
            .iter()
            .map(|triangle| {
                triangle.map(|p| transform.transform_point3(Vec3::from_array(p)).to_array())
            })
            .collect();
        let (low, high) = triangles.iter().flatten().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(low, high), p| {
                (
                    low.min(Vec3::from_array(*p)),
                    high.max(Vec3::from_array(*p)),
                )
            },
        );
        println!(
            "object={} at={:?} rotation={:?} scale={:?} solid_triangles={} bounds={low:?}..{high:?}",
            model.name,
            object.translation,
            object.rotation_degrees,
            object.scale,
            triangles.len()
        );
        if let Ok(part) = CollisionWorld::new(triangles) {
            parts.push((model.name.clone(), part));
        }
    }
    parts
}

/// Reports which geometry encloses the area a path search can reach: for each
/// reached position's unreachable neighbor, the terrain or object model whose
/// collision stands in the way at knee, waist and head height.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_FEET and EQ_PROBE_GOAL"]
fn inspect_enclosure() {
    use eq_client_core::movement::{PathProgress, PathSearch};
    use std::collections::{BTreeMap, HashSet};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let world = build_collision(&zone).unwrap();
    let (feet, goal) = (probe_point("EQ_PROBE_FEET"), probe_point("EQ_PROBE_GOAL"));
    // A human-sized model, as measured at admission.
    let mut search = PathSearch::new(feet, goal, 10.0, 6.7);
    let found = loop {
        match search.advance(&world, 5000) {
            PathProgress::Searching => (),
            done => break matches!(done, PathProgress::Found(_)),
        }
    };
    println!("found={found}");
    let reached: Vec<Vec3> = search.positions().collect();
    let (low, high) = reached.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| (low.min(*p), high.max(*p)),
    );
    println!("reached={} bounds={low:?}..{high:?}", reached.len());
    let parts = collision_parts(&zone, feet);
    #[allow(clippy::cast_possible_truncation)] // Two-unit grid cells.
    let cell = |p: Vec3| ((p.x / 2.0).round() as i32, (p.z / 2.0).round() as i32);
    let seen: HashSet<(i32, i32)> = reached.iter().map(|p| cell(*p)).collect();
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut ledges = 0;
    for p in &reached {
        for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let direction = Vec3::new(dx, 0.0, dz);
            if seen.contains(&cell(*p + direction * 2.0)) {
                continue;
            }
            let hits: Vec<&str> = parts
                .iter()
                .filter(|(_, part)| {
                    [0.9, 3.0, 5.5].iter().any(|height| {
                        part.ray_distance(*p + Vec3::Y * height, direction, 2.5)
                            .is_some()
                    })
                })
                .map(|(name, _)| name.as_str())
                .collect();
            if hits.is_empty() {
                ledges += 1;
                let beyond = *p + direction * 2.0;
                let floor = world.ground(beyond, 2.0, 200.0);
                let ceiling = |at: Vec3| world.ray_distance(at + Vec3::Y * 0.5, Vec3::Y, 30.0);
                println!(
                    "open_edge from={p:?} direction={direction:?} drop={:?} clearance_here={:?} clearance_beyond={:?}",
                    floor.map(|floor| p.y - floor),
                    ceiling(*p).map(|d| d + 0.5),
                    floor.and_then(
                        |floor| ceiling(Vec3::new(beyond.x, floor, beyond.z)).map(|d| d + 0.5)
                    ),
                );
            }
            for name in hits {
                *blockers.entry(name.to_owned()).or_default() += 1;
            }
        }
    }
    println!("open_edges_without_a_wall={ledges}");
    for (name, count) in blockers {
        println!("blocked_by={name} edges={count}");
    }
}

#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_POSITION, EQ_PROBE_MODEL and EQ_PROBE_RACE"]
fn inspect_admission_support() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let position: Vec<f32> = std::env::var("EQ_PROBE_POSITION")
        .unwrap()
        .split(',')
        .map(|number| number.parse().unwrap())
        .collect();
    assert_eq!(position.len(), 3, "expected renderer X,Y,Z");
    let position = Vec3::new(position[0], position[1], position[2]);
    let character = eq_client_assets::characters::load_character(
        &install.join("global_chr.s3d"),
        &std::env::var("EQ_PROBE_MODEL").unwrap(),
    )
    .unwrap();
    let height = character.height();
    let world = build_collision(&zone).unwrap();
    let camera = OrbitCamera {
        focus: position,
        radius: 40.0,
        yaw: 0.75,
        pitch: -60.0_f32.to_radians(),
    };
    let camera_delta = orbit_transform(&camera).translation - position;
    println!(
        "camera_obstruction={:?}",
        world.ray_distance(position, camera_delta.normalize(), camera_delta.length())
    );
    let surface = TerrainSurface::from_primitives(&zone.primitives);
    let rendered = surface.height_below(position.x, position.z, position.y + 0.5);
    // Players arrive with size 0: the race's default size.
    let race: u32 = std::env::var("EQ_PROBE_RACE").unwrap().parse().unwrap();
    let offset = eq_client_core::z_offset(race, 0.0);
    let feet = position - Vec3::Y * offset;
    println!("model_height={height} rendered_ground={rendered:?} feet_offset={offset}");
    println!(
        "solid_ground={:?} current_support={:?}",
        world.ground(position, 0.5, height * 2.0),
        world.ground(feet, 0.8, 1.5)
    );
    for displacement in [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z] {
        for distance in [0.1, 1.2675, 3.0443] {
            let delta = displacement * distance;
            let target_support = world.ground(feet + delta, 0.8, 1.5);
            let lower_support = world.ground(feet + delta, 0.8, 50.0);
            let resolved = world.step(feet, delta, height) - feet;
            println!(
                "direction={displacement:?} distance={distance} target_support={target_support:?} lower_support={lower_support:?} resolved={resolved:?}"
            );
        }
    }
    // Offline physics only: exercise the actual ledge and stair geometry without packets.
    let mut airborne = eq_client_core::movement::AirborneController::default();
    let mut cursor = feet;
    for frame in 0..120 {
        cursor = airborne.step(
            &world,
            cursor,
            eq_client_core::movement::VerticalPhysics {
                gravity: 32.0,
                terminal_speed: 40.0,
                jump_speed: 10.0,
            },
            eq_client_core::movement::MotionStep {
                horizontal: if frame < 40 {
                    Vec3::X * 0.1
                } else {
                    Vec3::ZERO
                },
                jump: false,
                seconds: 0.05,
                height,
            },
        );
    }
    println!(
        "offline_off_platform={cursor:?} landing={:?}",
        airborne.take_landing()
    );
    for _ in 0..40 {
        cursor = world.step(cursor, -Vec3::X * 0.1, height);
    }
    println!(
        "offline_return={cursor:?} distance_from_start={}",
        cursor.distance(feet)
    );
    let mut grounded = feet;
    for _ in 0..40 {
        grounded = world.step(grounded, Vec3::X * 0.1, height);
    }
    println!("grounded_off_platform={grounded:?}");
    for _ in 0..40 {
        grounded = world.step(grounded, -Vec3::X * 0.1, height);
    }
    println!(
        "grounded_return={grounded:?} distance_from_start={}",
        grounded.distance(feet)
    );
}

/// Replays a spot where a live character stopped moving: with the feet offset the
/// log's admission line reports (`EQ_PROBE_OFFSET`) and the logged position as a
/// renderer point (`EQ_PROBE_POSITION`), the capsule's clearance, the parts it
/// touches, and a short step in 16 directions, grounded and through the online
/// fall controller.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_MODEL, EQ_PROBE_OFFSET and EQ_PROBE_POSITION"]
fn inspect_stuck() {
    use eq_client_core::movement::{AirborneController, MotionStep, PROVISIONAL_PHYSICS};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let height = eq_client_assets::characters::load_character(
        &install.join("global_chr.s3d"),
        &std::env::var("EQ_PROBE_MODEL").unwrap(),
    )
    .unwrap()
    .height();
    let world = build_collision(&zone).unwrap();
    let offset: f32 = std::env::var("EQ_PROBE_OFFSET").unwrap().parse().unwrap();
    let feet = probe_point("EQ_PROBE_POSITION") - Vec3::Y * offset;
    println!(
        "height={height} feet_offset={offset} feet={feet:?} clearance={:?}",
        world.clearance(feet, height, 2.0)
    );
    for (name, part) in collision_parts(&zone, feet) {
        if let Some(clearance) = part.clearance(feet, height, 0.5) {
            println!("near={name} clearance={clearance}");
        }
    }
    let mut refused = 0;
    for turn in 0..16_u8 {
        let angle = f32::from(turn) * std::f32::consts::TAU / 16.0;
        let step = Vec3::new(angle.sin(), 0.0, angle.cos()) * 0.5;
        let grounded = world.step(feet, step, height) - feet;
        let online = AirborneController::default().step(
            &world,
            feet,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal: step,
                jump: false,
                seconds: 0.05,
                height,
            },
        ) - feet;
        if grounded.length() < 0.001 && online.length() < 0.001 {
            refused += 1;
        }
        println!("direction={step:?} grounded={grounded:?} online={online:?}");
    }
    println!("refused_directions={refused}/16");
}

/// Walks a route offline exactly as a script's walk step does online: the route
/// searched from `EQ_PROBE_FEET` to `EQ_PROBE_GOAL` (renderer feet points),
/// followed with the online stepper in 150 ms samples of 3.1 units, printing
/// each sample and how the walk ends. Door colliders are not loaded.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_MODEL, EQ_PROBE_FEET and EQ_PROBE_GOAL"]
fn inspect_walk() {
    use eq_client_core::movement::{AirborneController, Route, RouteStep};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let height = eq_client_assets::characters::load_character(
        &install.join("global_chr.s3d"),
        &std::env::var("EQ_PROBE_MODEL").unwrap(),
    )
    .unwrap()
    .height();
    let world = build_collision(&zone).unwrap();
    let (mut feet, goal) = (probe_point("EQ_PROBE_FEET"), probe_point("EQ_PROBE_GOAL"));
    let mut route = Route::new(feet, goal, 10.0, height);
    let mut airborne = AirborneController::default();
    for sample in 0..400_u64 {
        let now = std::time::Duration::from_millis(sample * 150);
        match route.next(&world, feet, goal, 5000, now) {
            RouteStep::Searching => (),
            RouteStep::Toward(next) => {
                let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z).normalize_or_zero();
                let moved = motion::fall_step(&mut airborne, &world, feet, to * 3.1, 0.15, height);
                println!(
                    "sample={sample} toward={next:?} feet={moved:?} moved={}",
                    moved.distance(feet)
                );
                feet = moved;
            }
            end => {
                println!("end={end:?} feet={feet:?} remaining={}", route.remaining());
                return;
            }
        }
    }
    println!("end=timeout feet={feet:?}");
}

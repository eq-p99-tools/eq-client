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

/// Separates a search that gives up on its expansion cap from one that runs out
/// of walkable positions. Each of `EQ_PROBE_FEET` and `EQ_PROBE_GOAL` (renderer
/// points a little above their floor) drops to the floor below it; the search
/// then runs once per cap in `EQ_PROBE_CAPS` (comma-separated, default the
/// client's own) and weight in `EQ_PROBE_WEIGHTS` (comma-separated, default
/// 1), with the wall toll on and off. `open=false` on a failure
/// means every walkable position from the start was searched.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_FEET and EQ_PROBE_GOAL"]
fn inspect_search_limits() {
    use eq_client_core::movement::{PathProgress, PathSearch};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let height = std::env::var("EQ_PROBE_MODEL").map_or(6.0, |model| {
        eq_client_assets::characters::load_character(&install.join("global_chr.s3d"), &model)
            .unwrap()
            .height()
    });
    let world = build_collision(&zone).unwrap();
    let floor = |at: Vec3| {
        let below = world.ray_distance(at, -Vec3::Y, 50.0).unwrap();
        at - Vec3::Y * below
    };
    let (feet, goal) = (
        floor(probe_point("EQ_PROBE_FEET")),
        floor(probe_point("EQ_PROBE_GOAL")),
    );
    let eq = |at: Vec3| Vec3::new(-at.z, at.x, at.y);
    println!(
        "height={height} feet={feet:?} goal={goal:?} eq_feet={:?} eq_goal={:?}",
        eq(feet),
        eq(goal)
    );
    let caps: Vec<usize> = std::env::var("EQ_PROBE_CAPS").map_or(vec![60_000], |caps| {
        caps.split(',')
            .map(|cap| cap.trim().parse().unwrap())
            .collect()
    });
    let weights: Vec<f32> = std::env::var("EQ_PROBE_WEIGHTS").map_or(vec![1.0], |weights| {
        weights
            .split(',')
            .map(|weight| weight.trim().parse().unwrap())
            .collect()
    });
    for (cap, weight) in caps
        .iter()
        .flat_map(|&cap| weights.iter().map(move |&weight| (cap, weight)))
    {
        for toll in [true, false] {
            let started = std::time::Instant::now();
            let mut search = PathSearch::new(feet, goal, 10.0, height)
                .with_limits(cap, toll)
                .with_weight(weight);
            let result = loop {
                match search.advance(&world, 5000) {
                    PathProgress::Searching => (),
                    done => break done,
                }
            };
            let elapsed = started.elapsed().as_secs_f32();
            let (low, high) = search
                .positions()
                .fold((f32::MAX, f32::MIN), |(low, high), p| {
                    (low.min(p.y), high.max(p.y))
                });
            let closest = search.closest().and_then(|path| path.last().copied());
            let length = match &result {
                PathProgress::Found(path) => Some(
                    path.windows(2)
                        .map(|pair| pair[0].distance(pair[1]))
                        .sum::<f32>()
                        + path.first().map_or(0.0, |first| first.distance(feet)),
                ),
                _ => None,
            };
            println!(
                "cap={cap} weight={weight} toll={toll} found={} expanded={} open={} reached={} seconds={elapsed:.2} \
                 path_length={length:?} floor_y={low:.1}..{high:.1} closest_eq={:?} closest_flat_distance={:?}",
                matches!(result, PathProgress::Found(_)),
                search.expanded(),
                search.has_open(),
                search.reached(),
                closest.map(eq),
                closest.map(|end| Vec2::new(end.x - goal.x, end.z - goal.z).length()),
            );
        }
    }
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
        let transform = object_transform(object).to_matrix();
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
            object.rotation,
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
        pulled: None,
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
/// `EQ_PROBE_DROPS=1` lets the route step off ledges, as an online session
/// that simulates falls does; the walker says when it falls, as online.
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
    let drops = std::env::var("EQ_PROBE_DROPS").is_ok_and(|drops| drops == "1");
    let mut route = Route::new(feet, goal, 10.0, height).with_drops(drops);
    let mut airborne = AirborneController::default();
    for sample in 0..400_u64 {
        let now = std::time::Duration::from_millis(sample * 150);
        let falling = airborne.peak().is_some();
        match route.next_with(&world, (feet, falling), goal, 5000, now) {
            RouteStep::Searching => (),
            RouteStep::Toward(next) => {
                let to = Vec3::new(next.x - feet.x, 0.0, next.z - feet.z).normalize_or_zero();
                let moved =
                    motion::fall_step(&mut airborne, &world, feet, to * 3.1, 0.15, height, false);
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

/// Maps the zone lines of `EQ_PROBE_ZONE`: samples the ground on a 10-unit grid
/// and prints, per zone-line tag, how many samples fall inside it and the EQ
/// `(x, y, z)` bounds they span, for walking into one on purpose.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
fn inspect_zone_lines() {
    use std::collections::BTreeMap;
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone =
        eq_client_assets::load_zone(&install, &std::env::var("EQ_PROBE_ZONE").unwrap()).unwrap();
    let world = build_collision(&zone).unwrap();
    let (low, high) = zone.collision.iter().flatten().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| {
            let p = Vec3::from_array(*p);
            (low.min(p), high.max(p))
        },
    );
    let mut found: BTreeMap<String, (usize, Vec3, Vec3)> = BTreeMap::new();
    let mut x = low.x;
    while x <= high.x {
        let mut z = low.z;
        while z <= high.z {
            let top = Vec3::new(x, high.y + 1.0, z);
            if let Some(depth) = world.ray_distance(top, -Vec3::Y, high.y - low.y + 2.0) {
                let ground = top.y - depth;
                let probe = [x, ground + 3.0, z];
                if let Some(line) = zone.regions.zone_line_at(probe) {
                    let at = world_position(probe, 0.0);
                    let eq = Vec3::new(at.x, at.y, at.z);
                    let entry = found.entry(format!("{line:?}")).or_insert((0, eq, eq));
                    entry.0 += 1;
                    entry.1 = entry.1.min(eq);
                    entry.2 = entry.2.max(eq);
                }
            }
            z += 10.0;
        }
        x += 10.0;
    }
    for (line, (samples, low, high)) in found {
        println!("zone_line={line} samples={samples} eq_xyz_bounds={low:?}..{high:?}");
    }
}

/// Every model the race table names ships in the installed character archives.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL"]
fn every_race_model_is_installed() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let mut installed = std::collections::BTreeSet::new();
    let mut unreadable = 0;
    for entry in std::fs::read_dir(&install).unwrap() {
        let path = entry.unwrap().path();
        let archive = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with("_chr.s3d"));
        if !archive {
            continue;
        }
        match eq_client_assets::characters::inspect_archive(&path) {
            Ok(definitions) => installed.extend(definitions.into_iter().filter_map(|definition| {
                definition.name.strip_suffix("_HS_DEF").map(str::to_owned)
            })),
            Err(_) => unreadable += 1,
        }
    }
    let missing: std::collections::BTreeSet<_> = (0..1000)
        .flat_map(|race| {
            (0..3).filter_map(move |gender| eq_client_core::races::model(race, gender))
        })
        .filter(|code| !installed.contains(*code))
        .collect();
    println!(
        "installed models {} unreadable archives {unreadable}",
        installed.len()
    );
    assert!(missing.is_empty(), "{missing:?}");
}

/// What the ground at a point is to the water and lava guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ground {
    /// The head of a body this tall stays above water, and the feet out of
    /// lava.
    Dry,
    /// The head would be under water.
    Deep,
    /// The feet would be in lava.
    Lava,
}

impl Ground {
    fn at(liquids: &dyn eq_client_core::movement::Liquids, feet: Vec3, height: f32) -> Self {
        use eq_client_core::movement::Liquid;
        if liquids.liquid_at(feet + Vec3::Y * 0.1) == Some(Liquid::Lava) {
            Self::Lava
        } else if liquids.liquid_at(feet + Vec3::Y * height) == Some(Liquid::Water) {
            Self::Deep
        } else {
            Self::Dry
        }
    }
}

/// What a place to watch the guard shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Look {
    /// A walk into water that stops before the head goes under.
    Wade,
    /// A walk that stops at a ledge over deep water, for jumping off it too.
    Edge,
    /// A walk toward lava that stops at its edge.
    Lava,
}

/// A walk from dry ground toward deep water or lava, and where the guard
/// stops it, in the renderer's frame.
#[derive(Clone, Copy, Debug)]
struct Spot {
    look: Look,
    start: Vec3,
    toward: Vec3,
    stop: Vec3,
    /// How far the feet went in water before the walk stopped.
    waded: f32,
    /// How far below where the walk stops the ground two units on lies.
    drop: f32,
}

impl Spot {
    fn direction(&self) -> Vec3 {
        Vec3::new(
            self.toward.x - self.start.x,
            0.0,
            self.toward.z - self.start.z,
        )
        .normalize_or_zero()
    }

    fn walked(&self) -> f32 {
        Vec2::new(self.stop.x - self.start.x, self.stop.z - self.start.z).length()
    }

    /// How well it shows its look: a long wade, or a walk of about a dozen
    /// units to the ledge or the lava.
    fn score(&self) -> f32 {
        match self.look {
            Look::Wade => self.waded + self.walked() * 0.1,
            Look::Edge | Look::Lava => -(self.walked() - 12.0).abs(),
        }
    }

    /// The EQ heading, from 0 up to 512, that walks toward what the walk
    /// heads for.
    fn heading(&self) -> f32 {
        let direction = self.direction();
        eq_client_core::world_heading(direction.x.atan2(direction.z))
    }

    /// The spot, its points in EQ coordinates as [`eq_xyz`] gives them.
    fn describe(&self, height: f32) -> String {
        format!(
            "look={:?} start_xyz={} heading={:.0} toward_xyz={} stops_at_xyz={} walked={:.1} \
             waded={:.1} drop={:.1}",
            self.look,
            eq_xyz(self.start, height),
            self.heading(),
            eq_xyz(self.toward, height),
            eq_xyz(self.stop, height),
            self.walked(),
            self.waded,
            self.drop,
        )
    }

    /// Script steps that show the look on a local `EQEmu` server: stand at
    /// the start, two units up so the character drops onto the ground, turn
    /// so W walks toward what the walk heads for, walk until the guard stops
    /// it, and report there; then at a ledge, jump holding forward, and
    /// after a wade, walk back out.
    fn script(&self, name: &str, height: f32) -> Vec<String> {
        let start = world_position(
            (self.start + Vec3::Y * (height * 0.5 + 2.0)).to_array(),
            0.0,
        );
        let heading = self.heading();
        let mut steps = vec![
            format!("gm goto {:.1} {:.1} {:.1}", start.x, start.y, start.z),
            "wait 3000".to_owned(),
            format!("camera {heading:.0} -20"),
            "hold w 10000".to_owned(),
            format!("report {name} stopped"),
            format!("screenshot {name}.png"),
        ];
        match self.look {
            Look::Wade => steps.extend([
                format!("camera {:.0} -20", (heading + 256.0) % 512.0),
                "hold w 5000".to_owned(),
                format!("report {name} back out"),
            ]),
            Look::Edge => steps.extend([
                "hold w+space 2000".to_owned(),
                format!("report {name} after the jump"),
            ]),
            Look::Lava => {}
        }
        steps
    }
}

/// Walks from `start` toward `toward` as the player's character walks, the
/// guard on, and says where it stops and how far the feet went in water.
fn walk_toward(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (start, toward): (Vec3, Vec3),
    height: f32,
) -> (Vec3, f32) {
    use eq_client_core::movement::{AirborneController, Liquid, MotionStep, PROVISIONAL_PHYSICS};
    let direction = Vec3::new(toward.x - start.x, 0.0, toward.z - start.z).normalize_or_zero();
    let mut controller = AirborneController::default();
    let (mut feet, mut waded) = (start, 0.0);
    for _ in 0..200 {
        let next = controller.step(
            world,
            feet,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal: direction * 0.5,
                jump: false,
                seconds: 0.05,
                height,
            },
        );
        if liquids.liquid_at(next + Vec3::Y * 0.1) == Some(Liquid::Water) {
            waded += Vec2::new(next.x - feet.x, next.z - feet.z).length();
        }
        feet = next;
    }
    (feet, waded)
}

/// A point in the renderer's frame as EQ X,Y,Z.
fn eq_point(point: Vec3) -> String {
    let at = world_position(point.to_array(), 0.0);
    format!("{:.1},{:.1},{:.1}", at.x, at.y, at.z)
}

/// Feet in the renderer's frame as EQ X,Y,Z, where `/loc` (which prints Y,
/// X, Z) and a script's `report` put a character of this height standing
/// there: its middle.
fn eq_xyz(feet: Vec3, height: f32) -> String {
    eq_point(feet + Vec3::Y * height * 0.5)
}

/// The floors a column of the zone offers a body this tall, top down: each
/// surface a ray straight down meets with room for the body over it. Roofs,
/// treetops and anything over the whole zone count too; the walks from them
/// sort them out.
fn floors(
    world: &eq_client_core::movement::CollisionWorld,
    (x, z): (f32, f32),
    (low, high): (f32, f32),
    height: f32,
) -> Vec<f32> {
    let mut floors = Vec::new();
    let (mut from, mut above) = (high + 1.0, f32::INFINITY);
    for _ in 0..32 {
        let Some(depth) = world.ray_distance(Vec3::new(x, from, z), -Vec3::Y, from - low + 1.0)
        else {
            break;
        };
        let surface = from - depth;
        if above - surface >= height {
            floors.push(surface);
        }
        // The next ray starts just under this surface, so it meets the next.
        (above, from) = (surface, surface - 0.01);
    }
    floors
}

/// What the liquid probe saw on its way, so a zone where it finds no spot
/// says why.
#[derive(Debug, Default)]
struct Tally {
    /// Columns sampled, and the floors in them.
    columns: usize,
    floors: usize,
    /// Floors by what they are to the guard, and those with their soles in
    /// water or lava, shallow or deep.
    grounds: std::collections::BTreeMap<Ground, usize>,
    wet: usize,
    /// Walks tried from dry floors toward deep water or lava.
    walks: usize,
    /// Walks dropped, by why, with the first few of each.
    dropped: std::collections::BTreeMap<&'static str, (usize, Vec<String>)>,
}

impl Tally {
    fn drop(&mut self, reason: &'static str, example: impl FnOnce() -> String) {
        let (count, examples) = self.dropped.entry(reason).or_default();
        *count += 1;
        if examples.len() < 3 {
            examples.push(example());
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "columns={} floors={} grounds={:?} wet={} walks={}",
            self.columns, self.floors, self.grounds, self.wet, self.walks
        )];
        for (reason, (count, examples)) in &self.dropped {
            lines.push(format!("dropped {reason}={count}"));
            lines.extend(examples.iter().map(|example| format!("  {example}")));
        }
        lines
    }
}

/// A zone's floors on a grid, by column, with what each is to the guard.
type FloorGrid = std::collections::HashMap<(i32, i32), Vec<(Vec3, Ground)>>;

/// Samples the floors on a grid this fine across `low..high`, in the
/// renderer's frame.
fn floor_grid(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (low, high): (Vec3, Vec3),
    (spacing, height): (f32, f32),
    tally: &mut Tally,
) -> FloorGrid {
    let mut grid = FloorGrid::new();
    let (mut i, mut x) = (0, low.x);
    while x <= high.x {
        let (mut j, mut z) = (0, low.z);
        while z <= high.z {
            tally.columns += 1;
            let column: Vec<_> = floors(world, (x, z), (low.y, high.y), height)
                .into_iter()
                .map(|y| {
                    let feet = Vec3::new(x, y, z);
                    let ground = Ground::at(liquids, feet, height);
                    *tally.grounds.entry(ground).or_default() += 1;
                    if liquids.liquid_at(feet + Vec3::Y * 0.1).is_some() {
                        tally.wet += 1;
                    }
                    (feet, ground)
                })
                .collect();
            tally.floors += column.len();
            if !column.is_empty() {
                grid.insert((i, j), column);
            }
            (j, z) = (j + 1, z + spacing);
        }
        (i, x) = (i + 1, x + spacing);
    }
    grid
}

/// Per 32-unit square, the dry floor whose nearest deep water, and the one
/// whose nearest lava, lies about four samples off (within ten, along the
/// floors a walker gets to), with what each heads for.
fn liquid_starts(grid: &FloorGrid) -> Vec<(Ground, Vec3, Vec3)> {
    use std::collections::BTreeMap;
    const DIRECTIONS: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    // How far a floor may rise from one sample to the next and still be
    // walked onto: a 45-degree slope, at the probe's four units apart.
    const CLIMB: f32 = 4.0;
    // Along a line of columns, the first deep water and the first lava a
    // walker gets to, how many samples off.
    let reach = |(i, j): (i32, i32), (di, dj): (i32, i32), start: Vec3| {
        let mut first: [Option<(i32, Vec3)>; 2] = [None, None];
        let mut level = start.y;
        for k in 1..=10 {
            // The floor a walker gets to: the highest it can climb onto or
            // drop to.
            let Some(&(feet, ground)) = grid.get(&(i + di * k, j + dj * k)).and_then(|column| {
                column
                    .iter()
                    .filter(|(feet, _)| feet.y <= level + CLIMB)
                    .max_by(|a, b| a.0.y.total_cmp(&b.0.y))
            }) else {
                break;
            };
            match ground {
                Ground::Deep => first[0] = first[0].or(Some((k, feet))),
                Ground::Lava => first[1] = first[1].or(Some((k, feet))),
                Ground::Dry => {}
            }
            level = feet.y;
        }
        first
    };
    // Blocks of eight by eight samples near deep water or lava: a dry floor
    // anywhere else has neither within reach, so it is never scanned.
    let block = |(i, j): (i32, i32)| (i.div_euclid(8), j.div_euclid(8));
    let near: std::collections::HashSet<(i32, i32)> = grid
        .iter()
        .filter(|(_, column)| column.iter().any(|(_, ground)| *ground != Ground::Dry))
        .flat_map(|(&cell, _)| {
            let (i, j) = block(cell);
            (-2..=2).flat_map(move |di| (-2..=2).map(move |dj| (i + di, j + dj)))
        })
        .collect();
    let mut chosen: BTreeMap<(i32, i32, Ground), (i32, Vec3, Vec3)> = BTreeMap::new();
    for (&cell, column) in grid
        .iter()
        .filter(|(cell, _)| near.contains(&block(**cell)))
    {
        for &(start, _) in column.iter().filter(|(_, ground)| *ground == Ground::Dry) {
            let lines = DIRECTIONS.map(|direction| reach(cell, direction, start));
            for (index, target) in [Ground::Deep, Ground::Lava].into_iter().enumerate() {
                let Some((k, toward)) = lines
                    .iter()
                    .filter_map(|first| first[index])
                    .min_by_key(|(k, _)| *k)
                else {
                    continue;
                };
                #[allow(clippy::cast_possible_truncation)] // A zone spans far fewer squares.
                let square = (
                    (start.x / 32.0).floor() as i32,
                    (start.z / 32.0).floor() as i32,
                    target,
                );
                let fit = (k - 4).abs();
                if chosen.get(&square).is_none_or(|(best, _, _)| fit < *best) {
                    chosen.insert(square, (fit, start, toward));
                }
            }
        }
    }
    chosen
        .into_iter()
        .map(|((_, _, target), (_, start, toward))| (target, start, toward))
        .collect()
}

/// Walks from dry ground toward deep water or lava with the guard on. A
/// walk the guard stops just short of what it heads for shows a ledge where
/// the ground beyond lies 4 or more units lower, else a wade, or lava's
/// edge; any other walk is tallied with why it shows nothing.
fn liquid_spot(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (target, start, toward): (Ground, Vec3, Vec3),
    (reach, height): (f32, f32),
    tally: &mut Tally,
) -> Option<Spot> {
    tally.walks += 1;
    let at = |feet: Vec3| eq_xyz(feet, height);
    // Ground that cannot be stood on, such as a steep roof, is no start.
    let settled = walk_toward(world, liquids, (start, start), height).0;
    if (settled.y - start.y).abs() > 0.5 {
        tally.drop("unsettled", || {
            format!("start_xyz={} settled_xyz={}", at(start), at(settled))
        });
        return None;
    }
    let (stop, waded) = walk_toward(world, liquids, (start, toward), height);
    let walk = || {
        format!(
            "toward={target:?} start_xyz={} toward_xyz={} stop_xyz={}",
            at(start),
            at(toward),
            at(stop)
        )
    };
    let stopped = Ground::at(liquids, stop, height);
    if stopped != Ground::Dry {
        tally.drop("stopped_in_liquid", || {
            format!("{} stopped_on={stopped:?}", walk())
        });
        return None;
    }
    let mut spot = Spot {
        look: Look::Lava,
        start,
        toward,
        stop,
        waded,
        drop: 0.0,
    };
    // The guard stopped it, not a wall: the ground two units on is what it
    // heads for.
    let ahead = stop + spot.direction() * 2.0 + Vec3::Y;
    let Some(depth) = world.ray_distance(ahead, -Vec3::Y, reach) else {
        tally.drop("no_floor_ahead", walk);
        return None;
    };
    let floor = ahead - Vec3::Y * depth;
    let beyond = Ground::at(liquids, floor, height);
    if beyond != target {
        tally.drop("stopped_short", || {
            format!("{} ahead_xyz={} ahead={beyond:?}", walk(), at(floor))
        });
        return None;
    }
    spot.drop = stop.y - floor.y;
    if target == Ground::Deep {
        spot.look = if spot.drop >= 4.0 {
            Look::Edge
        } else {
            Look::Wade
        };
    }
    Some(spot)
}

/// Samples the floors on a grid this fine across `low..high`, in the
/// renderer's frame, and walks from the best dry starts toward deep water
/// and lava; returns the walks that show the guard, and what was seen.
fn liquid_spots(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (low, high): (Vec3, Vec3),
    (spacing, height): (f32, f32),
) -> (Vec<Spot>, Tally) {
    let mut tally = Tally::default();
    let grid = floor_grid(world, liquids, (low, high), (spacing, height), &mut tally);
    let reach = high.y - low.y + 2.0;
    let spots = liquid_starts(&grid)
        .into_iter()
        .filter_map(|walk| liquid_spot(world, liquids, walk, (reach, height), &mut tally))
        .collect();
    (spots, tally)
}

/// Where the zone's water and lava are, by a scan of its volume (two units
/// apart in height, and across as far apart as keeps it to a few million
/// points): how many points of each and their EQ bounds, and for the first
/// few water points over a floor, that floor and what it is to the guard.
fn liquid_volume(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (low, high): (Vec3, Vec3),
    height: f32,
) -> Vec<String> {
    use eq_client_core::movement::Liquid;
    let size = high - low;
    let across = (size.x * size.z * size.y / 2.0 / 4.0e6).sqrt().max(8.0);
    let mut found: Vec<(Liquid, usize, Vec3, Vec3)> = Vec::new();
    let mut samples = Vec::new();
    let mut x = low.x;
    while x <= high.x {
        let mut z = low.z;
        while z <= high.z {
            let mut y = low.y;
            while y <= high.y {
                let point = Vec3::new(x, y, z);
                if let Some(liquid) = liquids.liquid_at(point) {
                    let at = world_position(point.to_array(), 0.0);
                    let eq = Vec3::new(at.x, at.y, at.z);
                    match found.iter_mut().find(|(kind, ..)| *kind == liquid) {
                        Some((_, count, least, most)) => {
                            *count += 1;
                            *least = least.min(eq);
                            *most = most.max(eq);
                        }
                        None => found.push((liquid, 1, eq, eq)),
                    }
                    if liquid == Liquid::Water && samples.len() < 3 {
                        let below = world.ray_distance(point, -Vec3::Y, point.y - low.y + 1.0);
                        if let Some(depth) = below {
                            samples.push((point, point - Vec3::Y * depth));
                        }
                    }
                }
                y += 2.0;
            }
            z += across;
        }
        x += across;
    }
    let mut lines = vec![format!("scanned_every={across:.0}")];
    lines.extend(found.into_iter().map(|(liquid, count, least, most)| {
        format!(
            "{liquid:?}_points={count} eq_bounds={:.0},{:.0},{:.0}..{:.0},{:.0},{:.0}",
            least.x, least.y, least.z, most.x, most.y, most.z
        )
    }));
    lines.extend(samples.into_iter().map(|(point, floor)| {
        format!(
            "water_sample at_xyz={} floor_xyz={} floor={:?}",
            eq_point(point),
            eq_point(floor),
            Ground::at(liquids, floor, height),
        )
    }));
    lines
}

/// The best few spots of each look, at least 48 units apart.
fn best_spots(mut spots: Vec<Spot>, each: usize) -> Vec<Spot> {
    spots.sort_by(|a, b| a.look.cmp(&b.look).then(b.score().total_cmp(&a.score())));
    let mut best: Vec<Spot> = Vec::new();
    for spot in spots {
        let mut taken = best.iter().filter(|other| other.look == spot.look);
        if taken.clone().count() < each
            && taken.all(|other| other.start.distance(spot.start) >= 48.0)
        {
            best.push(spot);
        }
    }
    best
}

/// Finds where the water and lava guard can be watched in `EQ_PROBE_ZONE`
/// (`qeynos` for South Qeynos, `lavastorm`): samples the floors on a 4-unit
/// grid, walks from dry ground toward the nearest deep water or lava with
/// the guard on, and prints the best places to wade in, to stand at a ledge
/// over deep water, and to walk toward lava, in EQ coordinates. Then it
/// prints a script that shows the best of each on a local `EQEmu` server,
/// and what it saw on the way: the floors, the walks and why any were
/// dropped, and where the zone's water and lava are. The body is 6 units
/// tall unless `EQ_PROBE_HEIGHT` says otherwise.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
fn inspect_liquids() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let zone = eq_client_assets::load_zone(&install, &name).unwrap();
    let height = std::env::var("EQ_PROBE_HEIGHT").map_or(6.0, |height| height.parse().unwrap());
    println!("liquid_regions={}", zone.regions.liquid_region_count());
    let world = build_collision(&zone).unwrap();
    let bounds = zone.collision.iter().flatten().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| {
            let p = Vec3::from_array(*p);
            (low.min(p), high.max(p))
        },
    );
    let liquids = ZoneLiquids(zone.regions.clone());
    let (spots, tally) = liquid_spots(&world, &liquids, bounds, (4.0, height));
    let spots = best_spots(spots, 5);
    for spot in &spots {
        println!("{}", spot.describe(height));
    }
    println!("script:\ngm zone {name}\nwait_zone {name}");
    for look in [Look::Wade, Look::Edge, Look::Lava] {
        if let Some(spot) = spots.iter().find(|spot| spot.look == look) {
            let label = format!("{look:?}").to_lowercase();
            for step in spot.script(&label, height) {
                println!("{step}");
            }
        }
    }
    println!(
        "seen: corners_xyz={} {}",
        eq_point(bounds.0),
        eq_point(bounds.1)
    );
    for line in tally.lines() {
        println!("{line}");
    }
    for line in liquid_volume(&world, &liquids, bounds, height) {
        println!("{line}");
    }
}

/// Replays one of the liquid probe's walks tick by tick on a zone of the
/// installed client, from `EQ_PROBE_START` toward `EQ_PROBE_TOWARD` (EQ
/// X,Y,Z of the body's middle, as the probe prints them): where the body is,
/// what it stands in and what the guard sees there, to a few ticks after it
/// first enters water or lava. The body is 6 units tall unless
/// `EQ_PROBE_HEIGHT` says otherwise.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_START and EQ_PROBE_TOWARD"]
fn trace_liquid_walk() {
    use eq_client_core::movement::{AirborneController, MotionStep, PROVISIONAL_PHYSICS};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let height = std::env::var("EQ_PROBE_HEIGHT").map_or(6.0, |height| height.parse().unwrap());
    let feet_at = |key: &str| {
        let eq: Vec<f32> = std::env::var(key)
            .unwrap()
            .split(',')
            .map(|value| value.trim().parse().unwrap())
            .collect();
        let middle = eq_client_core::render_position(eq_client_core::WorldPosition {
            x: eq[0],
            y: eq[1],
            z: eq[2],
            heading: 0.0,
        });
        Vec3::from_array(middle) - Vec3::Y * height * 0.5
    };
    let (start, toward) = (feet_at("EQ_PROBE_START"), feet_at("EQ_PROBE_TOWARD"));
    let zone = eq_client_assets::load_zone(&install, &name).unwrap();
    let world = build_collision(&zone).unwrap();
    let liquids = ZoneLiquids(zone.regions.clone());
    let direction = Vec3::new(toward.x - start.x, 0.0, toward.z - start.z).normalize_or_zero();
    let mut controller = AirborneController::default();
    let mut feet = start;
    let mut left = None;
    for tick in 0..200 {
        let view = world.guard_view(feet, height);
        let ground = Ground::at(&liquids, feet, height);
        // What the guard makes of the next step's end before walls and
        // floors move it.
        let ahead = world.guard_view(feet + direction * 0.5, height);
        println!(
            "tick={tick} at_xyz={} ground={ground:?} velocity={:.2} rest_xyz={} submerged={} dry={} ahead_rest_xyz={} ahead_dry={}",
            eq_xyz(feet, height),
            controller.velocity(),
            view.rest
                .map_or_else(|| "none".to_owned(), |rest| eq_xyz(rest, height)),
            view.submerged,
            view.dry,
            ahead
                .rest
                .map_or_else(|| "none".to_owned(), |rest| eq_xyz(rest, height)),
            ahead.dry,
        );
        if ground != Ground::Dry {
            left = Some(left.unwrap_or(6_u8));
        }
        if let Some(ticks) = &mut left {
            *ticks -= 1;
            if *ticks == 0 {
                break;
            }
        }
        feet = controller.step(
            &world,
            feet,
            PROVISIONAL_PHYSICS,
            MotionStep {
                horizontal: direction * 0.5,
                jump: false,
                seconds: 0.05,
                height,
            },
        );
    }
}

/// Liquids filling boxes, each from its lower corner to its upper.
#[derive(Clone)]
struct Boxes(Vec<(Vec3, Vec3, eq_client_core::movement::Liquid)>);

impl eq_client_core::movement::Liquids for Boxes {
    fn liquid_at(&self, point: Vec3) -> Option<eq_client_core::movement::Liquid> {
        self.0
            .iter()
            .find(|(low, high, _)| point.cmpge(*low).all() && point.cmple(*high).all())
            .map(|(_, _, liquid)| *liquid)
    }

    fn passes_through(
        &self,
        from: Vec3,
        to: Vec3,
        liquid: eq_client_core::movement::Liquid,
    ) -> bool {
        // The slab test: the path meets a box where it is between every pair
        // of the box's faces at once.
        let path = to - from;
        self.0
            .iter()
            .filter(|(_, _, filled)| *filled == liquid)
            .any(|(low, high, _)| {
                let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
                for axis in 0..3 {
                    if path[axis] == 0.0 {
                        if from[axis] < low[axis] || from[axis] > high[axis] {
                            return false;
                        }
                        continue;
                    }
                    let a = (low[axis] - from[axis]) / path[axis];
                    let b = (high[axis] - from[axis]) / path[axis];
                    enter = enter.max(a.min(b));
                    leave = leave.min(a.max(b));
                }
                enter <= leave
            })
    }

    fn within(&self, low: Vec3, high: Vec3) -> Option<Box<dyn eq_client_core::movement::Liquids>> {
        let boxes: Vec<_> = self
            .0
            .iter()
            .filter(|(l, h, _)| l.cmple(high).all() && h.cmpge(low).all())
            .copied()
            .collect();
        (!boxes.is_empty())
            .then(|| Box::new(Self(boxes)) as Box<dyn eq_client_core::movement::Liquids>)
    }
}

/// A made-up zone with a lake, lava and a ledge over deep water, all under
/// a ceiling, and its liquids.
fn made_up_zone() -> (eq_client_core::movement::CollisionWorld, Boxes) {
    use eq_client_core::movement::{CollisionWorld, Liquid};
    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }
    let mut triangles = Vec::new();
    // South of z -60, a bank from y 0 at x 10 down to a lake bed at y -10
    // from x 30, under water up to y -1.
    triangles.extend(quad(
        [-40.0, 0.0, -100.0],
        [10.0, 0.0, -100.0],
        [10.0, 0.0, -60.0],
        [-40.0, 0.0, -60.0],
    ));
    triangles.extend(quad(
        [10.0, 0.0, -100.0],
        [30.0, -10.0, -100.0],
        [30.0, -10.0, -60.0],
        [10.0, 0.0, -60.0],
    ));
    triangles.extend(quad(
        [30.0, -10.0, -100.0],
        [60.0, -10.0, -100.0],
        [60.0, -10.0, -60.0],
        [30.0, -10.0, -60.0],
    ));
    // Between z -40 and 0, flat ground with lava from x 30.
    triangles.extend(quad(
        [-40.0, 0.0, -40.0],
        [60.0, 0.0, -40.0],
        [60.0, 0.0, 0.0],
        [-40.0, 0.0, 0.0],
    ));
    // North of z 20, a ledge at y 10 to x 0 over ground at y 0 under water
    // up to y 8.
    triangles.extend(quad(
        [-40.0, 10.0, 20.0],
        [0.0, 10.0, 20.0],
        [0.0, 10.0, 60.0],
        [-40.0, 10.0, 60.0],
    ));
    triangles.extend(quad(
        [0.0, 0.0, 20.0],
        [60.0, 0.0, 20.0],
        [60.0, 0.0, 60.0],
        [0.0, 0.0, 60.0],
    ));
    // Over it all, a ceiling at y 20, as some zones have over them: as far
    // over the ledge as the water's floor is under it.
    triangles.extend(quad(
        [-40.0, 20.0, -100.0],
        [60.0, 20.0, -100.0],
        [60.0, 20.0, 60.0],
        [-40.0, 20.0, 60.0],
    ));
    let liquids = Boxes(vec![
        (
            Vec3::new(10.0, -20.0, -100.0),
            Vec3::new(60.0, -1.0, -60.0),
            Liquid::Water,
        ),
        (
            Vec3::new(30.0, -1.0, -40.0),
            Vec3::new(60.0, 0.5, 0.0),
            Liquid::Lava,
        ),
        (
            Vec3::new(0.0, -5.0, 20.0),
            Vec3::new(60.0, 8.0, 60.0),
            Liquid::Water,
        ),
    ]);
    let world = CollisionWorld::new(triangles)
        .unwrap()
        .with_liquids(liquids.clone());
    (world, liquids)
}

#[test]
fn the_liquid_probe_finds_a_wade_a_ledge_and_lava() {
    let (world, liquids) = made_up_zone();
    let bounds = (Vec3::new(-40.0, -10.0, -100.0), Vec3::new(60.0, 20.0, 60.0));
    let (spots, tally) = liquid_spots(&world, &liquids, bounds, (4.0, 6.0));
    // Every column has the ceiling, and most a floor under it.
    assert!(tally.floors > tally.columns, "{tally:?}");
    assert!(tally.grounds[&Ground::Deep] > 0 && tally.grounds[&Ground::Lava] > 0);
    assert!(tally.walks >= 3 && tally.lines()[0].starts_with("columns="));
    let volume = liquid_volume(&world, &liquids, bounds, 6.0);
    for line in ["Water_points=", "Lava_points=", "water_sample"] {
        assert!(
            volume.iter().any(|seen| seen.starts_with(line)),
            "{volume:?}"
        );
    }
    let spots = best_spots(spots, 3);
    let first = |look: Look| {
        spots
            .iter()
            .find(|spot| spot.look == look)
            .unwrap_or_else(|| panic!("no {look:?} in {spots:?}"))
    };
    // The wade stops where the head, six units up, meets the surface at y
    // -1: on the bank at y -7, x 24.
    let wade = first(Look::Wade);
    assert!((wade.stop.x - 24.0).abs() < 1.0, "{wade:?}");
    assert!(wade.waded > 5.0, "{wade:?}");
    // The ledge stops the walk at its edge, ten units over the water.
    let edge = first(Look::Edge);
    assert!(
        edge.stop.x < 0.5 && (edge.stop.y - 10.0).abs() < 0.01,
        "{edge:?}"
    );
    assert!((edge.drop - 10.0).abs() < 0.01, "{edge:?}");
    // The lava stops the walk before x 30.
    let lava = first(Look::Lava);
    assert!(lava.stop.x < 30.0 && lava.stop.x > 28.0, "{lava:?}");
    // Each look's script runs as the client's scripts do. The ledge's sends
    // the character to its start, the middle of the body two units over the
    // ledge at EQ Z 10, and jumps holding forward once the walk stops.
    for spot in &spots {
        assert!(spot.describe(6.0).starts_with("look="), "{spot:?}");
        let text = spot.script("look", 6.0).join("\n");
        let steps = crate::script::parse(&text, std::path::Path::new("."));
        assert!(steps.is_ok_and(|steps| steps.len() >= 6), "{text}");
    }
    let script = edge.script("edge", 6.0);
    assert!(
        script[0].starts_with("gm goto ") && script[0].ends_with(" 15.0"),
        "{script:?}"
    );
    assert_eq!(script[script.len() - 2], "hold w+space 2000");
}

/// Prints, for every race and gender in `EQ_PROBE_RACES` (as
/// [`inspect_model_heights`] reads them, defaulting to the playable races
/// and the Coldain), how far the lowest drawn point of
/// each standing and moving clip (`P01`, `L01`, `L02`) falls from the
/// base pose's, which is where a model's soles are hung: the lowest and
/// highest of the clip's bottoms over its frames at 30 a second, in model
/// units and in world units at size 4. A negative bottom draws the feet
/// that far under the floor the base pose stands on.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL"]
fn inspect_clip_soles() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zones = std::env::var("EQ_PROBE_ZONES").unwrap_or_else(|_| {
        "thurgadina,halas,kaladima,akanon,rivervale,qeynos,everfrost,cobaltscar".into()
    });
    let races = std::env::var("EQ_PROBE_RACES").unwrap_or_else(|_| {
        "1:0,1:1,2:0,2:1,3:0,3:1,4:0,4:1,5:0,5:1,6:0,6:1,7:0,7:1,8:0,8:1,9:0,9:1,\
             10:0,10:1,11:0,11:1,12:0,12:1,128:0,128:1,183:0,183:1"
            .into()
    });
    let donors = eq_client_assets::characters::DonorArchives::default();
    for pair in races.split(',') {
        let (race, gender) = pair.trim().split_once(':').unwrap();
        let (race, gender): (u32, u32) = (race.parse().unwrap(), gender.parse().unwrap());
        let Some(code) = eq_client_core::races::model(race, gender) else {
            continue;
        };
        let Some((zone, asset)) = zones.split(',').find_map(|zone| {
            eq_client_assets::characters::load_installed_character(&install, zone, code, &donors)
                .ok()
                .map(|asset| (zone, asset))
        }) else {
            println!("race={race} gender={gender} model={code} loaded=no");
            continue;
        };
        let (base, _) = asset.span().unwrap_or_default();
        let world = 4.0 / asset.height();
        let lowest = |clip: &str, seconds: f32| {
            asset
                .pose(clip, seconds)
                .iter()
                .zip(&asset.pieces)
                .filter(|(_, piece)| piece.base())
                .flat_map(|(pose, _)| &pose.positions)
                .map(|position| position[1])
                .reduce(f32::min)
                .unwrap_or(base)
        };
        for clip in ["P01", "L01", "L02"] {
            let Some(length) = asset.clip_seconds(clip) else {
                println!("race={race} gender={gender} model={code} zone={zone} clip={clip} none");
                continue;
            };
            let (low, high) = std::iter::successors(Some(0.0_f32), |seconds| {
                Some(seconds + 1.0 / 30.0).filter(|next| *next < length)
            })
            .map(|seconds| lowest(clip, seconds) - base)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), bottom| {
                (low.min(bottom), high.max(bottom))
            });
            println!(
                "race={race} gender={gender} model={code} zone={zone} clip={clip} \
                 seconds={length:.2} base_bottom={base:.3} height={:.3} \
                 bottom_from_base={low:.3}..{high:.3} at_size_4={:.3}..{:.3}",
                asset.height(),
                low * world,
                high * world
            );
        }
    }
}

/// Prints each model's base-pose span against the z offset table, to tell
/// whether the table's factors are the meshes' own geometry: for every race
/// and gender in `EQ_PROBE_RACES` (`race:gender` pairs, defaulting to the
/// short races and the classic models the table names), the model's lowest
/// and highest points, its height, the table's factor, and the factor its
/// geometry predicts if the origin hangs 0.625 x size under z, with the
/// height the global archives alone give and the race's default drawn size,
/// which the navmesh's walker sizes come from. Models load from the first
/// zone in `EQ_PROBE_ZONES` whose archives hold them.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL"]
fn inspect_model_heights() {
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zones = std::env::var("EQ_PROBE_ZONES").unwrap_or_else(|_| {
        "thurgadina,halas,kaladima,akanon,rivervale,qeynos,everfrost,cobaltscar".into()
    });
    let races = std::env::var("EQ_PROBE_RACES").unwrap_or_else(|_| {
        "1:0,1:1,8:0,8:1,11:0,11:1,12:0,12:1,183:0,183:1,14:0,46:0,63:0,66:0,90:0,90:1,\
         147:0,158:0,175:0,187:0,190:0,216:0"
            .into()
    });
    let donors = eq_client_assets::characters::DonorArchives::default();
    for pair in races.split(',') {
        let (race, gender) = pair.trim().split_once(':').unwrap();
        let (race, gender): (u32, u32) = (race.parse().unwrap(), gender.parse().unwrap());
        let Some(code) = eq_client_core::races::model(race, gender) else {
            println!("race={race} gender={gender} model=none");
            continue;
        };
        let loaded = zones.split(',').find_map(|zone| {
            eq_client_assets::characters::load_installed_character(&install, zone, code, &donors)
                .ok()
                .map(|asset| (zone, asset))
        });
        let Some((zone, asset)) = loaded else {
            println!("race={race} gender={gender} model={code} loaded=no");
            continue;
        };
        let (bottom, top) = asset.span().unwrap_or_default();
        let height = asset.height();
        let table = eq_client_core::z_offset(race, 5.0) / (0.2 * 5.0);
        let origin_factor = 3.125 - bottom / (0.2 * height);
        // The model's height from the global archives alone, and the size a
        // spawn of the race is drawn at.
        let global = eq_client_assets::characters::load_global_character(&install, code, &donors)
            .map_or_else(
                |_| "none".to_owned(),
                |asset| format!("{:.3}", asset.height()),
            );
        let drawn_size = eq_client_core::size_or_default(race, 0.0);
        println!(
            "race={race} gender={gender} model={code} zone={zone} bottom={bottom:.3} \
             top={top:.3} height={height:.3} global_height={global} \
             default_size={drawn_size:.2} table_factor={table:.3} \
             origin_rule_factor={origin_factor:.3} soles_rule_factor=3.125"
        );
    }
}

/// Routes a zone's ways over its navigation mesh and over the grid, as a
/// script's walk plans them with the navmesh switch on and off (the navmesh
/// plan's step 4 look). Nothing opens a window.
///
/// The mesh is built as the client builds it at zone entry, for the height
/// of `EQ_PROBE_MODEL` (default 6). Each route in `EQ_PROBE_ROUTES`
/// (`name=x,y,z>x,y,z` renderer points a little above their floor, separated
/// by `;`), or without it the two far ends of the mesh's largest island as
/// the route `far` (and with `EQ_PROBE_COVERED` set to a roof height, the
/// far pair with a down-facing roof within that height as the route `covered`, or a
/// line saying there is none), is searched by
/// both planners, 5,000 units of work a call, and
/// prints the planner, its legs, how many kept the clearance or passed only
/// the plain walk, each repaired leg's ends, any fallback, the seconds the
/// whole search took with its leg checks, the path's length, and how many of
/// its stretches take the walker's head under water. `EQ_PROBE_DOORS` (comma-separated ids) lists the doors a route may open;
/// door colliders are not loaded. A route with an end that has no floor
/// within 50 units under it is reported and skipped. `EQ_PROBE_LOCATE`
/// (renderer `x,y,z` points separated by `;`) prints the mesh's polygon and
/// floor height at each, if any. After each mesh route, a line gives the
/// islands under its ends and whether they are joined, and the same for a
/// fallback leg's ends. Each end's floor is sought from 8 units over it.
/// Last, a `look_under` line for each of the far pair's and the covered
/// pair's points and each fallback leg's ends: what lies over and under it. `EQ_PROBE_DROPS=1` lets both planners step
/// off ledges, and the mesh's drops taken are printed too.
///
/// The build line gives the mesh's ways off ledges and the seconds spent
/// finding them in the zone's own world, the next line the zone's liquid
/// regions and the cache file's bytes, then the three with the longest falls, each as EQ X,Y,Z
/// from its top to its landing and as a route ready for `EQ_PROBE_ROUTES`.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
#[allow(clippy::too_many_lines)]
fn inspect_mesh_routes() {
    use eq_client_core::movement::Liquids as _;
    use eq_client_core::movement::navmesh::{self, MeshNavigator, NavMesh, Settings};
    use eq_client_core::movement::{
        GridNavigator, Navigator, PathProgress, Planned, Scope, SearchRequest,
    };
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::Instant;
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone_name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let zone = eq_client_assets::load_zone(&install, &zone_name).unwrap();
    let triangles = collision_triangles(&zone);
    let world = build_collision(&zone).unwrap();
    let height = std::env::var("EQ_PROBE_MODEL").map_or(6.0, |model| {
        eq_client_assets::characters::load_character(&install.join("global_chr.s3d"), &model)
            .unwrap()
            .height()
    });
    let doors: Vec<u8> = std::env::var("EQ_PROBE_DOORS").map_or(Vec::new(), |doors| {
        doors
            .split(',')
            .map(|d| d.trim().parse().unwrap())
            .collect()
    });
    let drops = std::env::var("EQ_PROBE_DROPS").is_ok_and(|drops| drops == "1");
    let building = Instant::now();
    // EQ_PROBE_DROP_WORLD=triangles finds the drops in a world built from
    // the triangles alone, with no water, as before the zone's own world
    // was handed in.
    let triangles_world = std::env::var("EQ_PROBE_DROP_WORLD").is_ok_and(|w| w == "triangles");
    let (mesh, report) = NavMesh::build_in(
        &triangles,
        (!triangles_world).then_some(&world),
        &Settings::walker(height),
        navmesh::build_threads(),
        &AtomicBool::new(false),
    )
    .unwrap();
    println!(
        "zone={zone_name} triangles={} height={height} release={} build_seconds={:.2} polygons={} \
         islands={} refused_tiles={} deep_triangles={} drop_links={} drop_seconds={:.2} refused_stretches={} \
         drops={drops} drop_world={}",
        triangles.len(),
        !cfg!(debug_assertions),
        building.elapsed().as_secs_f32(),
        mesh.polygon_count(),
        mesh.island_count(),
        report.refused.len(),
        report.deep_triangles,
        report.drop_links,
        report.drop_elapsed.as_secs_f32(),
        report.refused_stretches,
        if triangles_world { "triangles" } else { "zone" },
    );
    let liquids = ZoneLiquids(zone.regions.clone());
    // The cache file's size, written to a folder of the probe's own and
    // removed again.
    let folder = std::env::temp_dir().join(format!("eq-probe-navmesh-{}", std::process::id()));
    let bytes = navmesh::cache::store(
        &folder,
        &zone_name,
        &triangles,
        &Settings::walker(height),
        &mesh,
    );
    let _ = std::fs::remove_dir_all(&folder);
    println!(
        "liquid_regions={} cache_bytes={}",
        zone.regions.liquid_region_count(),
        bytes.map_or_else(
            |error| format!("unknown ({error})"),
            |bytes| bytes.to_string()
        ),
    );
    let mut deepest: Vec<(Vec3, Vec3)> = mesh.drop_links().collect();
    deepest.sort_by(|a, b| (b.0.y - b.1.y).total_cmp(&(a.0.y - a.1.y)));
    let raised = |v: Vec3| format!("{:.1},{:.1},{:.1}", v.x, v.y + 2.0, v.z);
    for (k, (top, landing)) in deepest.iter().take(3).enumerate() {
        println!(
            "drop_link={k} top_eq={} landing_eq={} fall={:.1} route=drop{k}={}>{}",
            eq_point(*top),
            eq_point(*landing),
            top.y - landing.y,
            raised(*top),
            raised(*landing),
        );
    }
    // None where no floor lies within 50 units under the point; its route
    // is then reported and skipped.
    // Started 8 units over the point, so floor a mesh polygon sits a little
    // under, as it may on a steep slope, is still found.
    let floor = |at: Vec3| {
        let above = at + Vec3::Y * 8.0;
        let below = world.ray_distance(above, -Vec3::Y, 58.0)?;
        Some(above - Vec3::Y * below)
    };
    // Whether the mesh has floor at each of `EQ_PROBE_LOCATE`'s points
    // (renderer x,y,z, separated by `;`), and at what height.
    for at in std::env::var("EQ_PROBE_LOCATE")
        .iter()
        .flat_map(|points| points.split(';'))
        .filter(|p| !p.trim().is_empty())
    {
        let v: Vec<f32> = at.split(',').map(|n| n.trim().parse().unwrap()).collect();
        let at = Vec3::new(v[0], v[1], v[2]);
        let located = mesh.locate(at);
        println!(
            "locate at_eq={} polygon={:?} floor_height={:?}",
            eq_point(at),
            located.map(|(polygon, _)| polygon),
            located.map(|(_, height)| height),
        );
    }
    let point = |s: &str| {
        let v: Vec<f32> = s.split(',').map(|n| n.trim().parse().unwrap()).collect();
        floor(Vec3::new(v[0], v[1], v[2]))
    };
    let upright = Upright::new(&triangles);
    // Mesh points to look under: the far and covered pairs' own.
    let mut checked: Vec<(String, Vec3)> = Vec::new();
    let routes: Vec<(String, Option<Vec3>, Option<Vec3>)> = std::env::var("EQ_PROBE_ROUTES")
        .map_or_else(
            |_| {
                let (a, b) = mesh.far_pair().expect("a mesh with polygons");
                checked.extend([("far_start".to_owned(), a), ("far_goal".to_owned(), b)]);
                let mut routes = vec![(
                    "far".to_owned(),
                    floor(a + Vec3::Y * 2.0),
                    floor(b + Vec3::Y * 2.0),
                )];
                // The far pair under a roof: floor with a ceiling no more than
                // `EQ_PROBE_COVERED` units over it, as in a tunnel.
                if let Some(roof) = std::env::var("EQ_PROBE_COVERED")
                    .ok()
                    .and_then(|roof| roof.parse::<f32>().ok())
                {
                    // A triangle over it faces down within the roof's height, as
                    // a roof does; the slope its polygon sags under faces up
                    // and is passed over.
                    let covered = |at: Vec3| {
                        upright
                            .hits(at + Vec3::Y * 2.0, true)
                            .any(|(distance, up)| distance <= roof && up < 0.0)
                    };
                    if let Some((a, b)) = mesh.far_pair_where(covered) {
                        checked.extend([("covered_a".to_owned(), a), ("covered_b".to_owned(), b)]);
                        println!(
                            "covered_pair a_eq={} b_eq={} route=covered={}>{}",
                            eq_point(a),
                            eq_point(b),
                            raised(a),
                            raised(b),
                        );
                        routes.push((
                            "covered".to_owned(),
                            floor(a + Vec3::Y * 2.0),
                            floor(b + Vec3::Y * 2.0),
                        ));
                    } else {
                        println!("covered_pair none roof={roof}");
                    }
                }
                routes
            },
            |routes| {
                routes
                    .split(';')
                    .filter(|r| !r.trim().is_empty())
                    .map(|route| {
                        let (name, ends) = route.split_once('=').unwrap();
                        let (a, b) = ends.split_once('>').unwrap();
                        (name.trim().to_owned(), point(a), point(b))
                    })
                    .collect()
            },
        );
    let mesh = Arc::new(mesh);
    let navigators: [(&str, Box<dyn Navigator>); 2] = [
        ("grid", Box::new(GridNavigator)),
        ("mesh", Box::new(MeshNavigator::new(Arc::clone(&mesh)))),
    ];
    for (name, start, goal) in routes {
        let (Some(start), Some(goal)) = (start, goal) else {
            println!(
                "route={name} skipped: no floor within 50 under an end; start_has_floor={} \
                 goal_has_floor={}",
                start.is_some(),
                goal.is_some()
            );
            continue;
        };
        for (planner, navigator) in &navigators {
            let searching = Instant::now();
            let mut search = navigator.search(SearchRequest {
                start,
                goal,
                reach: 10.0,
                height,
                doors: doors.clone(),
                drops,
                scope: Scope::Whole,
            });
            let progress = loop {
                match search.advance(&world, 5000) {
                    PathProgress::Searching => (),
                    done => break done,
                }
            };
            let seconds = searching.elapsed().as_secs_f32();
            // Stretches of the path along which the walker's head would be
            // under water: a walk on a seabed.
            let under_water = match &progress {
                PathProgress::Found(path) => std::iter::once(start)
                    .chain(path.iter().copied())
                    .collect::<Vec<_>>()
                    .windows(2)
                    .filter(|pair| {
                        let head = Vec3::Y * height;
                        liquids.passes_through(
                            pair[0] + head,
                            pair[1] + head,
                            eq_client_core::movement::Liquid::Water,
                        )
                    })
                    .count(),
                _ => 0,
            };
            let (found, length, points) = match &progress {
                PathProgress::Found(path) => (
                    true,
                    std::iter::once(start)
                        .chain(path.iter().copied())
                        .collect::<Vec<_>>()
                        .windows(2)
                        .map(|pair| pair[0].distance(pair[1]))
                        .sum::<f32>(),
                    path.len(),
                ),
                _ => (false, 0.0, 0),
            };
            let detail = match search.planned() {
                Planned::Grid => String::new(),
                Planned::Mesh {
                    legs,
                    pieces,
                    cut,
                    loose,
                    clear,
                    drops,
                    repaired,
                    fallback,
                    refused,
                    longest_check,
                } => {
                    let repairs: Vec<String> = repaired
                        .iter()
                        .map(|(from, to)| format!("{}>{}", eq_point(*from), eq_point(*to)))
                        .collect();
                    format!(
                        " legs={legs} cut={cut} pieces={pieces} loose_pieces={loose} clear={clear} walk_only={} \
                         drops={drops} repaired={} repaired_eq=[{}] fallback={fallback:?} \
                         refused_walk={} refused_height={} refused_door={} longest_check_ms={:.2}",
                        pieces.saturating_sub(clear + drops + repaired.len()),
                        repaired.len(),
                        repairs.join(" "),
                        refused.walk,
                        refused.height,
                        refused.door,
                        longest_check.as_secs_f64() * 1000.0,
                    )
                }
            };
            println!(
                "route={name} planner={planner} eq_start={} eq_goal={} found={found} seconds={seconds:.3} \
                 searched={} length={length:.1} path_points={points} under_water={under_water}{detail}",
                eq_point(start),
                eq_point(goal),
                search.searched().0,
            );
            if let Planned::Mesh { fallback, .. } = search.planned() {
                println!(
                    "route={name} islands {}{}",
                    islands(&mesh, start, goal),
                    match fallback {
                        Some(eq_client_core::movement::Fallback::Leg { from, to }) => {
                            format!(" | fallback_leg {}", islands(&mesh, from, to))
                        }
                        _ => String::new(),
                    }
                );
                if let Some(eq_client_core::movement::Fallback::Leg { from, to }) = fallback {
                    checked.extend([
                        (format!("{name}_fallback_from"), from),
                        (format!("{name}_fallback_to"), to),
                    ]);
                }
            }
        }
    }
    for (label, at) in checked {
        println!("{}", look_under(&mesh, &world, &upright, &label, at));
    }
}

/// A zone's triangles by the squares of the ground they lie over, for rays
/// straight up and down.
struct Upright<'a> {
    triangles: &'a [[[f32; 3]; 3]],
    squares: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl<'a> Upright<'a> {
    const SQUARE: f32 = 16.0;

    #[allow(clippy::cast_possible_truncation)] // Zone coordinates.
    fn square(x: f32, z: f32) -> (i32, i32) {
        (
            (x / Self::SQUARE).floor() as i32,
            (z / Self::SQUARE).floor() as i32,
        )
    }

    fn new(triangles: &'a [[[f32; 3]; 3]]) -> Self {
        let mut squares: std::collections::HashMap<(i32, i32), Vec<usize>> =
            std::collections::HashMap::new();
        for (i, triangle) in triangles.iter().enumerate() {
            let low = triangle.iter().fold(Vec2::splat(f32::MAX), |low, v| {
                low.min(Vec2::new(v[0], v[2]))
            });
            let high = triangle.iter().fold(Vec2::splat(f32::MIN), |high, v| {
                high.max(Vec2::new(v[0], v[2]))
            });
            let (low, high) = (Self::square(low.x, low.y), Self::square(high.x, high.y));
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    squares.entry((x, z)).or_default().push(i);
                }
            }
        }
        Self { triangles, squares }
    }

    /// Two-sided: the nearest triangle straight up from `at`, or down, its
    /// distance and its normal's up part, as wound.
    fn nearest(&self, at: Vec3, up: bool) -> Option<(f32, f32)> {
        self.hits(at, up).min_by(|x, y| x.0.total_cmp(&y.0))
    }

    /// Every triangle straight up from `at`, or down, as [`Self::nearest`]
    /// gives it.
    fn hits(&self, at: Vec3, up: bool) -> impl Iterator<Item = (f32, f32)> + '_ {
        let direction = if up { Vec3::Y } else { -Vec3::Y };
        self.squares
            .get(&Self::square(at.x, at.z))
            .into_iter()
            .flatten()
            .filter_map(move |&i| {
                // Möller and Trumbore's test.
                let [first, second, third] = self.triangles[i].map(Vec3::from_array);
                let (along, across) = (second - first, third - first);
                let cross = direction.cross(across);
                let det = along.dot(cross);
                if det.abs() < 1e-9 {
                    return None;
                }
                let offset = at - first;
                let u = offset.dot(cross) / det;
                let back = offset.cross(along);
                let v = direction.dot(back) / det;
                let distance = across.dot(back) / det;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance > 1e-4)
                    .then(|| (distance, along.cross(across).normalize_or_zero().y))
            })
    }
}

/// What lies over and under a mesh point: the floor height of the polygon
/// under it; the collision floor nearest that height within a step and 3,
/// where a route's leg ends, and how far the polygon sags under it; the
/// nearest solid straight up and straight down, by distance and by which way
/// the triangle hit faces (its normal's up part, as wound), whichever side
/// the ray meets; and the size of the polygon's island with how many of its
/// polygons have solid floor within 50 units under their own points, sought
/// from 8 over.
fn look_under(
    mesh: &eq_client_core::movement::navmesh::NavMesh,
    world: &eq_client_core::movement::CollisionWorld,
    upright: &Upright,
    label: &str,
    at: Vec3,
) -> String {
    use eq_client_core::movement::MAX_GROUNDED_STEP;
    let shown = |hit: Option<(f32, f32)>| {
        hit.map_or_else(
            || "none".to_owned(),
            |(distance, up)| format!("{distance:.2} facing_up={up:.2}"),
        )
    };
    let located = mesh.locate(at);
    let collision_floor = located.and_then(|(_, height)| {
        let lift = MAX_GROUNDED_STEP + 3.0;
        world.nearest_ground(Vec3::new(at.x, height, at.z), lift, lift)
    });
    let island = located.map(|(polygon, _)| {
        let island = mesh.island_of(polygon);
        #[allow(clippy::cast_possible_truncation)] // Polygons are counted in u32.
        let members: Vec<u32> = (0..mesh.polygon_count() as u32)
            .filter(|&i| mesh.island_of(i) == island)
            .collect();
        let floored = members
            .iter()
            .filter_map(|&i| mesh.polygon_point(i))
            .filter(|p| {
                world
                    .ray_distance(*p + Vec3::Y * 8.0, -Vec3::Y, 58.0)
                    .is_some()
            })
            .count();
        (island, members.len(), floored)
    });
    format!(
        "look_under point={label} at_eq={} polygon_floor={:?} collision_floor={:?} sag={} \
         solid_above={} solid_below={} island={:?} island_polygons={:?} island_floored={:?}",
        eq_point(at),
        located.map(|(_, height)| height),
        collision_floor,
        located.zip(collision_floor).map_or_else(
            || "none".to_owned(),
            |((_, height), floor)| format!("{:.2}", floor - height)
        ),
        shown(upright.nearest(at, true)),
        shown(upright.nearest(at, false)),
        island.map(|i| i.0),
        island.map(|i| i.1),
        island.map(|i| i.2),
    )
}

/// The islands of the polygons under `a` and `b`, whether they are joined,
/// and whether a walk with drops reaches `b` from `a`.
fn islands(mesh: &eq_client_core::movement::navmesh::NavMesh, a: Vec3, b: Vec3) -> String {
    let (at_a, at_b) = (mesh.locate(a), mesh.locate(b));
    let joined = at_a
        .zip(at_b)
        .map(|((a, _), (b, _))| (mesh.connected(a, b), mesh.reaches(a, b, true)));
    format!(
        "a_eq={} a_island={:?} b_eq={} b_island={:?} joined={:?} reached_with_drops={:?}",
        eq_point(a),
        at_a.map(|(polygon, _)| mesh.island_of(polygon)),
        eq_point(b),
        at_b.map(|(polygon, _)| mesh.island_of(polygon)),
        joined.map(|(joined, _)| joined),
        joined.map(|(_, reached)| reached),
    )
}

/// Weighs one mesh build tagged for the three walker sizes against three
/// plain builds, one per size, on a real zone (the prebuild plan's timing
/// check). Nothing opens a window.
///
/// It first reads the zone whole and as its solids alone, and prints both
/// reads' seconds and their triangles' count and hash, which must match;
/// `EQ_PROBE_LOAD_ONLY=1` stops there. The
/// sizes are the races' drawn sizes. Then, for the tagged
/// build and each plain one: build seconds, polygons, islands, drop links,
/// refused tiles and the cache file's bytes. Per size, it gives the floor's
/// area on the tagged build's mesh for that size against the plain build's,
/// and the far pair of the largest island routed on both, drops off.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
#[allow(clippy::too_many_lines)]
fn inspect_mesh_sizes() {
    use eq_client_core::movement::navmesh::{self, MeshNavigator, NavMesh, Settings};
    use eq_client_core::movement::{Navigator, PathProgress, Planned, Scope, SearchRequest};
    use std::hash::{Hash, Hasher};
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::Instant;
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone_name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let hashed = |triangles: &[[[f32; 3]; 3]]| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for value in triangles.iter().flatten().flatten() {
            value.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    };
    let reading = Instant::now();
    let whole = eq_client_assets::load_zone(&install, &zone_name).unwrap();
    let whole_seconds = reading.elapsed().as_secs_f32();
    let reading = Instant::now();
    let zone = eq_client_assets::load_zone_collision(&install, &zone_name).unwrap();
    let solids_seconds = reading.elapsed().as_secs_f32();
    let (whole_triangles, triangles) = (collision_triangles(&whole), collision_triangles(&zone));
    drop(whole);
    println!(
        "zone={zone_name} release={} load_zone_seconds={whole_seconds:.2} \
         load_zone_collision_seconds={solids_seconds:.2} triangles={} collision_triangles={} \
         hash={:016x} collision_hash={:016x} same={}",
        !cfg!(debug_assertions),
        whole_triangles.len(),
        triangles.len(),
        hashed(&whole_triangles),
        hashed(&triangles),
        whole_triangles == triangles,
    );
    drop(whole_triangles);
    // EQ_PROBE_LOAD_ONLY=1 stops after the two reads, for a zone whose
    // builds would take too long to weigh.
    if std::env::var("EQ_PROBE_LOAD_ONLY").is_ok_and(|only| only == "1") {
        return;
    }
    let sizes = super::zone_navmesh::walker_sizes();
    println!("sizes={sizes:?}");
    let world = build_collision(&zone).unwrap();
    let cache_bytes = |mesh: &NavMesh, settings: &Settings| {
        let folder =
            std::env::temp_dir().join(format!("eq-probe-navmesh-sizes-{}", std::process::id()));
        let bytes = navmesh::cache::store(&folder, &zone_name, &triangles, settings, mesh);
        let _ = std::fs::remove_dir_all(&folder);
        bytes.map_or_else(
            |error| format!("unknown ({error})"),
            |bytes| bytes.to_string(),
        )
    };
    let build = |label: &str, settings: &Settings| {
        let building = Instant::now();
        let (mesh, report) = NavMesh::build_in(
            &triangles,
            Some(&world),
            settings,
            navmesh::build_threads(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let seconds = building.elapsed().as_secs_f32();
        println!(
            "build={label} seconds={seconds:.2} drop_seconds={:.2} polygons={} islands={} \
             drop_links={} refused_tiles={} deep_triangles={} refused_stretches={} cache_bytes={}",
            report.drop_elapsed.as_secs_f32(),
            mesh.polygon_count(),
            mesh.island_count(),
            report.drop_links,
            report.refused.len(),
            report.deep_triangles,
            report.refused_stretches,
            cache_bytes(&mesh, settings),
        );
        for (reason, count) in report.refusals_by_reason() {
            println!("build={label} refused={count} reason={reason}");
        }
        (mesh, seconds)
    };
    let (tagged, tagged_seconds) = build("tagged", &Settings::sized(sizes));
    let meshes = tagged.split(sizes);
    let route = |mesh: &Arc<NavMesh>, start: Vec3, goal: Vec3, height: f32| {
        let searching = Instant::now();
        let navigator = MeshNavigator::new(Arc::clone(mesh));
        let mut search = navigator.search(SearchRequest {
            start,
            goal,
            reach: 10.0,
            height,
            doors: Vec::new(),
            drops: false,
            scope: Scope::Whole,
        });
        let progress = loop {
            match search.advance(&world, 5000) {
                PathProgress::Searching => (),
                done => break done,
            }
        };
        let length = match &progress {
            PathProgress::Found(path) => std::iter::once(start)
                .chain(path.iter().copied())
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .sum::<f32>(),
            _ => 0.0,
        };
        let detail = match search.planned() {
            Planned::Mesh {
                legs,
                repaired,
                fallback,
                ..
            } => format!(
                "legs={legs} repaired={} fallback={fallback:?}",
                repaired.len()
            ),
            Planned::Grid => "grid".to_owned(),
        };
        format!(
            "found={} seconds={:.3} length={length:.1} {detail}",
            matches!(progress, PathProgress::Found(_)),
            searching.elapsed().as_secs_f32()
        )
    };
    let mut plain_seconds = 0.0;
    for (size, &height) in sizes.iter().enumerate() {
        let (plain, seconds) = build(&format!("plain{size}"), &Settings::walker(height));
        plain_seconds += seconds;
        let ours = meshes.for_height(height).unwrap();
        #[allow(clippy::cast_possible_truncation)] // Three sizes.
        let (area, plain_area) = (tagged.floor_area(size as u8), plain.floor_area(0));
        println!(
            "size={size} height={height:.2} tagged_polygons={} tagged_islands={} \
             plain_polygons={} plain_islands={} tagged_area={area:.0} plain_area={plain_area:.0} \
             area_change_percent={:+.2}",
            ours.polygon_count(),
            ours.island_count(),
            plain.polygon_count(),
            plain.island_count(),
            100.0 * (area - plain_area) / plain_area.max(1.0),
        );
        let plain = Arc::new(plain);
        if let Some((a, b)) = plain.far_pair() {
            let (start, goal) = (a + Vec3::Y * 2.0, b + Vec3::Y * 2.0);
            println!(
                "size={size} route=far eq_start={} eq_goal={} tagged: {} | plain: {}",
                eq_point(start),
                eq_point(goal),
                route(&ours, start, goal, height),
                route(&plain, start, goal, height),
            );
        }
    }
    println!(
        "tagged_seconds={tagged_seconds:.2} three_plain_seconds={plain_seconds:.2} \
         saving_percent={:.1}",
        100.0 * (plain_seconds - tagged_seconds) / plain_seconds.max(f32::EPSILON),
    );
}

/// Looks at one leg the tagged mesh fell back to the grid on, against
/// plain builds at the two shorter sizes (#268's East Commons check).
/// Nothing opens a window.
///
/// `EQ_PROBE_LEG` is the leg as renderer points, `x,y,z>x,y,z`, defaulting
/// to East Commons' fallback leg at bfa110c. For the tagged build's size 0
/// and size 1 meshes and for plain builds at those sizes, it prints the
/// polygon under each end (its size tag on the tagged build), whether the
/// ends share an island, the leg routed alone, and every 2 units along the
/// leg where no polygon lies under it, as runs of gaps. Each build's
/// refused tiles within two tiles of the leg are printed too.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
#[allow(clippy::too_many_lines)]
fn inspect_mesh_leg() {
    use eq_client_core::movement::navmesh::{self, MeshNavigator, NavMesh, Settings};
    use eq_client_core::movement::{Navigator, PathProgress, Planned, Scope, SearchRequest};
    use std::sync::{Arc, atomic::AtomicBool};
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone_name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let leg = std::env::var("EQ_PROBE_LEG")
        .unwrap_or_else(|_| "-441.4,-55.3,-2145.3>-1019.4,-15.2,-1521.2".to_owned());
    let point = |text: &str| {
        let v: Vec<f32> = text.split(',').map(|n| n.trim().parse().unwrap()).collect();
        Vec3::new(v[0], v[1], v[2])
    };
    let (from, to) = leg.split_once('>').unwrap();
    let (from, to) = (point(from), point(to));
    let zone = eq_client_assets::load_zone_collision(&install, &zone_name).unwrap();
    let triangles = collision_triangles(&zone);
    let world = build_collision(&zone).unwrap();
    let low = triangles
        .iter()
        .flatten()
        .fold(Vec3::splat(f32::MAX), |low, p| {
            low.min(Vec3::from_array(*p))
        });
    let sizes = super::zone_navmesh::walker_sizes();
    println!(
        "zone={zone_name} sizes={sizes:?} leg_eq={}>{} leg_length={:.1}",
        eq_point(from),
        eq_point(to),
        from.distance(to)
    );
    let build = |label: &str, settings: &Settings| {
        let (mesh, report) = NavMesh::build_in(
            &triangles,
            Some(&world),
            settings,
            navmesh::build_threads(),
            &AtomicBool::new(false),
        )
        .unwrap();
        #[allow(clippy::cast_possible_truncation)]
        let tile_of = |p: Vec3| {
            (
                ((p.x - low.x) / settings.tile).floor() as i32,
                ((p.z - low.z) / settings.tile).floor() as i32,
            )
        };
        let (a, b) = (tile_of(from), tile_of(to));
        let near: Vec<_> = report
            .refused
            .iter()
            .filter(|tile| {
                let (x, z) = tile.tile;
                x >= a.0.min(b.0) - 2
                    && x <= a.0.max(b.0) + 2
                    && z >= a.1.min(b.1) - 2
                    && z <= a.1.max(b.1) + 2
            })
            .map(|tile| format!("{:?}:{}", tile.tile, tile.step))
            .collect();
        println!(
            "build={label} polygons={} islands={} refused_tiles={} leg_tiles={a:?}..{b:?} \
             refused_near_leg={near:?}",
            mesh.polygon_count(),
            mesh.island_count(),
            report.refused.len(),
        );
        mesh
    };
    let route = |mesh: &Arc<NavMesh>, height: f32| {
        let navigator = MeshNavigator::new(Arc::clone(mesh));
        let mut search = navigator.search(SearchRequest {
            start: from,
            goal: to,
            reach: 10.0,
            height,
            doors: Vec::new(),
            drops: false,
            scope: Scope::Whole,
        });
        let progress = loop {
            match search.advance(&world, 5000) {
                PathProgress::Searching => (),
                done => break done,
            }
        };
        let detail = match search.planned() {
            Planned::Mesh {
                legs,
                repaired,
                fallback,
                ..
            } => format!(
                "legs={legs} repaired={} fallback={fallback:?}",
                repaired.len()
            ),
            Planned::Grid => "grid".to_owned(),
        };
        format!(
            "found={} {detail}",
            matches!(progress, PathProgress::Found(_))
        )
    };
    let look = |label: &str, mesh: &Arc<NavMesh>, tagged: Option<&NavMesh>, height: f32| {
        let end = |at: Vec3| {
            mesh.locate(at).map_or_else(
                || "none".to_owned(),
                |(polygon, floor)| {
                    let size = tagged
                        .and_then(|whole| whole.locate(at))
                        .map(|(i, _)| whole_size(tagged, i))
                        .unwrap_or_default();
                    format!("polygon={polygon} floor={floor:.2}{size}")
                },
            )
        };
        let joined = match (mesh.locate(from), mesh.locate(to)) {
            (Some((a, _)), Some((b, _))) => mesh.connected(a, b).to_string(),
            _ => "unknown".to_owned(),
        };
        // Runs of 2-unit steps along the leg with no polygon under them.
        let steps = (from.distance(to) / 2.0).ceil().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = steps as usize;
        let mut gaps = Vec::new();
        let mut run: Option<(usize, usize)> = None;
        for step in 0..=count {
            #[allow(clippy::cast_precision_loss)]
            let at = from.lerp(to, step as f32 / steps);
            // The leg's ends are a little over the floor; look a few units
            // either way of the straight line's height.
            let under = [0.0, 2.0, -2.0, 4.0, -4.0]
                .iter()
                .any(|dy| mesh.locate(at + Vec3::Y * *dy).is_some());
            match (under, &mut run) {
                (false, None) => run = Some((step, step)),
                (false, Some((_, last))) => *last = step,
                (true, Some(_)) => gaps.push(run.take().unwrap()),
                (true, None) => (),
            }
        }
        gaps.extend(run);
        #[allow(clippy::cast_precision_loss)]
        let gaps: Vec<String> = gaps
            .iter()
            .take(12)
            .map(|&(a, b)| {
                format!(
                    "{}..{} ({} steps)",
                    eq_point(from.lerp(to, a as f32 / steps)),
                    eq_point(from.lerp(to, b as f32 / steps)),
                    b - a + 1
                )
            })
            .collect();
        println!(
            "mesh={label} height={height:.1} start: {} | goal: {} | same_island={joined} | \
             route: {} | gaps={gaps:?}",
            end(from),
            end(to),
            route(mesh, height),
        );
    };
    let tagged = build("tagged", &Settings::sized(sizes));
    let meshes = tagged.split(sizes);
    for (size, &height) in sizes.iter().enumerate().take(2) {
        look(
            &format!("tagged{size}"),
            &meshes.for_height(height).unwrap(),
            Some(&tagged),
            height,
        );
        let plain = Arc::new(build(&format!("plain{size}"), &Settings::walker(height)));
        look(&format!("plain{size}"), &plain, None, height);
    }
}

/// Checks every link of a zone's tagged build and of a plain build at the
/// short size: whether each portal lies on an edge of the polygon it leads
/// to, within 1.5 cells across and the tiles' joining tolerance up or down,
/// and whether that polygon links back (#268's seam check). Nothing opens a
/// window.
///
/// It prints each build's link count and its faults, split into links
/// across a change of size and links within one, with the first ten of
/// each as EQ points. Then, along `EQ_PROBE_LEG` (renderer points
/// `x,y,z>x,y,z`, defaulting to East Commons' fallback leg at bfa110c),
/// every 2 units, it finds the zone's floor nearest the straight line's
/// height within 40 units and says how many steps have a floor there and a
/// polygon on it, on each build.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
#[allow(clippy::too_many_lines)]
fn inspect_mesh_links() {
    use eq_client_core::movement::navmesh::{self, NavMesh, Settings};
    use std::sync::atomic::AtomicBool;
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone_name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let leg = std::env::var("EQ_PROBE_LEG")
        .unwrap_or_else(|_| "-441.4,-55.3,-2145.3>-1019.4,-15.2,-1521.2".to_owned());
    let point = |text: &str| {
        let v: Vec<f32> = text.split(',').map(|n| n.trim().parse().unwrap()).collect();
        Vec3::new(v[0], v[1], v[2])
    };
    let (from, to) = leg.split_once('>').unwrap();
    let (from, to) = (point(from), point(to));
    let zone = eq_client_assets::load_zone_collision(&install, &zone_name).unwrap();
    let triangles = collision_triangles(&zone);
    let world = build_collision(&zone).unwrap();
    let sizes = super::zone_navmesh::walker_sizes();
    for (label, settings) in [
        ("tagged", Settings::sized(sizes)),
        ("plain0", Settings::walker(sizes[0])),
    ] {
        let (mesh, report) = NavMesh::build_in(
            &triangles,
            Some(&world),
            &settings,
            navmesh::build_threads(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let (across, up) = (
            settings.cell_size * 1.5,
            settings.cell_height * 4.0 + settings.climb,
        );
        let (links, faults) = mesh.link_faults(across, up);
        let (seam, same): (Vec<_>, Vec<_>) = faults
            .iter()
            .partition(|fault| fault.sizes.0 != fault.sizes.1);
        let one_way = faults.iter().filter(|fault| fault.one_way).count();
        println!(
            "zone={zone_name} build={label} polygons={} refused_tiles={} links={links} \
             across_limit={across:.2} up_limit={up:.2} faults={} seam_faults={} \
             same_size_faults={} one_way={one_way}",
            mesh.polygon_count(),
            report.refused.len(),
            faults.len(),
            seam.len(),
            same.len(),
        );
        for (kind, list) in [("seam", &seam), ("same", &same)] {
            for fault in list.iter().take(10) {
                println!(
                    "build={label} fault={kind} sizes={:?} eq_a={} eq_b={} across={:.2} up={:.2} \
                     one_way={}",
                    fault.sizes,
                    eq_point(fault.a),
                    eq_point(fault.b),
                    fault.across,
                    fault.up,
                    fault.one_way
                );
            }
        }
        // The leg's steps against the zone's own floor under them.
        let steps = (from.distance(to) / 2.0).ceil().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = steps as usize;
        let (mut floored, mut meshed, mut first_bare) = (0, 0, None);
        for step in 0..=count {
            #[allow(clippy::cast_precision_loss)]
            let at = from.lerp(to, step as f32 / steps);
            let Some(floor) = world.nearest_ground(at, 40.0, 40.0) else {
                continue;
            };
            floored += 1;
            let on = Vec3::new(at.x, floor, at.z);
            if mesh.locate(on).is_some() {
                meshed += 1;
            } else if first_bare.is_none() {
                first_bare = Some(eq_point(on));
            }
        }
        println!(
            "build={label} leg_eq={}>{} steps={} with_floor={floored} with_polygon_on_floor={meshed} \
             first_floor_without_polygon={}",
            eq_point(from),
            eq_point(to),
            count + 1,
            first_bare.unwrap_or_else(|| "none".to_owned())
        );
    }
}

/// ` size=` and the tag of polygon `i` on the whole tagged mesh, where
/// there is one.
fn whole_size(tagged: Option<&eq_client_core::movement::navmesh::NavMesh>, i: u32) -> String {
    tagged.map_or_else(String::new, |mesh| {
        format!(" size={}", mesh.polygon_size(i))
    })
}

/// Where a zone's solids lie up and down, and what lies far from the rest:
/// for a zone whose height range overflows the mesh's tiles (Dreadlands'
/// build refused every tile). Nothing opens a window.
///
/// It prints the bounds of the zone's own collision and of its placed
/// objects', the triangles by 1,000-unit band of height, and every placed
/// object or terrain triangle with a corner more than `EQ_PROBE_FAR`
/// (default 5,000) units below or above the terrain's middle band, each
/// object by its model, placement and scale and the count of the zone's
/// other solids, not far, within 60 units of it across the floor, at most
/// 40 lines.
#[test]
#[ignore = "requires EQ_PROBE_INSTALL and EQ_PROBE_ZONE"]
#[allow(clippy::too_many_lines)]
fn inspect_zone_heights() {
    use std::collections::BTreeMap;
    let install = PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let zone_name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let far: f32 = std::env::var("EQ_PROBE_FAR").map_or(5000.0, |far| far.parse().unwrap());
    let zone = eq_client_assets::load_zone_collision(&install, &zone_name).unwrap();
    let bounds = |triangles: &mut dyn Iterator<Item = [f32; 3]>| {
        triangles.fold(None, |bounds: Option<(Vec3, Vec3)>, p| {
            let p = Vec3::from_array(p);
            Some(bounds.map_or((p, p), |(low, high)| (low.min(p), high.max(p))))
        })
    };
    let terrain = bounds(&mut zone.collision.iter().flatten().copied());
    let placed: Vec<(usize, Vec<[[f32; 3]; 3]>)> = zone
        .objects
        .iter()
        .enumerate()
        .map(|(i, object)| {
            let transform = object_transform(object).to_matrix();
            let model = &zone.models[object.model];
            (
                i,
                model
                    .collision
                    .iter()
                    .map(|t| t.map(|p| transform.transform_point3(Vec3::from_array(p)).to_array()))
                    .collect(),
            )
        })
        .collect();
    let objects = bounds(&mut placed.iter().flat_map(|(_, t)| t.iter().flatten().copied()));
    let eq = |bounds: Option<(Vec3, Vec3)>| {
        bounds.map_or_else(
            || "none".to_owned(),
            |(low, high)| format!("{}..{}", eq_point(low), eq_point(high)),
        )
    };
    println!(
        "zone={zone_name} terrain_triangles={} terrain_eq={} objects={} object_triangles={} \
         objects_eq={}",
        zone.collision.len(),
        eq(terrain),
        zone.objects.len(),
        placed.iter().map(|(_, t)| t.len()).sum::<usize>(),
        eq(objects),
    );
    let mut bands: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    #[allow(clippy::cast_possible_truncation)] // Zone heights.
    let band = |t: &[[f32; 3]; 3]| (t.iter().map(|p| p[1]).sum::<f32>() / 3000.0).floor() as i32;
    for t in &zone.collision {
        bands.entry(band(t)).or_default().0 += 1;
    }
    for t in placed.iter().flat_map(|(_, t)| t) {
        bands.entry(band(t)).or_default().1 += 1;
    }
    for (band, (terrain, objects)) in &bands {
        println!(
            "band_eq_z={}..{} terrain={terrain} objects={objects}",
            band * 1000,
            (band + 1) * 1000
        );
    }
    // The middle of the band holding the most terrain.
    let middle = bands
        .iter()
        .max_by_key(|(_, (terrain, _))| *terrain)
        .map_or(0.0, |(band, _)| {
            #[allow(clippy::cast_precision_loss)] // A band number.
            let middle = (*band as f32 + 0.5) * 1000.0;
            middle
        });
    let is_far = |t: &[[f32; 3]; 3]| t.iter().any(|p| (p[1] - middle).abs() > far);
    let mut lines = 0;
    for t in zone.collision.iter().filter(|t| is_far(t)) {
        if lines < 20 {
            println!(
                "far_terrain_triangle={} {} {}",
                eq_point(Vec3::from_array(t[0])),
                eq_point(Vec3::from_array(t[1])),
                eq_point(Vec3::from_array(t[2])),
            );
        }
        lines += 1;
    }
    println!("far_terrain_triangles={lines}");
    let mut listed = 0;
    for (i, triangles) in &placed {
        let count = triangles.iter().filter(|t| is_far(t)).count();
        if count == 0 {
            continue;
        }
        let object = &zone.objects[*i];
        if listed < 20 {
            // The zone's other solids, not far, across the floor within 60
            // units of the object's own: a mesh tile holding the object
            // holds these too.
            let (low, high) = bounds(&mut triangles.iter().flatten().copied()).expect("triangles");
            let near = |t: &[[f32; 3]; 3]| {
                t.iter().any(|p| {
                    p[0] >= low.x - 60.0
                        && p[0] <= high.x + 60.0
                        && p[2] >= low.z - 60.0
                        && p[2] <= high.z + 60.0
                })
            };
            let others = zone
                .collision
                .iter()
                .chain(
                    placed
                        .iter()
                        .filter(|(j, _)| j != i)
                        .flat_map(|(_, t)| t.iter()),
                )
                .filter(|t| !is_far(t) && near(t))
                .count();
            println!(
                "far_object={i} model={} at_eq={} scale={:?} far_triangles={count} of {} \
                 bounds_eq={} other_solids_within_60={others}",
                zone.models[object.model].name,
                eq_point(Vec3::from_array(object.translation)),
                object.scale,
                triangles.len(),
                eq(Some((low, high))),
            );
        }
        listed += 1;
    }
    println!("far_objects={listed}");
}

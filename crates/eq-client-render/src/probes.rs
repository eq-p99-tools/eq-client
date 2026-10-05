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

    /// The spot in EQ coordinates, X, Y, Z, each point where `/loc` (which
    /// prints Y, X, Z) and a script's `report` put a character of this
    /// height standing there: its middle, not its feet.
    fn describe(&self, height: f32) -> String {
        let eq = |point: Vec3| {
            let at = world_position((point + Vec3::Y * height * 0.5).to_array(), 0.0);
            format!("{:.1},{:.1},{:.1}", at.x, at.y, at.z)
        };
        format!(
            "look={:?} start_xyz={} heading={:.0} toward_xyz={} stops_at_xyz={} walked={:.1} \
             waded={:.1} drop={:.1}",
            self.look,
            eq(self.start),
            self.heading(),
            eq(self.toward),
            eq(self.stop),
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

/// Samples the ground on a grid this fine across `low..high`, in the
/// renderer's frame, picks per 32-unit square the dry sample whose nearest
/// deep water, and the one whose nearest lava, lies about four samples off
/// (within ten), and walks there with the guard on. A walk the guard stops
/// just short of what it heads for shows a ledge where the ground beyond
/// lies 4 or more units lower, else a wade, or lava's edge.
fn liquid_spots(
    world: &eq_client_core::movement::CollisionWorld,
    liquids: &dyn eq_client_core::movement::Liquids,
    (low, high): (Vec3, Vec3),
    (spacing, height): (f32, f32),
) -> Vec<Spot> {
    use std::collections::{BTreeMap, HashMap};
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
    let mut grid: HashMap<(i32, i32), (Vec3, Ground)> = HashMap::new();
    let (mut i, mut x) = (0, low.x);
    while x <= high.x {
        let (mut j, mut z) = (0, low.z);
        while z <= high.z {
            let top = Vec3::new(x, high.y + 1.0, z);
            if let Some(depth) = world.ray_distance(top, -Vec3::Y, high.y - low.y + 2.0) {
                let feet = top - Vec3::Y * depth;
                grid.insert((i, j), (feet, Ground::at(liquids, feet, height)));
            }
            (j, z) = (j + 1, z + spacing);
        }
        (i, x) = (i + 1, x + spacing);
    }
    let mut chosen: BTreeMap<(i32, i32, Ground), (i32, Vec3, Vec3)> = BTreeMap::new();
    for (&(i, j), &(start, ground)) in &grid {
        if ground != Ground::Dry {
            continue;
        }
        for target in [Ground::Deep, Ground::Lava] {
            let nearest = DIRECTIONS
                .iter()
                .filter_map(|&(di, dj)| {
                    (1..=10).find_map(|k| match grid.get(&(i + di * k, j + dj * k)) {
                        None => Some(None),
                        Some(&(toward, ground)) if ground == target => Some(Some((k, toward))),
                        Some(_) => None,
                    })?
                })
                .min_by_key(|(k, _)| *k);
            let Some((k, toward)) = nearest else {
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
    chosen
        .into_iter()
        .filter_map(|((_, _, target), (_, start, toward))| {
            // Ground that cannot be stood on, such as a treetop, is no start.
            let settled = walk_toward(world, liquids, (start, start), height).0;
            if (settled.y - start.y).abs() > 0.5 {
                return None;
            }
            let (stop, waded) = walk_toward(world, liquids, (start, toward), height);
            let mut spot = Spot {
                look: Look::Lava,
                start,
                toward,
                stop,
                waded,
                drop: 0.0,
            };
            // The guard stopped it, not a wall: the ground two units on is
            // what it heads for.
            let ahead = stop + spot.direction() * 2.0 + Vec3::Y;
            let depth = world.ray_distance(ahead, -Vec3::Y, high.y - low.y + 2.0)?;
            let floor = ahead - Vec3::Y * depth;
            if Ground::at(liquids, stop, height) != Ground::Dry
                || Ground::at(liquids, floor, height) != target
            {
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
        })
        .collect()
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
/// (`qeynos` for South Qeynos, `lavastorm`): samples the ground on a 4-unit
/// grid, walks from dry ground toward the nearest deep water or lava with
/// the guard on, and prints the best places to wade in, to stand at a ledge
/// over deep water, and to walk toward lava, in EQ coordinates. Then it
/// prints a script that shows the best of each on a local `EQEmu` server.
/// The body is 6 units tall unless `EQ_PROBE_HEIGHT` says otherwise.
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
    let spots = best_spots(liquid_spots(&world, &liquids, bounds, (4.0, height)), 5);
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
}

/// Liquids filling boxes, each from its lower corner to its upper.
struct Boxes(Vec<(Vec3, Vec3, eq_client_core::movement::Liquid)>);

impl eq_client_core::movement::Liquids for Boxes {
    fn liquid_at(&self, point: Vec3) -> Option<eq_client_core::movement::Liquid> {
        self.0
            .iter()
            .find(|(low, high, _)| point.cmpge(*low).all() && point.cmple(*high).all())
            .map(|(_, _, liquid)| *liquid)
    }
}

#[test]
fn the_liquid_probe_finds_a_wade_a_ledge_and_lava() {
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
    let liquids = || {
        Boxes(vec![
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
        ])
    };
    let world = CollisionWorld::new(triangles)
        .unwrap()
        .with_liquids(liquids());
    let bounds = (Vec3::new(-40.0, -10.0, -100.0), Vec3::new(60.0, 10.0, 60.0));
    let spots = best_spots(liquid_spots(&world, &liquids(), bounds, (4.0, 6.0)), 3);
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

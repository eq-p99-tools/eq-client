//! Opt-in local geometry probes; require user-owned assets and explicit coordinates.
use super::*;

#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_POSITION and EQ_PROBE_MODEL"]
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
    let offset = rendered
        .map(|ground| position.y - ground)
        .filter(|offset| *offset >= -0.5 && *offset <= height * 2.0)
        .unwrap_or(height * 0.5);
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

//! Synthetic WLD archives exercise the real asset transform and selection adapter.
use super::*;
use std::io::Cursor;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// An upward floor with coincident opposite winding, an actual upper platform,
/// and a downward ceiling. Deliberately misleading shading normals must not
/// decide which side of the loaded geometry can nominate a selection floor.
fn world() -> Vec<u8> {
    squares(&[
        (0, 128, true, true),
        (32, 128, true, false),
        (64, 128, false, false),
    ])
}

/// A WLD of level squares centred on the origin: each its height, half its
/// side, whether it faces up and whether it faces both ways.
fn squares(squares: &[(i16, i16, bool, bool)]) -> Vec<u8> {
    let mut positions = Vec::<[i16; 3]>::new();
    let mut faces = Vec::<[u16; 3]>::new();
    for &(height, half, upward, both) in squares {
        let base = u16::try_from(positions.len()).unwrap();
        positions.extend([
            [-half, -half, height],
            [half, -half, height],
            [half, half, height],
            [-half, half, height],
        ]);
        let up = [[base, base + 2, base + 1], [base, base + 3, base + 2]];
        let down = [[base, base + 1, base + 2], [base, base + 2, base + 3]];
        faces.extend(if upward { up } else { down });
        if both {
            faces.extend(down);
        }
    }
    let count = |value: usize| u16::try_from(value).unwrap();
    let mut mesh = words(&[0, 0x0001_8003, 2, 0, 0, 0]);
    // Center, unknowns, radius and minimum/maximum, then the ten mesh counts.
    mesh.extend([0_u8; 12 + 12 + 4 + 24]);
    for value in [
        count(positions.len()),
        count(positions.len()),
        count(positions.len()),
        0,
        count(faces.len()),
        0,
        1,
        1,
        0,
        0,
    ] {
        mesh.extend(value.to_le_bytes());
    }
    for position in &positions {
        mesh.extend(position.iter().flat_map(|value| value.to_le_bytes()));
    }
    mesh.extend([0_u8; 4].repeat(positions.len()));
    mesh.extend([0_u8, 0, 129].repeat(positions.len())); // signed -127 Z
    for face in &faces {
        mesh.extend(0_u16.to_le_bytes());
        mesh.extend(face.iter().flat_map(|value| value.to_le_bytes()));
    }
    for value in [count(faces.len()), 0, count(positions.len()), 0] {
        mesh.extend(value.to_le_bytes());
    }
    let mut material = words(&[0, 0, 0x8000_0001, 0]);
    material.extend(1.0_f32.to_le_bytes());
    material.extend(1.0_f32.to_le_bytes());
    material.extend(0_i32.to_le_bytes());
    let fragments = [
        (0x30_u32, material),
        (0x31, words(&[0, 0, 1, 1])),
        (0x36, mesh),
    ];
    let largest = fragments.iter().map(|(_, body)| body.len()).max().unwrap();
    let mut bytes = words(&[
        0x5450_3d02,
        0x0001_5500,
        3,
        0,
        u32::try_from(largest).unwrap(),
        4,
        1,
    ]);
    bytes.extend([0x95, 0, 0, 0]); // encoded empty string and padding
    for (kind, mut body) in fragments {
        body.resize(body.len().div_ceil(4) * 4, 0);
        bytes.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(body);
    }
    bytes.extend([0xff; 4]);
    bytes
}

/// The zone a WLD makes, loaded through the real asset loader.
fn load(name: &str, wld: &[u8]) -> ZoneAsset {
    let directory =
        std::env::temp_dir().join(format!("eq-selection-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut archive = libeq::pfs::PfsWriter::create(Cursor::new(Vec::new())).unwrap();
    archive.insert("test.wld", wld).unwrap();
    std::fs::write(
        directory.join("test.s3d"),
        archive.finish().unwrap().into_inner(),
    )
    .unwrap();
    let loaded = eq_client_assets::load_zone(&directory, "test");
    std::fs::remove_dir_all(directory).unwrap();
    loaded.unwrap()
}

/// A spot at the server's `x`, `y` and `z`.
fn spot_at(x: f32, y: f32, z: f32) -> eq_client_core::select_spots::Spot {
    eq_client_core::select_spots::Spot {
        loc: [y, x, z],
        heading: 0.0,
        distance: 25.0,
        pitch: 10.0,
        reviewed: false,
    }
}

#[test]
fn a_spot_on_a_top_no_walk_reaches_is_flagged_and_one_on_the_floor_passes() {
    // A wide floor, and a small flat top 32 above its middle with no way up:
    // dropping off the top reaches the floor, but nothing on the floor
    // reaches the top.
    let zone = load(
        "reach",
        &squares(&[(0, 128, true, false), (32, 16, true, false)]),
    );
    let mesh = select_spot_mesh(None, "test", &zone).unwrap();
    let main = select_spot_main_island(&zone, &mesh).unwrap();
    assert!(main.dry && main.clear(), "{main:?}");
    let (on_floor, _) = check_select_spot(&zone, &spot_at(80.0, 80.0, 0.0), &mesh);
    assert_eq!(on_floor, SpotVerdict::Reachable);
    // The preview stands this one on the top, the first floor at or below
    // z + 5.
    let (on_top, _) = check_select_spot(&zone, &spot_at(0.0, 0.0, 33.0), &mesh);
    let SpotVerdict::Unreachable { z, island_area } = on_top else {
        panic!("{on_top:?}");
    };
    assert!((z - 32.0).abs() < 0.01, "{z}");
    assert!(
        island_area.is_some_and(|area| area < main.area),
        "{island_area:?}"
    );
    // Beside the floor's edge nothing stands the character.
    let (off, _) = check_select_spot(&zone, &spot_at(400.0, 0.0, 0.0), &mesh);
    assert_eq!(off, SpotVerdict::NoFloor);
    // A kept row excuses an unreachable floor, never a missing one.
    assert!(on_top.flagged(false) && !on_top.flagged(true));
    assert!(off.flagged(false) && off.flagged(true));
    assert!(!on_floor.flagged(false));
    assert!(!SpotVerdict::NoMainIsland.flagged(false));
    // The proposal stands on the floor a walk reaches.
    let spot = propose_select_spot(&zone, &mesh).unwrap();
    assert!(spot.loc[2].abs() < 0.01, "{spot:?}");
}

#[test]
fn nomination_follows_loaded_wld_winding_without_filtering_placement_or_collision() {
    let zone = load("winding", &world());
    // These explicit coordinates verify the WLD (x,y,z) -> render (x,z,-y)
    // conversion actually ran through the loader rather than a test-only path.
    assert!(
        zone.primitives
            .iter()
            .any(|part| part.positions.contains(&[-128.0, 32.0, 128.0]))
    );
    let all = TerrainSurface::from_primitives(&zone.primitives);
    assert_eq!(all.0.len(), 8);
    assert_eq!(zone.collision.len(), 8);
    let nominations = upward_triangles(&zone.primitives);
    assert_eq!(
        nominations.len(),
        4,
        "one upward side of the paired floor and the genuine platform"
    );
    assert!(nominations.iter().all(|triangle| {
        [0.0_f32, 32.0]
            .iter()
            .any(|height| (triangle[0][1] - height).abs() < 0.001)
    }));
    let floors = eq_client_core::movement::CollisionWorld::new(nominations).unwrap();
    assert!(floors.ground(Vec3::new(0.0, 32.0, 0.0), 0.1, 0.1).is_some());
    assert!(floors.ground(Vec3::new(0.0, 64.0, 0.0), 0.1, 0.1).is_none());
    // The downward ceiling remains in the preview's first-floor rule and
    // the solid collision/headroom world; only nomination was filtered.
    assert_eq!(all.height_below(0.0, 0.0, 69.0), Some(64.0));
    assert!(
        zone_collision(&zone)
            .unwrap()
            .ground(Vec3::new(0.0, 64.0, 0.0), 0.1, 0.1)
            .is_some()
    );
    let mesh = select_spot_mesh(None, "test", &zone).unwrap();
    let spot = propose_select_spot(&zone, &mesh).unwrap();
    assert!(
        [0.0_f32, 32.0]
            .iter()
            .any(|height| (spot.loc[2] - height).abs() < 0.001),
        "never nominate the downward ceiling: {spot:?}"
    );
    assert_eq!(TerrainSurface::from_primitives(&zone.primitives).0.len(), 8);
    assert_eq!(zone.collision.len(), 8);
}

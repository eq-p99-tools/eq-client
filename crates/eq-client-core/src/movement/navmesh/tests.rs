use super::cache::{self, Source};
use super::*;
use crate::movement::MAX_GROUNDED_STEP;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;

fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
    [[a, b, c], [a, c, d]]
}

/// A square floor at height `y`, facing up.
fn floor(y: f32, low: f32, high: f32) -> Vec<[[f32; 3]; 3]> {
    quad(
        [low, y, low],
        [low, y, high],
        [high, y, high],
        [high, y, low],
    )
    .to_vec()
}

/// A solid box's top and sides, facing out.
fn block(low: [f32; 3], high: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
    let [x0, y0, z0] = low;
    let [x1, y1, z1] = high;
    [
        quad([x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]),
        quad([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]),
        quad([x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1]),
        quad([x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]),
        quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]),
    ]
    .concat()
}

/// The walker's settings at a coarser cell and smaller tile, so a test
/// builds in milliseconds and still spans several tiles. The tile is a whole
/// number of cells, so tiles start every 16 units.
fn settings() -> Settings {
    Settings {
        cell_size: 0.25,
        tile: 16.0,
        ..Settings::walker(6.0)
    }
}

fn build(triangles: &[[[f32; 3]; 3]]) -> (NavMesh, BuildReport) {
    NavMesh::build(triangles, &settings(), 2, &AtomicBool::new(false)).unwrap()
}

/// Whether the floor under `a` and the floor under `b` are joined.
fn joined(mesh: &NavMesh, a: Vec3, b: Vec3) -> bool {
    let (Some((a, _)), Some((b, _))) = (mesh.locate(a), mesh.locate(b)) else {
        panic!("{a} or {b} is off the mesh");
    };
    mesh.connected(a, b)
}

/// A folder of its own under the system's temporary folder.
fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("eq-navmesh-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

/// A path's length from `start` through its points.
fn routes_length(start: Vec3, path: &[Vec3]) -> f32 {
    std::iter::once(start)
        .chain(path.iter().copied())
        .collect::<Vec<_>>()
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

fn files(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder.join("navmesh"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn open_floor_across_several_tiles_is_one_island() {
    // Four tiles by four.
    let (mesh, report) = build(&floor(0.0, -32.0, 32.0));
    assert_eq!(report.tiles, 16, "{report:?}");
    assert_eq!(report.tiles_with_polygons, report.tiles);
    assert!(report.refused.is_empty(), "{report:?}");
    assert_eq!(mesh.island_count(), 1);
    let (_, height) = mesh.locate(Vec3::new(-30.0, 0.5, 30.0)).unwrap();
    assert!(height.abs() < 0.3, "{height}");
    assert!(mesh.locate(Vec3::new(60.0, 0.0, 0.0)).is_none());
}

#[test]
fn stacked_floors_are_told_apart() {
    let mut triangles = floor(0.0, -20.0, 20.0);
    triangles.extend(floor(12.0, -10.0, 10.0));
    let (mesh, _) = build(&triangles);
    let (low, low_y) = mesh.locate(Vec3::new(0.0, 0.0, 0.0)).unwrap();
    let (high, high_y) = mesh.locate(Vec3::new(0.0, 12.0, 0.0)).unwrap();
    assert_ne!(low, high);
    assert!(
        low_y.abs() < 0.3 && (high_y - 12.0).abs() < 0.3,
        "{low_y} {high_y}"
    );
    assert!(!mesh.connected(low, high));
}

#[test]
fn tiles_far_apart_in_height_build_each_over_its_own_heights() {
    // Two floors 10,000 apart, a tile and more apart: the zone is taller
    // than a heightfield can hold, each tile isn't.
    let mut triangles = quad(
        [-32.0, 0.0, -8.0],
        [-32.0, 0.0, 8.0],
        [-8.0, 0.0, 8.0],
        [-8.0, 0.0, -8.0],
    )
    .to_vec();
    triangles.extend(quad(
        [8.0, 10_000.0, -8.0],
        [8.0, 10_000.0, 8.0],
        [32.0, 10_000.0, 8.0],
        [32.0, 10_000.0, -8.0],
    ));
    let (mesh, report) = build(&triangles);
    assert!(report.refused.is_empty(), "{report:?}");
    assert_eq!(report.deep_triangles, 0);
    assert!(mesh.locate(Vec3::new(-20.0, 0.0, 0.0)).is_some());
    assert!(mesh.locate(Vec3::new(20.0, 10_000.0, 0.0)).is_some());
}

#[test]
fn a_tile_taller_than_a_heightfield_leaves_out_what_lies_far_under_it() {
    // A floor, and a block 40,000 under it in one tile: the floor builds
    // in every tile, and the block's triangles are counted, not meshed.
    let mut triangles = floor(0.0, -32.0, 32.0);
    let deep = block([-28.0, -40_001.0, -28.0], [-26.0, -40_000.0, -26.0]);
    let deep_count = deep.len();
    triangles.extend(deep);
    let (mesh, report) = build(&triangles);
    assert!(report.refused.is_empty(), "{report:?}");
    assert_eq!(report.tiles_with_polygons, report.tiles);
    assert_eq!(report.deep_triangles, deep_count);
    assert_eq!(mesh.island_count(), 1);
    assert!(mesh.locate(Vec3::new(-27.0, 0.0, -27.0)).is_some());
    assert!(mesh.locate(Vec3::new(-27.0, -40_000.0, -27.0)).is_none());
}

#[test]
fn a_tile_s_own_heights_keep_the_mesh_a_zone_wide_heightfield_gives() {
    // A tile's bottom stays on the zone's cell grid, so a floor high over
    // the zone's lowest point builds where it did: one tile's floor at 0
    // and another's at 37.03, off the cell grid.
    let mut triangles = floor(0.0, -32.0, -16.0);
    triangles.extend(floor(37.03, 16.0, 32.0));
    let (mesh, report) = build(&triangles);
    let (_, high) = mesh.locate(Vec3::new(24.0, 37.03, 24.0)).unwrap();
    let alone = build(&floor(37.03, 16.0, 32.0)).0;
    let (_, high_alone) = alone.locate(Vec3::new(24.0, 37.03, 24.0)).unwrap();
    assert!(report.refused.is_empty(), "{report:?}");
    assert!((high - 37.03).abs() < 0.3, "{high}");
    assert!((high - high_alone).abs() < 0.3, "{high} {high_alone}");
}

#[test]
fn a_far_pair_can_be_kept_to_the_floor_a_test_takes() {
    let (mesh, _) = build(&floor(0.0, -32.0, 32.0));
    let (a, b) = mesh.far_pair_where(|at| at.x < 0.0 && at.z < 0.0).unwrap();
    assert!(a.x < 0.0 && a.z < 0.0 && b.x < 0.0 && b.z < 0.0, "{a} {b}");
    assert!(a.distance(b) > 30.0, "{a} {b}");
    assert!(mesh.far_pair_where(|_| false).is_none());
}

/// `triangles`' world with these boxes of water in it.
fn with_water(triangles: &[[[f32; 3]; 3]], water: &[(Vec3, Vec3)]) -> CollisionWorld {
    let boxes = water
        .iter()
        .map(|&(low, high)| (low, high, crate::movement::Liquid::Water))
        .collect();
    CollisionWorld::new(triangles.iter().copied())
        .unwrap()
        .with_liquids(crate::movement::liquids::Boxes(boxes))
}

#[test]
fn floor_under_water_too_deep_to_stand_in_has_no_mesh() {
    // Water over the floor's high-x half closes over a 6-tall walker's
    // head; a strip of water by its middle is shallower than that.
    let triangles = floor(0.0, -32.0, 32.0);
    let world = with_water(
        &triangles,
        &[
            (Vec3::new(4.0, -1.0, -40.0), Vec3::new(40.0, 10.0, 40.0)),
            (Vec3::new(-4.0, -1.0, -40.0), Vec3::new(4.0, 3.0, 40.0)),
        ],
    );
    let (mesh, report) = NavMesh::build_in(
        &triangles,
        Some(&world),
        &settings(),
        2,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(report.refused.is_empty(), "{report:?}");
    assert!(mesh.locate(Vec3::new(-20.0, 0.0, 0.0)).is_some());
    assert!(
        mesh.locate(Vec3::new(0.0, 0.0, 0.0)).is_some(),
        "the shallows"
    );
    assert!(mesh.locate(Vec3::new(20.0, 0.0, 0.0)).is_none(), "the deep");
    assert!(
        mesh.locate(Vec3::new(30.0, 0.0, 30.0)).is_none(),
        "the deep"
    );
}

#[test]
fn floor_under_water_the_tallest_size_can_wade_keeps_its_mesh() {
    // Water 7 deep closes over the two shorter sizes' heads, not the
    // tallest's: the floor stays, and each walker's own check decides.
    let triangles = floor(0.0, -32.0, 32.0);
    let world = with_water(
        &triangles,
        &[(Vec3::new(-40.0, -1.0, -40.0), Vec3::new(40.0, 7.0, 40.0))],
    );
    let sized = Settings {
        cell_size: 0.25,
        tile: 16.0,
        ..Settings::sized([4.0, 6.0, 9.0])
    };
    let build = |world: Option<&CollisionWorld>| {
        NavMesh::build_in(&triangles, world, &sized, 2, &AtomicBool::new(false))
            .unwrap()
            .0
    };
    let (wet, dry) = (build(Some(&world)), build(None));
    assert_eq!(wet.polygon_count(), dry.polygon_count());
    assert!((wet.floor_area(2) - dry.floor_area(2)).abs() < 1.0);
}

#[test]
fn a_far_pair_end_lies_on_its_polygon_even_where_the_polygon_bends() {
    // An L whose corners' average lies outside it, in the corner it bends
    // round.
    let vertices = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 1.0],
        [1.0, 1.0],
        [1.0, 10.0],
        [0.0, 10.0],
    ]
    .map(|[x, z]| Vec3::new(x, 0.0, z))
    .to_vec();
    let mesh = NavMesh::from_parts(
        vec![Polygon {
            vertices,
            links: Vec::new(),
            size: 0,
        }],
        Vec::new(),
    );
    let (a, b) = mesh.far_pair().unwrap();
    assert!(
        mesh.locate(a).is_some() && mesh.locate(b).is_some(),
        "{a} {b}"
    );
}

#[test]
fn stairs_join_at_the_riser_limit_and_not_past_it() {
    // Risers a little under and a little over the highest step, each flight
    // up to a landing on its own side of the floor.
    for (riser, joins) in [
        (MAX_GROUNDED_STEP - 0.2, true),
        (MAX_GROUNDED_STEP + 0.5, false),
    ] {
        let mut triangles = floor(0.0, -40.0, 40.0);
        for step in 1..=3 {
            #[allow(clippy::cast_precision_loss)]
            let (x, y) = ((step - 1) as f32 * 3.0, step as f32 * riser);
            triangles.extend(block([x, 0.0, -5.0], [x + 3.0, y, 5.0]));
        }
        let top = 4.0 * riser;
        triangles.extend(block([9.0, 0.0, -5.0], [30.0, top, 5.0]));
        let (mesh, _) = build(&triangles);
        assert_eq!(
            joined(&mesh, Vec3::new(-20.0, 0.0, 0.0), Vec3::new(25.0, top, 0.0)),
            joins,
            "riser {riser}"
        );
    }
}

#[test]
fn a_ramp_joins_below_the_floor_limit_and_not_above_it() {
    // A platform ten high whose one way up is a ramp along +x: 30 and 63
    // degrees, against the floor limit's 45.6.
    for (run, joins) in [(17.3, true), (5.0, false)] {
        let mut triangles = floor(0.0, -40.0, 40.0);
        triangles.extend(block([0.0, 0.0, -5.0], [30.0, 10.0, 5.0]));
        triangles.extend(quad(
            [-run, 0.0, -4.0],
            [-run, 0.0, 4.0],
            [0.0, 10.0, 4.0],
            [0.0, 10.0, -4.0],
        ));
        let (mesh, _) = build(&triangles);
        assert_eq!(
            joined(
                &mesh,
                Vec3::new(-30.0, 0.0, 20.0),
                Vec3::new(20.0, 10.0, 0.0)
            ),
            joins,
            "run {run}"
        );
    }
}

#[test]
fn a_doorway_narrower_than_the_walker_is_closed() {
    // A wall across z = 0, ten high, with a gap at x = 0.
    for (gap, open) in [(0.6, false), (4.0, true)] {
        let mut triangles = floor(0.0, -30.0, 30.0);
        triangles.extend(block([-30.0, 0.0, -0.5], [-gap / 2.0, 10.0, 0.5]));
        triangles.extend(block([gap / 2.0, 0.0, -0.5], [30.0, 10.0, 0.5]));
        let (mesh, _) = build(&triangles);
        assert_eq!(
            joined(&mesh, Vec3::new(0.0, 0.0, -15.0), Vec3::new(0.0, 0.0, 15.0)),
            open,
            "gap {gap}"
        );
    }
}

#[test]
fn tiles_join_across_their_seam() {
    let (mesh, _) = build(&floor(0.0, -32.0, 32.0));
    // Tiles start at the floor's low corner, so x = -32 + 16 is a seam.
    let seam = -32.0 + settings().tile;
    let (west, _) = mesh.locate(Vec3::new(seam - 1.0, 0.0, 0.0)).unwrap();
    let (east, _) = mesh.locate(Vec3::new(seam + 1.0, 0.0, 0.0)).unwrap();
    assert_ne!(west, east);
    assert!(mesh.connected(west, east));
    let across = mesh
        .polygons
        .iter()
        .flat_map(|p| &p.links)
        .filter(|link| (link.a.x - seam).abs() < 0.01 && (link.b.x - seam).abs() < 0.01)
        .count();
    assert!(across > 0);
}

#[test]
fn the_same_mesh_comes_from_one_thread_or_four() {
    let mut triangles = floor(0.0, -40.0, 40.0);
    triangles.extend(block([-5.0, 0.0, -5.0], [5.0, 10.0, 5.0]));
    let built = |threads| {
        NavMesh::build(&triangles, &settings(), threads, &AtomicBool::new(false)).unwrap()
    };
    let ((one, one_report), (four, four_report)) = (built(1), built(4));
    assert_eq!(one.polygons, four.polygons);
    assert_eq!(one_report.threads.len(), 1);
    assert_eq!(four_report.threads.len(), 4);
    let tiles = |report: &BuildReport| report.threads.iter().map(|t| t.0).sum::<usize>();
    assert_eq!(tiles(&one_report), one_report.tiles);
    assert_eq!(tiles(&four_report), four_report.tiles);
}

#[test]
fn a_mesh_reads_back_from_the_cache() {
    let folder = folder("read-back");
    let triangles = floor(0.0, -40.0, 40.0);
    let cancel = AtomicBool::new(false);
    let built = cache::load_or_build(
        Some(&folder),
        "zone",
        &triangles,
        None,
        &settings(),
        2,
        &cancel,
    )
    .unwrap();
    assert!(matches!(built.source, Source::Built(_)));
    let bytes = built.cached.unwrap();
    let read = cache::load_or_build(
        Some(&folder),
        "zone",
        &triangles,
        None,
        &settings(),
        2,
        &cancel,
    )
    .unwrap();
    assert!(matches!(read.source, Source::Cache));
    assert_eq!(read.cached.unwrap(), bytes);
    assert_eq!(read.mesh.polygons, built.mesh.polygons);
    assert_eq!(read.mesh.island, built.mesh.island);
    assert_eq!(read.mesh.island_count(), built.mesh.island_count());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_changed_setting_or_triangle_changes_the_key() {
    let triangles = floor(0.0, -40.0, 40.0);
    let key = cache::key(&triangles, &settings());
    assert_eq!(cache::key(&triangles, &settings()), key);
    let taller = Settings {
        height: 6.5,
        ..settings()
    };
    assert_ne!(cache::key(&triangles, &taller), key);
    let mut moved = triangles.clone();
    moved[1][2][1] = 0.01;
    assert_ne!(cache::key(&moved, &settings()), key);
}

#[test]
fn a_file_with_another_header_or_cut_short_is_rebuilt() {
    let folder = folder("rejected");
    let triangles = floor(0.0, -40.0, 40.0);
    let cancel = AtomicBool::new(false);
    let built = cache::load_or_build(
        Some(&folder),
        "zone",
        &triangles,
        None,
        &settings(),
        2,
        &cancel,
    )
    .unwrap();
    let path = cache::path(&folder, "zone", cache::key(&triangles, &settings())).unwrap();
    let good = std::fs::read(&path).unwrap();
    assert_eq!(good.len(), built.cached.unwrap());
    // Another format version in the header, then the body cut short.
    let mut other = good.clone();
    other[4] ^= 1;
    for bad in [other, good[..good.len() - 3].to_vec()] {
        std::fs::write(&path, bad).unwrap();
        assert!(cache::load(&folder, "zone", &triangles, &settings()).is_none());
        let again = cache::load_or_build(
            Some(&folder),
            "zone",
            &triangles,
            None,
            &settings(),
            2,
            &cancel,
        )
        .unwrap();
        assert!(matches!(again.source, Source::Built(_)));
        // The rebuilt mesh is stored over the rejected file.
        assert_eq!(again.cached.unwrap(), good.len());
        assert_eq!(std::fs::read(&path).unwrap(), good);
    }
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_new_key_removes_the_zone_s_stale_file() {
    let folder = folder("stale");
    let cancel = AtomicBool::new(false);
    let old = floor(0.0, -20.0, 20.0);
    let new = floor(1.0, -20.0, 20.0);
    // A temp file a crash left under an old key.
    std::fs::create_dir_all(folder.join("navmesh")).unwrap();
    std::fs::write(
        folder.join("navmesh").join("zone-0000000000000001.tmp"),
        b"half",
    )
    .unwrap();
    for (zone, triangles) in [("zone", &old), ("zone2", &old), ("zone", &new)] {
        let built = cache::load_or_build(
            Some(&folder),
            zone,
            triangles,
            None,
            &settings(),
            2,
            &cancel,
        )
        .unwrap();
        built.cached.unwrap();
    }
    let name = |zone: &str, triangles: &[[[f32; 3]; 3]]| {
        format!("{zone}-{:016x}.bin", cache::key(triangles, &settings()))
    };
    // The other zone's file stays; nothing half-written is left beside them.
    let mut expected = vec![name("zone", &new), name("zone2", &old)];
    expected.sort();
    assert_eq!(files(&folder), expected);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_build_cancelled_part_way_keeps_no_mesh_and_no_file() {
    let folder = folder("cancelled");
    let triangles = floor(0.0, -40.0, 40.0);
    // Cancelled once a few tiles are built.
    let asked = AtomicUsize::new(0);
    let cancelled = || asked.fetch_add(1, Ordering::Relaxed) >= 3;
    let loaded = cache::load_or_build_until(
        Some(&folder),
        "zone",
        &triangles,
        None,
        &settings(),
        1,
        &cancelled,
    );
    assert!(loaded.is_none());
    assert!(asked.load(Ordering::Relaxed) > 3);
    assert!(files(&folder).is_empty(), "{:?}", files(&folder));
    // Cancelled before the first tile.
    assert!(NavMesh::build(&triangles, &settings(), 4, &AtomicBool::new(true)).is_none());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_tile_that_panics_is_refused_and_the_rest_are_built() {
    let triangles = floor(0.0, -32.0, 32.0);
    // The first tile built panics; one thread, so that is tile (0, 0).
    let panicked = AtomicBool::new(false);
    let panicking = |triangles: &[[[f32; 3]; 3]],
                     indices: &[u32],
                     min: Vec3,
                     max: Vec3,
                     border: u16,
                     settings: &Settings| {
        assert!(panicked.swap(true, Ordering::Relaxed), "a tile that panics");
        build::build_tile(triangles, indices, min, max, border, settings, None)
    };
    let (polygons, report) =
        build::polygons_with(&triangles, &settings(), 1, &|| false, &panicking).unwrap();
    assert_eq!(
        report.refused,
        [RefusedTile {
            tile: (0, 0),
            step: "panic",
            reason: "a tile that panics".to_owned(),
        }]
    );
    assert_eq!(report.tiles_with_polygons, report.tiles - 1);
    let mesh = NavMesh::from_parts(polygons, Vec::new());
    assert!(mesh.locate(Vec3::new(-28.0, 0.0, -28.0)).is_none());
    assert!(mesh.locate(Vec3::new(28.0, 0.0, 28.0)).is_some());
}

#[test]
fn a_zone_name_that_is_not_a_plain_file_name_is_not_kept() {
    assert!(cache::path(Path::new("."), "../zone", 1).is_none());
    assert!(cache::path(Path::new("."), "", 1).is_none());
    assert!(cache::path(Path::new("."), "zone_2", 1).is_some());
}

/// Build time and each thread's share on a synthetic town (a 1,000-unit
/// floor of 4-unit quads with boxes for buildings), at the walker's own
/// settings; not a zone.
#[test]
#[ignore = "timing only; run with --release --ignored --nocapture"]
fn inspect_synthetic_build_balance() {
    let mut triangles = Vec::new();
    for i in 0..250u8 {
        for j in 0..250u8 {
            let (x, z) = (f32::from(i) * 4.0 - 500.0, f32::from(j) * 4.0 - 500.0);
            triangles.extend(quad(
                [x, 0.0, z],
                [x, 0.0, z + 4.0],
                [x + 4.0, 0.0, z + 4.0],
                [x + 4.0, 0.0, z],
            ));
        }
    }
    // Buildings on one quarter only, so tiles differ in cost.
    for i in 0..10u8 {
        for j in 0..10u8 {
            let (x, z) = (f32::from(i) * 40.0 - 480.0, f32::from(j) * 40.0 - 480.0);
            triangles.extend(block([x, 0.0, z], [x + 20.0, 15.0, z + 20.0]));
        }
    }
    for threads in [1, 4, build_threads()] {
        let (mesh, report) = NavMesh::build(
            &triangles,
            &Settings::walker(6.0),
            threads,
            &AtomicBool::new(false),
        )
        .unwrap();
        println!(
            "threads={threads} seconds={:.2} tiles={} polygons={} islands={} refused={:?}",
            report.elapsed.as_secs_f32(),
            report.tiles,
            mesh.polygon_count(),
            mesh.island_count(),
            report.refusals_by_reason(),
        );
        for (n, (tiles, busy)) in report.threads.iter().enumerate() {
            println!(
                "  thread={n} tiles={tiles} busy_seconds={:.2}",
                busy.as_secs_f32()
            );
        }
    }
}

mod routes {
    //! Routes over the mesh, each leg judged by the walker's edge tests.

    use super::{block, floor, settings};
    use crate::movement::navmesh::{MeshNavigator, NavMesh};
    use crate::movement::path::{CLEARANCE, DOOR_TOLL};
    use crate::movement::{
        CollisionMesh, CollisionWorld, Fallback, Navigator, PathProgress, Planned, Refusals,
        RouteSearch, Scope, SearchRequest,
    };
    use glam::{Vec2, Vec3};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    /// The world over `triangles`, and the mesh over the same triangles.
    fn zone(triangles: &[[[f32; 3]; 3]]) -> (CollisionWorld, MeshNavigator) {
        let (mesh, _) = NavMesh::build(triangles, &settings(), 2, &AtomicBool::new(false)).unwrap();
        (
            CollisionWorld::new(triangles.to_vec()).unwrap(),
            MeshNavigator::new(Arc::new(mesh)),
        )
    }

    fn request(start: Vec3, goal: Vec3, doors: Vec<u8>) -> SearchRequest {
        SearchRequest {
            start,
            goal,
            reach: 1.0,
            height: 6.0,
            doors,
            drops: false,
            scope: Scope::Whole,
        }
    }

    /// Runs `search` to its end, `budget` at a time; what it found, and the
    /// most work any one call did.
    fn run(
        search: &mut dyn RouteSearch,
        world: &CollisionWorld,
        budget: usize,
    ) -> (PathProgress, usize) {
        let mut most = 0;
        for _ in 0..100_000 {
            let before = search.searched().0;
            let progress = search.advance(world, budget);
            most = most.max(search.searched().0 - before);
            if progress != PathProgress::Searching {
                return (progress, most);
            }
        }
        panic!("the search never ended");
    }

    fn found(progress: PathProgress) -> Vec<Vec3> {
        match progress {
            PathProgress::Found(path) => path,
            other => panic!("{other:?}"),
        }
    }

    fn length(start: Vec3, path: &[Vec3]) -> f32 {
        std::iter::once(start)
            .chain(path.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum()
    }

    fn mesh_planned(search: &dyn RouteSearch) -> (usize, usize, Option<Fallback>) {
        match search.planned() {
            Planned::Mesh {
                legs,
                repaired,
                fallback,
                ..
            } => (legs, repaired.len(), fallback),
            Planned::Grid => panic!("the grid planned"),
        }
    }

    /// A wall across z = 0, ten high and a unit thick, from x = -40 to x = 20.
    fn wall_to(end: f32) -> Vec<[[f32; 3]; 3]> {
        block([-40.0, 0.0, -0.5], [end, 10.0, 0.5])
    }

    #[test]
    fn a_route_goes_straight_over_open_floor() {
        let (world, mesh) = zone(&floor(0.0, -40.0, 40.0));
        let (start, goal) = (Vec3::new(-30.0, 0.0, -30.0), Vec3::new(30.0, 0.0, 30.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        assert!(
            length(start, &path) <= start.distance(goal) * 1.01,
            "{path:?}"
        );
        assert!(path.last().unwrap().distance(goal) < 0.3, "{path:?}");
        assert_eq!(mesh_planned(&*search), (1, 0, None));
    }

    #[test]
    fn a_route_turns_at_a_wall_s_end_with_room_to_spare() {
        let mut triangles = floor(0.0, -40.0, 40.0);
        triangles.extend(wall_to(20.0));
        let (world, mesh) = zone(&triangles);
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 20.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        // Round the wall's end, every leg a clearance clear of its corners
        // (a little less where the funnel cuts between two insets).
        assert!(path.iter().any(|p| p.x > 20.0), "round the end: {path:?}");
        let flat = |v: Vec3| Vec2::new(v.x, v.z);
        let legs: Vec<Vec3> = std::iter::once(start).chain(path.iter().copied()).collect();
        for corner in [Vec2::new(20.0, -0.5), Vec2::new(20.0, 0.5)] {
            let nearest = legs
                .windows(2)
                .map(|pair| {
                    let (a, b) = (flat(pair[0]), flat(pair[1]));
                    let t = ((corner - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                    corner.distance(a.lerp(b, t))
                })
                .fold(f32::MAX, f32::min);
            assert!(
                nearest >= CLEARANCE - 0.25,
                "{nearest} from {corner}: {path:?}"
            );
        }
        let (legs, repaired, fallback) = mesh_planned(&*search);
        assert!(
            legs >= 2 && repaired == 0 && fallback.is_none(),
            "{legs} {repaired} {fallback:?}"
        );
    }

    #[test]
    fn a_doorway_is_crossed_through_its_middle_however_wide() {
        // Doorways of 8 and of 3 at x = 0 in a wall across z = 0; 3 is
        // narrower than twice the clearance.
        for gap in [8.0_f32, 3.0] {
            let mut triangles = floor(0.0, -30.0, 30.0);
            triangles.extend(block([-30.0, 0.0, -0.5], [-gap / 2.0, 10.0, 0.5]));
            triangles.extend(block([gap / 2.0, 0.0, -0.5], [30.0, 10.0, 0.5]));
            let (world, mesh) = zone(&triangles);
            let (start, goal) = (Vec3::new(-10.0, 0.0, -15.0), Vec3::new(10.0, 0.0, 15.0));
            let mut search = mesh.search(request(start, goal, Vec::new()));
            let path = found(run(&mut *search, &world, 256).0);
            // Where the path crosses the wall's line.
            let mut previous = start;
            let crossed = path
                .iter()
                .find_map(|&p| {
                    let at = (previous.z < 0.0 && p.z >= 0.0)
                        .then(|| previous.lerp(p, -previous.z / (p.z - previous.z)).x);
                    previous = p;
                    at
                })
                .expect("through the doorway");
            let room = (gap / 2.0 - CLEARANCE).max(0.0) + 0.5;
            assert!(
                crossed.abs() <= room,
                "gap {gap}: crossed at x = {crossed}, {path:?}"
            );
            assert_eq!(mesh_planned(&*search).2, None, "gap {gap}");
        }
    }

    /// A room 60 across with a wall across z = 0 and a doorway at
    /// 10 < x < 14 shut by door 7, the doorway the one way through.
    fn walled_room() -> (CollisionWorld, MeshNavigator) {
        let mut triangles = floor(0.0, -30.0, 30.0);
        triangles.extend(block([-30.0, 0.0, -0.5], [10.0, 10.0, 0.5]));
        triangles.extend(block([14.0, 0.0, -0.5], [30.0, 10.0, 0.5]));
        let (mut world, mesh) = zone(&triangles);
        let door = CollisionMesh::new(block([10.0, 0.0, -0.25], [14.0, 10.0, 0.25])).unwrap();
        world.set_door_obstacles(vec![(7, Arc::new(door))]);
        (world, mesh)
    }

    #[test]
    fn a_door_that_can_t_be_opened_closes_only_its_doorway() {
        let (world, mesh) = walled_room();
        // Across the room beside the door: routed on the mesh, past the door.
        let (start, goal) = (Vec3::new(-20.0, 0.0, -5.0), Vec3::new(25.0, 0.0, -3.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        assert!(
            length(start, &path) < start.distance(goal) * 1.05,
            "{path:?}"
        );
        assert_eq!(mesh_planned(&*search).2, None);
        // Through the door: no corridor past it, so the grid finds no way.
        let through = request(
            Vec3::new(12.0, 0.0, -10.0),
            Vec3::new(12.0, 0.0, 10.0),
            Vec::new(),
        );
        let mut search = mesh.search(through);
        assert_eq!(run(&mut *search, &world, 256).0, PathProgress::Failed);
        assert_eq!(mesh_planned(&*search).2, Some(Fallback::NoCorridor));
    }

    #[test]
    fn a_door_that_can_be_opened_costs_its_toll_only_through_its_doorway() {
        let (world, mesh) = walled_room();
        // Beside the door, the toll changes nothing.
        let (start, goal) = (Vec3::new(-20.0, 0.0, -5.0), Vec3::new(25.0, 0.0, -3.0));
        let mut search = mesh.search(request(start, goal, vec![7]));
        let path = found(run(&mut *search, &world, 256).0);
        assert!(
            length(start, &path) < start.distance(goal) * 1.05,
            "{path:?}"
        );
        assert!(search.doors_along().iter().all(Option::is_none));
        // Through it: one step crosses the door, from a cell short of it.
        let (start, goal) = (Vec3::new(12.0, 0.0, -10.0), Vec3::new(12.0, 0.0, 10.0));
        let mut search = mesh.search(request(start, goal, vec![7]));
        let path = found(run(&mut *search, &world, 256).0);
        let doors = search.doors_along();
        let crossing = doors
            .iter()
            .position(|door| *door == Some(7))
            .expect("door 7");
        assert_eq!(doors.iter().flatten().count(), 1, "{doors:?}");
        let before = if crossing == 0 {
            start
        } else {
            path[crossing - 1]
        };
        assert!(before.z <= -1.5, "{path:?}");
        assert!(path[crossing].z > 0.0, "{path:?}");
        // A way round as long as the toll loses to the door.
        let mut triangles = floor(0.0, -30.0, 30.0);
        triangles.extend(block([-30.0, 0.0, -0.5], [10.0, 10.0, 0.5]));
        triangles.extend(block([14.0, 0.0, -0.5], [16.0, 10.0, 0.5]));
        triangles.extend(block([22.0, 0.0, -0.5], [30.0, 10.0, 0.5]));
        let (mut world, mesh) = zone(&triangles);
        let door = CollisionMesh::new(block([10.0, 0.0, -0.25], [14.0, 10.0, 0.25])).unwrap();
        world.set_door_obstacles(vec![(7, Arc::new(door))]);
        let mut search = mesh.search(request(start, goal, vec![7]));
        let path = found(run(&mut *search, &world, 256).0);
        let round = length(start, &path);
        // Through the gap at 16 < x < 22 is longer by less than the toll.
        assert!(round - start.distance(goal) < DOOR_TOLL, "{round}");
        assert!(search.doors_along().iter().all(Option::is_none), "{path:?}");
    }

    /// The stand-in for the probe's Thurgadin stretch (EQ x -21..-44 at
    /// y -377): a corner the mesh doesn't know is there, beside a leg it
    /// gives. In the zone the cause was the mesh's voxel-rounded walls; here
    /// it is a post standing in the world but left out of the mesh. Either
    /// way the leg fails the walker's edge tests, and a short grid search
    /// replaces that leg only.
    fn post_beside_the_way() -> (CollisionWorld, MeshNavigator) {
        let mut triangles = floor(0.0, -40.0, 40.0);
        triangles.extend(wall_to(20.0));
        let (mut world, mesh) = zone(&triangles);
        let post = CollisionMesh::new(block([16.5, 0.0, -7.0], [17.5, 10.0, -6.0])).unwrap();
        world.set_obstacles(vec![Arc::new(post)]);
        (world, mesh)
    }

    #[test]
    fn a_leg_the_mesh_gets_wrong_is_repaired_by_a_short_grid_search() {
        let (world, mesh) = post_beside_the_way();
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 20.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        let (legs, repaired, fallback) = mesh_planned(&*search);
        assert_eq!((repaired, fallback), (1, None), "{legs} legs: {path:?}");
        // The repaired route is walkable from end to end.
        let mut feet = start;
        for &next in &path {
            let to = Vec2::new(next.x, next.z);
            feet = crate::movement::path::walk(&world, feet, to, 6.0)
                .unwrap_or_else(|| panic!("{feet} to {next} in {path:?}"));
        }
        assert!(feet.distance(goal) < 1.5, "{feet}");
    }

    /// The most a leg's check costs in the wall's zones: its length in grid
    /// cells, plus one, for legs under 28 long from the start round the
    /// wall's end.
    const ONE_LEG: usize = 15;

    fn mesh_refused(search: &dyn RouteSearch) -> Refusals {
        match search.planned() {
            Planned::Mesh { refused, .. } => refused,
            Planned::Grid => panic!("the grid planned"),
        }
    }

    #[test]
    fn a_leg_ends_on_the_floor_where_the_mesh_lies_under_it() {
        // The mesh's floor 2.5 under the world's, more than a step, as the
        // mesh's surface sinks under a sloped floor; the wall's end makes a
        // corner the route turns at, a mesh point.
        let mut low = floor(0.0, -40.0, 40.0);
        low.extend(wall_to(20.0));
        let (_, mesh) = zone(&low);
        let mut high = floor(2.5, -40.0, 40.0);
        high.extend(block([-40.0, 2.5, -0.5], [20.0, 12.5, 0.5]));
        let world = CollisionWorld::new(high).unwrap();
        let (start, goal) = (Vec3::new(0.0, 2.5, -20.0), Vec3::new(0.0, 2.5, 20.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        let (_, repaired, fallback) = mesh_planned(&*search);
        assert_eq!((repaired, fallback), (0, None), "{path:?}");
        assert_eq!(mesh_refused(&*search), Refusals::default());
        assert!(
            path.iter().all(|feet| (feet.y - 2.5).abs() < 0.01),
            "{path:?}"
        );
    }

    #[test]
    fn a_long_refused_leg_is_cut_and_only_its_refused_piece_repaired() {
        // Open floor the mesh sees clear, with a post in the world alone
        // halfway along the way.
        let (world, mesh) = {
            let (mut world, mesh) = zone(&floor(0.0, -150.0, 150.0));
            let post = CollisionMesh::new(block([-0.5, 0.0, 9.5], [0.5, 10.0, 10.5])).unwrap();
            world.set_obstacles(vec![Arc::new(post)]);
            (world, mesh)
        };
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 0.0, 140.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        let Planned::Mesh {
            legs,
            pieces,
            cut,
            clear,
            drops,
            repaired,
            fallback,
            refused,
            ..
        } = search.planned()
        else {
            panic!("the grid planned");
        };
        assert_eq!((legs, fallback), (1, None), "{path:?}");
        // The leg, 280 across, cut once into five pieces; only the piece by
        // the post refused, and the counts by outcome add up to the pieces.
        assert_eq!((cut, pieces), (1, 5));
        assert_eq!(
            refused,
            Refusals {
                walk: 1,
                ..Refusals::default()
            }
        );
        assert!(
            clear + drops + repaired.len() <= pieces,
            "{clear} {drops} {repaired:?}"
        );
        let [(from, to)] = repaired[..] else {
            panic!("{repaired:?}");
        };
        assert!(Vec2::new(to.x - from.x, to.z - from.z).length() <= 64.0 + 1.0);
        assert!(from.z < 10.0 && to.z > 10.0, "{from} {to}");
        assert!(path.last().unwrap().distance(goal) < 1.5, "{path:?}");
    }

    #[test]
    fn a_cut_leg_s_pieces_end_on_the_corridor_s_floor_not_its_straight_line() {
        // A ridge 20 high across the way, the straight line between its feet
        // far under its top, with a post in the world alone on its slope.
        let ridge = [(-150.0, 0.0), (0.0, 20.0), (150.0, 0.0)];
        let triangles: Vec<_> = ridge
            .windows(2)
            .flat_map(|pair| {
                let ((z0, y0), (z1, y1)) = (pair[0], pair[1]);
                super::quad(
                    [-40.0, y0, z0],
                    [-40.0, y1, z1],
                    [40.0, y1, z1],
                    [40.0, y0, z0],
                )
            })
            .collect();
        let (mut world, mesh) = zone(&triangles);
        let post = CollisionMesh::new(block([-0.5, 0.0, 29.5], [0.5, 30.0, 30.5])).unwrap();
        world.set_obstacles(vec![Arc::new(post)]);
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 0.0, 140.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        let (_, repaired, fallback) = mesh_planned(&*search);
        assert_eq!((repaired, fallback), (1, None), "{path:?}");
        assert!(matches!(search.planned(), Planned::Mesh { cut: 1, .. }));
        assert!(path.iter().any(|feet| feet.y > 15.0), "{path:?}");
    }

    /// A mesh built over a rise `high` in the middle of the way, its ramps
    /// 30 long, and a world whose floor is flat under it: the mesh off the
    /// floor there by more than its lift reaches, as a polygon's corners can
    /// lie off curved ground.
    fn rise_in_the_mesh_alone(high: f32) -> (CollisionWorld, MeshNavigator) {
        let ground = [
            (-150.0, 0.0),
            (-70.0, 0.0),
            (-40.0, high),
            (40.0, high),
            (70.0, 0.0),
            (150.0, 0.0),
        ];
        let triangles: Vec<_> = ground
            .windows(2)
            .flat_map(|pair| {
                let ((z0, y0), (z1, y1)) = (pair[0], pair[1]);
                super::quad(
                    [-40.0, y0, z0],
                    [-40.0, y1, z1],
                    [40.0, y1, z1],
                    [40.0, y0, z0],
                )
            })
            .collect();
        let (_, mesh) = zone(&triangles);
        (
            CollisionWorld::new(floor(0.0, -150.0, 150.0)).unwrap(),
            mesh,
        )
    }

    #[test]
    fn a_piece_partway_along_ends_on_the_floor_its_walk_reached_under_the_corridor() {
        let (world, mesh) = rise_in_the_mesh_alone(7.0);
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 0.0, 140.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let path = found(run(&mut *search, &world, 256).0);
        let Planned::Mesh {
            pieces,
            loose,
            repaired,
            fallback,
            refused,
            ..
        } = search.planned()
        else {
            panic!("the grid planned");
        };
        assert_eq!((repaired.len(), fallback), (0, None), "{repaired:?}");
        assert_eq!(refused, Refusals::default());
        // The pieces ending on the rise stood on the walked floor.
        assert!(loose >= 1 && loose < pieces, "{loose} of {pieces}");
        assert!(path.iter().all(|feet| feet.y.abs() < 0.01), "{path:?}");
    }

    #[test]
    fn a_piece_s_walk_with_no_corridor_within_reach_over_it_is_still_refused() {
        // The rise 14 high: the walked floor lies farther under the corridor
        // than a search's own end may stand off.
        let (world, mesh) = rise_in_the_mesh_alone(14.0);
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 0.0, 140.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        run(&mut *search, &world, 256);
        let Planned::Mesh { loose, refused, .. } = search.planned() else {
            panic!("the grid planned");
        };
        assert_eq!(loose, 0);
        assert!(refused.height >= 1, "{refused:?}");
    }

    #[test]
    fn a_leg_s_own_end_off_the_floor_is_still_refused() {
        let (world, mesh) = rise_in_the_mesh_alone(7.0);
        // The goal 5 over the floor, on the rise's ramp in the mesh: the last
        // piece's end, which stays strict.
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 5.0, -45.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        found(run(&mut *search, &world, 256).0);
        let Planned::Mesh {
            repaired, refused, ..
        } = search.planned()
        else {
            panic!("the grid planned");
        };
        assert_eq!(refused.height, 1, "{refused:?}");
        assert!(
            matches!(repaired[..], [(_, to)] if to == goal),
            "{repaired:?}"
        );
    }

    /// The most a piece's check costs: a piece's length in grid cells, plus
    /// one, from feet that ended within a hair of the last piece's end.
    const ONE_PIECE: usize = 34;

    #[test]
    fn a_long_leg_s_check_keeps_to_each_call_s_budget_a_piece_at_a_time() {
        let (world, mesh) = zone(&floor(0.0, -150.0, 150.0));
        let (start, goal) = (Vec3::new(0.0, 0.0, -140.0), Vec3::new(0.0, 0.0, 140.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let (progress, most) = run(&mut *search, &world, 16);
        found(progress);
        assert!(most <= 16 + ONE_PIECE, "{most}");
    }

    #[test]
    fn a_repair_and_its_fallback_keep_to_each_call_s_budget() {
        let (world, mesh) = post_beside_the_way();
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 20.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let (progress, most) = run(&mut *search, &world, 16);
        found(progress);
        assert_eq!(mesh_planned(&*search).1, 1);
        // A call's work is its budget, and at most one leg's check beyond.
        assert!(most <= 16 + ONE_LEG, "{most}");
    }

    #[test]
    fn a_leg_no_repair_can_mend_hands_the_whole_way_to_the_grid() {
        // The wall's gap filled in the world but open in the mesh.
        let mut triangles = floor(0.0, -40.0, 40.0);
        triangles.extend(wall_to(20.0));
        let (mut world, mesh) = zone(&triangles);
        let plug = CollisionMesh::new(block([20.0, 0.0, -0.5], [40.0, 10.0, 0.5])).unwrap();
        world.set_obstacles(vec![Arc::new(plug)]);
        let (start, goal) = (Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, 20.0));
        let mut search = mesh.search(request(start, goal, Vec::new()));
        let (progress, most) = run(&mut *search, &world, 64);
        assert_eq!(progress, PathProgress::Failed);
        assert!(matches!(
            mesh_planned(&*search).2,
            Some(Fallback::Leg { .. })
        ));
        assert!(most <= 64 + ONE_LEG, "{most}");
        // The grid's nearest reach stands, for a route that ends as close as it can.
        assert!(
            search
                .closest()
                .is_some_and(|path| path.last().unwrap().z < 0.0)
        );
    }

    #[test]
    fn ends_off_the_mesh_or_on_another_island_go_to_the_grid() {
        let mut triangles = floor(0.0, -20.0, 20.0);
        triangles.extend(floor(12.0, -10.0, 10.0));
        let (world, mesh) = zone(&triangles);
        let start = Vec3::new(-15.0, 0.0, -15.0);
        for (goal, why) in [
            (Vec3::new(0.0, 12.0, 0.0), Fallback::OtherIsland),
            (Vec3::new(0.0, 40.0, 0.0), Fallback::OffMesh),
        ] {
            let mut search = mesh.search(request(start, goal, Vec::new()));
            assert_eq!(run(&mut *search, &world, 256).0, PathProgress::Failed);
            assert_eq!(mesh_planned(&*search), (0, 0, Some(why)));
        }
    }

    #[test]
    fn a_short_way_round_stays_with_the_grid() {
        let (world, mesh) = zone(&floor(0.0, -20.0, 20.0));
        let mut local = request(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec::new());
        local.scope = Scope::Local;
        let mut search = mesh.search(local);
        found(run(&mut *search, &world, 256).0);
        assert_eq!(search.planned(), Planned::Grid);
    }
    mod drops {
        //! Ways off ledges, found at the build and taken by searches with
        //! drops on.

        use super::super::super::drops::{THINNING, links_with};
        use super::super::super::{DropLink, NavMesh};
        use super::{block, floor, found, length, request, run, settings, zone};
        use crate::movement::path::{drop, walk};
        use crate::movement::{
            CollisionMesh, CollisionWorld, Fallback, MAX_GROUNDED_STEP, Navigator, PathProgress,
            Planned, RouteSearch, SearchRequest,
        };
        use glam::{Vec2, Vec3};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        /// A floor at y = 0 and a platform `high` tall over z < -5, with a
        /// ramp up to it at its far end when `ramp` is set.
        fn platform(high: f32, ramp: bool) -> Vec<[[f32; 3]; 3]> {
            let mut triangles = floor(0.0, -40.0, 40.0);
            triangles.extend(block([-30.0, 0.0, -30.0], [20.0, high, -5.0]));
            if ramp {
                // Half as steep as long: well inside the walker's slope limit.
                let run = 2.0 * high;
                triangles.push([
                    [20.0, high, -30.0],
                    [20.0, high, -5.0],
                    [20.0 + run, 0.0, -5.0],
                ]);
                triangles.push([
                    [20.0, high, -30.0],
                    [20.0 + run, 0.0, -5.0],
                    [20.0 + run, 0.0, -30.0],
                ]);
            }
            triangles
        }

        fn falling(start: Vec3, goal: Vec3) -> SearchRequest {
            SearchRequest {
                drops: true,
                ..request(start, goal, Vec::new())
            }
        }

        fn planned(search: &dyn RouteSearch) -> (usize, usize, usize, Option<Fallback>) {
            match search.planned() {
                Planned::Mesh {
                    legs,
                    drops,
                    repaired,
                    fallback,
                    ..
                } => (legs, drops, repaired.len(), fallback),
                Planned::Grid => panic!("the grid planned"),
            }
        }

        /// Follows `path` from `start` as the walker would: a walk to each
        /// waypoint, or where a walk can't reach it, a drop; where it ends.
        fn follow(world: &CollisionWorld, start: Vec3, path: &[Vec3]) -> Vec3 {
            let mut feet = start;
            for &next in path {
                let to = Vec2::new(next.x, next.z);
                feet = walk(world, feet, to, 6.0)
                    .filter(|end| (end.y - next.y).abs() <= MAX_GROUNDED_STEP)
                    .or_else(|| drop(world, feet, to, 6.0))
                    .unwrap_or_else(|| panic!("{feet} to {next} in {path:?}"));
            }
            feet
        }

        fn links(mesh: &NavMesh) -> &[DropLink] {
            &mesh.drops
        }

        fn mesh_of(navigator: &crate::movement::navmesh::MeshNavigator) -> &NavMesh {
            navigator.mesh()
        }

        #[test]
        fn a_route_with_drops_steps_off_the_ledge_and_one_without_takes_the_ramp() {
            let (world, mesh) = zone(&platform(8.0, true));
            let (start, goal) = (Vec3::new(0.0, 8.0, -15.0), Vec3::new(0.0, 0.0, 10.0));
            let mut search = mesh.search(falling(start, goal));
            let path = found(run(&mut *search, &world, 256).0);
            let (_, drops, repaired, fallback) = planned(&*search);
            assert_eq!((drops, repaired, fallback), (1, 0, None), "{path:?}");
            assert!(length(start, &path) < 40.0, "{path:?}");
            assert!(
                follow(&world, start, &path).distance(goal) < 1.5,
                "{path:?}"
            );
            // Without drops, down the ramp.
            let mut search = mesh.search(request(start, goal, Vec::new()));
            let path = found(run(&mut *search, &world, 256).0);
            let (_, drops, _, fallback) = planned(&*search);
            assert_eq!((drops, fallback), (0, None), "{path:?}");
            assert!(path.iter().any(|p| p.x > 20.0), "{path:?}");
            assert!(
                follow(&world, start, &path).distance(goal) < 1.5,
                "{path:?}"
            );
        }

        #[test]
        fn a_ledge_deeper_than_the_drop_limit_gives_no_way_down() {
            let (world, mesh) = zone(&platform(30.0, false));
            assert_eq!(links(mesh_of(&mesh)).len(), 0);
            let (start, goal) = (Vec3::new(0.0, 30.0, -15.0), Vec3::new(0.0, 0.0, 10.0));
            let mut search = mesh.search(falling(start, goal));
            assert_eq!(run(&mut *search, &world, 256).0, PathProgress::Failed);
            assert_eq!(planned(&*search).3, Some(Fallback::OtherIsland));
        }

        #[test]
        fn a_drop_goes_one_way_only() {
            let (world, mesh) = zone(&platform(8.0, true));
            let (start, goal) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 8.0, -15.0));
            let mut search = mesh.search(falling(start, goal));
            let path = found(run(&mut *search, &world, 256).0);
            let (_, drops, _, fallback) = planned(&*search);
            assert_eq!((drops, fallback), (0, None), "{path:?}");
            assert!(path.iter().any(|p| p.x > 20.0), "up the ramp: {path:?}");
        }

        #[test]
        fn a_step_down_is_a_walk_not_a_drop() {
            let mut triangles = floor(0.0, -20.0, 20.0);
            triangles.extend(block([-20.0, 0.0, -20.0], [20.0, 1.5, 0.0]));
            let (_, mesh) = zone(&triangles);
            assert_eq!(links(mesh_of(&mesh)).len(), 0);
        }

        #[test]
        fn drops_along_one_ledge_onto_one_floor_are_kept_apart() {
            let (_, mesh) = zone(&platform(8.0, false));
            let links = links(mesh_of(&mesh));
            assert_ne!(links.len(), 0);
            for (i, a) in links.iter().enumerate() {
                assert!(a.top.y - a.landing.y > MAX_GROUNDED_STEP, "{a:?}");
                for b in &links[i + 1..] {
                    if (a.from, a.to) == (b.from, b.to) {
                        assert!(a.top.distance(b.top) >= THINNING, "{a:?} {b:?}");
                    }
                }
            }
        }

        #[test]
        fn islands_a_drop_leads_down_to_are_reached_only_with_drops() {
            let (world, mesh) = zone(&platform(8.0, false));
            let (top, below) = (Vec3::new(0.0, 8.0, -15.0), Vec3::new(0.0, 0.0, 10.0));
            let mut search = mesh.search(falling(top, below));
            let path = found(run(&mut *search, &world, 256).0);
            assert_eq!(planned(&*search).1, 1, "{path:?}");
            for (request, why) in [
                (request(top, below, Vec::new()), Fallback::OtherIsland),
                (falling(below, top), Fallback::OtherIsland),
            ] {
                let mut search = mesh.search(request);
                assert_eq!(run(&mut *search, &world, 256).0, PathProgress::Failed);
                assert_eq!(planned(&*search).3, Some(why));
            }
        }

        #[test]
        fn a_drop_the_world_refuses_is_repaired_with_drops() {
            // A railing at the ledge's edge, in the world but not in the mesh.
            let (mut world, mesh) = zone(&platform(8.0, false));
            let railing = CollisionMesh::new(block([-4.0, 8.0, -5.4], [4.0, 11.0, -5.0])).unwrap();
            world.set_obstacles(vec![Arc::new(railing)]);
            let (start, goal) = (Vec3::new(0.0, 8.0, -15.0), Vec3::new(0.0, 0.0, 10.0));
            let mut search = mesh.search(falling(start, goal));
            let path = found(run(&mut *search, &world, 256).0);
            let (_, _, repaired, fallback) = planned(&*search);
            assert!(
                repaired >= 1 && fallback.is_none(),
                "{repaired} {fallback:?} {path:?}"
            );
            assert!(
                follow(&world, start, &path).distance(goal) < 1.5,
                "{path:?}"
            );
        }

        #[test]
        fn a_drop_into_deep_water_is_no_way_down_in_a_world_with_water() {
            // Water over the floor below the ledge, deep enough to close over
            // the walker's head: its own drop never falls into it.
            let triangles = platform(8.0, false);
            let (_, navigator) = zone(&triangles);
            // The floor in front of the platform, where the water is.
            let into_water =
                |links: &[DropLink]| links.iter().filter(|l| l.landing.z > -5.0).count();
            assert_ne!(into_water(links(mesh_of(&navigator))), 0);
            let water = crate::movement::liquids::Boxes(vec![(
                Vec3::new(-40.0, -1.0, -5.0),
                Vec3::new(40.0, 7.0, 40.0),
                crate::movement::Liquid::Water,
            )]);
            let world = CollisionWorld::new(triangles.iter().copied())
                .unwrap()
                .with_liquids(water);
            let (mesh, report) = NavMesh::build_in(
                &triangles,
                Some(&world),
                &settings(),
                2,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(into_water(links(&mesh)), 0);
            // The floor under the water is left out of the mesh, so no drop
            // has a landing there.
            assert!(mesh.locate(Vec3::new(0.0, 0.0, 20.0)).is_none());
            // The drops off its other sides, onto dry floor, stay.
            assert_ne!(report.drop_links, 0);
        }

        #[test]
        fn drops_built_in_the_zones_world_leave_its_doors_out() {
            let triangles = platform(8.0, false);
            let (_, navigator) = zone(&triangles);
            let alone = links(mesh_of(&navigator)).to_vec();
            // A shut door along the ledge: the mesh's drops don't depend on it.
            let mut world = CollisionWorld::new(triangles.iter().copied()).unwrap();
            let door = CollisionMesh::new(block([-4.0, 8.0, -5.4], [4.0, 11.0, -5.0])).unwrap();
            world.set_door_obstacles(vec![(1, Arc::new(door))]);
            let (mesh, report) = NavMesh::build_in(
                &triangles,
                Some(&world),
                &settings(),
                2,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(report.drop_links, alone.len());
            assert_eq!(links(&mesh), &alone[..]);
        }

        #[test]
        fn a_build_cancelled_in_its_drops_keeps_nothing() {
            let triangles = platform(8.0, false);
            // One thread asks once a tile and once past the last: so many
            // asks, and the drops' first few, before it is cancelled.
            let (_, report) = super::super::build(&triangles);
            let asked = AtomicUsize::new(0);
            let limit = report.tiles + 3;
            let cancelled = || asked.fetch_add(1, Ordering::Relaxed) >= limit;
            assert!(NavMesh::build_until(&triangles, None, &settings(), 1, &cancelled).is_none());
            assert!(asked.load(Ordering::Relaxed) > limit);
        }

        #[test]
        fn a_stretch_whose_drops_panic_is_refused_and_the_rest_are_found() {
            let triangles = platform(8.0, false);
            let (_, navigator) = zone(&triangles);
            let mesh = mesh_of(&navigator);
            let world = CollisionWorld::new(triangles.iter().copied()).unwrap();
            let first = AtomicBool::new(true);
            let panicking = |mesh: &NavMesh, world: &CollisionWorld, stretch: &_, height: f32| {
                assert!(!first.swap(false, Ordering::Relaxed), "the first stretch");
                super::super::super::drops::stretch_links(mesh, world, stretch, height)
            };
            let (found, refused) = links_with(mesh, &world, 6.0, 1, &|| false, &panicking).unwrap();
            assert_eq!(refused, 1);
            assert_ne!(found.len(), 0);
        }

        #[test]
        fn drops_are_kept_in_the_cache() {
            let triangles = platform(8.0, false);
            let folder = super::super::folder("drops");
            let cancel = AtomicBool::new(false);
            let built = crate::movement::navmesh::cache::load_or_build(
                Some(&folder),
                "zone",
                &triangles,
                None,
                &settings(),
                2,
                &cancel,
            )
            .unwrap();
            let read =
                crate::movement::navmesh::cache::load(&folder, "zone", &triangles, &settings())
                    .unwrap();
            assert_ne!(built.mesh.drops.len(), 0);
            assert_eq!(read.drops, built.mesh.drops);
            let _ = std::fs::remove_dir_all(&folder);
        }
    }
}

mod sizes {
    //! One build tagged for three walker sizes, each size's mesh split out.

    use super::{block, floor, folder};
    use crate::movement::navmesh::{MeshNavigator, NavMesh, Settings, SizedMeshes, cache};
    use crate::movement::{
        CollisionWorld, Fallback, Navigator, PathProgress, Planned, Scope, SearchRequest,
    };
    use glam::Vec3;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    const SIZES: [f32; 3] = [4.0, 6.0, 9.0];

    /// A box hung over the floor: its sides, its top and its underside,
    /// which a walker meets overhead.
    fn beam(low: [f32; 3], high: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let ([x0, y0, z0], [x1, _, z1]) = (low, high);
        let mut triangles = block(low, high);
        // Facing down.
        triangles.push([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1]]);
        triangles.push([[x0, y0, z0], [x1, y0, z1], [x0, y0, z1]]);
        triangles
    }

    fn sized() -> Settings {
        Settings {
            cell_size: 0.25,
            tile: 16.0,
            ..Settings::sized(SIZES)
        }
    }

    fn plain(height: f32) -> Settings {
        Settings {
            cell_size: 0.25,
            tile: 16.0,
            ..Settings::walker(height)
        }
    }

    /// A floor split by a wall across x = 0, with a doorway at z -3..3 under
    /// a beam whose `underside` is that high, and an open way through at
    /// z 12..18.
    fn lintel(underside: f32) -> Vec<[[f32; 3]; 3]> {
        let mut triangles = floor(0.0, -20.0, 20.0);
        triangles.extend(block([-1.0, 0.0, -20.0], [1.0, 14.0, -3.0]));
        triangles.extend(beam([-1.0, underside, -3.0], [1.0, 14.0, 3.0]));
        triangles.extend(block([-1.0, 0.0, 3.0], [1.0, 14.0, 12.0]));
        triangles.extend(block([-1.0, 0.0, 18.0], [1.0, 14.0, 20.0]));
        triangles
    }

    fn build(triangles: &[[[f32; 3]; 3]], settings: &Settings) -> NavMesh {
        NavMesh::build(triangles, settings, 2, &AtomicBool::new(false))
            .unwrap()
            .0
    }

    #[test]
    fn every_link_of_a_tagged_build_meets_the_polygon_it_leads_to() {
        let triangles = lintel(5.0);
        for settings in [sized(), plain(SIZES[0])] {
            let mesh = build(&triangles, &settings);
            let (links, faults) = mesh.link_faults(settings.cell_size * 1.5, 0.5);
            assert!(links > 0);
            assert!(faults.is_empty(), "{faults:?}");
        }
    }

    #[test]
    fn a_taller_sizes_floor_stops_short_of_a_ceiling_too_low_for_it() {
        // The floor under a beam 5 high is the short size's alone. A plain
        // build for a taller walker erodes its floor back from the beam by
        // the walker's radius; the tagged build's taller sizes stop there
        // too, not at the beam's edge.
        let triangles = lintel(5.0);
        let reach = |mesh: &NavMesh| {
            mesh.polygons
                .iter()
                .flat_map(|polygon| polygon.vertices.iter())
                .filter(|v| v.x < -0.9 && v.z.abs() <= 3.5 && v.y < 2.0)
                .map(|v| v.x)
                .fold(f32::MIN, f32::max)
        };
        let tagged = build(&triangles, &sized()).split(SIZES);
        for height in [SIZES[1], SIZES[2]] {
            let plain = reach(&build(&triangles, &plain(height)));
            let sized = reach(&tagged.for_height(height).unwrap());
            assert!(plain < -1.2, "{height}: {plain}");
            assert!(
                (sized - plain).abs() <= 0.25,
                "{height}: {sized} against {plain}"
            );
        }
    }

    /// The route from `start` to `goal` for a walker `height` tall on
    /// `mesh`: its path, and any fallback.
    fn route(
        mesh: &Arc<NavMesh>,
        world: &CollisionWorld,
        start: Vec3,
        goal: Vec3,
        height: f32,
    ) -> (Option<Vec<Vec3>>, Option<Fallback>) {
        let navigator = MeshNavigator::new(Arc::clone(mesh));
        let mut search = navigator.search(SearchRequest {
            start,
            goal,
            reach: 1.0,
            height,
            doors: Vec::new(),
            drops: false,
            scope: Scope::Whole,
        });
        let progress = loop {
            match search.advance(world, 5000) {
                PathProgress::Searching => (),
                done => break done,
            }
        };
        let fallback = match search.planned() {
            Planned::Mesh { fallback, .. } => fallback,
            Planned::Grid => panic!("the grid planned"),
        };
        match progress {
            PathProgress::Found(path) => (Some(path), fallback),
            _ => (None, fallback),
        }
    }

    /// Whether the path from `start` crosses the wall at x = 0 under the
    /// beam.
    fn through_beam(start: Vec3, path: &[Vec3]) -> bool {
        std::iter::once(start)
            .chain(path.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| {
                let (a, b) = (pair[0], pair[1]);
                (a.x < 0.0) != (b.x < 0.0) && {
                    let z = a.z + (b.z - a.z) * (0.0 - a.x) / (b.x - a.x);
                    z.abs() < 3.0
                }
            })
    }

    #[test]
    fn a_low_beam_lets_the_short_size_through_and_sends_the_others_round() {
        let triangles = lintel(5.0);
        let world = CollisionWorld::new(triangles.iter().copied()).unwrap();
        let tagged = build(&triangles, &sized());
        // Every polygon on the floor under the beam carries the short size
        // alone.
        let under: Vec<u8> = tagged
            .polygons
            .iter()
            .filter(|polygon| {
                #[allow(clippy::cast_precision_loss)] // A polygon has a few corners.
                let centre = polygon.vertices.iter().sum::<Vec3>() / polygon.vertices.len() as f32;
                centre.x.abs() < 1.0 && centre.z.abs() < 3.0 && centre.y < 2.0
            })
            .map(|polygon| polygon.size)
            .collect();
        assert!(
            !under.is_empty() && under.iter().all(|&size| size == 0),
            "{under:?}"
        );
        let meshes = tagged.split(SIZES);
        let (start, goal) = (Vec3::new(-10.0, 0.5, 0.0), Vec3::new(10.0, 0.5, 0.0));
        let short = meshes.for_height(SIZES[0]).unwrap();
        let (path, fallback) = route(&short, &world, start, goal, SIZES[0]);
        let path = path.unwrap_or_else(|| panic!("{fallback:?}"));
        assert!(through_beam(start, &path), "{path:?}");
        for height in [SIZES[1], SIZES[2]] {
            let mesh = meshes.for_height(height).unwrap();
            let (path, fallback) = route(&mesh, &world, start, goal, height);
            let path = path.unwrap_or_else(|| panic!("{height}: {fallback:?}"));
            assert!(!through_beam(start, &path), "{height}: {path:?}");
            assert!(path.iter().any(|p| p.z > 10.0), "{height}: round: {path:?}");
        }
    }

    #[test]
    fn with_no_way_round_the_taller_sizes_fall_back() {
        let mut triangles = floor(0.0, -20.0, 20.0);
        triangles.extend(block([-1.0, 0.0, -20.0], [1.0, 14.0, -3.0]));
        triangles.extend(beam([-1.0, 5.0, -3.0], [1.0, 14.0, 3.0]));
        triangles.extend(block([-1.0, 0.0, 3.0], [1.0, 14.0, 20.0]));
        let world = CollisionWorld::new(triangles.iter().copied()).unwrap();
        let meshes = build(&triangles, &sized()).split(SIZES);
        let (start, goal) = (Vec3::new(-10.0, 0.5, 0.0), Vec3::new(10.0, 0.5, 0.0));
        let short = meshes.for_height(SIZES[0]).unwrap();
        assert!(route(&short, &world, start, goal, SIZES[0]).0.is_some());
        let medium = meshes.for_height(SIZES[1]).unwrap();
        let (path, fallback) = route(&medium, &world, start, goal, SIZES[1]);
        assert!(path.is_none());
        assert_eq!(fallback, Some(Fallback::OtherIsland));
    }

    #[test]
    fn each_size_of_a_tagged_build_matches_a_plain_build_at_that_size() {
        // Beams at each size's edges: one only the short fits, one the short
        // and medium fit.
        let mut triangles = floor(0.0, -40.0, 40.0);
        triangles.extend(block([-1.0, 0.0, -40.0], [1.0, 14.0, -24.0]));
        triangles.extend(beam([-1.0, 5.0, -24.0], [1.0, 14.0, -18.0]));
        triangles.extend(block([-1.0, 0.0, -18.0], [1.0, 14.0, 0.0]));
        triangles.extend(beam([-1.0, 7.5, 0.0], [1.0, 14.0, 6.0]));
        triangles.extend(block([-1.0, 0.0, 6.0], [1.0, 14.0, 24.0]));
        triangles.extend(block([-1.0, 0.0, 30.0], [1.0, 14.0, 40.0]));
        let world = CollisionWorld::new(triangles.iter().copied()).unwrap();
        let tagged = build(&triangles, &sized());
        let meshes = tagged.split(SIZES);
        let (start, goal) = (Vec3::new(-20.0, 0.5, -21.0), Vec3::new(20.0, 0.5, -21.0));
        for (size, &height) in SIZES.iter().enumerate() {
            let plain = Arc::new(build(&triangles, &plain(height)));
            let ours = meshes.for_height(height).unwrap();
            #[allow(clippy::cast_possible_truncation)] // Three sizes.
            let (area, plain_area) = (tagged.floor_area(size as u8), plain.floor_area(0));
            assert!(
                (area - plain_area).abs() <= 0.02 * plain_area,
                "{height}: {area} against {plain_area}"
            );
            let (ours, ours_fallback) = route(&ours, &world, start, goal, height);
            let (theirs, theirs_fallback) = route(&plain, &world, start, goal, height);
            assert_eq!(ours_fallback, theirs_fallback, "{height}");
            let (ours, theirs) = (ours.unwrap(), theirs.unwrap());
            let length = |path: &[Vec3]| super::routes_length(start, path);
            assert!(
                (length(&ours) - length(&theirs)).abs() <= 0.03 * length(&theirs),
                "{height}: {ours:?} against {theirs:?}"
            );
        }
    }

    #[test]
    fn a_walker_uses_the_smallest_size_that_fits_it_and_none_past_the_tallest() {
        let meshes: SizedMeshes = build(&floor(0.0, -10.0, 10.0), &sized()).split(SIZES);
        for (height, size) in [
            (3.5, Some(0)),
            (4.0, Some(0)),
            (5.0, Some(1)),
            (6.0, Some(1)),
            (8.0, Some(2)),
            (9.0, Some(2)),
            (9.5, None),
        ] {
            assert_eq!(meshes.size_for(height), size, "{height}");
        }
        assert!(meshes.for_height(9.5).is_none());
    }

    #[test]
    fn a_tagged_mesh_keeps_its_sizes_in_the_cache_and_is_counted_by_its_header() {
        let triangles = lintel(5.0);
        let folder = folder("sizes");
        assert!(!cache::present(&folder, "zone", &sized()));
        let built = cache::load_or_build(
            Some(&folder),
            "zone",
            &triangles,
            None,
            &sized(),
            2,
            &AtomicBool::new(false),
        )
        .unwrap();
        let read = cache::load(&folder, "zone", &triangles, &sized()).unwrap();
        assert_eq!(read.polygons, built.mesh.polygons);
        assert!(cache::present(&folder, "zone", &sized()));
        assert!(!cache::present(&folder, "other", &sized()));
        assert!(!cache::present(&folder, "zone", &plain(6.0)));
        // A file that isn't a mesh doesn't count.
        let name = super::files(&folder).pop().unwrap();
        std::fs::write(folder.join("navmesh").join(&name), b"EQNM").unwrap();
        assert!(!cache::present(&folder, "zone", &sized()));
        let _ = std::fs::remove_dir_all(&folder);
    }
}

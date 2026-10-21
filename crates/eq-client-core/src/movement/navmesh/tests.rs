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
    let built =
        cache::load_or_build(Some(&folder), "zone", &triangles, &settings(), 2, &cancel).unwrap();
    assert!(matches!(built.source, Source::Built(_)));
    let bytes = built.cached.unwrap();
    let read =
        cache::load_or_build(Some(&folder), "zone", &triangles, &settings(), 2, &cancel).unwrap();
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
    let built =
        cache::load_or_build(Some(&folder), "zone", &triangles, &settings(), 2, &cancel).unwrap();
    let path = cache::path(&folder, "zone", cache::key(&triangles, &settings())).unwrap();
    let good = std::fs::read(&path).unwrap();
    assert_eq!(good.len(), built.cached.unwrap());
    // Another format version in the header, then the body cut short.
    let mut other = good.clone();
    other[4] ^= 1;
    for bad in [other, good[..good.len() - 3].to_vec()] {
        std::fs::write(&path, bad).unwrap();
        assert!(cache::load(&folder, "zone", &triangles, &settings()).is_none());
        let again =
            cache::load_or_build(Some(&folder), "zone", &triangles, &settings(), 2, &cancel)
                .unwrap();
        assert!(matches!(again.source, Source::Built(_)));
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
    for (zone, triangles) in [("zone", &old), ("zone2", &old), ("zone", &new)] {
        let built =
            cache::load_or_build(Some(&folder), zone, triangles, &settings(), 2, &cancel).unwrap();
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

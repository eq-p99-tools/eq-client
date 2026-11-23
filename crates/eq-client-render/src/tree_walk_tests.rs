//! A walker meets an installed tree as the engine's walking sweep does (read
//! R49-49c): through the real asset loader and the movement world.
use super::*;

/// Walks a body from before a tree's branch, along it, to past its trunk,
/// on the zone's walking world. Returns the highest the feet stood above
/// the floor and how far past the trunk they ended (negative: short of it).
fn walk_into(zone: &ZoneAsset, name: &str) -> (f32, f32) {
    let world = collision_world(zone, collision_triangles(zone), None).unwrap();
    let object = zone
        .objects
        .iter()
        .find(|object| zone.models[object.model].name == name)
        .unwrap();
    let transform = object_transform(object).to_matrix();
    let points: Vec<Vec3> = zone.models[object.model]
        .primitives
        .iter()
        .flat_map(|primitive| primitive.positions.iter())
        .map(|&point| transform.transform_point3(Vec3::from_array(point)))
        .collect();
    let floor = points
        .iter()
        .map(|point| point.y)
        .fold(f32::INFINITY, f32::min);
    let top = *points.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    // The branch's far end from the trunk, on the floor.
    let start = points
        .iter()
        .copied()
        .max_by(|a, b| {
            let flat = |point: Vec3| Vec3::new(point.x - top.x, 0.0, point.z - top.z).length();
            flat(*a).total_cmp(&flat(*b))
        })
        .unwrap();
    let across = Vec3::new(top.x - start.x, 0.0, top.z - start.z);
    let toward = across.normalize();
    // The branch's middle line, 5 before it.
    let side = Vec3::Y.cross(toward);
    let middle = start + side * side.dot(top - start);
    let mut feet = Vec3::new(middle.x, floor, middle.z) - toward * 5.0;
    let mut highest = 0.0_f32;
    for _ in 0..50 {
        feet = world.step(feet, toward * 0.5, 6.0);
        highest = highest.max(feet.y - floor);
    }
    let past = toward.dot(feet - Vec3::new(top.x, floor, top.z));
    (highest, past)
}

#[test]
fn a_walker_passes_through_a_no_hit_tree_and_is_never_stood_on_its_branch() {
    let install = std::env::temp_dir().join(format!("eq-grove-{}", std::process::id()));
    std::fs::create_dir_all(&install).unwrap();
    eq_client_assets::testing::write_grove(&install);
    let zone = eq_client_assets::load_zone(&install, "grove");
    std::fs::remove_dir_all(&install).unwrap();
    let zone = zone.unwrap();

    // The no-hit tree: its branch 3 under the walker's path never lifts the
    // feet off the floor, and its trunk doesn't stop them.
    let (highest, past) = walk_into(&zone, "NOHIT_ACTORDEF");
    assert!(highest < 0.01, "lifted {highest} onto the no-hit branch");
    assert!(past > 5.0, "stopped {past} from the no-hit trunk");

    // The same shape on a referenced mesh is met: the walker is stood on
    // the branch or stopped at the trunk.
    let (highest, past) = walk_into(&zone, "SOLID_ACTORDEF");
    assert!(
        highest > 2.0 || past < 0.0,
        "the referenced tree was walked through ({highest}, {past})"
    );
}

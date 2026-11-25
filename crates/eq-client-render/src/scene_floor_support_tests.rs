use super::*;

#[test]
fn floor_support_preserves_authored_wld_winding_through_the_loader() {
    // This proves propagation, not an orientation convention by itself.
    // The renderer culls single-sided backs by this same winding, and
    // select_spots already accepts upward-authored geometric normals.
    // The standing/reachable F4 census supplies the real-data control.
    let directory = std::env::temp_dir().join(format!("floor-winding-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    eq_client_assets::testing::write_floor_winding(&directory);
    let zone = eq_client_assets::load_zone(&directory, "floor_winding").unwrap();
    let world = zone_collision(&zone).unwrap();
    assert_eq!(zone.collision.len(), 2);
    let drawn = zone
        .primitives
        .iter()
        .flat_map(|primitive| {
            primitive
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(move |triangle| {
                    triangle.map(|index| Vec3::from_array(primitive.positions[index as usize]))
                })
        })
        .collect::<Vec<_>>();
    for (index, sign) in [(0, 1.0), (1, -1.0)] {
        let solid = zone.collision[index].map(Vec3::from_array);
        assert_eq!(drawn[index], solid);
        let [a, b, c] = solid;
        assert!((b - a).cross(c - a).y * sign > 0.0);
        let center = (a + b + c) / 3.0;
        assert_eq!(world.ground(center, 0.1, 0.1).is_some(), sign > 0.0);
        assert!(
            world
                .ray_distance(center + Vec3::Y, -Vec3::Y, 2.0)
                .is_some()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

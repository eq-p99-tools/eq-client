use super::*;

#[test]
fn native_close_is_strictly_beyond_the_scaled_offset_in_two_dimensions() {
    let corpse = eq_client_core::WorldPosition::default();
    for offset in [1.875, 3.75, 7.5] {
        let limit = offset * 1.5 + 20.0;
        for (x, y, closes) in [
            (limit - 0.001, 0.0, false),
            (limit, 0.0, false),
            (limit + 0.001, 0.0, true),
            (0.0, limit + 0.001, true),
        ] {
            for z in [-10000.0, 0.0, 10000.0] {
                assert_eq!(
                    beyond(
                        eq_client_core::WorldPosition { x, y, z, ..corpse },
                        corpse,
                        offset
                    ),
                    closes
                );
            }
        }
    }
    for x in [f32::NAN, f32::INFINITY] {
        assert!(!beyond(
            eq_client_core::WorldPosition { x, ..corpse },
            corpse,
            3.75
        ));
    }
    assert!(!beyond(corpse, corpse, f32::NAN));
}

use super::*;

fn zones(session_id: u64, entries: &[(u32, u16)]) -> WorldEvent {
    WorldEvent::ZoneLineZones {
        session_id,
        zones: entries.iter().copied().collect(),
    }
}

#[test]
fn the_initial_exit_map_arrives_between_entered_and_connected() {
    let mut world = ClientWorld::default();
    assert!(game(&mut world, zones(1, &[(3, 54)])).ignored);
    game(&mut world, entered(1));
    assert!(!world.connected());
    assert!(!game(&mut world, zones(1, &[(3, 54)])).ignored);
    connection(&mut world, true, false);
    assert_eq!(world.zone_line_zones().get(&3), Some(&54));
}

#[test]
fn exit_map_replacement_invalidation_and_stale_admissions_are_distinct() {
    let mut world = admitted();
    game(&mut world, zones(1, &[(3, 54)]));
    let revision = world.zone_line_zones_revision();
    assert!(game(&mut world, zones(2, &[(3, 61)])).ignored);
    assert_eq!(world.zone_line_zones_revision(), revision);
    game(&mut world, zones(1, &[(4, 61)]));
    assert_eq!(world.zone_line_zones().get(&3), None);
    assert_eq!(world.zone_line_zones().get(&4), Some(&61));
    assert_ne!(world.zone_line_zones_revision(), revision);
    game(&mut world, zones(1, &[]));
    assert!(world.zone_line_zones().is_empty());
    game(&mut world, zones(1, &[(3, 54)]));
    game(&mut world, entered(2));
    assert!(world.zone_line_zones().is_empty());
    assert!(game(&mut world, zones(1, &[(3, 54)])).ignored);
    connection(&mut world, false, true);
    assert!(game(&mut world, zones(2, &[(3, 54)])).ignored);
    assert!(world.zone_line_zones().is_empty());
}

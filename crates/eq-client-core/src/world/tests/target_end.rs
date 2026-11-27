use super::*;
use crate::GameDialect;

fn death(id: u16) -> WorldEvent {
    WorldEvent::Death(Death {
        spawn_id: u32::from(id),
        killer_id: 9,
        corpse_id: u32::from(id),
        bind_zone_id: 0,
        corpse_name: None,
    })
}

#[test]
fn received_target_death_or_despawn_ends_attack_once_and_clears_only_that_target() {
    for dialect in [GameDialect::Titanium, GameDialect::EqMac, GameDialect::Rof2] {
        for event in [death(5), WorldEvent::Despawn(5)] {
            let mut world = admitted();
            world.generation = dialect;
            world.select_target(Some(5));
            let changes = game(&mut world, event.clone());
            assert!(changes.attack_ended, "{dialect:?} {event:?}");
            assert_eq!(world.target().selected, None);
            assert!(
                !world.target().sent,
                "the target clear uses ordinary session reconciliation"
            );
            assert!(!game(&mut world, event).attack_ended);
        }
    }
}

#[test]
fn unrelated_death_despawn_corpse_replacement_and_plain_clear_are_not_target_death() {
    for event in [death(6), WorldEvent::Despawn(6)] {
        let mut world = admitted();
        world.select_target(Some(5));
        assert!(!game(&mut world, event).attack_ended);
        assert_eq!(world.target().selected, Some(5));
    }
    let mut world = admitted();
    world.select_target(Some(5));
    let mut corpse = spawn(5);
    corpse.kind = SpawnKind::NpcCorpse;
    assert!(!game(&mut world, WorldEvent::Spawns(vec![corpse])).attack_ended);
    assert_eq!(world.target().selected, Some(5));
    world.select_target(None);
    // A plain target change has its own one-second/native exception rule;
    // this fix does not invent the currently unread character/name-list gate.
    assert!(!game(&mut world, death(5)).attack_ended);
}

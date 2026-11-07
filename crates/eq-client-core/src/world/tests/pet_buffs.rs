use super::*;
use crate::pets::{PetBuff, PetBuffs, PetIdentity};
use std::time::Duration;

fn at(world: &mut ClientWorld, event: WorldEvent, now: Instant) {
    world.apply(&WorldUpdate::Game(event), now, &NoSpells);
}

fn snapshot(pet: u16, spell_id: u32) -> WorldEvent {
    WorldEvent::PetBuffs(PetBuffs {
        pet,
        slots: vec![Some(PetBuff { spell_id, ticks: 6 })],
    })
}

fn appears(spawn_id: u16, pet_owner: Option<u16>) -> WorldEvent {
    WorldEvent::Spawns(vec![SpawnState {
        pet_owner,
        ..spawn(spawn_id)
    }])
}

fn buff(world: &ClientWorld) -> Option<(u16, u32)> {
    let buffs = world.pet_buffs()?;
    Some((buffs.pet, buffs.slots.first()?.as_ref()?.spell_id))
}

#[test]
fn the_latest_early_snapshot_survives_first_spawn_and_ownership() {
    let mut world = ClientWorld::default();
    game(&mut world, entered(1));
    game(&mut world, snapshot(6, 20));
    game(&mut world, snapshot(6, 21));
    game(
        &mut world,
        WorldEvent::OwnPet {
            session_id: 1,
            pet: None,
        },
    );
    game(&mut world, appears(6, None));
    connection(&mut world, true, false);
    assert_eq!(buff(&world), None);
    game(
        &mut world,
        WorldEvent::PetOwner {
            spawn_id: 6,
            owner: Some(9),
        },
    );
    assert_eq!(buff(&world), Some((6, 21)));
    game(
        &mut world,
        WorldEvent::OwnPet {
            session_id: 1,
            pet: Some(PetIdentity {
                spawn_id: 6,
                revision: 1,
            }),
        },
    );
    assert_eq!(buff(&world), Some((6, 21)));
    game(
        &mut world,
        WorldEvent::PetBuffs(PetBuffs {
            pet: 6,
            slots: vec![None],
        }),
    );
    assert_eq!(world.pet_buffs().unwrap().slots, [None]);
}

#[test]
fn early_snapshot_for_another_id_does_not_displace_the_current_pet_buffs() {
    let mut world = admitted();
    game(&mut world, appears(6, Some(9)));
    game(&mut world, snapshot(6, 20));
    game(&mut world, snapshot(7, 21));
    assert_eq!(buff(&world), Some((6, 20)));
    game(&mut world, appears(7, Some(9)));
    assert_eq!(
        buff(&world),
        None,
        "the two ownership reports are ambiguous"
    );
    game(&mut world, WorldEvent::Despawn(6));
    assert_eq!(buff(&world), Some((7, 21)));
}

#[test]
fn early_snapshot_has_a_bounded_window_without_expiring_displayed_buffs() {
    for (millis, tick_first) in [(999, false), (1_000, false), (1_000, true)] {
        let mut world = admitted();
        let received = Instant::now();
        at(&mut world, appears(6, Some(9)), received);
        at(&mut world, snapshot(6, 20), received);
        at(&mut world, snapshot(7, 21), received);

        let later = received + Duration::from_millis(millis);
        if tick_first {
            world.tick(later, &NoSpells);
        }
        assert_eq!(buff(&world), Some((6, 20)));
        at(&mut world, appears(7, Some(9)), later);
        at(&mut world, WorldEvent::Despawn(6), later);
        assert_eq!(buff(&world), (millis == 999).then_some((7, 21)));
        at(&mut world, snapshot(7, 23), later);
        assert_eq!(buff(&world), Some((7, 23)));
    }
}

#[test]
fn a_known_unowned_report_only_retires_an_id_with_snapshot_evidence() {
    for has_early_snapshot in [false, true] {
        let mut world = admitted();
        let now = Instant::now();
        at(&mut world, appears(6, None), now);
        if has_early_snapshot {
            at(&mut world, snapshot(6, 20), now);
        }
        at(
            &mut world,
            WorldEvent::PetOwner {
                spawn_id: 6,
                owner: None,
            },
            now,
        );
        at(&mut world, snapshot(6, 21), now);
        assert_eq!(buff(&world), None);
        at(
            &mut world,
            WorldEvent::PetOwner {
                spawn_id: 6,
                owner: Some(9),
            },
            now,
        );
        assert_eq!(buff(&world), (!has_early_snapshot).then_some((6, 21)));
        at(&mut world, snapshot(6, 22), now);
        assert_eq!(buff(&world), Some((6, 22)));
    }
}

#[test]
fn confirming_a_different_pet_discards_an_incompatible_early_snapshot() {
    let mut world = admitted();
    let now = Instant::now();
    at(&mut world, appears(7, Some(9)), now);
    at(&mut world, snapshot(7, 20), now);
    at(&mut world, snapshot(6, 21), now);
    at(
        &mut world,
        WorldEvent::OwnPet {
            session_id: 1,
            pet: Some(PetIdentity {
                spawn_id: 7,
                revision: 1,
            }),
        },
        now,
    );
    assert_eq!(buff(&world), Some((7, 20)));
    at(&mut world, appears(6, Some(9)), now);
    at(&mut world, WorldEvent::Despawn(7), now);
    assert_eq!(world.pet().unwrap().state.spawn_id, 6);
    assert_eq!(buff(&world), None);
}

#[test]
fn early_snapshots_cannot_cross_a_retired_id_or_conflicting_owner() {
    for boundary in [
        WorldEvent::Despawn(6),
        WorldEvent::Death(Death {
            spawn_id: 6,
            killer_id: 0,
            corpse_id: 6,
            bind_zone_id: 0,
            corpse_name: None,
        }),
        WorldEvent::PetOwner {
            spawn_id: 6,
            owner: Some(10),
        },
        WorldEvent::PetOwner {
            spawn_id: 6,
            owner: None,
        },
        WorldEvent::Charm(crate::control::Charm {
            owner_id: Some(9),
            pet_id: 6,
            charmed: false,
        }),
    ] {
        let mut world = admitted();
        game(&mut world, snapshot(6, 20));
        game(&mut world, boundary);
        game(&mut world, snapshot(6, 21));
        game(&mut world, appears(6, Some(9)));
        assert_eq!(buff(&world), None);
        game(&mut world, snapshot(6, 22));
        assert_eq!(buff(&world), Some((6, 22)));
    }

    let mut world = admitted();
    game(&mut world, appears(6, None));
    game(&mut world, snapshot(6, 20));
    game(&mut world, appears(6, Some(9)));
    assert_eq!(
        buff(&world),
        None,
        "a replacement may be reported without despawn"
    );
}

#[test]
fn early_snapshots_do_not_survive_reset_or_cross_admissions() {
    let mut world = admitted();
    game(&mut world, snapshot(6, 20));
    game(
        &mut world,
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
    );
    game(&mut world, snapshot(6, 21));
    game(
        &mut world,
        WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        },
    );
    game(&mut world, appears(6, Some(9)));
    assert_eq!(buff(&world), None);

    game(&mut world, snapshot(7, 22));
    connection(&mut world, false, false);
    game(&mut world, snapshot(7, 23));
    connection(&mut world, true, false);
    game(&mut world, WorldEvent::Despawn(6));
    game(&mut world, appears(7, Some(9)));
    assert_eq!(buff(&world), None);

    game(&mut world, snapshot(8, 24));
    game(&mut world, WorldEvent::DeathBind { session_id: 1 });
    game(&mut world, snapshot(8, 25));
    game(&mut world, entered(2));
    connection(&mut world, true, false);
    game(&mut world, appears(8, Some(9)));
    assert_eq!(buff(&world), None);
    game(&mut world, snapshot(8, 26));
    assert_eq!(buff(&world), Some((8, 26)));
}

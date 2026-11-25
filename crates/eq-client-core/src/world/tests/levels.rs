use super::*;
use crate::{GameDialect, listing::ListingChange, who::WhoFilter};

fn profile(world: &mut ClientWorld, session_id: u64, level: u8) {
    let mut event = entered(session_id);
    if let WorldEvent::Entered { player, .. } = &mut event {
        player.level = level;
        player.practice_points = Some(4);
    }
    game(world, event);
}

fn eqmac(level: u8) -> ClientWorld {
    let mut world = admitted();
    world.carry_as(GameDialect::EqMac);
    profile(&mut world, 2, level);
    world
}

#[test]
fn eqmac_level_appearance_changes_the_display_but_not_character_gates() {
    let mut world = eqmac(55);
    assert_eq!(world.displayed_level(), Some(55));
    game(
        &mut world,
        WorldEvent::Spawns(vec![
            SpawnState {
                kind: SpawnKind::Player,
                level: 55,
                ..spawn(9)
            },
            SpawnState {
                kind: SpawnKind::Player,
                level: 10,
                ..spawn(8)
            },
        ]),
    );
    for (spawn_id, level) in [(9, 1), (8, 2)] {
        game(
            &mut world,
            WorldEvent::Listing {
                spawn_id,
                change: ListingChange::Level(level),
            },
        );
    }
    assert_eq!(world.player().unwrap().level, 55);
    assert_eq!(world.displayed_level(), Some(1));
    assert_eq!(world.spawn(8).unwrap().state.level, 2);
    let listed = world.zone_who(&WhoFilter::default());
    assert_eq!(
        listed.iter().find(|p| p.name == "Example").unwrap().level,
        1
    );
    game(
        &mut world,
        WorldEvent::ListingSet {
            session_id: 2,
            change: ListingChange::Level(3),
        },
    );
    assert_eq!(world.player().unwrap().level, 55);
    assert_eq!(world.displayed_level(), Some(3));
}

#[test]
fn eqmac_level_messages_ignore_losses_and_equals_and_a_rise_sets_both_levels() {
    let mut world = eqmac(55);
    game(
        &mut world,
        WorldEvent::Spawns(vec![SpawnState {
            kind: SpawnKind::Player,
            level: 55,
            ..spawn(9)
        }]),
    );
    game(
        &mut world,
        WorldEvent::Listing {
            spawn_id: 9,
            change: ListingChange::Level(1),
        },
    );
    let revision = world.skills_revision;
    for current in [1, 55] {
        let changes = game(
            &mut world,
            WorldEvent::Level {
                current,
                previous: 55,
                experience: 999,
            },
        );
        assert_eq!(world.player().unwrap().level, 55);
        assert_eq!(world.displayed_level(), Some(1));
        assert_eq!(world.vitals().experience, None);
        assert!(changes.notices.is_empty() && changes.sounds.is_empty());
        assert_eq!(world.skills_revision, revision);
    }
    game(&mut world, WorldEvent::Experience(150));
    assert_eq!(world.player().unwrap().level, 55);
    assert_eq!(world.displayed_level(), Some(1));
    assert_eq!(world.vitals().experience, Some(150));
    let changes = game(
        &mut world,
        WorldEvent::Level {
            current: 56,
            previous: 55,
            experience: 777,
        },
    );
    assert_eq!(world.player().unwrap().level, 56);
    assert_eq!(world.displayed_level(), Some(56));
    assert_eq!(world.spawn(9).unwrap().state.level, 56);
    assert_eq!(world.vitals().experience, Some(777));
    assert!(changes.notices.is_empty() && changes.sounds.is_empty());
    assert_eq!(world.skills_revision, revision.wrapping_add(1));
}

#[test]
fn eqmac_level_profile_replaces_both_levels_and_the_practice_balance() {
    let mut world = eqmac(55);
    game(
        &mut world,
        WorldEvent::Listing {
            spawn_id: 9,
            change: ListingChange::Level(2),
        },
    );
    profile(&mut world, 3, 1);
    assert_eq!(world.player().unwrap().level, 1);
    assert_eq!(world.displayed_level(), Some(1));
    assert_eq!(world.player().unwrap().practice_points, Some(4));
    assert_eq!(world.vitals().experience, None);
    assert_eq!(ClientWorld::default().displayed_level(), None);
}

#[test]
fn other_generations_keep_their_existing_single_level_projection() {
    for dialect in [GameDialect::Titanium, GameDialect::Rof2] {
        let mut world = admitted();
        world.carry_as(dialect);
        game(
            &mut world,
            WorldEvent::Listing {
                spawn_id: 9,
                change: ListingChange::Level(3),
            },
        );
        assert_eq!(world.player().unwrap().level, 3);
        assert_eq!(world.displayed_level(), Some(3));
        game(
            &mut world,
            WorldEvent::Level {
                current: 1,
                previous: 3,
                experience: 50,
            },
        );
        assert_eq!(world.player().unwrap().level, 1);
        assert_eq!(world.displayed_level(), Some(1));
        assert_eq!(world.vitals().experience, Some(50));
    }
}

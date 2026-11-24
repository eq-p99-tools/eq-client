use super::*;
use crate::listing::{Anonymity, GuildRank, ListingChange};
use crate::world::GuildStatus;

fn player(spawn_id: u16, name: &str) -> SpawnState {
    SpawnState {
        name: name.into(),
        kind: SpawnKind::Player,
        ..spawn(spawn_id)
    }
}

fn gm(spawn_id: u16, name: &str) -> SpawnState {
    let mut spawn = player(spawn_id, name);
    spawn.appearance.titanium_gm = Some(1);
    spawn
}

fn listed(world: &mut ClientWorld, spawn_id: u16, change: ListingChange) {
    game(world, WorldEvent::Listing { spawn_id, change });
}

#[test]
fn guild_status_names_the_targets_guild_and_rank_as_eqemu_says_them() {
    let mut world = admitted();
    game(
        &mut world,
        WorldEvent::Spawns(vec![
            player(6, "Zed"),
            player(7, "Ann"),
            player(8, "Bo"),
            player(10, "Cy"),
            SpawnState {
                kind: SpawnKind::PlayerCorpse,
                ..player(11, "Gone")
            },
            gm(12, "Mod"),
        ]),
    );
    game(
        &mut world,
        WorldEvent::GuildNames(vec![(3, "Seekers".into())]),
    );
    for (spawn_id, rank) in [
        (6, GuildRank::Leader),
        (7, GuildRank::Officer),
        (8, GuildRank::Member),
    ] {
        listed(&mut world, spawn_id, ListingChange::Guild(Some(3)));
        listed(&mut world, spawn_id, ListingChange::GuildRank(rank));
    }
    let status = |world: &mut ClientWorld, target| {
        world.select_target(target);
        world.guild_status(None)
    };
    let named = |name: &str| (name.to_owned(), "Seekers".to_owned());
    assert_eq!(status(&mut world, None), Some(GuildStatus::NoPlayer));
    // An NPC and a player's corpse are no players.
    assert_eq!(status(&mut world, Some(5)), Some(GuildStatus::NoPlayer));
    assert_eq!(status(&mut world, Some(11)), Some(GuildStatus::NoPlayer));
    // A game master is refused, by target or by name.
    listed(&mut world, 12, ListingChange::Guild(Some(3)));
    listed(&mut world, 12, ListingChange::GuildRank(GuildRank::Member));
    assert_eq!(status(&mut world, Some(12)), Some(GuildStatus::NoPlayer));
    assert_eq!(world.guild_status(Some("Mod")), Some(GuildStatus::NoPlayer));
    let (name, guild) = named("Zed");
    assert_eq!(
        status(&mut world, Some(6)),
        Some(GuildStatus::Leader { name, guild })
    );
    let (name, guild) = named("Ann");
    assert_eq!(
        status(&mut world, Some(7)),
        Some(GuildStatus::Officer { name, guild })
    );
    let (name, guild) = named("Bo");
    assert_eq!(
        status(&mut world, Some(8)),
        Some(GuildStatus::Member { name, guild })
    );
    assert_eq!(
        status(&mut world, Some(10)),
        Some(GuildStatus::NoGuild { name: "Cy".into() })
    );
    // Anonymity isn't tested: the guild the server sent shows.
    for anonymity in [
        Anonymity::Roleplaying,
        Anonymity::Anonymous,
        Anonymity::AnonymousRoleplaying,
    ] {
        listed(&mut world, 8, ListingChange::Anonymity(anonymity));
        assert!(matches!(
            status(&mut world, Some(8)),
            Some(GuildStatus::Member { .. })
        ));
    }
    // A guild the world's list doesn't name, or a rank not come yet, says
    // nothing.
    listed(&mut world, 10, ListingChange::Guild(Some(4)));
    listed(&mut world, 10, ListingChange::GuildRank(GuildRank::Member));
    assert_eq!(status(&mut world, Some(10)), None);
    listed(&mut world, 10, ListingChange::Guild(Some(3)));
    assert_eq!(status(&mut world, Some(10)), None);
    // A rank past the three has a line naming only the player.
    listed(
        &mut world,
        10,
        ListingChange::GuildRank(GuildRank::Other(3)),
    );
    assert_eq!(
        status(&mut world, Some(10)),
        Some(GuildStatus::Ranked {
            name: "Cy".into(),
            guild: "Seekers".into()
        })
    );
    // A typed name finds the player, whatever the target, ignoring case;
    // a name no spawn has, or an NPC's, is refused.
    world.select_target(Some(10));
    let (name, guild) = named("Zed");
    assert_eq!(
        world.guild_status(Some("zed")),
        Some(GuildStatus::Leader { name, guild })
    );
    assert_eq!(
        world.guild_status(Some("Nobody")),
        Some(GuildStatus::NoPlayer)
    );
}

#[test]
fn guild_status_of_the_player_reads_their_own_listing() {
    let mut world = admitted();
    world.select_target(Some(9));
    assert_eq!(
        world.guild_status(None),
        Some(GuildStatus::NoGuild {
            name: world.player().unwrap().name.clone()
        })
    );
    // The player's own GM flag is the own spawn record's, and refuses too.
    world.player.as_mut().unwrap().appearance.titanium_gm = Some(1);
    assert_eq!(world.guild_status(None), Some(GuildStatus::NoPlayer));
}

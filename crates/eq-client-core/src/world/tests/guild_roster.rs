use super::*;
use crate::{
    guild::GuildUpdate,
    listing::{GuildRank, ListingChange},
};

fn guilded() -> ClientWorld {
    let mut world = admitted();
    world.player.as_mut().unwrap().listing.guild = Some(1);
    world
}

fn listed(world: &mut ClientWorld) {
    game(
        world,
        WorldEvent::Guild(GuildUpdate::Members(vec![guild_member("Example")])),
    );
}

fn add(name: &str) -> GuildUpdate {
    GuildUpdate::MemberAdded {
        guild_id: 1,
        name: name.into(),
        level: 8,
        class: 2,
        rank: GuildRank::Member,
        zone_id: Some(2),
        last_on: 7,
    }
}

#[test]
fn an_add_has_an_unknown_note_and_does_not_replace_received_notes() {
    let mut world = guilded();
    listed(&mut world);
    assert_eq!(world.guild_member_note("Example"), Some(""));
    game(&mut world, WorldEvent::Guild(add("Second")));
    assert_eq!(world.guild_member_note("Second"), None);
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::MemberRenamed {
            guild_id: 1,
            old_name: "Second".into(),
            new_name: "Renamed".into(),
        }),
    );
    assert_eq!(world.guild_member_note("Renamed"), None);
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::MemberNote {
            guild_id: 1,
            name: "Renamed".into(),
            note: "Received".into(),
        }),
    );
    game(&mut world, WorldEvent::Guild(add("Renamed")));
    assert_eq!(world.guild_member_note("Renamed"), Some("Received"));
    assert_eq!(world.guild_members().unwrap().len(), 2);
    listed(&mut world);
    assert_eq!(world.guild_members().unwrap().len(), 1);
    assert_eq!(world.guild_member_note("Renamed"), None);
}

#[test]
fn another_guild_cannot_change_or_disband_the_received_roster() {
    let mut world = guilded();
    listed(&mut world);
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::MemberNote {
            guild_id: 2,
            name: "Example".into(),
            note: "Wrong guild".into(),
        }),
    );
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::Disbanded { guild_id: 2 }),
    );
    assert_eq!(world.guild_member_note("Example"), Some(""));
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::MemberRemoved {
            guild_id: 1,
            name: "Example".into(),
        }),
    );
    assert!(world.guild_members().is_none());
}

#[test]
fn membership_changes_and_character_admission_boundaries_forget_roster_evidence() {
    let mut world = guilded();
    for change in [ListingChange::Guild(Some(2)), ListingChange::Guild(None)] {
        listed(&mut world);
        game(
            &mut world,
            WorldEvent::Listing {
                spawn_id: 9,
                change,
            },
        );
        assert!(world.guild_members().is_none());
    }
    listed(&mut world);
    game(
        &mut world,
        WorldEvent::ListingSet {
            session_id: 1,
            change: ListingChange::Guild(Some(1)),
        },
    );
    assert!(world.guild_members().is_none());

    for reset in [
        Reset::Entered,
        Reset::Camped,
        Reset::Zoning { to_bind: false },
        Reset::Lost {
            ended: false,
            transferring: false,
        },
        Reset::Lost {
            ended: true,
            transferring: false,
        },
    ] {
        let mut world = guilded();
        listed(&mut world);
        game(&mut world, WorldEvent::Guild(add("Second")));
        world.reset(reset);
        assert!(world.guild_members().is_none(), "{reset:?}");
        assert!(world.guild_unknown_notes.is_empty(), "{reset:?}");
    }
}

#[test]
fn deltas_before_a_full_list_do_not_invent_membership() {
    let mut world = guilded();
    game(&mut world, WorldEvent::Guild(add("Second")));
    assert!(world.guild_members().is_none());
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::Members(Vec::new())),
    );
    assert_eq!(world.guild_members(), Some([].as_slice()));
}

#[test]
fn roster_updates_outside_a_connected_admission_are_ignored() {
    let mut world = guilded();
    listed(&mut world);
    connection(&mut world, false, false);
    assert!(world.guild_members().is_none());
    assert!(
        game(
            &mut world,
            WorldEvent::Guild(GuildUpdate::Members(vec![guild_member("Late")]))
        )
        .ignored
    );
    assert!(world.guild_members().is_none());
    connection(&mut world, true, false);
    assert!(world.guild_members().is_none());
    listed(&mut world);
    assert!(world.guild_members().is_some());
}

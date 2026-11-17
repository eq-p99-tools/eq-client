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

fn transfer_fixture() -> ClientWorld {
    let mut world = guilded();
    game(
        &mut world,
        WorldEvent::WorldName {
            short_name: "synthetic".into(),
        },
    );
    listed(&mut world);
    game(&mut world, WorldEvent::Guild(add("Second")));
    world
}

fn offer(to_bind: bool) -> WorldEvent {
    WorldEvent::ZoneTransfer(ZoneOffer {
        zone_id: 2,
        instance_id: 0,
        position: WorldPosition::default(),
        reason: 0,
        to_bind,
        solicited: true,
    })
}

fn arrive(world: &mut ClientWorld, player: PlayerState, zone_id: u16) {
    game(
        world,
        WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id: 2,
            zone: "qeynos2".into(),
            zone_id,
            player: Box::new(player),
            far_clip: None,
            spell_landing_flag: None,
        },
    );
    connection(world, true, false);
}

#[test]
fn ordinary_transfer_keeps_received_roster_and_note_provenance_for_the_same_character() {
    let mut world = transfer_fixture();
    let before = world.guild_members().unwrap().to_vec();
    let mut player = world.player().cloned().unwrap();
    // A zone changes the spawn ID; the character and guild are the identity.
    player.spawn_id = 44;
    game(&mut world, offer(false));
    assert_eq!(world.guild_members(), Some(before.as_slice()));
    connection(&mut world, false, false);
    assert_eq!(world.guild_members(), Some(before.as_slice()));
    arrive(&mut world, player, 2);
    assert_eq!(world.guild_members(), Some(before.as_slice()));
    assert_eq!(world.guild_member_note("Example"), Some(""));
    assert_eq!(world.guild_member_note("Second"), None);
    game(
        &mut world,
        WorldEvent::Guild(GuildUpdate::MemberNote {
            guild_id: 1,
            name: "Second".into(),
            note: "New report".into(),
        }),
    );
    assert_eq!(world.guild_member_note("Second"), Some("New report"));
}

#[test]
fn a_refused_ordinary_transfer_resumes_the_same_received_roster() {
    let mut world = transfer_fixture();
    let before = world.guild_members().unwrap().to_vec();
    game(&mut world, offer(false));
    game(
        &mut world,
        WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        },
    );
    assert!(world.in_world());
    assert_eq!(world.guild_members(), Some(before.as_slice()));
    assert_eq!(world.guild_member_note("Second"), None);
}

#[test]
fn transfer_evidence_does_not_cross_world_character_guild_or_destination_boundaries() {
    for case in 0..7 {
        let mut world = transfer_fixture();
        let mut player = world.player().cloned().unwrap();
        game(&mut world, offer(case == 5));
        let mut zone_id = 2;
        match case {
            0 => player.name = "Other".into(),
            1 => player.listing.guild = Some(2),
            2 => zone_id = 3,
            3 => {
                game(
                    &mut world,
                    WorldEvent::WorldName {
                        short_name: "other-world".into(),
                    },
                );
            }
            4 => world.world_name = None,
            5 => (), // A bind/death transfer is outside this ordinary-transfer scope.
            6 => {
                connection(&mut world, false, true);
            }
            _ => unreachable!(),
        }
        arrive(&mut world, player, zone_id);
        assert!(world.guild_members().is_none(), "case {case}");
        assert!(world.guild_unknown_notes.is_empty(), "case {case}");
    }
}

#[test]
fn character_selection_clears_roster_even_without_a_camp_event() {
    let mut world = transfer_fixture();
    game(
        &mut world,
        WorldEvent::CharacterSelection {
            connection_id: 2,
            selection_id: 2,
            characters: Vec::new(),
            creation: None,
            deletion: None,
        },
    );
    assert!(world.guild_members().is_none());
    assert!(world.guild_unknown_notes.is_empty());
}

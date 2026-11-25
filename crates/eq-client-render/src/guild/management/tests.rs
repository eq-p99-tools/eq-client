use super::*;
use crate::{clicks::testing as clicks, online::testing};
use eq_client_core::{
    WorldEvent,
    guild::{GuildMember, GuildTribute, GuildUpdate},
    listing::ListingChange,
};
use std::sync::mpsc::{self, Receiver};

fn member(name: &str, rank: GuildRank) -> GuildMember {
    GuildMember {
        name: name.into(),
        level: 1,
        class: 1,
        rank,
        banker: false,
        alt: false,
        last_on: 0,
        tribute: GuildTribute {
            on: false,
            total: 0,
            last: 0,
        },
        note: String::new(),
        zone_id: Some(1),
    }
}

fn online(actor_rank: GuildRank, target_rank: GuildRank) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.listing.guild = Some(3);
    player.listing.guild_rank = Some(actor_rank);
    testing::admit(&mut online, 11, player);
    let mut spawn = testing::pet(8, 7);
    spawn.kind = eq_client_core::SpawnKind::Player;
    spawn.name = "Namedmember".into();
    spawn.pet_owner = None;
    testing::news(
        &mut online,
        [
            WorldEvent::Spawns(vec![spawn]),
            WorldEvent::Guild(GuildUpdate::Members(vec![
                member("Example", actor_rank),
                member("Namedmember", target_rank),
            ])),
        ],
    );
    online
}

fn selected(online: &OnlineState) -> State {
    State {
        selected: Selection::new(online.world(), "Namedmember"),
        ..default()
    }
}

fn queue() -> (Outbox, Receiver<ClientCommand>) {
    let (sender, receiver) = mpsc::sync_channel(8);
    (Outbox::new(Some(sender)), receiver)
}

fn rank(online: &mut OnlineState, name: &str, rank: GuildRank) {
    testing::news(
        online,
        [WorldEvent::Guild(GuildUpdate::MemberRank {
            guild_id: Some(3),
            name: name.into(),
            rank,
            banker: false,
            alt: false,
        })],
    );
}

#[test]
fn guild_management_known_ranks_and_server_capability_guard_each_action() {
    use GuildRank::{Leader, Member, Officer, Other};
    for (actor, target, expected) in [
        (Member, Member, [false, false, false]),
        (Officer, Member, [true, false, false]),
        (Officer, Officer, [true, false, false]),
        (Leader, Member, [true, true, false]),
        (Leader, Officer, [true, false, true]),
        (Leader, Leader, [false, false, false]),
        (Other(9), Member, [false, false, false]),
        (Leader, Other(9), [false, false, false]),
    ] {
        let online = online(actor, target);
        let state = selected(&online);
        for (action, allowed) in [Action::Remove, Action::Promote, Action::Demote]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                state.reason(online.world(), action).is_ok(),
                allowed,
                "{actor:?} {target:?} {action:?}"
            );
        }
    }
    let mut online = online(Leader, Member);
    let state = State {
        selected: Selection::new(online.world(), "Example"),
        ..default()
    };
    assert!(state.reason(online.world(), Action::Remove).is_err());
    testing::news(&mut online, [WorldEvent::Despawn(8)]);
    assert!(
        selected(&online)
            .reason(online.world(), Action::Promote)
            .is_err()
    );
    let mut player = testing::player(7);
    player.listing.guild = Some(3);
    player.listing.guild_rank = Some(Leader);
    testing::enter_with(&mut online, 12, player, None, vec![Capability::GuildRoster]);
    testing::news(
        &mut online,
        [WorldEvent::Guild(GuildUpdate::Members(vec![member(
            "Namedmember",
            Member,
        )]))],
    );
    assert!(
        selected(&online)
            .reason(online.world(), Action::Remove)
            .is_err()
    );
}

#[test]
fn guild_management_cancel_and_confirmation_send_only_the_frozen_canonical_target() {
    let online = online(GuildRank::Leader, GuildRank::Member);
    let mut state = selected(&online);
    let mut asked = Asked::default();
    let (outbox, sent) = queue();
    state.begin(online.world(), &mut asked, Action::Remove);
    assert!(state.question(state.serial).contains("Remove Namedmember"));
    assert!(sent.try_recv().is_err());
    asked.give(false);
    state.answer(online.world(), &mut asked, &outbox, Instant::now());
    assert!(sent.try_recv().is_err());
    state.begin(online.world(), &mut asked, Action::Remove);
    asked.give(true);
    state.answer(online.world(), &mut asked, &outbox, Instant::now());
    assert!(
        matches!(sent.try_recv().unwrap(), ClientCommand::GuildRemove { session_id: 11, name } if name == "Namedmember")
    );
    assert!(state.note.contains("queued"));
    assert!(state.pending.is_some());
    state.begin(online.world(), &mut asked, Action::Remove);
    assert!(asked.question().is_none());
    assert!(sent.try_recv().is_err());
}

#[test]
fn guild_management_cannot_override_another_question_or_use_a_stale_actor_rank() {
    let mut online = online(GuildRank::Leader, GuildRank::Member);
    let mut state = selected(&online);
    let mut asked = Asked::default();
    assert!(asked.ask(Question::Surname));
    state.begin(online.world(), &mut asked, Action::Remove);
    assert_eq!(asked.question(), Some(Question::Surname));
    assert!(state.asking.is_none());
    asked.give(true);
    state.begin(online.world(), &mut asked, Action::Remove);
    assert!(state.asking.is_none());
    assert_eq!(asked.answer(Question::Surname), Some(true));
    rank(&mut online, "Example", GuildRank::Member);
    let state = selected(&online);
    assert!(state.reason(online.world(), Action::Remove).is_err());
}

#[test]
fn guild_management_changed_membership_rank_actor_admission_and_close_withdraw_intent() {
    for boundary in 0..5 {
        let mut online = online(GuildRank::Leader, GuildRank::Member);
        let mut state = selected(&online);
        let mut asked = Asked::default();
        let (outbox, sent) = queue();
        state.begin(online.world(), &mut asked, Action::Remove);
        asked.give(true);
        match boundary {
            0 => {
                // Remove/readd with the identical final member must still invalidate.
                testing::news(
                    &mut online,
                    [
                        WorldEvent::Guild(GuildUpdate::MemberRemoved {
                            guild_id: 3,
                            name: "Namedmember".into(),
                        }),
                        WorldEvent::Guild(GuildUpdate::MemberAdded {
                            guild_id: 3,
                            name: "Namedmember".into(),
                            level: 1,
                            class: 1,
                            rank: GuildRank::Member,
                            zone_id: Some(1),
                            last_on: 0,
                        }),
                    ],
                );
            }
            1 => rank(&mut online, "Namedmember", GuildRank::Officer),
            2 => testing::news(
                &mut online,
                [WorldEvent::Listing {
                    spawn_id: 7,
                    change: ListingChange::GuildRank(GuildRank::Member),
                }],
            ),
            3 => {
                let mut player = testing::player(7);
                player.listing.guild = Some(3);
                player.listing.guild_rank = Some(GuildRank::Leader);
                testing::admit(&mut online, 12, player);
            }
            _ => {}
        }
        state.advance(online.world(), &mut asked, boundary != 4, Instant::now());
        state.answer(online.world(), &mut asked, &outbox, Instant::now());
        assert!(sent.try_recv().is_err(), "boundary {boundary}");
        assert!(state.asking.is_none());
    }
}

#[test]
fn guild_management_waits_for_received_effect_and_keeps_refusal_timeout_unconfirmed() {
    for action in [Action::Remove, Action::Promote, Action::Demote] {
        let initial = if action == Action::Demote {
            GuildRank::Officer
        } else {
            GuildRank::Member
        };
        let mut online = online(GuildRank::Leader, initial);
        let mut state = selected(&online);
        let mut asked = Asked::default();
        let (outbox, _sent) = queue();
        let now = Instant::now();
        state.begin(online.world(), &mut asked, action);
        asked.give(true);
        state.answer(online.world(), &mut asked, &outbox, now);
        // Queue acceptance and unrelated roster traffic do not establish an effect.
        state.advance(online.world(), &mut asked, true, now);
        assert!(state.pending.is_some());
        rank(&mut online, "Example", GuildRank::Leader);
        state.advance(online.world(), &mut asked, true, now);
        assert!(state.pending.is_some());
        match action {
            Action::Remove => testing::news(
                &mut online,
                [WorldEvent::Guild(GuildUpdate::MemberRemoved {
                    guild_id: 3,
                    name: "Namedmember".into(),
                })],
            ),
            Action::Promote => rank(&mut online, "Namedmember", GuildRank::Officer),
            Action::Demote => rank(&mut online, "Namedmember", GuildRank::Member),
        }
        state.advance(online.world(), &mut asked, true, now);
        assert!(state.pending.is_none());
        assert!(state.note.starts_with("Server roster confirms"));
    }
    for refused in [false, true] {
        let mut online = online(GuildRank::Officer, GuildRank::Member);
        let mut state = selected(&online);
        let mut asked = Asked::default();
        let (outbox, _sent) = queue();
        let now = Instant::now();
        state.begin(online.world(), &mut asked, Action::Remove);
        asked.give(true);
        state.answer(online.world(), &mut asked, &outbox, now);
        // A foreign admission's refusal must not alter this request.
        testing::news(
            &mut online,
            [WorldEvent::GuildRefused {
                session_id: 99,
                reason: "foreign".into(),
                string_id: None,
            }],
        );
        state.advance(online.world(), &mut asked, true, now);
        assert!(state.pending.is_some());
        if refused {
            testing::news(
                &mut online,
                [WorldEvent::GuildRefused {
                    session_id: 11,
                    reason: "denied".into(),
                    string_id: None,
                }],
            );
        }
        state.advance(
            online.world(),
            &mut asked,
            true,
            now + Duration::from_secs(31),
        );
        assert!(state.pending.is_none());
        assert!(!state.note.contains("Server roster confirms"));
        assert_eq!(online.world().guild_members().unwrap().len(), 2);
    }
}

#[test]
fn guild_management_real_roster_click_and_button_share_the_guarded_confirmation_route() {
    let mut app = App::new();
    let online = online(GuildRank::Leader, GuildRank::Member);
    let mut shown = Shown::default();
    shown.open(WindowId::Guild);
    let (outbox, sent) = queue();
    app.insert_resource(online)
        .insert_resource(shown)
        .insert_resource(outbox)
        .init_resource::<State>()
        .init_resource::<Asked>()
        .add_systems(
            Update,
            (
                super::super::roster::window,
                super::super::roster::fill,
                input,
                present,
            )
                .chain(),
        );
    clicks::install(&mut app);
    app.update();
    let world = app.world_mut();
    let row = world
        .query::<(Entity, &Member)>()
        .iter(world)
        .find(|(_, row)| row.name == "Namedmember")
        .unwrap()
        .0;
    let remove = world
        .query::<(Entity, &Action)>()
        .iter(world)
        .find(|(_, action)| **action == Action::Remove)
        .unwrap()
        .0;
    clicks::click(&mut app, row);
    clicks::click(&mut app, remove);
    assert!(matches!(
        app.world().resource::<Asked>().question(),
        Some(Question::GuildManagement { .. })
    ));
    assert!(sent.try_recv().is_err());
    app.world_mut().resource_mut::<Asked>().give(true);
    app.update();
    assert!(
        matches!(sent.try_recv().unwrap(), ClientCommand::GuildRemove { name, .. } if name == "Namedmember")
    );
    assert!(sent.try_recv().is_err());
}

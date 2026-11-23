use super::*;
use crate::online::{OnlineState, testing};
use eq_client_core::{Capability, SpawnState, WorldEvent};

/// A world of this generation with the player (7, "Example") and others:
/// a player, a creature, a player's corpse, a creature's corpse and the
/// player's own corpse.
fn zone(generation: GameDialect) -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::generation(&mut online, generation);
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities: vec![Capability::Talking],
            choices: Vec::new(),
            session_id: 5,
            zone: "qeytoqrg".into(),
            player: Box::new(testing::player(7)),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        }],
    );
    testing::connect(&mut online, true);
    let spawn = |spawn_id, name: &str, kind| SpawnState {
        name: name.into(),
        kind,
        pet_owner: None,
        ..testing::pet(spawn_id, 7)
    };
    testing::news(
        &mut online,
        [WorldEvent::Spawns(vec![
            spawn(8, "Friend", SpawnKind::Player),
            spawn(9, "a_guard00", SpawnKind::Npc),
            spawn(10, "Someone's corpse0", SpawnKind::PlayerCorpse),
            spawn(11, "a_rat's_corpse01", SpawnKind::NpcCorpse),
            spawn(12, "Example's corpse0", SpawnKind::PlayerCorpse),
            spawn(13, "Guard Rolan", SpawnKind::Npc),
        ])],
    );
    online
}

fn aimed(online: &mut OnlineState, target: Option<u16>, line: &str) -> Option<Targeted> {
    online.select_target(target);
    targeted(line, online.world())
}

fn tell(line: &str) -> Targeted {
    Targeted::Tell(Ok(line.to_owned()))
}

#[test]
fn titanium_ttell_follows_the_programs_tests_by_target_kind() {
    let mut online = zone(GameDialect::Titanium);
    assert_eq!(aimed(&mut online, None, "/tell Friend hi"), None);
    assert_eq!(aimed(&mut online, None, "hello"), None);
    // No target, or no words: nothing said and nothing sent.
    assert_eq!(
        aimed(&mut online, None, "/ttell hi"),
        Some(Targeted::Nothing)
    );
    for line in ["/ttell", "/ttell   "] {
        assert_eq!(
            aimed(&mut online, Some(8), line),
            Some(Targeted::Nothing),
            "{line}"
        );
    }
    // Another player, by the name the server sent.
    assert_eq!(
        aimed(&mut online, Some(8), "/TTELL   hi  there "),
        Some(tell("/tell Friend hi  there"))
    );
    // A creature's name has a `_`: refused with the program's line.
    assert_eq!(
        aimed(&mut online, Some(9), "/ttell hi"),
        Some(Targeted::Say(Line::Refused))
    );
    // A player's corpse: cut at the apostrophe, so the owner gets the tell.
    assert_eq!(
        aimed(&mut online, Some(10), "/ttell hi"),
        Some(tell("/tell Someone hi"))
    );
    // Type 3 keeps its name as it stands, `_` and all; the server answers.
    assert_eq!(
        aimed(&mut online, Some(11), "/ttell hi"),
        Some(tell("/tell a_rat's_corpse01 hi"))
    );
    // The player themself, and their own corpse, come to their own name.
    for target in [7, 12] {
        assert_eq!(
            aimed(&mut online, Some(target), "/ttell hi"),
            Some(Targeted::Say(Line::ToSelf)),
            "{target}"
        );
    }
    // This client's guard: a name that cannot be carried as a typed line.
    assert_eq!(
        aimed(&mut online, Some(13), "/ttell hi"),
        Some(Targeted::Say(Line::Refused))
    );
}

#[test]
fn the_name_tests_match_the_read() {
    assert_eq!(recipient("Friend", SpawnKind::Player), Some("Friend"));
    assert_eq!(recipient("GM#Name", SpawnKind::Player), None);
    assert_eq!(recipient("Guardbob", SpawnKind::Npc), Some("Guardbob"));
    assert_eq!(recipient("a_bat", SpawnKind::Npc), None);
    assert_eq!(
        recipient("Owner's corpse", SpawnKind::PlayerCorpse),
        Some("Owner")
    );
    assert_eq!(recipient("a_bat's_corpse", SpawnKind::PlayerCorpse), None);
    assert_eq!(
        recipient("a_bat's_corpse", SpawnKind::NpcCorpse),
        Some("a_bat's_corpse")
    );
    assert_eq!(recipient("", SpawnKind::NpcCorpse), None);
    assert_eq!(recipient("", SpawnKind::Player), None);
    assert_eq!(recipient("Odd's", SpawnKind::Unknown(9)), Some("Odd"));
}

#[test]
fn titanium_whotarget_lists_any_target_by_its_name_and_ignores_words() {
    let mut online = zone(GameDialect::Titanium);
    assert_eq!(
        aimed(&mut online, None, "/whotarget"),
        Some(Targeted::Nothing)
    );
    // The program runs its `/who` word parser on the name, so it lists as
    // a typed `/who <name>` does.
    let who = |text: &str| {
        Some(Targeted::Who(
            eq_client_core::who::parse(text).map(|request| request.filter),
        ))
    };
    assert_eq!(
        who("Friend"),
        Some(Targeted::Who(
            crate::chat::zone_who_request("/who Friend").unwrap()
        ))
    );
    for line in ["/whotarget", "/whotarget all wiz"] {
        assert_eq!(aimed(&mut online, Some(8), line), who("Friend"), "{line}");
    }
    assert_eq!(aimed(&mut online, Some(9), "/whotarget"), who("a_guard00"));
    assert_eq!(aimed(&mut online, Some(7), "/whotarget"), who("Example"));
}

#[test]
fn the_unread_programs_keep_this_clients_own_rules() {
    for generation in [GameDialect::EqMac, GameDialect::Rof2] {
        let mut online = zone(generation);
        let refused = Some(Targeted::Tell(Err(OWN_REFUSAL.to_owned())));
        assert_eq!(aimed(&mut online, None, "/ttell hi"), refused);
        assert_eq!(
            aimed(&mut online, None, "/whotarget"),
            Some(Targeted::Who(Err(OWN_UNSEEN.to_owned())))
        );
        assert_eq!(
            aimed(&mut online, Some(8), "/ttell hi"),
            Some(tell("/tell Friend hi"))
        );
        for target in [7, 9, 10, 11] {
            assert_eq!(
                aimed(&mut online, Some(target), "/ttell hi"),
                refused,
                "{generation:?} {target}"
            );
        }
    }
}

#[test]
fn ttell_says_the_programs_lines() {
    for (line, fallback) in [
        (Line::Refused, "You cannot send a tell to that target."),
        (Line::ToSelf, "You cannot send a tell to yourself."),
    ] {
        let mut app = App::new();
        app.init_resource::<ChatState>();
        app.add_systems(Update, say);
        app.world_mut().resource_mut::<ChatState>().targeted_line = Some(line);
        app.update();
        let chat = app.world().resource::<ChatState>();
        assert!(chat.targeted_line.is_none());
        let said = format!(
            "{:?}",
            chat.history.lines(eq_client_core::chat::ChatTab::All)
        );
        assert!(said.contains(fallback), "{said}");
    }
}

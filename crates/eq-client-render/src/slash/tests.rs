use super::*;
use crate::online::{OnlineState, testing};
use eq_client_core::{WorldEvent, world::SpellCatalog};

/// Spell data that knows one spell's effects: 500 summons a mount, 501
/// doesn't.
struct Spells;

impl SpellCatalog for Spells {
    fn timing(&self, _spell: u32) -> Option<eq_client_core::world::SpellTiming> {
        None
    }

    fn instant_effect(&self, _spell: u32) -> bool {
        false
    }

    fn feigns(&self, _spell: u32) -> bool {
        false
    }

    fn effect_ids(&self, spell: u32) -> Option<[u32; 12]> {
        let mut effects = [254; 12];
        effects[0] = match spell {
            500 => SUMMON_MOUNT,
            501 => 0,
            _ => return None,
        };
        Some(effects)
    }
}

fn admitted(capabilities: Vec<Capability>) -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities,
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
    online
}

fn holding(online: &mut OnlineState, spell_id: u32) {
    let buff = eq_client_core::Buff {
        spell_id,
        caster_level: 20,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks: 4,
        counters: 0,
        caster_id: 7,
    };
    testing::news(online, [WorldEvent::BuffSnapshot(vec![Some(buff)])]);
}

#[test]
fn the_words_are_the_commands_spelled_out_and_ignore_what_follows() {
    assert_eq!(Asked::typed("/yell"), Some(Asked::Yell));
    assert_eq!(Asked::typed("/YELL for help"), Some(Asked::Yell));
    assert_eq!(Asked::typed("/stopcast"), Some(Asked::StopCast));
    assert_eq!(Asked::typed("/stopsong"), None);
    assert_eq!(Asked::typed("yell"), None);
    // A typed abbreviation reaches here spelled out.
    assert_eq!(
        crate::commands::spelled("/stopc").as_deref(),
        Some("/stopcast")
    );
}

#[test]
fn a_yell_says_its_line_and_sends_with_no_target_or_posture_test() {
    let online = admitted(vec![Capability::Yelling]);
    let (line, send) = answer(Asked::Yell, online.world(), None, &Messages::default()).unwrap();
    assert_eq!(line.unwrap().words.text, "You yell for help.");
    assert_eq!(send, Some(Sent::Yell));
}

#[test]
fn a_server_without_the_send_keeps_todays_answer() {
    let online = admitted(Vec::new());
    for asked in [
        Asked::Yell,
        Asked::StopCast,
        Asked::Duel { named: false },
        Asked::Lfg(LfgWords::None),
    ] {
        let refusal =
            answer(asked, online.world(), Some(&Spells), &Messages::default()).unwrap_err();
        assert_eq!(
            refusal.words.text,
            format!("This client does not take /{} that way yet.", asked.name())
        );
    }
}

#[test]
fn a_cast_stop_sends_unless_a_mount_is_summoned() {
    let mut online = admitted(vec![Capability::StoppingCasts]);
    let stop = || {
        answer(
            Asked::StopCast,
            online.world(),
            Some(&Spells),
            &Messages::default(),
        )
    };
    assert_eq!(stop(), Ok((None, Some(Sent::StopCast))));
    holding(&mut online, 501);
    assert_eq!(
        answer(
            Asked::StopCast,
            online.world(),
            Some(&Spells),
            &Messages::default()
        ),
        Ok((None, Some(Sent::StopCast)))
    );
    // A summoned mount: the program's line, nothing sent (6.487).
    holding(&mut online, 500);
    let refusal = answer(
        Asked::StopCast,
        online.world(),
        Some(&Spells),
        &Messages::default(),
    )
    .unwrap_err();
    assert_eq!(refusal.words.text, "You can't stop casting now.");
    // A buff whose effects can't be read counts as a mount.
    holding(&mut online, 502);
    assert!(
        answer(
            Asked::StopCast,
            online.world(),
            Some(&Spells),
            &Messages::default()
        )
        .is_err()
    );
    assert!(answer(Asked::StopCast, online.world(), None, &Messages::default()).is_err());
}

#[test]
fn the_answer_system_sends_once_and_says_the_line() {
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted(vec![
        Capability::Yelling,
        Capability::StoppingCasts,
    ]))
    .insert_resource(Outbox::new(Some(queue)))
    .init_resource::<ChatState>()
    .add_systems(Update, answers);
    for (asked, expected) in [
        (
            Asked::Yell,
            ClientCommand::YellForHelp {
                session_id: 5,
                created: std::time::Instant::now(),
            },
        ),
        (
            Asked::StopCast,
            ClientCommand::StopCasting {
                session_id: 5,
                created: std::time::Instant::now(),
            },
        ),
    ] {
        app.world_mut().resource_mut::<ChatState>().slash = Some(asked);
        app.update();
        let sent = received.try_recv().unwrap();
        assert_eq!(
            std::mem::discriminant(&sent),
            std::mem::discriminant(&expected)
        );
        assert_eq!(sent.session_id(), Some(5));
        assert!(received.try_recv().is_err());
        app.update();
        assert!(received.try_recv().is_err());
    }
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        "You yell for help."
    );
}

#[test]
fn a_plain_assist_takes_the_target_and_says_two_lines_without_one() {
    assert_eq!(Asked::typed("/assist"), Some(Asked::Assist));
    assert_eq!(Asked::typed("/ASSIST "), Some(Asked::Assist));
    // An option, a main-assist word or a name is the chat's own form.
    assert_eq!(Asked::typed("/assist Friend"), None);
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted(vec![Capability::Assisting]))
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answers);
    app.world_mut().resource_mut::<ChatState>().slash = Some(Asked::Assist);
    app.update();
    assert!(received.try_recv().is_err());
    let lines: Vec<String> = app
        .world()
        .resource::<ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::All)
        .into_iter()
        .map(|(_, line)| line.message.text.clone())
        .collect();
    assert_eq!(lines, [UNASSISTED[0].1, UNASSISTED[1].1]);
    app.world_mut()
        .resource_mut::<OnlineState>()
        .select_target(Some(9));
    app.world_mut().resource_mut::<ChatState>().slash = Some(Asked::Assist);
    app.update();
    assert!(matches!(
        received.try_recv().unwrap(),
        ClientCommand::Assist {
            session_id: 5,
            spawn_id: 9
        }
    ));
}

#[test]
fn the_mac_era_client_keeps_its_earlier_no_target_answer() {
    let lines = unassisted(
        eq_client_assets::ui::InstalledClient::EqMac,
        &Messages::default(),
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].words.text, NO_TARGET);
}

#[test]
fn a_server_without_assisting_keeps_the_answers_from_before_r46() {
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted(Vec::new()))
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answers);
    let ask = |app: &mut App| {
        app.world_mut().resource_mut::<ChatState>().slash = Some(Asked::Assist);
        app.update();
        assert!(received.try_recv().is_err());
        app.world().resource::<ChatState>().newest()
    };
    assert_eq!(ask(&mut app), NO_TARGET);
    app.world_mut()
        .resource_mut::<OnlineState>()
        .select_target(Some(9));
    assert_eq!(ask(&mut app), crate::outbox::UNAVAILABLE);
}

/// An admitted world where the player (7) targets `target`, with another
/// player (12), an NPC (20) and a player's corpse (21) in the zone.
fn targeting(target: Option<u16>) -> OnlineState {
    use eq_client_core::SpawnKind;
    let mut online = admitted(vec![Capability::Dueling]);
    let spawns = [
        (12, SpawnKind::Player),
        (20, SpawnKind::Npc),
        (21, SpawnKind::PlayerCorpse),
    ]
    .map(|(id, kind)| {
        let mut spawn = testing::pet(id, 0);
        spawn.pet_owner = None;
        spawn.kind = kind;
        spawn
    });
    testing::spawns(&mut online, spawns.to_vec());
    testing::news(
        &mut online,
        [WorldEvent::TargetChanged {
            session_id: 5,
            request_id: 0,
            state: eq_client_core::targets::TargetState {
                revision: 1,
                spawn_id: target,
                available: true,
            },
        }],
    );
    online
}

#[test]
fn the_duel_word_notes_any_words_after_it() {
    assert_eq!(Asked::typed("/duel"), Some(Asked::Duel { named: false }));
    assert_eq!(Asked::typed("/DUEL  "), Some(Asked::Duel { named: false }));
    assert_eq!(
        Asked::typed("/duel Somebody"),
        Some(Asked::Duel { named: true })
    );
}

#[test]
fn a_titanium_duel_sends_the_target_as_it_stands_whatever_follows() {
    let duel = |online: &OnlineState, named| {
        answer(
            Asked::Duel { named },
            online.world(),
            None,
            &Messages::default(),
        )
    };
    // The program reads no words and tests nothing itself (read R49-6h):
    // the session words each outcome.
    for (target, sent) in [
        (Some(12), 12),
        (Some(7), 7),
        (Some(20), 20),
        (Some(21), 21),
        (None, 0),
    ] {
        let online = targeting(target);
        for named in [false, true] {
            assert_eq!(
                duel(&online, named),
                Ok((None, Some(Sent::Duel { target: sent }))),
                "{target:?} {named}"
            );
        }
    }
}

#[test]
fn a_mac_duel_sends_the_target_as_it_stands_and_refuses_a_name() {
    let duel = |online: &OnlineState, named| {
        answer(
            Asked::Duel { named },
            online.world(),
            None,
            &Messages::default(),
        )
    };
    let mac = |target| {
        let mut online = targeting(target);
        testing::generation(&mut online, eq_client_core::GameDialect::EqMac);
        online
    };
    // The program takes any target and tests none first itself (TAKP read
    // 11g): the session refuses no target, the player and a corpse with
    // the program's lines, and challenges an NPC.
    for (target, sent) in [
        (Some(12), 12),
        (Some(7), 7),
        (Some(20), 20),
        (Some(21), 21),
        (None, 0),
    ] {
        assert_eq!(
            duel(&mac(target), false),
            Ok((None, Some(Sent::Duel { target: sent }))),
            "{target:?}"
        );
    }
    // Its words after `/duel` are unread: they send nothing.
    assert_eq!(
        duel(&mac(Some(12)), true).unwrap_err().words.text,
        "This client does not take a name after /duel yet; target the player and type /duel."
    );
}

#[test]
fn a_duel_elsewhere_challenges_only_another_player_targeted_and_never_by_name() {
    let duel = |online: &OnlineState, named| {
        answer(
            Asked::Duel { named },
            online.world(),
            None,
            &Messages::default(),
        )
    };
    let rof2 = |target| {
        let mut online = targeting(target);
        testing::generation(&mut online, eq_client_core::GameDialect::Rof2);
        online
    };
    // Another player targeted: sent, with no line of ours.
    let online = rof2(Some(12));
    assert_eq!(
        duel(&online, false),
        Ok((None, Some(Sent::Duel { target: 12 })))
    );
    // Words after it send nothing, even with a player targeted.
    assert_eq!(
        duel(&online, true).unwrap_err().words.text,
        "This client does not take a name after /duel yet; target the player and type /duel."
    );
    // No target, the player, an NPC, a corpse, or a spawn not in the zone.
    for target in [None, Some(7), Some(20), Some(21), Some(99)] {
        let online = rof2(target);
        assert_eq!(
            duel(&online, false).unwrap_err().words.text,
            "You must first target another player to challenge to a duel.",
            "{target:?}"
        );
    }
}

#[test]
fn decline_goes_to_the_session_where_it_is_offered() {
    assert_eq!(Asked::typed("/decline"), Some(Asked::Decline));
    assert_eq!(Asked::typed("/DECLINE now"), Some(Asked::Decline));
    let decline =
        |online: &OnlineState| answer(Asked::Decline, online.world(), None, &Messages::default());
    // The session says the program's line for each state.
    let online = admitted(vec![Capability::DecliningDuels]);
    assert_eq!(decline(&online), Ok((None, Some(Sent::Decline))));
    // Titanium's session offers no `/decline`.
    let online = admitted(vec![Capability::Dueling]);
    assert_eq!(
        decline(&online).unwrap_err().words.text,
        "This client does not take /decline that way yet."
    );
}

#[test]
fn the_answer_system_sends_a_challenge_once() {
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(targeting(Some(12)))
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answers);
    app.world_mut().resource_mut::<ChatState>().slash = Some(Asked::Duel { named: false });
    app.update();
    assert!(matches!(
        received.try_recv(),
        Ok(ClientCommand::ChallengeDuel {
            session_id: 5,
            target: 12,
            ..
        })
    ));
    app.update();
    assert!(received.try_recv().is_err());
}

#[test]
fn lfg_takes_alone_on_or_off_and_refuses_any_other_word() {
    assert_eq!(Asked::typed("/lfg"), Some(Asked::Lfg(LfgWords::None)));
    assert_eq!(Asked::typed("/LFG On"), Some(Asked::Lfg(LfgWords::On)));
    assert_eq!(Asked::typed("/lfg off"), Some(Asked::Lfg(LfgWords::Off)));
    assert_eq!(
        Asked::typed("/lfg 20 30"),
        Some(Asked::Lfg(LfgWords::Other))
    );
    assert_eq!(
        Asked::typed("/lfg on now"),
        Some(Asked::Lfg(LfgWords::Other))
    );
    // On Titanium, on and off say the program's line as they send, and
    // anything else is its usage line (read R49-6i).
    let titanium = admitted(vec![Capability::LookingForGroup]);
    let messages = Messages::parse("EQST0002\n0 3\n13134 On.\n13135 Off.\n13136 Usage.\n");
    let said = |words| answer(Asked::Lfg(words), titanium.world(), None, &messages);
    assert_eq!(
        said(LfgWords::On),
        Ok((
            Some(Said::official("On.").typed(0x111)),
            Some(Sent::Looking(Some(true)))
        ))
    );
    assert_eq!(
        said(LfgWords::Off),
        Ok((
            Some(Said::official("Off.").typed(0x111)),
            Some(Sent::Looking(Some(false)))
        ))
    );
    for words in [LfgWords::None, LfgWords::Other] {
        assert_eq!(
            said(words),
            Ok((Some(Said::official("Usage.").typed(0x111)), None))
        );
    }
    // Elsewhere this client's forms stand.
    let mut online = admitted(vec![Capability::LookingForGroup]);
    testing::generation(&mut online, eq_client_core::GameDialect::EqMac);
    let lfg = |words| {
        answer(
            Asked::Lfg(words),
            online.world(),
            None,
            &Messages::default(),
        )
    };
    assert_eq!(lfg(LfgWords::None), Ok((None, Some(Sent::Looking(None)))));
    assert_eq!(
        lfg(LfgWords::On),
        Ok((None, Some(Sent::Looking(Some(true)))))
    );
    assert_eq!(
        lfg(LfgWords::Off),
        Ok((None, Some(Sent::Looking(Some(false)))))
    );
    assert_eq!(
        lfg(LfgWords::Other).unwrap_err().words.text,
        "This client takes /lfg alone, /lfg on or /lfg off."
    );
}

#[test]
fn the_answer_system_sends_lfg_once() {
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(admitted(vec![Capability::LookingForGroup]))
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answers);
    app.world_mut().resource_mut::<ChatState>().slash = Some(Asked::Lfg(LfgWords::On));
    app.update();
    assert!(matches!(
        received.try_recv(),
        Ok(ClientCommand::LookForGroup {
            session_id: 5,
            on: Some(true)
        })
    ));
    app.update();
    assert!(received.try_recv().is_err());
}

fn named_spawn(spawn_id: u16, name: &str) -> eq_client_core::SpawnState {
    eq_client_core::SpawnState {
        name: name.into(),
        pet_owner: None,
        ..testing::pet(spawn_id, 0)
    }
}

/// An app that answers `/assist` with words, on a server that takes
/// assists, with a Friendly (20), a Friend (21) and the player (7).
fn assisting() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let mut online = admitted(vec![Capability::Assisting]);
    testing::spawns(
        &mut online,
        vec![named_spawn(20, "Friendly000"), named_spawn(21, "Friend000")],
    );
    online.select_target(Some(20));
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(queue)))
        .insert_resource(crate::options::OptionsState::default())
        .init_resource::<ChatState>()
        .add_systems(Update, assist_named);
    (app, received)
}

fn assist_with(app: &mut App, words: &str) -> Vec<String> {
    app.world_mut().resource_mut::<ChatState>().assist_words = Some(words.into());
    app.update();
    app.world()
        .resource::<ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::All)
        .into_iter()
        .map(|(_, line)| line.message.text.clone())
        .collect()
}

#[test]
fn assist_with_a_name_takes_the_shortest_prefix_match_and_keeps_the_target() {
    let (mut app, received) = assisting();
    assert_eq!(assist_with(&mut app, "FRI"), Vec::<String>::new());
    assert!(matches!(
        received.try_recv().unwrap(),
        ClientCommand::Assist {
            session_id: 5,
            spawn_id: 21
        }
    ));
    assert_eq!(assist_with(&mut app, "friendl"), Vec::<String>::new());
    assert!(matches!(
        received.try_recv().unwrap(),
        ClientCommand::Assist { spawn_id: 20, .. }
    ));
    // The assist doesn't change the target; its answer does.
    assert_eq!(
        app.world()
            .resource::<OnlineState>()
            .world()
            .target()
            .selected,
        Some(20)
    );
}

#[test]
fn assist_naming_no_one_is_silent_and_naming_ourselves_says_its_line() {
    let (mut app, received) = assisting();
    assert_eq!(assist_with(&mut app, "nobody"), Vec::<String>::new());
    assert!(received.try_recv().is_err());
    assert_eq!(assist_with(&mut app, "exam"), [ASSIST_SELF.1]);
    assert!(received.try_recv().is_err());
    let chat = app.world().resource::<ChatState>();
    let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
    assert_eq!(lines[0].1.message_type, Some(ASSIST_SELF_COLOUR));
}

#[test]
fn the_option_words_set_attack_on_assist_and_say_its_state() {
    let (mut app, received) = assisting();
    assert_eq!(assist_with(&mut app, "ON"), [ATTACK_ON_ASSIST_LINES[0].1]);
    assert!(
        app.world()
            .resource::<crate::options::OptionsState>()
            .options
            .attack_on_assist
    );
    assert_eq!(
        assist_with(&mut app, "off").last().map(String::as_str),
        Some(ATTACK_ON_ASSIST_LINES[1].1)
    );
    assert!(
        !app.world()
            .resource::<crate::options::OptionsState>()
            .options
            .attack_on_assist
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn the_mac_era_client_keeps_assisting_a_player_by_name() {
    let (mut app, received) = assisting();
    app.insert_resource(crate::ViewerSettings(crate::ViewerConfig {
        installed_client: eq_client_assets::ui::InstalledClient::EqMac,
        ..crate::ViewerConfig::default()
    }));
    // Not a player in the zone: this client's earlier refusal.
    assert_eq!(assist_with(&mut app, "fri").len(), 1);
    assert!(received.try_recv().is_err());
}

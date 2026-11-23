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
fn a_duel_elsewhere_challenges_only_another_player_targeted_and_never_by_name() {
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
    // Another player targeted: sent, with no line of ours.
    let online = mac(Some(12));
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
        let online = mac(target);
        assert_eq!(
            duel(&online, false).unwrap_err().words.text,
            "You must first target another player to challenge to a duel.",
            "{target:?}"
        );
    }
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
    let online = admitted(vec![Capability::LookingForGroup]);
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

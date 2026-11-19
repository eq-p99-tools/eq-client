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
    assert_eq!(send, Some(Asked::Yell));
}

#[test]
fn a_server_without_the_send_keeps_todays_answer() {
    let online = admitted(Vec::new());
    for asked in [Asked::Yell, Asked::StopCast] {
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
    assert_eq!(stop(), Ok((None, Some(Asked::StopCast))));
    holding(&mut online, 501);
    assert_eq!(
        answer(
            Asked::StopCast,
            online.world(),
            Some(&Spells),
            &Messages::default()
        ),
        Ok((None, Some(Asked::StopCast)))
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

use super::*;
use crate::{
    online::{OnlineState, testing},
    spellbook::SpellNames,
};
use eq_client_core::{
    SpellUpdate, WorldEvent,
    spell_tables::{Binding, Context, Event, Fact, Policy, SpellId, Stage, Table, Update},
};
use std::time::{Duration, Instant};

pub(crate) fn names() -> SpellNames {
    let mut fields = vec!["0"; 16];
    fields[0] = "73";
    fields[1] = "Synthetic A";
    fields[13] = "4000";
    fields[14] = "1500";
    fields[15] = "10000";
    let a = fields.join("^");
    fields[0] = "74";
    fields[1] = "Synthetic B";
    let mut names = SpellNames::parse(&format!("{a}\n{}", fields.join("^")));
    let binding = Binding::new(Policy::Project1999);
    names.bind(binding);
    for fact in [
        Fact::RunStarted {
            run_id: 1,
            attempt: 1,
        },
        Fact::Table(Event::new(
            Context::new(1, 1, Stage::World),
            Update::Table(Table::Registration(vec![
                SpellId::new(73).unwrap(),
                SpellId::new(74).unwrap(),
            ])),
        )),
        Fact::ZonePending { run_id: 1 },
        Fact::Table(Event::new(
            Context::new(1, 2, Stage::Zone { session_id: 8 }),
            Update::Table(Table::Replay(vec![])),
        )),
        Fact::ZoneEntered {
            run_id: 1,
            session_id: 8,
        },
    ] {
        names.receive(&binding.envelope(fact));
    }
    assert!(names.ready());
    names
}

pub(crate) fn admitted(now: Instant, names: &SpellNames) -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::generation(&mut online, eq_client_core::GameDialect::Titanium);
    let mut player = testing::player(7);
    player.class = Some(2);
    player.memorized_spells[..3].copy_from_slice(&[Some(73), Some(74), Some(73)]);
    testing::admit(&mut online, 8, player);
    online.tick(now, names);
    assert!(online.world().casting().p99_gem_start());
    online
}

pub(crate) fn news(online: &mut OnlineState, event: WorldEvent, now: Instant, names: &SpellNames) {
    testing::news_at(online, [event], now);
    online.tick(now, names);
}

pub(crate) fn completed(online: &mut OnlineState, at: Instant, names: &SpellNames) {
    news(
        online,
        WorldEvent::CastPending {
            session_id: 8,
            spell_id: Some(73),
            gem: Some(0),
        },
        at.checked_sub(Duration::from_secs(4)).unwrap(),
        names,
    );
    news(
        online,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 7,
            spell_id: 73,
            duration_ms: 4000,
        }),
        at.checked_sub(Duration::from_secs(4)).unwrap(),
        names,
    );
    news(
        online,
        WorldEvent::CastPending {
            session_id: 8,
            spell_id: None,
            gem: None,
        },
        at,
        names,
    );
    news(
        online,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 73,
            keep_casting: false,
        }),
        at,
        names,
    );
}

fn request(gem: u8) -> requests::Request {
    requests::Request {
        gem,
        target_id: 7,
        forgetting: false,
        linked_timer: None,
        checks: requests::CastChecks::default(),
    }
}

#[test]
fn p99_normal_requests_block_at_5999_admit_at_6000_and_send_once_while_pending() {
    let names = names();
    let start = Instant::now();
    let at = start + Duration::from_secs(4);
    let mut online = admitted(start, &names);
    completed(&mut online, at, &names);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let outbox = crate::outbox::Outbox::new(Some(tx));
    let messages = messages::Messages::default();
    let map = crate::keys::KeyMap::default();
    let mut chat = crate::chat::ChatState::default();
    let mut press = |online: &OnlineState, gem, now| {
        requests::spell_at(
            &mut chat,
            online.world(),
            online.world().player().unwrap(),
            &outbox,
            &request(gem),
            (&messages, &map),
            Vec::new,
            now,
        );
    };
    let early = at + Duration::from_millis(5999);
    for _ in 0..4 {
        press(&online, 0, early);
    }
    assert!(rx.try_recv().is_err());
    let boundary = at + Duration::from_secs(6);
    press(&online, 0, boundary);
    assert!(matches!(
        rx.try_recv().unwrap(),
        eq_client_core::ClientCommand::CastSpell {
            gem: 0,
            spell_id: 73,
            ..
        }
    ));
    news(
        &mut online,
        WorldEvent::CastPending {
            session_id: 8,
            spell_id: Some(73),
            gem: Some(0),
        },
        boundary,
        &names,
    );
    for _ in 0..4 {
        press(&online, 0, boundary);
    }
    assert!(rx.try_recv().is_err());
    // A pending request also keeps B from racing actual cast ownership.
    press(&online, 1, boundary);
    assert!(rx.try_recv().is_err());
}

#[test]
fn p99_normal_b_press_is_available_after_a_completion_but_not_during_real_cast() {
    let names = names();
    let start = Instant::now();
    let at = start + Duration::from_secs(4);
    let mut online = admitted(start, &names);
    news(
        &mut online,
        WorldEvent::CastPending {
            session_id: 8,
            spell_id: Some(73),
            gem: Some(0),
        },
        start,
        &names,
    );
    news(
        &mut online,
        WorldEvent::Spell(SpellUpdate::Began {
            caster_id: 7,
            spell_id: 73,
            duration_ms: 4000,
        }),
        start,
        &names,
    );
    let messages = messages::Messages::default();
    let map = crate::keys::KeyMap::default();
    let check = |online: &OnlineState, now| {
        requests::check(
            online.world(),
            online.world().player().unwrap(),
            &request(1),
            (&messages, &map),
            now,
        )
    };
    assert!(check(&online, start).is_err());
    news(
        &mut online,
        WorldEvent::Spell(SpellUpdate::Mana {
            spell_id: 73,
            keep_casting: false,
        }),
        at,
        &names,
    );
    news(
        &mut online,
        WorldEvent::Spell(SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 73,
            reduction_ms: 0,
        }),
        at,
        &names,
    );
    assert!(check(&online, at).is_ok());
    assert!(check(&online, at + Duration::from_millis(1)).is_ok());
    assert_eq!(
        online.world().casting().gem_cast_wait(0, 73, None, at),
        Some(Duration::from_secs(6))
    );
    news(
        &mut online,
        WorldEvent::OwnStun {
            session_id: 8,
            revision: 1,
            state: eq_client_core::control::StunState::Stunned,
        },
        at,
        &names,
    );
    assert!(check(&online, at).is_err());
    news(
        &mut online,
        WorldEvent::OwnStun {
            session_id: 8,
            revision: 2,
            state: eq_client_core::control::StunState::Clear,
        },
        at,
        &names,
    );
    assert!(check(&online, at).is_ok());
}

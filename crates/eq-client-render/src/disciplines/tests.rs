use super::*;
use crate::online::testing;
use eq_client_core::{WorldEvent, disciplines::Reuse};

/// A Titanium spell line: a discipline when `flagged`, a warrior's at
/// `level`, with this timer slot, timed and self-targeted when `timed_self`.
fn line(id: u32, name: &str, flagged: bool, level: u8, timer: u32, timed_self: bool) -> String {
    let mut fields: Vec<String> = vec!["0".into(); 217];
    fields[0] = id.to_string();
    fields[1] = name.into();
    for field in &mut fields[104..120] {
        *field = "255".into();
    }
    fields[104] = level.to_string();
    fields[167] = timer.to_string();
    fields[168] = u8::from(flagged).to_string();
    fields[16] = if timed_self { "3" } else { "0" }.into();
    fields[98] = "6".into();
    fields.join("^")
}

/// The spell file the tests use.
fn names() -> SpellNames {
    SpellNames::parse(
        &[
            line(4503, "Evasive Discipline", true, 52, 1, true),
            line(4499, "Defensive Discipline", true, 55, 2, true),
            line(4585, "Fellstrike Discipline", true, 58, 2, false),
            line(4600, "Evasion Strike", true, 60, 3, false),
            line(4700, "Not A Discipline", false, 1, 4, false),
        ]
        .join("\n"),
    )
}

/// An admitted warrior of this level who knows these disciplines.
fn warrior(level: u8, known: &[u32]) -> OnlineState {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.level = level;
    player.disciplines = Some(known.to_vec());
    testing::admit(&mut online, 1, player);
    online
}

fn messages() -> Messages {
    Messages::default()
}

fn typed(online: &OnlineState, words: &str, now: Instant) -> Outcome {
    decide(
        online.world(),
        &names(),
        &messages(),
        &Asked::Typed(words.into()),
        now,
    )
}

fn used(online: &OnlineState, spell: u32, now: Instant) -> Outcome {
    decide(
        online.world(),
        &names(),
        &messages(),
        &Asked::Use(spell),
        now,
    )
}

fn timer(online: &mut OnlineState, timer: u32, seconds: u64, now: Instant) {
    testing::news_at(
        online,
        [WorldEvent::DisciplineReuse(Reuse {
            timer,
            remaining: Duration::from_secs(seconds),
        })],
        now,
    );
}

fn texts(outcome: &Outcome) -> Vec<(String, Option<u32>)> {
    match outcome {
        Outcome::Lines(lines) => lines
            .iter()
            .map(|line| (line.words.text.clone(), line.message_type))
            .collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_class_without_disciplines_gets_the_gate_line_in_its_colour() {
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.class = Some(2); // a cleric
    testing::admit(&mut online, 1, player);
    let now = Instant::now();
    for words in ["", "evasive"] {
        assert_eq!(
            texts(&typed(&online, words, now)),
            [("Your class has no disciplines.".to_owned(), Some(0x111))]
        );
    }
}

#[test]
fn no_word_lists_each_known_discipline_whose_timer_runs_and_sends_nothing() {
    let now = Instant::now();
    let mut online = warrior(60, &[4503, 0, 4499, 4585, 9999]);
    assert_eq!(typed(&online, "", now), Outcome::Lines(Vec::new()));
    // Timer 2 is shared by two known disciplines: each prints, in list
    // order, with the same time left; the unknown spell 9999 is skipped.
    timer(&mut online, 2, 3725, now);
    assert_eq!(
        texts(&typed(&online, "  ", now)),
        [
            (
                "Defensive Discipline can be used again in 1h 2m 5s.".to_owned(),
                Some(0x10f)
            ),
            (
                "Fellstrike Discipline can be used again in 1h 2m 5s.".to_owned(),
                Some(0x10f)
            ),
        ]
    );
    timer(&mut online, 1, 65, now);
    assert_eq!(
        texts(&typed(&online, "", now))[0].0,
        "Evasive Discipline can be used again in 1m 5s."
    );
}

#[test]
fn a_word_uses_the_first_known_discipline_it_begins_in_list_order() {
    let now = Instant::now();
    let online = warrior(60, &[4600, 4503, 4499]);
    // "eva" begins both Evasion Strike and Evasive Discipline: the first in
    // the known list wins, whatever the case.
    assert_eq!(
        typed(&online, "EVA", now),
        Outcome::Cast {
            spell_id: 4600,
            timer: 3,
            target_id: 0,
            timed_self: false,
        }
    );
    assert_eq!(
        typed(&online, "defensive", now),
        Outcome::Cast {
            spell_id: 4499,
            timer: 2,
            target_id: 0,
            timed_self: true,
        }
    );
    // A word that begins no known name is silent, as is a number.
    assert_eq!(typed(&online, "fell", now), Outcome::Silent);
    assert_eq!(typed(&online, "4503", now), Outcome::Silent);
    // The whole name and more matches nothing.
    assert_eq!(
        typed(&online, "defensive discipline now", now),
        Outcome::Silent
    );
}

#[test]
fn the_first_match_is_used_even_when_it_fails_its_checks() {
    let now = Instant::now();
    // Evasion Strike comes first and is above a level 55 warrior: silent,
    // with no fall through to Evasive Discipline.
    let online = warrior(55, &[4600, 4503]);
    assert_eq!(typed(&online, "eva", now), Outcome::Silent);
}

#[test]
fn an_unknown_unflagged_or_too_high_discipline_is_refused_silently() {
    let now = Instant::now();
    let online = warrior(52, &[4503, 4499, 4700]);
    // Not in the known list.
    assert_eq!(used(&online, 4585, now), Outcome::Silent);
    // Known but not flagged as a discipline in the spell file.
    assert_eq!(used(&online, 4700, now), Outcome::Silent);
    // Known but above the player's level.
    assert_eq!(used(&online, 4499, now), Outcome::Silent);
    // No spell record at all.
    assert_eq!(used(&warrior(60, &[9999]), 9999, now), Outcome::Silent);
    // A class with no level for it (255): a paladin knowing a warrior one.
    let mut online = OnlineState::new(true);
    let mut player = testing::player(7);
    player.class = Some(3);
    player.level = 60;
    player.disciplines = Some(vec![4503]);
    testing::admit(&mut online, 1, player);
    assert_eq!(used(&online, 4503, now), Outcome::Silent);
}

#[test]
fn a_discipline_on_its_timer_prints_the_time_left_and_is_refused() {
    let now = Instant::now();
    let mut online = warrior(60, &[4503]);
    timer(&mut online, 1, 30, now);
    assert_eq!(
        texts(&used(&online, 4503, now)),
        [(
            "Evasive Discipline can be used again in 0m 30s.".to_owned(),
            Some(0x10f)
        )]
    );
    // An elapsed timer lets it go.
    timer(&mut online, 1, 0, now);
    assert!(matches!(used(&online, 4503, now), Outcome::Cast { .. }));
}

#[test]
fn a_timed_self_discipline_is_refused_while_the_discipline_slot_is_taken() {
    use eq_client_core::{Buff, BuffUpdate};
    let now = Instant::now();
    let mut online = warrior(60, &[4503, 4585]);
    let buff = |slot, spell_id| {
        WorldEvent::Buff(BuffUpdate {
            applies: eq_network_game::buffs::Applies::Received,
            record_type: 0,
            entity_id: 7,
            slot,
            spell_id,
            buff: Some(Buff {
                spell_id,
                caster_level: 60,
                effect_type: 2,
                bard_modifier: 0,
                duration_ticks: 10,
                counters: 0,
                caster_id: 0,
            }),
        })
    };
    // Something in another slot leaves it free.
    testing::news(&mut online, [WorldEvent::BuffSnapshot(vec![None; 25])]);
    testing::news(&mut online, [buff(3, 200)]);
    assert!(matches!(used(&online, 4503, now), Outcome::Cast { .. }));
    // A spell in slot 24 takes it: the timed self one is refused with its
    // line, and one with no duration still goes.
    testing::news(&mut online, [buff(24, 201)]);
    assert_eq!(
        texts(&used(&online, 4503, now)),
        [(
            "You can't use that discipline right now.".to_owned(),
            Some(0x10f)
        )]
    );
    assert!(matches!(used(&online, 4585, now), Outcome::Cast { .. }));
}

#[test]
fn a_timed_self_discipline_is_refused_while_the_server_says_one_is_active() {
    use eq_client_core::{Buff, BuffUpdate, disciplines::TITANIUM_BUFF_SLOT};
    let now = Instant::now();
    let mut online = warrior(60, &[4503]);
    testing::news(
        &mut online,
        [WorldEvent::Buff(BuffUpdate {
            applies: eq_network_game::buffs::Applies::Received,
            record_type: 0,
            entity_id: 7,
            slot: TITANIUM_BUFF_SLOT,
            spell_id: 4499,
            buff: Some(Buff {
                spell_id: 4499,
                caster_level: 60,
                effect_type: 2,
                bard_modifier: 0,
                duration_ticks: 10,
                counters: 0,
                caster_id: 0,
            }),
        })],
    );
    assert!(matches!(
        online.world().casting().active_discipline(),
        Active::Titanium(4499)
    ));
    assert!(matches!(used(&online, 4503, now), Outcome::Lines(_)));
    assert!(waits(online.world(), &names(), 4503, now));
}

#[test]
fn the_cast_carries_the_current_target_or_none() {
    let now = Instant::now();
    let mut online = warrior(60, &[4585]);
    assert!(matches!(
        used(&online, 4585, now),
        Outcome::Cast { target_id: 0, .. }
    ));
    // Even the player's own spawn, as the program sends whatever is
    // targeted.
    online.select_target(Some(7));
    assert!(matches!(
        used(&online, 4585, now),
        Outcome::Cast { target_id: 7, .. }
    ));
}

#[test]
fn a_part_second_left_counts_as_a_whole_one() {
    let line = time_line(
        &messages(),
        "Evasive Discipline",
        Duration::from_millis(500),
    );
    assert_eq!(
        line.words.text,
        "Evasive Discipline can be used again in 0m 1s."
    );
}

#[test]
fn the_installed_lines_take_the_name_and_the_parts_in_order() {
    let messages = Messages::parse("EQST0002\n2\n5807 %1 h%2 m%3 s%4\n5808 %1 m%2 s%3\n");
    let line = time_line(&messages, "Evasive", Duration::from_secs(3725));
    assert_eq!(line.words.text, "Evasive h1 m2 s5");
    assert_eq!(line.message_type, Some(0x10f));
    let line = time_line(&messages, "Evasive", Duration::from_secs(65));
    assert_eq!(line.words.text, "Evasive m1 s5");
}

/// An app running the discipline system for this world, sending to `tx`.
fn running(online: OnlineState) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(tx)))
        .insert_resource(names())
        .init_resource::<crate::ViewerSettings>()
        .init_resource::<ChatState>()
        .add_systems(Update, run);
    (app, rx)
}

fn ask(app: &mut App, asked: Asked) -> Vec<String> {
    app.world_mut()
        .resource_mut::<ChatState>()
        .disciplines
        .push(asked);
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
fn a_discipline_meets_the_gem_casts_refusals_while_a_cast_is_under_way() {
    // Casting: refused with the gem's words, nothing sent.
    let mut online = warrior(60, &[4585]);
    testing::spell(
        &mut online,
        eq_client_core::SpellUpdate::Began {
            caster_id: 7,
            spell_id: 200,
            duration_ms: 3000,
        },
    );
    let (mut app, rx) = running(online);
    let lines = ask(&mut app, Asked::Use(4585));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Already casting:")),
        "{lines:?}"
    );
    assert!(rx.try_recv().is_err());
    // A request the server hasn't answered: refused the same way.
    let mut online = warrior(60, &[4585]);
    testing::pending_cast(&mut online, Some(200));
    let (mut app, rx) = running(online);
    let lines = ask(&mut app, Asked::Typed("fell".into()));
    assert!(
        lines.contains(&"Waiting for the server to acknowledge the cast".to_owned()),
        "{lines:?}"
    );
    assert!(rx.try_recv().is_err());
    // Neither: it goes.
    let (mut app, rx) = running(warrior(60, &[4585]));
    ask(&mut app, Asked::Use(4585));
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::UseDiscipline {
            spell_id: 4585,
            target_id: 0,
            timed_self: false,
            ..
        })
    ));
}

#[test]
fn the_mac_era_client_keeps_this_clients_answer() {
    let (mut app, rx) = running(warrior(60, &[4585]));
    app.insert_resource(crate::ViewerSettings(crate::ViewerConfig {
        installed_client: eq_client_assets::ui::InstalledClient::EqMac,
        ..default()
    }));
    let lines = ask(&mut app, Asked::Typed(String::new()));
    assert!(lines.contains(&"This client does not take disciplines yet.".to_owned()));
    assert!(rx.try_recv().is_err());
}

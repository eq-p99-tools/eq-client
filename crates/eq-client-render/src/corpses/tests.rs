use super::*;
use crate::online::testing;
use eq_client_core::{SpawnState, WorldEvent, group::GroupUpdate};

fn corpse(spawn_id: u16, name: &str, kind: SpawnKind) -> SpawnState {
    SpawnState {
        name: name.into(),
        kind,
        pet_owner: None,
        ..testing::pet(spawn_id, 0)
    }
}

/// The player (Example) with their own corpse 10, a group member's 11, a
/// stranger's 12 and an NPC's 13.
fn zone() -> OnlineState {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(1));
    testing::spawns(
        &mut online,
        vec![
            corpse(10, "Example's_corpse0", SpawnKind::PlayerCorpse),
            corpse(11, "Friend's_corpse0", SpawnKind::PlayerCorpse),
            corpse(12, "Stranger's_corpse0", SpawnKind::PlayerCorpse),
            corpse(13, "a_gnoll's_corpse12", SpawnKind::NpcCorpse),
        ],
    );
    testing::news(
        &mut online,
        [WorldEvent::Group(GroupUpdate::Members {
            leader: "Friend".into(),
            members: vec!["Friend".into(), "Example".into()],
        })],
    );
    online
}

fn hidden(state: &HiddenCorpses) -> Vec<u16> {
    state.hidden.iter().copied().collect()
}

fn ask(online: &OnlineState, state: &mut HiddenCorpses, words: &str) -> Vec<String> {
    answer(state, online.world(), words, &Messages::default())
        .into_iter()
        .map(|said| said.words.text)
        .collect()
}

#[test]
fn the_two_hiding_words_sweep_the_corpses_present_once() {
    let online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    assert_eq!(ask(&online, &mut state, "OTHERS"), [OTHERS_LINE.1]);
    assert_eq!(hidden(&state), [11, 12, 13]);
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    assert_eq!(ask(&online, &mut state, "group"), [GROUP_LINE.1]);
    assert_eq!(hidden(&state), [12, 13]);
    // A sweep is not a mode: a corpse that comes afterwards shows.
    state.follow(online.world());
    let mut online = online;
    testing::spawns(
        &mut online,
        vec![corpse(14, "Later's_corpse0", SpawnKind::PlayerCorpse)],
    );
    state.follow(online.world());
    assert_eq!(hidden(&state), [12, 13]);
}

/// Opens the loot window on a corpse, then closes it, following each step
/// as the frame would.
fn loot(online: &mut OnlineState, state: &mut HiddenCorpses, corpse_id: u16) {
    online.open_loot(corpse_id);
    state.follow(online.world());
    online.close_loot();
    state.follow(online.world());
}

#[test]
fn the_looted_word_hides_each_corpse_at_its_loot_close_but_our_own() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    assert_eq!(ask(&online, &mut state, "LOOTED"), [LOOTED_LINE.1]);
    // Setting the mode hides nothing by itself.
    assert_eq!(hidden(&state), Vec::<u16>::new());
    loot(&mut online, &mut state, 12);
    assert_eq!(hidden(&state), [12]);
    // Our own corpse stays shown; a group member's doesn't.
    loot(&mut online, &mut state, 10);
    loot(&mut online, &mut state, 11);
    assert_eq!(hidden(&state), [11, 12]);
    // A corpse that arrives isn't hidden until it is looted.
    testing::spawns(
        &mut online,
        vec![corpse(14, "Later's_corpse0", SpawnKind::PlayerCorpse)],
    );
    state.follow(online.world());
    assert!(!state.hides(14));
    // Moving from one corpse to another closes the first.
    online.open_loot(13);
    state.follow(online.world());
    online.open_loot(14);
    state.follow(online.world());
    assert!(state.hides(13) && !state.hides(14));
}

#[test]
fn without_the_mode_a_loot_close_hides_nothing() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    loot(&mut online, &mut state, 12);
    assert_eq!(hidden(&state), Vec::<u16>::new());
}

#[test]
fn show_all_and_a_new_zone_session_clear_the_looted_mode() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    ask(&online, &mut state, "looted");
    loot(&mut online, &mut state, 12);
    ask(&online, &mut state, "anything");
    assert_eq!(hidden(&state), Vec::<u16>::new());
    loot(&mut online, &mut state, 12);
    assert_eq!(hidden(&state), Vec::<u16>::new());
    // Set again, then a new session: inferred, 6.490.
    ask(&online, &mut state, "looted");
    testing::admit(&mut online, 2, testing::player(1));
    testing::spawns(
        &mut online,
        vec![corpse(12, "Stranger's_corpse0", SpawnKind::PlayerCorpse)],
    );
    state.follow(online.world());
    loot(&mut online, &mut state, 12);
    assert_eq!(hidden(&state), Vec::<u16>::new());
}

#[test]
fn any_other_word_shows_them_all_and_no_word_gives_help() {
    let online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    ask(&online, &mut state, "others");
    assert_eq!(ask(&online, &mut state, "anything"), [SHOWN_LINE.1]);
    assert_eq!(hidden(&state), Vec::<u16>::new());
    assert_eq!(ask(&online, &mut state, "  ").len(), 3);
    assert!(!state.hides(10));
}

#[test]
fn a_new_zone_session_forgets_every_hidden_corpse() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    ask(&online, &mut state, "others");
    // The next zone reuses the ids: the same corpses, a new session.
    testing::admit(&mut online, 2, testing::player(1));
    testing::spawns(
        &mut online,
        vec![
            corpse(11, "Other's_corpse0", SpawnKind::PlayerCorpse),
            corpse(12, "a_bat's_corpse3", SpawnKind::NpcCorpse),
        ],
    );
    state.follow(online.world());
    assert_eq!(hidden(&state), Vec::<u16>::new());
}

#[test]
fn a_corpse_gone_from_the_zone_is_forgotten() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    ask(&online, &mut state, "others");
    testing::news(&mut online, [WorldEvent::Despawn(12)]);
    state.follow(online.world());
    assert_eq!(hidden(&state), [11, 13]);
}

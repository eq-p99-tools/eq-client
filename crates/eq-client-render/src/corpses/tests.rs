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
    assert_eq!(ask(&online, &mut state, "OTHERS"), [OTHERS_LINE.1]);
    assert_eq!(hidden(&state), [11, 12, 13]);
    let mut state = HiddenCorpses::default();
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

#[test]
fn the_flag_hides_corpses_as_they_come_but_the_players_own() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    state.follow(online.world());
    assert_eq!(ask(&online, &mut state, "new"), [LATER_LINE.1]);
    // Setting the flag hides nothing present.
    state.follow(online.world());
    assert_eq!(hidden(&state), Vec::<u16>::new());
    testing::spawns(
        &mut online,
        vec![
            corpse(14, "Later's_corpse0", SpawnKind::PlayerCorpse),
            corpse(15, "Example's_corpse1", SpawnKind::PlayerCorpse),
        ],
    );
    state.follow(online.world());
    assert_eq!(hidden(&state), [14]);
}

#[test]
fn any_other_word_shows_them_all_and_no_word_gives_help() {
    let online = zone();
    let mut state = HiddenCorpses::default();
    ask(&online, &mut state, "others");
    ask(&online, &mut state, "new");
    assert_eq!(ask(&online, &mut state, "anything"), [SHOWN_LINE.1]);
    assert_eq!(hidden(&state), Vec::<u16>::new());
    assert!(!state.later);
    assert_eq!(ask(&online, &mut state, "  ").len(), 3);
    assert!(!state.hides(10));
}

#[test]
fn a_corpse_gone_from_the_zone_is_forgotten() {
    let mut online = zone();
    let mut state = HiddenCorpses::default();
    ask(&online, &mut state, "others");
    testing::news(&mut online, [WorldEvent::Despawn(12)]);
    state.follow(online.world());
    assert_eq!(hidden(&state), [11, 13]);
}

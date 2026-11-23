use super::*;
use crate::online::{OnlineState, testing};
use eq_client_core::WorldEvent;

/// A world with the player (7, "Example") of this class and level, a
/// Friendly (20) and a Friend (21), on a server that takes shields.
fn world(class: u32, level: u8, target: Option<u16>) -> OnlineState {
    world_on(vec![Capability::Shielding], class, level, target)
}

/// The same world on a server with these capabilities.
fn world_on(
    capabilities: Vec<Capability>,
    class: u32,
    level: u8,
    target: Option<u16>,
) -> OnlineState {
    let mut player = testing::player(7);
    player.class = Some(class);
    player.level = level;
    let mut online = OnlineState::new(true);
    testing::news(
        &mut online,
        [WorldEvent::Entered {
            capabilities,
            choices: Vec::new(),
            session_id: 5,
            zone: "qeytoqrg".into(),
            player: Box::new(player),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        }],
    );
    testing::connect(&mut online, true);
    let spawn = |spawn_id, name: &str| eq_client_core::SpawnState {
        name: name.into(),
        pet_owner: None,
        ..testing::pet(spawn_id, 0)
    };
    testing::spawns(
        &mut online,
        vec![spawn(20, "Friendly000"), spawn(21, "Friend000")],
    );
    online.select_target(target);
    online
}

/// The words of what `/shield` with these words says, or the spawn sent.
fn decided(online: &OnlineState, words: &str) -> Result<u16, Vec<String>> {
    match decide(online.world(), words, &Messages::default()) {
        Outcome::Send(id) => Ok(id),
        Outcome::Say(lines) => Err(lines
            .into_iter()
            .map(|said| {
                assert_eq!(said.message_type, Some(COLOUR));
                said.words.text
            })
            .collect()),
        Outcome::Silent => Err(Vec::new()),
    }
}

#[test]
fn the_words_after_shield_are_kept_whole() {
    assert_eq!(typed("/shield"), Some(String::new()));
    assert_eq!(typed("/SHIELD  Friend  Two "), Some("Friend  Two".into()));
    assert_eq!(typed("/shieldx"), None);
    assert_eq!(typed("shield"), None);
}

#[test]
fn a_warrior_under_thirty_or_another_class_says_one_line() {
    for (class, level) in [(WARRIOR, 29), (3, 60)] {
        for words in ["", "friend"] {
            assert_eq!(
                decided(&world(class, level, Some(20)), words),
                Err(vec![NOT_ABLE.1.to_owned()]),
                "{class} {level} {words:?}"
            );
        }
    }
}

#[test]
fn a_name_picks_the_shortest_prefix_match_and_never_the_target() {
    let online = world(WARRIOR, 30, Some(20));
    assert_eq!(decided(&online, "FRI"), Ok(21));
    assert_eq!(decided(&online, "friendl"), Ok(20));
    // No match, or extra words that make the name match none, are silent.
    assert_eq!(decided(&online, "nobody"), Err(Vec::new()));
    assert_eq!(decided(&online, "friend two"), Err(Vec::new()));
    assert_eq!(decided(&online, "exam"), Err(vec![NAMED_SELF.1.to_owned()]));
}

#[test]
fn the_target_is_taken_with_no_words() {
    assert_eq!(decided(&world(WARRIOR, 30, Some(20)), ""), Ok(20));
    assert_eq!(
        decided(&world(WARRIOR, 30, None), ""),
        Err(NO_TARGET.map(|(_, ours)| ours.to_owned()).to_vec())
    );
    // A target gone from the spawn table counts as none.
    assert_eq!(
        decided(&world(WARRIOR, 30, Some(44)), ""),
        Err(NO_TARGET.map(|(_, ours)| ours.to_owned()).to_vec())
    );
    assert_eq!(
        decided(&world(WARRIOR, 30, Some(7)), ""),
        Err(vec![TARGETED_SELF.1.to_owned()])
    );
}

#[test]
fn the_mac_era_client_keeps_the_earlier_checks_and_words() {
    let mac = |class, level, target| {
        let mut online = world(class, level, target);
        testing::generation(&mut online, GameDialect::EqMac);
        online
    };
    let earlier = |online: &OnlineState, words: &str| match decide(
        online.world(),
        words,
        &Messages::default(),
    ) {
        Outcome::Send(id) => Ok(id),
        Outcome::Say(lines) => Err(lines.len()),
        Outcome::Silent => Err(0),
    };
    assert_eq!(earlier(&mac(WARRIOR, 30, Some(20)), ""), Ok(20));
    for (class, level, target, words) in [
        (WARRIOR, 30, Some(20), "friend"),
        (3, 60, Some(20), ""),
        (WARRIOR, 29, Some(20), ""),
        (WARRIOR, 30, None, ""),
        (WARRIOR, 30, Some(7), ""),
    ] {
        assert_eq!(
            earlier(&mac(class, level, target), words),
            Err(1),
            "{class} {level} {target:?} {words:?}"
        );
    }
}

fn app(online: OnlineState) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let mut app = App::new();
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answer);
    (app, received)
}

fn shield_with(app: &mut App, words: &str) -> usize {
    app.world_mut().resource_mut::<ChatState>().shield_words = Some(words.into());
    app.update();
    let chat = app.world().resource::<ChatState>();
    assert!(chat.shield_words.is_none());
    chat.history.lines(eq_client_core::chat::ChatTab::All).len()
}

#[test]
fn the_answer_system_sends_once_or_says_its_lines() {
    let (mut app, received) = app(world(WARRIOR, 30, Some(20)));
    assert_eq!(shield_with(&mut app, ""), 0);
    assert!(matches!(
        received.try_recv().unwrap(),
        ClientCommand::Shield {
            session_id: 5,
            spawn_id: 20,
            ..
        }
    ));
    assert!(received.try_recv().is_err());
    assert_eq!(shield_with(&mut app, "exam"), 1);
    assert!(received.try_recv().is_err());
}

#[test]
fn a_server_that_takes_no_shields_refuses_only_the_send() {
    let (mut refused, received) = app(world_on(Vec::new(), WARRIOR, 30, Some(20)));
    assert_eq!(shield_with(&mut refused, ""), 1);
    assert!(received.try_recv().is_err());
    let chat = refused.world().resource::<ChatState>();
    let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
    assert_eq!(lines[0].1.message.text, outbox::UNAVAILABLE);
    // The program's own lines come before any send, on any server.
    let (mut low, _) = app(world_on(Vec::new(), WARRIOR, 29, Some(20)));
    assert_eq!(shield_with(&mut low, ""), 1);
    let chat = low.world().resource::<ChatState>();
    let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
    assert_eq!(lines[0].1.message.text, NOT_ABLE.1);
}

#[test]
fn a_typed_line_reaches_the_answer_through_the_chat() {
    let mut chat = ChatState::default();
    assert_eq!(
        crate::chat::client_request("/shield Friend", &mut chat),
        Some(Ok(()))
    );
    assert_eq!(chat.shield_words.as_deref(), Some("Friend"));
}

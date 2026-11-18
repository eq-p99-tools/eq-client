//! Ordered keyboard routing through Bevy input and the client's typing stages.
//! Fabricated Window focus is logical routing evidence, not an OS focus check.
use super::*;
use crate::keys::{Act, Chord, Keys};
use bevy::input::{ButtonState, InputPlugin};

#[derive(Default, Resource, Debug, PartialEq, Eq)]
struct GameInput {
    focused: bool,
    slot: bool,
    forward: bool,
}

fn app(detached: bool, draft: &str) -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(4);
    let mut online = OnlineState::new(true);
    crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
    let mut app = App::new();
    crate::keys::testing::install(&mut app);
    crate::systems::configure_stages(&mut app);
    app.add_plugins(InputPlugin)
        .init_resource::<ChatState>()
        .init_resource::<crate::windows::pointer::Wheel>()
        .insert_resource(online)
        .insert_resource(crate::outbox::Outbox::new(Some(sender)))
        .init_resource::<GameInput>()
        .add_systems(
            Update,
            (crate::keys::presses::capture, input, capture)
                .chain()
                .in_set(crate::systems::Stage::Typing),
        )
        .add_systems(
            Update,
            (|keys: Keys, mut observed: ResMut<GameInput>| {
                *observed = GameInput {
                    focused: keys.focused(),
                    slot: keys.pressed(Act::Slot(0)),
                    forward: keys.focused()
                        && keys.map.holding(&keys.input, &[]).held(Act::Forward),
                };
            })
            .in_set(crate::systems::Stage::Input),
        );
    crate::keys::testing::bind(&mut app, Act::Forward, Chord::key(KeyCode::KeyW));
    crate::keys::testing::bind(&mut app, Act::Slot(0), Chord::key(KeyCode::Digit1));
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        PrimaryWindow,
    ));
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    chat.groups.select(WindowId::Chat, ChatTab::Say);
    if detached {
        chat.groups.detach(WindowId::Chat, ChatTab::Say).unwrap();
    }
    chat.draft.replace(draft);
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = true;
    (app, receiver)
}

fn key(code: KeyCode, down: bool, repeat: bool) -> KeyboardInput {
    let (logical_key, text) = match code {
        KeyCode::Enter => (Key::Enter, None),
        KeyCode::Escape => (Key::Escape, None),
        KeyCode::Slash => (Key::Character("/".into()), Some("/".into())),
        KeyCode::Digit1 => (Key::Character("1".into()), Some("1".into())),
        KeyCode::KeyW => (Key::Character("w".into()), Some("w".into())),
        _ => unreachable!(),
    };
    KeyboardInput {
        key_code: code,
        logical_key,
        text: text.filter(|_| down),
        state: if down {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        repeat,
        window: Entity::PLACEHOLDER,
    }
}

fn frame(app: &mut App, events: impl IntoIterator<Item = KeyboardInput>) {
    app.world_mut().write_message_batch(events);
    app.update();
}

fn assert_game(app: &App, focused: bool, slot: bool, forward: bool) {
    assert_eq!(
        app.world().resource::<GameInput>(),
        &GameInput {
            focused,
            slot,
            forward
        }
    );
}

fn assert_said(receiver: &std::sync::mpsc::Receiver<ClientCommand>, expected: &str) {
    assert!(matches!(receiver.try_recv().unwrap(),
        ClientCommand::SendChat(OutboundChat::Say(text)) if text == expected));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn keys_after_enter_belong_to_game_without_changing_the_submitted_line() {
    for detached in [false, true] {
        let (mut app, receiver) = app(detached, "before");
        frame(
            &mut app,
            [
                key(KeyCode::Enter, true, false),
                key(KeyCode::Digit1, true, false),
                key(KeyCode::KeyW, true, false),
            ],
        );
        assert_said(&receiver, "before");
        assert_game(&app, true, true, true);
        assert_eq!(app.world().resource::<ChatState>().draft.text(), "");
    }
}

#[test]
fn keys_before_enter_and_their_repeats_stay_with_the_draft_until_a_fresh_press() {
    for detached in [false, true] {
        let (mut app, receiver) = app(detached, "before");
        frame(
            &mut app,
            [
                key(KeyCode::Digit1, true, false),
                key(KeyCode::KeyW, true, false),
                key(KeyCode::Enter, true, false),
            ],
        );
        assert_said(&receiver, "before1w");
        assert_game(&app, true, false, false);
        frame(
            &mut app,
            [
                key(KeyCode::Digit1, true, true),
                key(KeyCode::KeyW, true, true),
            ],
        );
        assert_game(&app, true, false, false);
        frame(
            &mut app,
            [
                key(KeyCode::Digit1, false, false),
                key(KeyCode::KeyW, false, false),
                key(KeyCode::Digit1, true, false),
                key(KeyCode::KeyW, true, false),
            ],
        );
        assert_game(&app, true, true, true);
    }
}

#[test]
fn a_slash_after_submission_opens_a_new_draft_in_the_same_window() {
    for detached in [false, true] {
        let (mut app, receiver) = app(detached, "before");
        let owner = app.world().resource::<ChatState>().groups.owner();
        frame(
            &mut app,
            [
                key(KeyCode::Enter, true, false),
                key(KeyCode::Slash, true, false),
            ],
        );
        assert_said(&receiver, "before");
        let chat = app.world().resource::<ChatState>();
        assert_eq!(chat.draft.text(), "/");
        assert_eq!(chat.groups.owner(), owner);
        assert_game(&app, false, false, false);
    }
}

#[test]
fn enter_repeat_neither_submits_an_opened_draft_nor_reopens_a_submitted_one() {
    let (mut app, receiver) = app(false, "before");
    frame(&mut app, [key(KeyCode::Enter, true, false)]);
    assert_said(&receiver, "before");
    frame(&mut app, [key(KeyCode::Enter, true, true)]);
    assert_game(&app, true, false, false);
    frame(
        &mut app,
        [
            key(KeyCode::Enter, false, false),
            key(KeyCode::Enter, true, false),
        ],
    );
    assert_game(&app, false, false, false);
    frame(
        &mut app,
        [
            key(KeyCode::Enter, true, true),
            key(KeyCode::Digit1, true, false),
        ],
    );
    assert_eq!(app.world().resource::<ChatState>().draft.text(), "1");
    assert_game(&app, false, false, false);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn a_refused_enter_keeps_later_keys_in_the_draft_while_escape_releases_them() {
    let (mut app, receiver) = app(true, "/tell Friend");
    frame(
        &mut app,
        [
            key(KeyCode::Enter, true, false),
            key(KeyCode::Digit1, true, false),
        ],
    );
    assert_eq!(
        app.world().resource::<ChatState>().draft.text(),
        "/tell Friend1"
    );
    assert_game(&app, false, false, false);
    assert!(receiver.try_recv().is_err());
    frame(
        &mut app,
        [
            key(KeyCode::Escape, true, false),
            key(KeyCode::KeyW, true, false),
        ],
    );
    assert_game(&app, true, false, true);
    assert!(
        app.world()
            .resource::<crate::keys::Typing>()
            .escape_consumed
    );
}

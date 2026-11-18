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
        KeyCode::ArrowUp => (Key::ArrowUp, None),
        KeyCode::ArrowDown => (Key::ArrowDown, None),
        KeyCode::ShiftLeft => (Key::Shift, None),
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

/// Sends a line from the composing window as a typed Enter does.
fn send(app: &mut App, text: &str) {
    app.world_mut()
        .resource_mut::<ChatState>()
        .draft
        .replace(text);
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = true;
    frame(
        app,
        [
            key(KeyCode::Enter, true, false),
            key(KeyCode::Enter, false, false),
        ],
    );
}

/// Shift with an arrow, pressed and let go; `repeat` adds a held repeat.
fn shifted(app: &mut App, arrow: KeyCode, repeat: bool) {
    let mut events = vec![
        key(KeyCode::ShiftLeft, true, false),
        key(arrow, true, false),
    ];
    if repeat {
        events.push(key(arrow, true, true));
    }
    events.extend([
        key(arrow, false, false),
        key(KeyCode::ShiftLeft, false, false),
    ]);
    frame(app, events);
}

fn shown(app: &App) -> String {
    app.world().resource::<ChatState>().draft.marked()
}

fn composing(app: &mut App) {
    app.world_mut()
        .resource_mut::<crate::keys::Typing>()
        .composing = true;
}

#[test]
fn shift_up_and_down_step_through_sent_lines_and_past_the_newest_clear_the_line() {
    let (mut app, receiver) = app(false, "");
    for text in ["a", "b", "c"] {
        send(&mut app, text);
        assert_said(&receiver, text);
    }
    composing(&mut app);
    app.world_mut()
        .resource_mut::<ChatState>()
        .draft
        .replace("unsent");
    let mut seen = Vec::new();
    for arrow in [
        KeyCode::ArrowUp,
        KeyCode::ArrowUp,
        KeyCode::ArrowUp,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowDown,
        KeyCode::ArrowDown,
    ] {
        shifted(&mut app, arrow, false);
        seen.push(shown(&app));
    }
    assert_eq!(seen, ["c|", "b|", "a|", "a|", "b|", "c|", "|"]);
    // Recalling sends nothing and leaves composition open.
    assert!(receiver.try_recv().is_err());
    assert!(app.world().resource::<crate::keys::Typing>().composing);
    // A camp or a lost session forgets them.
    app.world_mut().resource_mut::<ChatState>().end_session();
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "|");
}

#[test]
fn a_recalled_line_is_edited_and_sent_once_while_repeats_and_plain_arrows_do_not_step() {
    let (mut app, receiver) = app(false, "");
    for text in ["a", "b"] {
        send(&mut app, text);
        assert_said(&receiver, text);
    }
    composing(&mut app);
    // A held repeat steps no further than the press.
    shifted(&mut app, KeyCode::ArrowUp, true);
    assert_eq!(shown(&app), "b|");
    // Plain Up and Down leave the line as it is.
    frame(
        &mut app,
        [
            key(KeyCode::ArrowUp, true, false),
            key(KeyCode::ArrowUp, false, false),
            key(KeyCode::ArrowDown, true, false),
            key(KeyCode::ArrowDown, false, false),
        ],
    );
    assert_eq!(shown(&app), "b|");
    frame(
        &mut app,
        [
            key(KeyCode::Digit1, true, false),
            key(KeyCode::Digit1, false, false),
            key(KeyCode::Enter, true, false),
            key(KeyCode::Enter, false, false),
        ],
    );
    assert_said(&receiver, "b1");
    // The edited line is now the newest.
    composing(&mut app);
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "b1|");
}

#[test]
fn a_closed_line_takes_no_recall_and_escape_keeps_the_draft_for_it() {
    let (mut app, receiver) = app(false, "");
    send(&mut app, "a");
    assert_said(&receiver, "a");
    composing(&mut app);
    app.world_mut()
        .resource_mut::<ChatState>()
        .draft
        .replace("kept");
    frame(
        &mut app,
        [
            key(KeyCode::Escape, true, false),
            key(KeyCode::Escape, false, false),
        ],
    );
    assert!(!app.world().resource::<crate::keys::Typing>().composing);
    // Up does not open the line (read R34 (f)).
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "kept|");
    assert!(!app.world().resource::<crate::keys::Typing>().composing);
    frame(
        &mut app,
        [
            key(KeyCode::Enter, true, false),
            key(KeyCode::Enter, false, false),
        ],
    );
    assert_eq!(shown(&app), "kept|");
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "a|");
    assert!(receiver.try_recv().is_err());
}

#[test]
fn each_window_recalls_its_own_lines_without_moving_composition() {
    let (mut app, receiver) = app(true, "");
    let main = WindowId::Chat;
    let other = app
        .world()
        .resource::<ChatState>()
        .groups
        .detached()
        .next()
        .unwrap();
    let focus = |app: &mut App, window| {
        assert!(
            app.world_mut()
                .resource_mut::<ChatState>()
                .groups
                .focus(window)
        );
    };
    focus(&mut app, other);
    send(&mut app, "detached");
    assert_said(&receiver, "detached");
    focus(&mut app, main);
    composing(&mut app);
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "|");
    assert_eq!(app.world().resource::<ChatState>().groups.owner(), main);
    focus(&mut app, other);
    shifted(&mut app, KeyCode::ArrowUp, false);
    assert_eq!(shown(&app), "detached|");
    assert_eq!(app.world().resource::<ChatState>().groups.owner(), other);
    // A closed window's lines go with it.
    app.world_mut()
        .resource_mut::<ChatState>()
        .groups
        .close(other);
    frame(&mut app, []);
    assert!(
        app.world_mut()
            .resource_mut::<ChatState>()
            .recall
            .older(other)
            .is_none()
    );
}

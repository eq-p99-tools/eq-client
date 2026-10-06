//! The login screen's keys and presses, and the client's own login window:
//! the login servers to choose from, the account and password boxes,
//! Connect and Quit, and a line saying what is happening. Tab moves between
//! the boxes, Enter connects, and Escape stops a login under way. Where the
//! installation has its own login screen, that shows instead
//! ([`super::official`]), and its controls do what this window's do.
use super::{Availability, Connected, Field, FrontEnd, Link, Lit, Request, Screen};
use crate::{online::OnlineState, theme, theme::Size};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

/// The longest account or password the boxes take.
const LONGEST: usize = 64;

/// The window's root.
#[derive(Component)]
pub(crate) struct Root;

/// What a control of the window does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Chooses a login server, by its place.
    Server(usize),
    /// Types in the account box.
    Account,
    /// Types in the password box.
    Password,
    /// Logs in.
    Connect,
    /// Logs in, then plays on the world last played on this login server
    /// as soon as the list shows it taking players.
    QuickConnect,
    /// Chooses the next login server.
    NextServer,
    /// Stops a login under way; with none, leaves the game.
    Cancel,
    /// Leaves the game.
    Quit,
}

/// What the player asked of the idle login screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    /// Nothing that logs in.
    Nothing,
    /// To log in.
    Connect,
    /// To log in and play on the world last played on.
    QuickConnect,
}

/// Takes the player's typing and presses while the login screen shows, and
/// draws the window again when what it shows changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn form(
    mut commands: Commands,
    (mut front, mut online, mut link): (ResMut<FrontEnd>, ResMut<OnlineState>, Link),
    keys: crate::keys::Keys,
    mut typed: MessageReader<KeyboardInput>,
    buttons: Query<(Ref<Interaction>, &Action)>,
    (roots, look): (
        Query<Entity, With<Root>>,
        Option<Res<super::official::LoginLook>>,
    ),
    mut previous: Local<String>,
    mut exit: MessageWriter<AppExit>,
) {
    let visible = front.has_login() && front.screen(&online) == Screen::Login;
    let official = look.is_some_and(|look| look.official());
    let typing: Vec<_> = typed.read().filter(|key| key.state.is_pressed()).collect();
    if visible && keys.window_focused() {
        let pressed = buttons
            .iter()
            .filter(|(interaction, _)| {
                interaction.is_changed() && **interaction == Interaction::Pressed
            })
            .map(|(_, action)| *action)
            .collect::<Vec<_>>();
        let cancel = pressed.contains(&Action::Cancel);
        if pressed.contains(&Action::Quit) || (cancel && !front.running()) {
            exit.write(AppExit::Success);
        }
        if front.running() {
            // A login under way: Escape or Cancel stops it.
            if cancel || keys.input.just_pressed(KeyCode::Escape) {
                front.end(None, &mut link);
            }
        } else {
            let connected = match take(&mut front, &pressed, &typing, &keys.input) {
                Asked::Nothing => Connected::No,
                Asked::Connect => front.connect(&mut online, &mut link),
                Asked::QuickConnect => front.quick_connect(&mut online, &mut link),
            };
            if connected == Connected::Reopened {
                exit.write(AppExit::Success);
            }
        }
    }
    let signature = format!("{visible}:{official}:{}", appearance(&front));
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    if visible && !official {
        spawn(&mut commands, &front);
    }
}

/// What the login screen shows of the front end, which draws it again when
/// it changes.
pub(super) fn appearance(front: &FrontEnd) -> String {
    format!(
        "{}:{}:{}:{}:{:?}:{}:{:?}",
        front.running(),
        front.chosen,
        front.account,
        front.password.chars().count(),
        front.field,
        front.status,
        front
            .servers
            .iter()
            .map(|server| (&server.name, &server.world, &server.availability))
            .collect::<Vec<_>>(),
    )
}

/// Applies the idle login screen's presses and typing; says whether the
/// player asked to log in.
fn take(
    front: &mut FrontEnd,
    pressed: &[Action],
    typing: &[&KeyboardInput],
    input: &ButtonInput<KeyCode>,
) -> Asked {
    let mut asked = if front.request.take() == Some(Request::Connect) {
        Asked::Connect
    } else {
        Asked::Nothing
    };
    for action in pressed {
        match action {
            Action::Server(index) => front.choose(*index),
            Action::NextServer if !front.servers.is_empty() => {
                front.choose((front.chosen + 1) % front.servers.len());
            }
            Action::Account => front.field = Field::Account,
            Action::Password => front.field = Field::Password,
            Action::Connect => asked = Asked::Connect,
            Action::QuickConnect => asked = Asked::QuickConnect,
            Action::NextServer | Action::Cancel | Action::Quit => (),
        }
    }
    if input.just_pressed(KeyCode::Tab) {
        front.field = match front.field {
            Field::Account => Field::Password,
            Field::Password | Field::None => Field::Account,
        };
    }
    if input.just_pressed(KeyCode::Escape) {
        front.field = Field::None;
    }
    let count = front.servers.len();
    if count != 0 && front.field == Field::None {
        if input.just_pressed(KeyCode::ArrowDown) {
            front.choose((front.chosen + 1) % count);
        } else if input.just_pressed(KeyCode::ArrowUp) {
            front.choose((front.chosen + count - 1) % count);
        }
    }
    if asked == Asked::Nothing
        && (input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::NumpadEnter))
    {
        asked = Asked::Connect;
    }
    for key in typing {
        let field = match front.field {
            Field::Account => &mut front.account,
            Field::Password => &mut *front.password,
            Field::None => continue,
        };
        match &key.logical_key {
            Key::Backspace => {
                field.pop();
            }
            Key::Character(text) => {
                // Room for the longest at the first character, so that the
                // box never moves a password and leaves a copy behind.
                if field.is_empty() {
                    field.reserve(LONGEST * 4);
                }
                for character in text.chars().filter(|c| !c.is_control()) {
                    if field.chars().count() < LONGEST {
                        field.push(character);
                    }
                }
            }
            _ => (),
        }
    }
    asked
}

/// Draws the window: the login servers, the boxes, the buttons and the
/// line under them.
fn spawn(commands: &mut Commands, front: &FrontEnd) {
    let logging_in = front.running();
    super::cover(commands, Root).with_children(|root| {
        root.spawn(super::panel()).with_children(|panel| {
            theme::label(panel, "LOGIN", Size::Display);
            theme::label(panel, "Login server", Size::Label);
            for (index, server) in front.servers.iter().enumerate() {
                let note = match &server.availability {
                    Availability::Here => String::new(),
                    Availability::Reopens(folder) => {
                        format!("Opens the client again with its installation, {folder}, named in login-servers.txt")
                    }
                    Availability::Unavailable(reason) => reason.clone(),
                };
                let usable =
                    !logging_in && !matches!(server.availability, Availability::Unavailable(_));
                let mut row = super::lit_button(
                    panel,
                    Action::Server(index),
                    &server.name,
                    Lit {
                        usable,
                        on: index == front.chosen,
                    },
                );
                if !note.is_empty() {
                    row.insert(crate::tooltip::Tooltip(note));
                }
            }
            field(
                panel,
                Action::Account,
                "Account",
                &front.account,
                front.field == Field::Account,
                logging_in,
            );
            let stars = "*".repeat(front.password.chars().count());
            field(
                panel,
                Action::Password,
                "Password",
                &stars,
                front.field == Field::Password,
                logging_in,
            );
            panel
                .spawn(Node {
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|buttons| {
                    let usable = !logging_in
                        && front.chosen().is_some_and(|server| {
                            !matches!(server.availability, Availability::Unavailable(_))
                        });
                    super::lit_button(
                        buttons,
                        Action::Connect,
                        "Connect",
                        Lit { usable, on: false },
                    );
                    super::lit_button(
                        buttons,
                        Action::Quit,
                        "Quit",
                        Lit {
                            usable: true,
                            on: false,
                        },
                    );
                });
            panel.spawn(theme::text(guidance(front), Size::Label, theme::INK));
        });
    });
}

/// A box the player types in, under its name.
fn field(
    panel: &mut ChildSpawnerCommands,
    action: Action,
    name: &str,
    shown: &str,
    focused: bool,
    locked: bool,
) {
    theme::label(panel, name, Size::Label);
    panel
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(8), px(6)),
                min_height: px(30),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::INSET),
            BorderColor::all(if focused && !locked {
                theme::FOCUS
            } else {
                theme::EDGE
            }),
        ))
        .with_child(theme::text(
            if focused && !locked {
                format!("{shown}_")
            } else {
                shown.to_owned()
            },
            Size::Large,
            if locked {
                theme::INK_DIM
            } else {
                theme::INK_BRIGHT
            },
        ));
}

/// The line under the buttons: what is happening, why the last login
/// ended, what Connect does on this login server, or how to use the window.
pub(super) fn guidance(front: &FrontEnd) -> String {
    if front.running() {
        return "Logging in... | Escape: stop".to_owned();
    }
    if !front.status.is_empty() {
        return front.status.clone();
    }
    match front.chosen().map(|server| &server.availability) {
        Some(Availability::Reopens(folder)) => reopens(folder),
        Some(Availability::Unavailable(reason)) => reason.clone(),
        _ => "Tab: next box | Enter: connect".to_owned(),
    }
}

/// What Connect does on a login server whose installation is another
/// folder, as the player reads it.
pub(super) fn reopens(folder: &str) -> String {
    format!("Connect opens the client again with {folder}")
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Heard, server};
    use super::*;
    use std::sync::{Arc, Mutex};

    fn app(servers: Vec<super::super::LoginServer>) -> (App, Arc<Mutex<Heard>>) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let fake = Fake {
            servers,
            heard: heard.clone(),
        };
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(FrontEnd::new(Box::new(fake)))
            .insert_resource(OnlineState::new(true))
            .insert_resource(crate::online::Updates(std::sync::Mutex::new(None)))
            .insert_resource(crate::outbox::Outbox::new(None))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (super::super::claim, form, super::super::watch).chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        (app, heard)
    }

    fn type_text(app: &mut App, text: &str) {
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        for character in text.chars() {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyA,
                logical_key: Key::Character(character.to_string().into()),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        }
        app.update();
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(key);
        input.clear();
    }

    #[test]
    fn the_login_screen_types_the_password_hidden_and_hands_it_over_once() {
        let (mut app, heard) = app(vec![server("Example", "remembered")]);
        app.update();
        // The remembered account fills its box, and the password box waits;
        // what is typed there is the login screen's alone.
        assert_eq!(app.world().resource::<FrontEnd>().field, Field::Password);
        assert!(app.world().resource::<crate::keys::Typing>().logging_in);
        type_text(&mut app, "secret");
        let shown: Vec<String> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        assert!(shown.iter().any(|text| text == "******_"));
        assert!(!shown.iter().any(|text| text.contains("secret")));
        press(&mut app, KeyCode::Enter);
        let front = app.world().resource::<FrontEnd>();
        assert!(front.running());
        assert_eq!(front.password.as_str(), "");
        assert_eq!(
            heard.lock().unwrap().connects,
            [(0, "remembered".to_owned(), "secret".to_owned())]
        );
        // The window shows the login under way and takes no more typing.
        type_text(&mut app, "more");
        assert_eq!(heard.lock().unwrap().connects.len(), 1);
        assert_eq!(app.world().resource::<FrontEnd>().password.as_str(), "");
    }

    #[test]
    fn a_login_that_ends_shows_the_login_screen_again_with_the_reason() {
        let (mut app, heard) = app(vec![server("Example", "remembered")]);
        app.world_mut().resource_mut::<FrontEnd>().account = "typed".into();
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Connect);
        app.update();
        let (updates, thread) = {
            let heard = heard.lock().unwrap();
            let (updates, _, thread) = &heard.sessions[0];
            (updates.clone(), thread.clone())
        };
        *thread.reason.lock().unwrap() = Some("Login account or password was rejected".into());
        updates
            .send(eq_client_core::WorldUpdate::Connection(
                eq_client_core::world::Link::Ended,
            ))
            .unwrap();
        // The news reaches the world as the session's receiver would hand it.
        let update = {
            let front = app.world().resource::<crate::online::Updates>();
            front
                .0
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .try_recv()
                .unwrap()
        };
        crate::online::testing::apply(&mut app.world_mut().resource_mut::<OnlineState>(), &update);
        app.update();
        let front = app.world().resource::<FrontEnd>();
        assert!(!front.running());
        assert_eq!(front.status, "Login account or password was rejected");
        // The account typed stays for another try.
        assert_eq!(front.account, "typed");
        assert!(super::super::Worker::finished(&thread));
        // The session's channels are let go.
        assert!(
            app.world()
                .resource::<crate::online::Updates>()
                .0
                .lock()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_login_server_on_another_installation_reopens_and_one_unavailable_says_why() {
        let mut other = server("Elsewhere", "someone");
        other.availability = Availability::Reopens("C:/Elsewhere".into());
        let mut missing = server("Missing", "");
        missing.availability = Availability::Unavailable("No installation".into());
        let (mut app, _) = app(vec![other, missing]);
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Connect);
        app.update();
        assert_ne!(app.world().resource::<Messages<AppExit>>().len(), 0);
        // Choosing the unavailable one says why, and Connect does nothing.
        app.world_mut().resource_mut::<FrontEnd>().choose(1);
        app.world_mut().resource_mut::<FrontEnd>().account = "someone".into();
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Connect);
        app.update();
        let front = app.world().resource::<FrontEnd>();
        assert!(!front.running());
        assert_eq!(front.status, "No installation");
    }
}

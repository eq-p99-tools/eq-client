//! The client's own list of the login server's worlds: each world's name and
//! how many play there, or that it is down or locked, with the world last
//! played on highlighted first. Up and Down browse, Enter or a second click
//! plays, and Escape goes back to the login screen.
use super::{FrontEnd, Link, Lit, Request, Screen};
use crate::{online::OnlineState, theme, theme::Size};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    servers::{ServerChoice, ServerRefusal, ServerStatus},
};
use std::time::Duration;

/// How soon a second click on a highlighted world plays on it.
const SECOND_CLICK: Duration = Duration::from_millis(500);

/// The installed client's login strings, `eqlsstr_us.txt`, by which a
/// Titanium login server says why it refused a world.
#[derive(Resource, Default)]
pub(crate) struct LoginStrings(pub(crate) eq_client_assets::strings::StringTable);

impl LoginStrings {
    /// The installation's table, or an empty one.
    pub(crate) fn load(directory: Option<&std::path::Path>) -> Self {
        Self(
            directory
                .and_then(|directory| {
                    eq_client_assets::strings::StringTable::read_login(directory).ok()
                })
                .unwrap_or_default(),
        )
    }
}

/// The window's root.
#[derive(Component)]
pub(crate) struct Root;

/// The scrolling list of worlds.
#[derive(Component)]
pub(crate) struct Rows;

/// The space between two worlds' rows.
const GAP: f32 = 4.0;

/// What a control of the window does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Highlights a world, by its place; a second click plays on it.
    World(usize),
    /// Plays on the highlighted world.
    Play,
    /// Goes back to the login screen.
    Back,
}

/// The world a list highlights first: the one last played on, where it is
/// listed and takes players, or else the first that does.
pub(crate) fn first(servers: &[ServerChoice], last: Option<&str>) -> Option<usize> {
    let open = |server: &&ServerChoice| server.status.open();
    last.and_then(|last| {
        servers
            .iter()
            .position(|server| server.name.eq_ignore_ascii_case(last) && server.status.open())
    })
    .or_else(|| servers.iter().position(|server| open(&server)))
}

/// Takes the player's presses while the list shows, and draws the window
/// again when what it shows changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn worlds(
    mut commands: Commands,
    (mut front, mut online, mut link): (ResMut<FrontEnd>, ResMut<OnlineState>, Link),
    keys: crate::keys::Keys,
    strings: Option<Res<LoginStrings>>,
    buttons: Query<(Ref<Interaction>, &Action)>,
    roots: Query<Entity, With<Root>>,
    lists: Query<&ScrollPosition, With<Rows>>,
    time: Res<Time<Real>>,
    mut clicked: Local<Option<(usize, Duration)>>,
    mut previous: Local<String>,
    mut built: Local<Option<u64>>,
) {
    let visible = front.screen(&online) == Screen::Servers;
    if visible && keys.window_focused() {
        let asked = online
            .world()
            .servers()
            .is_some_and(|list| list.asked.is_some());
        let pressed: Vec<_> = buttons
            .iter()
            .filter(|(interaction, _)| {
                interaction.is_changed() && **interaction == Interaction::Pressed
            })
            .map(|(_, action)| *action)
            .collect();
        if pressed.contains(&Action::Back) || keys.input.just_pressed(KeyCode::Escape) {
            front.end(None, &mut link);
        } else if !asked {
            let now = time.elapsed();
            let mut play = front.request.take() == Some(Request::Play)
                || pressed.contains(&Action::Play)
                || keys.input.just_pressed(KeyCode::Enter)
                || keys.input.just_pressed(KeyCode::NumpadEnter);
            for action in &pressed {
                if let Action::World(index) = action {
                    let again = clicked.is_some_and(|(last, at)| {
                        last == *index && now.saturating_sub(at) <= SECOND_CLICK
                    });
                    highlight(&mut front, &online, *index);
                    play |= again && front.highlighted == Some(*index);
                    *clicked = Some((*index, now));
                }
            }
            browse(&mut front, &online, &keys.input);
            if play && let Err(refusal) = ask(&mut front, &mut online, &link) {
                front.status = refusal;
            }
        }
    }
    let list = online.world().servers();
    let signature = format!(
        "{visible}:{:?}:{:?}:{}",
        list.map(|list| (list.selection_id, &list.servers, list.asked, &list.refused)),
        front.highlighted,
        front.status,
    );
    if *previous == signature {
        return;
    }
    *previous = signature;
    // The same list, drawn again, keeps its place; a new one starts at the top.
    let same = list.is_some_and(|list| *built == Some(list.selection_id));
    let offset = lists
        .iter()
        .next()
        .filter(|_| same)
        .map_or(0.0, |position| position.y);
    for root in &roots {
        commands.entity(root).despawn();
    }
    *built = None;
    if visible && let Some(list) = list {
        let strings = strings.as_deref().map(|strings| &strings.0);
        spawn(&mut commands, list, (&front, offset), strings);
        *built = Some(list.selection_id);
    }
}

/// Scrolls the list of worlds as the wheel turns over it, and keeps the
/// highlighted world in view as Up and Down browse.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn scroll(
    front: Res<FrontEnd>,
    wheel: Res<crate::windows::pointer::Wheel>,
    mut lists: Query<
        (
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
        ),
        With<Rows>,
    >,
    rows: Query<(&Action, &ComputedNode, &UiGlobalTransform)>,
    mut followed: Local<Option<usize>>,
) {
    let Ok((entity, list, place, mut position)) = lists.single_mut() else {
        *followed = None;
        return;
    };
    if wheel.surface == Some(entity) {
        crate::windows::scroll_by(&mut position, list, wheel.pixels);
    }
    if front.highlighted == *followed {
        return;
    }
    let Some(index) = front.highlighted else {
        *followed = None;
        return;
    };
    let Some((row, row_place)) = rows
        .iter()
        .find(|(action, ..)| **action == Action::World(index))
        .map(|(_, node, place)| (node, place))
    else {
        return;
    };
    // A row drawn this frame has no size until the layout places it.
    if row.size().y <= 0.0 {
        return;
    }
    let edges = |node: &ComputedNode, place: &UiGlobalTransform| {
        let half = node.size().y / 2.0;
        (place.translation.y - half, place.translation.y + half)
    };
    let (top, bottom) = edges(list, place);
    let (row_top, row_bottom) = edges(row, row_place);
    let scale = list.inverse_scale_factor();
    if row_top < top {
        crate::windows::scroll_by(&mut position, list, (top - row_top) * scale);
    } else if row_bottom > bottom {
        crate::windows::scroll_by(&mut position, list, (bottom - row_bottom) * scale);
    }
    *followed = Some(index);
}

/// Highlights a world that takes players.
fn highlight(front: &mut FrontEnd, online: &OnlineState, index: usize) {
    let open = online
        .world()
        .servers()
        .and_then(|list| list.servers.get(index))
        .is_some_and(|server| server.status.open());
    if open {
        front.highlighted = Some(index);
        front.status.clear();
    }
}

/// Moves the highlight to the next or previous world that takes players.
fn browse(front: &mut FrontEnd, online: &OnlineState, input: &ButtonInput<KeyCode>) {
    let down = input.just_pressed(KeyCode::ArrowDown);
    if !down && !input.just_pressed(KeyCode::ArrowUp) {
        return;
    }
    let Some(list) = online.world().servers() else {
        return;
    };
    let count = list.servers.len();
    let start = front
        .highlighted
        .unwrap_or(if down { count - 1 } else { 0 });
    let next = (1..=count)
        .map(|step| {
            if down {
                (start + step) % count
            } else {
                (start + count - step % count) % count
            }
        })
        .find(|index| list.servers[*index].status.open());
    if let Some(next) = next {
        front.highlighted = Some(next);
        front.status.clear();
    }
}

/// Asks to play on the highlighted world.
///
/// # Errors
/// Says why the request did not leave.
fn ask(front: &mut FrontEnd, online: &mut OnlineState, link: &Link) -> Result<(), String> {
    let list = online
        .world()
        .servers()
        .ok_or_else(|| "The login server's list is gone".to_owned())?;
    let index = front
        .highlighted
        .ok_or_else(|| "Choose a world first".to_owned())?;
    let server = list
        .servers
        .get(index)
        .filter(|server| server.status.open())
        .ok_or_else(|| "That world takes no players now".to_owned())?;
    let (selection_id, name) = (list.selection_id, server.name.clone());
    link.send(
        online.world(),
        ClientCommand::SelectServer {
            selection_id,
            index,
        },
    )
    .map_err(|refusal| refusal.text().to_owned())?;
    online.ask_server(index);
    front.chose_world(selection_id, &name);
    front.status.clear();
    Ok(())
}

/// What a world's row says after its name: how many play there, or that it
/// is down or locked, and whether the list marks it preferred.
fn detail(server: &ServerChoice) -> String {
    let state = match (server.status, server.players) {
        (ServerStatus::Up, Some(players)) => format!("{players} players"),
        (ServerStatus::Up, None) => "Up".to_owned(),
        (ServerStatus::Down, _) => "Down".to_owned(),
        (ServerStatus::Locked, _) => "Locked".to_owned(),
    };
    if server.preferred {
        format!("{state} | Preferred")
    } else {
        state
    }
}

/// Why the login server refused a world: the installed client's words for
/// a Titanium login server's string, the login server's own words, or ours.
fn refusal_text(
    refusal: &ServerRefusal,
    strings: Option<&eq_client_assets::strings::StringTable>,
) -> String {
    match refusal {
        ServerRefusal::Message(id) => strings
            .and_then(|strings| strings.argument_free(*id))
            .map_or_else(
                || format!("The login server refused that world (login string {id})"),
                str::to_owned,
            ),
        ServerRefusal::Text(text) => text.trim().to_owned(),
    }
}

/// The line under the list: why the last world was refused, the world being
/// asked for, or how to use the list.
fn guidance(
    list: &eq_client_core::world::ServerList,
    front: &FrontEnd,
    strings: Option<&eq_client_assets::strings::StringTable>,
) -> String {
    if let Some(index) = list.asked {
        let name = list
            .servers
            .get(index)
            .map_or("", |server| server.name.as_str());
        return format!("Connecting to {name}... | Escape: back");
    }
    if !front.status.is_empty() {
        return front.status.clone();
    }
    if let Some(refusal) = &list.refused {
        return refusal_text(refusal, strings);
    }
    if list.servers.iter().any(|server| server.status.open()) {
        "Up/Down: browse | Enter: play | Escape: back".to_owned()
    } else {
        "No world takes players now | Escape: back".to_owned()
    }
}

/// Draws the window: a row for each world, the buttons and the line under
/// them.
fn spawn(
    commands: &mut Commands,
    list: &eq_client_core::world::ServerList,
    (front, offset): (&FrontEnd, f32),
    strings: Option<&eq_client_assets::strings::StringTable>,
) {
    let asked = list.asked.is_some();
    super::cover(commands, Root).with_children(|root| {
        root.spawn(super::panel()).with_children(|panel| {
            theme::label(panel, "SERVERS", Size::Display);
            if let Some(server) = front.chosen() {
                panel.spawn(theme::text(server.name.clone(), Size::Label, theme::INK));
            }
            // A login server may list hundreds of worlds.
            panel
                .spawn((
                    Rows,
                    crate::windows::pointer::TakesWheel,
                    ScrollPosition(Vec2::new(0.0, offset)),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(GAP),
                        max_height: Val::Vh(55.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|rows| {
                    for (index, server) in list.servers.iter().enumerate() {
                        let open = server.status.open();
                        let mut row = super::lit_button(
                            rows,
                            Action::World(index),
                            &format!("{}  |  {}", server.name, detail(server)),
                            Lit {
                                usable: open && !asked,
                                on: front.highlighted == Some(index),
                            },
                        );
                        if !open {
                            row.insert(crate::tooltip::Tooltip(
                                "This world takes no players now".to_owned(),
                            ));
                        }
                    }
                });
            panel
                .spawn(Node {
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|buttons| {
                    let usable = !asked && front.highlighted.is_some();
                    super::lit_button(buttons, Action::Play, "Play", Lit { usable, on: false });
                    super::lit_button(
                        buttons,
                        Action::Back,
                        "Back",
                        Lit {
                            usable: true,
                            on: false,
                        },
                    );
                });
            panel.spawn(theme::text(
                guidance(list, front, strings),
                Size::Label,
                theme::INK,
            ));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Heard, server};
    use super::*;
    use eq_client_core::WorldEvent;
    use std::sync::{Arc, Mutex};

    fn world(name: &str, status: ServerStatus, players: Option<u32>) -> ServerChoice {
        ServerChoice {
            name: name.into(),
            status,
            players,
            preferred: false,
        }
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    #[test]
    fn the_list_highlights_the_last_world_played_where_it_takes_players() {
        let servers = [
            world("Down", ServerStatus::Down, None),
            world("First", ServerStatus::Up, Some(3)),
            world("Last", ServerStatus::Up, Some(9)),
        ];
        assert_eq!(first(&servers, Some("last")), Some(2));
        assert_eq!(first(&servers, Some("Down")), Some(1));
        assert_eq!(first(&servers, None), Some(1));
        assert_eq!(first(&servers[..1], None), None);
        assert_eq!(detail(&servers[2]), "9 players");
        let mut locked = world("Locked", ServerStatus::Locked, None);
        locked.preferred = true;
        assert_eq!(detail(&locked), "Locked | Preferred");
    }

    #[test]
    fn a_refusal_reads_in_the_installed_words_or_the_servers_or_ours() {
        let table =
            eq_client_assets::strings::StringTable::parse("EQST0002\n0\n326 Example refusal\n");
        assert_eq!(
            refusal_text(&ServerRefusal::Message(326), Some(&table)),
            "Example refusal"
        );
        assert_eq!(
            refusal_text(&ServerRefusal::Message(327), Some(&table)),
            "The login server refused that world (login string 327)"
        );
        assert_eq!(
            refusal_text(&ServerRefusal::Text(" Its own words ".into()), None),
            "Its own words"
        );
    }

    /// An app whose session logs in on a login server that remembers the
    /// world "Busy", and lists three worlds, the first of them down.
    fn listed() -> (App, Arc<Mutex<Heard>>) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let mut remembered = server("Example", "someone");
        remembered.world = Some("Busy".into());
        let fake = Fake {
            servers: vec![remembered],
            heard: heard.clone(),
        };
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(FrontEnd::new(Box::new(fake)))
            .insert_resource(OnlineState::new(true))
            .insert_resource(crate::online::Updates(std::sync::Mutex::new(None)))
            .insert_resource(crate::outbox::Outbox::new(None))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time<Real>>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (super::super::form, super::super::watch, worlds).chain(),
            );
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Connect);
        app.update();
        assert!(app.world().resource::<FrontEnd>().running());
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::ServerSelection {
                selection_id: 5,
                servers: vec![
                    world("Down", ServerStatus::Down, None),
                    world("Busy", ServerStatus::Up, Some(40)),
                    world("Quiet", ServerStatus::Up, Some(2)),
                ],
            }],
        );
        app.update();
        (app, heard)
    }

    #[test]
    fn the_list_keeps_its_place_when_drawn_again() {
        let (mut app, _) = listed();
        let offset = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&mut ScrollPosition, With<Rows>>()
                .single_mut(app.world_mut())
                .unwrap()
                .y
        };
        app.world_mut()
            .query_filtered::<&mut ScrollPosition, With<Rows>>()
            .single_mut(app.world_mut())
            .unwrap()
            .y = 50.0;
        // Browsing draws the list again, where it was.
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(2));
        assert!((offset(&mut app) - 50.0).abs() < 0.01);
        // A new list starts at the top.
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::ServerSelection {
                selection_id: 6,
                servers: vec![world("Other", ServerStatus::Up, None)],
            }],
        );
        app.update();
        assert!(offset(&mut app).abs() < 0.01);
    }

    #[test]
    fn the_player_plays_on_an_open_world_and_chooses_again_after_a_refusal() {
        // A session is logging in, and its login server lists its worlds.
        let (mut app, heard) = listed();
        // The remembered world is highlighted first, and Enter plays there.
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(1));
        press(&mut app, KeyCode::Enter);
        let queue = |heard: &Arc<Mutex<Heard>>| {
            let heard = heard.lock().unwrap();
            std::iter::from_fn(|| heard.sessions[0].1.try_recv().ok()).collect::<Vec<_>>()
        };
        assert_eq!(
            queue(&heard),
            [ClientCommand::SelectServer {
                selection_id: 5,
                index: 1
            }]
        );
        // Asked, the list takes no other choice until the answer.
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(1));
        // Refused, the player browses past the down world and plays again.
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::ServerRefused {
                selection_id: 5,
                refusal: ServerRefusal::Message(326),
            }],
        );
        press(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<FrontEnd>().highlighted, Some(2));
        app.world_mut().resource_mut::<FrontEnd>().request = Some(Request::Play);
        app.update();
        assert_eq!(
            queue(&heard),
            [ClientCommand::SelectServer {
                selection_id: 5,
                index: 2
            }]
        );
        // The world's character list says the session played there.
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::CharacterSelection {
                selection_id: 1,
                characters: Vec::new(),
            }],
        );
        app.update();
        assert_eq!(
            heard.lock().unwrap().played,
            [(0, "someone".to_owned(), Some("Quiet".to_owned()))]
        );
    }
}

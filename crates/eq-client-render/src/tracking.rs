//! The track window, as the Titanium program keeps it (read R14 section 2,
//! R15a): the Tracking press opens it, the server's list fills it in its
//! own order, each name in the colour of its level against the player's,
//! a click chooses an entry, Track follows it and closes the window,
//! Cancel stops tracking and closes it as `/stoptracking` does, and the
//! close box only closes it. The skin's `TrackingWnd` draws it
//! where the skin has one; elsewhere the client's own panel shows the same
//! list and Track button.
//!
//! The options (read R49-30 e, checklist 6.638): a Ranger's `/tracksort`
//! orders the list by the server's distances and is saved at once; a
//! Ranger's `/trackfilter` hides NPCs by colour for the session; anyone's
//! `/trackplayers` lists all players, none, or the group's, and is saved.
//! Another class's sort or filter gets the program's Rangers-only line. The
//! skin's controls for them stay greyed until the skin's control names are
//! read (6.446).
use crate::{
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{self, Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    combat::ConColor,
    tracking::TrackAction,
    world::{ClientWorld, track_color},
};

/// The Ranger's class.
const RANGER: u32 = 4;

/// The line for another class's `/tracksort` or `/trackfilter`, an
/// `eqstr_us.txt` string ID (read R14 section 2).
const RANGERS_ONLY: u32 = 0x3363;

/// A typed track option, waiting for its answer: the command's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TrackOption {
    /// `/tracksort`.
    Sort(String),
    /// `/trackfilter`.
    Filter(String),
    /// `/trackplayers`.
    Players(String),
}

impl TrackOption {
    /// The option a typed line, spelled out, asks for.
    pub(crate) fn typed(command: &str) -> Option<Self> {
        let command = command.strip_prefix('/')?;
        let (name, words) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        let words = words.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "tracksort" => Some(Self::Sort(words)),
            "trackfilter" => Some(Self::Filter(words)),
            "trackplayers" => Some(Self::Players(words)),
            _ => None,
        }
    }
}

/// Whether the player is a Ranger; None while the class is unknown.
fn ranger(world: &ClientWorld) -> Option<bool> {
    world
        .player()
        .and_then(|player| player.class)
        .map(|class| class == RANGER)
}

/// Answers a typed track option with the program's line, and sets it: the
/// sort and the players choice in the character's options, which saves
/// them; the filter for the session only.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn answer_option(
    mut chat: ResMut<crate::chat::ChatState>,
    online: Res<OnlineState>,
    options: Option<ResMut<crate::options::OptionsState>>,
    messages: Option<Res<crate::hud::messages::Messages>>,
) {
    use eq_client_core::track_options::{
        FILTER_CLEARED, FILTER_COLOURS, FILTER_SET, PLAYERS_UNKNOWN, SORT_UNKNOWN, TrackPlayers,
        TrackSort,
    };
    let Some(option) = chat.track_option.take() else {
        return;
    };
    let empty = crate::hud::messages::Messages::default();
    let messages = messages.as_deref().unwrap_or(&empty);
    let rangers = !matches!(option, TrackOption::Players(_));
    match ranger(online.world()) {
        // The class is unknown before the profile: nothing to answer yet.
        None => return,
        Some(false) if rangers => {
            let said = messages.said_text(
                RANGERS_ONLY,
                "Sorting and filtering tracking are for Rangers.",
            );
            chat.history.push(crate::chat::system_line(said));
            return;
        }
        Some(_) => (),
    }
    let Some(mut options) = options else {
        return;
    };
    let said = match option {
        TrackOption::Sort(word) => match TrackSort::typed(&word) {
            Some(sort) => {
                options.options.track_sort = sort;
                messages.said_text(sort.line(), "Tracking sort set.")
            }
            None => messages.said_text(SORT_UNKNOWN, "Use /tracksort N, D, C, RD or RC."),
        },
        TrackOption::Players(word) => {
            let chosen = TrackPlayers::ALL.into_iter().find(|players| {
                messages
                    .argument_free(players.word_line())
                    .unwrap_or(players.own_word())
                    .eq_ignore_ascii_case(&word)
            });
            match chosen {
                Some(players) => {
                    options.options.track_players = players;
                    messages.said_text(players.line(), "Tracking players set.")
                }
                None => messages.said_text(PLAYERS_UNKNOWN, "Use /trackplayers on, off or group."),
            }
        }
        TrackOption::Filter(word) => {
            let colour = FILTER_COLOURS.iter().find_map(|(_, bit, line, own)| {
                let said = messages.argument_free(*line).unwrap_or(own);
                said.eq_ignore_ascii_case(&word)
                    .then(|| (*bit, said.to_owned()))
            });
            // A word for no colour: the program's answer is unread [I], so
            // nothing is said.
            let Some((bit, said)) = colour else {
                return;
            };
            let set = options.track_filter.toggle(bit);
            let (line, own) = if set {
                (FILTER_SET, "Tracking now hides that colour.")
            } else {
                (FILTER_CLEARED, "Tracking now shows that colour.")
            };
            messages.said_or(line, &[said], own)
        }
    };
    chat.history.push(crate::chat::system_line(said));
}

/// The track window's Track button.
#[derive(Component)]
pub(crate) struct TrackButton;

/// The track window's Cancel, which stops tracking as `/stoptracking` does
/// and closes the window (read R15d: the constructor 0x5663a4 binds
/// `DoneButton` to 0x566312). The close box only closes it [I].
#[derive(Component)]
pub(crate) struct StopButton;

/// A row of the track window's list: the spawn it names, and its place
/// from the top.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackRow {
    pub(crate) spawn_id: u16,
    pub(crate) index: usize,
}

/// The track window's list, which the client fills from the server's list.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct TrackRows {
    /// Each column's width, from the left.
    columns: Vec<f32>,
    /// The rows last drawn, to redraw only when one changes.
    drawn: Option<Vec<Row>>,
    /// Inputs last projected; None also covers a newly created window.
    checked: Option<Inputs>,
}

impl TrackRows {
    pub(crate) fn new(columns: Vec<f32>) -> Self {
        Self {
            columns,
            drawn: None,
            checked: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inputs {
    session: Option<u64>,
    revision: u64,
    level: Option<u8>,
    choices: Choices,
    /// Changes with the group, which `/trackplayers group` lists by.
    group: u64,
}

/// The options that shape the list: the saved sort and players choice and
/// the session's filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Choices {
    pub(crate) sort: eq_client_core::track_options::TrackSort,
    pub(crate) filter: eq_client_core::track_options::TrackFilter,
    pub(crate) players: eq_client_core::track_options::TrackPlayers,
}

impl Choices {
    /// The choices the character's options hold.
    pub(crate) fn of(options: Option<&crate::options::OptionsState>) -> Self {
        options.map_or_else(Self::default, |options| Self {
            sort: options.options.track_sort,
            filter: options.track_filter,
            players: options.options.track_players,
        })
    }
}

/// A hash of the group's names, so the list redraws when it changes.
fn group_key(world: &ClientWorld) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(group) = world.group() {
        group.leader.hash(&mut hasher);
        group.members.hash(&mut hasher);
    }
    hasher.finish()
}

/// A row as drawn: the spawn, its name, its colour and whether it is the one
/// chosen.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    spawn_id: u16,
    name: String,
    color: Option<ConColor>,
    chosen: bool,
}

/// The listed spawns the zone still knows, as the program lists them
/// (0x566194, 0x566ce6 and 0x566fe6): players by the players choice; for a
/// Ranger, NPCs hidden by the filter's colours and the rows in the sort's
/// order; for anyone else, the server's order with nothing filtered.
fn rows(world: &ClientWorld, choices: Choices) -> Vec<Row> {
    use eq_client_core::{SpawnKind, track_options::TrackPlayers};
    let window = world.track_window();
    let yours = world.player().map(|player| player.level);
    let ranger = ranger(world) == Some(true);
    let grouped = |name: &str| {
        world.group().is_some_and(|group| {
            group
                .leader
                .iter()
                .chain(&group.members)
                .any(|member| member.eq_ignore_ascii_case(name))
        })
    };
    let mut listed: Vec<(Row, f32)> = window
        .entries
        .iter()
        .filter_map(|entry| {
            let spawn = &world.spawn(entry.spawn_id)?.state;
            let color = yours
                .filter(|_| spawn.level != 0)
                .map(|yours| track_color(yours, spawn.level));
            let shown = if spawn.kind == SpawnKind::Player {
                match choices.players {
                    TrackPlayers::On => true,
                    TrackPlayers::Off => false,
                    TrackPlayers::Group => grouped(&spawn.name),
                }
            } else {
                !(ranger && color.is_some_and(|color| choices.filter.hides(color)))
            };
            shown.then(|| {
                (
                    Row {
                        spawn_id: entry.spawn_id,
                        name: eq_client_core::entities::display_name(&spawn.name),
                        color,
                        chosen: window.selected == Some(entry.spawn_id),
                    },
                    entry.distance,
                )
            })
        })
        .collect();
    if ranger {
        choices.sort.order(&mut listed, |(_, distance)| *distance);
    }
    listed.into_iter().map(|(row, _)| row).collect()
}

/// Keeps the window with the world's: it opens when the press reaches the
/// server and closes when Track follows an entry, `/stoptracking` stops or
/// the zone ends. A window the player closes, with Cancel or otherwise,
/// closes the world's too.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut online: ResMut<OnlineState>,
    mut shown: ResMut<Shown>,
    mut was: Local<bool>,
) {
    let open = online.world().track_window().open;
    match (open, *was) {
        (true, false) => shown.open(WindowId::Tracking),
        (false, true) => shown.close(WindowId::Tracking),
        (true, true) if !shown.is_open(WindowId::Tracking) => online.close_track_window(),
        _ => (),
    }
    *was = online.world().track_window().open;
}

/// Chooses the clicked row's spawn, follows the chosen one when Track is
/// pressed, and stops tracking when Cancel is.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    rows: Query<(&Interaction, &TrackRow), Changed<Interaction>>,
    (track, stop): (
        crate::clicks::Clicks<TrackButton>,
        crate::clicks::Clicks<StopButton>,
    ),
) {
    // The session sends the stop only while following, as the program
    // stops only an active tracking.
    if stop.any() {
        let _ = outbox.post(online.world(), |stamp| ClientCommand::Track {
            session_id: stamp.session_id,
            action: TrackAction::Stop,
        });
        online.close_track_window();
    }
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed
            && online.world().track_window().selected != Some(row.spawn_id)
        {
            online.select_tracked(Some(row.spawn_id));
        }
    }
    if track.any()
        && let Some(spawn_id) = online.world().track_window().selected
    {
        let _ = outbox.post(online.world(), |stamp| ClientCommand::Track {
            session_id: stamp.session_id,
            action: TrackAction::Follow(spawn_id),
        });
    }
}

/// The height of a row of the list.
const ROW_HEIGHT: f32 = 16.0;

/// Fills the track window's list again whenever a row changes: a new list,
/// the choice or the player's level.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn fill(
    mut commands: Commands,
    (online, options): (Res<OnlineState>, Option<Res<crate::options::OptionsState>>),
    mut lists: Query<(Entity, &mut TrackRows, Option<&Children>)>,
) {
    let world = online.world();
    let choices = Choices::of(options.as_deref());
    let input = Inputs {
        session: world.session_id(),
        revision: world.track_window().revision,
        level: world.player().map(|player| player.level),
        choices,
        group: group_key(world),
    };
    if lists.iter().all(|(_, list, _)| list.checked == Some(input)) {
        return;
    }
    let wanted = rows(world, choices);
    for (entity, mut list, children) in &mut lists {
        if list.checked == Some(input) {
            continue;
        }
        list.checked = Some(input);
        if list.drawn.as_ref() == Some(&wanted) {
            continue;
        }
        if let Some(children) = children {
            for child in children {
                commands.entity(*child).despawn();
            }
        }
        let width = list.columns.first().copied().unwrap_or(150.0);
        for (index, row) in wanted.iter().enumerate() {
            let line = commands
                .spawn((
                    Button,
                    TrackRow {
                        spawn_id: row.spawn_id,
                        index,
                    },
                    Node {
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if row.chosen {
                        theme::BUTTON
                    } else {
                        Color::NONE
                    }),
                    ChildOf(entity),
                ))
                .id();
            commands.spawn((
                theme::text(row.name.clone(), Size::Small, theme::con(row.color)),
                TextLayout::new(Justify::Left, LineBreak::NoWrap),
                Node {
                    width: px(width),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ChildOf(line),
            ));
        }
        list.drawn = Some(wanted.clone());
    }
}

/// The client's own track window's width.
const OWN_WIDTH: f32 = 180.0;

/// Gives the open track window its frame. The client's own panel goes in
/// it, the list with Track and Cancel under it; where the skin has a track window, the
/// skin draws over that panel ([`crate::skinned::apply`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn frame(
    mut commands: Commands,
    (shown, mut stack): (Res<Shown>, ResMut<windows::Stack>),
    frames: Query<(Entity, &WindowId), With<windows::Frame>>,
) {
    let displayed = shown.displayed(WindowId::Tracking);
    let mut exists = false;
    for (entity, id) in &frames {
        if *id != WindowId::Tracking {
            continue;
        }
        if displayed {
            exists = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !displayed || exists {
        return;
    }
    // Each opening comes to the front, so Escape closes it first.
    stack.raise(WindowId::Tracking);
    let frame = windows::frame(
        &mut commands,
        WindowId::Tracking,
        Node {
            width: px(OWN_WIDTH),
            padding: UiRect::all(px(6)),
            row_gap: px(4),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    commands.entity(frame).with_children(|panel| {
        panel.spawn((
            TrackRows::new(vec![OWN_WIDTH - 12.0]),
            windows::pointer::TakesWheel,
            Node {
                flex_direction: FlexDirection::Column,
                min_height: px(ROW_HEIGHT * 4.0),
                max_height: px(320),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ));
        panel
            .spawn(Node {
                column_gap: px(6),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                let button = Node {
                    padding: UiRect::axes(px(8), px(2)),
                    ..default()
                };
                row.spawn((
                    Button,
                    TrackButton,
                    crate::outbox::Needs::Capability(eq_client_core::Capability::Tracking),
                    button.clone(),
                    BackgroundColor(theme::BUTTON),
                ))
                .with_children(|button| {
                    // The client's own words for the skin's two buttons.
                    button.spawn(theme::text("Track", Size::Body, theme::INK));
                });
                row.spawn((
                    Button,
                    StopButton,
                    crate::skinned::Closes(WindowId::Tracking),
                    button,
                    BackgroundColor(theme::BUTTON),
                ))
                .with_children(|button| {
                    button.spawn(theme::text("Cancel", Size::Body, theme::INK));
                });
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        WorldEvent,
        tracking::{Tracked, TrackingUpdate},
    };

    fn listed(ids: &[u16]) -> WorldEvent {
        WorldEvent::Tracking(TrackingUpdate::Listed(
            ids.iter()
                .map(|spawn_id| Tracked {
                    spawn_id: *spawn_id,
                    distance: 10.0,
                })
                .collect(),
        ))
    }

    fn admitted() -> OnlineState {
        let mut online = OnlineState::new(true);
        let mut player = testing::player(7);
        player.level = 20;
        testing::admit(&mut online, 1, player);
        let creature = |spawn_id, name: &str, level| {
            let mut spawn = testing::pet(spawn_id, 0);
            spawn.pet_owner = None;
            spawn.name = name.into();
            spawn.level = level;
            spawn
        };
        testing::spawns(
            &mut online,
            vec![creature(30, "a_bat00", 14), creature(31, "a_snake00", 23)],
        );
        online
    }

    fn pressed() -> WorldEvent {
        WorldEvent::AbilityUsed {
            session_id: 1,
            ability: eq_client_core::abilities::Ability::Tracking,
            ready_in: std::time::Duration::from_secs(10),
            string_id: None,
        }
    }

    #[test]
    fn the_rows_keep_the_servers_order_and_colour_each_name_by_level() {
        let mut online = admitted();
        testing::news(&mut online, [listed(&[31, 99, 30])]);
        let rows = rows(online.world(), Choices::default());
        // The unknown spawn is left out, as the program skips it.
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["a snake", "a bat"]);
        assert_eq!(rows[0].color, Some(ConColor::Red));
        // Level 14 against 20 is near the green edge: light blue.
        assert_eq!(rows[1].color, Some(ConColor::LightBlue));
        online.select_tracked(Some(30));
        assert!(super::rows(online.world(), Choices::default())[1].chosen);
    }

    /// Admits a player of this class, keeping the zone's spawns.
    fn of_class(online: &mut OnlineState, class: Option<u32>) {
        let mut player = testing::player(7);
        player.level = 20;
        player.class = class;
        testing::admit(online, 1, player);
    }

    #[test]
    fn a_rangers_choices_shape_the_list_and_anothers_keeps_the_servers() {
        use eq_client_core::track_options::{TrackFilter, TrackPlayers, TrackSort};
        // A list of a red snake 30 away, a player 5 away and a green bat 20
        // away, for a level-20 player of this class.
        let listing = |class| {
            use eq_client_core::SpawnKind::{Npc, Player};
            let mut online = OnlineState::new(true);
            of_class(&mut online, class);
            let creature = |spawn_id, name: &str, level, kind| {
                let mut spawn = testing::pet(spawn_id, 0);
                spawn.pet_owner = None;
                spawn.name = name.into();
                spawn.level = level;
                spawn.kind = kind;
                spawn
            };
            testing::spawns(
                &mut online,
                vec![
                    creature(30, "a_bat00", 14, Npc),
                    creature(31, "a_snake00", 23, Npc),
                    creature(32, "Friend", 20, Player),
                ],
            );
            testing::news(
                &mut online,
                [WorldEvent::Tracking(TrackingUpdate::Listed(
                    [(31, 30.0), (32, 5.0), (30, 20.0)]
                        .into_iter()
                        .map(|(spawn_id, distance)| Tracked { spawn_id, distance })
                        .collect(),
                ))],
            );
            online
        };
        let ids = |online: &OnlineState, choices| {
            rows(online.world(), choices)
                .iter()
                .map(|row| row.spawn_id)
                .collect::<Vec<_>>()
        };
        let sorted = |sort| Choices {
            sort,
            ..Choices::default()
        };
        let online = listing(Some(RANGER));
        assert_eq!(ids(&online, Choices::default()), [31, 32, 30]);
        assert_eq!(ids(&online, sorted(TrackSort::Distance)), [32, 30, 31]);
        assert_eq!(
            ids(&online, sorted(TrackSort::ReverseDistance)),
            [31, 30, 32]
        );
        assert_eq!(ids(&online, sorted(TrackSort::Consider)), [31, 32, 30]);
        assert_eq!(
            ids(&online, sorted(TrackSort::ReverseConsider)),
            [31, 32, 30]
        );
        // The bat (green, or light blue near the edge) hides; a player is
        // never filtered by colour.
        let filtered = Choices {
            filter: TrackFilter(0x1 | 0x4 | 0x20),
            ..Choices::default()
        };
        assert_eq!(ids(&online, filtered), [31, 32]);
        // Players: none, or only the group's.
        let players = |players| Choices {
            players,
            ..Choices::default()
        };
        assert_eq!(ids(&online, players(TrackPlayers::Off)), [31, 30]);
        assert_eq!(ids(&online, players(TrackPlayers::Group)), [31, 30]);
        // Another class: the server's order, nothing filtered by colour,
        // but the players choice still holds.
        let online = listing(Some(1));
        let all = Choices {
            sort: TrackSort::Distance,
            filter: TrackFilter(0x3f),
            players: TrackPlayers::Off,
        };
        assert_eq!(ids(&online, all), [31, 30]);
        assert_eq!(
            ids(
                &online,
                Choices {
                    players: TrackPlayers::On,
                    ..all
                }
            ),
            [31, 32, 30]
        );
    }

    fn answering(class: Option<u32>) -> App {
        let mut online = admitted();
        of_class(&mut online, class);
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::options::OptionsState::default())
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, answer_option);
        app
    }

    fn answer(app: &mut App, option: TrackOption) -> Option<String> {
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .track_option = Some(option);
        app.update();
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .last()
            .map(|(_, line)| line.message.text.clone())
    }

    #[test]
    fn each_option_word_sets_its_choice_and_says_its_line() {
        use eq_client_core::track_options::{TrackFilter, TrackPlayers, TrackSort};
        assert_eq!(
            TrackOption::typed("/tracksort rd"),
            Some(TrackOption::Sort("rd".into()))
        );
        assert_eq!(
            TrackOption::typed("/TrackFilter  light blue "),
            Some(TrackOption::Filter("light blue".into()))
        );
        assert_eq!(
            TrackOption::typed("/trackplayers"),
            Some(TrackOption::Players(String::new()))
        );
        assert_eq!(TrackOption::typed("/tracking"), None);

        let mut app = answering(Some(RANGER));
        let options = |app: &App| {
            let state = app.world().resource::<crate::options::OptionsState>();
            (
                state.options.track_sort,
                state.options.track_filter,
                state.options.track_players,
                state.track_filter,
            )
        };
        answer(&mut app, TrackOption::Sort("rd".into()));
        assert_eq!(options(&app).0, TrackSort::ReverseDistance);
        // A word the sort doesn't take changes nothing.
        let said = answer(&mut app, TrackOption::Sort("x".into()));
        assert_eq!(said.as_deref(), Some("Use /tracksort N, D, C, RD or RC."));
        assert_eq!(options(&app).0, TrackSort::ReverseDistance);
        // The filter toggles for the session; the saved mask stays.
        answer(&mut app, TrackOption::Filter("Green".into()));
        answer(&mut app, TrackOption::Filter("light blue".into()));
        assert_eq!(options(&app).3, TrackFilter(0x21));
        answer(&mut app, TrackOption::Filter("green".into()));
        assert_eq!(options(&app).3, TrackFilter(0x20));
        assert_eq!(options(&app).1, TrackFilter(0));
        // A word for no colour says nothing.
        let before = answer(&mut app, TrackOption::Filter("purple".into()));
        assert_eq!(before.as_deref(), Some("Tracking now shows that colour."));
        answer(&mut app, TrackOption::Players("group".into()));
        assert_eq!(options(&app).2, TrackPlayers::Group);
        let said = answer(&mut app, TrackOption::Players("maybe".into()));
        assert_eq!(said.as_deref(), Some("Use /trackplayers on, off or group."));
        assert_eq!(options(&app).2, TrackPlayers::Group);

        // Another class: sort and filter get the Rangers-only line and
        // change nothing; the players choice is anyone's.
        let mut app = answering(Some(1));
        for option in [
            TrackOption::Sort("d".into()),
            TrackOption::Filter("green".into()),
        ] {
            assert_eq!(
                answer(&mut app, option).as_deref(),
                Some("Sorting and filtering tracking are for Rangers.")
            );
        }
        assert_eq!(options(&app).0, TrackSort::Normal);
        assert_eq!(options(&app).3, TrackFilter(0));
        answer(&mut app, TrackOption::Players("off".into()));
        assert_eq!(options(&app).2, TrackPlayers::Off);
    }

    #[test]
    fn the_window_follows_the_worlds_and_closing_it_closes_the_worlds() {
        let mut app = crate::testing::app();
        app.add_systems(Update, window);
        let mut online = admitted();
        testing::news(&mut online, [pressed()]);
        assert!(online.world().track_window().open);
        app.insert_resource(online);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Tracking));
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Tracking);
        app.update();
        let online = app.world().resource::<OnlineState>();
        assert!(!online.world().track_window().open);
        // Following an entry closes the window the world closed.
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        testing::news(&mut online, [pressed()]);
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Tracking));
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::Tracking(TrackingUpdate::Following {
                spawn_id: 30,
                string_id: 0x2f08,
                arguments: vec!["a_bat00".into()],
            })],
        );
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Tracking));
    }

    #[test]
    fn escape_and_the_close_box_close_the_window_and_following_goes_on() {
        // Both close here, as nothing toggles the track window (B2 run 1b:
        // Escape stopped at it and closed nothing, and its box was greyed).
        assert!(crate::skinned::closes_here(WindowId::Tracking));
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx))).add_systems(
            Update,
            (crate::escape::route, crate::skinned::close, window, frame).chain(),
        );
        let opened = |app: &mut App| {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            testing::news(&mut online, [pressed()]);
            app.update();
            app.update();
            assert!(
                app.world()
                    .resource::<Shown>()
                    .displayed(WindowId::Tracking)
            );
        };
        let following = |app: &App| {
            app.world()
                .resource::<OnlineState>()
                .world()
                .track_window()
                .following
        };
        let mut online = admitted();
        testing::news(
            &mut online,
            [WorldEvent::Tracking(TrackingUpdate::Following {
                spawn_id: 30,
                string_id: 0x2f08,
                arguments: vec!["a_bat00".into()],
            })],
        );
        app.insert_resource(online);
        opened(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Tracking));
        assert!(
            !app.world()
                .resource::<OnlineState>()
                .world()
                .track_window()
                .open
        );
        assert_eq!(following(&app), Some(30));
        opened(&mut app);
        let close_box = app
            .world_mut()
            .spawn((
                Button,
                crate::skinned::Closes(WindowId::Tracking),
                Interaction::None,
            ))
            .id();
        crate::clicks::testing::click(&mut app, close_box);
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Tracking));
        assert_eq!(following(&app), Some(30));
        // Neither sends anything.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn the_one_title_box_closes_and_an_opening_comes_to_the_front() {
        // The close box only closes: it is the one box drawn.
        assert!(!WindowId::Tracking.can_minimize());
        let mut app = crate::testing::app();
        app.add_systems(Update, frame);
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Tracking);
        app.update();
        assert_eq!(
            app.world()
                .resource::<windows::Stack>()
                .front_to_back()
                .next(),
            Some(WindowId::Tracking)
        );
    }

    #[test]
    fn cancel_stops_tracking_and_closes_the_window() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, buttons);
        let mut online = admitted();
        testing::news(&mut online, [pressed()]);
        app.insert_resource(online);
        let cancel = app
            .world_mut()
            .spawn((Button, StopButton, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, cancel);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Track {
                action: TrackAction::Stop,
                ..
            })
        ));
        assert!(
            !app.world()
                .resource::<OnlineState>()
                .world()
                .track_window()
                .open
        );
    }

    #[test]
    fn track_follows_the_chosen_row() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = crate::testing::app();
        app.insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, (fill, buttons).chain());
        let mut online = admitted();
        testing::news(&mut online, [listed(&[31, 30])]);
        app.insert_resource(online);
        let list = app.world_mut().spawn(TrackRows::new(vec![150.0])).id();
        app.update();
        let second = app.world().get::<Children>(list).unwrap()[1];
        app.world_mut()
            .entity_mut(second)
            .insert(Interaction::Pressed);
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .track_window()
                .selected,
            Some(30)
        );
        // The chosen row is drawn chosen, from the next frame.
        app.update();
        let chosen = app.world().get::<Children>(list).unwrap()[1];
        assert_eq!(
            app.world().get::<BackgroundColor>(chosen).unwrap().0,
            theme::BUTTON
        );
        let button = app
            .world_mut()
            .spawn((Button, TrackButton, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, button);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Track {
                action: TrackAction::Follow(30),
                ..
            })
        ));
    }
}

//! The track window, as the Titanium program keeps it (read R14 section 2,
//! R15a): the Tracking press opens it, the server's list fills it in its
//! own order, each name in the colour of its level against the player's,
//! a click chooses an entry, Track follows it and closes the window,
//! Cancel stops tracking and closes it as `/stoptracking` does, and the
//! close box only closes it. The skin's `TrackingWnd` draws it
//! where the skin has one; elsewhere the client's own panel shows the same
//! list and Track button. The Ranger's sort, colour filters and players
//! choice are not read closely enough to act on, so their controls are
//! drawn greyed (recording checklist 6.446); another class's `/tracksort`
//! or `/trackfilter` gets the program's Rangers-only line.
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

/// A Ranger's tracking option typed by another class, waiting for its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RangersOnly;

/// Whether a typed command is a Ranger's tracking option, `/tracksort` or
/// `/trackfilter`, from a player of another class. A Ranger's own is not
/// read closely enough to act on (6.446), and an unknown class is left to
/// the server.
pub(crate) fn rangers_only(command: &str, world: &ClientWorld) -> bool {
    let name = command
        .strip_prefix('/')
        .and_then(|command| command.split_whitespace().next())
        .unwrap_or_default();
    (name.eq_ignore_ascii_case("tracksort") || name.eq_ignore_ascii_case("trackfilter"))
        && world
            .player()
            .and_then(|player| player.class)
            .is_some_and(|class| class != RANGER)
}

/// Says the program's Rangers-only line for a tracking option another class
/// typed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn say_rangers_only(
    mut chat: ResMut<crate::chat::ChatState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
) {
    if chat.rangers_only.is_none() {
        return;
    }
    chat.rangers_only = None;
    // The client's own words where the installation has no string table.
    let fallback = "Sorting and filtering tracking are for Rangers.";
    let said = messages.as_deref().map_or_else(
        || crate::chat::Said::own(fallback),
        |messages| messages.said_or(RANGERS_ONLY, &[], fallback),
    );
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

/// The listed spawns the zone still knows, in the server's order.
fn rows(world: &ClientWorld) -> Vec<Row> {
    let window = world.track_window();
    let yours = world.player().map(|player| player.level);
    window
        .entries
        .iter()
        .filter_map(|entry| {
            let spawn = &world.spawn(entry.spawn_id)?.state;
            Some(Row {
                spawn_id: entry.spawn_id,
                name: eq_client_core::entities::display_name(&spawn.name),
                color: yours
                    .filter(|_| spawn.level != 0)
                    .map(|yours| track_color(yours, spawn.level)),
                chosen: window.selected == Some(entry.spawn_id),
            })
        })
        .collect()
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
    online: Res<OnlineState>,
    mut lists: Query<(Entity, &mut TrackRows, Option<&Children>)>,
) {
    let world = online.world();
    let input = Inputs {
        session: world.session_id(),
        revision: world.track_window().revision,
        level: world.player().map(|player| player.level),
    };
    if lists.iter().all(|(_, list, _)| list.checked == Some(input)) {
        return;
    }
    let wanted = rows(world);
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
        let rows = rows(online.world());
        // The unknown spawn is left out, as the program skips it.
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["a snake", "a bat"]);
        assert_eq!(rows[0].color, Some(ConColor::Red));
        assert_eq!(rows[1].color, Some(ConColor::Green));
        online.select_tracked(Some(30));
        assert!(super::rows(online.world())[1].chosen);
    }

    #[test]
    fn only_a_rangers_tracking_options_pass() {
        let mut online = admitted();
        let class = |online: &mut OnlineState, class| {
            let mut player = testing::player(7);
            player.class = class;
            testing::admit(online, 1, player);
        };
        class(&mut online, Some(1));
        assert!(rangers_only("/tracksort 1", online.world()));
        assert!(rangers_only("/TRACKFILTER", online.world()));
        assert!(!rangers_only("/trackplayers on", online.world()));
        class(&mut online, Some(RANGER));
        assert!(!rangers_only("/tracksort 1", online.world()));
        class(&mut online, None);
        assert!(!rangers_only("/tracksort", online.world()));
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

//! Map search and watch controls share normal keyboard, click and scrolling rules.
use super::{MapView, entities, watch::WatchState};
use crate::{
    keys::{Typing, search},
    online::OnlineState,
    theme,
    tooltip::Tooltip,
    windows::{self, Shown, WindowId},
};
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};

#[derive(Component)]
pub(crate) enum Control {
    ClearSearch,
    WatchQuery,
    WatchSelected,
    ToggleList,
    Remove(String),
}

#[derive(Component)]
pub(crate) enum Label {
    Search,
    Watches,
    Alert,
    SelectedWatch,
}

#[derive(Component)]
pub(crate) struct SearchBox;

#[derive(Component)]
pub(crate) struct WatchList;

/// Claim text input before chat sees the same frame's keys. Clicking elsewhere,
/// closing the map or losing desktop focus releases it.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn focus(
    mouse: Res<ButtonInput<MouseButton>>,
    shown: Res<Shown>,
    windows: Query<&Window, With<PrimaryWindow>>,
    boxes: Query<&Interaction, With<SearchBox>>,
    mut typing: ResMut<Typing>,
) {
    if !shown.displayed(WindowId::Map)
        || !windows.single().is_ok_and(|window| window.focused)
        || boxes.is_empty()
    {
        typing.map_search = false;
        return;
    }
    if mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]) {
        typing.map_search = boxes
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed);
        if typing.map_search {
            typing.composing = false;
            typing.counting = false;
        }
    }
}

/// Edits only the local search; Enter leaves it visible, Escape leaves typing.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    mut keyboard: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    mut view: ResMut<MapView>,
    keys: Res<ButtonInput<Key>>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut pending: Local<Option<ClipboardRead>>,
) {
    if !typing.map_search || typing.spell_search_claimed {
        keyboard.clear();
        *pending = None;
        return;
    }
    let events: Vec<_> = keyboard
        .read_with_id()
        .filter(|(_, id)| !typing.spell_search_captured(*id))
        .map(|(event, _)| event.clone())
        .collect();
    if let Some(finish) = search::edit(
        &mut view.query,
        &events,
        &keys,
        clipboard.as_deref_mut(),
        &mut pending,
    ) {
        typing.map_search = false;
        typing.escape_consumed |= finish == search::Finish::Escape;
    }
}

/// Search and watch buttons never use the game outbox.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn actions(
    clicks: crate::clicks::Clicks<Control>,
    mut view: ResMut<MapView>,
    online: Res<OnlineState>,
    mut watches: ResMut<WatchState>,
) {
    for action in clicks.iter() {
        match action {
            Control::ClearSearch => view.query.clear(),
            Control::WatchQuery => watches.add(&view.query),
            Control::WatchSelected => {
                if let Some(entity) = entities::selected(online.world(), &view) {
                    if watches.names.contains(entity.name) {
                        watches.remove(entity.name);
                    } else {
                        watches.add(entity.name);
                    }
                }
            }
            Control::ToggleList => watches.list_open = !watches.list_open,
            Control::Remove(name) => watches.remove(name),
        }
    }
}

/// Update compact labels and rebuild the watch list only when its names change.
#[allow(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::too_many_arguments
)]
pub(crate) fn update(
    mut commands: Commands,
    view: Res<MapView>,
    watches: Res<WatchState>,
    online: Res<OnlineState>,
    typing: Res<Typing>,
    mut labels: Query<(&Label, &mut Text, &mut Node)>,
    mut lists: Query<(Entity, &mut Node), (With<WatchList>, Without<Label>)>,
    mut drawn: Local<Option<(Entity, eq_client_core::maps::Watches)>>,
) {
    for (kind, mut text, mut node) in &mut labels {
        let wanted = match kind {
            Label::Search => {
                if view.query.is_empty() && !typing.map_search {
                    "Search names...".to_owned()
                } else {
                    format!("{}{}", view.query, if typing.map_search { "|" } else { "" })
                }
            }
            Label::Watches => format!("Tracked ({})", watches.names.names().count()),
            Label::Alert => watches.last_alert.clone(),
            Label::SelectedWatch => if entities::selected(online.world(), &view)
                .is_some_and(|entity| watches.names.contains(entity.name))
            {
                "Untrack"
            } else {
                "Track"
            }
            .to_owned(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
        if matches!(kind, Label::Alert) {
            let display = if text.0.is_empty() {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
    let Ok((list, mut node)) = lists.single_mut() else {
        return;
    };
    node.display = if watches.list_open {
        Display::Flex
    } else {
        Display::None
    };
    if drawn
        .as_ref()
        .is_some_and(|(old, names)| *old == list && names == &watches.names)
    {
        return;
    }
    commands
        .entity(list)
        .despawn_children()
        .with_children(|panel| {
            panel.spawn(theme::text(
                "Exact names · click to remove",
                theme::Size::Small,
                theme::INK,
            ));
            if watches.names.names().next().is_none() {
                panel.spawn(theme::text(
                    "Inspect an entity and choose Track,\nor type a full name and Track name.",
                    theme::Size::Small,
                    theme::INK,
                ));
            }
            for name in watches.names.names() {
                panel.spawn((
                    Control::Remove(name.to_owned()),
                    button(&format!("× {name}")),
                ));
            }
        });
    *drawn = Some((list, watches.names.clone()));
}

/// The watch list consumes wheel input instead of zooming the world camera.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn scroll(
    wheel: Res<windows::pointer::Wheel>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<WatchList>>,
) {
    if let Some(surface) = wheel.surface
        && let Ok((node, mut scroll)) = lists.get_mut(surface)
    {
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        scroll.0.y = (scroll.0.y - wheel.pixels).clamp(0.0, maximum);
    }
}

pub(super) fn button(label: &str) -> impl Bundle {
    (
        Button,
        theme::text(label, theme::Size::Small, theme::INK_BRIGHT),
        Node {
            padding: UiRect::axes(px(6), px(4)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme::BUTTON),
    )
}

pub(super) fn chrome(parent: &mut ChildSpawnerCommands) {
    super::presentation::chrome(parent);
    parent.spawn((Node { position_type: PositionType::Absolute, left: px(6), right: px(6), top: px(6),
        column_gap: px(4), padding: UiRect::all(px(4)), ..default() },
        windows::KeepsPress, BackgroundColor(theme::SCRIM), ZIndex(10))).with_children(|row| {
        row.spawn((SearchBox, Label::Search, button("Search names..."),
            Tooltip("Case-insensitive name search. Non-matches are dimmed. Enter finishes; Delete clears.".into())))
            .insert(Node { flex_grow: 1.0, min_width: px(40), padding: UiRect::all(px(4)), overflow: Overflow::clip(), ..default() });
        row.spawn((Control::ClearSearch, button("Clear")));
        row.spawn((Control::WatchQuery, button("Track name"), Tooltip("Track this exact displayed name and alert on appearances, even when currently absent.".into())));
        row.spawn((Control::ToggleList, Label::Watches, button("Tracked (0)")));
    });
    parent.spawn((
        WatchList,
        windows::KeepsPress,
        windows::pointer::TakesWheel,
        ScrollPosition::default(),
        Node {
            position_type: PositionType::Absolute,
            right: px(6),
            top: px(74),
            width: px(260),
            max_height: percent(50),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::all(px(6)),
            overflow: Overflow::scroll_y(),
            display: Display::None,
            ..default()
        },
        BackgroundColor(theme::SCRIM),
        ZIndex(15),
    ));
    parent.spawn((
        Label::Alert,
        BackgroundColor(theme::SCRIM),
        theme::text("", theme::Size::Small, theme::INK_WARM),
        Node {
            position_type: PositionType::Absolute,
            left: px(6),
            right: px(6),
            top: px(122),
            padding: UiRect::all(px(3)),
            ..default()
        },
        ZIndex(10),
        bevy::ui::FocusPolicy::Pass,
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                top: px(107),
                column_gap: px(6),
                ..default()
            },
            BackgroundColor(theme::SCRIM),
            ZIndex(10),
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|row| {
            row.spawn(theme::text("PC", theme::Size::Small, entities::PLAYER));
            for (name, con) in [
                ("NPC: green", eq_client_core::combat::ConColor::Green),
                ("blue", eq_client_core::combat::ConColor::Blue),
                ("white", eq_client_core::combat::ConColor::White),
                ("yellow", eq_client_core::combat::ConColor::Yellow),
                ("red", eq_client_core::combat::ConColor::Red),
            ] {
                row.spawn(theme::text(name, theme::Size::Small, theme::con(Some(con))));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_frame_paste_shortcut_never_inserts_a_literal_v() {
        use bevy::input::ButtonState;
        let mut app = crate::testing::app();
        app.add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<Key>>()
            .add_systems(Update, input);
        app.world_mut().resource_mut::<Typing>().map_search = true;
        for (logical_key, state, text) in [
            (Key::Control, ButtonState::Pressed, None),
            (Key::Character("v".into()), ButtonState::Pressed, Some("v")),
            (Key::Control, ButtonState::Released, None),
        ] {
            app.world_mut().write_message(KeyboardInput {
                logical_key,
                state,
                text: text.map(Into::into),
                key_code: KeyCode::KeyV,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
        assert_eq!(app.world().resource::<MapView>().query, "");
        let mut query = "x".repeat(126);
        search::append(&mut query, "\n\ré雪");
        assert_eq!(query.len(), 128);
        assert!(query.ends_with('é'));
    }

    #[test]
    fn search_owns_text_and_escape_without_starting_chat_or_game_actions() {
        let mut app = crate::testing::app();
        app.add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<Key>>()
            .add_systems(Update, (crate::chat::input, input).chain());
        app.world_mut().resource_mut::<Typing>().map_search = true;
        let event = |logical_key, text: Option<&str>| KeyboardInput {
            key_code: KeyCode::KeyG,
            logical_key,
            text: text.map(Into::into),
            state: bevy::input::ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .write_message(event(Key::Character("guard".into()), Some("guard")));
        app.update();
        assert_eq!(app.world().resource::<MapView>().query, "guard");
        assert!(!app.world().resource::<Typing>().composing);
        assert_eq!(app.world().resource::<crate::chat::ChatState>().draft(), "");
        app.world_mut().write_message(event(Key::Escape, None));
        app.update();
        let typing = app.world().resource::<Typing>();
        assert!(!typing.map_search && typing.escape_consumed);
        assert_eq!(app.world().resource::<MapView>().query, "guard");
    }

    #[test]
    fn watch_buttons_add_exact_absent_names_and_remove_them() {
        let mut app = crate::testing::app();
        app.add_systems(Update, actions);
        app.world_mut().resource_mut::<MapView>().query = "a rare guard".into();
        let add = app
            .world_mut()
            .spawn((Control::WatchQuery, Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, add);
        assert!(
            app.world()
                .resource::<WatchState>()
                .names
                .contains("a_rare_guard001")
        );
        let remove = app
            .world_mut()
            .spawn((Control::Remove("a rare guard".into()), Interaction::None))
            .id();
        crate::clicks::testing::click(&mut app, remove);
        assert!(
            !app.world()
                .resource::<WatchState>()
                .names
                .contains("a_rare_guard002")
        );
    }
}

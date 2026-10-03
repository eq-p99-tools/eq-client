//! Pre-zone character selection uses occupied server slots, never typed
//! names: in the skin's character list where the installation has one, or
//! else in the client's own list.
use super::{online::OnlineState, outbox::Outbox, windows::WindowId};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{CharacterChoice, ClientCommand};

pub(super) struct Selection {
    id: u64,
    entries: Vec<CharacterChoice>,
    selected: Option<u8>,
    submitted: bool,
    message: String,
}

impl Selection {
    pub fn new(id: u64, entries: Vec<CharacterChoice>) -> Self {
        Self {
            id,
            entries,
            selected: None,
            submitted: false,
            message: String::new(),
        }
    }

    /// Identity the server list was published with.
    pub(super) const fn id(&self) -> u64 {
        self.id
    }

    /// The slot of the character chosen, if any.
    pub(crate) const fn chosen(&self) -> Option<u8> {
        self.selected
    }

    /// Highlights a listed character by exact server spelling, ignoring case.
    pub(super) fn choose_named(&mut self, name: &str) -> bool {
        let slot = self
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.slot);
        if slot.is_some() && !self.submitted {
            self.selected = slot;
        }
        slot.is_some()
    }

    /// Asks once; a refused request leaves the choice available for another
    /// click, and the outbox says why.
    fn enter(&mut self, outbox: &Outbox, world: &eq_client_core::world::ClientWorld) {
        if self.submitted {
            return;
        }
        let Some(slot) = self.selected else {
            return;
        };
        if !self.entries.iter().any(|entry| entry.slot == slot) {
            return;
        }
        let command = ClientCommand::SelectCharacter {
            selection_id: self.id,
            slot,
        };
        match outbox.send(world, command) {
            Ok(()) => {
                self.submitted = true;
                self.message = "Entering world...".into();
            }
            Err(refusal) => self.message = refusal.text().into(),
        }
    }
}

#[derive(Component)]
pub(super) struct Root;
#[derive(Component)]
pub(super) enum Action {
    Choose(u8),
    Enter,
    /// Leaves the game, as the skin's Quit does.
    Quit,
}

/// The words on a skin's character button: the name of the character in its
/// slot, in the skin's colour, or the skin's own words for an empty slot.
#[derive(Component)]
pub(crate) struct SlotName {
    pub(crate) slot: u8,
    pub(crate) empty: String,
    pub(crate) ink: Color,
}

/// Applies selection input, shows the skin's list while the character list
/// is up, and rebuilds the cover and the client's own panel only when their
/// state changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    mut online: ResMut<OnlineState>,
    lines: Res<crate::notices::Lines>,
    outbox: Res<Outbox>,
    keys: crate::keys::Keys,
    navigation: Res<super::navigation::NavigationKeys>,
    buttons: Query<(Ref<Interaction>, &Action)>,
    roots: Query<Entity, With<Root>>,
    mut previous: Local<String>,
    (mut shown, skinned, mut exit): (
        ResMut<super::windows::Shown>,
        Res<crate::skinned::Skinned>,
        MessageWriter<AppExit>,
    ),
) {
    let focused = keys.focused();
    let keys = navigation.sample(&keys.input);
    // A session, or the preview's characters, until a character is in.
    let visible =
        (online.enabled || online.selection.is_some()) && online.world().session_id().is_none();
    if visible != shown.is_open(WindowId::CharacterSelect) {
        if visible {
            shown.open(WindowId::CharacterSelect);
        } else {
            shown.close(WindowId::CharacterSelect);
        }
    }
    let pressed = |action: fn(&Action) -> bool| {
        buttons.iter().any(|(interaction, button)| {
            interaction.is_changed() && *interaction == Interaction::Pressed && action(button)
        })
    };
    // Quit leaves the game from the list, whatever it shows.
    if visible && focused && pressed(|action| matches!(action, Action::Quit)) {
        exit.write(AppExit::Success);
    }
    let (choosing, world) = online.choosing();
    if visible
        && focused
        && let Some(selection) = choosing
        && !selection.submitted
    {
        for (interaction, action) in &buttons {
            if !interaction.is_changed() || *interaction != Interaction::Pressed {
                continue;
            }
            match action {
                // An empty slot of the skin's list chooses nothing.
                Action::Choose(slot) => {
                    if selection.entries.iter().any(|entry| entry.slot == *slot) {
                        selection.selected = Some(*slot);
                    }
                }
                Action::Enter => selection.enter(&outbox, world),
                Action::Quit => (),
            }
        }
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::ArrowUp) {
            let index = selection
                .entries
                .iter()
                .position(|entry| Some(entry.slot) == selection.selected);
            let count = selection.entries.len();
            if count != 0 {
                let next = index.map_or(0, |index| {
                    if keys.just_pressed(KeyCode::ArrowUp) {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    }
                });
                selection.selected = Some(selection.entries[next].slot);
            }
        }
        if keys.just_pressed(KeyCode::Enter) {
            selection.enter(&outbox, world);
        }
    }
    let status = lines.status.text();
    let skinned = skinned.has(WindowId::CharacterSelect);
    let signature = format!(
        "{visible}:{skinned}:{status}:{:?}",
        online.selection.as_ref().map(|s| (
            s.id,
            s.selected,
            s.submitted,
            &s.message,
            s.entries.len()
        ))
    );
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    if visible {
        spawn(&mut commands, online.selection.as_ref(), status, skinned);
    }
}

/// Writes each character's name on the skin's character buttons, and the
/// skin's own words, dimmed, on an empty slot's, as this client creates no
/// characters yet; hovering a character says its level, where the server
/// gives it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn names(
    online: Res<OnlineState>,
    mut words: Query<(&SlotName, &mut Text, &mut TextColor)>,
    mut tips: Query<(&Action, &mut crate::tooltip::Tooltip)>,
) {
    let entries = online
        .selection
        .as_ref()
        .map_or(&[][..], |selection| &selection.entries[..]);
    let entry = |slot: u8| entries.iter().find(|entry| entry.slot == slot);
    for (name, mut text, mut color) in &mut words {
        let (wanted, ink) = entry(name.slot)
            .map_or((name.empty.as_str(), theme::INK_DIM), |entry| {
                (entry.name.as_str(), name.ink)
            });
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
        if color.0 != ink {
            color.0 = ink;
        }
    }
    for (action, mut tip) in &mut tips {
        let Action::Choose(slot) = action else {
            continue;
        };
        let wanted = entry(*slot)
            .and_then(|entry| entry.level)
            .map_or_else(String::new, |level| format!("Level {level}"));
        if tip.0 != wanted {
            tip.0 = wanted;
        }
    }
}

/// What the list says under it: the session's state, its messages, or how
/// to use it.
fn guidance<'a>(selection: Option<&'a Selection>, status: &'a str) -> &'a str {
    match selection {
        None => status,
        Some(selection) if selection.entries.is_empty() => {
            "No characters on this server. Create one with the official client first."
        }
        Some(selection) if !selection.message.is_empty() => &selection.message,
        Some(_) => "Select a character | Up/Down: browse | Enter: connect",
    }
}

/// Covers the zone preview until a character has completed zone admission:
/// under the skin's list, with the list's guidance at its foot, or with the
/// client's own panel on it.
fn spawn(commands: &mut Commands, selection: Option<&Selection>, status: &str, skinned: bool) {
    if skinned {
        commands
            .spawn((
                Root,
                Button,
                // Under the skin's list, which is drawn at the screen's layer.
                GlobalZIndex(super::windows::Layer::Screen.base() - 1),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::bottom(px(40)),
                    ..default()
                },
                BackgroundColor(theme::COVER),
            ))
            .with_children(|root| {
                theme::label(root, guidance(selection, status), Size::Label);
            });
        return;
    }
    commands
        .spawn((
            Root,
            Button,
            GlobalZIndex(super::windows::Layer::Screen.base() - 1),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme::COVER),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(440),
                    max_width: percent(95),
                    padding: UiRect::all(px(24)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(theme::TITLE_BAR),
            ))
            .with_children(|panel| {
                theme::label(panel, "CHARACTER SELECT", Size::Display);
                let Some(selection) = selection.filter(|selection| !selection.entries.is_empty())
                else {
                    theme::label(panel, guidance(selection, status), Size::Large);
                    return;
                };
                for entry in &selection.entries {
                    let detail = entry
                        .level
                        .map_or_else(String::new, |level| format!("  |  Level {level}"));
                    button(
                        panel,
                        Action::Choose(entry.slot),
                        &format!("{}{detail}", entry.name),
                        selection.selected == Some(entry.slot),
                    );
                }
                if selection.selected.is_some() && !selection.submitted {
                    button(panel, Action::Enter, "Enter World", true);
                }
                theme::label(panel, guidance(Some(selection), status), Size::Label);
            });
        });
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, text: &str, selected: bool) {
    theme::button_with(parent, action, text, Size::Large).insert((
        Node {
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(theme::button(true, selected, Interaction::None)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::PrimaryWindow;
    #[test]
    fn keyboard_selection_queues_the_server_slot_and_disappears_after_admission() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut state = OnlineState::new(true);
        state.selection = Some(Selection::new(
            7,
            vec![CharacterChoice {
                slot: 3,
                name: "Example".into(),
                level: Some(1),
                zone_id: None,
            }],
        ));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(state)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .init_resource::<crate::notices::Lines>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::navigation::NavigationKeys>()
            .init_resource::<crate::windows::Shown>()
            .init_resource::<crate::skinned::Skinned>()
            .add_message::<AppExit>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectCharacter {
                selection_id: 7,
                slot: 3
            }
        );
        app.update();
        assert!(rx.try_recv().is_err());
        crate::online::testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            8,
            crate::online::testing::player(1),
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<Root>>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn entry_requires_selection_and_never_duplicates_or_discards_a_full_queue() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let queue = tx.clone();
        let sender = crate::outbox::Outbox::new(Some(tx));
        let world = eq_client_core::world::ClientWorld::default();
        let mut selection = Selection::new(
            7,
            vec![CharacterChoice {
                slot: 3,
                name: "Example".into(),
                level: Some(1),
                zone_id: Some(22),
            }],
        );
        selection.enter(&sender, &world);
        assert!(rx.try_recv().is_err());
        selection.selected = Some(3);
        queue
            .try_send(ClientCommand::SelectCharacter {
                selection_id: 6,
                slot: 1,
            })
            .unwrap();
        selection.enter(&sender, &world);
        assert!(!selection.submitted);
        rx.try_recv().unwrap();
        selection.enter(&sender, &world);
        selection.enter(&sender, &world);
        assert!(selection.submitted);
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectCharacter {
                selection_id: 7,
                slot: 3
            }
        );
        assert!(rx.try_recv().is_err());
    }
}

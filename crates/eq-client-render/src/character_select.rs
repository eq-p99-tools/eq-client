//! Pre-zone character selection uses occupied server slots, never typed names.
use super::{hud::HudState, online::OnlineState, outbox::Outbox};
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
}

/// Applies selection input and rebuilds the small panel only when its state changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    mut online: ResMut<OnlineState>,
    hud: Res<HudState>,
    outbox: Res<Outbox>,
    keys: crate::keys::Keys,
    navigation: Res<super::navigation::NavigationKeys>,
    buttons: Query<(Ref<Interaction>, &Action)>,
    roots: Query<Entity, With<Root>>,
    mut previous: Local<String>,
) {
    let focused = keys.focused();
    let keys = navigation.sample(&keys.input);
    // A session, or the preview's characters, until a character is in.
    let visible =
        (online.enabled || online.selection.is_some()) && online.world().session_id().is_none();
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
                Action::Choose(slot) => selection.selected = Some(*slot),
                Action::Enter => selection.enter(&outbox, world),
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
    let signature = format!(
        "{visible}:{}:{:?}",
        hud.status,
        online
            .selection
            .as_ref()
            .map(|s| (s.id, s.selected, s.submitted, &s.message))
    );
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    if visible {
        spawn(&mut commands, online.selection.as_ref(), &hud.status);
    }
}

/// Covers the zone preview until a character has completed zone admission.
fn spawn(commands: &mut Commands, selection: Option<&Selection>, status: &str) {
    commands
        .spawn((
            Root,
            Button,
            GlobalZIndex(500),
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
                let Some(selection) = selection else {
                    theme::label(panel, status, Size::Large);
                    return;
                };
                if selection.entries.is_empty() {
                    theme::label(
                        panel,
                        "No characters on this server. Create one with the official client first.",
                        Size::Large,
                    );
                    return;
                }
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
                theme::label(
                    panel,
                    if selection.message.is_empty() {
                        "Select a character | Up/Down: browse | Enter: connect"
                    } else {
                        &selection.message
                    },
                    Size::Label,
                );
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
            .init_resource::<HudState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::navigation::NavigationKeys>()
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

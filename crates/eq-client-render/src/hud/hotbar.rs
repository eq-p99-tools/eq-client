//! Session-local action bindings; activating a binding uses the normal command validators.
use bevy::prelude::*;
mod item_art;
#[cfg(test)]
mod item_tests;
pub(crate) use item_art::update as item_artwork;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Gem(u8),
    Item {
        slot: eq_client_core::inventory::InventorySlot,
        id: u32,
    },
    Sit,
    Stand,
}

#[derive(Resource)]
pub(crate) struct Bindings(pub(super) [Option<Action>; 10]);

impl Default for Bindings {
    fn default() -> Self {
        Self(std::array::from_fn(|index| {
            Some(match index {
                8 => Action::Sit,
                9 => Action::Stand,
                _ => Action::Gem(u8::try_from(index).expect("ten slots")),
            })
        }))
    }
}

impl Bindings {
    /// Resolves the current gem binding without retaining a previously memorized spell.
    pub(crate) fn gem(&self, slot: usize) -> Option<usize> {
        match self.0.get(slot).copied().flatten()? {
            Action::Gem(gem) => Some(usize::from(gem)),
            Action::Sit | Action::Stand | Action::Item { .. } => None,
        }
    }
}

#[derive(Component)]
pub(crate) struct Slot(pub usize);
#[derive(Component)]
pub(crate) struct Caption(pub usize);
#[derive(Component)]
pub(crate) struct Hint;

/// Builds ten bound action buttons in the requested five-by-two layout.
pub(super) fn spawn(commands: &mut Commands, root: Entity) {
    let frame = super::panel(commands, root, 244.0);
    crate::windows::titled(commands, frame, "ACTIONS");
    for (row_index, keys) in [["1", "2", "3", "4", "5"], ["6", "7", "8", "9", "0"]]
        .into_iter()
        .enumerate()
    {
        let row = super::row(commands, frame, 4.0);
        for (column, key) in keys.into_iter().enumerate() {
            let index = row_index * 5 + column;
            let button = super::slot(commands, row, key, 40.0, true);
            commands
                .entity(button)
                .insert((
                    Button,
                    Slot(index),
                    crate::outbox::Needs(eq_client_core::Capability::Casting),
                ))
                .with_child(crate::spell_icons::artwork(
                    crate::spell_icons::Source::Action(index),
                    30.0,
                ))
                .with_child(item_art::artwork(index));
            let caption = super::label(commands, button, "", 8.0, super::INK);
            commands.entity(caption).insert((
                Caption(index),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(1),
                    right: px(2),
                    ..default()
                },
            ));
        }
    }
    let hint = super::label(commands, frame, "", 9.0, super::INK);
    commands.entity(hint).insert((
        Hint,
        Node {
            height: px(52),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

fn digit(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ]
    .iter()
    .position(|key| keys.just_pressed(*key))
}

/// Resolves unmodified number keys or a button press into a typed action.
pub(super) fn requested(
    keys: &ButtonInput<KeyCode>,
    bindings: &Bindings,
    clicks: &Query<(&Interaction, &Slot), Changed<Interaction>>,
) -> Option<Action> {
    if keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
    ]) {
        return None;
    }
    let slot = clicks
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, slot)| slot.0)
        .or_else(|| digit(keys))?;
    bindings.0.get(slot).copied().flatten()
}

/// Ctrl+number binds the hovered gem or item; Ctrl+Shift+number clears the slot.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<crate::chat::ChatState>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    gems: Query<(&Interaction, &super::SpellGem)>,
    items: Query<(&Interaction, &crate::inventory::SlotButton)>,
    online: Option<Res<crate::online::OnlineState>>,
    mut bindings: ResMut<Bindings>,
) {
    if !chat.composing
        && windows.single().is_ok_and(|window| window.focused)
        && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && let Some(index) = digit(&keys)
    {
        if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
            bindings.0[index] = None;
        } else if let Some((_, gem)) = gems
            .iter()
            .find(|(interaction, _)| **interaction != Interaction::None)
        {
            bindings.0[index] = Some(Action::Gem(gem.0));
        } else if let Some(online) = online
            && let Some((_, slot)) = items
                .iter()
                .find(|(interaction, _)| **interaction != Interaction::None)
            && let Some(item) = online.world.inventory().items().get(&slot.0)
            && item.activation.effect.is_some()
        {
            bindings.0[index] = Some(Action::Item {
                slot: slot.0,
                id: item.details.id,
            });
        }
    }
}

/// Presents current bindings, spell identity and cooldowns independently of keyboard focus.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn presentation(
    bindings: Res<Bindings>,
    online: Res<crate::online::OnlineState>,
    names: Res<crate::spellbook::SpellNames>,
    mut slots: Query<(&Interaction, &Slot, &mut BackgroundColor)>,
    mut labels: Query<(&mut Text, Option<&Caption>, Option<&Hint>)>,
) {
    let now = std::time::Instant::now();
    let inventory = online.world.inventory();
    let hovered = slots
        .iter()
        .find(|(interaction, ..)| **interaction != Interaction::None)
        .map(|(_, slot, ..)| slot.0);
    for (interaction, slot, mut color) in &mut slots {
        let spell = bindings.gem(slot.0).and_then(|gem| {
            online
                .world
                .player()
                .map_or([None; 8], |player| player.memorized_spells)
                .get(gem)
                .copied()
                .flatten()
        });
        let missing_item = match bindings.0[slot.0] {
            Some(Action::Item { slot, id }) => bound_item(inventory, slot, id).is_none(),
            _ => false,
        };
        let empty = missing_item
            || bindings.0[slot.0].is_none()
            || (bindings.gem(slot.0).is_some() && spell.is_none());
        let waiting = spell.is_some_and(|id| {
            online.world.casting().pending.is_some()
                || online.world.casting().cast.is_some()
                || !online
                    .world
                    .casting()
                    .cooldowns
                    .remaining(id, now)
                    .is_zero()
        });
        color.0 = if *interaction != Interaction::None {
            Color::srgb(0.18, 0.25, 0.32)
        } else if empty {
            Color::srgb(0.04, 0.05, 0.06)
        } else if waiting {
            Color::srgb(0.20, 0.14, 0.08)
        } else {
            Color::srgb(0.09, 0.16, 0.22)
        };
    }
    for (mut text, caption, hint) in &mut labels {
        if let Some(caption) = caption {
            text.0 = match bindings.0[caption.0] {
                Some(Action::Gem(gem)) => {
                    if online
                        .world
                        .player()
                        .map_or([None; 8], |player| player.memorized_spells)
                        .get(usize::from(gem))
                        .copied()
                        .flatten()
                        .is_some()
                    {
                        format!("G{}", gem + 1)
                    } else {
                        "—".into()
                    }
                }
                Some(Action::Sit) => "Sit".into(),
                Some(Action::Stand) => "Stand".into(),
                Some(Action::Item { .. }) => "Item".into(),
                None => "—".into(),
            };
        }
        if hint.is_some() {
            text.0 = match hovered.and_then(|index| bindings.0[index]) {
                Some(Action::Gem(gem)) => match online.world.player().map_or([None; 8], |player| player.memorized_spells).get(usize::from(gem)).copied().flatten() {
                    Some(spell) => {
                        let status = if online.world.casting().pending.is_some() {
                            "Awaiting cast acknowledgement".into()
                        } else if online.world.casting().cast.is_some() {
                            "Casting".into()
                        } else {
                            let remaining = online.world.casting().cooldowns.remaining(spell, now);
                            if remaining.is_zero() { "Uses current target".into() }
                            else { format!("Available in {:.1}s", remaining.as_secs_f32()) }
                        };
                        format!("{}\n{}\n{status}", names.label(spell), names.details(spell))
                    }
                    None => format!("Gem {} is empty\nOpen spellbook [B] to memorize", gem + 1),
                },
                Some(Action::Sit) => "Sit down".into(),
                Some(Action::Stand) => "Stand up".into(),
                Some(Action::Item { slot, id }) => bound_item(inventory, slot, id)
                    .map_or_else(|| format!("Bound item unavailable\n{} / rebind after moving it", slot.label()),
                        |item| format!("{}\n{} / uses current target or self", item.details.name, slot.label())),
                None if hovered.is_some() => "Unassigned\nHover gem or item + Ctrl+number: bind".into(),
                None => "Hover action for details\nHover gem or item + Ctrl+number: bind\nCtrl+Shift+number: clear".into(),
            };
        }
    }
}

/// A slot binding never silently activates a different item placed into that slot.
fn bound_item(
    inventory: &eq_client_core::inventory::Inventory,
    slot: eq_client_core::inventory::InventorySlot,
    id: u32,
) -> Option<&eq_client_core::inventory::InventoryItem> {
    inventory
        .items()
        .get(&slot)
        .filter(|item| item.details.id == id && item.activation.effect.is_some())
}

/// Keeps what each slot needs of the session in step with its binding:
/// sitting and standing are moves, a gem or an item's effect is a cast.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn needs(bindings: Res<Bindings>, mut slots: Query<(&Slot, &mut crate::outbox::Needs)>) {
    for (slot, mut needs) in &mut slots {
        let wanted = match bindings.0[slot.0] {
            Some(Action::Sit | Action::Stand) => eq_client_core::Capability::Moving,
            _ => eq_client_core::Capability::Casting,
        };
        if needs.0 != wanted {
            needs.0 = wanted;
        }
    }
}

/// Item shortcuts share inventory validation, request IDs, cursor rules and worker feedback.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn item_actions(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    clicks: Query<(&Interaction, &Slot), Changed<Interaction>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    chat: Res<crate::chat::ChatState>,
    online: Res<crate::online::OnlineState>,
    sender: Res<crate::outbox::Outbox>,
    mut hud: ResMut<super::HudState>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
) {
    if chat.composing || !windows.single().is_ok_and(|window| window.focused) {
        return;
    }
    let Some(Action::Item { slot, id }) = requested(&keys, &bindings, &clicks) else {
        return;
    };
    let message = if bound_item(online.world.inventory(), slot, id).is_none() {
        "Bound item unavailable; rebind after moving it".to_owned()
    } else {
        inventory.activate_shortcut(
            slot,
            &online,
            &sender,
            online.world.target().selected,
            online.world.casting().cast.is_some() || online.world.casting().pending.is_some(),
        )
    };
    hud.action_feedback = Some((std::time::Instant::now(), message));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_follows_current_spell_and_distinguishes_empty_from_unassigned() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        let mut app = App::new();
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        app.init_resource::<Bindings>()
            .insert_resource(online)
            .insert_resource(crate::spellbook::SpellNames::parse(&fields.join("^")))
            .add_systems(Update, presentation);
        app.world_mut()
            .spawn((Slot(0), Interaction::Hovered, BackgroundColor::default()));
        let hint = app.world_mut().spawn((Hint, Text::default())).id();
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Gem 1 is empty")
        );
        crate::online::testing::spell(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 73,
                mode: 1,
            },
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Synthetic spell")
        );
        crate::online::testing::pending_cast(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            Some(73),
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Awaiting cast acknowledgement")
        );
        app.world_mut().resource_mut::<Bindings>().0[0] = Some(Action::Gem(1));
        app.update();
        assert_eq!(app.world().resource::<Bindings>().gem(0), Some(1));
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Gem 2 is empty")
        );
        app.world_mut().resource_mut::<Bindings>().0[0] = None;
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .starts_with("Unassigned")
        );
    }
    #[test]
    fn binding_and_clearing_requires_focused_non_chat_input() {
        let mut app = App::new();
        app.init_resource::<Bindings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, update);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .spawn((Interaction::Hovered, super::super::SpellGem(4)));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::Digit1);
        }
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .composing = true;
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .composing = false;
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_eq!(app.world().resource::<Bindings>().0[0], None);
    }
}

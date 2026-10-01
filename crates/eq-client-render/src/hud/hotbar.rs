//! Session-local action bindings; activating a binding uses the normal command validators.
use crate::theme::{self, Size};
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
    Ability(eq_client_core::abilities::Ability),
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
            Action::Sit | Action::Stand | Action::Item { .. } | Action::Ability(_) => None,
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
    crate::windows::titled(commands, frame, crate::windows::WindowId::Actions);
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
            let caption = super::label(commands, button, "", Size::Caption, theme::INK);
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
    let hint = super::label(commands, frame, "", Size::Caption, theme::INK);
    commands.entity(hint).insert((
        Hint,
        Node {
            height: px(52),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

/// The first of the ten slots whose action the keys freshly pressed.
fn slot_pressed(keys: &crate::keys::Keys, act: fn(u8) -> crate::keys::Act) -> Option<usize> {
    (0..10u8)
        .find(|slot| keys.pressed(act(*slot)))
        .map(usize::from)
}

/// Resolves a slot's key or a button press into a typed action.
pub(super) fn requested(
    keys: &crate::keys::Keys,
    bindings: &Bindings,
    clicks: &Query<(&Interaction, &Slot), Changed<Interaction>>,
) -> Option<Action> {
    let slot = clicks
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, slot)| slot.0)
        .or_else(|| slot_pressed(keys, crate::keys::Act::Slot))?;
    bindings.0.get(slot).copied().flatten()
}

/// The spell gems and the Actions window's ability buttons, which Ctrl and a
/// number bind while under the pointer.
type Bindable<'w, 's> = (
    Query<'w, 's, (&'static Interaction, &'static super::SpellGem)>,
    Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static crate::abilities::AbilityButton,
        ),
    >,
);

/// Ctrl+number binds the hovered gem, ability or item; Ctrl+Shift+number
/// clears the slot.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    keys: crate::keys::Keys,
    (gems, abilities): Bindable,
    items: Query<(&Interaction, &crate::inventory::SlotButton)>,
    online: Option<Res<crate::online::OnlineState>>,
    mut bindings: ResMut<Bindings>,
) {
    if let Some(index) = slot_pressed(&keys, crate::keys::Act::ClearSlot) {
        bindings.0[index] = None;
    } else if let Some(index) = slot_pressed(&keys, crate::keys::Act::BindSlot) {
        let ability = online.as_ref().and_then(|online| {
            abilities
                .iter()
                .find(|(interaction, _)| **interaction != Interaction::None)
                .and_then(|(_, button)| crate::abilities::assigned(online.world(), *button))
        });
        if let Some((_, gem)) = gems
            .iter()
            .find(|(interaction, _)| **interaction != Interaction::None)
        {
            bindings.0[index] = Some(Action::Gem(gem.0));
        } else if let Some(ability) = ability {
            bindings.0[index] = Some(Action::Ability(ability));
        } else if let Some(online) = online
            && let Some((_, slot)) = items
                .iter()
                .find(|(interaction, _)| **interaction != Interaction::None)
            && let Some(item) = online.world().inventory().items().get(&slot.0)
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
    map: Res<crate::keys::KeyMap>,
    mut slots: Query<(&Interaction, &Slot, &mut BackgroundColor)>,
    mut labels: Query<(&mut Text, Option<&Caption>, Option<&Hint>)>,
) {
    let now = std::time::Instant::now();
    let inventory = online.world().inventory();
    let hovered = slots
        .iter()
        .find(|(interaction, ..)| **interaction != Interaction::None)
        .map(|(_, slot, ..)| slot.0);
    for (interaction, slot, mut color) in &mut slots {
        let spell = bindings.gem(slot.0).and_then(|gem| online.world().gem(gem));
        let missing_item = match bindings.0[slot.0] {
            Some(Action::Item { slot, id }) => bound_item(inventory, slot, id).is_none(),
            _ => false,
        };
        let empty = missing_item
            || bindings.0[slot.0].is_none()
            || (bindings.gem(slot.0).is_some() && spell.is_none());
        let ability_waits = match bindings.0[slot.0] {
            Some(Action::Ability(ability)) => online.world().ability_wait(ability, now).is_some(),
            _ => false,
        };
        let waiting = ability_waits
            || spell.is_some_and(|id| {
                online.world().casting().pending.is_some()
                    || online.world().casting().cast.is_some()
                    || !online
                        .world()
                        .casting()
                        .cooldowns
                        .remaining(id, now)
                        .is_zero()
            });
        color.0 = theme::readiness(*interaction != Interaction::None, empty, waiting);
    }
    for (mut text, caption, hint) in &mut labels {
        if let Some(caption) = caption {
            text.0 = match bindings.0[caption.0] {
                Some(Action::Gem(gem)) => {
                    if online.world().gem(usize::from(gem)).is_some() {
                        format!("G{}", gem + 1)
                    } else {
                        "-".into()
                    }
                }
                Some(Action::Sit) => "Sit".into(),
                Some(Action::Stand) => "Stand".into(),
                Some(Action::Item { .. }) => "Item".into(),
                Some(Action::Ability(ability)) => ability.name().into(),
                None => "-".into(),
            };
        }
        if hint.is_some() {
            text.0 = hovered_detail(&bindings, hovered, online.world(), (&names, &map), now);
        }
    }
}

/// What the hovered slot holds, for the window's detail line.
fn hovered_detail(
    bindings: &Bindings,
    hovered: Option<usize>,
    world: &eq_client_core::world::ClientWorld,
    (names, map): (&crate::spellbook::SpellNames, &crate::keys::KeyMap),
    now: std::time::Instant,
) -> String {
    let inventory = world.inventory();
    match hovered.and_then(|index| bindings.0[index]) {
        Some(Action::Gem(gem)) => match world.gem(usize::from(gem)) {
            Some(spell) => {
                let status = if world.casting().pending.is_some() {
                    "Awaiting cast acknowledgement".into()
                } else if world.casting().cast.is_some() {
                    "Casting".into()
                } else {
                    let remaining = world.casting().cooldowns.remaining(spell, now);
                    if remaining.is_zero() {
                        "Uses current target".into()
                    } else {
                        format!("Available in {:.1}s", remaining.as_secs_f32())
                    }
                };
                format!("{}\n{}\n{status}", names.label(spell), names.details(spell))
            }
            None => super::empty_gem(gem, map),
        },
        Some(Action::Sit) => "Sit down".into(),
        Some(Action::Stand) => "Stand up".into(),
        Some(Action::Ability(ability)) => match world.ability_wait(ability, now) {
            Some(wait) => format!(
                "{}\nAvailable in {:.0}s",
                ability.name(),
                wait.as_secs_f32().ceil()
            ),
            None => format!("{}\nReady", ability.name()),
        },
        Some(Action::Item { slot, id }) => bound_item(inventory, slot, id).map_or_else(
            || {
                format!(
                    "Bound item unavailable\n{}; rebind after moving it",
                    slot.label()
                )
            },
            |item| {
                format!(
                    "{}\n{}; uses the current target or yourself",
                    item.details.name,
                    slot.label()
                )
            },
        ),
        None if hovered.is_some() => "Unassigned".into(),
        None => String::new(),
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
            Some(Action::Ability(_)) => eq_client_core::Capability::Abilities,
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
    keys: crate::keys::Keys,
    bindings: Res<Bindings>,
    clicks: Query<(&Interaction, &Slot), Changed<Interaction>>,
    online: Res<crate::online::OnlineState>,
    sender: Res<crate::outbox::Outbox>,
    mut lines: ResMut<crate::notices::Lines>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
) {
    if !keys.focused() {
        return;
    }
    let Some(Action::Item { slot, id }) = requested(&keys, &bindings, &clicks) else {
        return;
    };
    let message = if bound_item(online.world().inventory(), slot, id).is_none() {
        "Bound item unavailable; rebind after moving it".to_owned()
    } else {
        inventory.activate_shortcut(
            slot,
            &online,
            &sender,
            online.world().target().selected,
            online.world().casting().cast.is_some() || online.world().casting().pending.is_some(),
        )
    };
    lines.feedback.flash(message, std::time::Instant::now());
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
        crate::keys::testing::install(&mut app);
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
        crate::keys::testing::install(&mut app);
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
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
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

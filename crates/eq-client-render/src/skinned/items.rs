//! The windows that hold items, drawn from the skin: the inventory's page of
//! slots, purse and figure; each open bag; and the bank. Every slot is the
//! inventory's own slot button, so a click moves an item, a shifted click
//! splits a stack, a right click inspects it (or opens a bag) and an Alt
//! right click uses it, as in the client's own inventory window.
use super::{Skinned, at, picture};
use crate::{
    inventory::SlotButton,
    sheets::Art,
    theme::{self, Size},
    windows::{Shown, Stack, WindowId},
};
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{InvSlot, View},
    ui::Area,
};
use eq_client_core::inventory::InventorySlot;
use std::collections::BTreeSet;

/// A slot drawn from the skin; a bag's slot knows its bag and its place in it.
#[derive(Component, Clone, Copy)]
pub(crate) struct SkinSlot {
    in_bag: Option<(InventorySlot, u8)>,
}

/// What a skinned slot shows of its item, drawn again when the inventory
/// changes.
#[derive(Component)]
pub(crate) struct Content;

/// A bag window's name and picture, the bag's own.
#[derive(Component, Clone, Copy)]
pub(crate) enum BagPart {
    Name(InventorySlot),
    Icon(InventorySlot),
}

/// A button that closes its window, as each window's Done button does.
#[derive(Component, Clone, Copy)]
pub(crate) struct Closes(pub(crate) WindowId);

/// The slot a skin's slot number names in this window: the player's own in
/// the inventory and the bank, a place in the bag in a bag's window.
fn slot_of(window: WindowId, number: u32) -> Option<(InventorySlot, Option<(InventorySlot, u8)>)> {
    let number = i32::try_from(number).ok()?;
    match window {
        WindowId::Bag(bag) => {
            let index = u8::try_from(number.checked_sub(30)?).ok()?;
            let bag = InventorySlot(bag);
            Some((bag.child(index)?, Some((bag, index))))
        }
        _ => Some((InventorySlot(number), None)),
    }
}

/// An item slot: the skin's empty picture, with the item drawn over it.
pub(super) fn slot(
    window: &mut ChildSpawnerCommands,
    art: &mut Art,
    slot: &InvSlot,
    inside: &Area,
    owner: WindowId,
) {
    let Some((number, in_bag)) = slot.slot.and_then(|number| slot_of(owner, number)) else {
        return;
    };
    let area = slot.area;
    window
        .spawn((
            Button,
            SlotButton(number),
            SkinSlot { in_bag },
            at(
                inside.x + area.x,
                inside.y + area.y,
                area.width,
                area.height,
            ),
        ))
        .with_children(|cell| {
            if let Some(piece) = &slot.background {
                picture(cell, art, piece, at(0.0, 0.0, area.width, area.height));
            }
        });
}

/// The inventory's figure: the paperdoll in the skin's frame. An item
/// dropped on it goes where it fits, as the skin's tooltip says.
pub(super) fn figure(
    window: &mut ChildSpawnerCommands,
    view: &View,
    inside: &Area,
    paperdoll: Option<&crate::paperdoll::PaperdollImage>,
) {
    let area = view.area;
    let mut node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    node.justify_content = JustifyContent::Center;
    node.align_items = AlignItems::Center;
    node.overflow = Overflow::clip();
    let mut figure = window.spawn((
        Button,
        crate::inventory::StoreCursor,
        node,
        BackgroundColor(theme::WELL),
    ));
    if let Some(tooltip) = &view.tooltip {
        figure.insert(crate::tooltip::Tooltip(tooltip.clone()));
    }
    if let Some(paperdoll) = paperdoll {
        // The figure keeps its proportions inside the frame.
        let scale = (area.width / paperdoll.size.x).min(area.height / paperdoll.size.y);
        figure.with_child((
            ImageNode::new(paperdoll.handle.clone()),
            Node {
                width: px(paperdoll.size.x * scale),
                height: px(paperdoll.size.y * scale),
                ..default()
            },
        ));
    }
}

/// Windows with no frame until they open: built when they open and gone
/// when they close.
const fn framed_while_open(id: WindowId) -> bool {
    matches!(
        id,
        WindowId::Bank
            | WindowId::Bag(_)
            | WindowId::Give
            | WindowId::ActionsWindow
            | WindowId::PetInfo
            | WindowId::Options
            | WindowId::Training
            | WindowId::Skills
    )
}

/// Opens and closes the windows that have no frame of their own until then:
/// a bag's, while it is open and still a bag, the bank's, while it is open
/// and a banker is in reach, and the give, Actions, Pet Info, Options,
/// Training and Skills windows while they are open.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn frames(
    mut commands: Commands,
    settings: Res<crate::ViewerSettings>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<Stack>),
    state: Res<crate::inventory::InventoryState>,
    online: Res<crate::online::OnlineState>,
    frames: Query<(Entity, &WindowId)>,
) {
    if settings.0.eq_directory.is_none() {
        return;
    }
    if !state.bank_open() {
        shown.close(WindowId::Bank);
    }
    let items = online.world().inventory().items();
    let gone: Vec<WindowId> = shown
        .ids()
        .filter(|id| match id {
            WindowId::Bag(slot) => items
                .get(&InventorySlot(*slot))
                .is_none_or(|bag| bag.bag_slots == 0),
            _ => false,
        })
        .collect();
    for id in gone {
        shown.close(id);
    }
    let mut drawn = BTreeSet::new();
    for (frame, id) in &frames {
        if framed_while_open(*id) {
            if shown.is_open(*id) {
                drawn.insert(*id);
            } else {
                commands.entity(frame).despawn();
            }
        }
    }
    let wanted: Vec<WindowId> = shown
        .ids()
        .filter(|id| framed_while_open(*id) && !drawn.contains(id))
        .collect();
    // A window just opened comes to the front.
    for id in wanted {
        crate::windows::frame(&mut commands, id, Node::default());
        stack.raise(id);
    }
}

/// Opens a bag's window, or closes it if it is open.
pub(crate) fn toggle_bag(shown: &mut Shown, slot: InventorySlot) {
    let id = WindowId::Bag(slot.0);
    if shown.is_open(id) {
        shown.close(id);
    } else {
        shown.open(id);
    }
}

/// Draws each skinned slot's item when the inventory changes or a slot is
/// drawn: its icon (or its initials without one) and its stack's count. A
/// bag's slots beyond its size are hidden, and its window shows its name and
/// picture.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system parameters.
pub(crate) fn contents(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    mut art: Art,
    (mut last, added): (Local<Option<u64>>, Query<(), Added<SkinSlot>>),
    mut slots: Query<(Entity, &SlotButton, &SkinSlot, &mut Node, Option<&Children>)>,
    old: Query<(), With<Content>>,
    (mut names, mut icons): (
        Query<(&BagPart, &mut Text)>,
        Query<(&BagPart, &mut ImageNode)>,
    ),
) {
    let inventory = online.world().inventory();
    let revision = inventory.revision();
    if *last == Some(revision) && added.is_empty() {
        return;
    }
    *last = Some(revision);
    let items = inventory.items();
    for (cell, button, skin, mut node, children) in &mut slots {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        let fits = skin
            .in_bag
            .is_none_or(|(bag, index)| items.get(&bag).is_some_and(|bag| index < bag.bag_slots));
        let display = if fits { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let Some(item) = items.get(&button.0) else {
            commands.entity(cell).remove::<crate::tooltip::Tooltip>();
            continue;
        };
        // The official client names an item under the pointer.
        commands
            .entity(cell)
            .insert(crate::tooltip::Tooltip(item.details.name.clone()));
        let (width, height) = match (node.width, node.height) {
            (Val::Px(width), Val::Px(height)) => (width, height),
            _ => (40.0, 40.0),
        };
        commands.entity(cell).with_children(|cell| {
            if let Some(icon) = art.item(item.icon) {
                cell.spawn((Content, icon, at(1.0, 1.0, width - 2.0, height - 2.0)));
            } else {
                let initials: String = item
                    .details
                    .name
                    .split_whitespace()
                    .filter_map(|word| word.chars().next())
                    .take(2)
                    .collect();
                cell.spawn((
                    Content,
                    theme::text(initials, Size::Label, theme::INK_BRIGHT),
                    at(4.0, 4.0, width - 8.0, height - 8.0),
                ));
            }
            if let Some(count) = item.stack_count.filter(|count| *count > 1) {
                cell.spawn((
                    Content,
                    theme::text(count.to_string(), Size::Body, theme::INK_BRIGHT),
                    TextLayout::new(Justify::Right, LineBreak::NoWrap),
                    at(0.0, height - 14.0, width - 3.0, 13.0),
                ));
            }
        });
    }
    for (part, mut text) in &mut names {
        if let BagPart::Name(bag) = part {
            let name = items
                .get(bag)
                .map_or_else(String::new, |bag| bag.details.name.clone());
            if text.0 != name {
                text.0 = name;
            }
        }
    }
    for (part, mut image) in &mut icons {
        if let BagPart::Icon(bag) = part
            && let Some(icon) = items.get(bag).and_then(|bag| art.item(bag.icon))
        {
            *image = icon;
        }
    }
}

/// Closes a window when its Done button is pressed, and a bag or the bank
/// when Escape finds it in front.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn close(
    mut shown: ResMut<Shown>,
    escape: Res<crate::escape::Escape>,
    buttons: Query<(&Interaction, &Closes), Changed<Interaction>>,
) {
    for (interaction, Closes(window)) in &buttons {
        if *interaction == Interaction::Pressed {
            shown.close(*window);
        }
    }
    // Closing the give window cancels the exchange; see `give::window`.
    if let crate::escape::Escape::Close(id) = *escape
        && framed_while_open(id)
    {
        shown.close(id);
    }
}

/// The quantity picker, over the bottom of the skinned inventory.
#[derive(Component)]
pub(crate) struct Picker;

/// Shows the quantity picker while a stack is being split, over the skinned
/// inventory, which has no place of its own for it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn picker(
    mut commands: Commands,
    state: Res<crate::inventory::InventoryState>,
    online: Res<crate::online::OnlineState>,
    skinned: Res<Skinned>,
    frames: Query<(Entity, &WindowId)>,
    pickers: Query<Entity, With<Picker>>,
    mut last: Local<Option<(u64, u64)>>,
) {
    if !skinned.has(WindowId::Inventory) {
        return;
    }
    let inventory = online.world().inventory();
    let now = (state.revision(), inventory.revision());
    if *last == Some(now) {
        return;
    }
    *last = Some(now);
    for picker in &pickers {
        commands.entity(picker).despawn();
    }
    if !state.splitting() {
        return;
    }
    let Some((frame, _)) = frames.iter().find(|(_, id)| **id == WindowId::Inventory) else {
        return;
    };
    commands.entity(frame).with_children(|window| {
        window
            .spawn((
                Picker,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(8),
                    right: px(8),
                    bottom: px(8),
                    ..default()
                },
                theme::surface(),
                GlobalZIndex(crate::windows::Layer::Popup.base()),
            ))
            .with_children(|picker| {
                crate::inventory::quantity_picker(
                    picker,
                    crate::inventory::View {
                        state: &state,
                        inventory,
                    },
                );
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::inventory::InventoryUpdate;

    #[test]
    fn a_bags_slot_numbers_name_its_places() {
        let bag = WindowId::Bag(22);
        let first = slot_of(bag, 30).unwrap();
        assert_eq!(first.1, Some((InventorySlot(22), 0)));
        assert_eq!(first.0, InventorySlot(22).child(0).unwrap());
        assert_eq!(slot_of(bag, 39).unwrap().1, Some((InventorySlot(22), 9)));
        assert!(slot_of(bag, 29).is_none());
        // The inventory and the bank use the player's own numbering.
        assert_eq!(
            slot_of(WindowId::Inventory, 22).unwrap(),
            (InventorySlot(22), None)
        );
        assert_eq!(
            slot_of(WindowId::Bank, 2003).unwrap(),
            (InventorySlot(2003), None)
        );
    }

    fn app() -> App {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("installation".into());
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(crate::preview::items()),
        );
        app.insert_resource(online)
            .add_systems(Update, (frames, contents).chain());
        app
    }

    fn bag_frames(app: &mut App) -> Vec<WindowId> {
        let mut frames = app.world_mut().query::<&WindowId>();
        frames
            .iter(app.world())
            .copied()
            .filter(|id| matches!(id, WindowId::Bag(_) | WindowId::Bank))
            .collect()
    }

    #[test]
    fn a_bag_window_follows_its_bag_and_the_bank_its_banker() {
        let mut app = app();
        // The preview's backpack is in slot 22; slot 13 holds a sword.
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Bag(22));
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Bag(13));
        app.world_mut().resource_mut::<Shown>().open(WindowId::Bank);
        app.update();
        // A sword is no bag, and no banker is near.
        assert_eq!(bag_frames(&mut app), [WindowId::Bag(22)]);
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Bank));
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Bag(22));
        app.update();
        assert_eq!(bag_frames(&mut app), []);
    }

    #[test]
    fn a_bags_slots_beyond_its_size_are_hidden_and_items_are_named() {
        let mut app = app();
        // The backpack holds eight; the window has places for ten.
        let bag = InventorySlot(22);
        let mut slots = Vec::new();
        for index in [0, 7, 8, 9] {
            slots.push(
                app.world_mut()
                    .spawn((
                        Button,
                        SlotButton(bag.child(index).unwrap()),
                        SkinSlot {
                            in_bag: Some((bag, index)),
                        },
                        at(0.0, 0.0, 40.0, 40.0),
                    ))
                    .id(),
            );
        }
        let sword = app
            .world_mut()
            .spawn((
                Button,
                SlotButton(InventorySlot(13)),
                SkinSlot { in_bag: None },
                at(0.0, 0.0, 40.0, 40.0),
            ))
            .id();
        crate::sheets::testing::blank(&mut app);
        app.update();
        let shown: Vec<bool> = slots
            .iter()
            .map(|slot| app.world().get::<Node>(*slot).unwrap().display != Display::None)
            .collect();
        assert_eq!(shown, [true, true, false, false]);
        assert_eq!(
            app.world().get::<crate::tooltip::Tooltip>(sword).unwrap().0,
            "Preview sword"
        );
    }
}

//! The windows that hold items, drawn from the skin: the inventory's page of
//! slots, purse and figure; each open bag; and the bank. Every slot is the
//! inventory's own slot button, so a click moves an item, a shifted click
//! splits a stack, a right click inspects it (or opens a bag) and an Alt
//! right click uses it, as in the client's own inventory window.
use super::{Skinned, at, picture};
use crate::{
    hud::hotbar,
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

/// One of the other player's trade slots (0 to 7), which shows what they
/// put in; a right click inspects it.
#[derive(Component, Clone, Copy)]
pub(crate) struct TheirSlot(pub(crate) u8);

/// A place in the loot window, from 0, which shows the corpse's item there;
/// a click takes it.
#[derive(Component, Clone, Copy)]
pub(crate) struct LootSlot(pub(crate) u16);

/// The loot window's first place's number (`EQType`); the rest follow.
const FIRST_LOOT_PLACE: u32 = 5000;

/// What a skinned slot shows of its item, drawn again when the inventory
/// changes.
#[derive(Component)]
pub(crate) struct Content;

/// A bag window's name and picture, the bag's own, or those of the world
/// container open for the player.
#[derive(Component, Clone, Copy)]
pub(crate) enum BagPart {
    Name(InventorySlot),
    Icon(InventorySlot),
    WorldName,
    WorldIcon,
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
        // A world container's ten places are 4000 to 4009.
        WindowId::WorldContainer => {
            let index = number.checked_sub(30).filter(|index| *index < 10)?;
            Some((InventorySlot(4000 + index), None))
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
    if owner == WindowId::Loot {
        loot_slot(window, art, slot, inside);
        return;
    }
    let Some((number, in_bag)) = slot.slot.and_then(|number| slot_of(owner, number)) else {
        return;
    };
    let area = slot.area;
    let node = at(
        inside.x + area.x,
        inside.y + area.y,
        area.width,
        area.height,
    );
    let theirs = number
        .0
        .checked_sub(eq_client_core::exchange::THEIR_FIRST_SLOT)
        .and_then(|index| u8::try_from(index).ok());
    let mut cell = match theirs {
        // The other player's slots are not the player's to fill.
        Some(index) if owner == WindowId::Trade && index < 8 => {
            window.spawn((Button, TheirSlot(index), node))
        }
        _ => window.spawn((
            Button,
            SlotButton(number),
            hotbar::Pickable(hotbar::Source::Item(number)),
            SkinSlot { in_bag },
            node,
        )),
    };
    cell.with_children(|cell| {
        if let Some(piece) = &slot.background {
            picture(cell, art, piece, at(0.0, 0.0, area.width, area.height));
        }
    });
}

/// A place in the loot window: the skin's empty picture, with the corpse's
/// item there drawn over it (`loot`). A click takes the item into the packs,
/// as the client's own loot window does; which click does what in the
/// official window is not checked yet (its request can take an item to the
/// cursor or into the packs).
fn loot_slot(window: &mut ChildSpawnerCommands, art: &mut Art, slot: &InvSlot, inside: &Area) {
    let Some(place) = slot
        .slot
        .and_then(|number| number.checked_sub(FIRST_LOOT_PLACE))
        .and_then(|place| u16::try_from(place).ok())
    else {
        return;
    };
    let area = slot.area;
    window
        .spawn((
            Button,
            LootSlot(place),
            crate::trade::Action::Take(place),
            crate::outbox::Needs::Capability(eq_client_core::Capability::Looting),
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

/// Draws the corpse's item in each place of the loot window, when the loot
/// changes or a place is drawn.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system parameters.
pub(crate) fn loot(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    mut art: Art,
    (mut last, added): (
        Local<Option<Vec<(u16, u32, Option<u32>)>>>,
        Query<(), Added<LootSlot>>,
    ),
    slots: Query<(Entity, &LootSlot, &Node, Option<&Children>)>,
    old: Query<(), With<Content>>,
) {
    let items = online.world().loot().map(|loot| &loot.items);
    let shown: Vec<_> = items
        .into_iter()
        .flatten()
        .map(|(place, item)| (*place, item.details.id, item.stack_count))
        .collect();
    if last.as_ref() == Some(&shown) && added.is_empty() {
        return;
    }
    *last = Some(shown);
    for (cell, place, node, children) in &slots {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        draw(
            &mut commands,
            &mut art,
            cell,
            node,
            items.and_then(|items| items.get(&place.0)),
        );
    }
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
pub(crate) const fn framed_while_open(id: WindowId) -> bool {
    matches!(
        id,
        WindowId::Bank
            | WindowId::Bag(_)
            | WindowId::Give
            | WindowId::Trade
            | WindowId::Quantity
            | WindowId::ActionsWindow
            | WindowId::PetInfo
            | WindowId::Group
            | WindowId::Raid
            | WindowId::ShortEffects
            | WindowId::CastBar
            | WindowId::Options
            | WindowId::Training
            | WindowId::Skills
            | WindowId::Confirmation
            | WindowId::Note
            | WindowId::Book
            | WindowId::WorldContainer
            | WindowId::Map
            | WindowId::CharacterSelect
    )
}

/// Opens and closes the windows that have no frame of their own until then:
/// a bag's, while it is open and still a bag, the bank's, while it is open
/// and a banker is in reach, and the give, quantity, Actions, Pet Info,
/// Group, Raid, short effects, casting, Options, Training, Skills,
/// confirmation, note and book windows while they are open and not hidden.
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
            if shown.displayed(*id) {
                drawn.insert(*id);
            } else {
                commands.entity(frame).despawn();
            }
        }
    }
    let wanted: Vec<WindowId> = shown
        .ids()
        .filter(|id| framed_while_open(*id) && shown.displayed(*id) && !drawn.contains(id))
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
    (online, messages): (
        Res<crate::online::OnlineState>,
        Option<Res<crate::hud::messages::Messages>>,
    ),
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
        draw(&mut commands, &mut art, cell, &node, items.get(&button.0));
    }
    let container = online.world().container();
    for (part, mut text) in &mut names {
        let name = match part {
            BagPart::Name(bag) => items
                .get(bag)
                .map_or_else(String::new, |bag| bag.details.name.clone()),
            BagPart::WorldName => {
                container.map_or_else(String::new, |view| world_name(view, messages.as_deref()))
            }
            BagPart::Icon(_) | BagPart::WorldIcon => continue,
        };
        if text.0 != name {
            text.0 = name;
        }
    }
    for (part, mut image) in &mut icons {
        let icon = match part {
            BagPart::Icon(bag) => items.get(bag).and_then(|bag| bag.details.icon),
            // Servers may send no icon (0) for a world container.
            BagPart::WorldIcon => container.map(|view| view.icon).filter(|icon| *icon != 0),
            BagPart::Name(_) | BagPart::WorldName => None,
        };
        if let Some(icon) = icon.and_then(|icon| art.item(icon)) {
            *image = icon;
        }
    }
}

/// Draws an item in a slot: its icon (or its initials without one) and its
/// stack's count, named under the pointer as in the official client; an
/// empty slot loses its name.
fn draw(
    commands: &mut Commands,
    art: &mut Art,
    cell: Entity,
    node: &Node,
    item: Option<&eq_client_core::inventory::InventoryItem>,
) {
    let Some(item) = item else {
        commands.entity(cell).remove::<crate::tooltip::Tooltip>();
        return;
    };
    commands
        .entity(cell)
        .insert(crate::tooltip::Tooltip(item.details.name.clone()));
    let (width, height) = match (node.width, node.height) {
        (Val::Px(width), Val::Px(height)) => (width, height),
        _ => (40.0, 40.0),
    };
    commands.entity(cell).with_children(|cell| {
        if let Some(icon) = item.details.icon.and_then(|icon| art.item(icon)) {
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

/// Draws what the other player put in each of their trade slots, when it
/// changes or a slot is drawn.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system parameters.
pub(crate) fn theirs(
    mut commands: Commands,
    online: Res<crate::online::OnlineState>,
    mut art: Art,
    (mut last, added): (
        Local<Option<Vec<(u8, u32, Option<u32>)>>>,
        Query<(), Added<TheirSlot>>,
    ),
    slots: Query<(Entity, &TheirSlot, &Node, Option<&Children>)>,
    old: Query<(), With<Content>>,
) {
    let theirs = online.world().exchange().map(|exchange| &exchange.theirs);
    let shown: Vec<_> = theirs
        .into_iter()
        .flatten()
        .map(|(index, item)| (*index, item.details.id, item.stack_count))
        .collect();
    if last.as_ref() == Some(&shown) && added.is_empty() {
        return;
    }
    *last = Some(shown);
    for (cell, slot, node, children) in &slots {
        for child in children.into_iter().flatten() {
            if old.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        draw(
            &mut commands,
            &mut art,
            cell,
            node,
            theirs.and_then(|theirs| theirs.get(&slot.0)),
        );
    }
}

/// What a world container's window calls it: the name the server sends,
/// or, without one, its type's name in the installed client's strings.
fn world_name(
    view: &eq_client_core::ground::ContainerView,
    messages: Option<&crate::hud::messages::Messages>,
) -> String {
    if !view.name.is_empty() {
        return view.name.clone();
    }
    u8::try_from(view.object_type)
        .ok()
        .and_then(eq_client_core::tradeskills::type_name)
        .zip(messages)
        .map_or_else(String::new, |(id, messages)| messages.text(id, ""))
}

/// Closes a window when its Done button or close box is pressed, and a bag
/// or the bank when Escape finds it in front; one something else opens, such
/// as the pet window, is hidden instead.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn close(
    mut shown: ResMut<Shown>,
    escape: Res<crate::escape::Escape>,
    buttons: Query<(&Interaction, &Closes), Changed<Interaction>>,
) {
    for (interaction, Closes(window)) in &buttons {
        if *interaction == Interaction::Pressed {
            shown.dismiss(*window);
        }
    }
    // Closing the give or trade window cancels the exchange; see
    // `give::window`.
    if let crate::escape::Escape::Close(id) = *escape
        && framed_while_open(id)
    {
        shown.dismiss(id);
    }
}

/// Opens the skin's quantity window while the player chooses how many of a
/// stack or of a kind of coins to pick up from the skinned inventory, and
/// closes it once they have; closing it, as its close box does, takes
/// nothing. The client's own inventory keeps its own picker.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn quantity(
    mut shown: ResMut<Shown>,
    mut state: ResMut<crate::inventory::InventoryState>,
    skinned: Res<Skinned>,
    mut opened: Local<bool>,
) {
    let open = shown.is_open(WindowId::Quantity);
    if *opened && !open {
        state.cancel_split();
    }
    let wanted = state.splitting() && skinned.has(WindowId::Inventory);
    if wanted && !open {
        shown.open(WindowId::Quantity);
    } else if !wanted && open {
        shown.close(WindowId::Quantity);
    }
    *opened = wanted;
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

    #[test]
    fn the_quantity_window_opens_while_the_player_picks_and_closing_it_takes_nothing() {
        use crate::inventory::InventoryState;
        use eq_client_core::money::{Coin, CoinPlace};
        let mut app = crate::testing::app();
        app.add_systems(Update, quantity);
        app.world_mut()
            .resource_mut::<Skinned>()
            .0
            .insert(WindowId::Inventory);
        let pick = |app: &mut App| {
            app.world_mut()
                .resource_mut::<InventoryState>()
                .select_coins(CoinPlace::Purse, Coin::Gold, 12);
            app.update();
        };
        let open = |app: &App| app.world().resource::<Shown>().is_open(WindowId::Quantity);
        pick(&mut app);
        assert!(open(&app));
        // Picking up, or cancelling, closes it.
        app.world_mut()
            .resource_mut::<InventoryState>()
            .cancel_split();
        app.update();
        assert!(!open(&app));
        // Closing it, as its close box does, takes nothing.
        pick(&mut app);
        app.world_mut()
            .resource_mut::<Shown>()
            .dismiss(WindowId::Quantity);
        app.update();
        assert!(app.world().resource::<InventoryState>().picked().is_none());
        assert!(!open(&app));
        // The client's own inventory keeps its own picker.
        app.world_mut().resource_mut::<Skinned>().0.clear();
        pick(&mut app);
        assert!(!open(&app));
    }

    #[test]
    fn a_loot_place_shows_the_corpses_item_in_its_slot() {
        use eq_client_core::{WorldEvent, WorldUpdate, loot::LootUpdate, world::NoSpells};
        let mut online = OnlineState::new(false);
        online.open_loot(9);
        let mut item = crate::preview::items().into_iter().next().unwrap();
        // The corpse's second place, the window's second place.
        item.slot = InventorySlot(23);
        for event in [
            LootUpdate::Item {
                place: 1,
                item: Box::new(item.clone()),
            },
            LootUpdate::Listed { corpse_id: 9 },
        ] {
            online.tell(
                &WorldUpdate::Game(WorldEvent::Loot(event)),
                std::time::Instant::now(),
                &NoSpells,
            );
        }
        let mut app = crate::testing::app();
        app.insert_resource(online).add_systems(Update, loot);
        let place = |app: &mut App, index| {
            app.world_mut()
                .spawn((LootSlot(index), at(0.0, 0.0, 40.0, 40.0)))
                .id()
        };
        let (first, second) = (place(&mut app, 0), place(&mut app, 1));
        app.update();
        let named = |app: &App, place| {
            app.world()
                .get::<crate::tooltip::Tooltip>(place)
                .map(|tooltip| tooltip.0.clone())
        };
        assert_eq!(named(&app, first), None);
        assert_eq!(named(&app, second), Some(item.details.name));
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

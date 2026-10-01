//! Compact inventory drawer with validated slot moves and local item inspection.
pub(super) mod colors;
pub(super) mod cursor;
mod interaction;
mod layout;

use bevy::prelude::*;
use eq_client_core::inventory::{Inventory, InventorySlot, InventoryUpdate};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Tab {
    #[default]
    Inventory,
    Bank,
}

/// The inventory window: what it shows and the actions under way. The items
/// themselves are the world's.
#[derive(Resource, Default)]
#[allow(clippy::struct_excessive_bools)] // These flags represent independent UI and server state.
pub(super) struct InventoryState {
    colors: colors::Colors,
    pub hovered: bool,
    open: bool,
    demo: bool,
    tab: Tab,
    revision: u64,
    next_use_id: u64,
    bank_open: bool,
    actions: interaction::Actions,
    /// Moves the offline demo settled itself, on their way to the world.
    demo_news: Vec<InventoryUpdate>,
}

/// What the inventory window draws: its own state, and the items as the
/// world has them.
#[derive(Clone, Copy)]
pub(super) struct View<'a> {
    pub state: &'a InventoryState,
    pub inventory: &'a Inventory,
}

impl InventoryState {
    /// Whether an action is under way that Escape cancels: a split, or
    /// storing the cursor's item.
    pub(super) const fn action_under_way(&self) -> bool {
        self.actions.auto_store || self.actions.split.is_some()
    }

    /// Whether the inventory window is showing.
    pub(super) const fn is_open(&self) -> bool {
        self.open
    }

    /// Current request feedback, also shown when inventory is closed for hotbar use.
    pub(crate) fn action_message(&self) -> &str {
        &self.actions.message
    }
    /// Uses the same item controller for action-bar and inventory activation.
    pub(crate) fn activate_shortcut(
        &mut self,
        slot: InventorySlot,
        online: &super::online::OnlineState,
        sender: &crate::outbox::Outbox,
        target: Option<u16>,
        casting: bool,
    ) -> String {
        self.use_slot(slot, online, sender, target, casting);
        self.actions.message.clone()
    }
    /// Closes personal banking as soon as admission, life, or banker proximity changes.
    fn refresh_bank_access(&mut self, online: &super::online::OnlineState) {
        if self.demo && !online.enabled {
            return;
        }
        let available = !self.demo
            && online.world.connected()
            && online.world.death().is_none()
            && online.world.session_id().is_some()
            && online.world.player().is_some_and(|player| {
                online
                    .world
                    .spawns()
                    .values()
                    .map(|spawn| &spawn.state)
                    .any(|spawn| eq_client_core::inventory::banker_in_range(player.position, spawn))
            });
        if self.bank_open != available {
            self.bank_open = available;
            self.actions.split = None;
            if !available && self.tab == Tab::Bank {
                self.tab = Tab::Inventory;
            }
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Follows a change to the world's inventory: one gone stale ends the
    /// split and the automatic storage under way.
    pub fn refresh(&mut self, stale: bool) {
        if stale {
            self.actions.auto_store = false;
            self.actions.split = None;
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Forgets the window's state along with the inventory the world forgot.
    pub fn forget(&mut self) {
        self.cancel_actions();
        self.demo_news.clear();
        self.demo = false;
        self.bank_open = false;
        // The bank tab closes with the bank; a stale choice would show no slots.
        self.tab = Tab::Inventory;
        self.revision = self.revision.wrapping_add(1);
        self.hovered = false;
    }
}
#[derive(Component)]
pub(super) struct Toggle;
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Rows;
#[derive(Component)]
pub(super) struct Status;
#[derive(Component)]
pub(super) struct HoverLabel;
#[derive(Component)]
pub(super) struct TabButton(Tab);
#[derive(Component)]
pub(super) struct SlotButton(pub(super) InventorySlot);

/// The equipment layout of the skin in use, read again when the skin changes.
#[derive(Default)]
pub(super) struct SkinLayout {
    skin: Option<String>,
    layout: Option<eq_client_assets::ui::EquipmentLayout>,
}

impl SkinLayout {
    /// Reads `skin`'s layout, or the default skin's when that fails, unless this
    /// skin's is the one already read. True when the layout was read again.
    fn follow(&mut self, directory: Option<&std::path::Path>, skin: &str) -> bool {
        use eq_client_assets::ui::{DEFAULT_SKIN, equipment_layout};
        if self.skin.as_deref() == Some(skin) {
            return false;
        }
        self.skin = Some(skin.to_owned());
        self.layout = directory.and_then(|directory| {
            equipment_layout(directory, skin)
                .or_else(|error| {
                    if skin == DEFAULT_SKIN {
                        return Err(error);
                    }
                    warn!("Inventory layout of skin {skin} unavailable: {error}");
                    equipment_layout(directory, DEFAULT_SKIN)
                })
                .inspect_err(|error| warn!("Inventory skin layout unavailable: {error}"))
                .ok()
        });
        true
    }
}
#[derive(Component)]
pub(super) struct StoreCursor;
#[derive(Component)]
pub(super) struct SplitStack(InventorySlot);

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct RenderStamp {
    revision: u64,
    tab: Tab,
    open: bool,
    bank_open: bool,
    root: Entity,
    /// The world inventory's revision.
    inventory: u64,
}

/// Creates the inventory toggle and an initially closed drawer.
pub(super) fn spawn(commands: &mut Commands) {
    cursor::spawn(commands);
    commands
        .spawn((
            super::hud::HudRoot,
            Button,
            Toggle,
            GlobalZIndex(25),
            // Beside the target window, clear of the status box, which grows
            // with the status line and the interaction prompt, and of the
            // spellbook at the top right.
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                left: percent(50),
                margin: UiRect::left(px(156)),
                padding: UiRect::axes(px(12), px(7)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.032, 0.04, 0.95)),
        ))
        .with_children(|button| {
            label(button, "Inventory [I]", 12.0);
        });
    let frame = commands
        .spawn((
            super::hud::HudRoot,
            Panel,
            super::windows::pointer::TakesWheel,
            GlobalZIndex(25),
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                top: px(110),
                width: px(600),
                max_width: percent(95),
                max_height: percent(62),
                overflow: Overflow::scroll_y(),
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgb(0.025, 0.032, 0.04)),
        ))
        .id();
    super::windows::interactive(commands, frame);
    commands.entity(frame).with_children(|panel| {
        super::windows::title_bar(panel, frame, "INVENTORY");
        panel
            .spawn(Node {
                column_gap: px(6),
                ..default()
            })
            .with_children(|tabs| {
                for (tab, name) in [(Tab::Inventory, "Inventory"), (Tab::Bank, "Bank")] {
                    tabs.spawn((
                        Button,
                        TabButton(tab),
                        Node {
                            padding: UiRect::axes(px(12), px(6)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.08, 0.10, 0.13)),
                    ))
                    .with_children(|button| {
                        label(button, name, 12.0);
                    });
                }
            });
        panel.spawn((
            Status,
            Text::new("Inventory not received"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.72, 0.60)),
        ));
        panel.spawn((
            HoverLabel,
            Text::new("Click: move / count: split / right-click: inspect / Alt+right-click: use"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.87, 0.80)),
            Node {
                min_height: px(14),
                ..default()
            },
        ));
        panel.spawn((
            Rows,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        ));
    });
}
fn label(parent: &mut ChildSpawnerCommands, text: &str, size: f32) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.88, 0.89, 0.91)),
    ));
}

/// Left-click moves through the cursor, Shift opens a quantity picker, and right-click inspects.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    online: Res<super::online::OnlineState>,
    sender: Res<crate::outbox::Outbox>,
    toggles: Query<&Interaction, (With<Toggle>, Changed<Interaction>)>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    slots: Query<(&Interaction, &SlotButton)>,
    store: Query<&Interaction, (With<StoreCursor>, Changed<Interaction>)>,
    split_buttons: Query<(&Interaction, &interaction::SplitAction), Changed<Interaction>>,
    stack_counts: Query<(&Interaction, &SplitStack), Changed<Interaction>>,
    mut state: ResMut<InventoryState>,
    mut items: ResMut<super::items::ItemState>,
    (chat, escape): (Res<super::chat::ChatState>, Res<super::escape::Escape>),
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    state.refresh_bank_access(&online);
    if !windows.single().is_ok_and(|window| window.focused) {
        state.actions.auto_store = false;
        return;
    }
    if *escape == super::escape::Escape::Inventory {
        state.actions.auto_store = false;
        state.actions.split = None;
        if state.actions.pending.is_none() {
            state.actions.message.clear();
        }
        state.revision = state.revision.wrapping_add(1);
    }
    if (!chat.composing && keys.just_pressed(KeyCode::KeyI))
        || toggles.iter().any(|i| *i == Interaction::Pressed)
    {
        state.open = !state.open;
    }
    if !state.open {
        state.actions.auto_store = false;
        state.actions.split = None;
        return;
    }
    // A nested quantity button consumes the click before its enclosing item slot.
    if let Some((_, stack)) = stack_counts
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
    {
        if online
            .world
            .inventory()
            .items()
            .contains_key(&InventorySlot::CURSOR)
        {
            state.click_slot(stack.0, false, &online, &sender);
        } else {
            state.select_split(stack.0, online.world.inventory());
        }
        return;
    }
    if store
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        state.actions.split = None;
        state.actions.auto_store = true;
    }
    state.store_cursor(&online, &sender);
    for (interaction, action) in &split_buttons {
        if *interaction == Interaction::Pressed {
            state.split_action(*action, &online, &sender);
        }
    }
    for (interaction, tab) in &tabs {
        if *interaction == Interaction::Pressed && (tab.0 != Tab::Bank || state.bank_open) {
            state.tab = tab.0;
        }
    }
    for (interaction, slot) in &slots {
        if *interaction == Interaction::None {
            continue;
        }
        if mouse.just_pressed(MouseButton::Right) {
            if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
                if !chat.composing {
                    let casting = online.world.casting();
                    let casting = casting.cast.is_some() || casting.pending.is_some();
                    state.use_slot(
                        slot.0,
                        &online,
                        &sender,
                        online.world.target().selected,
                        casting,
                    );
                }
            } else if let Some(item) = online.world.inventory().items().get(&slot.0) {
                items.open_received(item.details.clone());
            }
        } else if mouse.just_pressed(MouseButton::Left) && *interaction == Interaction::Pressed {
            if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                state.select_split(slot.0, online.world.inventory());
            } else {
                state.click_slot(slot.0, false, &online, &sender);
            }
        }
    }
}

fn visible_slots(inventory: &Inventory, tab: Tab) -> Vec<InventorySlot> {
    let roots: Vec<_> = match tab {
        Tab::Inventory => (0..31).map(InventorySlot).collect(),
        Tab::Bank => (2000..=2007).map(InventorySlot).collect(),
    };
    let mut slots = Vec::new();
    for root in roots {
        slots.push(root);
        if let Some(item) = inventory.items().get(&root) {
            slots.extend((0..item.bag_slots).filter_map(|index| root.child(index)));
        }
    }
    // Preserve unusual addresses and partial child-only updates visibly.
    if tab == Tab::Inventory {
        for slot in inventory.items().keys() {
            if !slot.is_equipment() && slot.0 < 2000 && !slots.contains(slot) {
                slots.push(*slot);
            }
        }
    }
    slots
}

/// Rebuilds rows only when contents or the selected tab change.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    state: Res<InventoryState>,
    online: Res<super::online::OnlineState>,
    settings: Res<super::ViewerSettings>,
    mut art: super::sheets::Art,
    skin: Res<super::skin::UiSkin>,
    mut skin_layout: Local<SkinLayout>,
    figure: Option<Res<super::paperdoll::PaperdollImage>>,
    mut previous: Local<Option<RenderStamp>>,
    mut panels: Query<&mut Node, (With<Panel>, Without<TabButton>)>,
    rows: Query<Entity, With<Rows>>,
    mut statuses: Query<&mut Text, With<Status>>,
    mut tabs: Query<(&TabButton, &mut BackgroundColor, &mut Node), Without<Panel>>,
) {
    for mut panel in &mut panels {
        panel.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Ok(root) = rows.single() else {
        return;
    };
    let directory = settings.0.eq_directory.as_deref();
    // Equipment follows the skin's inventory window; without one, a labeled grid.
    if skin_layout.follow(directory, &skin.0) {
        *previous = None;
    }
    let inventory = online.world.inventory();
    let stamp = RenderStamp {
        revision: state.revision,
        tab: state.tab,
        open: state.open,
        bank_open: state.bank_open,
        root,
        inventory: inventory.revision(),
    };
    if *previous == Some(stamp) {
        return;
    }
    *previous = Some(stamp);
    for mut status in &mut statuses {
        status.0 = if state.demo {
            "Offline demo / click to pick up / click count: choose quantity".into()
        } else if inventory.awaiting_correction() {
            "Server corrected inventory / waiting for remaining slots".into()
        } else if inventory.stale() {
            "Contents may be outdated - awaiting refresh".into()
        } else if !inventory.received() {
            "Inventory not received in full".into()
        } else {
            "Click to pick up, place or swap / click count: choose quantity".into()
        };
    }
    for mut status in &mut statuses {
        if inventory.received() && !inventory.stale() && !state.actions.message.is_empty() {
            status.0.clone_from(&state.actions.message);
        } else if inventory.predicted() && !inventory.stale() && !state.demo {
            status.0 = "Move sent / contents include local prediction".into();
        }
    }
    if !state.bank_open && state.tab == Tab::Bank {
        return;
    }
    for (tab, mut color, mut node) in &mut tabs {
        node.display = if tab.0 == Tab::Bank && !state.bank_open {
            Display::None
        } else {
            Display::Flex
        };
        color.0 = if tab.0 == state.tab {
            Color::srgb(0.20, 0.22, 0.28)
        } else {
            Color::srgb(0.08, 0.10, 0.13)
        };
    }
    commands.entity(root).despawn_children();
    commands.entity(root).with_children(|list| {
        layout::contents(
            list,
            View {
                state: &state,
                inventory,
            },
            skin_layout.layout.as_ref().map(|layout| layout::Paperdoll {
                layout,
                figure: figure.as_deref(),
            }),
            &mut art,
        );
    });
}

/// Highlights squares and shows full names without crowding the slot grid.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn feedback(
    online: Res<super::online::OnlineState>,
    mut slots: Query<(&SlotButton, &Interaction, &mut BorderColor)>,
    mut labels: Query<&mut Text, With<HoverLabel>>,
) {
    let mut description =
        "Click: move / count: split / right-click: inspect / Alt+right-click: use".to_owned();
    let inventory = online.world.inventory();
    for (slot, interaction, mut border) in &mut slots {
        let item = inventory.items().get(&slot.0);
        let hovered = *interaction != Interaction::None;
        let tint = if slot.0 == InventorySlot::CURSOR && item.is_some() {
            Color::srgb(0.45, 0.82, 1.0)
        } else if hovered {
            Color::srgb(0.88, 0.77, 0.45)
        } else if item.is_some() {
            Color::srgb(0.48, 0.43, 0.31)
        } else {
            Color::srgb(0.22, 0.25, 0.28)
        };
        *border = BorderColor::all(tint);
        if hovered {
            let value = item.map_or_else(
                || {
                    if inventory.received() && !inventory.stale() {
                        "Empty".into()
                    } else {
                        "Unknown".into()
                    }
                },
                |item| {
                    let count = item
                        .stack_count
                        .map_or_else(String::new, |n| format!(" x{n}"));
                    let charges = if item.stack_count.is_none() && item.charges > 0 {
                        format!(" / {} charges", item.charges)
                    } else {
                        String::new()
                    };
                    format!("{}{count}{charges}", item.details.name)
                },
            );
            description = format!("{}: {value}", slot.0.label());
            if let Some(effect) = item.and_then(|item| item.activation.effect.as_ref()) {
                use std::fmt::Write;
                let _ = write!(
                    description,
                    "\nAlt+right-click: use spell {} / level {}",
                    effect.spell_id, effect.required_level
                );
            }
        }
    }
    for mut text in &mut labels {
        if text.0 != description {
            text.0.clone_from(&description);
        }
    }
}

/// Scrolls inventory without zooming or dragging the camera through the drawer.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut panels: Query<
        (
            Entity,
            &UiGlobalTransform,
            &ComputedNode,
            &mut ScrollPosition,
        ),
        With<Panel>,
    >,
    wheel: Res<super::windows::pointer::Wheel>,
    mut state: ResMut<InventoryState>,
) {
    state.hovered = false;
    if !state.open {
        return;
    }
    let Some(cursor) = windows
        .single()
        .ok()
        .and_then(Window::physical_cursor_position)
    else {
        return;
    };
    for (entity, transform, node, mut position) in &mut panels {
        if super::windows::contains(cursor, transform, node) {
            state.hovered = true;
        }
        if wheel.surface == Some(entity) {
            super::windows::scroll_by(&mut position, node, wheel.pixels);
        }
    }
}

/// Supplies clearly labelled synthetic contents for offline visual validation.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn demo(
    settings: Res<super::ViewerSettings>,
    mut online: ResMut<super::online::OnlineState>,
    mut state: ResMut<InventoryState>,
) {
    if !settings.0.demo_inventory || online.enabled {
        return;
    }
    state
        .demo_news
        .push(InventoryUpdate::Snapshot(demo_items()));
    tell(&mut state, &mut online);
    state.open = true;
    state.bank_open = settings.0.demo_bank;
    state.tab = if state.bank_open {
        Tab::Bank
    } else {
        Tab::Inventory
    };
    state.demo = true;
}

/// The offline demo settles its own moves: they reach the world as news, as
/// the session's would.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn settle(
    mut state: ResMut<InventoryState>,
    mut online: ResMut<super::online::OnlineState>,
) {
    tell(&mut state, &mut online);
}

/// Gives the world the demo's news and follows what it changed.
fn tell(state: &mut InventoryState, online: &mut super::online::OnlineState) {
    use eq_client_core::{WorldEvent, WorldUpdate, world::NoSpells};
    for update in std::mem::take(&mut state.demo_news) {
        let news = WorldUpdate::Game(WorldEvent::Inventory(update));
        if online
            .world
            .apply(&news, std::time::Instant::now(), &NoSpells)
            .inventory
        {
            state.refresh(online.world.inventory().stale());
        }
    }
}

pub(crate) fn demo_items() -> Vec<eq_client_core::inventory::InventoryItem> {
    use eq_client_core::{ItemDetails, inventory::InventoryItem};
    let mut items = Vec::new();
    for (slot, id, name, count, bag) in [
        (13, 1, "Preview sword", None, 0),
        (22, 2, "Preview backpack", None, 8),
        (23, 7, "Preview satchel", None, 10),
        (261, 8, "Preview arrows", Some(50), 0),
        (251, 3, "Preview rations", Some(20), 0),
        (252, 4, "Preview bandages", Some(7), 0),
        (30, 5, "Preview lantern", None, 0),
        (2000, 6, "Preview bank item", None, 0),
    ] {
        items.push(InventoryItem {
            activation: eq_client_core::inventory::ItemActivation::default(),
            scroll_spell: None,
            rules: eq_client_core::inventory::ItemPlacement {
                stack_size: if id == 8 { 100 } else { 20 },
                size: 1,
                bag_size: 4,
                item_type: if id == 8 { 27 } else { 0 },
                ..default()
            },
            slot: InventorySlot(slot),
            icon: match id {
                2 => 557,
                7 => 539,
                3 => 537,
                4 => 538,
                8 => 598,
                _ => 519,
            },
            stack_count: count,
            charges: 0,
            bag_slots: bag,
            details: ItemDetails {
                equipment: None,
                bonuses: None,
                id,
                name: name.into(),
                lore: String::new(),
                weight_tenths: 10,
                slots: if id == 1 {
                    1 << 13
                } else if id == 8 {
                    1 << 21
                } else {
                    0
                },
                classes: u32::MAX,
                races: u32::MAX,
                flags: Vec::new(),
                stats: Vec::new(),
            },
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut app = crate::testing::app();
        app.add_systems(Startup, |mut commands: Commands| {
            spawn(&mut commands);
            super::super::items::spawn(&mut commands);
        })
        .add_systems(
            Update,
            (input, settle, update, feedback, super::super::items::update).chain(),
        );
        app
    }
    #[test]
    fn chat_typing_and_unfocused_keys_do_not_toggle_inventory() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        app.update();
        assert!(!app.world().resource::<InventoryState>().open);
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = false;
        let world = app.world_mut();
        let mut windows = world.query::<&mut Window>();
        windows.single_mut(world).unwrap().focused = false;
        app.update();
        assert!(!app.world().resource::<InventoryState>().open);
    }

    #[test]
    fn inventory_toggle_tabs_and_local_inspection_work_without_a_network_sender() {
        let mut app = app();
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            InventoryUpdate::Snapshot(demo_items()),
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyI);
        app.update();
        assert!(app.world().resource::<InventoryState>().open);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut().resource_mut::<InventoryState>().tab = Tab::Inventory;
        app.update();
        let world = app.world_mut();
        let mut rows = world.query::<(&SlotButton, &mut Interaction)>();
        let (_, mut interaction) = rows
            .iter_mut(world)
            .find(|(slot, _)| slot.0 == InventorySlot(251))
            .unwrap();
        *interaction = Interaction::Hovered;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        assert_eq!(
            app.world()
                .resource::<super::super::items::ItemState>()
                .selected(),
            Some(3)
        );
        let world = app.world_mut();
        let mut text = world.query_filtered::<&Text, With<super::super::items::ItemText>>();
        assert!(text.single(world).unwrap().0.contains("Preview rations"));
        // The world forgets the inventory, as when the player camps.
        *app.world_mut()
            .resource_mut::<super::super::online::OnlineState>() =
            super::super::online::OnlineState::new(false);
        app.world_mut().resource_mut::<InventoryState>().forget();
        app.update();
        let world = app.world_mut();
        let mut text = world.query::<&Text>();
        assert!(text.iter(world).any(|t| t.0 == "?"));
        assert!(!text.iter(world).any(|t| t.0 == "Preview rations x20"));
    }
    #[test]
    fn bags_show_empty_capacity_and_bank_items_are_separate() {
        let mut inventory = Inventory::default();
        inventory.apply(InventoryUpdate::Snapshot(demo_items()));
        let packs = visible_slots(&inventory, Tab::Inventory);
        assert!(packs.contains(&InventorySlot(254)));
        assert!(!packs.contains(&InventorySlot(2000)));
        assert!(!InventoryState::default().bank_open);
        assert_eq!(
            visible_slots(&inventory, Tab::Bank),
            (2000..=2007).map(InventorySlot).collect::<Vec<_>>()
        );
    }
    #[test]
    fn shift_click_opens_picker_and_buttons_pick_up_the_selected_quantity() {
        let mut app = app();
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            InventoryUpdate::Snapshot(
                demo_items()
                    .into_iter()
                    .filter(|item| item.slot != InventorySlot::CURSOR)
                    .collect(),
            ),
        );
        {
            let mut state = app.world_mut().resource_mut::<InventoryState>();
            state.demo = true;
            state.open = true;
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        press_slot(&mut app, 251);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert!(
            app.world()
                .resource::<InventoryState>()
                .actions
                .split
                .is_some()
        );
        for choose_more in [true, false] {
            let world = app.world_mut();
            let mut buttons = world.query::<(&interaction::SplitAction, &mut Interaction)>();
            for (action, mut interaction) in buttons.iter_mut(world) {
                if matches!(
                    (choose_more, action),
                    (true, interaction::SplitAction::More)
                        | (false, interaction::SplitAction::Confirm)
                ) {
                    *interaction = Interaction::Pressed;
                }
            }
            app.update();
        }
        assert!(
            app.world()
                .resource::<InventoryState>()
                .actions
                .split
                .is_none()
        );
        let items = items(&app);
        assert_eq!(items.items()[&InventorySlot::CURSOR].stack_count, Some(2));
        assert_eq!(items.items()[&InventorySlot(251)].stack_count, Some(18));
    }

    #[test]
    fn left_click_uses_the_cursor_and_escape_does_not_discard_a_held_item() {
        let mut app = app();
        crate::online::testing::inventory(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            InventoryUpdate::Snapshot(
                demo_items()
                    .into_iter()
                    .filter(|item| item.slot != InventorySlot::CURSOR)
                    .collect(),
            ),
        );
        {
            let mut state = app.world_mut().resource_mut::<InventoryState>();
            state.demo = true;
            state.open = true;
            state.tab = Tab::Inventory;
        }
        app.update();
        press_slot(&mut app, 251);
        assert_eq!(
            items(&app).items()[&InventorySlot::CURSOR].stack_count,
            Some(20)
        );
        press_slot(&mut app, 24);
        assert_eq!(
            items(&app).items()[&InventorySlot(24)].stack_count,
            Some(20)
        );
        assert!(!items(&app).items().contains_key(&InventorySlot(251)));
        press_slot(&mut app, 24);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(items(&app).items().contains_key(&InventorySlot::CURSOR));
    }

    /// The items as the world has them.
    fn items(app: &App) -> &Inventory {
        app.world()
            .resource::<super::super::online::OnlineState>()
            .world
            .inventory()
    }
    fn press_slot(app: &mut App, slot: i32) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        let world = app.world_mut();
        let mut slots = world.query::<(&SlotButton, &mut Interaction)>();
        for (button, mut interaction) in slots.iter_mut(world) {
            *interaction = if button.0 == InventorySlot(slot) {
                Interaction::Pressed
            } else {
                Interaction::None
            };
        }
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
    }
}

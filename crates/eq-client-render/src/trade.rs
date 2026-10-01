//! Corpse loot and merchant windows; the server settles every transfer.
//!
//! L loots the targeted corpse, U trades with the targeted NPC, and Escape
//! closes the window (see `escape`). A window closes only once the server has
//! been told, so the server never keeps a session the player can no longer end.
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::{
    ClientCommand, Coins, SpawnKind,
    inventory::InventoryItem,
    loot::{LootResponse, LootUpdate},
    merchant::{MerchantItem, MerchantUpdate},
};
use std::collections::BTreeMap;

use super::windows;

/// The NPC class that answers ordinary shop requests; servers ignore other classes.
const MERCHANT_CLASS: u8 = 41;

/// Open loot and merchant sessions for the current admission.
#[derive(Resource, Default)]
pub(super) struct TradeState {
    session: Option<u64>,
    /// Carried coins last reported by the server.
    pub coins: Option<Coins>,
    loot: Option<LootWindow>,
    merchant: Option<MerchantWindow>,
    revision: u64,
}

struct LootWindow {
    corpse_id: u16,
    name: String,
    items: BTreeMap<u16, InventoryItem>,
    listed: bool,
    pending: Option<u16>,
    loot_all: bool,
}

struct MerchantWindow {
    merchant_id: u16,
    name: String,
    stock: BTreeMap<u32, MerchantItem>,
}

impl TradeState {
    /// Whether the loot window is open.
    pub(super) const fn looting(&self) -> bool {
        self.loot.is_some()
    }

    /// Whether the merchant window is open.
    pub(super) const fn shopping(&self) -> bool {
        self.merchant.is_some()
    }

    /// Forgets windows from an old admission; coins stay until the server reports them.
    pub(super) fn reset(&mut self, session: Option<u64>) {
        if self.session != session {
            self.session = session;
            self.loot = None;
            self.merchant = None;
            self.revision += 1;
        }
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Open windows as `(slot, item id)` lists, for script reports.
    pub(super) fn summary(&self) -> String {
        let loot = self.loot.as_ref().map(|window| {
            let items: Vec<_> = window
                .items
                .iter()
                .map(|(slot, item)| (*slot, item.details.id))
                .collect();
            format!("loot corpse={} items={items:?}", window.corpse_id)
        });
        let merchant = self.merchant.as_ref().map(|window| {
            let stock: Vec<_> = window
                .stock
                .values()
                .map(|entry| (entry.slot, entry.item.details.id, entry.price))
                .collect();
            format!("merchant {} stock={stock:?}", window.merchant_id)
        });
        [loot, merchant]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Like the Titanium client, applies coin changes the server reports without a
    /// money update (loot coins, purchase prices). Only the total is tracked exactly;
    /// the next server money update restores the true denominations.
    fn adjust_coins(&mut self, copper: i64) {
        if let Some(coins) = &mut self.coins {
            let total = i64::try_from(coins.total_copper()).unwrap_or(i64::MAX);
            let total = u64::try_from(total.saturating_add(copper).max(0)).unwrap_or(0);
            let denomination = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
            *coins = Coins {
                platinum: denomination(total / 1000),
                gold: denomination(total / 100 % 10),
                silver: denomination(total / 10 % 10),
                copper: denomination(total % 10),
            };
        }
    }

    /// Applies one loot update and returns chat feedback.
    pub(super) fn apply_loot(&mut self, update: LootUpdate) -> Option<String> {
        self.changed();
        let window = self.loot.as_mut()?;
        match update {
            LootUpdate::Opened { response, coins } => {
                let refusal = match response {
                    LootResponse::Normal => None,
                    LootResponse::SomeoneElse => Some("Someone else is looting that corpse."),
                    LootResponse::NotAtThisTime => {
                        Some("You cannot loot that corpse at this time.")
                    }
                    LootResponse::Hostiles => Some("You cannot loot while a hostile is nearby."),
                    LootResponse::TooFar => Some("You are too far away to loot that corpse."),
                    LootResponse::Other(_) => Some("You cannot loot that corpse."),
                };
                if let Some(refusal) = refusal {
                    self.loot = None;
                    return Some(refusal.into());
                }
                self.adjust_coins(i64::try_from(coins.total_copper()).unwrap_or(0));
                (coins.total_copper() > 0).then(|| {
                    format!(
                        "You receive {} from the corpse.",
                        coin_text(coins.total_copper())
                    )
                })
            }
            LootUpdate::Item(item) => {
                if let Ok(slot) = u16::try_from(item.slot.0) {
                    window.items.insert(slot, *item);
                }
                None
            }
            LootUpdate::Listed { corpse_id } => {
                if corpse_id == window.corpse_id {
                    window.listed = true;
                }
                None
            }
            LootUpdate::Taken { slot, accepted } => {
                if window.pending == Some(slot) {
                    window.pending = None;
                }
                if accepted {
                    window.items.remove(&slot);
                    None
                } else {
                    window.loot_all = false;
                    Some("You cannot take that item.".into())
                }
            }
            LootUpdate::Closed => {
                self.loot = None;
                None
            }
        }
    }

    /// Applies one merchant update and returns chat feedback.
    pub(super) fn apply_merchant(&mut self, update: MerchantUpdate) -> Option<String> {
        self.changed();
        let window = self.merchant.as_mut()?;
        match update {
            MerchantUpdate::Opened { accepted, .. } => {
                if !accepted {
                    self.merchant = None;
                    return Some("That merchant will not trade with you.".into());
                }
            }
            MerchantUpdate::Item(entry) => {
                window.stock.insert(entry.slot, *entry);
            }
            MerchantUpdate::Removed { slot } => {
                window.stock.remove(&slot);
            }
            MerchantUpdate::Closed => self.merchant = None,
            MerchantUpdate::Bought { price, .. } => self.adjust_coins(-i64::from(price)),
            // The session removes the sold units from the inventory, and a server
            // money update follows.
            MerchantUpdate::Sold { .. } => (),
        }
        None
    }
}

/// Fills both windows with synthetic items for an explicitly offline preview.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn demo(
    settings: Res<super::ViewerSettings>,
    online: Res<super::online::OnlineState>,
    mut trade: ResMut<TradeState>,
) {
    if !settings.0.demo_trade || online.enabled {
        return;
    }
    let items = super::inventory::demo_items();
    trade.coins = Some(Coins {
        platinum: 3,
        gold: 1,
        silver: 4,
        copper: 7,
    });
    trade.loot = Some(LootWindow {
        corpse_id: 1,
        name: "a preview rat".into(),
        items: items
            .iter()
            .take(3)
            .zip(22u16..)
            .map(|(item, slot)| (slot, item.clone()))
            .collect(),
        listed: true,
        pending: None,
        loot_all: false,
    });
    trade.merchant = Some(MerchantWindow {
        merchant_id: 2,
        name: "Preview merchant".into(),
        stock: items
            .into_iter()
            .skip(3)
            .zip(1u32..)
            .map(|(item, slot)| {
                (
                    slot,
                    MerchantItem {
                        slot,
                        price: slot * 137,
                        quantity: 0,
                        item,
                    },
                )
            })
            .collect(),
    });
    trade.changed();
}

/// `1p 2g 3s 4c`, omitting empty denominations.
pub(super) fn coin_text(copper: u64) -> String {
    let parts: Vec<String> = [
        (copper / 1000, "p"),
        (copper / 100 % 10, "g"),
        (copper / 10 % 10, "s"),
        (copper % 10, "c"),
    ]
    .iter()
    .filter(|(amount, _)| *amount > 0)
    .map(|(amount, unit)| format!("{amount}{unit}"))
    .collect();
    if parts.is_empty() {
        "0c".into()
    } else {
        parts.join(" ")
    }
}

/// A loot or merchant window, by the list it shows.
#[derive(Component)]
pub(super) struct Panel(Rows);

/// The scrolling item list inside the loot or merchant window.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Rows {
    Loot,
    Merchant,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Take(u16),
    TakeAll,
    EndLoot,
    Buy(u32),
    Sell(i32),
    EndShop,
}

/// Opens, drives and closes sessions from keys and window buttons.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    online: Res<super::online::OnlineState>,
    sender: Res<super::target::CommandsToServer>,
    mut trade: ResMut<TradeState>,
    mut chat: ResMut<super::chat::ChatState>,
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    escape: Res<super::escape::Escape>,
) {
    trade.reset(online.world.session_id());
    let (Some(session_id), Some(player), Some(sender)) = (
        online.world.session_id(),
        online.world.player(),
        sender.0.as_ref(),
    ) else {
        return;
    };
    if !online.world.connected() || online.world.death().is_some() {
        return;
    }
    let now = std::time::Instant::now();
    let own_id = player.spawn_id;
    let send = |command: ClientCommand| sender.try_send(command).is_ok();
    let focused = !chat.composing && windows.single().is_ok_and(|window| window.focused);
    let targeted = online.world.target().selected.and_then(|id| {
        online
            .world
            .spawn(id)
            .map(|spawn| &spawn.state)
            .map(|spawn| (id, spawn))
    });
    let mut clicked: Vec<Action> = buttons
        .iter()
        .filter(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, action)| *action)
        .collect();
    match *escape {
        super::escape::Escape::Loot => clicked.push(Action::EndLoot),
        super::escape::Escape::Shop => clicked.push(Action::EndShop),
        _ => (),
    }
    if focused && keys.just_pressed(KeyCode::KeyL) {
        if trade.loot.is_some() {
            clicked.push(Action::EndLoot);
        } else if let Some((corpse_id, spawn)) = targeted.filter(|(_, spawn)| {
            matches!(spawn.kind, SpawnKind::NpcCorpse | SpawnKind::PlayerCorpse)
        }) {
            if send(ClientCommand::Loot {
                session_id,
                corpse_id,
                created: now,
            }) {
                trade.loot = Some(LootWindow {
                    corpse_id,
                    name: super::combat::display_name(&spawn.name),
                    items: BTreeMap::new(),
                    listed: false,
                    pending: None,
                    loot_all: false,
                });
                trade.changed();
            }
        } else {
            chat.history.push(super::chat::system_line(
                "Target a corpse to loot it.".into(),
            ));
        }
    }
    if focused && keys.just_pressed(KeyCode::KeyU) {
        if trade.merchant.is_some() {
            clicked.push(Action::EndShop);
        } else if let Some((merchant_id, spawn)) = targeted.filter(|(_, spawn)| {
            spawn.kind == SpawnKind::Npc
                && !spawn.invisible
                && spawn.class.is_none_or(|class| class == MERCHANT_CLASS)
        }) {
            if send(ClientCommand::Shop {
                session_id,
                merchant_id,
                own_id,
                open: true,
                created: now,
            }) {
                trade.merchant = Some(MerchantWindow {
                    merchant_id,
                    name: super::combat::display_name(&spawn.name),
                    stock: BTreeMap::new(),
                });
                trade.changed();
            }
        } else {
            chat.history.push(super::chat::system_line(
                "Target a merchant to trade with it.".into(),
            ));
        }
    }
    for action in clicked {
        match action {
            Action::Take(slot) => {
                if let Some(window) = trade
                    .loot
                    .as_mut()
                    .filter(|window| window.pending.is_none())
                    && send(ClientCommand::LootItem {
                        session_id,
                        corpse_id: window.corpse_id,
                        own_id,
                        slot,
                        auto: true,
                        created: now,
                    })
                {
                    window.pending = Some(slot);
                }
            }
            Action::TakeAll => {
                if let Some(window) = trade.loot.as_mut() {
                    window.loot_all = true;
                }
            }
            Action::EndLoot => {
                if let Some(corpse_id) = trade.loot.as_ref().map(|window| window.corpse_id)
                    && send(ClientCommand::EndLoot {
                        session_id,
                        corpse_id,
                    })
                {
                    trade.loot = None;
                    trade.changed();
                }
            }
            Action::Buy(slot) => {
                if let Some(window) = &trade.merchant {
                    send(ClientCommand::Buy {
                        session_id,
                        merchant_id: window.merchant_id,
                        own_id,
                        slot,
                        quantity: 1,
                        created: now,
                    });
                }
            }
            Action::Sell(slot) => {
                // The row may be older than the inventory: sell only what the
                // slot holds now.
                let Some(item) = online
                    .world
                    .inventory()
                    .items()
                    .get(&eq_client_core::inventory::InventorySlot(slot))
                else {
                    continue;
                };
                if no_drop(item) {
                    chat.history.push(super::chat::system_line(
                        "The merchant will not buy NO DROP items.".into(),
                    ));
                    continue;
                }
                let quantity = item.stack_count.unwrap_or(1).max(1);
                if let Some(window) = &trade.merchant {
                    send(ClientCommand::Sell {
                        session_id,
                        merchant_id: window.merchant_id,
                        slot,
                        quantity,
                        created: now,
                    });
                }
            }
            Action::EndShop => {
                if let Some(merchant_id) = trade.merchant.as_ref().map(|window| window.merchant_id)
                    && send(ClientCommand::Shop {
                        session_id,
                        merchant_id,
                        own_id,
                        open: false,
                        created: now,
                    })
                {
                    trade.merchant = None;
                    trade.changed();
                }
            }
        }
    }
    // Loot all takes one item at a time, waiting for each acknowledgement.
    let next = trade
        .loot
        .as_ref()
        .filter(|window| window.loot_all && window.listed && window.pending.is_none())
        .map(|window| (window.corpse_id, window.items.keys().next().copied()));
    match next {
        Some((corpse_id, Some(slot))) => {
            if send(ClientCommand::LootItem {
                session_id,
                corpse_id,
                own_id,
                slot,
                auto: true,
                created: now,
            }) && let Some(window) = trade.loot.as_mut()
            {
                window.pending = Some(slot);
            }
        }
        Some((corpse_id, None))
            if send(ClientCommand::EndLoot {
                session_id,
                corpse_id,
            }) =>
        {
            trade.loot = None;
            trade.changed();
        }
        Some(_) | None => (),
    }
}

/// Rebuilds the loot and merchant windows' contents whenever they change, keeping
/// each list's scroll offset so a sale does not jump back to the top. A window
/// stays the same window while it is open, so one being dragged keeps
/// following the pointer as its rows change.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn present(
    mut commands: Commands,
    trade: Res<TradeState>,
    online: Res<super::online::OnlineState>,
    panels: Query<(Entity, &Panel)>,
    lists: Query<(&Rows, &ScrollPosition)>,
    mut shown: Local<Option<(u64, u64)>>,
) {
    let inventory = online.world.inventory();
    let signature = (trade.revision, inventory.revision());
    if *shown == Some(signature) {
        return;
    }
    *shown = Some(signature);
    let offset = |kind: Rows| {
        lists
            .iter()
            .find(|(list, _)| **list == kind)
            .map_or(0.0, |(_, position)| position.y)
    };
    let (loot_offset, merchant_offset) = (offset(Rows::Loot), offset(Rows::Merchant));
    let mut open: Vec<(Rows, Entity)> = panels
        .iter()
        .map(|(entity, panel)| (panel.0, entity))
        .collect();
    let mut frame = |kind: Rows| {
        open.iter()
            .position(|(shown, _)| *shown == kind)
            .map(|index| open.swap_remove(index).1)
    };
    let (loot_frame, merchant_frame) = (
        trade.loot.as_ref().and_then(|_| frame(Rows::Loot)),
        trade.merchant.as_ref().and_then(|_| frame(Rows::Merchant)),
    );
    // Windows that closed.
    for (_, entity) in open {
        commands.entity(entity).despawn();
    }
    if let Some(window) = &trade.loot {
        let rows: Vec<(Action, String)> = window
            .items
            .iter()
            .map(|(slot, item)| (Action::Take(*slot), item_label(item)))
            .collect();
        let status = if window.listed && rows.is_empty() {
            "Nothing left on this corpse"
        } else if window.listed {
            "Click an item to take it"
        } else {
            "Opening..."
        };
        show_panel(
            &mut commands,
            loot_frame,
            // Stable titles keep a dragged window in place for every corpse.
            "LOOT",
            (px(24), Val::Auto, px(110)),
            &format!("{}: {status}", window.name),
            (Rows::Loot, &rows, loot_offset),
            &[(Action::TakeAll, "Loot all"), (Action::EndLoot, "Done")],
        );
    }
    if let Some(window) = &trade.merchant {
        let mut rows: Vec<(Action, String)> = window
            .stock
            .values()
            .map(|entry| {
                (
                    Action::Buy(entry.slot),
                    format!(
                        "Buy  {}  {}",
                        item_label(&entry.item),
                        coin_text(u64::from(entry.price))
                    ),
                )
            })
            .collect();
        rows.extend(
            inventory
                .items()
                .values()
                .filter(|item| {
                    sellable_slot(item.slot.0)
                        && !no_drop(item)
                        && !holds_items(item, inventory.items())
                })
                .map(|item| {
                    (
                        Action::Sell(item.slot.0),
                        format!("Sell {}", item_label(item)),
                    )
                }),
        );
        let coins = trade
            .coins
            .map_or_else(|| "--".into(), |coins| coin_text(coins.total_copper()));
        show_panel(
            &mut commands,
            merchant_frame,
            "MERCHANT",
            (Val::Auto, px(24), px(96)),
            &format!("{}   Your coin: {coins}", window.name),
            (Rows::Merchant, &rows, merchant_offset),
            &[(Action::EndShop, "Done")],
        );
    }
}

/// Scrolls the loot or merchant list the wheel turns.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    wheel: Res<windows::pointer::Wheel>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<Rows>>,
) {
    if let Some((node, mut position)) = wheel.surface.and_then(|list| lists.get_mut(list).ok()) {
        windows::scroll_by(&mut position, node, wheel.pixels);
    }
}

/// Carried slots and their bag contents; equipment and the cursor are excluded.
fn sellable_slot(slot: i32) -> bool {
    (22..=29).contains(&slot) || (251..=330).contains(&slot)
}

/// A container with anything inside: selling it would sell its contents too, and
/// the official client asks for it to be emptied first.
fn holds_items(
    item: &InventoryItem,
    items: &std::collections::BTreeMap<eq_client_core::inventory::InventorySlot, InventoryItem>,
) -> bool {
    item.bag_slots > 0
        && (0..10)
            .filter_map(|index| item.slot.child(index))
            .any(|child| items.contains_key(&child))
}

/// Merchants silently ignore offers of NO DROP items, so they are never offered.
fn no_drop(item: &InventoryItem) -> bool {
    item.details.flags.iter().any(|flag| flag == "NO DROP")
}

fn item_label(item: &InventoryItem) -> String {
    match item.stack_count {
        Some(count) if count > 1 => format!("{} x{count}", item.details.name),
        _ => item.details.name.clone(),
    }
}

/// A window whose item list scrolls between a fixed status line and footer, so
/// the closing buttons stay reachable however long the list is. An open window
/// keeps its frame and only its contents are rebuilt.
fn show_panel(
    commands: &mut Commands,
    frame: Option<Entity>,
    title: &str,
    (left, right, top): (Val, Val, Val),
    status: &str,
    (list, rows, offset): (Rows, &[(Action, String)], f32),
    footer: &[(Action, &str)],
) {
    let frame = if let Some(frame) = frame {
        commands.entity(frame).despawn_children();
        frame
    } else {
        commands
            .spawn((
                Panel(list),
                windows::Frame::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left,
                    right,
                    top,
                    width: px(300),
                    padding: UiRect::all(px(6)),
                    row_gap: px(4),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                // Above inventory and spellbook, below item inspection.
                GlobalZIndex(26),
                BackgroundColor(Color::srgba(0.025, 0.032, 0.04, 0.94)),
            ))
            .id()
    };
    commands.entity(frame).with_children(|parent| {
        windows::title_bar(parent, frame, title);
        parent.spawn((
            Text::new(status),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::srgb(0.73, 0.77, 0.81)),
        ));
        parent
            .spawn((
                list,
                windows::pointer::TakesWheel,
                ScrollPosition(Vec2::new(0.0, offset)),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    max_height: px(320),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                for (action, label) in rows {
                    button(list, *action, label);
                }
            });
        parent
            .spawn(Node {
                column_gap: px(6),
                ..default()
            })
            .with_children(|row| {
                for (action, label) in footer {
                    button(row, *action, label);
                }
            });
    });
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, label: &str) {
    parent
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(6), px(3)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.14, 0.18)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.92, 0.94)),
            ));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loot_window_being_dragged_follows_the_pointer_while_its_rows_change() {
        use bevy::math::{Affine2, DVec2};
        let mut app = App::new();
        app.insert_resource(TradeState {
            loot: Some(LootWindow {
                corpse_id: 7,
                name: "Corpse".into(),
                items: BTreeMap::new(),
                listed: true,
                pending: None,
                loot_all: false,
            }),
            ..TradeState::default()
        })
        .insert_resource(super::super::online::OnlineState::new(false))
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<windows::DragState>()
        .add_systems(Update, (present, windows::input).chain());
        let mut window = Window {
            focused: true,
            resolution: bevy::window::WindowResolution::new(800, 600),
            ..default()
        };
        window.set_physical_cursor_position(Some(DVec2::new(100.0, 100.0)));
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        app.update();
        let panel = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<Panel>>()
                .single(app.world())
                .unwrap()
        };
        let frame = panel(&mut app);
        // Where the UI layout puts the window: 300 by 200 at (24, 110).
        app.world_mut().entity_mut(frame).insert((
            ComputedNode {
                size: Vec2::new(300.0, 200.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(174.0, 210.0))),
        ));
        let title = app
            .world_mut()
            .query_filtered::<Entity, With<windows::TitleBar>>()
            .single(app.world())
            .unwrap();
        *app.world_mut().get_mut::<Interaction>(title).unwrap() = Interaction::Pressed;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        // A loot reply changes the rows while the pointer moves on.
        app.world_mut().resource_mut::<TradeState>().changed();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(DVec2::new(140.0, 120.0)));
        app.update();
        assert_eq!(panel(&mut app), frame);
        let node = app.world().get::<Node>(frame).unwrap();
        assert_eq!((node.left, node.top), (px(64.0), px(130.0)));
    }

    #[test]
    fn one_wheel_turn_scrolls_only_the_list_on_top() {
        use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
        let mut app = App::new();
        app.add_message::<MouseWheel>()
            .init_resource::<windows::pointer::Wheel>()
            .add_systems(Update, (windows::pointer::wheel, scroll).chain());
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(bevy::math::DVec2::new(100.0, 100.0)));
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        // The merchant's list dragged over the loot list, both long enough to scroll.
        let list = |app: &mut App, rows: Rows, stack: u32| {
            app.world_mut()
                .spawn((
                    rows,
                    windows::pointer::TakesWheel,
                    ScrollPosition(Vec2::new(0.0, 50.0)),
                    ComputedNode {
                        size: Vec2::new(200.0, 200.0),
                        content_size: Vec2::new(200.0, 1000.0),
                        inverse_scale_factor: 1.0,
                        ..default()
                    },
                    bevy::ui::ComputedStackIndex(stack),
                    UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
                        100.0, 100.0,
                    ))),
                ))
                .id()
        };
        let (below, above) = (
            list(&mut app, Rows::Loot, 1),
            list(&mut app, Rows::Merchant, 2),
        );
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: -1.0,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        let offset = |entity| app.world().get::<ScrollPosition>(entity).unwrap().y;
        assert!(offset(above) > 50.0);
        assert!((offset(below) - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn containers_with_contents_are_not_offered_for_sale() {
        let items: BTreeMap<_, _> = super::super::inventory::demo_items()
            .into_iter()
            .map(|item| (item.slot, item))
            .collect();
        let by_slot = |slot| &items[&eq_client_core::inventory::InventorySlot(slot)];
        // The backpack (22) holds rations; the satchel (23) holds arrows.
        assert!(holds_items(by_slot(22), &items));
        assert!(!holds_items(by_slot(13), &items));
        let mut emptied = items.clone();
        emptied.retain(|slot, _| slot.0 != 261);
        assert!(!holds_items(by_slot(23), &emptied));
    }

    #[test]
    fn long_lists_scroll_inside_the_window_while_its_buttons_stay_outside() {
        let stock: BTreeMap<u32, MerchantItem> = super::super::inventory::demo_items()
            .into_iter()
            .cycle()
            .take(30)
            .zip(1u32..)
            .map(|(item, slot)| {
                (
                    slot,
                    MerchantItem {
                        slot,
                        price: 10,
                        quantity: 0,
                        item,
                    },
                )
            })
            .collect();
        let mut app = App::new();
        app.insert_resource(TradeState {
            merchant: Some(MerchantWindow {
                merchant_id: 8,
                name: "Merchant".into(),
                stock,
            }),
            ..TradeState::default()
        })
        .insert_resource(super::super::online::OnlineState::new(false))
        .add_systems(Update, present);
        app.update();
        let world = app.world_mut();
        let in_list = |world: &World, entity: Entity| {
            std::iter::successors(Some(entity), |entity| {
                world.get::<ChildOf>(*entity).map(ChildOf::parent)
            })
            .any(|entity| world.get::<Rows>(entity) == Some(&Rows::Merchant))
        };
        let buttons: Vec<(Entity, Action)> = world
            .query::<(Entity, &Action)>()
            .iter(world)
            .map(|(entity, action)| (entity, *action))
            .collect();
        assert_eq!(
            buttons
                .iter()
                .filter(
                    |(entity, action)| matches!(action, Action::Buy(_)) && in_list(world, *entity)
                )
                .count(),
            30
        );
        let done = buttons
            .iter()
            .find(|(_, action)| *action == Action::EndShop)
            .unwrap()
            .0;
        assert!(!in_list(world, done));
        // A rebuild, as after a sale, keeps the list where the player scrolled it.
        let mut lists = world.query_filtered::<&mut ScrollPosition, With<Rows>>();
        lists.single_mut(world).unwrap().y = 120.0;
        world.resource_mut::<TradeState>().changed();
        app.update();
        let world = app.world_mut();
        assert!(
            (world
                .query_filtered::<&ScrollPosition, With<Rows>>()
                .single(world)
                .unwrap()
                .y
                - 120.0)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn coins_format_by_denomination() {
        assert_eq!(coin_text(0), "0c");
        assert_eq!(coin_text(1234), "1p 2g 3s 4c");
        assert_eq!(coin_text(1005), "1p 5c");
    }

    #[test]
    fn loot_refusals_close_the_window_and_acknowledgements_remove_items() {
        let mut trade = TradeState {
            loot: Some(LootWindow {
                corpse_id: 9,
                name: "a rat".into(),
                items: BTreeMap::new(),
                listed: false,
                pending: Some(22),
                loot_all: false,
            }),
            ..TradeState::default()
        };
        assert_eq!(
            trade.apply_loot(LootUpdate::Opened {
                response: LootResponse::Normal,
                coins: Coins {
                    platinum: 0,
                    gold: 1,
                    silver: 0,
                    copper: 2,
                },
            }),
            Some("You receive 1g 2c from the corpse.".into())
        );
        assert_eq!(trade.apply_loot(LootUpdate::Listed { corpse_id: 9 }), None);
        assert!(trade.loot.as_ref().unwrap().listed);
        assert_eq!(
            trade.apply_loot(LootUpdate::Taken {
                slot: 22,
                accepted: false
            }),
            Some("You cannot take that item.".into())
        );
        assert_eq!(trade.loot.as_ref().unwrap().pending, None);
        assert_eq!(
            trade.apply_loot(LootUpdate::Opened {
                response: LootResponse::TooFar,
                coins: Coins::default(),
            }),
            Some("You are too far away to loot that corpse.".into())
        );
        assert!(trade.loot.is_none());
        assert_eq!(trade.apply_loot(LootUpdate::Closed), None);
    }

    #[test]
    fn loot_coins_and_purchases_adjust_the_carried_total() {
        let mut trade = TradeState {
            coins: Some(Coins {
                platinum: 1,
                gold: 0,
                silver: 0,
                copper: 0,
            }),
            loot: Some(LootWindow {
                corpse_id: 9,
                name: "a rat".into(),
                items: BTreeMap::new(),
                listed: false,
                pending: None,
                loot_all: false,
            }),
            merchant: Some(MerchantWindow {
                merchant_id: 8,
                name: "Merchant".into(),
                stock: BTreeMap::new(),
            }),
            ..TradeState::default()
        };
        trade.apply_loot(LootUpdate::Opened {
            response: LootResponse::Normal,
            coins: Coins {
                platinum: 0,
                gold: 0,
                silver: 1,
                copper: 5,
            },
        });
        assert_eq!(trade.coins.unwrap().total_copper(), 1015);
        trade.apply_merchant(MerchantUpdate::Bought {
            slot: 2,
            quantity: 1,
            price: 20,
        });
        assert_eq!(
            trade.coins.unwrap(),
            Coins {
                platinum: 0,
                gold: 9,
                silver: 9,
                copper: 5,
            }
        );
    }

    #[test]
    fn a_window_closes_only_once_the_server_is_told() {
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(
            &mut online,
            1,
            eq_client_core::PlayerState {
                name: "Example".into(),
                base_attributes: None,
                spawn_id: 7,
                race: 1,
                gender: 0,
                class: Some(2),
                deity: None,
                level: 1,
                position: eq_client_core::WorldPosition::default(),
                mana: 0,
                endurance: None,
                skills: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: Some(100),
                appearance: eq_client_core::outfit::Appearance::default(),
            },
        );
        // A command queue that can no longer take anything.
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        drop(receiver);
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(TradeState {
                session: Some(1),
                loot: Some(LootWindow {
                    corpse_id: 9,
                    name: "a rat".into(),
                    items: BTreeMap::new(),
                    listed: true,
                    pending: None,
                    loot_all: false,
                }),
                ..TradeState::default()
            })
            .insert_resource(super::super::target::CommandsToServer(Some(sender)))
            .init_resource::<super::super::target::TargetState>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::escape::Escape>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.world_mut()
            .spawn((Button, Action::EndLoot, Interaction::Pressed));
        app.update();
        assert!(app.world().resource::<TradeState>().loot.is_some());
    }

    #[test]
    fn refused_or_closed_merchants_close_the_window() {
        let mut trade = TradeState {
            merchant: Some(MerchantWindow {
                merchant_id: 9,
                name: "Merchant".into(),
                stock: BTreeMap::new(),
            }),
            ..TradeState::default()
        };
        assert_eq!(
            trade.apply_merchant(MerchantUpdate::Opened {
                merchant_id: 9,
                accepted: false,
                rate: 1.0
            }),
            Some("That merchant will not trade with you.".into())
        );
        assert!(trade.merchant.is_none());
        assert!(
            sellable_slot(22) && sellable_slot(300) && !sellable_slot(30) && !sellable_slot(13)
        );
    }
}

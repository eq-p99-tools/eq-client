//! Corpse loot and merchant windows; the server settles every transfer.
//!
//! L loots the targeted corpse, U trades with the targeted NPC, Escape closes both.
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::{
    ClientCommand, Coins, SpawnKind,
    inventory::InventoryItem,
    loot::{LootResponse, LootUpdate},
    merchant::{MerchantItem, MerchantUpdate},
};
use std::collections::BTreeMap;

use super::windows;

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
            MerchantUpdate::Bought { .. } | MerchantUpdate::Sold { .. } => (),
        }
        None
    }
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

#[derive(Component)]
pub(super) struct Panel;
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
    target: Res<super::target::TargetState>,
    sender: Res<super::target::CommandsToServer>,
    inventory: Res<super::inventory::InventoryState>,
    mut trade: ResMut<TradeState>,
    mut chat: ResMut<super::chat::ChatState>,
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    trade.reset(online.session_id);
    let (Some(session_id), Some(player), Some(sender)) =
        (online.session_id, online.player.as_ref(), sender.0.as_ref())
    else {
        return;
    };
    if !online.connected || online.death.is_some() {
        return;
    }
    let now = std::time::Instant::now();
    let own_id = player.spawn_id;
    let send = |command: ClientCommand| sender.try_send(command).is_ok();
    let focused = !chat.composing && windows.single().is_ok_and(|window| window.focused);
    let targeted = target
        .selected
        .and_then(|id| online.spawns.get(&id).map(|spawn| (id, spawn)));
    let mut clicked: Vec<Action> = buttons
        .iter()
        .filter(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, action)| *action)
        .collect();
    if focused && keys.just_pressed(KeyCode::Escape) {
        clicked.extend([Action::EndLoot, Action::EndShop]);
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
        } else if let Some((merchant_id, spawn)) =
            targeted.filter(|(_, spawn)| spawn.kind == SpawnKind::Npc && !spawn.invisible)
        {
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
                if let Some(window) = trade.loot.take() {
                    send(ClientCommand::EndLoot {
                        session_id,
                        corpse_id: window.corpse_id,
                    });
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
                let quantity = inventory
                    .data
                    .items()
                    .get(&eq_client_core::inventory::InventorySlot(slot))
                    .map_or(1, |item| item.stack_count.unwrap_or(1).max(1));
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
                if let Some(window) = trade.merchant.take() {
                    send(ClientCommand::Shop {
                        session_id,
                        merchant_id: window.merchant_id,
                        own_id,
                        open: false,
                        created: now,
                    });
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
        Some((corpse_id, None)) => {
            send(ClientCommand::EndLoot {
                session_id,
                corpse_id,
            });
            trade.loot = None;
            trade.changed();
        }
        None => (),
    }
}

/// Rebuilds the loot and merchant windows whenever their contents change.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn present(
    mut commands: Commands,
    trade: Res<TradeState>,
    inventory: Res<super::inventory::InventoryState>,
    panels: Query<Entity, With<Panel>>,
    mut shown: Local<Option<(u64, u64)>>,
) {
    let signature = (trade.revision, inventory.data.revision());
    if *shown == Some(signature) {
        return;
    }
    *shown = Some(signature);
    for panel in &panels {
        commands.entity(panel).despawn();
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
        spawn_panel(
            &mut commands,
            &format!("LOOT {}", window.name.to_uppercase()),
            (px(20), px(160)),
            status,
            &rows,
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
                .data
                .items()
                .values()
                .filter(|item| sellable_slot(item.slot.0))
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
        spawn_panel(
            &mut commands,
            &format!("MERCHANT {}", window.name.to_uppercase()),
            (px(20), px(420)),
            &format!("Your coin: {coins}"),
            &rows,
            &[(Action::EndShop, "Done")],
        );
    }
}

/// Carried slots and their bag contents; equipment and the cursor are excluded.
fn sellable_slot(slot: i32) -> bool {
    (22..=29).contains(&slot) || (251..=330).contains(&slot)
}

fn item_label(item: &InventoryItem) -> String {
    match item.stack_count {
        Some(count) if count > 1 => format!("{} x{count}", item.details.name),
        _ => item.details.name.clone(),
    }
}

fn spawn_panel(
    commands: &mut Commands,
    title: &str,
    (left, top): (Val, Val),
    status: &str,
    rows: &[(Action, String)],
    footer: &[(Action, &str)],
) {
    let frame = commands
        .spawn((
            Panel,
            windows::Frame::default(),
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                width: px(300),
                max_height: px(360),
                padding: UiRect::all(px(6)),
                row_gap: px(4),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip_y(),
                ..default()
            },
            GlobalZIndex(18),
            BackgroundColor(Color::srgba(0.025, 0.032, 0.04, 0.94)),
        ))
        .id();
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
        for (action, label) in rows {
            button(parent, *action, label);
        }
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

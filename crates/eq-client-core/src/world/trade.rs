//! Looting a corpse, trading with a merchant and handing items to another
//! character: what the player opened, and what the server listed in it.
use super::Notice;
use crate::{
    Coins,
    exchange::ExchangeUpdate,
    inventory::InventoryItem,
    loot::{LootResponse, LootUpdate},
    merchant::{MerchantItem, MerchantUpdate},
};
use std::collections::BTreeMap;

/// A corpse the player is looting.
#[derive(Clone, Debug, PartialEq)]
pub struct Loot {
    /// The corpse.
    pub corpse_id: u16,
    /// Its items, by corpse slot.
    pub items: BTreeMap<u16, InventoryItem>,
    /// Whether the server has listed every item.
    pub listed: bool,
}

/// A merchant the player is trading with.
#[derive(Clone, Debug, PartialEq)]
pub struct Merchant {
    /// The merchant.
    pub merchant_id: u16,
    /// What they sell, by list slot.
    pub stock: BTreeMap<u32, MerchantItem>,
}

/// A give window the player asked for, or has open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exchange {
    /// The character on the other side.
    pub with: u16,
    /// How many trade slots the window has.
    pub slots: u8,
    /// Whether their answer has opened the window.
    pub open: bool,
    /// Whether the player clicked Give.
    pub given: bool,
}

impl Exchange {
    /// How many trade slots the player may fill now: none until the window
    /// opens.
    #[must_use]
    pub const fn trade_slots(&self) -> u8 {
        if self.open { self.slots } else { 0 }
    }
}

/// The loot, merchant and give windows the player opened, as the server
/// fills them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Trade {
    pub loot: Option<Loot>,
    pub merchant: Option<Merchant>,
    pub exchange: Option<Exchange>,
}

impl Trade {
    /// Follows the server's word on the corpse being looted, returning the
    /// coins it handed over. False when no corpse is open.
    pub(super) fn loot(&mut self, update: &LootUpdate, coins: &mut Option<Coins>) -> bool {
        let Some(loot) = self.loot.as_mut() else {
            return false;
        };
        match update {
            LootUpdate::Opened {
                response,
                coins: taken,
            } => {
                if matches!(response, LootResponse::Normal) {
                    adjust(coins, i64::try_from(taken.total_copper()).unwrap_or(0));
                } else {
                    self.loot = None;
                }
            }
            LootUpdate::Item(item) => {
                if let Ok(slot) = u16::try_from(item.slot.0) {
                    loot.items.insert(slot, (**item).clone());
                }
            }
            LootUpdate::Listed { corpse_id } => {
                if *corpse_id == loot.corpse_id {
                    loot.listed = true;
                }
            }
            LootUpdate::Taken { slot, accepted } => {
                if *accepted {
                    loot.items.remove(slot);
                }
            }
            LootUpdate::Closed => self.loot = None,
        }
        true
    }

    /// Follows the server's word on the merchant open. False when none is.
    pub(super) fn merchant(&mut self, update: &MerchantUpdate, coins: &mut Option<Coins>) -> bool {
        let Some(merchant) = self.merchant.as_mut() else {
            return false;
        };
        match update {
            MerchantUpdate::Opened { accepted, .. } => {
                if !accepted {
                    self.merchant = None;
                }
            }
            MerchantUpdate::Item(entry) => {
                merchant.stock.insert(entry.slot, (**entry).clone());
            }
            MerchantUpdate::Removed { slot } => {
                merchant.stock.remove(slot);
            }
            MerchantUpdate::Closed => self.merchant = None,
            MerchantUpdate::Bought { price, .. } => adjust(coins, -i64::from(*price)),
            // The session removes the sold units from the inventory, and a
            // server money update follows.
            MerchantUpdate::Sold { .. } => (),
        }
        true
    }
}

impl Trade {
    /// Follows the server's word on a give window. False when the news is
    /// not about the one the player asked for.
    pub(super) fn exchange(&mut self, update: ExchangeUpdate) -> bool {
        let Some(exchange) = self.exchange.as_mut() else {
            return false;
        };
        match update {
            ExchangeUpdate::Opened { with } if u32::from(exchange.with) == with => {
                exchange.open = true;
            }
            ExchangeUpdate::Finished | ExchangeUpdate::Cancelled { .. } => self.exchange = None,
            ExchangeUpdate::Busy { by } if u32::from(exchange.with) == by => self.exchange = None,
            _ => return false,
        }
        true
    }
}

/// What an applied loot reply tells the player.
pub(super) fn loot_notice(update: &LootUpdate) -> Option<Notice> {
    match update {
        LootUpdate::Opened {
            response: LootResponse::Normal,
            coins,
        } => (coins.total_copper() > 0).then_some(Notice::LootCoins(*coins)),
        LootUpdate::Opened { response, .. } => Some(Notice::LootRefused(*response)),
        LootUpdate::Taken {
            accepted: false, ..
        } => Some(Notice::ItemRefused),
        _ => None,
    }
}

/// What an applied merchant reply tells the player.
pub(super) fn merchant_notice(update: &MerchantUpdate) -> Option<Notice> {
    matches!(
        update,
        MerchantUpdate::Opened {
            accepted: false,
            ..
        }
    )
    .then_some(Notice::ShopRefused)
}

/// Like the Titanium client, applies coin changes the server reports without
/// a money update (loot coins, purchase prices). Only the total is exact; the
/// next money update restores the true denominations.
fn adjust(coins: &mut Option<Coins>, copper: i64) {
    if let Some(coins) = coins {
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

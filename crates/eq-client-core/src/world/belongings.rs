//! News about what the player owns and trades: the inventory and its
//! moves, item uses, coins, the corpse being looted, the merchant and the
//! give window.
use super::{Changes, ClientWorld, Notice, Reply, trade};
use crate::{
    exchange::ExchangeUpdate, inventory::InventoryUpdate, loot::LootUpdate,
    merchant::MerchantUpdate,
};

/// What the player has put in an open give or trade window.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Offering {
    items: Vec<(crate::inventory::InventorySlot, u32, Option<u32>)>,
    coins: crate::Coins,
}

impl ClientWorld {
    /// The inventory changed; what equipped items give may change with it.
    pub(super) fn inventory_news(&mut self, update: &InventoryUpdate, changes: &mut Changes) {
        self.inventory.apply(update.clone());
        changes.inventory = true;
        self.refresh_item_hp();
    }

    /// The server's word on an item use the player asked for.
    pub(super) fn item_used(
        &self,
        (session_id, request_id): (u64, u64),
        error: Option<&String>,
        changes: &mut Changes,
    ) {
        if self.accepts_reply(session_id) {
            changes.replies.push(Reply::ItemUse {
                session_id,
                request_id,
                error: error.cloned(),
            });
        } else {
            changes.ignored = true;
        }
    }

    /// News of the corpse being looted.
    pub(super) fn loot_news(&mut self, update: &LootUpdate, changes: &mut Changes) {
        if self.zone.trade.loot(update) {
            changes.trade = true;
            changes.notices.extend(trade::loot_notice(update));
            if let LootUpdate::Taken { slot, accepted } = update {
                changes.replies.push(Reply::LootTaken {
                    slot: *slot,
                    accepted: *accepted,
                });
            }
        } else {
            changes.ignored = true;
        }
    }

    /// News of the merchant's stock and the player's trades with them.
    pub(super) fn merchant_news(&mut self, update: &MerchantUpdate, changes: &mut Changes) {
        if self.zone.trade.merchant(update) {
            changes.trade = true;
            changes.notices.extend(trade::merchant_notice(update));
        } else {
            changes.ignored = true;
        }
    }

    /// News of the give window the player asked for.
    pub(super) fn exchange_news(&mut self, update: &ExchangeUpdate, changes: &mut Changes) {
        if self.zone.trade.exchange(update) {
            changes.trade = true;
            if let ExchangeUpdate::Busy { .. } = update {
                changes
                    .notices
                    .push(Notice::GiveRefused("They are busy".into()));
            }
        } else {
            changes.ignored = true;
        }
    }

    /// What the player has put in the open window: the items in its slots
    /// and the coins given.
    pub(super) fn offering(&self) -> Offering {
        if self.zone.trade.exchange.is_none() {
            return Offering::default();
        }
        Offering {
            items: self
                .inventory
                .items()
                .iter()
                .filter(|(slot, _)| slot.is_in_trade())
                .map(|(slot, item)| (*slot, item.details.id, item.stack_count))
                .collect(),
            coins: self.wallet.given,
        }
    }

    /// Anything the player put in undoes both sides' Give or Trade clicks,
    /// as servers undo them (`EQEmu`'s `Trade::AddEntity` and coin moves).
    pub(super) fn reconsider(&mut self, before: &Offering) {
        if *before != self.offering()
            && let Some(exchange) = self.zone.trade.exchange.as_mut()
        {
            exchange.undo_clicks();
        }
    }

    /// The request to trade was not sent, or nobody answered it.
    pub(super) fn exchange_refused(
        &mut self,
        session_id: u64,
        reason: &str,
        changes: &mut Changes,
    ) {
        if self.session_id == Some(session_id) {
            self.zone.trade.exchange = None;
            changes.trade = true;
            changes.notices.push(Notice::GiveRefused(reason.to_owned()));
        } else {
            changes.ignored = true;
        }
    }

    /// A coin move was not sent; the coins stayed where they were.
    pub(super) fn coins_refused(&self, session_id: u64, reason: &str, changes: &mut Changes) {
        if self.session_id == Some(session_id) {
            changes
                .notices
                .push(Notice::TradeRefused(reason.to_owned()));
        } else {
            changes.ignored = true;
        }
    }

    /// The merchant would not trade.
    pub(super) fn merchant_refused(&self, session_id: u64, reason: &str, changes: &mut Changes) {
        if self.session_id == Some(session_id) {
            changes
                .notices
                .push(Notice::TradeRefused(reason.to_owned()));
        } else {
            changes.ignored = true;
        }
    }
}

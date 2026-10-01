//! News about what the player owns and trades: the inventory and its
//! moves, item uses, coins, the corpse being looted, the merchant and the
//! give window.
use super::{Changes, ClientWorld, Notice, Reply, trade};
use crate::{
    exchange::ExchangeUpdate, inventory::InventoryUpdate, loot::LootUpdate,
    merchant::MerchantUpdate,
};

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
        if self.zone.trade.loot(update, &mut self.coins) {
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
        if self.zone.trade.merchant(update, &mut self.coins) {
            changes.trade = true;
            changes.notices.extend(trade::merchant_notice(update));
        } else {
            changes.ignored = true;
        }
    }

    /// News of the give window the player asked for.
    pub(super) fn exchange_news(&mut self, update: ExchangeUpdate, changes: &mut Changes) {
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

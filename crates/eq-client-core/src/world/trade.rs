//! Looting a corpse, trading with a merchant and handing items to another
//! character: what the player opened, and what the server listed in it.
use super::Notice;
use crate::{
    exchange::{ExchangeUpdate, Partner},
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

/// Who asked for a give or trade window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// The player: what they hold on the cursor goes in when the window
    /// opens.
    Player,
    /// Another player, whose request the session took.
    Partner,
}

/// A give or trade window the player asked for, or has open.
#[derive(Clone, Debug, PartialEq)]
pub struct Exchange {
    /// The character on the other side.
    pub with: u16,
    /// Who they are, which decides the window.
    pub partner: Partner,
    /// How many trade slots the window has.
    pub slots: u8,
    /// Whether their answer has opened the window.
    pub open: bool,
    /// Who asked for it.
    pub asker: Asker,
    /// Whether the player clicked Give or Trade since anything put in last
    /// undid it.
    pub given: bool,
    /// Whether the other player clicked Trade since anything put in last
    /// undid it.
    pub partner_accepted: bool,
    /// What the other player put in, by their trade slot (0 to 7).
    pub theirs: BTreeMap<u8, InventoryItem>,
}

impl Exchange {
    /// A window the player asked for, before the answer.
    #[must_use]
    pub const fn asked(with: u16, partner: Partner) -> Self {
        Self {
            with,
            partner,
            slots: partner.slots(),
            open: false,
            asker: Asker::Player,
            given: false,
            partner_accepted: false,
            theirs: BTreeMap::new(),
        }
    }

    /// How many trade slots the player may fill now: none until the window
    /// opens.
    #[must_use]
    pub const fn trade_slots(&self) -> u8 {
        if self.open { self.slots } else { 0 }
    }

    /// Anything put in undoes both sides' clicks, as servers undo them.
    pub(super) const fn undo_clicks(&mut self) {
        self.given = false;
        self.partner_accepted = false;
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
    /// Follows the server's word on the corpse being looted; the session
    /// adds the coins it handed over to the purse. False when no corpse is
    /// open.
    pub(super) fn loot(&mut self, update: &LootUpdate) -> bool {
        let Some(loot) = self.loot.as_mut() else {
            return false;
        };
        match update {
            LootUpdate::Opened { response, .. } => {
                if !matches!(response, LootResponse::Normal) {
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

    /// Follows the server's word on the merchant open; the session takes a
    /// purchase's price from the purse. False when none is.
    pub(super) fn merchant(&mut self, update: &MerchantUpdate) -> bool {
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
            // The session moves the purse and the inventory.
            MerchantUpdate::Bought { .. } | MerchantUpdate::Sold { .. } => (),
        }
        true
    }
}

impl Trade {
    /// Follows the server's word on a give or trade window, and the
    /// session's on another player's request it took. False when the news
    /// is not about the window the player has.
    pub(super) fn exchange(&mut self, update: &ExchangeUpdate) -> bool {
        if let ExchangeUpdate::Taken { from } = update {
            let Ok(with) = u16::try_from(*from) else {
                return false;
            };
            self.exchange = Some(Exchange {
                open: true,
                asker: Asker::Partner,
                ..Exchange::asked(with, Partner::Player)
            });
            return true;
        }
        let Some(exchange) = self.exchange.as_mut() else {
            return false;
        };
        match update {
            ExchangeUpdate::Opened { with } if u32::from(exchange.with) == *with => {
                exchange.open = true;
            }
            ExchangeUpdate::Offered { index, item } => {
                exchange.theirs.insert(*index, (**item).clone());
                exchange.undo_clicks();
            }
            // The coins themselves are the session's to count.
            ExchangeUpdate::Coins { .. } => exchange.undo_clicks(),
            ExchangeUpdate::Accepted { by } if u32::from(exchange.with) == *by => {
                exchange.partner_accepted = true;
            }
            ExchangeUpdate::Finished | ExchangeUpdate::Cancelled { .. } => self.exchange = None,
            ExchangeUpdate::Busy { by } if u32::from(exchange.with) == *by => self.exchange = None,
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

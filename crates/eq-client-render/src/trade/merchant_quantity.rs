//! Merchant quantity intent. Holdings and transaction outcomes belong to the session.
use super::{Chosen, TradeState};
use crate::outbox::{Outbox, Stamp};
use eq_client_core::{ClientCommand, WorldEvent, WorldUpdate, world::ClientWorld};

/// A picker is valid only for the selection and replicas it opened against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Selection {
    session: u64,
    merchant: u16,
    catalog: u64,
    inventory: u64,
    choice_revision: u64,
    pub(super) chosen: Chosen,
}

#[cfg(test)]
mod tests;

impl Selection {
    pub(super) fn same_shop(self, world: &ClientWorld) -> bool {
        world.session_id() == Some(self.session)
            && world
                .merchant()
                .is_some_and(|open| open.merchant_id == self.merchant)
    }
    pub(crate) fn current(self, trade: &TradeState, world: &ClientWorld) -> bool {
        world.in_world()
            && world.session_id() == Some(self.session)
            && world
                .merchant()
                .is_some_and(|open| open.merchant_id == self.merchant)
            && world.merchant_revision() == self.catalog
            && world.inventory().revision() == self.inventory
            && trade.revision == self.choice_revision
            && trade.chosen() == Some(self.chosen)
    }
}

/// Local button exclusion, never a prediction of money or item delivery.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pending {
    session: u64,
    merchant: u16,
    chosen: Chosen,
    quantity: u32,
    unanswered: bool,
}

impl TradeState {
    pub(crate) fn merchant_status(&self) -> Option<&'static str> {
        self.merchant_pending.map(|pending| {
            if pending.unanswered {
                "Outcome unknown"
            } else {
                "Waiting for merchant"
            }
        })
    }

    /// Display a quote for the explicitly selected count, without changing
    /// holdings. An unread sale quote remains unknown.
    pub(crate) fn quantity_quote(&self, world: &ClientWorld, quantity: u32) -> Option<u32> {
        match self.chosen()? {
            Chosen::Ware(slot) => self
                .ware_price(world, world.merchant()?.stock.get(&slot)?.price)
                .checked_mul(quantity),
            Chosen::Carried(slot) => {
                let item = world
                    .inventory()
                    .items()
                    .get(&eq_client_core::inventory::InventorySlot(slot))?;
                if let Some(prices) = self.prices {
                    return u32::try_from(prices.sale_total(
                        item.details.price?,
                        world.merchant()?.rate,
                        quantity,
                    ))
                    .ok();
                }
                if !crate::outbox::offered(world, eq_client_core::Capability::MerchantOffers) {
                    return None;
                }
                let mut offered = item.clone();
                if offered.stack_count.is_some() {
                    offered.stack_count = Some(quantity);
                }
                world.merchant()?.offer(&offered)
            }
        }
    }

    pub(super) fn quantity_selection(&self, world: &ClientWorld) -> Option<Selection> {
        Some(Selection {
            session: world.session_id()?,
            merchant: world.merchant()?.merchant_id,
            catalog: world.merchant_revision(),
            inventory: world.inventory().revision(),
            choice_revision: self.revision,
            chosen: self.chosen()?,
        })
    }

    /// Use decoded bounds only. An unknown stack limit permits one, and a
    /// non-stackable (including charged) item is one item, not its charges.
    pub(super) fn quantity_limit(
        &self,
        world: &ClientWorld,
        chosen: Chosen,
    ) -> Result<u32, &'static str> {
        if self.merchant_pending.is_some() {
            return Err("Wait for the pending merchant result");
        }
        if !world.in_world() || world.merchant().is_none() || self.merchant.is_none() {
            return Err("Open the merchant again");
        }
        let inventory = world.inventory();
        if !inventory.received() || inventory.stale() || world.inventory_moves_pending() {
            return Err("Wait for current inventory before trading");
        }
        if inventory
            .items()
            .contains_key(&eq_client_core::inventory::InventorySlot::CURSOR)
        {
            return Err("Place the cursor item before trading");
        }
        let limit = match chosen {
            Chosen::Ware(slot) => {
                let open = world.merchant().ok_or("Open the merchant again")?;
                let ware = open
                    .stock
                    .get(&slot)
                    .ok_or("Choose a current merchant item")?;
                let mut limit = if ware.item.stack_count.is_some() {
                    ware.item.rules.stack_size.max(1)
                } else {
                    1
                };
                // Zero supplies no finite stock bound. Positive counts (also
                // large sentinel values) can only tighten the decoded stack bound.
                if ware.quantity > 0 {
                    limit = limit.min(ware.quantity);
                }
                let purse = world
                    .coins()
                    .ok_or("Wait for current coins before buying")?
                    .total_copper();
                let unit = self.ware_price(world, ware.price);
                if unit > 0 {
                    limit = limit.min(u32::try_from(purse / u64::from(unit)).unwrap_or(u32::MAX));
                    limit = limit.min(u32::MAX / unit);
                }
                limit
            }
            Chosen::Carried(slot) => {
                if !super::sellable_slot(slot) {
                    return Err("Choose a carried item to sell");
                }
                let item = inventory
                    .items()
                    .get(&eq_client_core::inventory::InventorySlot(slot))
                    .ok_or("Choose a current carried item")?;
                if super::no_drop(item) {
                    return Err("The merchant will not buy NO DROP items.");
                }
                if super::holds_items(item, inventory.items()) {
                    return Err("Empty the bag before selling it.");
                }
                item.stack_count.unwrap_or(1)
            }
        };
        if limit == 0 {
            Err("You do not have enough coins or items for that trade")
        } else {
            Ok(limit)
        }
    }

    /// Revalidate the exact selection immediately before its one submission.
    pub(super) fn submit_quantity(
        &mut self,
        world: &ClientWorld,
        outbox: &Outbox,
        stamp: Stamp,
        selection: Selection,
        quantity: u32,
    ) -> Result<(), &'static str> {
        if !selection.current(self, world) || selection.session != stamp.session_id {
            return Err("Merchant or inventory changed; choose the item again");
        }
        let limit = self.quantity_limit(world, selection.chosen)?;
        if quantity == 0 || quantity > limit {
            return Err("Choose a positive quantity within the current trade limit");
        }
        let own_id = world.player().ok_or("Enter the world first")?.spawn_id;
        let command = match selection.chosen {
            Chosen::Ware(slot) => ClientCommand::Buy {
                session_id: stamp.session_id,
                merchant_id: selection.merchant,
                own_id,
                slot,
                quantity,
                created: stamp.created,
            },
            Chosen::Carried(slot) => ClientCommand::Sell {
                session_id: stamp.session_id,
                merchant_id: selection.merchant,
                own_id,
                slot,
                quantity,
                created: stamp.created,
            },
        };
        if outbox.send(world, command).is_ok() {
            self.merchant_pending = Some(Pending {
                session: stamp.session_id,
                merchant: selection.merchant,
                chosen: selection.chosen,
                quantity,
                unanswered: false,
            });
            self.changed();
        }
        Ok(())
    }

    /// Called only for news accepted by the world. A stock refresh or unrelated
    /// reply cannot release a queued action. Uncertainty never retries it.
    pub(crate) fn merchant_news(&mut self, update: &WorldUpdate, world: &ClientWorld) {
        let Some(pending) = self.merchant_pending else {
            return;
        };
        if world.session_id() != Some(pending.session)
            || !world.in_world()
            || world
                .merchant()
                .is_some_and(|open| open.merchant_id != pending.merchant)
        {
            self.merchant_pending = None;
            self.changed();
            return;
        }
        let done = match update {
            WorldUpdate::Game(WorldEvent::Merchant(
                eq_client_core::merchant::MerchantUpdate::Closed,
            )) => true,
            WorldUpdate::Game(WorldEvent::Merchant(
                eq_client_core::merchant::MerchantUpdate::Bought { slot, quantity, .. },
            )) => {
                matches!(pending.chosen, Chosen::Ware(place) if (*quantity == 0 || place == *slot) && *quantity <= pending.quantity)
            }
            WorldUpdate::Game(WorldEvent::Merchant(
                eq_client_core::merchant::MerchantUpdate::Sold { slot, quantity, .. },
            )) => {
                matches!(pending.chosen, Chosen::Carried(place) if (*slot < 0 || place == *slot) && *quantity <= pending.quantity)
            }
            WorldUpdate::Game(WorldEvent::MerchantRefused { session_id, .. }) => {
                *session_id == pending.session
            }
            WorldUpdate::Game(WorldEvent::MerchantUnanswered { session_id, .. })
                if *session_id == pending.session =>
            {
                if let Some(pending) = self.merchant_pending.as_mut() {
                    pending.unanswered = true;
                }
                self.changed();
                false
            }
            _ => false,
        };
        if done {
            self.merchant_pending = None;
            self.merchant_choice = None;
            // A fresh explicit choice is required for the next transaction.
            if let Some(window) = self.merchant.as_mut() {
                window.chosen = None;
            }
            self.changed();
        }
    }
}

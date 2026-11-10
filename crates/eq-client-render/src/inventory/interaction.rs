//! Inventory selection and submission; UI intent never carries raw packet bytes.
use super::{InventorySlot, InventoryState};
use crate::{
    online::OnlineState,
    outbox::{Outbox, Stamp},
};
use eq_client_core::{
    ClientCommand,
    inventory::{Inventory, InventoryActor, InventoryMove, MoveQuantity},
    money::{Coin, CoinPlace, CoinTransfer},
};
use std::{num::NonZeroU32, time::Instant};

#[derive(Default)]
pub(super) struct Actions {
    pub last_use: Option<Instant>,
    pending_use: Option<(u64, u64)>,
    pub auto_store: bool,
    pub split: Option<SplitSelection>,
    pub pending: Option<(u64, u64, bool)>,
    pub message: String,
    /// Coins the quantity picker took, for the coins system to move.
    pub coins: Option<CoinTransfer>,
    /// Confirmed UI intent, revalidated and submitted by the merchant owner.
    pub merchant: Option<(crate::trade::MerchantSelection, u32)>,
}

/// What handing the cursor's item to an opening give window did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handed {
    /// Nothing was on the cursor.
    Nothing,
    /// It went onto a stack, which may leave the rest on the cursor.
    Stacked,
    /// It went into an empty slot, or the move was refused and said why.
    Placed,
    /// No slot could take it.
    NoRoom,
}

/// What a quantity picker takes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picked {
    /// A stack in this inventory slot.
    Stack(InventorySlot),
    /// Coins of one kind in a place.
    Coins(CoinPlace, Coin),
    Merchant(crate::trade::MerchantSelection),
}

pub(super) struct SplitSelection {
    pub picked: Picked,
    pub revision: u64,
    /// How many to take; none while a number typed in the quantity window
    /// has been erased.
    pub amount: u32,
    pub available: u32,
}

#[derive(bevy::prelude::Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitAction {
    Less,
    More,
    Minimum,
    Maximum,
    Confirm,
    Cancel,
}
impl InventoryState {
    pub(crate) fn select_merchant(
        &mut self,
        selection: crate::trade::MerchantSelection,
        available: u32,
    ) {
        self.actions.auto_store = false;
        self.actions.merchant = None;
        self.actions.split = Some(SplitSelection {
            picked: Picked::Merchant(selection),
            revision: 0,
            amount: 1,
            available,
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn picking_merchant(&self) -> bool {
        self.actions
            .split
            .as_ref()
            .is_some_and(|split| matches!(split.picked, Picked::Merchant(_)))
    }

    pub(crate) fn merchant_quantity(&self) -> Option<u32> {
        self.actions
            .split
            .as_ref()
            .and_then(|split| matches!(split.picked, Picked::Merchant(_)).then_some(split.amount))
    }

    pub(crate) fn cancel_merchant(&mut self) {
        if self.picking_merchant() || self.actions.merchant.is_some() {
            self.cancel_split();
        }
    }

    pub(crate) fn refresh_merchant(
        &mut self,
        trade: &crate::trade::TradeState,
        world: &eq_client_core::world::ClientWorld,
    ) {
        if self.actions.merchant.is_some_and(|(selection, _)| !selection.current(trade, world)) || self.actions.split.as_ref().is_some_and(|split| {
            matches!(split.picked, Picked::Merchant(selection) if !selection.current(trade, world))
        }) {
            self.cancel_split();
            self.refuse("Merchant or inventory changed; choose the item again".into());
        }
    }

    pub(crate) fn take_merchant(&mut self) -> Option<(crate::trade::MerchantSelection, u32)> {
        self.actions.merchant.take()
    }

    /// Refuses the player's action: the inventory's line says why, and so
    /// does the chat (`say_refusals`), as the official client says its
    /// refusals. Progress, such as a move under way, stays in the line.
    pub(super) fn refuse(&mut self, refusal: String) {
        self.actions.message.clone_from(&refusal);
        if !refusal.is_empty() {
            self.refused.push(refusal);
        }
    }

    /// Selects from the submitted replica, never a queued display projection.
    /// Only received server text opens a read window.
    pub(super) fn read_slot(&mut self, slot: InventorySlot, online: &OnlineState, sender: &Outbox) {
        let result = (|| -> anyhow::Result<()> {
            anyhow::ensure!(
                !online.world().inventory_moves_pending(),
                "Finish the queued item moves before reading"
            );
            let inventory = online.world().worker_inventory();
            anyhow::ensure!(
                inventory.received() && !inventory.stale(),
                "Wait for current inventory before reading"
            );
            let selection = eq_client_core::books::ReadSelection::capture(inventory, slot)?;
            sender.post(online.world(), |stamp| ClientCommand::ReadItem {
                session_id: stamp.session_id,
                revision: inventory.revision(),
                selection: Box::new(selection),
                created: stamp.created,
            })?;
            Ok(())
        })();
        if let Err(error) = result {
            self.refuse(crate::outbox::window_line(&error));
        }
    }

    /// Queues an explicit click effect without predicting charges or inventory changes.
    pub(super) fn use_slot(
        &mut self,
        slot: InventorySlot,
        online: &OnlineState,
        sender: &Outbox,
        target: Option<u16>,
        casting: bool,
        before: impl FnOnce() -> Vec<eq_client_core::chat::ChatLine>,
    ) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let inventory = online.world().inventory();
        let result = (|| -> anyhow::Result<()> {
            use anyhow::{Context, ensure};
            let stamp = sender.stamp(online.world())?;
            ensure!(
                !casting,
                "Wait for the current cast to finish or interrupt it"
            );
            ensure!(
                !online.world().inventory_moves_pending(),
                "Finish the pending inventory move first"
            );
            ensure!(
                !inventory.items().contains_key(&InventorySlot::CURSOR),
                "Place the cursor item before using an item"
            );
            let now = stamp.created;
            ensure!(
                self.actions.pending_use.is_none(),
                "Item use is already queued; wait for worker feedback"
            );
            ensure!(
                self.actions.last_use.is_none_or(
                    |last| now.duration_since(last) >= std::time::Duration::from_secs(1)
                ),
                "Wait briefly before using another item"
            );
            let player = online
                .world()
                .player()
                .context("Character data is unavailable")?;
            let session_id = stamp.session_id;
            let target_id = target.unwrap_or(player.spawn_id);
            let request = eq_client_core::inventory::ItemUse {
                request_id: self
                    .next_use_id
                    .checked_add(1)
                    .context("Item-use request IDs exhausted")?,
                session_id,
                revision: inventory.revision(),
                slot,
                target_id,
                created: now,
            };
            let available = target_id == player.spawn_id
                || online
                    .world()
                    .spawn(target_id)
                    .map(|spawn| &spawn.state)
                    .is_some_and(|spawn| online.world().can_see(spawn));
            // The Mac-era program drops invisibility before the item's own
            // checks, as for a gem's cast.
            self.said.extend(before());
            inventory.prepare_item_cast(&request, player.level, available)?;
            let request_id = request.request_id;
            sender.send(online.world(), ClientCommand::UseItem(request))?;
            self.actions.last_use = Some(now);
            self.next_use_id = request_id;
            self.actions.pending_use = Some((session_id, request_id));
            Ok(())
        })();
        // A use sent says nothing; the cast bar shows the effect once the
        // server takes it.
        match result {
            Ok(()) => self.actions.message.clear(),
            Err(error) => self.refuse(crate::outbox::window_line(&error)),
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Takes the answer to the pending item use, never an earlier one;
    /// success is submission, not activation.
    pub fn item_use_result(
        &mut self,
        session_id: u64,
        request_id: u64,
        error: Option<String>,
        present_error: bool,
    ) {
        if self.actions.pending_use != Some((session_id, request_id)) {
            return;
        }
        self.actions.pending_use = None;
        match error {
            None => {
                self.actions.message =
                    "Item-use packet submitted; waiting for server result".into();
            }
            Some(reason) if present_error => self.refuse(format!("Item use rejected: {reason}")),
            Some(reason) => self.actions.message = format!("Item use rejected: {reason}"),
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Discards unsent selection and pending presentation when leaving an admission.
    pub fn cancel_actions(&mut self) {
        self.actions = Actions::default();
        self.revision = self.revision.wrapping_add(1);
    }

    /// Matches a worker result to its admission and source revision, never a later move.
    pub fn action_result(
        &mut self,
        session_id: u64,
        revision: u64,
        error: Option<String>,
        present_error: bool,
        inventory: &Inventory,
    ) {
        if self.ground_drop_result(session_id, revision, error.as_deref())
            || self.destruction_result(session_id, revision, error.as_deref())
        {
            return;
        }
        let Some((pending_session, pending_revision, to_cursor)) = self.actions.pending else {
            return;
        };
        if pending_session != session_id {
            return;
        }
        if pending_revision != revision {
            if let Some(error) = error {
                self.actions.auto_store = false;
                if present_error {
                    self.refuse(error);
                }
            }
            return;
        }
        self.actions.pending = None;
        if error.is_some() {
            self.actions.auto_store = false;
        }
        if let Some(error) = error {
            if present_error {
                self.refuse(error);
            }
        } else if to_cursor || inventory.items().contains_key(&InventorySlot::CURSOR) {
            self.actions.message = "Item is on the cursor; choose a destination".into();
        } else {
            // The worker reports its send, not the server's acceptance.
            self.actions.message = "Move sent; placement is a local prediction".into();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Chooses a quantity for an empty-cursor stack click. The modifiers
    /// request all or one; neither changes placement or a single item.
    pub(super) fn left_click_slot(
        &mut self,
        slot: InventorySlot,
        whole: bool,
        one: bool,
        online: &mut OnlineState,
        sender: &Outbox,
    ) {
        let inventory = online.world().inventory();
        let stack = !inventory.items().contains_key(&InventorySlot::CURSOR)
            && online
                .world()
                .coins_in(CoinPlace::Cursor)
                .is_none_or(|coins| coins.is_empty())
            && inventory
                .items()
                .get(&slot)
                .and_then(|item| item.stack_count)
                .is_some_and(|count| count > 1);
        if stack && !whole && !one {
            self.select_split(slot, inventory);
        } else {
            self.click_slot(slot, stack && one, online, sender);
        }
    }

    /// Picks up, places, or swaps an item through the real EQ cursor.
    pub(super) fn click_slot(
        &mut self,
        slot: InventorySlot,
        split: bool,
        online: &mut OnlineState,
        sender: &Outbox,
    ) -> bool {
        self.actions.auto_store = false;
        self.actions.split = None;
        let result = self.try_click(slot, split.then_some(NonZeroU32::MIN), online, sender);
        let queued = result.is_ok();
        if let Err(error) = result {
            self.refuse(crate::outbox::window_line(&error));
        }
        self.revision = self.revision.wrapping_add(1);
        queued
    }

    /// Opens a quantity picker bound to the current inventory revision.
    pub(super) fn select_split(&mut self, slot: InventorySlot, inventory: &Inventory) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let count = inventory
            .items()
            .get(&slot)
            .and_then(|item| item.stack_count);
        if inventory.stale() || inventory.items().contains_key(&InventorySlot::CURSOR) {
            self.refuse("Finish the current cursor move first".into());
        } else if let Some(available) = count.filter(|count| *count > 1) {
            self.actions.split = Some(SplitSelection {
                picked: Picked::Stack(slot),
                revision: inventory.revision(),
                amount: 1,
                available,
            });
        } else {
            self.refuse("Choose a stack with more than one item".into());
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// How many the quantity picker takes, of how many there are, while it
    /// is open.
    pub(crate) fn picked(&self) -> Option<(u32, u32)> {
        self.actions
            .split
            .as_ref()
            .map(|split| (split.amount, split.available))
    }

    /// Sets how many the picker takes from a place along its slider, from
    /// 0.0 for one to 1.0 for all of them; a place beyond either end is that
    /// end.
    pub(crate) fn slide_amount(&mut self, along: f32) {
        let Some(split) = &mut self.actions.split else {
            return;
        };
        let span = split.available.saturating_sub(1);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a place along the slider, between none and all of a pile"
        )]
        let past_one = (along.clamp(0.0, 1.0) * span as f32).round() as u32;
        let amount = past_one.min(span) + 1;
        if split.amount != amount {
            split.amount = amount;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Where the picker's amount sits along its slider, from 0.0 for one to
    /// 1.0 for all of them, while it is open.
    pub(crate) fn amount_along(&self) -> Option<f32> {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a place along the slider, between none and all of a pile"
        )]
        self.picked().map(|(amount, available)| {
            if available > 1 {
                amount.saturating_sub(1) as f32 / (available - 1) as f32
            } else {
                1.0
            }
        })
    }

    /// A digit typed at the end of the picker's number; a number over how
    /// many there are becomes all of them.
    pub(crate) fn type_digit(&mut self, digit: u32) {
        let Some(split) = &mut self.actions.split else {
            return;
        };
        split.amount = split
            .amount
            .saturating_mul(10)
            .saturating_add(digit)
            .min(split.available);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Takes the last digit off the picker's number.
    pub(crate) fn erase_digit(&mut self) {
        let Some(split) = &mut self.actions.split else {
            return;
        };
        split.amount /= 10;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Closes the quantity picker without taking anything.
    pub(crate) fn cancel_split(&mut self) {
        self.actions.merchant = None;
        if self.actions.split.take().is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Adjusts or submits a selected quantity without replaying stale
    /// selections. Accepting none, a number typed away, takes nothing; nor
    /// does accepting more coins than their place now holds, as when a
    /// purchase spent some while the picker was open: the player chooses
    /// again, as for a stack that changed.
    pub(crate) fn split_action(
        &mut self,
        action: SplitAction,
        online: &mut OnlineState,
        sender: &Outbox,
    ) {
        let Some(mut selection) = self.actions.split.take() else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        let stack = match selection.picked {
            Picked::Stack(slot) => Some(slot),
            Picked::Coins(..) | Picked::Merchant(_) => None,
        };
        if stack.is_some() && selection.revision != online.world().inventory().revision() {
            self.refuse("Inventory changed; choose the stack again".into());
            return;
        }
        match action {
            SplitAction::Less => selection.amount = selection.amount.saturating_sub(1).max(1),
            SplitAction::More => {
                selection.amount = selection.amount.saturating_add(1).min(selection.available);
            }
            SplitAction::Minimum => selection.amount = 1,
            SplitAction::Maximum => selection.amount = selection.available,
            SplitAction::Cancel => return,
            SplitAction::Confirm if selection.amount == 0 => return,
            SplitAction::Confirm => {
                match selection.picked {
                    Picked::Merchant(merchant) => {
                        self.actions.merchant = Some((merchant, selection.amount));
                    }
                    Picked::Stack(slot) => {
                        if let Err(error) =
                            self.try_click(slot, NonZeroU32::new(selection.amount), online, sender)
                        {
                            self.refuse(crate::outbox::window_line(&error));
                        }
                    }
                    Picked::Coins(place, coin) => {
                        let held = online
                            .world()
                            .coins_in(place)
                            .map_or(0, |coins| coins.of(coin));
                        if held < selection.amount {
                            self.refuse("The coins changed; choose them again".into());
                        } else {
                            self.actions.coins = Some(CoinTransfer {
                                from: place,
                                to: CoinPlace::Cursor,
                                coin,
                                into: coin,
                                amount: selection.amount,
                            });
                        }
                    }
                }
                return;
            }
        }
        self.actions.split = Some(selection);
    }

    /// Advances automatic storage by one validated move against the latest local projection.
    pub(super) fn store_cursor(&mut self, online: &mut OnlineState, sender: &Outbox) {
        if !self.actions.auto_store {
            return;
        }
        let inventory = online.world().inventory();
        if !inventory.items().contains_key(&InventorySlot::CURSOR) {
            self.actions.auto_store = false;
            return;
        }
        let destination = self
            .actor(online)
            .and_then(|actor| inventory.auto_store_destination(actor));
        let result =
            destination.and_then(|destination| self.try_click(destination, None, online, sender));
        if let Err(error) = result {
            self.actions.auto_store = false;
            self.refuse(crate::outbox::window_line(&error));
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Hands the item on the cursor to the open give window, by the
    /// installed generation's `rule`: under
    /// [`eq_client_assets::ui::GiveAtOpen::StackFirst`] onto a stack of the
    /// same item with room left, else into the first empty slot.
    pub(crate) fn hand_over_cursor(
        &mut self,
        online: &mut OnlineState,
        sender: &Outbox,
        rule: eq_client_assets::ui::GiveAtOpen,
    ) -> Handed {
        let world = online.world();
        let slots = world
            .exchange()
            .map_or(0, eq_client_core::world::Exchange::trade_slots);
        let items = world.inventory().items();
        let Some(held) = items.get(&InventorySlot::CURSOR) else {
            return Handed::Nothing;
        };
        let trade = || (0..i32::from(slots)).map(|index| InventorySlot(3000 + index));
        // A stack of the same item with room left takes it first.
        let stack = (rule == eq_client_assets::ui::GiveAtOpen::StackFirst
            && held.stack_count.is_some())
        .then(|| {
            trade().find(|slot| {
                items.get(slot).is_some_and(|other| {
                    other.details.id == held.details.id
                        && other.stack_count.unwrap_or(0) < other.rules.stack_size
                })
            })
        })
        .flatten();
        if let Some(slot) = stack {
            if !self.click_slot(slot, false, online, sender) {
                return Handed::Placed;
            }
            // The projection shows any remainder immediately; a refusal
            // stops automatic handover without replaying queued operations.
            return if online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
            {
                Handed::Stacked
            } else {
                Handed::Placed
            };
        }
        match trade().find(|slot| !items.contains_key(slot)) {
            Some(slot) => {
                self.click_slot(slot, false, online, sender);
                Handed::Placed
            }
            None => Handed::NoRoom,
        }
    }

    fn actor(&self, online: &OnlineState) -> anyhow::Result<InventoryActor> {
        use anyhow::Context;
        let exchange = online.world().exchange();
        let trade_slots = exchange.map_or(0, eq_client_core::world::Exchange::trade_slots);
        // An NPC takes NO DROP items; another player never does.
        let trade_no_drop = exchange
            .is_none_or(|exchange| exchange.partner == eq_client_core::exchange::Partner::Npc);
        let world_container = online.world().container().is_some();
        if self.demo {
            return Ok(InventoryActor {
                bank_access: self.bank_open,
                dual_wield: None,
                deity: None,
                class: Some(1),
                race: 1,
                level: 60,
                trade_slots,
                trade_no_drop,
                world_container,
            });
        }
        let player = online
            .world()
            .player()
            .context("Character data is unavailable")?;
        Ok(InventoryActor {
            bank_access: self.bank_open,
            deity: player.deity,
            dual_wield: player
                .skills
                .as_ref()
                .and_then(|skills| skills.get(22))
                .copied(),
            class: player.class,
            race: player.race,
            level: player.level,
            trade_slots,
            trade_no_drop,
            world_container,
        })
    }

    #[allow(clippy::too_many_lines)] // One transactional path keeps validation and submission adjacent.
    fn try_click(
        &mut self,
        slot: InventorySlot,
        split: Option<NonZeroU32>,
        online: &mut OnlineState,
        sender: &Outbox,
    ) -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        let inventory = online.world().inventory();
        ensure!(
            online
                .world()
                .coins_in(CoinPlace::Cursor)
                .is_none_or(|coins| coins.is_empty()),
            "Put the coins on the cursor down first"
        );
        // The offline demo settles its own moves; online, the outbox stamps
        // the move, or says why not.
        let stamp = if self.demo {
            Stamp {
                session_id: 0,
                created: Instant::now(),
            }
        } else {
            sender.stamp(online.world())?
        };
        ensure!(
            inventory.received() && !inventory.stale(),
            "Wait for a current inventory snapshot"
        );
        let trade_slots = online
            .world()
            .exchange()
            .map_or(0, eq_client_core::world::Exchange::trade_slots);
        ensure!(
            slot.is_equipment()
                || slot.is_carried()
                || slot == InventorySlot::CURSOR
                || (self.bank_open && slot.is_personal_bank())
                || (slot.is_trade() && slot.0 - 3000 < i32::from(trade_slots))
                || (slot.is_world() && online.world().container().is_some()),
            "This slot is view only"
        );
        let cursor_item = inventory.items().get(&InventorySlot::CURSOR);
        let (from, to, quantity, to_cursor) = if let Some(item) = cursor_item {
            ensure!(
                slot != InventorySlot::CURSOR,
                "Choose a destination for the cursor item"
            );
            let quantity =
                if let Some(destination) = inventory.items().get(&slot).filter(|other| {
                    other.details.id == item.details.id && item.stack_count.is_some()
                }) {
                    let free = destination
                        .rules
                        .stack_size
                        .saturating_sub(destination.stack_count.unwrap_or(0));
                    let amount = item.stack_count.unwrap_or(0).min(free);
                    MoveQuantity::Count(
                        NonZeroU32::new(amount)
                            .context("This stack is full or its capacity is unknown")?,
                    )
                } else {
                    MoveQuantity::Whole
                };
            (InventorySlot::CURSOR, slot, quantity, false)
        } else {
            ensure!(
                slot != InventorySlot::CURSOR,
                "The cursor is empty; choose an item to pick up"
            );
            let item = inventory
                .items()
                .get(&slot)
                .context("Choose an item to pick up")?;
            self.actions.message = format!("Picking up {}", item.details.name);
            (
                slot,
                InventorySlot::CURSOR,
                split.map_or(MoveQuantity::Whole, MoveQuantity::Count),
                true,
            )
        };
        let revision = inventory.revision();
        let actor = self.actor(online)?;
        let request = InventoryMove {
            sequence: None,
            session_id: stamp.session_id,
            revision,
            from,
            to,
            created: stamp.created,
            quantity,
        };
        let update = inventory.plan_move(&request, actor)?;
        if self.demo {
            // The offline demo settles the move itself, as the server would.
            let mut after = inventory.clone();
            after.apply(update.clone());
            self.demo_news.push(update);
            self.actions.message = if after.items().contains_key(&InventorySlot::CURSOR) {
                "Item is on the cursor; choose a destination".into()
            } else {
                "Item placed locally (offline demo)".into()
            };
        } else {
            online.queue_inventory_move(request, actor, sender)?;
            self.actions.pending = Some((stamp.session_id, revision, to_cursor));
            self.actions.message = if to_cursor {
                "Picking item up onto cursor".into()
            } else {
                "Placing cursor item".into()
            };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::inventory::InventoryUpdate;

    #[test]
    fn dependent_refusals_drain_actions_without_repeating_the_root_chat_notice() {
        let mut state = InventoryState::default();
        state.actions.pending = Some((7, 3, false));
        state.actions.auto_store = true;
        let inventory = Inventory::default();
        state.action_result(7, 1, Some("Access changed".into()), true, &inventory);
        state.action_result(7, 2, Some("Predecessor refused".into()), false, &inventory);
        state.action_result(7, 3, Some("Predecessor refused".into()), false, &inventory);
        assert_eq!(state.refused, ["Access changed"]);
        assert_eq!(state.actions.message, "Access changed");
        assert_eq!(state.actions.pending, None);
        assert!(!state.actions.auto_store);
        state.actions.pending = Some((7, 4, false));
        state.action_result(7, 4, Some("Different root".into()), true, &inventory);
        assert_eq!(state.refused, ["Access changed", "Different root"]);
    }

    /// The inventory window beside the world holding the items.
    struct Bench {
        state: InventoryState,
        online: OnlineState,
    }

    impl Bench {
        /// The offline demo's items, with an empty cursor, for a player
        /// admitted in this session.
        fn demo(session_id: u64) -> Self {
            let mut online = OnlineState::new(true);
            testing::admit(&mut online, session_id, test_player());
            let mut bench = Self {
                state: InventoryState::default(),
                online,
            };
            bench.tell(InventoryUpdate::Snapshot(
                crate::preview::items()
                    .into_iter()
                    .filter(|item| item.slot != InventorySlot::CURSOR)
                    .collect(),
            ));
            bench.state.demo = true;
            bench
        }

        /// The items as the world has them.
        fn data(&self) -> &Inventory {
            self.online.world().inventory()
        }

        /// The session reports a change to the inventory.
        fn tell(&mut self, update: InventoryUpdate) {
            testing::inventory(&mut self.online, update);
            self.state.refresh(self.data().stale());
        }

        /// The demo's own moves reach the world.
        fn settle(&mut self) {
            for update in std::mem::take(&mut self.state.demo_news) {
                self.tell(update);
            }
        }

        fn click(&mut self, slot: i32, split: bool) {
            self.state.click_slot(
                InventorySlot(slot),
                split,
                &mut self.online,
                &crate::outbox::Outbox::new(None),
            );
            self.settle();
        }

        fn split(&mut self, action: SplitAction) {
            self.state
                .split_action(action, &mut self.online, &crate::outbox::Outbox::new(None));
            self.settle();
        }

        fn store(&mut self) {
            self.state
                .store_cursor(&mut self.online, &crate::outbox::Outbox::new(None));
            self.settle();
        }

        fn select_split(&mut self, slot: i32) {
            self.state
                .select_split(InventorySlot(slot), self.online.world().inventory());
        }

        /// Puts five of the rations on the cursor beside a stack of eighteen.
        fn hold_rations(&mut self) {
            let mut cursor = self.data().items()[&InventorySlot(251)].clone();
            cursor.slot = InventorySlot::CURSOR;
            cursor.stack_count = Some(5);
            let mut destination = self.data().items()[&InventorySlot(251)].clone();
            destination.stack_count = Some(18);
            self.tell(InventoryUpdate::Set(vec![cursor, destination]));
        }
    }

    #[test]
    fn item_use_queues_current_instance_without_consuming_it_and_rejects_duplicate_or_busy_actions()
    {
        use eq_client_core::inventory::{ClickEffect, ClickKind, ItemActivation};
        let mut bench = Bench::demo(9);
        let mut item = bench.data().items().values().next().unwrap().clone();
        item.slot = InventorySlot(13);
        item.activation = ItemActivation {
            effect: Some(ClickEffect {
                spell_id: 73,
                kind: ClickKind::Click,
                required_level: 1,
                effect_level: 1,
                cast_time_ms: 1000,
                recast_delay_seconds: 0,
                recast_type: 0,
            }),
            ..ItemActivation::default()
        };
        item.charges = 3;
        bench.tell(InventoryUpdate::Snapshot(vec![item]));
        let before = bench.data().clone();
        let Bench { state, online } = &mut bench;
        let own_id = online.world().player().unwrap().spawn_id;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let sender = crate::outbox::Outbox::new(Some(sender));
        state.use_slot(InventorySlot(13), online, &sender, None, true, Vec::new);
        assert!(receiver.try_recv().is_err());
        state.use_slot(
            InventorySlot(13),
            online,
            &sender,
            Some(65500),
            false,
            Vec::new,
        );
        assert!(receiver.try_recv().is_err());
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        let ClientCommand::UseItem(request) = receiver.try_recv().unwrap() else {
            panic!("expected item use")
        };
        assert_eq!(request.slot, InventorySlot(13));
        assert_eq!(request.session_id, 9);
        assert_eq!(request.revision, before.revision());
        assert_eq!(request.target_id, own_id);
        assert_eq!(online.world().inventory(), &before);
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        assert!(receiver.try_recv().is_err());
        // Elapsing the rate limit cannot replace a request awaiting worker feedback.
        state.actions.last_use = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.actions.pending_use, Some((9, request.request_id)));
        state.item_use_result(8, request.request_id, None, true);
        state.item_use_result(9, request.request_id + 1, None, true);
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        assert!(receiver.try_recv().is_err());
        state.item_use_result(
            9,
            request.request_id,
            Some("Synthetic rejection".into()),
            true,
        );
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        let ClientCommand::UseItem(next) = receiver.try_recv().unwrap() else {
            panic!("expected a fresh manual item-use request")
        };
        assert_eq!(next.request_id, request.request_id + 1);
        state.cancel_actions();
        state.use_slot(
            InventorySlot(13),
            online,
            &crate::outbox::Outbox::new(None),
            None,
            false,
            Vec::new,
        );
        assert!(state.actions.last_use.is_none());
        testing::connect(online, false);
        state.use_slot(InventorySlot(13), online, &sender, None, false, Vec::new);
        assert!(receiver.try_recv().is_err());
        assert_eq!(online.world().inventory(), &before);
    }

    #[test]
    fn item_use_feedback_ignores_old_replies_and_never_claims_effect_success() {
        let mut bench = Bench::demo(9);
        let before = bench.data().clone();
        let state = &mut bench.state;
        state.actions.pending_use = Some((9, 2));
        state.actions.message = "Queued".into();
        state.item_use_result(9, 1, Some("old rejection".into()), true);
        state.item_use_result(8, 2, None, true);
        assert_eq!(state.actions.message, "Queued");
        state.item_use_result(9, 2, Some("Target unavailable".into()), true);
        assert_eq!(
            state.actions.message,
            "Item use rejected: Target unavailable"
        );
        // Only the refusal is on its way to the chat; progress stays here.
        assert_eq!(state.refused, ["Item use rejected: Target unavailable"]);
        state.actions.pending_use = Some((9, 3));
        state.item_use_result(9, 3, None, true);
        assert_eq!(
            state.actions.message,
            "Item-use packet submitted; waiting for server result"
        );
        state.item_use_result(9, 2, Some("late duplicate".into()), true);
        assert!(state.actions.message.contains("submitted"));
        assert_eq!(state.refused.len(), 1);
        state.actions.pending_use = Some((9, 4));
        state.next_use_id = 4;
        state.cancel_actions();
        state.item_use_result(9, 4, None, true);
        assert_eq!(state.actions.message, "");
        assert_eq!(state.next_use_id, 4);
        assert_eq!(bench.data(), &before);
    }

    #[test]
    fn plain_stack_click_waits_for_quantity_before_posting_a_move() {
        let mut bench = Bench::demo(7);
        let before = bench.data().clone();
        let available = before.items()[&InventorySlot(251)].stack_count.unwrap();
        let Bench { state, online } = &mut bench;
        state.demo = false;
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let sender = Outbox::new(Some(tx));
        state.left_click_slot(InventorySlot(251), false, false, online, &sender);
        assert_eq!(state.picked(), Some((1, available)));
        assert_eq!(online.world().inventory(), &before);
        assert!(rx.try_recv().is_err());
        state.erase_digit();
        state.type_digit(2);
        state.split_action(SplitAction::Confirm, online, &sender);
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("expected the confirmed quantity move");
        };
        assert_eq!(
            (request.from, request.to),
            (InventorySlot(251), InventorySlot::CURSOR)
        );
        assert_eq!(
            request.quantity,
            MoveQuantity::Count(NonZeroU32::new(2).unwrap())
        );
        assert_eq!(
            online.world().inventory().items()[&InventorySlot::CURSOR].stack_count,
            Some(2)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn stack_modifiers_post_whole_or_one_without_a_quantity_picker() {
        for (whole, one) in [(true, false), (false, true), (true, true)] {
            let mut bench = Bench::demo(7);
            let available = bench.data().items()[&InventorySlot(251)]
                .stack_count
                .unwrap();
            let Bench { state, online } = &mut bench;
            state.demo = false;
            let (tx, rx) = std::sync::mpsc::sync_channel(2);
            let sender = Outbox::new(Some(tx));
            state.left_click_slot(InventorySlot(251), whole, one, online, &sender);
            assert_eq!(state.picked(), None);
            let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
                panic!("expected a modifier's move");
            };
            assert_eq!(
                request.quantity,
                if one {
                    MoveQuantity::Count(NonZeroU32::MIN)
                } else {
                    MoveQuantity::Whole
                }
            );
            assert_eq!(
                online.world().inventory().items()[&InventorySlot::CURSOR].stack_count,
                Some(if one { 1 } else { available })
            );
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn stack_modifiers_preserve_cursor_placement_and_single_items() {
        for (whole, one) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut bench = Bench::demo(7);
            bench.hold_rations();
            bench.state.left_click_slot(
                InventorySlot(251),
                whole,
                one,
                &mut bench.online,
                &Outbox::new(None),
            );
            bench.settle();
            assert_eq!(bench.state.picked(), None);
            assert_eq!(
                bench.data().items()[&InventorySlot(251)].stack_count,
                Some(20)
            );
            assert_eq!(
                bench.data().items()[&InventorySlot::CURSOR].stack_count,
                Some(3)
            );
            let mut single = bench
                .data()
                .items()
                .values()
                .find(|item| item.bag_slots == 0 && item.stack_count.is_none())
                .unwrap()
                .clone();
            single.slot = InventorySlot(23);
            bench.tell(InventoryUpdate::Snapshot(vec![single.clone()]));
            bench.state.left_click_slot(
                InventorySlot(23),
                whole,
                one,
                &mut bench.online,
                &Outbox::new(None),
            );
            bench.settle();
            assert_eq!(bench.state.picked(), None);
            assert_eq!(
                bench.data().items()[&InventorySlot::CURSOR].details,
                single.details
            );
            assert!(!bench.data().items().contains_key(&InventorySlot(23)));
        }
    }

    #[test]
    fn quantity_picker_moves_selected_count_and_rejects_stale_selections() {
        let mut bench = Bench::demo(7);
        bench.select_split(251);
        bench.split(SplitAction::More);
        bench.split(SplitAction::More);
        bench.split(SplitAction::Confirm);
        assert_eq!(
            bench.data().items()[&InventorySlot::CURSOR].stack_count,
            Some(3)
        );
        assert_eq!(
            bench.data().items()[&InventorySlot(251)].stack_count,
            Some(17)
        );
        assert!(bench.state.actions.split.is_none());
        bench.click(251, false);
        bench.select_split(251);
        bench.split(SplitAction::Maximum);
        assert_eq!(bench.state.actions.split.as_ref().unwrap().amount, 20);
        bench.split(SplitAction::More);
        assert_eq!(bench.state.actions.split.as_ref().unwrap().amount, 20);
        bench.split(SplitAction::Minimum);
        bench.split(SplitAction::Less);
        assert_eq!(bench.state.actions.split.as_ref().unwrap().amount, 1);
        bench.tell(InventoryUpdate::Invalidated);
        let before = bench.data().clone();
        bench.split(SplitAction::Confirm);
        assert_eq!(bench.data(), &before);
        assert!(bench.state.actions.split.is_none());
    }

    #[test]
    fn coins_accepted_after_their_place_holds_fewer_are_chosen_again() {
        use eq_client_core::{Coins, WorldEvent};
        let mut bench = Bench::demo(7);
        let purse = |gold| {
            WorldEvent::Coins(Coins {
                gold,
                ..Coins::default()
            })
        };
        testing::news(&mut bench.online, [purse(40)]);
        bench.state.select_coins(CoinPlace::Purse, Coin::Gold, 40);
        bench.state.slide_amount(1.0);
        // A purchase spends some of the gold while the window is open.
        testing::news(&mut bench.online, [purse(25)]);
        bench.split(SplitAction::Confirm);
        assert!(bench.state.take_coins().is_none());
        assert_eq!(
            bench.state.refused,
            ["The coins changed; choose them again"]
        );
        assert!(bench.state.picked().is_none());
        // What the purse still holds can be taken.
        bench.state.select_coins(CoinPlace::Purse, Coin::Gold, 25);
        bench.split(SplitAction::Confirm);
        let taken = bench.state.take_coins().unwrap();
        assert_eq!((taken.coin, taken.amount), (Coin::Gold, 25));
    }

    #[test]
    fn the_quantity_windows_slider_and_number_set_the_amount() {
        let mut bench = Bench::demo(7);
        bench.select_split(251);
        let state = &mut bench.state;
        assert_eq!(state.picked(), Some((1, 20)));
        assert_eq!(state.amount_along(), Some(0.0));
        // The slider runs from one at its left end to all at its right.
        state.slide_amount(1.0);
        assert_eq!(state.picked(), Some((20, 20)));
        state.slide_amount(0.5);
        assert_eq!(state.picked(), Some((11, 20)));
        state.slide_amount(-3.0);
        assert_eq!(state.picked(), Some((1, 20)));
        state.slide_amount(9.0);
        assert_eq!(state.amount_along(), Some(1.0));
        // Typing goes on from the number's end; Backspace takes the last
        // digit, and a number over the stack is all of it.
        state.erase_digit();
        state.erase_digit();
        assert_eq!(state.picked(), Some((0, 20)));
        state.type_digit(1);
        state.type_digit(5);
        assert_eq!(state.picked(), Some((15, 20)));
        state.type_digit(4);
        assert_eq!(state.picked(), Some((20, 20)));
        // Accepting a number typed away takes nothing.
        state.erase_digit();
        state.erase_digit();
        let before = bench.data().clone();
        bench.split(SplitAction::Confirm);
        assert_eq!(bench.data(), &before);
        assert!(bench.state.picked().is_none());
        bench.select_split(251);
        bench.state.cancel_split();
        assert!(bench.state.picked().is_none());
    }

    #[test]
    fn automatic_storage_merges_then_places_remainder_and_stops() {
        let mut bench = Bench::demo(7);
        bench.hold_rations();
        bench.state.actions.auto_store = true;
        bench.store();
        assert_eq!(
            bench.data().items()[&InventorySlot::CURSOR].stack_count,
            Some(3)
        );
        bench.store();
        assert!(!bench.data().items().contains_key(&InventorySlot::CURSOR));
        assert_eq!(
            bench.data().items()[&InventorySlot(24)].stack_count,
            Some(3)
        );
        bench.store();
        assert!(!bench.state.actions.auto_store);
    }

    #[test]
    fn automatic_storage_projects_remainder_without_feedback_and_stops_on_rejection() {
        let mut bench = Bench::demo(7);
        bench.hold_rations();
        let original = bench.data().clone();
        let Bench { state, online } = &mut bench;
        state.demo = false;
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let sender = crate::outbox::Outbox::new(Some(tx));
        state.actions.auto_store = true;
        state.store_cursor(online, &sender);
        state.store_cursor(online, &sender);
        let ClientCommand::MoveInventory(first) = rx.try_recv().unwrap() else {
            panic!("first move")
        };
        let ClientCommand::MoveInventory(second) = rx.try_recv().unwrap() else {
            panic!("remainder move")
        };
        assert_eq!(
            second.sequence.unwrap().after,
            Some(first.sequence.unwrap().id)
        );
        assert!(
            !online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        for request in [first, second] {
            testing::news(
                online,
                [eq_client_core::WorldEvent::InventoryAction {
                    session_id: 7,
                    revision: request.revision,
                    sequence: request.sequence,
                    update: None,
                    error: Some("Rejected".into()),
                }],
            );
            state.action_result(
                7,
                request.revision,
                Some("Rejected".into()),
                true,
                online.world().inventory(),
            );
        }
        state.store_cursor(online, &sender);
        assert!(!state.actions.auto_store);
        assert!(rx.try_recv().is_err());
        assert_eq!(online.world().inventory(), &original);
    }

    #[test]
    fn clicking_matching_stack_merges_the_cursor_back_into_it() {
        let mut bench = Bench::demo(7);
        bench.click(251, true);
        assert_eq!(
            bench.data().items()[&InventorySlot::CURSOR].stack_count,
            Some(1)
        );
        bench.click(251, false);
        assert!(!bench.data().items().contains_key(&InventorySlot::CURSOR));
        assert_eq!(
            bench.data().items()[&InventorySlot(251)].stack_count,
            Some(20)
        );
    }

    #[test]
    fn merging_fills_destination_and_leaves_excess_on_cursor() {
        let mut bench = Bench::demo(7);
        bench.hold_rations();
        bench.click(251, false);
        assert_eq!(
            bench.data().items()[&InventorySlot::CURSOR].stack_count,
            Some(3)
        );
        assert_eq!(
            bench.data().items()[&InventorySlot(251)].stack_count,
            Some(20)
        );
        assert!(bench.state.actions.message.contains("on the cursor"));
        let before = bench.data().clone();
        bench.click(251, false);
        assert_eq!(bench.data(), &before);
        assert!(bench.state.actions.message.contains("full"));
    }

    #[test]
    fn demo_moves_filled_bags_splits_stacks_and_equips_using_the_shared_validator() {
        let mut bench = Bench::demo(7);
        bench.click(22, false);
        bench.click(24, false);
        assert!(bench.data().items().contains_key(&InventorySlot(271)));
        assert!(!bench.data().items().contains_key(&InventorySlot(251)));
        bench.click(271, true);
        bench.click(25, false);
        assert_eq!(
            bench.data().items()[&InventorySlot(271)].stack_count,
            Some(19)
        );
        assert_eq!(
            bench.data().items()[&InventorySlot(25)].stack_count,
            Some(1)
        );
        bench.click(13, false);
        bench.click(22, false);
        assert!(!bench.data().items().contains_key(&InventorySlot(13)));
        bench.click(22, false);
        bench.click(13, false);
        assert_eq!(
            bench.data().items()[&InventorySlot(13)].details.name,
            "Preview sword"
        );
        assert!(bench.state.actions.pending.is_none());
    }

    #[test]
    fn updates_preserve_the_real_cursor_and_view_only_slots_never_move() {
        let mut bench = Bench::demo(7);
        bench.click(251, false);
        bench.tell(InventoryUpdate::Set(vec![
            crate::preview::items()[0].clone(),
        ]));
        assert!(bench.data().items().contains_key(&InventorySlot::CURSOR));
        bench.click(24, false);
        assert!(bench.data().items().contains_key(&InventorySlot(24)));
        let before = bench.data().clone();
        bench.click(2000, false);
        assert_eq!(bench.data(), &before);
    }

    #[test]
    fn queue_failure_preserves_projection_and_refused_chain_never_replays() {
        let mut bench = Bench::demo(7);
        let before = bench.data().clone();
        let Bench { state, online } = &mut bench;
        state.demo = false;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = crate::outbox::Outbox::new(Some(tx));
        assert!(state.click_slot(InventorySlot(251), false, online, &sender));
        let picked = online.world().inventory().clone();
        assert!(picked.predicted());
        assert!(!state.click_slot(InventorySlot(24), false, online, &sender));
        assert_eq!(online.world().inventory(), &picked);
        let ClientCommand::MoveInventory(first) = rx.try_recv().unwrap() else {
            panic!("pick up")
        };
        assert!(state.click_slot(InventorySlot(25), false, online, &sender));
        let ClientCommand::MoveInventory(second) = rx.try_recv().unwrap() else {
            panic!("place")
        };
        assert_eq!(
            (second.from, second.to),
            (InventorySlot::CURSOR, InventorySlot(25))
        );
        for request in [first, second] {
            testing::news(
                online,
                [eq_client_core::WorldEvent::InventoryAction {
                    session_id: 7,
                    revision: request.revision,
                    sequence: request.sequence,
                    update: None,
                    error: Some("Rejected".into()),
                }],
            );
            state.action_result(
                7,
                request.revision,
                Some("Rejected".into()),
                true,
                online.world().inventory(),
            );
        }
        assert_eq!(online.world().inventory(), &before);
        assert!(!online.world().inventory_moves_pending());
        assert_eq!(state.actions.message, "Rejected");
        testing::connect(online, false);
        assert!(!state.click_slot(InventorySlot(251), false, online, &sender));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn banking_tracks_nearby_service_and_projects_only_valid_queued_moves() {
        let mut bench = Bench::demo(7);
        bench.state.demo = false;
        let before = bench.data().clone();
        let Bench { state, online } = &mut bench;
        testing::spawn_entry(
            online,
            9,
            eq_client_core::SpawnState {
                class: Some(40),
                spawn_id: 9,
                name: "Synthetic banker".into(),
                kind: eq_client_core::SpawnKind::Npc,
                race: 1,
                gender: 0,
                position: eq_client_core::WorldPosition::default(),
                velocity: [0.0; 3],
                forward: 0.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                size: 6.0,
                invisible: false,
                appearance: eq_client_core::outfit::Appearance::default(),
                level: 0,
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
                pet_owner: None,
                hp_percent: None,
            },
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let sender = crate::outbox::Outbox::new(Some(tx));
        state.refresh_bank_access(online);
        assert!(state.bank_open);
        // The bank's window names the banker in reach.
        assert_eq!(state.banker(), "Synthetic banker");
        state.click_slot(InventorySlot(2000), false, online, &sender);
        assert!(
            matches!(rx.try_recv().unwrap(), ClientCommand::MoveInventory(request)
            if request.from == InventorySlot(2000) && request.to == InventorySlot::CURSOR)
        );
        assert_eq!(online.world().inventory().revision(), before.revision() + 1);
        assert!(
            online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        assert!(
            !online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot(2000))
        );
        state.tab = super::super::Tab::Bank;
        testing::place_axis(online, |position| position.x = 21.0);
        state.refresh_bank_access(online);
        assert!(!state.bank_open);
        assert_eq!(state.banker(), "");
        assert_eq!(state.tab, super::super::Tab::Inventory);
        // Closing the bank never discards a move already submitted to the worker.
        assert!(state.actions.pending.is_some());
        testing::place_axis(online, |position| position.x = 0.0);
        state.refresh_bank_access(online);
        assert!(state.bank_open);
        testing::connect(online, false);
        state.refresh_bank_access(online);
        assert!(!state.bank_open);
        testing::connect(online, true);
        testing::news(online, [eq_client_core::WorldEvent::Despawn(9)]);
        state.refresh_bank_access(online);
        assert!(!state.bank_open);
    }

    fn test_player() -> eq_client_core::PlayerState {
        eq_client_core::PlayerState {
            name: "Example".into(),
            titanium_environment: None,
            base_attributes: None,
            deity: None,
            class: Some(1),
            spawn_id: 1,
            race: 1,
            gender: 0,
            level: 60,
            position: eq_client_core::WorldPosition::default(),
            mana: 0,
            endurance: Some(0),
            skills: None,
            practice_points: None,
            languages: None,
            disciplines: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
            name_parts: eq_client_core::names::NameParts::default(),
        }
    }
}

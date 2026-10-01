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
}

/// What a quantity picker takes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picked {
    /// A stack in this inventory slot.
    Stack(InventorySlot),
    /// Coins of one kind in a place.
    Coins(CoinPlace, Coin),
}

pub(super) struct SplitSelection {
    pub picked: Picked,
    pub revision: u64,
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
    /// Queues an explicit click effect without predicting charges or inventory changes.
    pub(super) fn use_slot(
        &mut self,
        slot: InventorySlot,
        online: &OnlineState,
        sender: &Outbox,
        target: Option<u16>,
        casting: bool,
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
                self.actions.pending.is_none(),
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
                    .is_some_and(|spawn| !spawn.invisible);
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
        self.actions.message = result.map_or_else(
            |error| crate::outbox::window_line(&error),
            |()| String::new(),
        );
        self.revision = self.revision.wrapping_add(1);
    }

    /// Returns whether the reply matched; success is submission, not activation.
    pub fn item_use_result(
        &mut self,
        session_id: u64,
        request_id: u64,
        error: Option<String>,
    ) -> bool {
        if self.actions.pending_use != Some((session_id, request_id)) {
            return false;
        }
        self.actions.pending_use = None;
        self.actions.message = error.map_or_else(
            || "Item-use packet submitted; waiting for server result".into(),
            |reason| format!("Item use rejected: {reason}"),
        );
        self.revision = self.revision.wrapping_add(1);
        true
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
        inventory: &Inventory,
    ) {
        let Some((pending_session, pending_revision, to_cursor)) = self.actions.pending else {
            return;
        };
        if (pending_session, pending_revision) != (session_id, revision) {
            return;
        }
        self.actions.pending = None;
        if error.is_some() {
            self.actions.auto_store = false;
        }
        self.actions.message = error.unwrap_or_else(|| {
            if to_cursor || inventory.items().contains_key(&InventorySlot::CURSOR) {
                "Item is on the cursor; choose a destination".into()
            } else {
                // Servers acknowledge only refused moves, so the placement
                // settles without a later message; claim nothing more.
                "Item placed".into()
            }
        });
        self.revision = self.revision.wrapping_add(1);
    }

    /// Picks up, places, or swaps an item through the real EQ cursor.
    pub(super) fn click_slot(
        &mut self,
        slot: InventorySlot,
        split: bool,
        online: &OnlineState,
        sender: &Outbox,
    ) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let result = self.try_click(slot, split.then_some(NonZeroU32::MIN), online, sender);
        if let Err(error) = result {
            self.actions.message = crate::outbox::window_line(&error);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Opens a quantity picker bound to the current inventory revision.
    pub(super) fn select_split(&mut self, slot: InventorySlot, inventory: &Inventory) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let count = inventory
            .items()
            .get(&slot)
            .and_then(|item| item.stack_count);
        if self.actions.pending.is_some()
            || inventory.stale()
            || inventory.items().contains_key(&InventorySlot::CURSOR)
        {
            self.actions.message = "Finish the current cursor move first".into();
        } else if let Some(available) = count.filter(|count| *count > 1) {
            self.actions.split = Some(SplitSelection {
                picked: Picked::Stack(slot),
                revision: inventory.revision(),
                amount: 1,
                available,
            });
        } else {
            self.actions.message = "Choose a stack with more than one item".into();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Adjusts or submits a selected quantity without replaying stale selections.
    pub(super) fn split_action(
        &mut self,
        action: SplitAction,
        online: &OnlineState,
        sender: &Outbox,
    ) {
        let Some(mut selection) = self.actions.split.take() else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        let stack = match selection.picked {
            Picked::Stack(slot) => Some(slot),
            Picked::Coins(..) => None,
        };
        if stack.is_some() && selection.revision != online.world().inventory().revision() {
            self.actions.message = "Inventory changed; choose the stack again".into();
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
            SplitAction::Confirm => {
                match selection.picked {
                    Picked::Stack(slot) => {
                        if let Err(error) =
                            self.try_click(slot, NonZeroU32::new(selection.amount), online, sender)
                        {
                            self.actions.message = crate::outbox::window_line(&error);
                        }
                    }
                    Picked::Coins(place, coin) => {
                        self.actions.coins = Some(CoinTransfer {
                            from: place,
                            to: CoinPlace::Cursor,
                            coin,
                            into: coin,
                            amount: selection.amount,
                        });
                    }
                }
                return;
            }
        }
        self.actions.split = Some(selection);
    }

    /// Advances automatic storage by one validated move after the previous result arrives.
    pub(super) fn store_cursor(&mut self, online: &OnlineState, sender: &Outbox) {
        if !self.actions.auto_store || self.actions.pending.is_some() {
            return;
        }
        let inventory = online.world().inventory();
        if !inventory.items().contains_key(&InventorySlot::CURSOR) {
            self.actions.auto_store = false;
            return;
        }
        let result = self.actor(online).and_then(|actor| {
            let destination = inventory.auto_store_destination(actor)?;
            self.try_click(destination, None, online, sender)
        });
        if let Err(error) = result {
            self.actions.auto_store = false;
            self.actions.message = crate::outbox::window_line(&error);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Hands the item on the cursor to the open give window: into its first
    /// empty slot, as the official client does when the window opens.
    pub(crate) fn hand_over_cursor(&mut self, online: &OnlineState, sender: &Outbox) {
        let world = online.world();
        let slots = world
            .exchange()
            .map_or(0, eq_client_core::world::Exchange::trade_slots);
        let items = world.inventory().items();
        if !items.contains_key(&InventorySlot::CURSOR) {
            return;
        }
        let empty = (0..i32::from(slots))
            .map(|index| InventorySlot(3000 + index))
            .find(|slot| !items.contains_key(slot));
        if let Some(slot) = empty {
            self.click_slot(slot, false, online, sender);
        }
    }

    fn actor(&self, online: &OnlineState) -> anyhow::Result<InventoryActor> {
        use anyhow::Context;
        let trade_slots = online
            .world()
            .exchange()
            .map_or(0, eq_client_core::world::Exchange::trade_slots);
        if self.demo {
            return Ok(InventoryActor {
                bank_access: self.bank_open,
                dual_wield: None,
                deity: None,
                class: Some(1),
                race: 1,
                level: 60,
                trade_slots,
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
        })
    }

    #[allow(clippy::too_many_lines)] // One transactional path keeps validation and submission adjacent.
    fn try_click(
        &mut self,
        slot: InventorySlot,
        split: Option<NonZeroU32>,
        online: &OnlineState,
        sender: &Outbox,
    ) -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        let inventory = online.world().inventory();
        ensure!(
            self.actions.pending.is_none(),
            "Waiting for the queued move; it will not be retried"
        );
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
                || (slot.is_trade() && slot.0 - 3000 < i32::from(trade_slots)),
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
            sender.send(online.world(), ClientCommand::MoveInventory(request))?;
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
                &self.online,
                &crate::outbox::Outbox::new(None),
            );
            self.settle();
        }

        fn split(&mut self, action: SplitAction) {
            self.state
                .split_action(action, &self.online, &crate::outbox::Outbox::new(None));
            self.settle();
        }

        fn store(&mut self) {
            self.state
                .store_cursor(&self.online, &crate::outbox::Outbox::new(None));
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
        state.use_slot(InventorySlot(13), online, &sender, None, true);
        assert!(receiver.try_recv().is_err());
        state.use_slot(InventorySlot(13), online, &sender, Some(65500), false);
        assert!(receiver.try_recv().is_err());
        state.use_slot(InventorySlot(13), online, &sender, None, false);
        let ClientCommand::UseItem(request) = receiver.try_recv().unwrap() else {
            panic!("expected item use")
        };
        assert_eq!(request.slot, InventorySlot(13));
        assert_eq!(request.session_id, 9);
        assert_eq!(request.revision, before.revision());
        assert_eq!(request.target_id, own_id);
        assert_eq!(online.world().inventory(), &before);
        state.use_slot(InventorySlot(13), online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        // Elapsing the rate limit cannot replace a request awaiting worker feedback.
        state.actions.last_use = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        state.use_slot(InventorySlot(13), online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.actions.pending_use, Some((9, request.request_id)));
        state.item_use_result(8, request.request_id, None);
        state.item_use_result(9, request.request_id + 1, None);
        state.use_slot(InventorySlot(13), online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        state.item_use_result(9, request.request_id, Some("Synthetic rejection".into()));
        state.use_slot(InventorySlot(13), online, &sender, None, false);
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
        );
        assert!(state.actions.last_use.is_none());
        testing::connect(online, false);
        state.use_slot(InventorySlot(13), online, &sender, None, false);
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
        state.item_use_result(9, 1, Some("old rejection".into()));
        state.item_use_result(8, 2, None);
        assert_eq!(state.actions.message, "Queued");
        state.item_use_result(9, 2, Some("Target unavailable".into()));
        assert_eq!(
            state.actions.message,
            "Item use rejected: Target unavailable"
        );
        state.actions.pending_use = Some((9, 3));
        state.item_use_result(9, 3, None);
        assert_eq!(
            state.actions.message,
            "Item-use packet submitted; waiting for server result"
        );
        state.item_use_result(9, 2, Some("late duplicate".into()));
        assert!(state.actions.message.contains("submitted"));
        state.actions.pending_use = Some((9, 4));
        state.next_use_id = 4;
        state.cancel_actions();
        state.item_use_result(9, 4, None);
        assert_eq!(state.actions.message, "");
        assert_eq!(state.next_use_id, 4);
        assert_eq!(bench.data(), &before);
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
    fn automatic_storage_waits_for_worker_and_stops_on_rejection() {
        let mut bench = Bench::demo(7);
        bench.click(251, false);
        let Bench { state, online } = &mut bench;
        state.demo = false;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = crate::outbox::Outbox::new(Some(tx));
        state.actions.auto_store = true;
        state.store_cursor(online, &sender);
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("wrong command")
        };
        state.store_cursor(online, &sender);
        assert!(rx.try_recv().is_err());
        state.action_result(
            7,
            request.revision,
            Some("Rejected".into()),
            online.world().inventory(),
        );
        state.store_cursor(online, &sender);
        assert!(!state.actions.auto_store);
        assert!(rx.try_recv().is_err());
        assert!(
            online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
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
    fn queue_submission_does_not_predict_or_confirm_and_pending_moves_cannot_duplicate() {
        let mut bench = Bench::demo(7);
        let before = bench.data().clone();
        let Bench { state, online } = &mut bench;
        state.demo = false;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = crate::outbox::Outbox::new(Some(tx));
        state.click_slot(InventorySlot(251), false, online, &sender);
        state.click_slot(InventorySlot(24), false, online, &sender);
        assert_eq!(online.world().inventory(), &before);
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("wrong command")
        };
        assert_eq!(request.from, InventorySlot(251));
        assert_eq!(request.to, InventorySlot::CURSOR);
        state.click_slot(InventorySlot(25), false, online, &sender);
        assert!(rx.try_recv().is_err());
        state.action_result(8, request.revision, None, online.world().inventory());
        assert!(state.actions.pending.is_some());
        state.action_result(
            7,
            request.revision,
            Some("Rejected".into()),
            online.world().inventory(),
        );
        assert!(state.actions.pending.is_none());
        assert_eq!(online.world().inventory(), &before);
        assert_eq!(state.actions.message, "Rejected");
        testing::connect(online, false);
        state.click_slot(InventorySlot(251), false, online, &sender);
        assert!(state.actions.pending.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn banking_tracks_nearby_service_and_queues_cursor_moves_without_prediction() {
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
                size: 6.0,
                invisible: false,
                appearance: eq_client_core::outfit::Appearance::default(),
                level: 0,
                listing: eq_client_core::listing::Listing::default(),
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
        assert_eq!(online.world().inventory(), &before);
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(100),
            appearance: eq_client_core::outfit::Appearance::default(),
            listing: eq_client_core::listing::Listing::default(),
        }
    }
}

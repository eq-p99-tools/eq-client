//! Inventory selection and submission; UI intent never carries raw packet bytes.
use super::{InventorySlot, InventoryState};
use crate::{online::OnlineState, target::CommandsToServer};
use eq_client_core::{
    ClientCommand,
    inventory::{InventoryActor, InventoryMove, MoveQuantity},
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
}

pub(super) struct SplitSelection {
    pub slot: InventorySlot,
    pub revision: u64,
    pub amount: u32,
    pub available: u32,
}

#[derive(bevy::prelude::Component, Clone, Copy)]
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
        sender: &CommandsToServer,
        target: Option<u16>,
        casting: bool,
    ) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let result = (|| -> anyhow::Result<()> {
            use anyhow::{Context, ensure};
            ensure!(
                online.enabled && online.connected && online.death.is_none(),
                "Connect to use an item"
            );
            ensure!(
                !casting,
                "Wait for the current cast to finish or interrupt it"
            );
            ensure!(
                self.actions.pending.is_none(),
                "Finish the pending inventory move first"
            );
            ensure!(
                !self.data.items().contains_key(&InventorySlot(30)),
                "Place the cursor item before using an item"
            );
            let now = Instant::now();
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
                .player
                .as_ref()
                .context("Character data is unavailable")?;
            let session_id = online.session_id.context("No active admission")?;
            let target_id = target.unwrap_or(player.spawn_id);
            let request = eq_client_core::inventory::ItemUse {
                request_id: self
                    .next_use_id
                    .checked_add(1)
                    .context("Item-use request IDs exhausted")?,
                session_id,
                revision: self.data.revision(),
                slot,
                target_id,
                created: now,
            };
            let available = target_id == player.spawn_id
                || online
                    .spawns
                    .get(&target_id)
                    .is_some_and(|spawn| !spawn.invisible);
            self.data
                .prepare_item_cast(&request, session_id, player.level, available, now)?;
            let request_id = request.request_id;
            sender
                .0
                .as_ref()
                .context("Network worker unavailable")?
                .try_send(ClientCommand::UseItem(request))
                .context("Item-use request could not be queued")?;
            self.actions.last_use = Some(now);
            self.next_use_id = request_id;
            self.actions.pending_use = Some((session_id, request_id));
            Ok(())
        })();
        self.actions.message = result.map_or_else(
            |error| error.to_string(),
            |()| "Item use queued; waiting for server cast updates".into(),
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
    pub fn action_result(&mut self, session_id: u64, revision: u64, error: Option<String>) {
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
            if to_cursor || self.data.items().contains_key(&InventorySlot(30)) {
                "Item is on the cursor / choose a destination".into()
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
        sender: &CommandsToServer,
    ) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let result = self.try_click(slot, split.then_some(NonZeroU32::MIN), online, sender);
        if let Err(error) = result {
            self.actions.message = error.to_string();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Opens a quantity picker bound to the current inventory revision.
    pub(super) fn select_split(&mut self, slot: InventorySlot) {
        self.actions.auto_store = false;
        self.actions.split = None;
        let count = self
            .data
            .items()
            .get(&slot)
            .and_then(|item| item.stack_count);
        if self.actions.pending.is_some()
            || self.data.stale()
            || self.data.items().contains_key(&InventorySlot(30))
        {
            self.actions.message = "Finish the current cursor move first".into();
        } else if let Some(available) = count.filter(|count| *count > 1) {
            self.actions.split = Some(SplitSelection {
                slot,
                revision: self.data.revision(),
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
        sender: &CommandsToServer,
    ) {
        let Some(mut selection) = self.actions.split.take() else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        if selection.revision != self.data.revision() {
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
                if let Err(error) = self.try_click(
                    selection.slot,
                    NonZeroU32::new(selection.amount),
                    online,
                    sender,
                ) {
                    self.actions.message = error.to_string();
                }
                return;
            }
        }
        self.actions.split = Some(selection);
    }

    /// Advances automatic storage by one validated move after the previous result arrives.
    pub(super) fn store_cursor(&mut self, online: &OnlineState, sender: &CommandsToServer) {
        if !self.actions.auto_store || self.actions.pending.is_some() {
            return;
        }
        if !self.data.items().contains_key(&InventorySlot(30)) {
            self.actions.auto_store = false;
            return;
        }
        let result = self.actor(online).and_then(|(_, actor)| {
            let destination = self.data.auto_store_destination(actor)?;
            self.try_click(destination, None, online, sender)
        });
        if let Err(error) = result {
            self.actions.auto_store = false;
            self.actions.message = error.to_string();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn actor(&self, online: &OnlineState) -> anyhow::Result<(u64, InventoryActor)> {
        use anyhow::Context;
        if self.demo {
            return Ok((
                0,
                InventoryActor {
                    bank_access: self.bank_open,
                    dual_wield: None,
                    deity: None,
                    class: Some(1),
                    race: 1,
                    level: 60,
                },
            ));
        }
        let player = online
            .player
            .as_ref()
            .context("Character data is unavailable")?;
        Ok((
            online.session_id.context("No active admission")?,
            InventoryActor {
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
            },
        ))
    }

    #[allow(clippy::too_many_lines)] // One transactional path keeps validation and submission adjacent.
    fn try_click(
        &mut self,
        slot: InventorySlot,
        split: Option<NonZeroU32>,
        online: &OnlineState,
        sender: &CommandsToServer,
    ) -> anyhow::Result<()> {
        const CURSOR: InventorySlot = InventorySlot(30);
        use anyhow::{Context, ensure};
        ensure!(
            self.actions.pending.is_none(),
            "Waiting for the queued move; it will not be retried"
        );
        ensure!(
            self.demo || (online.connected && online.death.is_none()),
            "Connect to move items"
        );
        ensure!(
            self.data.received() && !self.data.stale(),
            "Wait for a current inventory snapshot"
        );
        ensure!(
            matches!(slot.0,0..=30|251..=330) || (self.bank_open && slot.is_personal_bank()),
            "This slot is view only"
        );
        let cursor_item = self.data.items().get(&CURSOR);
        let (from, to, quantity, to_cursor) = if let Some(item) = cursor_item {
            ensure!(slot != CURSOR, "Choose a destination for the cursor item");
            let quantity = if let Some(destination) =
                self.data.items().get(&slot).filter(|other| {
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
            (CURSOR, slot, quantity, false)
        } else {
            ensure!(
                slot != CURSOR,
                "The cursor is empty; choose an item to pick up"
            );
            let item = self
                .data
                .items()
                .get(&slot)
                .context("Choose an item to pick up")?;
            self.actions.message = format!("Picking up {}", item.details.name);
            (
                slot,
                CURSOR,
                split.map_or(MoveQuantity::Whole, MoveQuantity::Count),
                true,
            )
        };
        let revision = self.data.revision();
        let (session_id, actor) = self.actor(online)?;
        let request = InventoryMove {
            session_id,
            revision,
            from,
            to,
            created: Instant::now(),
            quantity,
        };
        self.data.plan_move(&request, actor)?;
        if self.demo {
            self.data
                .submit_move(&request, session_id, actor, Instant::now(), |_| Ok(()))?;
            self.actions.message = if self.data.items().contains_key(&CURSOR) {
                "Item is on the cursor / choose a destination".into()
            } else {
                "Item placed locally / offline demo".into()
            };
        } else {
            sender
                .0
                .as_ref()
                .context("Network worker is unavailable")?
                .try_send(ClientCommand::MoveInventory(request))
                .context("Move could not be queued")?;
            self.actions.pending = Some((session_id, revision, to_cursor));
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
    use eq_client_core::inventory::InventoryUpdate;

    fn demo() -> InventoryState {
        let mut state = InventoryState::default();
        state.apply(InventoryUpdate::Snapshot(
            super::super::demo_items()
                .into_iter()
                .filter(|item| item.slot != InventorySlot(30))
                .collect(),
        ));
        state.demo = true;
        state
    }
    fn click(state: &mut InventoryState, slot: i32, split: bool) {
        state.click_slot(
            InventorySlot(slot),
            split,
            &OnlineState::new(false),
            &CommandsToServer(None),
        );
    }
    #[test]
    fn item_use_queues_current_instance_without_consuming_it_and_rejects_duplicate_or_busy_actions()
    {
        use eq_client_core::inventory::{ClickEffect, ClickKind, ItemActivation};
        let mut state = demo();
        let mut item = state.data.items().values().next().unwrap().clone();
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
        state.apply(InventoryUpdate::Snapshot(vec![item]));
        let before = state.data.clone();
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(9);
        online.player = Some(test_player());
        let own_id = online.player.as_ref().unwrap().spawn_id;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let sender = CommandsToServer(Some(sender));
        state.use_slot(InventorySlot(13), &online, &sender, None, true);
        assert!(receiver.try_recv().is_err());
        state.use_slot(InventorySlot(13), &online, &sender, Some(65500), false);
        assert!(receiver.try_recv().is_err());
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        let ClientCommand::UseItem(request) = receiver.try_recv().unwrap() else {
            panic!("expected item use")
        };
        assert_eq!(request.slot, InventorySlot(13));
        assert_eq!(request.session_id, 9);
        assert_eq!(request.revision, before.revision());
        assert_eq!(request.target_id, own_id);
        assert_eq!(state.data, before);
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        // Elapsing the rate limit cannot replace a request awaiting worker feedback.
        state.actions.last_use = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.actions.pending_use, Some((9, request.request_id)));
        state.item_use_result(8, request.request_id, None);
        state.item_use_result(9, request.request_id + 1, None);
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        state.item_use_result(9, request.request_id, Some("Synthetic rejection".into()));
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        let ClientCommand::UseItem(next) = receiver.try_recv().unwrap() else {
            panic!("expected a fresh manual item-use request")
        };
        assert_eq!(next.request_id, request.request_id + 1);
        state.cancel_actions();
        state.use_slot(
            InventorySlot(13),
            &online,
            &CommandsToServer(None),
            None,
            false,
        );
        assert!(state.actions.last_use.is_none());
        online.connected = false;
        state.use_slot(InventorySlot(13), &online, &sender, None, false);
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.data, before);
    }

    #[test]
    fn item_use_feedback_ignores_old_replies_and_never_claims_effect_success() {
        let mut state = demo();
        let before = state.data.clone();
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
        assert!(state.actions.message.is_empty());
        assert_eq!(state.next_use_id, 4);
        assert_eq!(state.data, before);
    }

    #[test]
    fn quantity_picker_moves_selected_count_and_rejects_stale_selections() {
        let mut state = demo();
        let online = OnlineState::new(false);
        let sender = CommandsToServer(None);
        state.select_split(InventorySlot(251));
        state.split_action(SplitAction::More, &online, &sender);
        state.split_action(SplitAction::More, &online, &sender);
        state.split_action(SplitAction::Confirm, &online, &sender);
        assert_eq!(state.data.items()[&InventorySlot(30)].stack_count, Some(3));
        assert_eq!(
            state.data.items()[&InventorySlot(251)].stack_count,
            Some(17)
        );
        assert!(state.actions.split.is_none());
        click(&mut state, 251, false);
        state.select_split(InventorySlot(251));
        state.split_action(SplitAction::Maximum, &online, &sender);
        assert_eq!(state.actions.split.as_ref().unwrap().amount, 20);
        state.split_action(SplitAction::More, &online, &sender);
        assert_eq!(state.actions.split.as_ref().unwrap().amount, 20);
        state.split_action(SplitAction::Minimum, &online, &sender);
        state.split_action(SplitAction::Less, &online, &sender);
        assert_eq!(state.actions.split.as_ref().unwrap().amount, 1);
        state.apply(InventoryUpdate::Invalidated);
        let before = state.data.clone();
        state.split_action(SplitAction::Confirm, &online, &sender);
        assert_eq!(state.data, before);
        assert!(state.actions.split.is_none());
    }

    #[test]
    fn automatic_storage_merges_then_places_remainder_and_stops() {
        let mut state = demo();
        let mut cursor = state.data.items()[&InventorySlot(251)].clone();
        cursor.slot = InventorySlot(30);
        cursor.stack_count = Some(5);
        let mut destination = state.data.items()[&InventorySlot(251)].clone();
        destination.stack_count = Some(18);
        state.apply(InventoryUpdate::Set(vec![cursor, destination]));
        state.actions.auto_store = true;
        let online = OnlineState::new(false);
        let sender = CommandsToServer(None);
        state.store_cursor(&online, &sender);
        assert_eq!(state.data.items()[&InventorySlot(30)].stack_count, Some(3));
        state.store_cursor(&online, &sender);
        assert!(!state.data.items().contains_key(&InventorySlot(30)));
        assert_eq!(state.data.items()[&InventorySlot(24)].stack_count, Some(3));
        state.store_cursor(&online, &sender);
        assert!(!state.actions.auto_store);
    }

    #[test]
    fn automatic_storage_waits_for_worker_and_stops_on_rejection() {
        let mut state = demo();
        click(&mut state, 251, false);
        state.demo = false;
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(7);
        online.player = Some(test_player());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = CommandsToServer(Some(tx));
        state.actions.auto_store = true;
        state.store_cursor(&online, &sender);
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("wrong command")
        };
        state.store_cursor(&online, &sender);
        assert!(rx.try_recv().is_err());
        state.action_result(7, request.revision, Some("Rejected".into()));
        state.store_cursor(&online, &sender);
        assert!(!state.actions.auto_store);
        assert!(rx.try_recv().is_err());
        assert!(state.data.items().contains_key(&InventorySlot(30)));
    }

    #[test]
    fn clicking_matching_stack_merges_the_cursor_back_into_it() {
        let mut state = demo();
        click(&mut state, 251, true);
        assert_eq!(state.data.items()[&InventorySlot(30)].stack_count, Some(1));
        click(&mut state, 251, false);
        assert!(!state.data.items().contains_key(&InventorySlot(30)));
        assert_eq!(
            state.data.items()[&InventorySlot(251)].stack_count,
            Some(20)
        );
    }
    #[test]
    fn merging_fills_destination_and_leaves_excess_on_cursor() {
        let mut state = demo();
        let mut cursor = state.data.items()[&InventorySlot(251)].clone();
        cursor.slot = InventorySlot(30);
        cursor.stack_count = Some(5);
        let mut destination = state.data.items()[&InventorySlot(251)].clone();
        destination.stack_count = Some(18);
        state.apply(InventoryUpdate::Set(vec![cursor, destination]));
        click(&mut state, 251, false);
        assert_eq!(state.data.items()[&InventorySlot(30)].stack_count, Some(3));
        assert_eq!(
            state.data.items()[&InventorySlot(251)].stack_count,
            Some(20)
        );
        assert!(state.actions.message.contains("on the cursor"));
        let before = state.data.clone();
        click(&mut state, 251, false);
        assert_eq!(state.data, before);
        assert!(state.actions.message.contains("full"));
    }
    #[test]
    fn demo_moves_filled_bags_splits_stacks_and_equips_using_the_shared_validator() {
        let mut state = demo();
        click(&mut state, 22, false);
        click(&mut state, 24, false);
        assert!(state.data.items().contains_key(&InventorySlot(271)));
        assert!(!state.data.items().contains_key(&InventorySlot(251)));
        click(&mut state, 271, true);
        click(&mut state, 25, false);
        assert_eq!(
            state.data.items()[&InventorySlot(271)].stack_count,
            Some(19)
        );
        assert_eq!(state.data.items()[&InventorySlot(25)].stack_count, Some(1));
        click(&mut state, 13, false);
        click(&mut state, 22, false);
        assert!(!state.data.items().contains_key(&InventorySlot(13)));
        click(&mut state, 22, false);
        click(&mut state, 13, false);
        assert_eq!(
            state.data.items()[&InventorySlot(13)].details.name,
            "Preview sword"
        );
        assert!(state.actions.pending.is_none());
    }
    #[test]
    fn updates_preserve_the_real_cursor_and_view_only_slots_never_move() {
        let mut state = demo();
        click(&mut state, 251, false);
        state.apply(InventoryUpdate::Set(vec![
            super::super::demo_items()[0].clone(),
        ]));
        assert!(state.data.items().contains_key(&InventorySlot(30)));
        click(&mut state, 24, false);
        assert!(state.data.items().contains_key(&InventorySlot(24)));
        let before = state.data.clone();
        click(&mut state, 2000, false);
        assert_eq!(state.data, before);
    }
    #[test]
    fn queue_submission_does_not_predict_or_confirm_and_pending_moves_cannot_duplicate() {
        let mut state = demo();
        state.demo = false;
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(7);
        online.player = Some(test_player());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = CommandsToServer(Some(tx));
        let before = state.data.clone();
        state.click_slot(InventorySlot(251), false, &online, &sender);
        state.click_slot(InventorySlot(24), false, &online, &sender);
        assert_eq!(state.data, before);
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("wrong command")
        };
        assert_eq!(request.from, InventorySlot(251));
        assert_eq!(request.to, InventorySlot(30));
        state.click_slot(InventorySlot(25), false, &online, &sender);
        assert!(rx.try_recv().is_err());
        state.action_result(8, request.revision, None);
        assert!(state.actions.pending.is_some());
        state.action_result(7, request.revision, Some("Rejected".into()));
        assert!(state.actions.pending.is_none());
        assert_eq!(state.data, before);
        assert_eq!(state.actions.message, "Rejected");
        online.connected = false;
        state.click_slot(InventorySlot(251), false, &online, &sender);
        assert!(state.actions.pending.is_none());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn banking_tracks_nearby_service_and_queues_cursor_moves_without_prediction() {
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(7);
        online.player = Some(test_player());
        online.spawns.insert(
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
            },
        );
        let mut state = InventoryState::default();
        state.apply(InventoryUpdate::Snapshot(
            super::super::demo_items()
                .into_iter()
                .filter(|item| item.slot != InventorySlot(30))
                .collect(),
        ));
        let before = state.data.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let sender = CommandsToServer(Some(tx));
        state.refresh_bank_access(&online);
        assert!(state.bank_open);
        state.click_slot(InventorySlot(2000), false, &online, &sender);
        assert!(
            matches!(rx.try_recv().unwrap(), ClientCommand::MoveInventory(request)
            if request.from == InventorySlot(2000) && request.to == InventorySlot(30))
        );
        assert_eq!(state.data, before);
        state.tab = super::super::Tab::Bank;
        online.player.as_mut().unwrap().position.x = 21.0;
        state.refresh_bank_access(&online);
        assert!(!state.bank_open);
        assert_eq!(state.tab, super::super::Tab::Inventory);
        // Closing the bank never discards a move already submitted to the worker.
        assert!(state.actions.pending.is_some());
        online.player.as_mut().unwrap().position.x = 0.0;
        state.refresh_bank_access(&online);
        assert!(state.bank_open);
        online.connected = false;
        state.refresh_bank_access(&online);
        assert!(!state.bank_open);
        online.connected = true;
        online.spawns.clear();
        state.refresh_bank_access(&online);
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
        }
    }
}

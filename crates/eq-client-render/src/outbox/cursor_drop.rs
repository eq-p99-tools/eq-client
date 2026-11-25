//! A local reservation before publishing a drop to the asynchronous worker.
use super::{ClientCommand, ClientWorld, Outbox, Refusal, SyncSender, TrySendError};
use eq_client_core::inventory::InventoryDrop;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(super) struct Pending {
    next: u64,
    held: Option<(u64, u64)>,
}

impl Pending {
    pub(super) fn in_admission(&self, session: Option<u64>) -> bool {
        self.held
            .is_some_and(|(_, admission)| Some(admission) == session)
    }
}

/// Keeps the local cursor lock until refusal or the matching worker result.
pub(crate) struct DropReceipt {
    pending: Arc<Mutex<Pending>>,
    id: u64,
}

impl Drop for DropReceipt {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock()
            && pending.held.is_some_and(|(id, _)| id == self.id)
        {
            pending.held = None;
        }
    }
}

/// Exactly one immutable command, publishable only after local presentation.
pub(crate) struct DropPublication {
    queue: SyncSender<ClientCommand>,
    command: ClientCommand,
    pending: Arc<Mutex<Pending>>,
    id: u64,
}

impl DropPublication {
    /// Publishes once after the caller has staged its local presentation.
    ///
    /// # Errors
    /// Refuses a withdrawn reservation, poisoned lock, full or closed queue.
    pub(crate) fn publish(self) -> Result<(), Refusal> {
        let pending = self.pending.lock().map_err(|_| Refusal::Busy)?;
        if pending.held.is_none_or(|(id, _)| id != self.id) {
            return Err(Refusal::Away);
        }
        self.queue
            .try_send(self.command)
            .map_err(|error| match error {
                TrySendError::Full(_) => Refusal::Busy,
                TrySendError::Disconnected(_) => Refusal::Ended,
            })
    }
}

impl Outbox {
    /// Validates queue/provider state and reserves cursor ownership without
    /// publishing. The caller stores the receipt and stages its local effects
    /// before publishing, dropping the receipt on a definitive queue refusal.
    ///
    /// # Errors
    /// Refuses unavailable admissions/providers, another cursor owner or an
    /// unusable local reservation. No command has been published on error.
    pub(crate) fn prepare_cursor_drop(
        &self,
        world: &ClientWorld,
        request: InventoryDrop,
    ) -> Result<(DropPublication, DropReceipt), Refusal> {
        self.reserve_cursor_drop(world, request)
            .inspect_err(|refusal| self.refused(*refusal))
    }

    fn reserve_cursor_drop(
        &self,
        world: &ClientWorld,
        request: InventoryDrop,
    ) -> Result<(DropPublication, DropReceipt), Refusal> {
        let session = request.session_id;
        if Some(session) != world.session_id() {
            return Err(Refusal::Away);
        }
        let command = ClientCommand::DropItem(Box::new(request));
        let queue = self.check(world, &command)?.clone();
        let mut pending = self.cursor_drop.lock().map_err(|_| Refusal::Busy)?;
        if pending.in_admission(world.session_id()) {
            return Err(Refusal::CursorDropPending);
        }
        let id = pending.next.checked_add(1).ok_or(Refusal::Busy)?;
        pending.next = id;
        pending.held = Some((id, session));
        Ok((
            DropPublication {
                queue,
                command,
                pending: self.cursor_drop.clone(),
                id,
            },
            DropReceipt {
                pending: self.cursor_drop.clone(),
                id,
            },
        ))
    }
}

pub(super) fn consumes_cursor(command: &ClientCommand) -> bool {
    match command {
        ClientCommand::MoveInventory(request) => {
            request.from == eq_client_core::inventory::InventorySlot::CURSOR
                || request.to == eq_client_core::inventory::InventorySlot::CURSOR
                || request.from.is_in_cursor_bag()
                || request.to.is_in_cursor_bag()
        }
        ClientCommand::DropItem(_)
        | ClientCommand::DestroyItem(_)
        | ClientCommand::OfferTrade { .. }
        | ClientCommand::DestroyCoins { .. }
        | ClientCommand::ScribeSpell { .. }
        | ClientCommand::PickUp { .. }
        | ClientCommand::LootItem { .. }
        | ClientCommand::Combine { .. } => true,
        ClientCommand::MoveCoins { transfer, .. } => {
            transfer.from == eq_client_core::money::CoinPlace::Cursor
                || transfer.to == eq_client_core::money::CoinPlace::Cursor
        }
        ClientCommand::UseItem(request) => {
            request.slot == eq_client_core::inventory::InventorySlot::CURSOR
        }
        _ => false,
    }
}

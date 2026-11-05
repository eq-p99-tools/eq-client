//! One owner of queued inventory projections over the ordered worker replica.
use super::{Changes, ClientWorld, Reply};
use crate::inventory::{
    Inventory, InventoryActor, InventoryMove, InventoryUpdate, MoveRules, MoveSequence,
};
use anyhow::{Context, Result, ensure};
use std::collections::VecDeque;

const MAX_PENDING: usize = 64;

struct Pending {
    sequence: MoveSequence,
    revision: u64,
    update: InventoryUpdate,
}

#[derive(Default)]
pub(super) struct Moves {
    rules: Option<MoveRules>,
    next_id: u64,
    pending: VecDeque<Pending>,
    pub(super) projected: Option<Inventory>,
    interrupted: bool,
    /// A root refusal was already offered for this still-draining queue.
    refused: bool,
}

impl Moves {
    /// Keep identities monotonic even if a reset resumes this same admission.
    pub(super) fn clear(&mut self) {
        *self = Self {
            next_id: self.next_id,
            rules: self.rules,
            ..Self::default()
        };
    }
    /// An intervening worker update discards the speculative overlay, not packets
    /// already handed to the worker. Their results still have to drain in order.
    pub(super) fn interrupt(&mut self) {
        self.projected = None;
        self.interrupted = !self.pending.is_empty();
    }
}

impl ClientWorld {
    /// Freezes one whole cursor instance for explicit destructive confirmation.
    ///
    /// # Errors
    /// Refuses unavailable providers/admissions, pending moves, uncertain state,
    /// populated containers and cursor queues.
    pub fn prepare_cursor_destruction(&self) -> Result<crate::inventory::InventoryDestroy> {
        let request = crate::inventory::InventoryDestroy {
            session_id: self.session_id().context("Enter the world first")?,
            revision: self.inventory().revision(),
            item: self
                .inventory()
                .items()
                .get(&crate::inventory::InventorySlot::CURSOR)
                .context("The cursor is empty")?
                .clone(),
            created: std::time::Instant::now(),
        };
        self.check_cursor_destruction(&request)?;
        Ok(request)
    }

    /// Revalidates the frozen admission, revision, whole quantity and identity.
    ///
    /// # Errors
    /// Changed inventory or session requires a new explicit confirmation.
    pub fn check_cursor_destruction(
        &self,
        request: &crate::inventory::InventoryDestroy,
    ) -> Result<()> {
        ensure!(
            self.in_world() && self.accepts_reply(request.session_id),
            "Inventory admission changed"
        );
        ensure!(
            !self.inventory_moves_pending(),
            "Finish the pending inventory move first"
        );
        let rules = self
            .inventory_moves
            .rules
            .context("Inventory policy is unavailable")?;
        self.inventory().plan_destroy(rules, request)?;
        Ok(())
    }

    /// Configures host placement from the network producer's inventory policy.
    /// Missing policy refuses moves; the installed generation is not a fallback.
    /// A changed policy discards the overlay while outstanding results drain.
    pub fn plan_inventory_as(&mut self, rules: Option<MoveRules>) {
        if self.inventory_moves.rules != rules {
            self.inventory_moves.interrupt();
            self.inventory_moves.rules = rules;
        }
    }

    /// Whether local move results remain outstanding. This is a prerequisite
    /// for operations such as scribing, never a gate between successive moves.
    #[must_use]
    pub fn inventory_moves_pending(&self) -> bool {
        !self.inventory_moves.pending.is_empty()
    }

    /// Validates against the current projection and commits it only when enqueue
    /// succeeds. The callback must enqueue exactly this request once, in order.
    /// No server receipt or successful delivery is inferred.
    ///
    /// # Errors
    /// Refuses stale admissions, interrupted sequences, excessive queued work,
    /// invalid placement and enqueue failures without changing the projection.
    pub fn queue_inventory_move(
        &mut self,
        mut request: InventoryMove,
        actor: InventoryActor,
        enqueue: impl FnOnce(&Self, &InventoryMove) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            self.in_world() && self.accepts_reply(request.session_id),
            "Inventory admission changed"
        );
        ensure!(
            !self.inventory_moves.interrupted,
            "Inventory changed; wait for queued move results, then select again"
        );
        ensure!(
            self.inventory_moves.pending.len() < MAX_PENDING,
            "Too many queued item moves"
        );
        let before = self.offering();
        let rules = self
            .inventory_moves
            .rules
            .context("Inventory placement policy is unavailable")?;
        let update = self.inventory().plan_move_for(rules, &request, actor)?;
        let id = self
            .inventory_moves
            .next_id
            .checked_add(1)
            .context("Item move identities exhausted")?;
        let sequence = MoveSequence {
            id,
            after: self
                .inventory_moves
                .pending
                .back()
                .map(|move_| move_.sequence.id),
        };
        request.sequence = Some(sequence);
        let mut projected = self.inventory().clone();
        projected.apply(update.clone());
        enqueue(self, &request)?;
        self.inventory_moves.next_id = id;
        self.inventory_moves.pending.push_back(Pending {
            sequence,
            revision: request.revision,
            update,
        });
        self.inventory_moves.projected = Some(projected);
        self.reconsider(&before);
        Ok(())
    }

    pub(super) fn inventory_move_result(
        &mut self,
        (session_id, revision, sequence): (u64, u64, Option<MoveSequence>),
        update: Option<&InventoryUpdate>,
        error: Option<&String>,
        changes: &mut Changes,
    ) {
        // A same-admission success can arrive after death/zoning/link reset.
        // Retained request identity, rather than current ability to act, owns it.
        if self.session_id != Some(session_id)
            || (sequence.is_none() && !self.accepts_reply(session_id))
            || error.is_some() == update.is_some()
        {
            changes.ignored = true;
            return;
        }
        let before = self.offering();
        let mut apply = error.is_none() && update.is_some();
        if matches!(update, Some(InventoryUpdate::DestroyCursor)) {
            // A late submission result cannot destroy a newer received cursor.
            apply &= self.inventory.revision() == revision;
        }
        if let Some(sequence) = sequence {
            let Some(pending) = self.inventory_moves.pending.front() else {
                changes.ignored = true;
                return;
            };
            if pending.sequence != sequence || pending.revision != revision {
                changes.ignored = true;
                return;
            }
            let matches = error.is_none()
                && update == Some(&pending.update)
                && self.inventory.revision() == revision;
            apply &= matches;
            self.inventory_moves.pending.pop_front();
            if !matches {
                self.inventory_moves.interrupt();
            }
        } else {
            self.inventory_moves.interrupt();
        }
        let present_error = error.is_none()
            || !self.inventory_moves.refused
            || sequence.is_none_or(|sequence| sequence.after.is_none());
        self.inventory_moves.refused |= error.is_some();
        if apply && let Some(update) = update {
            self.inventory.apply(update.clone());
        }
        if self.inventory_moves.pending.is_empty() {
            self.inventory_moves.projected = None;
            self.inventory_moves.interrupted = false;
            self.inventory_moves.refused = false;
        }
        changes.inventory = true;
        self.refresh_item_hp();
        self.reconsider(&before);
        changes.replies.push(Reply::InventoryMove {
            session_id,
            revision,
            error: error.cloned(),
            present_error,
        });
    }
}

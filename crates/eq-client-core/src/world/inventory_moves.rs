//! One owner of queued inventory projections over the ordered worker replica.
use super::{Changes, ClientWorld, Reply};
use crate::inventory::{Inventory, InventoryActor, InventoryMove, InventoryUpdate, MoveSequence};
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
    next_id: u64,
    pending: VecDeque<Pending>,
    pub(super) projected: Option<Inventory>,
    interrupted: bool,
}

impl Moves {
    /// Keep identities monotonic even if a reset resumes this same admission.
    pub(super) fn clear(&mut self) {
        *self = Self {
            next_id: self.next_id,
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
        let update = self.inventory().plan_move(&request, actor)?;
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
        if !self.accepts_reply(session_id) {
            changes.ignored = true;
            return;
        }
        let before = self.offering();
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
            self.inventory_moves.pending.pop_front();
            if !matches {
                self.inventory_moves.interrupt();
            }
        } else {
            self.inventory_moves.interrupt();
        }
        if error.is_none()
            && let Some(update) = update
        {
            self.inventory.apply(update.clone());
        }
        if self.inventory_moves.pending.is_empty() {
            self.inventory_moves.projected = None;
            self.inventory_moves.interrupted = false;
        }
        changes.inventory = true;
        self.refresh_item_hp();
        self.reconsider(&before);
        changes.replies.push(Reply::InventoryMove {
            session_id,
            revision,
            error: error.cloned(),
        });
    }
}

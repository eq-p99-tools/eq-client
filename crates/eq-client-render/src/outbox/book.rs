//! Local arbitration for the worker's untagged spellbook results.
use super::Refusal;
use eq_client_core::{BookActionStatus, ClientCommand, SpellUpdate, WorldEvent};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Local overdue threshold, including any preparation before submission.
/// The worker releases its hold and reports Unconfirmed at its 30-second deadline.
pub(crate) const BOOK_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Default)]
pub(super) struct BookQueue {
    pending: Option<Pending>,
    pub(super) applying: bool,
    pub(super) interrupted: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Expected {
    Forget(u8, u32),
    Slot { slot: u32, spell: u32, mode: u32 },
    Delete(u16),
    Swap(u16, u16),
}

impl Expected {
    pub(super) fn from_command(command: &ClientCommand) -> Option<Self> {
        Some(match *command {
            ClientCommand::ForgetSpell { gem, spell_id, .. } => Self::Forget(gem, spell_id),
            ClientCommand::MemorizeSpell { gem, spell_id, .. } => Self::Slot {
                slot: u32::from(gem),
                spell: spell_id,
                mode: 1,
            },
            ClientCommand::ScribeSpell { slot, spell_id, .. } => Self::Slot {
                slot: u32::from(slot),
                spell: spell_id,
                mode: 0,
            },
            ClientCommand::DeleteSpell { slot, .. } => Self::Delete(slot),
            ClientCommand::SwapSpell { from, to, .. } => Self::Swap(from, to),
            _ => return None,
        })
    }

    fn matches(self, update: &SpellUpdate) -> bool {
        match (self, update) {
            (Self::Forget(gem, spell), update) => update.clears_gem(u32::from(gem), spell),
            (
                Self::Slot { slot, spell, mode },
                SpellUpdate::Slot {
                    slot: received,
                    spell_id,
                    mode: received_mode,
                },
            ) => (slot, spell, mode) == (*received, *spell_id, *received_mode),
            (
                Self::Delete(slot),
                SpellUpdate::BookDeletion {
                    slot: received,
                    success: true,
                },
            ) => slot == *received,
            (
                Self::Swap(from, to),
                SpellUpdate::BookSwap {
                    from: first,
                    to: second,
                },
            ) => (from, to) == (*first, *second),
            _ => false,
        }
    }
}

/// Keeps one edit's progress and answer for its callers after the book is free.
#[derive(Clone, Debug)]
pub(crate) struct BookEditReceipt {
    session: u64,
    status: Arc<Mutex<Option<BookActionStatus>>>,
}

impl BookEditReceipt {
    pub(super) fn new(session: u64) -> Self {
        Self {
            session,
            status: Arc::default(),
        }
    }

    pub(crate) fn belongs_to(&self, session: Option<u64>) -> bool {
        session == Some(self.session)
    }

    pub(crate) fn status(&self) -> Option<BookActionStatus> {
        self.status.lock().ok()?.clone()
    }

    fn publish(&self, status: BookActionStatus) {
        if let Ok(mut latest) = self.status.lock() {
            *latest = Some(status);
        }
    }
}

struct Pending {
    expected: Expected,
    session: Option<u64>,
    since: Instant,
    unconfirmed: bool,
    acknowledged: bool,
    observed: bool,
    receipt: Option<BookEditReceipt>,
}

impl BookQueue {
    pub(super) fn can_edit(
        &mut self,
        session: Option<u64>,
        set_step: bool,
        now: Instant,
    ) -> Result<(), Refusal> {
        if self.applying != set_step {
            return Err(Refusal::SpellSet);
        }
        self.pending.take_if(|pending| pending.session != session);
        match &self.pending {
            Some(pending)
                if pending.unconfirmed
                    || now.saturating_duration_since(pending.since) >= BOOK_TIMEOUT =>
            {
                Err(Refusal::BookUnconfirmed)
            }
            Some(_) => Err(Refusal::BookPending),
            None => Ok(()),
        }
    }

    pub(super) fn sent(
        &mut self,
        expected: Expected,
        session: Option<u64>,
        now: Instant,
        receipt: Option<BookEditReceipt>,
    ) {
        self.pending = Some(Pending {
            expected,
            session,
            since: now,
            unconfirmed: false,
            acknowledged: false,
            observed: false,
            receipt,
        });
    }

    pub(super) fn observe(&mut self, event: &WorldEvent, session: Option<u64>) {
        let Some(pending) = self
            .pending
            .as_mut()
            .filter(|pending| pending.session == session)
        else {
            return;
        };
        if let Some(outcome) = pending.observe(event) {
            if let Some(receipt) = pending.receipt.take() {
                receipt.publish(outcome);
            }
            self.pending = None;
        }
    }

    /// Mark a possibly submitted request unknown without dropping its reply match.
    /// Returns whether a manual edit has newly become unconfirmed.
    pub(super) fn interrupt(&mut self, session: Option<u64>) -> bool {
        let Some(pending) = self
            .pending
            .as_mut()
            .filter(|pending| pending.session == session)
        else {
            return false;
        };
        let was_unconfirmed = pending.unconfirmed;
        pending.progress(&BookActionStatus::Unconfirmed);
        !self.applying && !was_unconfirmed
    }

    pub(super) fn reset(&mut self) {
        self.pending = None;
    }
}

impl Pending {
    fn progress(&mut self, status: &BookActionStatus) {
        if matches!(status, BookActionStatus::Unconfirmed) {
            self.unconfirmed = true;
        }
        if let Some(receipt) = &self.receipt {
            let status = if self.unconfirmed {
                BookActionStatus::Unconfirmed
            } else {
                status.clone()
            };
            receipt.publish(status);
        }
    }

    /// The worker's tracked confirmation can arrive before the world update.
    /// Require both before releasing a confirmed edit for its caller.
    fn observe(&mut self, event: &WorldEvent) -> Option<BookActionStatus> {
        match event {
            WorldEvent::BookAction(
                status @ (BookActionStatus::Preparing
                | BookActionStatus::Submitted
                | BookActionStatus::AwaitingReply),
            ) => {
                self.progress(status);
            }
            WorldEvent::BookAction(BookActionStatus::Confirmed) => {
                self.acknowledged = true;
            }
            WorldEvent::BookAction(
                status @ (BookActionStatus::Rejected(_)
                | BookActionStatus::Cancelled(_)
                | BookActionStatus::Unconfirmed),
            ) => {
                return Some(status.clone());
            }
            WorldEvent::Spell(update) => {
                self.observed |= self.expected.matches(update);
            }
            _ => {}
        }
        (self.acknowledged && self.observed).then_some(BookActionStatus::Confirmed)
    }
}

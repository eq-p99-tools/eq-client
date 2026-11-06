//! Local arbitration for the worker's untagged spellbook results.
use super::Refusal;
use eq_client_core::{BookActionStatus, ClientCommand, SpellUpdate, WorldEvent};
use std::time::{Duration, Instant};

/// Covers normal preparation and the worker's reply window. A late result
/// can still change the gems, so elapsed time does not release the book.
pub(crate) const BOOK_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Default)]
pub(super) struct BookQueue {
    pending: Option<Pending>,
    pub(super) applying: bool,
    pub(super) interrupted: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Expected {
    Forget(u8),
    Slot { slot: u32, spell: u32, mode: u32 },
    Delete(u16),
    Swap(u16, u16),
}

impl Expected {
    pub(super) fn from_command(command: &ClientCommand) -> Option<Self> {
        Some(match *command {
            ClientCommand::ForgetSpell { gem, .. } => Self::Forget(gem),
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
            (Self::Forget(gem), SpellUpdate::Slot { slot, mode: 2, .. }) => u32::from(gem) == *slot,
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

struct Pending {
    expected: Expected,
    session: Option<u64>,
    since: Instant,
    acknowledged: bool,
    observed: bool,
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
            Some(pending) if now.saturating_duration_since(pending.since) >= BOOK_TIMEOUT => {
                Err(Refusal::BookUnconfirmed)
            }
            Some(_) => Err(Refusal::BookPending),
            None => Ok(()),
        }
    }

    pub(super) fn sent(&mut self, expected: Expected, session: Option<u64>, now: Instant) {
        self.pending = Some(Pending {
            expected,
            session,
            since: now,
            acknowledged: false,
            observed: false,
        });
    }

    pub(super) fn observe(&mut self, event: &WorldEvent, session: Option<u64>) {
        self.pending
            .take_if(|pending| pending.session == session && pending.observe(event));
    }

    pub(super) fn reset(&mut self) {
        self.pending = None;
    }
}

impl Pending {
    /// The worker's tracked confirmation can arrive before the corresponding
    /// world update. A forget has only a submission and an exact gem clear.
    fn observe(&mut self, event: &WorldEvent) -> bool {
        let forgetting = matches!(self.expected, Expected::Forget(_));
        match event {
            WorldEvent::BookAction(BookActionStatus::Submitted) if forgetting => {
                self.acknowledged = true;
            }
            WorldEvent::BookAction(BookActionStatus::Confirmed) if !forgetting => {
                self.acknowledged = true;
            }
            WorldEvent::BookAction(
                BookActionStatus::Rejected(_) | BookActionStatus::Cancelled(_),
            ) if !forgetting || !self.acknowledged => {
                return true;
            }
            WorldEvent::Spell(update) if (!forgetting || self.acknowledged) => {
                self.observed |= self.expected.matches(update);
            }
            _ => {}
        }
        self.acknowledged && self.observed
    }
}

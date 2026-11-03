//! Producer-bound spell-table facts crossing the worker/presentation boundary.
pub use eq_network_game::spell_tables::{
    Assignment, Context, Event, SpellId, Stage, Table, Update,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// Which selected server policy may use this provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Policy {
    /// Installed definitions without P99's replay policy.
    Installed,
    /// P99's source snapshots and zone replay.
    Project1999,
}

/// An opaque provider lifetime, allocated when the actual producer is created.
/// A queued event retains this binding even after the active producer changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    epoch: u64,
    policy: Policy,
}

impl Binding {
    /// Creates a new provider epoch. Reuse the returned binding across network
    /// attempts belonging to the same logical producer/provider lifetime.
    ///
    /// # Panics
    /// Panics if the process exhausts all provider epoch identifiers.
    pub fn new(policy: Policy) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let epoch = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("spell provider epochs exhausted");
        Self { epoch, policy }
    }

    /// The policy selected at producer creation.
    pub const fn policy(self) -> Policy {
        self.policy
    }

    /// Binds one producer fact before it is enqueued. Consumers must never
    /// replace this binding with their own current epoch while draining.
    pub const fn envelope(self, fact: Fact) -> Envelope {
        Envelope {
            binding: self,
            fact,
        }
    }
}

/// A lifecycle fact from the actual producer, or a complete network table.
#[derive(Clone, Debug)]
pub enum Fact {
    /// A network attempt actually started, as reported by its Connecting status.
    /// A table carrying another run ID cannot establish this transition itself.
    RunStarted {
        /// Actual network attempt identity, shared with table contexts.
        run_id: u64,
        /// Monotonic start number assigned by this producer before enqueue.
        attempt: u64,
    },
    /// A real zone connection is beginning; source snapshots remain intact.
    ZonePending {
        /// The actual started network attempt.
        run_id: u64,
    },
    /// Admission confirms which zone the pre-profile replay belongs to.
    ZoneEntered {
        /// The actual started network attempt.
        run_id: u64,
        /// The admitted zone identity.
        session_id: u64,
    },
    /// A complete table or typed network failure, in receive order.
    Table(Event),
}

/// An immutable producer binding attached before enqueue.
#[derive(Clone, Debug)]
pub struct Envelope {
    binding: Binding,
    fact: Fact,
}

impl Envelope {
    /// The provider and policy selected by the originating producer.
    pub const fn binding(&self) -> Binding {
        self.binding
    }

    /// The fact retained with its original producer binding.
    pub const fn fact(&self) -> &Fact {
        &self.fact
    }
}

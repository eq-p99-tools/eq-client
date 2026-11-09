//! Installed-provider lifetime and transactional P99 replay, independent of zones.
use eq_client_assets::spells::{Definition, Definitions, ReplayProjection};
use eq_client_core::spell_tables::{Binding, Envelope, Event, Fact, SpellId, Stage, Table, Update};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};

/// Worker-side view of the same spell-provider rules used by presentation.
/// The session delivers each provider fact before the worker handles further
/// zone input. The worker owns buff membership independently of these assets.
pub struct SightProvider(Mutex<Provider>);

impl SightProvider {
    /// Read the installed Titanium spell file for this provider binding.
    pub fn load(binding: Binding, directory: &Path) -> Self {
        let definitions = Definitions::read(directory, &eq_client_assets::spells::Layout::TITANIUM)
            .unwrap_or_default();
        Self::from_definitions(binding, definitions.into())
    }

    /// Start an independent session provider over the launch's installed spells.
    pub fn from_definitions(binding: Binding, definitions: Arc<Definitions>) -> Self {
        Self(Mutex::new(Provider::new(binding, definitions)))
    }

    /// Apply the same run, admission and table ordering as the renderer.
    pub fn receive(&self, envelope: &Envelope) {
        if let Ok(mut provider) = self.0.lock() {
            provider.receive(envelope);
        }
    }
}

impl std::fmt::Debug for SightProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SightProvider")
            .finish_non_exhaustive()
    }
}

impl eq_client_core::vision::SightCatalog for SightProvider {
    fn sees_invisible(&self, spell_id: u32, caster_level: u16, landed: bool) -> bool {
        let Ok(provider) = self.0.lock() else {
            return false;
        };
        provider
            .get(spell_id)
            .and_then(|spell| spell.mechanics.as_ref())
            .is_some_and(|mechanics| {
                (!landed
                    || !matches!(
                        mechanics.base_duration(caster_level),
                        None | Some(eq_client_assets::spells::BaseDuration::Ticks(0))
                    ))
                    && mechanics.effects.iter().any(|effect| {
                        effect.id == 13
                            && effect
                                .base_magnitude(caster_level)
                                .is_some_and(|value| value > 0)
                    })
            })
    }
}

/// Mechanics availability for the current provider and zone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Readiness {
    Installed,
    AwaitingRegistration,
    AwaitingReplay,
    Ready { session_id: u64 },
    Unavailable(Failure),
}

/// A whole-provider failure is sticky until explicit replacement. This is the
/// host's conservative recovery policy, not a claim about native cache teardown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    Network,
    RunIdentity,
    Order,
    Stage,
    RegistrationMissing,
    SourceMissing(SpellId),
    DestinationMissing(SpellId),
}

pub(super) struct Provider {
    binding: Binding,
    base: Arc<Definitions>,
    effective: BTreeMap<u32, Definition>,
    snapshots: BTreeMap<SpellId, ReplayProjection>,
    registered: bool,
    readiness: Readiness,
    attempt: u64,
    run_id: Option<u64>,
    revision: u64,
    zone: Option<u64>,
    seen_zones: BTreeSet<u64>,
    zone_pending: bool,
}

impl Provider {
    pub(super) fn new(binding: Binding, base: Arc<Definitions>) -> Self {
        let readiness = if binding.policy().requires_server_tables() {
            Readiness::AwaitingRegistration
        } else {
            Readiness::Installed
        };
        Self {
            binding,
            base,
            effective: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            registered: false,
            readiness,
            attempt: 0,
            run_id: None,
            revision: 0,
            zone: None,
            seen_zones: BTreeSet::new(),
            zone_pending: false,
        }
    }

    pub(super) fn bind(&mut self, binding: Binding) {
        if self.binding != binding {
            *self = Self::new(binding, self.base.clone());
        }
    }

    pub(super) const fn readiness(&self) -> &Readiness {
        &self.readiness
    }

    pub(super) fn ready(&self) -> bool {
        matches!(
            self.readiness,
            Readiness::Installed | Readiness::Ready { .. }
        )
    }

    /// Labels stay useful while mechanics await a replay. Replay never changes
    /// these destination-owned strings.
    pub(super) fn text(&self, id: u32) -> Option<&Definition> {
        self.base.get(id)
    }

    pub(super) fn get(&self, id: u32) -> Option<&Definition> {
        if self.ready() { self.current(id) } else { None }
    }

    fn current(&self, id: u32) -> Option<&Definition> {
        self.effective.get(&id).or_else(|| self.base.get(id))
    }

    fn fail(&mut self, reason: Failure) {
        if !matches!(self.readiness, Readiness::Unavailable(_)) {
            self.readiness = Readiness::Unavailable(reason);
        }
    }

    fn wait(&mut self) {
        if !matches!(self.readiness, Readiness::Unavailable(_)) {
            self.readiness = if self.registered {
                Readiness::AwaitingReplay
            } else {
                Readiness::AwaitingRegistration
            };
        }
    }

    /// Receives the producer's original binding; stale producers cannot retag
    /// their queued Connecting or table facts as the replacement provider.
    /// Whether this fact first made the provider unavailable. Sticky failures
    /// do not repeat the transition; explicit replacement rearms it.
    pub(super) fn receive(&mut self, envelope: &Envelope) -> bool {
        let was_unavailable = matches!(self.readiness, Readiness::Unavailable(_));
        self.apply_event(envelope);
        !was_unavailable && matches!(self.readiness, Readiness::Unavailable(_))
    }

    fn apply_event(&mut self, envelope: &Envelope) {
        if envelope.binding() != self.binding || !self.binding.policy().requires_server_tables() {
            return;
        }
        match envelope.fact() {
            Fact::RunUnavailable { attempt } => {
                if *attempt <= self.attempt {
                    return;
                }
                self.attempt = *attempt;
                self.run_id = None;
                self.revision = 0;
                self.zone = None;
                self.zone_pending = false;
                self.fail(Failure::RunIdentity);
            }
            Fact::RunStarted { run_id, attempt } => {
                if *attempt <= self.attempt {
                    return;
                }
                if self.attempt.checked_add(1) != Some(*attempt) {
                    self.fail(Failure::Order);
                }
                self.attempt = *attempt;
                self.run_id = Some(*run_id);
                self.revision = 0;
                self.zone = None;
                self.zone_pending = false;
                self.wait();
            }
            Fact::ZonePending { run_id } if self.run_id == Some(*run_id) => {
                self.zone = None;
                self.zone_pending = true;
                self.wait();
            }
            Fact::ZoneEntered { run_id, session_id } if self.run_id == Some(*run_id) => {
                if self.zone != Some(*session_id) {
                    // Admission without a replay remains explicitly waiting.
                    if self.zone.is_some() {
                        self.fail(Failure::Stage);
                    }
                    self.zone = Some(*session_id);
                    self.seen_zones.insert(*session_id);
                    self.zone_pending = false;
                    self.wait();
                }
            }
            Fact::Table(event) => self.table(event),
            Fact::ZonePending { .. } | Fact::ZoneEntered { .. } => (),
        }
    }

    fn table(&mut self, event: &Event) {
        let context = event.context;
        // Only a real producer start may establish or change the accepted run.
        if self.run_id != Some(context.run_id) {
            return;
        }
        if context.revision <= self.revision {
            return;
        }
        if self.revision.checked_add(1) != Some(context.revision) {
            self.fail(Failure::Order);
            return;
        }
        self.revision = context.revision;
        if matches!(self.readiness, Readiness::Unavailable(_)) {
            return;
        }
        match (&event.update, context.stage) {
            (Update::Table(Table::Registration(ids)), Stage::World) => self.register(ids),
            (Update::Table(Table::Replay(assignments)), Stage::Zone { session_id }) => {
                if self.zone != Some(session_id) {
                    if !self.zone_pending || self.seen_zones.contains(&session_id) {
                        self.fail(Failure::Stage);
                        return;
                    }
                    self.zone = Some(session_id);
                    self.seen_zones.insert(session_id);
                    self.zone_pending = false;
                }
                self.replay(assignments, session_id);
            }
            (Update::Unavailable { .. }, _) => self.fail(Failure::Network),
            _ => self.fail(Failure::Stage),
        }
    }

    fn register(&mut self, ids: &[SpellId]) {
        let mut staged = BTreeMap::new();
        for id in ids {
            if self.snapshots.contains_key(id) || staged.contains_key(id) {
                continue;
            }
            let Some(source) = self.current(u32::from(id.get())) else {
                self.fail(Failure::SourceMissing(*id));
                return;
            };
            staged.insert(*id, ReplayProjection::snapshot(source));
        }
        self.snapshots.extend(staged);
        self.registered = true;
        // Registration alone does not certify a zone. A later registration
        // does not revoke an already accepted replay in that same zone.
        if !matches!(self.readiness, Readiness::Ready { .. }) {
            self.wait();
        }
    }

    fn replay(
        &mut self,
        assignments: &[eq_client_core::spell_tables::Assignment],
        session_id: u64,
    ) {
        if !self.registered {
            self.fail(Failure::RegistrationMissing);
            return;
        }
        let mut staged = BTreeMap::new();
        for assignment in assignments {
            let Some(source) = self.snapshots.get(&assignment.source) else {
                self.fail(Failure::SourceMissing(assignment.source));
                return;
            };
            let id = u32::from(assignment.destination.get());
            let Some(destination) = self.current(id) else {
                self.fail(Failure::DestinationMissing(assignment.destination));
                return;
            };
            // Clone the DESTINATION to retain its excluded/future fields.
            // Only the explicit numeric source projection is then applied.
            let mut destination = destination.clone();
            source.apply(&mut destination);
            staged.insert(id, destination);
        }
        self.effective.extend(staged);
        self.readiness = Readiness::Ready { session_id };
    }
}

#[cfg(test)]
mod tests;

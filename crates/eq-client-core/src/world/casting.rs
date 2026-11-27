//! The player's own casting: the cast in progress, a request awaiting its
//! answer, the last interruption, and when each memorized spell can be cast
//! again.
use crate::{
    SpellUpdate,
    disciplines::{Active, Timers},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// The Bard's class number.
pub(super) const BARD: u32 = 8;

/// How long a spell takes to come back, from the installed client's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpellTiming {
    /// Time before any spell can be cast again.
    pub recovery_ms: u32,
    /// Time before this spell can be cast again.
    pub recast_ms: u32,
}

/// What the world needs from the installed client's spell data. The data is
/// the user's own, read where it is installed; the world never guesses it.
pub trait SpellCatalog {
    /// A spell's recovery and recast times, when known.
    fn timing(&self, spell: u32) -> Option<SpellTiming>;

    /// Whether a spell's effect is over at once, leaving no buff behind.
    fn instant_effect(&self, spell: u32) -> bool;

    /// The spell fields that choose the band the Titanium program places a
    /// landing buff in; `None` where they are not known, which leaves the
    /// landing without a modelled slot.
    fn buff_band(&self, _spell: u32) -> Option<crate::buffs::Band> {
        None
    }

    /// A spell's twelve effect ids in order, where the spell data has them.
    fn effect_ids(&self, _spell: u32) -> Option<[u32; 12]> {
        None
    }

    /// A spell's twelve effects with the base, formula and max the slot
    /// finder's step 4 works their values out from (read R49-22e, 0x40f339),
    /// where the spell data has them.
    fn placement_effects(&self, _spell: u32) -> Option<[crate::buffs::placement::Effect; 12]> {
        None
    }

    /// Whether a spell has a Bard level (its Bard class level is not 255,
    /// 0x44e755(8)), where the spell data has the class levels.
    fn bard_level(&self, _spell: u32) -> Option<bool> {
        None
    }

    /// A spell's target type (Titanium field 98), where the spell data has
    /// it.
    fn target_type(&self, _spell: u32) -> Option<u32> {
        None
    }

    /// Whether any of a spell's effect slots is the feign effect (74),
    /// which the Titanium program lies the player down for when it lands on
    /// them (read R21). A spell whose effects are unknown feigns no one
    /// (recording checklist 6.455).
    fn feigns(&self, spell: u32) -> bool;
}

/// A catalog that knows no spells: nothing comes back on a timer, and every
/// lasting effect is kept.
pub struct NoSpells;

impl SpellCatalog for NoSpells {
    fn timing(&self, _spell: u32) -> Option<SpellTiming> {
        None
    }

    fn instant_effect(&self, _spell: u32) -> bool {
        false
    }

    fn feigns(&self, _spell: u32) -> bool {
        false
    }
}

/// A notice waiting for the spells' timing, in the order it came.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    /// A gem's refresh: spell, reduction, when, and whether it starts the
    /// spell's recovery (a Titanium Bard's doesn't, 6.642).
    Refresh(u32, u32, Instant, bool),
    /// A Bard's song stop, with the spells in the gems then.
    SongStop([Option<u32>; 8]),
}

/// When each memorized spell can be cast again. Timers start only from the
/// server's word: the profile at admission, or a gem's refresh notice.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns {
    /// Notices waiting for the spells' timing, in the order they came.
    pending: Vec<Pending>,
    /// When each spell comes back.
    spells: BTreeMap<u32, Instant>,
    /// When any spell comes back.
    recovery: Option<Instant>,
}

impl Cooldowns {
    /// Starts over from the profile's per-gem remaining times. A spell in
    /// two gems keeps the longer timer, and an empty gem starts none.
    pub fn restore(&mut self, gems: &[Option<u32>; 8], remaining: Option<[u32; 8]>, now: Instant) {
        *self = Self::default();
        if let Some(remaining) = remaining {
            for (spell, millis) in gems.iter().zip(remaining) {
                if let Some(spell) = spell.filter(|_| millis != 0) {
                    let end = now + Duration::from_millis(u64::from(millis));
                    self.spells
                        .entry(spell)
                        .and_modify(|previous| *previous = (*previous).max(end))
                        .or_insert(end);
                }
            }
        }
    }

    /// Notes a gem's refresh, which the spell's timing turns into a timer.
    pub fn refresh(&mut self, spell: u32, reduction: u32, now: Instant) {
        self.pending
            .push(Pending::Refresh(spell, reduction, now, true));
    }

    /// Notes a refresh of a Bard's own gem under the Titanium program's
    /// rule (0x458ed8 kind 3, read R49-21 c; 6.642): the spell's own timer
    /// starts as any other's, but the wait before any spell is set to the
    /// refresh's time rather than adding the spell's recovery.
    pub fn refresh_without_recovery(&mut self, spell: u32, reduction: u32, now: Instant) {
        self.pending
            .push(Pending::Refresh(spell, reduction, now, false));
    }

    /// Notes a Bard's song stop (0x40f06d, read R49-21 c, 6.542): the
    /// program then resets the timers of the gems whose spells have no
    /// recast, and the timer before any spell. Applied in order with the
    /// refreshes, once the spells' timing is known.
    pub fn song_stopped(&mut self, gems: &[Option<u32>; 8]) {
        self.pending.push(Pending::SongStop(*gems));
    }

    /// Starts the timers for refreshes whose spells the catalog knows,
    /// applies song stops in their place among them, and forgets the timers
    /// that ran out. Unknown timing stays unknown: no default reuse time is
    /// invented, and a song stop keeps a spell's timer it can't judge.
    pub fn resolve(&mut self, spells: &dyn SpellCatalog, now: Instant) {
        self.spells.retain(|_, end| *end > now);
        for pending in std::mem::take(&mut self.pending) {
            let (spell, reduction, received, recovers) = match pending {
                Pending::Refresh(spell, reduction, received, recovers) => {
                    (spell, reduction, received, recovers)
                }
                Pending::SongStop(gems) => {
                    self.recovery = None;
                    for spell in gems.into_iter().flatten() {
                        if spells
                            .timing(spell)
                            .is_some_and(|timing| timing.recast_ms == 0)
                        {
                            self.spells.remove(&spell);
                        }
                    }
                    continue;
                }
            };
            if let Some(timing) = spells.timing(spell) {
                self.spells.insert(
                    spell,
                    received
                        + Duration::from_millis(u64::from(
                            timing.recast_ms.saturating_sub(reduction),
                        )),
                );
                let recovery_ms = if recovers { timing.recovery_ms } else { 0 };
                self.recovery = Some(received + Duration::from_millis(u64::from(recovery_ms)));
            }
        }
    }

    /// How long until a spell can be cast again.
    #[must_use]
    pub fn remaining(&self, spell: u32, now: Instant) -> Duration {
        self.spells
            .get(&spell)
            .copied()
            .into_iter()
            .chain(self.recovery)
            .map(|end| end.saturating_duration_since(now))
            .max()
            .unwrap_or_default()
    }
}

/// What a spell notice did to the player's casting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastNews {
    /// A cast began.
    Began,
    /// A gem refreshed, ending the cast of its spell.
    Refreshed,
    /// The cast was interrupted.
    Interrupted,
    /// A received result ended the cast.
    Ended,
}

/// The player's own casting.
#[derive(Clone, Debug, Default)]
pub struct Casting {
    /// The cast in progress: spell, when it began and how long it takes.
    pub cast: Option<(u16, Instant, Duration)>,
    /// A cast request the session has not had answered yet.
    pub pending: Option<u32>,
    /// The song a Bard is singing, as the session tells it under the
    /// Titanium program's rules (`WorldEvent::Singing`, read R49-21).
    /// While its own cast is the one under way, nothing else waits on it.
    pub singing: Option<u32>,
    /// A Bard's `/melody` list (read R49-33 c); each admission starts with
    /// none.
    pub melody: super::Melody,
    /// Whether the player's own refreshes follow the Titanium program's
    /// rule for a Bard, starting no recovery (6.642): a Bard admitted where
    /// the session offers `Capability::BardRefreshTimers`.
    pub(super) bard_refresh: bool,
    /// The request associated with the active cast. TAKP can begin a Luclin
    /// port as spell 2935 and end it naming the requested port.
    requested: Option<u32>,
    /// Request context for the ordered cast notice that follows.
    next_requested: Option<u32>,
    /// The last interruption: when, and the server's message for it.
    pub interrupted: Option<(Instant, u32)>,
    /// When each memorized spell can be cast again.
    pub cooldowns: Cooldowns,
    /// Local Titanium gem clocks; other dialects retain their existing timers.
    pub(super) gem_clocks: Option<super::gem_timing::GemClocks>,
    /// When the admitted knight's profile or locally estimated recovery elapses.
    pub(super) knight_ready: Option<(Instant, crate::knights::ReuseSource)>,
    pub(super) discipline_timers: Timers,
    pub(super) linked_timers: Timers,
    pub(super) active_discipline: Active,
}

impl Casting {
    /// Whether a cast is under way that holds other actions back: one
    /// awaiting the server's answer, or one in progress that isn't the
    /// song a Bard is singing. The program holds nothing because of a song
    /// (read R49-21d); an item cast alongside one holds as any cast.
    #[must_use]
    pub fn holds_actions(&self) -> bool {
        self.pending.is_some()
            || self
                .cast
                .is_some_and(|(spell, ..)| self.singing != Some(u32::from(spell)))
    }

    /// Time left on knight recovery. A missing profile field
    /// stays unknown. Zero means the last reported wait has elapsed.
    #[must_use]
    pub fn knight_wait(&self, now: Instant) -> Option<Duration> {
        Some(self.knight_ready?.0.saturating_duration_since(now))
    }

    /// Whether the current knight timer was received or estimated locally.
    #[must_use]
    pub fn knight_reuse_source(&self) -> Option<crate::knights::ReuseSource> {
        self.knight_ready.map(|(_, source)| source)
    }

    /// Last reported active discipline. Silence preserves unknown state.
    #[must_use]
    pub const fn active_discipline(&self) -> Active {
        self.active_discipline
    }

    /// Remaining received reuse in the discipline namespace.
    #[must_use]
    pub fn discipline_wait(&self, timer: u32, now: Instant) -> Option<Duration> {
        self.discipline_timers.remaining(timer, now)
    }

    /// Remaining received reuse in the linked spell namespace.
    #[must_use]
    pub fn linked_wait(&self, timer: u32, now: Instant) -> Option<Duration> {
        self.linked_timers.remaining(timer, now)
    }

    /// The longest known spell wait for its own reuse or linked spell group.
    #[must_use]
    pub fn spell_wait(&self, spell: u32, linked: Option<u32>, now: Instant) -> Duration {
        let linked_wait = linked
            .and_then(|group| self.linked_wait(group, now))
            .unwrap_or_default();
        self.cooldowns.remaining(spell, now).max(linked_wait)
    }

    /// The gem's painted readiness, including shared recovery and linked reuse.
    /// `None` means a received transition still needs installed spell timing.
    #[must_use]
    pub fn gem_wait(
        &self,
        gem: u8,
        spell: u32,
        linked: Option<u32>,
        now: Instant,
    ) -> Option<Duration> {
        self.gem_wait_for(gem, spell, linked, now, false)
    }

    /// The ordinary gem click's strict timer boundary. Hotkeys and scripted
    /// input use this same gate; cast completion remains the server's event.
    #[must_use]
    pub fn gem_cast_wait(
        &self,
        gem: u8,
        spell: u32,
        linked: Option<u32>,
        now: Instant,
    ) -> Option<Duration> {
        self.gem_wait_for(gem, spell, linked, now, true)
    }

    fn gem_wait_for(
        &self,
        gem: u8,
        spell: u32,
        linked: Option<u32>,
        now: Instant,
        click: bool,
    ) -> Option<Duration> {
        let Some(clocks) = &self.gem_clocks else {
            return Some(self.spell_wait(spell, linked, now));
        };
        let linked_wait = linked
            .and_then(|group| self.linked_wait(group, now))
            .unwrap_or_default();
        Some(clocks.wait(gem, spell, now, click)?.max(linked_wait))
    }

    pub(super) fn gem_submitted(&mut self, gem: u8, spell: u32, now: Instant) {
        if let Some(clocks) = &mut self.gem_clocks {
            clocks.submitted(gem, spell, now);
        }
    }

    /// Tracks the worker's pending request separately from any active cast.
    /// When the worker releases a request before forwarding its answer,
    /// keep the spell for the begin or result that follows.
    pub(super) fn pend(&mut self, spell_id: Option<u32>) {
        self.next_requested = match spell_id {
            Some(_) => None,
            None => self.pending.or(self.next_requested),
        };
        self.pending = spell_id;
    }

    pub(super) fn late_begin(&mut self, requested_spell: u32) {
        self.next_requested = Some(requested_spell);
    }

    /// Follows the player's own casts. Resource updates alone never claim
    /// that a cast succeeded. A cast ends on a result for its spell or for
    /// the spell asked for.
    pub(super) fn observe(
        &mut self,
        own_id: u16,
        gems: &[Option<u32>; 8],
        update: &SpellUpdate,
        now: Instant,
    ) -> Option<CastNews> {
        if let SpellUpdate::Mana {
            spell_id,
            keep_casting: false,
        } = *update
            && let Some(clocks) = &mut self.gem_clocks
        {
            clocks.mana(spell_id);
        }
        let casting = |spell_id: u32| {
            self.cast.is_some_and(|(active, _, _)| {
                u32::from(active) == spell_id || self.requested == Some(spell_id)
            })
        };
        match *update {
            SpellUpdate::Slot {
                slot,
                spell_id,
                mode: 1,
            } if usize::try_from(slot).ok().and_then(|slot| gems.get(slot))
                == Some(&Some(spell_id)) =>
            {
                if let (Some(clocks), Ok(gem)) = (&mut self.gem_clocks, u8::try_from(slot)) {
                    clocks.memorized(gem, spell_id, now);
                }
                None
            }
            SpellUpdate::Slot { slot, mode: 2, .. } => {
                if let (Some(clocks), Ok(gem)) = (&mut self.gem_clocks, u8::try_from(slot)) {
                    clocks.forgotten(gem);
                }
                None
            }
            SpellUpdate::BarRefresh {
                slot,
                spell_id,
                reduction_ms,
            } if usize::try_from(slot).ok().and_then(|slot| gems.get(slot))
                == Some(&Some(spell_id)) =>
            {
                if let (Some(clocks), Ok(gem)) = (&mut self.gem_clocks, u8::try_from(slot)) {
                    clocks.refreshed(gem, spell_id, reduction_ms, now, self.bard_refresh);
                }
                if self.bard_refresh {
                    self.cooldowns
                        .refresh_without_recovery(spell_id, reduction_ms, now);
                } else {
                    self.cooldowns.refresh(spell_id, reduction_ms, now);
                }
                self.melody.refreshed();
                if casting(spell_id) {
                    self.cast = None;
                    self.requested = None;
                }
                if self.next_requested == Some(spell_id) {
                    self.next_requested = None;
                }
                Some(CastNews::Refreshed)
            }
            SpellUpdate::BarRefresh { slot, spell_id, .. } if class_refresh(slot, spell_id) => {
                let active = casting(spell_id);
                // The worker clears its pending marker before forwarding the
                // refresh. An instant cast can answer without a Begin.
                let harm_touch = crate::knights::KnightAbility::HarmTouch;
                if self.next_requested.is_some_and(|requested| {
                    requested == spell_id
                        || (harm_touch.matches_spell(requested)
                            && harm_touch.matches_spell(spell_id))
                }) {
                    self.next_requested = None;
                }
                if active {
                    self.cast = None;
                    self.requested = None;
                    Some(CastNews::Ended)
                } else {
                    None
                }
            }
            SpellUpdate::Began {
                caster_id,
                spell_id,
                duration_ms,
            } if caster_id == own_id => {
                self.interrupted = None;
                // An own begin answers the pending request even if its
                // clear notice has not been applied yet. Keep the request
                // as an alias of the spell the server actually named.
                let requested = self.next_requested.take().or_else(|| self.pending.take());
                if requested.is_some() || self.cast.is_none_or(|(active, ..)| active != spell_id) {
                    self.requested = requested;
                }
                self.cast = Some((spell_id, now, Duration::from_millis(u64::from(duration_ms))));
                if let Some(clocks) = &mut self.gem_clocks {
                    clocks.began(u32::from(spell_id), self.requested);
                }
                Some(CastNews::Began)
            }
            SpellUpdate::Interrupted {
                caster_id,
                message_id,
                ..
            } if caster_id == u32::from(own_id) => {
                self.cast = None;
                self.requested = None;
                self.next_requested = None;
                self.interrupted = Some((now, message_id));
                Some(CastNews::Interrupted)
            }
            SpellUpdate::Mana {
                spell_id,
                keep_casting: false,
            } if casting(spell_id) => {
                self.cast = None;
                self.requested = None;
                Some(CastNews::Ended)
            }
            // A request that ended before its cast began, as a fizzle does.
            SpellUpdate::Mana {
                spell_id,
                keep_casting: false,
            } if self.next_requested == Some(spell_id) => {
                self.next_requested = None;
                None
            }
            _ => None,
        }
    }

    /// Releases the identified local hold and forgets its request alias.
    pub(super) fn release_hold(&mut self, spell_id: u32) -> bool {
        let active = self.cast.map(|(spell, ..)| u32::from(spell));
        if active.or(self.pending) != Some(spell_id) {
            return false;
        }
        self.cast = None;
        self.pending = self.pending.filter(|&pending| pending != spell_id);
        self.requested = None;
        true
    }

    /// Drops what was in flight: the cast, the request and the interruption.
    pub(super) fn drop_actions(&mut self) {
        self.cast = None;
        self.pending = None;
        self.requested = None;
        self.next_requested = None;
        self.interrupted = None;
        self.active_discipline = Active::Unknown;
    }

    /// Forgets the request in flight and every timer.
    pub(super) fn reset_cooldowns(&mut self) {
        self.pending = None;
        self.requested = None;
        self.next_requested = None;
        self.cooldowns = Cooldowns::default();
        self.gem_clocks = None;
        self.knight_ready = None;
        self.discipline_timers = Timers::default();
        self.linked_timers = Timers::default();
        self.active_discipline = Active::Unknown;
    }
}

/// A class cast refresh is a cast result, with no gem or reuse update.
fn class_refresh(slot: u32, spell_id: u32) -> bool {
    use crate::{disciplines, knights};
    use knights::KnightAbility::{HarmTouch, LayOnHands};
    let knight = HarmTouch.matches_spell(spell_id) || LayOnHands.matches_spell(spell_id);
    if slot == u32::from(knights::CAST_SLOT) {
        return knight;
    }
    // EQEmu can translate its internal ability slot to slot 10, the slot
    // every discipline casts from (read R48); the refresh settles only the
    // cast of the spell it names.
    slot == u32::from(disciplines::CAST_SLOT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_class_refresh_settles_the_cast_without_creating_reuse() {
        let now = Instant::now();
        let mut casting = Casting::default();
        let gems = [None; 8];
        casting.pend(Some(88));
        casting.observe(
            7,
            &gems,
            &SpellUpdate::Began {
                caster_id: 7,
                spell_id: 2821,
                duration_ms: 10,
            },
            now,
        );
        let refresh = |slot, spell_id| SpellUpdate::BarRefresh {
            slot,
            spell_id,
            reduction_ms: 0,
        };
        assert_eq!(casting.observe(7, &gems, &refresh(8, 2821), now), None);
        assert_eq!(casting.observe(7, &gems, &refresh(9, 87), now), None);
        assert_eq!(
            casting.observe(7, &gems, &refresh(9, 88), now),
            Some(CastNews::Ended)
        );
        assert_eq!(casting.knight_wait(now), None);
        assert!(casting.cast.is_none());
    }

    #[test]
    fn a_pre_begin_class_refresh_cannot_leak_its_alias_into_another_cast() {
        let now = Instant::now();
        let gems = [None; 8];
        for (requested, slot, resolved) in [
            (88, 9, 88),
            (88, 9, 2821),
            (88, 10, 88),
            (88, 10, 2821),
            (4499, 10, 4499),
        ] {
            let mut casting = Casting::default();
            casting.pend(Some(requested));
            casting.pend(None);
            let refresh = SpellUpdate::BarRefresh {
                slot,
                spell_id: resolved,
                reduction_ms: 0,
            };
            assert_eq!(casting.observe(7, &gems, &refresh, now), None);
            assert_eq!(casting.next_requested, None);
            let unrelated = SpellUpdate::Began {
                caster_id: 7,
                spell_id: 200,
                duration_ms: 500,
            };
            assert_eq!(
                casting.observe(7, &gems, &unrelated, now),
                Some(CastNews::Began)
            );
            assert_eq!(casting.observe(7, &gems, &refresh, now), None);
            let repeated_base = SpellUpdate::BarRefresh {
                slot,
                spell_id: requested,
                reduction_ms: 0,
            };
            assert_eq!(casting.observe(7, &gems, &repeated_base, now), None);
            assert!(matches!(casting.cast, Some((200, _, _))));
        }
    }

    #[test]
    fn only_the_exact_class_refresh_consumes_the_pre_begin_alias() {
        let now = Instant::now();
        let gems = [None; 8];
        for (requested, wrong_slot, wrong_spell) in [
            (88, 8, 88),
            (88, 9, 87),
            (4499, 9, 4499),
            (4499, 10, 4503),
            (200, 9, 88),
        ] {
            let mut casting = Casting::default();
            casting.pend(Some(requested));
            casting.pend(None);
            let refresh = SpellUpdate::BarRefresh {
                slot: wrong_slot,
                spell_id: wrong_spell,
                reduction_ms: 0,
            };
            assert_eq!(casting.observe(7, &gems, &refresh, now), None);
            assert_eq!(casting.next_requested, Some(requested));
        }
    }

    #[test]
    fn releasing_an_old_cast_keeps_the_new_begin_alias() {
        let now = Instant::now();
        let gems = [None; 8];
        let begin = |spell_id| SpellUpdate::Began {
            caster_id: 7,
            spell_id,
            duration_ms: 3000,
        };
        let end = |spell_id| SpellUpdate::Mana {
            spell_id,
            keep_casting: false,
        };
        let mut casting = Casting::default();
        casting.pend(Some(42));
        casting.pend(None);
        casting.observe(7, &gems, &begin(2935), now);

        casting.pend(Some(43));
        casting.pend(None);
        assert!(casting.release_hold(2935));
        assert!(casting.cast.is_none());

        casting.observe(7, &gems, &begin(2936), now);
        assert_eq!(casting.observe(7, &gems, &end(42), now), None);
        assert_eq!(casting.cast.map(|(spell, ..)| spell), Some(2936));
        casting.observe(7, &gems, &begin(2936), now);
        assert_eq!(
            casting.observe(7, &gems, &end(43), now),
            Some(CastNews::Ended)
        );
        assert!(casting.cast.is_none());
    }

    struct Catalog;

    impl SpellCatalog for Catalog {
        fn timing(&self, spell: u32) -> Option<SpellTiming> {
            (spell == 42).then_some(SpellTiming {
                recovery_ms: 1500,
                recast_ms: 6000,
            })
        }

        fn instant_effect(&self, _spell: u32) -> bool {
            false
        }

        fn feigns(&self, _spell: u32) -> bool {
            false
        }
    }

    #[test]
    fn admission_replaces_old_timers_and_restores_only_populated_gems() {
        let start = Instant::now();
        let mut timers = Cooldowns::default();
        timers.refresh(99, 0, start);
        let gems = [Some(42), None, Some(42), Some(73), None, None, None, None];
        timers.restore(&gems, Some([1200, 9000, 3000, 0, 0, 0, 0, 0]), start);
        assert_eq!(timers.pending, []);
        assert_eq!(timers.remaining(42, start), Duration::from_secs(3));
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
        assert_eq!(timers.remaining(99, start), Duration::ZERO);
        assert_eq!(
            timers.remaining(42, start + Duration::from_secs(2)),
            Duration::from_secs(1)
        );
        timers.restore(&gems, None, start);
        assert_eq!(timers.remaining(42, start), Duration::ZERO);
    }

    #[test]
    fn server_refresh_applies_reduction_and_shared_recovery_without_guessing_missing_data() {
        let start = Instant::now();
        let mut timers = Cooldowns::default();
        timers.refresh(42, 2000, start);
        timers.resolve(&Catalog, start);
        assert_eq!(timers.remaining(42, start), Duration::from_secs(4));
        assert_eq!(timers.remaining(99, start), Duration::from_millis(1500));
        assert_eq!(
            timers.remaining(42, start + Duration::from_secs(4)),
            Duration::ZERO
        );
        timers.refresh(42, u32::MAX, start);
        timers.resolve(&Catalog, start);
        assert_eq!(timers.remaining(42, start), Duration::from_millis(1500));
        let mut unknown = Cooldowns::default();
        unknown.refresh(99, 0, start);
        unknown.resolve(&NoSpells, start);
        assert_eq!(unknown.remaining(99, start), Duration::ZERO);
    }

    /// Song 73 has no recast, spell 42 has one; both have a recovery.
    struct Songs;

    impl SpellCatalog for Songs {
        fn timing(&self, spell: u32) -> Option<SpellTiming> {
            match spell {
                73 => Some(SpellTiming {
                    recovery_ms: 1500,
                    recast_ms: 0,
                }),
                42 => Catalog.timing(42),
                _ => None,
            }
        }

        fn instant_effect(&self, _spell: u32) -> bool {
            false
        }

        fn feigns(&self, _spell: u32) -> bool {
            false
        }
    }

    #[test]
    fn a_song_stop_resets_only_the_timers_of_spells_with_no_recast() {
        let start = Instant::now();
        let gems = [Some(73), Some(42), Some(99), None, None, None, None, None];
        let mut timers = Cooldowns::default();
        // The profile's timers at admission.
        timers.restore(&gems, Some([4000, 9000, 9000, 0, 0, 0, 0, 0]), start);
        assert_eq!(timers.remaining(73, start), Duration::from_secs(4));
        timers.song_stopped(&gems);
        timers.resolve(&Songs, start);
        // No recast: ready at once, as the program's stop leaves it.
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
        // A recast keeps its end; unknown timing is kept, not guessed.
        assert_eq!(timers.remaining(42, start), Duration::from_secs(9));
        assert_eq!(timers.remaining(99, start), Duration::from_secs(9));
    }

    #[test]
    fn a_song_stop_applies_in_order_with_the_refreshes_around_it() {
        let start = Instant::now();
        let gems = [Some(73), None, None, None, None, None, None, None];
        // A refresh before the stop, both waiting for the timing: the stop
        // clears what the refresh started.
        let mut timers = Cooldowns::default();
        timers.refresh(73, 0, start);
        timers.song_stopped(&gems);
        timers.resolve(&Songs, start);
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
        // A refresh after the stop starts its recovery again.
        timers.song_stopped(&gems);
        timers.refresh(73, 0, start);
        timers.resolve(&Songs, start);
        assert_eq!(timers.remaining(73, start), Duration::from_millis(1500));
        // A stop with unknown timing still clears the shared recovery.
        timers.song_stopped(&[None; 8]);
        timers.resolve(&NoSpells, start);
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
    }

    #[test]
    fn a_bards_refresh_starts_its_spells_timer_but_no_recovery() {
        let start = Instant::now();
        let mut timers = Cooldowns::default();
        // Anyone else's refresh starts the recovery; a Bard's, under the
        // Titanium program's rule, sets it to the refresh's time.
        timers.refresh(73, 0, start);
        timers.resolve(&Songs, start);
        assert_eq!(timers.remaining(99, start), Duration::from_millis(1500));
        timers.refresh_without_recovery(73, 0, start);
        timers.resolve(&Songs, start);
        assert_eq!(timers.remaining(99, start), Duration::ZERO);
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
        // The spell's own recast still runs.
        timers.refresh_without_recovery(42, 2000, start);
        timers.resolve(&Songs, start);
        assert_eq!(timers.remaining(42, start), Duration::from_secs(4));
        assert_eq!(timers.remaining(73, start), Duration::ZERO);
    }

    #[test]
    fn only_the_players_own_casts_and_gems_change_their_casting() {
        let now = Instant::now();
        let mut casting = Casting::default();
        let gems = [Some(42), None, None, None, None, None, None, None];
        let began = |caster_id| SpellUpdate::Began {
            caster_id,
            spell_id: 42,
            duration_ms: 3000,
        };
        assert_eq!(casting.observe(7, &gems, &began(8), now), None);
        assert_eq!(
            casting.observe(7, &gems, &began(7), now),
            Some(CastNews::Began)
        );
        assert!(casting.cast.is_some());
        // A refresh for a spell in no gem is someone else's business.
        let refresh = |slot| SpellUpdate::BarRefresh {
            slot,
            spell_id: 42,
            reduction_ms: 0,
        };
        assert_eq!(casting.observe(7, &gems, &refresh(1), now), None);
        assert_eq!(
            casting.observe(7, &gems, &refresh(0), now),
            Some(CastNews::Refreshed)
        );
        assert!(casting.cast.is_none());
        casting.observe(7, &gems, &began(7), now);
        let interrupted = SpellUpdate::Interrupted {
            caster_id: 7,
            message_id: 199,
            caster_name: None,
        };
        assert_eq!(
            casting.observe(7, &gems, &interrupted, now),
            Some(CastNews::Interrupted)
        );
        assert_eq!(casting.interrupted, Some((now, 199)));
    }

    #[test]
    fn a_cast_begun_under_another_spell_ends_with_the_requested_ones_result() {
        let now = Instant::now();
        let gems = [Some(42), None, None, None, None, None, None, None];
        // TAKP begins a Luclin port as spell 2935 and ends it naming the
        // port; the session answers the request before the beginning arrives.
        let began = SpellUpdate::Began {
            caster_id: 7,
            spell_id: 2935,
            duration_ms: 10_000,
        };
        let ended = |spell_id| SpellUpdate::Mana {
            spell_id,
            keep_casting: false,
        };
        let refreshed = SpellUpdate::BarRefresh {
            slot: 0,
            spell_id: 42,
            reduction_ms: 0,
        };
        for end in [ended(42), refreshed] {
            let mut casting = Casting::default();
            casting.pend(Some(42));
            casting.pend(None);
            assert_eq!(casting.pending, None);
            casting.observe(7, &gems, &began, now);
            assert_eq!(casting.observe(7, &gems, &ended(43), now), None);
            assert!(casting.cast.is_some());
            assert!(casting.observe(7, &gems, &end, now).is_some());
            assert!(casting.cast.is_none());
        }
        // A request that ends before anything began is forgotten.
        let mut casting = Casting::default();
        casting.pend(Some(42));
        casting.pend(None);
        assert_eq!(casting.observe(7, &gems, &ended(42), now), None);
        casting.observe(7, &gems, &began, now);
        assert_eq!(casting.observe(7, &gems, &ended(42), now), None);
        assert!(casting.cast.is_some());
        // A new request forgets the last one's spell.
        let mut casting = Casting::default();
        casting.pend(Some(42));
        casting.pend(None);
        casting.pend(Some(43));
        casting.pend(None);
        casting.observe(7, &gems, &began, now);
        assert_eq!(casting.observe(7, &gems, &ended(42), now), None);
        assert!(casting.cast.is_some());
    }
}

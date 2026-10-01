//! The player's own casting: the cast in progress, a request awaiting its
//! answer, the last interruption, and when each memorized spell can be cast
//! again.
use crate::SpellUpdate;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

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
}

/// When each memorized spell can be cast again. Timers start only from the
/// server's word: the profile at admission, or a gem's refresh notice.
#[derive(Clone, Debug, Default)]
pub struct Cooldowns {
    /// Refresh notices waiting for the spell's timing: spell, reduction, when.
    pending: Vec<(u32, u32, Instant)>,
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
        self.pending.push((spell, reduction, now));
    }

    /// Starts the timers for refreshes whose spells the catalog knows, and
    /// forgets the ones that ran out. Unknown timing stays unknown: no
    /// default reuse time is invented.
    pub fn resolve(&mut self, spells: &dyn SpellCatalog, now: Instant) {
        self.spells.retain(|_, end| *end > now);
        for (spell, reduction, received) in self.pending.drain(..) {
            if let Some(timing) = spells.timing(spell) {
                self.spells.insert(
                    spell,
                    received
                        + Duration::from_millis(u64::from(
                            timing.recast_ms.saturating_sub(reduction),
                        )),
                );
                self.recovery =
                    Some(received + Duration::from_millis(u64::from(timing.recovery_ms)));
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
    /// The cast's spell spent its mana and ended.
    Ended,
}

/// The player's own casting.
#[derive(Clone, Debug, Default)]
pub struct Casting {
    /// The cast in progress: spell, when it began and how long it takes.
    pub cast: Option<(u16, Instant, Duration)>,
    /// A cast request the session has not had answered yet.
    pub pending: Option<u32>,
    /// The last interruption: when, and the server's message for it.
    pub interrupted: Option<(Instant, u32)>,
    /// When each memorized spell can be cast again.
    pub cooldowns: Cooldowns,
}

impl Casting {
    /// Follows the player's own casts. Resource updates alone never claim
    /// that a cast succeeded.
    pub(super) fn observe(
        &mut self,
        own_id: u16,
        gems: &[Option<u32>; 8],
        update: &SpellUpdate,
        now: Instant,
    ) -> Option<CastNews> {
        let casting = |spell_id: u32| {
            self.cast
                .is_some_and(|(active, _, _)| u32::from(active) == spell_id)
        };
        match *update {
            SpellUpdate::BarRefresh {
                slot,
                spell_id,
                reduction_ms,
            } if usize::try_from(slot).ok().and_then(|slot| gems.get(slot))
                == Some(&Some(spell_id)) =>
            {
                self.cooldowns.refresh(spell_id, reduction_ms, now);
                if casting(spell_id) {
                    self.cast = None;
                }
                Some(CastNews::Refreshed)
            }
            SpellUpdate::Began {
                caster_id,
                spell_id,
                duration_ms,
            } if caster_id == own_id => {
                self.interrupted = None;
                self.cast = Some((spell_id, now, Duration::from_millis(u64::from(duration_ms))));
                Some(CastNews::Began)
            }
            SpellUpdate::Interrupted {
                caster_id,
                message_id,
            } if caster_id == u32::from(own_id) => {
                self.cast = None;
                self.interrupted = Some((now, message_id));
                Some(CastNews::Interrupted)
            }
            SpellUpdate::Mana {
                spell_id,
                keep_casting: false,
            } if casting(spell_id) => {
                self.cast = None;
                Some(CastNews::Ended)
            }
            _ => None,
        }
    }

    /// Drops what was in flight: the cast, the request and the interruption.
    pub(super) fn drop_actions(&mut self) {
        self.cast = None;
        self.pending = None;
        self.interrupted = None;
    }

    /// Forgets the request in flight and every timer.
    pub(super) fn reset_cooldowns(&mut self) {
        self.pending = None;
        self.cooldowns = Cooldowns::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn admission_replaces_old_timers_and_restores_only_populated_gems() {
        let start = Instant::now();
        let mut timers = Cooldowns::default();
        timers.refresh(99, 0, start);
        let gems = [Some(42), None, Some(42), Some(73), None, None, None, None];
        timers.restore(&gems, Some([1200, 9000, 3000, 0, 0, 0, 0, 0]), start);
        assert!(timers.pending.is_empty());
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
        };
        assert_eq!(
            casting.observe(7, &gems, &interrupted, now),
            Some(CastNews::Interrupted)
        );
        assert_eq!(casting.interrupted, Some((now, 199)));
    }
}

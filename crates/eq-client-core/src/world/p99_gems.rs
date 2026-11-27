//! P99's explicitly requested early-start input policy, separate from stock
//! Titanium's painted/global/provisional clocks. No prediction is a cast ACK.
use super::SpellCatalog;
use crate::SpellUpdate;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
enum Basis {
    Profile(Duration),
    Refresh(u32),
    Memorized,
}

#[derive(Clone, Copy, Debug)]
struct Reuse {
    anchor: Instant,
    basis: Basis,
    full_reuse: Option<Duration>,
    ready: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
struct Attempt {
    gem: u8,
    requested: u32,
    actual: Option<u32>,
    completed: Option<Instant>,
    reduction: u32,
}

impl Attempt {
    fn matches(self, spell: u32) -> bool {
        self.requested == spell || self.actual == Some(spell)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Gems {
    pub(super) enabled: bool,
    reuse: BTreeMap<u32, Reuse>,
    observed_cast: BTreeMap<u32, Duration>,
    attempt: Option<Attempt>,
}

impl Gems {
    pub(super) fn restore(
        gems: &[Option<u32>; 8],
        remaining: Option<[u32; 8]>,
        now: Instant,
    ) -> Self {
        let mut state = Self::default();
        if let Some(remaining) = remaining {
            for (spell, millis) in gems.iter().zip(remaining) {
                if let Some(spell) = spell.filter(|_| millis != 0) {
                    let duration = Duration::from_millis(u64::from(millis));
                    state
                        .reuse
                        .entry(spell)
                        .and_modify(|reuse| {
                            if let Basis::Profile(previous) = &mut reuse.basis {
                                *previous = (*previous).max(duration);
                            }
                        })
                        .or_insert(Reuse {
                            anchor: now,
                            basis: Basis::Profile(duration),
                            full_reuse: None,
                            ready: None,
                        });
                }
            }
        }
        state
    }

    pub(super) fn submitted(&mut self, gem: u8, requested: u32) {
        // Pending/active ownership is held by Casting, never by an invented
        // ten-second reuse lock. A submission or rejected press starts no reuse.
        self.attempt = Some(Attempt {
            gem,
            requested,
            actual: None,
            completed: None,
            reduction: 0,
        });
    }

    pub(super) fn clear_attempt(&mut self) {
        self.attempt = None;
    }

    fn finish(&mut self, spell: u32, now: Instant, reduction: Option<u32>) -> bool {
        let Some(attempt) = self
            .attempt
            .as_mut()
            .filter(|attempt| attempt.matches(spell))
        else {
            return false;
        };
        if reduction.is_none() && attempt.actual.is_none() {
            // A pre-Begin Mana may release a fizzle/rejection. It is not
            // authority to start successful-cast reuse.
            return false;
        }
        // Mana and refresh can describe the same completion. A later receipt
        // cannot move its established anchor, nor subtract its credit twice.
        let already_completed = attempt.completed.is_some();
        let anchor = *attempt.completed.get_or_insert(now);
        if let Some(reduction) = reduction {
            attempt.reduction = reduction;
        }
        let full_reuse = self
            .reuse
            .get(&attempt.requested)
            .filter(|reuse| already_completed && reuse.anchor == anchor)
            .and_then(|reuse| reuse.full_reuse);
        self.reuse.insert(
            attempt.requested,
            Reuse {
                anchor,
                basis: Basis::Refresh(attempt.reduction),
                full_reuse,
                ready: None,
            },
        );
        true
    }

    pub(super) fn observe(
        &mut self,
        own: u16,
        gems: &[Option<u32>; 8],
        event: &SpellUpdate,
        now: Instant,
    ) {
        match *event {
            SpellUpdate::Began {
                caster_id,
                spell_id,
                duration_ms,
            } if caster_id == own => {
                let duration = Duration::from_millis(u64::from(duration_ms));
                self.observed_cast.insert(u32::from(spell_id), duration);
                if let Some(attempt) = &mut self.attempt {
                    attempt.actual = Some(u32::from(spell_id));
                    self.observed_cast.insert(attempt.requested, duration);
                }
            }
            SpellUpdate::Mana {
                spell_id,
                keep_casting: false,
            } => {
                self.finish(spell_id, now, None);
            }
            SpellUpdate::BarRefresh {
                slot,
                spell_id,
                reduction_ms,
            } if usize::try_from(slot).ok().and_then(|slot| gems.get(slot))
                == Some(&Some(spell_id)) =>
            {
                let matching = self.attempt.is_some_and(|attempt| {
                    u32::from(attempt.gem) == slot && attempt.matches(spell_id)
                });
                if !matching || !self.finish(spell_id, now, Some(reduction_ms)) {
                    self.reuse.insert(
                        spell_id,
                        Reuse {
                            anchor: now,
                            basis: Basis::Refresh(reduction_ms),
                            full_reuse: None,
                            ready: None,
                        },
                    );
                }
            }
            SpellUpdate::Slot {
                slot,
                spell_id,
                mode: 1,
            } if usize::try_from(slot).ok().and_then(|slot| gems.get(slot))
                == Some(&Some(spell_id)) =>
            {
                // Keep confirmed preparation's existing guard, separate from
                // successful-cast repeat reuse; its server boundary is unread.
                self.reuse.insert(
                    spell_id,
                    Reuse {
                        anchor: now,
                        basis: Basis::Memorized,
                        full_reuse: None,
                        ready: None,
                    },
                );
            }
            SpellUpdate::Interrupted { caster_id, .. } if caster_id == u32::from(own) => {
                self.clear_attempt()
            }
            _ => {}
        }
    }

    pub(super) fn resolve(&mut self, spells: &dyn SpellCatalog, titanium: bool) {
        self.enabled = titanium && spells.p99_gem_start();
        for (spell, reuse) in &mut self.reuse {
            let cast = self
                .observed_cast
                .get(spell)
                .copied()
                .or_else(|| spells.cast_duration(*spell));
            // Once known, this completion's full reuse remains anchored.
            // A later table replay or receipt must not redefine its deadline.
            reuse.full_reuse = reuse.full_reuse.or_else(|| match reuse.basis {
                Basis::Profile(remaining) => Some(remaining),
                Basis::Refresh(_) => spells
                    .timing(*spell)
                    .map(|timing| Duration::from_millis(u64::from(timing.recast_ms))),
                Basis::Memorized => spells
                    .timing(*spell)
                    .map(|timing| Duration::from_millis(u64::from(timing.recast_ms) + 1000)),
            });
            let credit = match reuse.basis {
                Basis::Memorized => Duration::ZERO,
                Basis::Profile(_) => cast.unwrap_or_default(),
                // The raw packet does not label its reduction's basis.
                // Overlap received and cast credit until independence is known.
                Basis::Refresh(reduction) => {
                    Duration::from_millis(u64::from(reduction)).max(cast.unwrap_or_default())
                }
            };
            reuse.ready = reuse
                .full_reuse
                .map(|full| reuse.anchor + full.saturating_sub(credit));
        }
    }

    pub(super) fn wait(&self, spell: u32, now: Instant) -> Option<Duration> {
        self.reuse
            .get(&spell)
            .map_or(Some(Duration::ZERO), |reuse| {
                Some(reuse.ready?.saturating_duration_since(now))
            })
    }
}

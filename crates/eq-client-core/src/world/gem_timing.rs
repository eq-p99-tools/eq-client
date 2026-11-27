//! Titanium's local gem click clocks, separate from server cast completion.
use super::SpellCatalog;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
enum Reuse {
    Memorized,
    Refreshed(u32),
}

#[derive(Clone, Copy, Debug)]
enum Lock {
    Until {
        spell: u32,
        end: Instant,
        provisional: bool,
    },
    Timing {
        spell: u32,
        received: Instant,
        reuse: Reuse,
    },
}

impl Lock {
    const fn spell(self) -> u32 {
        match self {
            Self::Until { spell, .. } | Self::Timing { spell, .. } => spell,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Global {
    Until(Instant),
    Timing { spell: u32, received: Instant },
}

#[derive(Clone, Copy, Debug)]
struct Cast {
    gem: u8,
    requested: u32,
    actual: Option<u32>,
}

/// Received profile clocks and the native program's local clock transitions.
/// A missing spell record remains unresolved rather than inventing a delay.
#[derive(Clone, Debug, Default)]
pub(super) struct GemClocks {
    gems: [Option<Lock>; 8],
    stopped: [bool; 8],
    global: Option<Global>,
    cast: Option<Cast>,
}

impl GemClocks {
    pub(super) fn restore(
        gems: &[Option<u32>; 8],
        remaining: Option<[u32; 8]>,
        now: Instant,
    ) -> Self {
        let mut clocks = Self::default();
        if let Some(remaining) = remaining {
            for (index, (spell, millis)) in gems.iter().zip(remaining).enumerate() {
                if let Some(spell) = spell.filter(|_| millis != 0) {
                    clocks.gems[index] = Some(Lock::Until {
                        spell,
                        end: now + Duration::from_millis(u64::from(millis)),
                        provisional: false,
                    });
                }
            }
        }
        clocks
    }

    pub(super) fn memorized(&mut self, gem: u8, spell: u32, received: Instant) {
        if let Some(slot) = self.gems.get_mut(usize::from(gem)) {
            self.stopped[usize::from(gem)] = false;
            *slot = Some(Lock::Timing {
                spell,
                received,
                reuse: Reuse::Memorized,
            });
        }
    }

    pub(super) fn forgotten(&mut self, gem: u8) {
        if let Some(slot) = self.gems.get_mut(usize::from(gem)) {
            self.stopped[usize::from(gem)] = false;
            *slot = None;
        }
    }

    pub(super) fn submitted(&mut self, gem: u8, spell: u32, now: Instant) {
        let Some(slot) = self.gems.get_mut(usize::from(gem)) else {
            return;
        };
        let end = now + Duration::from_secs(10);
        self.stopped[usize::from(gem)] = false;
        *slot = Some(Lock::Until {
            spell,
            end,
            provisional: true,
        });
        self.global = Some(Global::Until(end));
        self.cast = Some(Cast {
            gem,
            requested: spell,
            actual: None,
        });
    }

    pub(super) fn began(&mut self, spell: u32, requested: Option<u32>) {
        if let Some(cast) = &mut self.cast
            && (requested == Some(cast.requested) || spell == cast.requested)
        {
            cast.actual = Some(spell);
        }
    }

    pub(super) fn mana(&mut self, spell: u32, now: Instant) {
        let Some(cast) = self
            .cast
            .filter(|cast| cast.requested == spell || cast.actual == Some(spell))
        else {
            return;
        };
        let slot = &mut self.gems[usize::from(cast.gem)];
        if matches!(slot, Some(Lock::Until { spell, provisional: true, .. }) if *spell == cast.requested)
        {
            *slot = Some(Lock::Until {
                spell: cast.requested,
                end: now,
                provisional: false,
            });
        }
        self.global = Some(Global::Until(now));
        self.cast = None;
    }

    pub(super) fn refreshed(
        &mut self,
        gem: u8,
        spell: u32,
        reduction: u32,
        received: Instant,
        bard: bool,
    ) {
        let Some(slot) = self.gems.get_mut(usize::from(gem)) else {
            return;
        };
        self.stopped[usize::from(gem)] = false;
        *slot = Some(Lock::Timing {
            spell,
            received,
            reuse: Reuse::Refreshed(reduction),
        });
        self.global = if bard {
            Some(Global::Until(received))
        } else {
            Some(Global::Timing { spell, received })
        };
        if self
            .cast
            .is_some_and(|cast| cast.gem == gem && cast.requested == spell)
        {
            self.cast = None;
        }
    }

    pub(super) fn song_stopped(&mut self, spells: &dyn SpellCatalog) {
        self.global = None;
        for (slot, stopped) in self.gems.iter_mut().zip(&mut self.stopped) {
            match slot.and_then(|lock| spells.timing(lock.spell())) {
                Some(timing) => {
                    *stopped = false;
                    if timing.recast_ms == 0 {
                        *slot = None;
                    }
                }
                None => *stopped = slot.is_some(),
            }
        }
    }

    pub(super) fn resolve(&mut self, spells: &dyn SpellCatalog) {
        for (slot, stopped) in self.gems.iter_mut().zip(&mut self.stopped) {
            if *stopped && let Some(timing) = slot.and_then(|lock| spells.timing(lock.spell())) {
                *stopped = false;
                if timing.recast_ms == 0 {
                    *slot = None;
                }
            }
            if let Some(Lock::Timing {
                spell,
                received,
                reuse,
            }) = *slot
                && let Some(timing) = spells.timing(spell)
            {
                let millis = match reuse {
                    Reuse::Memorized => u64::from(timing.recast_ms) + 1000,
                    Reuse::Refreshed(reduction) => {
                        u64::from(timing.recast_ms.saturating_sub(reduction))
                    }
                };
                *slot = Some(Lock::Until {
                    spell,
                    end: received + Duration::from_millis(millis),
                    provisional: false,
                });
            }
        }
        if let Some(Global::Timing { spell, received }) = self.global
            && let Some(timing) = spells.timing(spell)
        {
            self.global = Some(Global::Until(
                received + Duration::from_millis(u64::from(timing.recovery_ms)),
            ));
        }
    }

    /// The click's gem boundary is strict; the painted gem accepts equality.
    /// The shared recovery boundary is strict for both native paths.
    pub(super) fn wait(&self, gem: u8, spell: u32, now: Instant, click: bool) -> Option<Duration> {
        let own = match *self.gems.get(usize::from(gem))? {
            Some(lock) if lock.spell() != spell => Duration::ZERO,
            Some(Lock::Until { end, .. }) => until(end, now, click),
            Some(Lock::Timing { .. }) => return None,
            None => Duration::ZERO,
        };
        let global = match self.global {
            Some(Global::Until(end)) => until(end, now, true),
            Some(Global::Timing { .. }) => return None,
            None => Duration::ZERO,
        };
        Some(own.max(global))
    }
}

fn until(end: Instant, now: Instant, strict: bool) -> Duration {
    let wait = end.saturating_duration_since(now);
    if strict && now == end {
        Duration::from_nanos(1)
    } else {
        wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Catalog;
    impl SpellCatalog for Catalog {
        fn timing(&self, spell: u32) -> Option<super::super::SpellTiming> {
            (spell == 42 || spell == 43).then_some(super::super::SpellTiming {
                recovery_ms: 1500,
                recast_ms: 6000,
            })
        }
        fn instant_effect(&self, _: u32) -> bool {
            false
        }
        fn feigns(&self, _: u32) -> bool {
            false
        }
    }

    #[test]
    fn profile_clocks_stay_per_slot_even_when_two_gems_hold_the_same_spell() {
        let now = Instant::now();
        let clocks = GemClocks::restore(
            &[Some(42), Some(42), None, None, None, None, None, None],
            Some([2000, 6000, 0, 0, 0, 0, 0, 0]),
            now,
        );
        assert_eq!(clocks.wait(0, 42, now, true), Some(Duration::from_secs(2)));
        assert_eq!(clocks.wait(1, 42, now, true), Some(Duration::from_secs(6)));
    }

    #[test]
    fn a_song_stop_with_late_spell_data_never_resets_a_later_submission() {
        struct Song;
        impl SpellCatalog for Song {
            fn timing(&self, _: u32) -> Option<super::super::SpellTiming> {
                Some(super::super::SpellTiming {
                    recovery_ms: 0,
                    recast_ms: 0,
                })
            }
            fn instant_effect(&self, _: u32) -> bool {
                false
            }
            fn feigns(&self, _: u32) -> bool {
                false
            }
        }
        let now = Instant::now();
        let mut clocks = GemClocks::default();
        clocks.memorized(0, 42, now);
        clocks.memorized(1, 43, now);
        clocks.song_stopped(&super::super::NoSpells);
        clocks.submitted(1, 43, now);
        clocks.resolve(&Song);
        assert_eq!(clocks.gems[0].map(Lock::spell), None);
        assert_eq!(clocks.wait(1, 43, now, true), Some(Duration::from_secs(10)));
    }

    #[test]
    fn memorize_confirmation_locks_only_its_gem_for_full_recast_plus_one_second() {
        let now = Instant::now();
        let mut clocks = GemClocks::default();
        clocks.memorized(1, 42, now);
        assert_eq!(clocks.wait(1, 42, now, true), None);
        clocks.resolve(&Catalog);
        assert_eq!(clocks.wait(1, 42, now, true), Some(Duration::from_secs(7)));
        assert_eq!(clocks.wait(0, 42, now, true), Some(Duration::ZERO));
        let boundary = now + Duration::from_secs(7);
        assert_eq!(clocks.wait(1, 42, boundary, false), Some(Duration::ZERO));
        assert!(clocks.wait(1, 42, boundary, true).unwrap() > Duration::ZERO);
        assert_eq!(
            clocks.wait(1, 42, boundary + Duration::from_millis(1), true),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn refresh_uses_full_recast_reduction_and_separate_global_recovery() {
        let now = Instant::now();
        let mut clocks = GemClocks::default();
        clocks.submitted(0, 42, now);
        clocks.mana(42, now);
        assert!(clocks.wait(0, 42, now, true).unwrap() > Duration::ZERO);
        assert_eq!(
            clocks.wait(0, 42, now + Duration::from_nanos(1), true),
            Some(Duration::ZERO)
        );
        let received = now + Duration::from_secs(3);
        clocks.refreshed(0, 42, 2000, received, false);
        clocks.resolve(&Catalog);
        assert_eq!(
            clocks.wait(0, 42, received, true),
            Some(Duration::from_secs(4))
        );
        assert_eq!(
            clocks.wait(1, 43, received, true),
            Some(Duration::from_millis(1500))
        );
        clocks.refreshed(0, 42, u32::MAX, received, true);
        clocks.resolve(&Catalog);
        assert!(clocks.wait(1, 43, received, true).unwrap() > Duration::ZERO);
        assert_eq!(
            clocks.wait(1, 43, received + Duration::from_nanos(1), true),
            Some(Duration::ZERO)
        );
        assert!(clocks.wait(0, 42, received, true).unwrap() > Duration::ZERO);
        assert_eq!(
            clocks.wait(0, 42, received + Duration::from_millis(1), true),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn provisional_context_does_not_let_an_old_result_clear_a_new_cast_or_refresh() {
        let now = Instant::now();
        let mut clocks = GemClocks::default();
        clocks.submitted(0, 42, now);
        assert_eq!(clocks.wait(1, 43, now, true), Some(Duration::from_secs(10)));
        clocks.submitted(1, 43, now);
        clocks.mana(42, now);
        assert_eq!(clocks.wait(1, 43, now, true), Some(Duration::from_secs(10)));
        clocks.began(44, Some(43));
        clocks.mana(44, now);
        assert_eq!(
            clocks.wait(1, 43, now + Duration::from_nanos(1), true),
            Some(Duration::ZERO)
        );
        clocks.refreshed(1, 43, 0, now, false);
        clocks.resolve(&Catalog);
        clocks.mana(43, now);
        assert_eq!(clocks.wait(1, 43, now, true), Some(Duration::from_secs(6)));
    }

    #[test]
    fn delayed_catalog_is_unknown_and_uses_receive_time_without_resurrecting_replaced_slots() {
        let now = Instant::now();
        let mut clocks = GemClocks::default();
        clocks.memorized(0, 42, now);
        clocks.resolve(&super::super::NoSpells);
        assert_eq!(clocks.wait(0, 42, now, true), None);
        clocks.resolve(&Catalog);
        assert_eq!(
            clocks.wait(0, 42, now + Duration::from_secs(2), true),
            Some(Duration::from_secs(5))
        );
        clocks.memorized(0, 42, now);
        clocks.forgotten(0);
        clocks.resolve(&Catalog);
        assert_eq!(clocks.wait(0, 43, now, true), Some(Duration::ZERO));
    }
}

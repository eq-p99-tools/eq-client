//! Presentation timers start only on server spell-bar refresh notifications.
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Cooldowns {
    pending: Vec<(u32, u32, Instant)>,
    spells: BTreeMap<u32, Instant>,
    recovery: Option<Instant>,
}

impl Cooldowns {
    /// Replaces previous-admission state with server-provided per-gem remaining time.
    /// Duplicate spells retain the longest timer and empty slots never create one.
    pub(super) fn restore(
        &mut self,
        gems: &[Option<u32>; 8],
        remaining: Option<[u32; 8]>,
        now: Instant,
    ) {
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
    pub(super) fn refresh(&mut self, spell: u32, reduction: u32, now: Instant) {
        self.pending.push((spell, reduction, now));
    }

    /// Missing local timing stays unknown; no default reuse duration is invented.
    pub(super) fn resolve(&mut self, names: &super::super::spellbook::SpellNames, now: Instant) {
        self.spells.retain(|_, end| *end > now);
        for (spell, reduction, received) in self.pending.drain(..) {
            if let Some(timing) = names.timing(spell) {
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

    pub(crate) fn remaining(&self, spell: u32, now: Instant) -> Duration {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spellbook::SpellNames;

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
        let mut fields = vec!["0"; 16];
        fields[0] = "42";
        fields[1] = "Synthetic spell";
        fields[14] = "1500";
        fields[15] = "6000";
        let names = SpellNames::parse(&fields.join("^"));
        let start = Instant::now();
        let mut timers = Cooldowns::default();
        timers.refresh(42, 2000, start);
        timers.resolve(&names, start);
        assert_eq!(timers.remaining(42, start), Duration::from_secs(4));
        assert_eq!(timers.remaining(99, start), Duration::from_millis(1500));
        assert_eq!(
            timers.remaining(42, start + Duration::from_secs(4)),
            Duration::ZERO
        );
        timers.refresh(42, u32::MAX, start);
        timers.resolve(&names, start);
        assert_eq!(timers.remaining(42, start), Duration::from_millis(1500));
        let mut unknown = Cooldowns::default();
        unknown.refresh(99, 0, start);
        unknown.resolve(&names, start);
        assert_eq!(unknown.remaining(99, start), Duration::ZERO);
        fields[15] = "invalid";
        let malformed = SpellNames::parse(&fields.join("^"));
        assert_eq!(malformed.label(42), "Synthetic spell");
        assert!(malformed.timing(42).is_none());
    }
}

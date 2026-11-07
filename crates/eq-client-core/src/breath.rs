//! Local air for ordinary Titanium immersion. Health remains a server report.
//!
//! Provenance: the water-breathing effect and races, the breath-level
//! effect, the capacity formula and the fixed racial reserves, the one
//! drowning tick of an eighth of maximum health per second, and the refill
//! on the next due tick come from the author's reading of the Titanium
//! program, whose addresses are not yet cited in this repository. Until they
//! are, treat every constant in this module as inferred (recording checklist
//! 6.393).
use crate::environment::Tick;
use std::{num::NonZeroU32, time::Instant};

/// Water-breathing spell effect.
pub const WATER_BREATHING: u32 = 14;
/// Percentage used to calculate the air reserve.
pub const SET_BREATH_LEVEL: u32 = 246;

/// Whether the race itself can breathe in water.
#[must_use]
pub const fn water_breathing_race(race: u32) -> bool {
    matches!(race, 24 | 35 | 74)
}

/// Titanium's air capacity in one-second ticks. The racial reserves are
/// fixed. Other races use effective STA and the breath-level percentage.
#[must_use]
pub fn titanium_capacity(race: u32, stamina: Option<i64>, breath_level: i64) -> Option<NonZeroU32> {
    let ticks = match race {
        128 => 127,
        330 => 256,
        _ => {
            let percent = if breath_level > 0 { breath_level } else { 100 };
            let ticks = stamina?.saturating_sub(30).saturating_mul(percent) / 100;
            if ticks < 10 { 10 } else { ticks.min(percent) }
        }
    };
    NonZeroU32::new(u32::try_from(ticks).ok()?)
}

/// The facts needed to run the air counter this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sample {
    /// The character can breathe, either in air or through an active effect.
    CanBreathe,
    /// The character's head is in a nonbreathable region and air capacity is known.
    Submerged(NonZeroU32),
    /// The region or a necessary character input is unresolved.
    Unknown,
}

/// A visible Air Remaining reading, measured in ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Air {
    /// Current air, before clamping to the displayed capacity.
    pub remaining: u32,
    /// Capacity under the current effects.
    pub capacity: NonZeroU32,
}

impl Air {
    /// Fraction of the gauge to fill.
    #[must_use]
    #[allow(clippy::cast_precision_loss, reason = "fraction for a UI gauge")]
    pub fn ratio(self) -> f32 {
        self.remaining.min(self.capacity.get()) as f32 / self.capacity.get() as f32
    }
}

/// One admission's air and tick clock. No hit-point state is kept here.
#[derive(Debug, Default)]
pub struct Breath {
    remaining: Option<u32>,
    clock: Tick,
    air: Option<Air>,
}

impl Breath {
    /// The gauge is visible only during known breath depletion.
    #[must_use]
    pub const fn air(&self) -> Option<Air> {
        self.air
    }

    /// Breaks elapsed continuity without granting air. Used on correction
    /// or while a required observation is missing.
    pub const fn interrupt(&mut self) {
        self.clock.interrupt();
        self.air = None;
    }

    /// Samples the present state and, at most once per elapsed second,
    /// consumes air or proposes a damage amount. Late frames never replay
    /// missed ticks. The first empty tick follows the last depletion tick.
    pub fn sample(
        &mut self,
        now: Instant,
        sample: Sample,
        damage_maximum: Option<u32>,
    ) -> Option<u32> {
        let capacity = match sample {
            Sample::CanBreathe => {
                if self.clock.due(now) {
                    self.remaining = None;
                }
                self.air = None;
                return None;
            }
            Sample::Unknown => {
                self.interrupt();
                return None;
            }
            Sample::Submerged(capacity) => capacity,
        };
        let remaining = self.remaining.get_or_insert(capacity.get());
        let mut damage = None;
        if self.clock.due(now) {
            if *remaining == 0 {
                damage = damage_maximum.map(|hp| hp / 8).filter(|&amount| amount > 0);
            } else {
                *remaining -= 1;
            }
        }
        self.air = Some(Air {
            remaining: *remaining,
            capacity,
        });
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn titanium_breath_uses_stamina_percent_caps_and_racial_reserves() {
        let ticks = |race, sta, percent| titanium_capacity(race, sta, percent).map(NonZeroU32::get);
        assert_eq!(ticks(1, Some(75), 0), Some(45));
        assert_eq!(ticks(1, Some(75), 150), Some(67));
        assert_eq!(ticks(1, Some(200), 0), Some(100));
        assert_eq!(ticks(1, Some(31), 0), Some(10));
        assert_eq!(ticks(1, Some(200), 5), Some(10));
        assert_eq!(ticks(1, None, 0), None);
        assert_eq!(ticks(128, None, 150), Some(127));
        assert_eq!(ticks(330, None, 100), Some(256));
    }

    #[test]
    fn breath_refills_and_proposes_one_damage_tick_only_after_depletion() {
        let mut breath = Breath::default();
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let wet = Sample::Submerged(NonZeroU32::MIN.saturating_add(1));
        assert_eq!(breath.sample(at(0), wet, Some(103)), None);
        assert_eq!(breath.sample(at(1), wet, Some(103)), None);
        // A long gap consumes one tick, and a second update there consumes none.
        assert_eq!(breath.sample(at(15), wet, Some(103)), None);
        assert_eq!(breath.air().unwrap().remaining, 0);
        assert_eq!(breath.sample(at(15), wet, Some(103)), None);
        assert_eq!(breath.sample(at(16), wet, Some(103)), Some(12));
        assert_eq!(breath.sample(at(17), wet, None), None);
        assert_eq!(breath.sample(at(18), wet, Some(7)), None);

        assert_eq!(breath.sample(at(19), Sample::CanBreathe, Some(103)), None);
        assert!(breath.air().is_none());
        assert_eq!(breath.sample(at(20), wet, Some(103)), None);
        assert_eq!(breath.air().unwrap().remaining, 1);
    }

    #[test]
    fn brief_air_or_water_breathing_hides_the_gauge_without_refilling() {
        let mut breath = Breath::default();
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let wet = Sample::Submerged(NonZeroU32::MIN.saturating_add(1));
        for millis in [0, 1_000, 2_000] {
            assert_eq!(breath.sample(at(millis), wet, Some(80)), None);
        }
        // Air and a breathing effect both supply CanBreathe. The first
        // interval misses the next tick, and the second includes one.
        breath.sample(at(2_400), Sample::CanBreathe, Some(80));
        assert!(breath.air().is_none());
        breath.sample(at(2_600), wet, Some(80));
        assert_eq!(breath.air().unwrap().remaining, 0);
        assert_eq!(breath.sample(at(3_000), wet, Some(80)), Some(10));
        breath.sample(at(3_300), Sample::CanBreathe, Some(80));
        breath.sample(at(4_000), Sample::CanBreathe, Some(80));
        breath.sample(at(4_200), wet, Some(80));
        assert_eq!(breath.air().unwrap().remaining, 2);
    }

    #[test]
    fn unknown_water_and_corrections_break_timing_without_refilling() {
        let mut breath = Breath::default();
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let wet = Sample::Submerged(NonZeroU32::MIN);
        breath.sample(at(0), wet, Some(80));
        breath.sample(at(1), wet, Some(80));
        assert_eq!(breath.air().unwrap().remaining, 0);
        breath.sample(at(2), Sample::Unknown, Some(80));
        assert!(breath.air().is_none());
        assert_eq!(breath.sample(at(50), wet, Some(80)), None);
        assert_eq!(breath.air().unwrap().remaining, 0);
        breath.interrupt();
        assert_eq!(breath.sample(at(51), wet, Some(80)), None);
        assert_eq!(breath.sample(at(52), wet, Some(80)), Some(10));
    }
}

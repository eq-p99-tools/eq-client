//! Deterministic local magnitudes, before effect-specific and server modifiers.
use super::Effect;

impl Effect {
    /// Evaluates supported level-scaling rules with signed caps.
    /// Time-dependent, random, and unknown formulas return None. Instrument,
    /// stacking, focus, and server-specific adjustments are not applied here.
    #[must_use]
    pub fn base_magnitude(&self, caster_level: u16) -> Option<i64> {
        let base = i64::from(self.base);
        let cap = i64::from(self.maximum);
        let magnitude = base.abs();
        let level = i64::from(caster_level);
        let descending = cap != 0 && cap < base;
        let direction = if descending { -1 } else { 1 };
        let mut value = match self.formula {
            0 | 100 => magnitude,
            60 | 70 => magnitude / 100,
            1..=99 => magnitude + level * i64::from(self.formula),
            101 => direction * (magnitude + level / 2),
            102..=105 => direction * (magnitude + level * i64::from(self.formula - 101)),
            109 => direction * (magnitude + level / 4),
            110 => magnitude + level / 6,
            111 => direction * (magnitude + 6 * (level - 16)),
            112 => direction * (magnitude + 8 * (level - 24)),
            113 => direction * (magnitude + 10 * (level - 34)),
            114 => direction * (magnitude + 15 * (level - 44)),
            115 => magnitude + 7 * (level - 15).max(0),
            116 => magnitude + 10 * (level - 24).max(0),
            117 => magnitude + 13 * (level - 34).max(0),
            118 => magnitude + 20 * (level - 44).max(0),
            119 => magnitude + level / 8,
            121 => magnitude + level / 3,
            _ => return None,
        };
        if cap != 0 {
            value = if descending {
                value.max(cap)
            } else {
                value.min(cap)
            };
        }
        Some(if base < 0 && value > 0 { -value } else { value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn effect(base: i32, maximum: i32, formula: u32) -> Effect {
        Effect {
            id: 7,
            base,
            limit: 0,
            maximum,
            formula,
        }
    }
    #[test]
    fn growth_caps_and_penalties_preserve_direction() {
        for (base, cap, level, expected) in [
            (10, 20, 1, 11),
            (10, 20, 60, 20),
            (-10, 20, 60, -20),
            (-10, -20, 1, -11),
            (-10, -20, 60, -20),
            (50, 30, 1, 30),
        ] {
            assert_eq!(effect(base, cap, 102).base_magnitude(level), Some(expected));
        }
        assert_eq!(effect(-10, 0, 100).base_magnitude(60), Some(-10));
    }
    #[test]
    fn thresholds_rounding_and_special_divisors_are_explicit() {
        assert_eq!(effect(10, 0, 101).base_magnitude(3), Some(11));
        assert_eq!(effect(20, 0, 115).base_magnitude(15), Some(20));
        assert_eq!(effect(20, 0, 115).base_magnitude(16), Some(27));
        for formula in [60, 70] {
            assert_eq!(effect(250, 0, formula).base_magnitude(60), Some(2));
        }
        assert_eq!(effect(10, 0, 4).base_magnitude(5), Some(30));
    }
    #[test]
    fn unknown_rules_and_extreme_values_do_not_become_zero_or_overflow() {
        for formula in [106, 107, 108, 120, 122, 123, 999, u32::MAX] {
            assert_eq!(effect(10, 20, formula).base_magnitude(60), None);
        }
        assert_eq!(
            effect(i32::MIN, 0, 100).base_magnitude(u16::MAX),
            Some(i64::from(i32::MIN))
        );
        assert_eq!(
            effect(i32::MAX, 0, 99).base_magnitude(u16::MAX),
            Some(i64::from(i32::MAX) + i64::from(u16::MAX) * 99)
        );
    }
}

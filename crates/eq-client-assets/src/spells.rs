//! Numeric spell mechanics from the user's patched `spells_us.txt`.
//! Field positions follow the `SPDat` layout; these are local base values, not server state.
mod definitions;
mod magnitude;
pub mod resources;

pub use definitions::{Definition, Definitions, Timing};

/// One effect slot, preserving unsupported effect and formula numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effect {
    /// Effect identifier; 254 is a conventional empty slot.
    pub id: u32,
    /// Signed base value, including penalties.
    pub base: i32,
    /// Effect-specific secondary parameter.
    pub limit: i32,
    /// Signed bound used by the effect formula.
    pub maximum: i32,
    /// Scaling formula identifier.
    pub formula: u32,
}

/// Local duration semantics before server rules or focus effects are applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaseDuration {
    /// A finite number of six-second base ticks.
    Ticks(u32),
    /// Persists until an explicit removal condition.
    Permanent,
    /// Persists while within the aura's scope.
    Aura,
}

/// Alternate duration metadata, conventionally named `PvP` duration in `SPDat`.
/// Its presence does not establish which rules a particular server uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlternateDuration {
    /// Raw formula identifier, retaining unknown server-specific values.
    pub formula: u32,
    /// Raw duration parameter; its interpretation depends on the formula.
    pub duration: u32,
}

/// Numeric data for a spell's local base effects, with optional extended fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mechanics {
    /// Duration scaling formula, not an era or patch selector.
    pub duration_formula: u32,
    /// Finite duration cap; zero means uncapped.
    pub duration_cap: u32,
    /// Fields 181/182, if both are valid; never automatically replaces primary duration.
    pub alternate_duration: Option<AlternateDuration>,
    /// All twelve slots in original order, including empty/unknown effects.
    pub effects: [Effect; 12],
}

impl Mechanics {
    /// Whether the spell takes effect at once. Only an explicit zero
    /// duration is instant; unknown duration rules stay unresolved.
    #[must_use]
    pub fn instant(&self) -> bool {
        self.duration_formula == 0
            && self.duration_cap == 0
            && self
                .alternate_duration
                .is_some_and(|alternate| alternate.formula == 0 && alternate.duration == 0)
    }

    /// Parses a complete numeric projection; missing/malformed fields remain unavailable.
    #[must_use]
    pub fn from_fields(fields: &[&str]) -> Option<Self> {
        let signed = |index: usize| fields.get(index)?.parse::<i32>().ok();
        let unsigned = |index: usize| fields.get(index)?.parse::<u32>().ok();
        let mut effects = Vec::with_capacity(12);
        for slot in 0..12 {
            effects.push(Effect {
                id: unsigned(86 + slot)?,
                base: signed(20 + slot)?,
                limit: signed(32 + slot)?,
                maximum: signed(44 + slot)?,
                formula: unsigned(70 + slot)?,
            });
        }
        Some(Self {
            duration_formula: unsigned(16)?,
            duration_cap: unsigned(17)?,
            alternate_duration: unsigned(181)
                .zip(unsigned(182))
                .map(|(formula, duration)| AlternateDuration { formula, duration }),
            effects: effects.try_into().ok()?,
        })
    }

    /// Evaluates modern `EQEmu` primary duration rules, not an authoritative expiration time.
    /// P99 and other dialects can differ; alternate duration selection is not inferred.
    /// Unknown formulas return None instead of implying an instant or permanent spell.
    #[must_use]
    pub fn base_duration(&self, caster_level: u16) -> Option<BaseDuration> {
        let level = u32::from(caster_level);
        let ticks = match self.duration_formula {
            0 => 0,
            1 => (level / 2).max(1),
            2 => (level / 2).max(1) + 5,
            3 => level * 30,
            4 => 50,
            5 => 2,
            6 => level / 2 + 2,
            7 => level,
            8 => level + 10,
            9 => level * 2 + 10,
            10 => level * 3 + 10,
            11 => (level + 3) * 30,
            12 => (level / 4).max(1),
            13 => level * 4 + 10,
            14 => (level + 2) * 5,
            15 => (level + 10) * 10,
            50 => return Some(BaseDuration::Permanent),
            51 => return Some(BaseDuration::Aura),
            200.. => self.duration_formula,
            _ => return None,
        };
        Some(BaseDuration::Ticks(if self.duration_cap == 0 {
            ticks
        } else {
            ticks.min(self.duration_cap)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alternate_duration_is_optional_and_does_not_override_primary_rules() {
        let mut fields = vec!["0"; 183];
        fields[16] = "11";
        fields[17] = "270";
        fields[181] = "999";
        fields[182] = "13";
        let mechanics = Mechanics::from_fields(&fields).unwrap();
        assert_eq!(
            mechanics.alternate_duration,
            Some(AlternateDuration {
                formula: 999,
                duration: 13,
            })
        );
        assert_eq!(mechanics.base_duration(1), Some(BaseDuration::Ticks(120)));
        for length in [98, 145, 181, 182] {
            let partial = Mechanics::from_fields(&fields[..length]).unwrap();
            assert_eq!(partial.alternate_duration, None);
            assert_eq!(partial.base_duration(1), mechanics.base_duration(1));
        }
        for invalid in ["bad", "-1", "4294967296"] {
            fields[182] = invalid;
            assert_eq!(
                Mechanics::from_fields(&fields).unwrap().alternate_duration,
                None
            );
        }
        fields[181] = "0";
        fields[182] = "0";
        assert_eq!(
            Mechanics::from_fields(&fields).unwrap().alternate_duration,
            Some(AlternateDuration {
                formula: 0,
                duration: 0
            })
        );
        fields[181] = "bad";
        assert_eq!(
            Mechanics::from_fields(&fields).unwrap().alternate_duration,
            None
        );
    }
    #[test]
    fn complete_slots_preserve_signed_values_and_unknown_identifiers() {
        let mut fields = vec!["0"; 98];
        fields[16] = "11";
        fields[17] = "270";
        fields[20] = "-10";
        fields[32] = "-2";
        fields[44] = "-30";
        fields[70] = "999";
        fields[86] = "654";
        fields[97] = "254";
        let mechanics = Mechanics::from_fields(&fields).unwrap();
        assert_eq!(
            mechanics.effects[0],
            Effect {
                id: 654,
                base: -10,
                limit: -2,
                maximum: -30,
                formula: 999
            }
        );
        assert_eq!(mechanics.effects[11].id, 254);
        assert!(Mechanics::from_fields(&fields[..97]).is_none());
        fields[20] = "bad";
        assert!(Mechanics::from_fields(&fields).is_none());
    }
    #[test]
    fn duration_caps_special_rules_and_unknowns_stay_distinct() {
        let mut mechanics = Mechanics::from_fields(&vec!["0"; 98]).unwrap();
        mechanics.duration_formula = 11;
        mechanics.duration_cap = 270;
        assert_eq!(mechanics.base_duration(1), Some(BaseDuration::Ticks(120)));
        assert_eq!(mechanics.base_duration(60), Some(BaseDuration::Ticks(270)));
        mechanics.duration_formula = 50;
        assert_eq!(mechanics.base_duration(1), Some(BaseDuration::Permanent));
        mechanics.duration_formula = 51;
        assert_eq!(mechanics.base_duration(1), Some(BaseDuration::Aura));
        mechanics.duration_formula = 99;
        assert_eq!(mechanics.base_duration(1), None);
    }
}

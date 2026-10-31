//! Numeric spell mechanics from the user's installed spell file
//! (`spells_us.txt` on Titanium, `spells_en.txt` on the Mac-era client).
//! Field positions follow each generation's [`Layout`]; these are local base
//! values, not server state.
mod definitions;
pub mod effects;
mod magnitude;
pub mod resources;

pub use definitions::{Definition, Definitions, Timing};

/// Where a client generation's spell file keeps the fields this client
/// reads: the file's name in the installation and the first column of each
/// block, counted from 0. A field a generation's layout leaves out is
/// unavailable for every spell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    /// The file's name in the installation.
    pub file: &'static str,
    base: usize,
    limit: Option<usize>,
    maximum: usize,
    formula: usize,
    effects: usize,
    icon: Option<usize>,
    gem_icon: Option<usize>,
    gem_tint: Option<(usize, &'static [u8])>,
    casting_animation: Option<usize>,
    effect: Option<usize>,
    alternate_duration: Option<usize>,
}

impl Layout {
    /// Titanium's `spells_us.txt`, the `SPDat` layout (203 columns in its
    /// program's parser, 0x44dbf0). Its program draws a gem's icon from the
    /// book icon column (144), so it has no gem icon column of its own
    /// (column 57 is read by no gem or book draw), and tints the gem by
    /// column 98 (spell-file column report of 10-31, 03:25Z).
    pub const TITANIUM: Self = Self {
        file: "spells_us.txt",
        base: 20,
        limit: Some(32),
        maximum: 44,
        formula: 70,
        effects: 86,
        icon: Some(144),
        gem_icon: None,
        gem_tint: Some((98, definitions::TITANIUM_GEM_TINTS)),
        casting_animation: Some(120),
        effect: Some(145),
        alternate_duration: Some(181),
    };

    /// The Mac-era client's `spells_en.txt` (147 fields a row, a few rows
    /// of 139; its program's parser, 0x4d6b40, reads 141 and ignores the
    /// rest). It keeps no limit block. Seen in use in the program
    /// (spell-file column report of 10-31, 03:25Z): the name (1), the fade
    /// text (8), the bases (20-31), maximums (32-43), formulas (58-69) and
    /// effect ids (74-85), the gem tint (86, with 18 as AE duration) and the
    /// gem's cell in the skin's gem icons (131). The book icon's column is
    /// not found yet, and the casting gesture, particle effect (132, not
    /// used yet) and alternate duration are unavailable. Range (9), cast,
    /// recovery and recast times (13-15), duration (16, 17) and mana (19)
    /// are `EQEmu`'s names by position: the program reads their width and
    /// type, not their meaning (recording checklist item 6.289).
    pub const EQMAC: Self = Self {
        file: "spells_en.txt",
        base: 20,
        limit: None,
        maximum: 32,
        formula: 58,
        effects: 74,
        icon: None,
        gem_icon: Some(131),
        gem_tint: Some((86, definitions::EQMAC_GEM_TINTS)),
        casting_animation: None,
        effect: None,
        alternate_duration: None,
    };
}

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

    /// Parses a complete numeric projection from a Titanium line; see
    /// [`Mechanics::read`].
    #[must_use]
    pub fn from_fields(fields: &[&str]) -> Option<Self> {
        Self::read(fields, &Layout::TITANIUM)
    }

    /// Parses a complete numeric projection from a line laid out as
    /// `layout`; missing/malformed fields remain unavailable, and a limit
    /// the layout has no block for is 0.
    #[must_use]
    pub fn read(fields: &[&str], layout: &Layout) -> Option<Self> {
        let signed = |index: usize| fields.get(index)?.parse::<i32>().ok();
        let unsigned = |index: usize| fields.get(index)?.parse::<u32>().ok();
        let mut effects = Vec::with_capacity(12);
        for slot in 0..12 {
            effects.push(Effect {
                id: unsigned(layout.effects + slot)?,
                base: signed(layout.base + slot)?,
                limit: match layout.limit {
                    Some(limit) => signed(limit + slot)?,
                    None => 0,
                },
                maximum: signed(layout.maximum + slot)?,
                formula: unsigned(layout.formula + slot)?,
            });
        }
        Some(Self {
            duration_formula: unsigned(16)?,
            duration_cap: unsigned(17)?,
            alternate_duration: layout
                .alternate_duration
                .and_then(|column| unsigned(column).zip(unsigned(column + 1)))
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

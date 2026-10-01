//! Every spell's local definition from the user's `spells_us.txt`: its name,
//! icon, base costs and timing, and its mechanics. These are the installed
//! client's base values, not what a server adjusts them to.
use super::Mechanics;
use std::{collections::BTreeMap, io, path::Path};

/// One spell as the installed client defines it. Fields that are missing or
/// malformed on its line stay unavailable rather than defaulting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    /// The spell's name, if the line has one.
    pub name: Option<String>,
    /// Field 144: the cell in the default UI's spell icon sheets.
    pub icon: Option<u32>,
    /// Field 19: the unmodified mana cost.
    pub mana: Option<u32>,
    /// Field 13: the unmodified cast time, in milliseconds.
    pub cast_ms: Option<u32>,
    /// Field 9: the range, if finite and not negative.
    pub range: Option<f32>,
    /// Fields 14 and 15: recovery and same-spell reuse, in milliseconds.
    pub timing: Option<Timing>,
    /// The spell's numeric mechanics.
    pub mechanics: Option<Mechanics>,
}

/// How long a cast holds the caster and the spell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// Before any spell can be cast again.
    pub recovery_ms: u32,
    /// Before this spell can be cast again.
    pub recast_ms: u32,
}

/// The installed client's spells, by id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definitions(BTreeMap<u32, Definition>);

impl Definitions {
    /// Reads `spells_us.txt` from the installation.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read.
    pub fn read(eq_directory: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(eq_directory.join("spells_us.txt"))?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Parses the file's `^`-separated lines; a line keeps its valid fields
    /// even when others are unavailable or malformed.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut spells = BTreeMap::new();
        for line in text.lines() {
            let fields: Vec<_> = line.split('^').take(183).collect();
            let Some(id) = fields.first().and_then(|field| field.parse::<u32>().ok()) else {
                continue;
            };
            let number = |index: usize| fields.get(index).and_then(|field| field.parse().ok());
            let definition = Definition {
                name: fields
                    .get(1)
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
                icon: number(144),
                mana: number(19),
                cast_ms: number(13),
                range: fields
                    .get(9)
                    .and_then(|field| field.parse::<f32>().ok())
                    .filter(|range| range.is_finite() && *range >= 0.0),
                timing: number(14)
                    .zip(number(15))
                    .map(|(recovery_ms, recast_ms)| Timing {
                        recovery_ms,
                        recast_ms,
                    }),
                mechanics: Mechanics::from_fields(&fields),
            };
            spells.insert(id, definition);
        }
        Self(spells)
    }

    /// A spell's definition.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Definition> {
        self.0.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_keeps_its_valid_fields_and_skips_malformed_ones() {
        let mut fields = vec![""; 183];
        fields[0] = "42";
        fields[1] = " Synthetic spell ";
        fields[9] = "-5";
        fields[13] = "1500";
        fields[14] = "2250";
        fields[15] = "oops";
        fields[19] = "10";
        fields[144] = "7";
        let spells = Definitions::parse(&format!("{}\nnot a spell\n", fields.join("^")));
        let spell = spells.get(42).expect("parsed");
        assert_eq!(spell.name.as_deref(), Some("Synthetic spell"));
        assert_eq!(spell.icon, Some(7));
        assert_eq!(spell.mana, Some(10));
        assert_eq!(spell.cast_ms, Some(1500));
        assert_eq!(spell.range, None);
        assert_eq!(spell.timing, None);
        assert!(spells.get(0).is_none());
    }
}

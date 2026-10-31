//! Every spell's local definition from the user's spell file: its name,
//! icon, base costs and timing, and its mechanics. These are the installed
//! client's base values, not what a server adjusts them to.
use super::{Layout, Mechanics};
use std::{collections::BTreeMap, io, path::Path};

/// One spell as the installed client defines it. Fields that are missing or
/// malformed on its line stay unavailable rather than defaulting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    /// The spell's name, if the line has one.
    pub name: Option<String>,
    /// The cell in the default UI's spell icon sheets (Titanium field 144;
    /// unread on the Mac-era client).
    pub icon: Option<u32>,
    /// The cell in the skin's gem icons (`A_SpellGems`) that a gem shows
    /// in place of [`Definition::icon`], on a client whose program draws
    /// its gems from a column of their own: the Mac-era client's field 131.
    /// Titanium's gems draw the book icon, so it has none.
    pub gem_icon: Option<u32>,
    /// The colour, as ARGB, that tints a gem's background while it holds
    /// the spell (Titanium field 98, Mac-era field 86, and field 18 for one
    /// colour). See [`gem_tint`].
    pub gem_tint: Option<u32>,
    /// Field 19: the unmodified mana cost.
    pub mana: Option<u32>,
    /// Field 13: the unmodified cast time, in milliseconds.
    pub cast_ms: Option<u32>,
    /// Field 9: the range, if finite and not negative.
    pub range: Option<f32>,
    /// Fields 14 and 15: recovery and same-spell reuse, in milliseconds.
    pub timing: Option<Timing>,
    /// Field 120 (unread on the Mac-era client): the motion the caster plays while casting, as the
    /// servers' animation number, such as 42 to 44 for the three casting
    /// gestures.
    pub casting_animation: Option<u16>,
    /// Field 145 (unread on the Mac-era client): the particle effect the installed client draws for the
    /// spell, by its record in the client's effect file.
    pub effect: Option<u32>,
    /// The spell's numeric mechanics.
    pub mechanics: Option<Mechanics>,
    /// Field 8: what the client says when the spell fades from the player.
    pub fades: Option<String>,
}

/// How long a cast holds the caster and the spell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// Before any spell can be cast again.
    pub recovery_ms: u32,
    /// Before this spell can be cast again.
    pub recast_ms: u32,
}

/// Which of the gem colours each value of Titanium's field 98 picks, as
/// its program maps it (the 44-entry table at 0x44d103).
pub(super) const TITANIUM_GEM_TINTS: &[u8] = &[
    0, 0, 1, 2, 1, 0, 3, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 1, 1, 1, 0, 4, 1, 4, 1,
    5, 5, 0, 5, 5, 5, 0, 5, 2, 2, 1, 0,
];

/// Which of the gem colours each value of the Mac-era client's field 86
/// picks, as its program maps it (the 42-entry table at 0x41911d, the same
/// at all three gem sites). It differs from Titanium's at 34 and 38.
pub(super) const EQMAC_GEM_TINTS: &[u8] = &[
    0, 0, 1, 2, 1, 0, 3, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 1, 1, 1, 0, 4, 1, 4, 1,
    5, 5, 5, 5, 5, 5, 5, 5, 2, 2,
];

/// The colour, as ARGB, that tints a gem's background for a spell whose
/// tint field is `index`: the case `cases` picks, where the fifth case
/// depends on whether field 18 is under 3000 (unavailable leaves the colour
/// unknown). A value past the table gives opaque black, as read; how the
/// piece draw treats that colour is untraced.
#[must_use]
pub fn gem_tint(cases: &[u8], index: u32, field_18: Option<u32>) -> Option<u32> {
    let case = usize::try_from(index)
        .ok()
        .and_then(|index| cases.get(index))
        .copied()
        .unwrap_or(5);
    Some(match case {
        0 => 0xffde_2121,
        1 => 0xff52_42de,
        2 => 0xff9c_31de,
        3 => 0xffce_de29,
        4 if field_18? < 3000 => 0xff42_de63,
        4 => 0xffde_7b21,
        _ => 0xff00_0000,
    })
}

/// The installed client's spells, by id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definitions(BTreeMap<u32, Definition>);

impl Definitions {
    /// Reads the installation's spell file, laid out as `layout`.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read.
    pub fn read(eq_directory: &Path, layout: &Layout) -> io::Result<Self> {
        let bytes = std::fs::read(eq_directory.join(layout.file))?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes), layout))
    }

    /// Parses the file's `^`-separated lines; a line keeps its valid fields
    /// even when others are unavailable or malformed.
    #[must_use]
    pub fn parse(text: &str, layout: &Layout) -> Self {
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
                icon: layout.icon.and_then(number),
                gem_icon: layout.gem_icon.and_then(number),
                gem_tint: layout
                    .gem_tint
                    .and_then(|(column, cases)| gem_tint(cases, number(column)?, number(18))),
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
                casting_animation: layout
                    .casting_animation
                    .and_then(|column| fields.get(column))
                    .and_then(|field| field.parse().ok()),
                effect: layout.effect.and_then(number),
                mechanics: Mechanics::read(&fields, layout),
                fades: fields
                    .get(8)
                    .map(|text| text.trim())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
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
        fields[8] = " The synthetic spell fades. ";
        fields[9] = "-5";
        fields[13] = "1500";
        fields[14] = "2250";
        fields[15] = "oops";
        fields[19] = "10";
        fields[120] = "44";
        fields[144] = "7";
        fields[145] = "86";
        let spells = Definitions::parse(
            &format!("{}\nnot a spell\n", fields.join("^")),
            &Layout::TITANIUM,
        );
        let spell = spells.get(42).expect("parsed");
        assert_eq!(spell.name.as_deref(), Some("Synthetic spell"));
        assert_eq!(spell.fades.as_deref(), Some("The synthetic spell fades."));
        assert_eq!(spell.icon, Some(7));
        assert_eq!(spell.mana, Some(10));
        assert_eq!(spell.cast_ms, Some(1500));
        assert_eq!(spell.casting_animation, Some(44));
        assert_eq!(spell.effect, Some(86));
        assert_eq!(spell.range, None);
        assert_eq!(spell.timing, None);
        assert!(spells.get(0).is_none());
    }

    #[test]
    fn a_gems_tint_follows_the_programs_table() {
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 0, None), Some(0xffde_2121));
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 2, None), Some(0xff52_42de));
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 40, None), Some(0xff9c_31de));
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 6, None), Some(0xffce_de29));
        assert_eq!(
            gem_tint(TITANIUM_GEM_TINTS, 8, Some(2999)),
            Some(0xff42_de63)
        );
        assert_eq!(
            gem_tint(TITANIUM_GEM_TINTS, 8, Some(3000)),
            Some(0xffde_7b21)
        );
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 8, None), None);
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 33, None), Some(0xff00_0000));
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 44, None), Some(0xff00_0000));
        assert_eq!(gem_tint(TITANIUM_GEM_TINTS, 34, None), Some(0xffde_2121));
        assert_eq!(gem_tint(EQMAC_GEM_TINTS, 34, None), Some(0xff00_0000));
        assert_eq!(gem_tint(EQMAC_GEM_TINTS, 41, None), Some(0xff9c_31de));
        assert_eq!(gem_tint(EQMAC_GEM_TINTS, 42, None), Some(0xff00_0000));
        let mut fields = vec!["0"; 183];
        fields[0] = "5";
        fields[98] = "21";
        fields[18] = "100";
        let spells = Definitions::parse(&fields.join("^"), &Layout::TITANIUM);
        assert_eq!(spells.get(5).unwrap().gem_tint, Some(0xff42_de63));
        fields[86] = "34";
        let mac = Definitions::parse(&fields.join("^"), &Layout::EQMAC);
        assert_eq!(mac.get(5).unwrap().gem_tint, Some(0xff00_0000));
    }

    #[test]
    fn a_mac_era_line_reads_its_own_columns_and_leaves_the_unread_out() {
        let mut fields = vec!["0"; 147];
        fields[0] = "242";
        fields[1] = "Synthetic snare";
        fields[13] = "1500";
        fields[19] = "25";
        fields[21] = "-40";
        fields[33] = "-55";
        fields[44] = "2501";
        fields[45] = "2054";
        fields[131] = "17";
        fields[59] = "102";
        fields[74] = "10";
        fields[75] = "3";
        fields[76..86].fill("254");
        let spells = Definitions::parse(&fields.join("^"), &Layout::EQMAC);
        let spell = spells.get(242).expect("parsed");
        assert_eq!(spell.name.as_deref(), Some("Synthetic snare"));
        assert_eq!((spell.cast_ms, spell.mana), (Some(1500), Some(25)));
        assert_eq!(spell.gem_icon, Some(17));
        assert_eq!(spell.icon, None);
        assert_eq!(spell.casting_animation, None);
        assert_eq!(spell.effect, None);
        let mechanics = spell.mechanics.as_ref().expect("mechanics");
        let snare = mechanics.effects[1];
        assert_eq!(
            (
                snare.id,
                snare.base,
                snare.limit,
                snare.maximum,
                snare.formula
            ),
            (3, -40, 0, -55, 102)
        );
        assert_eq!(mechanics.effects[0].id, 10);
        assert_eq!(mechanics.alternate_duration, None);
        let titanium = Definitions::parse(&fields.join("^"), &Layout::TITANIUM);
        assert_eq!(titanium.get(242).and_then(|spell| spell.gem_icon), None);
    }

    #[test]
    fn the_casting_animation_is_field_120_and_stays_unknown_when_malformed() {
        let mut fields = vec!["0"; 183];
        fields[0] = "7";
        fields[119] = "43";
        fields[120] = "42";
        fields[121] = "44";
        let parse = |fields: &[&str]| {
            Definitions::parse(&fields.join("^"), &Layout::TITANIUM)
                .get(7)
                .expect("parsed")
                .casting_animation
        };
        assert_eq!(parse(&fields), Some(42));
        for malformed in ["", "-1", "70000", "x"] {
            fields[120] = malformed;
            assert_eq!(parse(&fields), None, "{malformed:?}");
        }
        assert_eq!(parse(&fields[..120]), None);
    }
}

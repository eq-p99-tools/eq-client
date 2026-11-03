//! The installed generation's Skills-window rules and numeric class data.
//! The owned CSVs preserve the constructor's values; words are resolved
//! separately through the installed string table.
mod table;

use crate::ui::InstalledClient;
use std::sync::OnceLock;
use table::SkillTable;
pub use table::{ClassRule, SkillTableError};

/// Whether the available inputs establish that the program lists a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The program's rule lists the row.
    Shown,
    /// The program's rule leaves the row out.
    Hidden,
    /// A class, value or look-ahead input needed for this row is unknown.
    Unresolved,
}

/// Inputs to the program's capped, two-level look-ahead. Absence does not
/// imply a maximum level or a player-spawn classification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookAhead {
    /// Whether the associated spawn has the program's player classification.
    pub player_spawn: Option<bool>,
    /// The upper level the program reads, when independently established.
    pub bound: Option<u8>,
}

impl LookAhead {
    fn levels(self, current: u8) -> (u8, u8) {
        if self.player_spawn == Some(false) || self.bound.is_some_and(|bound| bound <= current) {
            return (current, current);
        }
        let advanced = current.saturating_add(2).min(self.bound.unwrap_or(u8::MAX));
        if self.player_spawn == Some(true) && self.bound.is_some() {
            (advanced, advanced)
        } else {
            (current, advanced)
        }
    }
}

/// The character facts the generation's skill-list rule reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillInputs {
    /// The profile's base player race, not a model or illusion race.
    pub race: u32,
    /// The profile's class, when decoded.
    pub class: Option<u32>,
    /// The character's current level.
    pub level: u8,
    /// Independently established look-ahead inputs.
    pub look_ahead: LookAhead,
}

/// Numeric data and list rules belonging to one installed client generation.
#[derive(Clone, Debug)]
pub struct SkillRules {
    client: InstalledClient,
    table: SkillTable,
}

/// The semantic rank text, before resolving words from installed strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankText {
    /// An argument-free entry in the installed string table.
    String(u32),
    /// String 0x871 with this numeric training-level argument.
    TrainableAt(u8),
    /// No defined rank can be established from the available inputs.
    Unknown,
}

/// The two value columns of a listed skill or language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowText {
    /// The rank column, retaining the generation's sentinel ordering.
    pub rank: RankText,
    /// The numeric column; absence means the program leaves it empty.
    pub value: Option<i32>,
}

impl SkillRules {
    /// Parses a complete, owned constructor table for this generation.
    ///
    /// # Errors
    /// Rejects wrong columns, missing or duplicate cells, inconsistent name
    /// IDs, out-of-range indices and values that cannot fit the read fields.
    pub fn parse(client: InstalledClient, text: &str) -> Result<Self, SkillTableError> {
        Ok(Self {
            client,
            table: SkillTable::parse(client, text)?,
        })
    }

    /// The installed string-table ID for a skill's name, including IDs
    /// outside the older consumer's short skill-name list.
    #[must_use]
    pub fn name_string(&self, skill: u32) -> Option<u32> {
        self.table.name_string(skill)
    }

    /// A defined player class's constructor values. Class zero is unused;
    /// the Mac-era program has no class sixteen, whose data stays unread.
    #[must_use]
    pub fn class_rule(&self, skill: u32, class: u32) -> Option<ClassRule> {
        let last = match self.client {
            InstalledClient::Titanium => 16,
            InstalledClient::EqMac => 15,
        };
        (1..=last)
            .contains(&class)
            .then(|| self.table.class_rule(skill, class))
            .flatten()
    }

    /// Whether the generation lists this skill (Titanium 0x41b0de;
    /// Mac-era 0x4b9f08). Unknown look-ahead inputs matter only if the two
    /// possible ends of the level range give different answers.
    #[must_use]
    pub fn presence(&self, skill: u32, raw: u32, inputs: SkillInputs) -> Presence {
        if skill >= 100
            || (self.client == InstalledClient::Titanium && skill == 57 && inputs.race != 12)
        {
            return Presence::Hidden;
        }
        let Some(class) = inputs.class.and_then(|class| self.class_rule(skill, class)) else {
            return Presence::Unresolved;
        };
        let value = match self.client {
            InstalledClient::EqMac => u16::try_from(raw)
                .ok()
                .map(|value| i32::from(i16::from_le_bytes(value.to_le_bytes()))),
            InstalledClient::Titanium => i32::try_from(raw).ok(),
        };
        let Some(value) = value else {
            return Presence::Unresolved;
        };
        let racial = self.racial_base(skill, inputs.race) != 0;
        let at_level = |level| match self.client {
            InstalledClient::EqMac if value >= 254 || !class.usable() => racial,
            InstalledClient::Titanium
                if !class.usable() || (value == 0 && class.train_level > 1) =>
            {
                racial
            }
            InstalledClient::EqMac | InstalledClient::Titanium => {
                level >= class.train_level || (value >= 1 && racial)
            }
        };
        let (low, high) = inputs.look_ahead.levels(inputs.level);
        match (at_level(low), at_level(high)) {
            (true, true) => Presence::Shown,
            (false, false) => Presence::Hidden,
            _ => Presence::Unresolved,
        }
    }

    /// The racial base for a player's base race (Titanium 0x4184f4,
    /// Mac-era 0x4b9fac). NPC variants never stand in for player races.
    #[must_use]
    pub const fn racial_base(&self, skill: u32, race: u32) -> u16 {
        match (race, skill) {
            (4 | 128, 27) | (4 | 6 | 11, 29) | (8, 40) | (11 | 130, 42) | (12, 57) | (130, 39) => {
                50
            }
            (128, 50) => 100,
            (330, 50) if matches!(self.client, InstalledClient::Titanium) => 125,
            _ => 0,
        }
    }

    /// A rank's installed string ID. The programs compare inclusive upper
    /// bounds 10 through 100, then 2000 (Titanium 0x65caa0; Mac-era
    /// 0x5fe3ec). A language at exactly 100 uses the last rank instead.
    /// Values beyond the defined thresholds retain an unknown rank.
    #[must_use]
    pub fn rank_string(&self, value: u32, language: bool) -> Option<u32> {
        let rank = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 2000]
            .iter()
            .position(|bound| value <= *bound)?;
        let rank = if language && value == 100 { 10 } else { rank };
        Some(0xc98 + u32::try_from(rank).ok()?)
    }

    /// The displayed value and rank before installed-string substitution.
    /// Mac-era 0x4329d6 replaces raw skill 255 with its racial result first;
    /// its numeric column is empty for remaining values 254 and above.
    /// Titanium 0x55475a uses static ranks when no custom-description message
    /// has been received, and formats a later training level only for zero.
    #[must_use]
    pub fn row_text(&self, id: u32, raw: u32, inputs: SkillInputs) -> Option<RowText> {
        let language = (100..132).contains(&id);
        let value = if language {
            i32::from(u8::try_from(raw).ok()?)
        } else if id < 100 {
            match self.client {
                InstalledClient::EqMac if raw == 255 => {
                    i32::from(self.racial_base(id, inputs.race))
                }
                InstalledClient::EqMac => {
                    i32::from(i16::from_le_bytes(u16::try_from(raw).ok()?.to_le_bytes()))
                }
                InstalledClient::Titanium => i32::try_from(raw).ok()?,
            }
        } else {
            return None;
        };
        let class = inputs.class.and_then(|class| self.class_rule(id, class));
        let rank = match self.client {
            InstalledClient::EqMac if value == 254 => class.map_or(RankText::Unknown, |class| {
                RankText::TrainableAt(class.train_level)
            }),
            // The fixed decimal level argument at 0x5fe49c is 66.
            InstalledClient::EqMac if value == 255 => RankText::TrainableAt(66),
            InstalledClient::Titanium if !language && value == 0 => class
                .filter(|class| class.usable() && class.train_level > inputs.level)
                .map_or_else(
                    || self.rank(value, language),
                    |class| RankText::TrainableAt(class.train_level),
                ),
            _ => self.rank(value, language),
        };
        let numeric = match self.client {
            InstalledClient::EqMac => (value < 254).then_some(value),
            InstalledClient::Titanium => Some(value),
        };
        Some(RowText {
            rank,
            value: numeric,
        })
    }

    fn rank(&self, value: i32, language: bool) -> RankText {
        if value < 0 && self.client == InstalledClient::EqMac {
            return RankText::String(0xc98);
        }
        u32::try_from(value)
            .ok()
            .and_then(|value| self.rank_string(value, language))
            .map_or(RankText::Unknown, RankText::String)
    }

    /// Both programs add nonzero language bytes from slots zero through
    /// thirty-one, independently of class skill eligibility.
    #[must_use]
    pub const fn language_present(language: u8, value: u8) -> bool {
        language < 32 && value != 0
    }

    /// The installed language-name string ID (Titanium 0x492595;
    /// Mac-era 0x52d997). The common-language selector remains an explicit
    /// unknown input until the world's corresponding state is established.
    #[must_use]
    pub fn language_name_string(language: u8, common_variant: Option<bool>) -> Option<u32> {
        match language {
            0 => match common_variant {
                Some(true) => Some(0xc29),
                Some(false) => Some(0xc2a),
                None => None,
            },
            1..=24 => Some(0xc7f + u32::from(language)),
            25..=31 => Some(0xc2b),
            _ => None,
        }
    }
}

pub(crate) fn for_client(client: InstalledClient) -> &'static SkillRules {
    static TITANIUM: OnceLock<SkillRules> = OnceLock::new();
    static EQMAC: OnceLock<SkillRules> = OnceLock::new();
    let (cache, csv) = match client {
        InstalledClient::Titanium => (
            &TITANIUM,
            include_str!("../data/skills/skill-tables-titanium-20261103-v2.csv"),
        ),
        InstalledClient::EqMac => (
            &EQMAC,
            include_str!("../data/skills/skill-tables-eqmac-20261103-v2.csv"),
        ),
    };
    cache.get_or_init(|| {
        SkillRules::parse(client, csv).expect("owned constructor skill table is complete")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    fn synthetic(client: InstalledClient) -> String {
        let field = match client {
            InstalledClient::Titanium => "class_usable",
            InstalledClient::EqMac => "class_cap",
        };
        let mut text = format!("skill,class,train_level,{field},name_eqstr\n");
        for skill in 0..100 {
            for class in 0..17 {
                let (level, usable) = match (skill, class) {
                    (10 | 57, 1) => (1, 100),
                    (11 | 27, 1) => (5, 100),
                    _ => (1, 0),
                };
                writeln!(text, "{skill},{class},{level},{usable},{}", 8000 + skill).unwrap();
            }
        }
        text
    }

    fn inputs(race: u32, level: u8) -> SkillInputs {
        SkillInputs {
            race,
            class: Some(1),
            level,
            look_ahead: LookAhead::default(),
        }
    }

    #[test]
    fn both_owned_tables_are_complete_and_unused_class_slots_stay_unavailable() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let rules = client.skill_rules();
            for skill in 0..100 {
                assert!(rules.name_string(skill).is_some());
                assert!(rules.class_rule(skill, 1).is_some());
                assert!(rules.class_rule(skill, 0).is_none());
                assert_eq!(
                    rules.class_rule(skill, 16).is_some(),
                    client == InstalledClient::Titanium
                );
            }
            assert!(rules.name_string(100).is_none());
            assert!(rules.class_rule(0, 17).is_none());
        }
        // The Mac-era default train level must never substitute for usability.
        let rules = InstalledClient::EqMac.skill_rules();
        assert!((0..100).any(|skill| (1..16).any(|class| {
            rules
                .class_rule(skill, class)
                .is_some_and(|rule| rule.train_level == 1 && !rule.usable())
        })));
    }

    #[test]
    fn malformed_and_incomplete_constructor_tables_fail_before_becoming_rules() {
        let client = InstalledClient::Titanium;
        let text = synthetic(client);
        assert!(SkillRules::parse(client, &text).is_ok());
        assert!(matches!(
            SkillRules::parse(InstalledClient::EqMac, &text),
            Err(SkillTableError::Columns)
        ));
        let duplicate = format!("{text}0,0,1,0,8000\n");
        assert!(matches!(
            SkillRules::parse(client, &duplicate),
            Err(SkillTableError::Duplicate { skill: 0, class: 0 })
        ));
        let missing = text
            .lines()
            .filter(|line| !line.starts_with("99,16,"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            SkillRules::parse(client, &missing),
            Err(SkillTableError::Missing {
                skill: 99,
                class: 16
            })
        ));
        for malformed in [
            text.replacen("0,1,1,0,8000", "0,1,1,0,9000", 1),
            text.replacen("0,0,1,0,8000", "100,0,1,0,8000", 1),
            text.replacen("0,0,1,0,8000", "0,17,1,0,8000", 1),
            text.replacen("0,0,1,0,8000", "0,0,256,0,8000", 1),
            text.replacen("0,0,1,0,8000", "0,0,1,65536,8000", 1),
        ] {
            assert!(SkillRules::parse(client, &malformed).is_err());
        }
    }

    #[test]
    fn zero_values_follow_the_generations_distinct_class_rules() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let rules = SkillRules::parse(client, &synthetic(client)).unwrap();
            assert_eq!(rules.presence(10, 0, inputs(1, 10)), Presence::Shown);
            assert_eq!(rules.presence(11, 0, inputs(1, 1)), Presence::Hidden);
            assert_eq!(
                rules.presence(11, 0, inputs(1, 10)),
                if client == InstalledClient::EqMac {
                    Presence::Shown
                } else {
                    Presence::Hidden
                }
            );
            assert_eq!(rules.presence(12, 20, inputs(1, 10)), Presence::Hidden);
            assert_eq!(
                rules.presence(
                    10,
                    0,
                    SkillInputs {
                        class: None,
                        ..inputs(1, 10)
                    }
                ),
                Presence::Unresolved
            );
        }
    }

    #[test]
    fn the_two_level_look_ahead_needs_its_actual_bound_only_at_a_boundary() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let rules = SkillRules::parse(client, &synthetic(client)).unwrap();
            let raw = u32::from(client != InstalledClient::EqMac);
            assert_eq!(rules.presence(11, raw, inputs(1, 1)), Presence::Hidden);
            assert_eq!(rules.presence(11, raw, inputs(1, 4)), Presence::Unresolved);
            assert_eq!(rules.presence(11, raw, inputs(1, 5)), Presence::Shown);
            for (bound, wanted) in [
                (4, Presence::Hidden),
                (5, Presence::Shown),
                (8, Presence::Shown),
            ] {
                let input = SkillInputs {
                    look_ahead: LookAhead {
                        player_spawn: Some(true),
                        bound: Some(bound),
                    },
                    ..inputs(1, 4)
                };
                assert_eq!(rules.presence(11, raw, input), wanted);
            }
            let input = SkillInputs {
                look_ahead: LookAhead {
                    player_spawn: Some(false),
                    bound: None,
                },
                ..inputs(1, 4)
            };
            assert_eq!(rules.presence(11, raw, input), Presence::Hidden);
        }
    }

    #[test]
    fn sentinel_rows_and_racial_bases_do_not_turn_into_positive_rank_heuristics() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let rules = SkillRules::parse(client, &synthetic(client)).unwrap();
            for value in [254, 255] {
                assert_eq!(
                    rules.presence(10, value, inputs(1, 10)),
                    if client == InstalledClient::EqMac {
                        Presence::Hidden
                    } else {
                        Presence::Shown
                    }
                );
                assert_eq!(rules.presence(29, value, inputs(4, 10)), Presence::Shown);
            }
            assert_eq!(rules.presence(27, 0, inputs(4, 10)), Presence::Shown);
            assert_eq!(rules.racial_base(29, 11), 50);
            for npc_race in [77, 81, 94, 98, 112] {
                for skill in [27, 29, 40, 42] {
                    assert_eq!(rules.racial_base(skill, npc_race), 0);
                }
            }
            assert_eq!(
                rules.racial_base(50, 330),
                if client == InstalledClient::Titanium {
                    125
                } else {
                    0
                }
            );
        }
        let titanium = SkillRules::parse(
            InstalledClient::Titanium,
            &synthetic(InstalledClient::Titanium),
        )
        .unwrap();
        assert_eq!(titanium.presence(57, 10, inputs(1, 10)), Presence::Hidden);
        assert_eq!(titanium.presence(57, 0, inputs(12, 10)), Presence::Shown);
        assert_eq!(
            InstalledClient::EqMac
                .skill_rules()
                .presence(10, 65536, inputs(1, 10)),
            Presence::Unresolved
        );
    }

    #[test]
    fn rank_boundaries_and_mastered_languages_use_installed_string_ids() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let rules = client.skill_rules();
            for (value, rank) in [
                (0, 0xc98),
                (10, 0xc98),
                (11, 0xc99),
                (20, 0xc99),
                (100, 0xca1),
                (101, 0xca2),
                (2000, 0xca2),
            ] {
                assert_eq!(rules.rank_string(value, false), Some(rank));
            }
            assert_eq!(rules.rank_string(100, true), Some(0xca2));
            assert_eq!(rules.rank_string(2001, false), None);
        }
    }

    #[test]
    fn row_text_keeps_generation_sentinel_order_and_both_columns() {
        let eqmac =
            SkillRules::parse(InstalledClient::EqMac, &synthetic(InstalledClient::EqMac)).unwrap();
        let titanium = SkillRules::parse(
            InstalledClient::Titanium,
            &synthetic(InstalledClient::Titanium),
        )
        .unwrap();
        let input = inputs(4, 1);
        assert_eq!(
            eqmac.row_text(29, 255, input),
            Some(RowText {
                rank: RankText::String(0xc9c),
                value: Some(50)
            })
        );
        assert_eq!(
            eqmac.row_text(29, 254, input),
            Some(RowText {
                rank: RankText::TrainableAt(1),
                value: None
            })
        );
        assert_eq!(
            eqmac.row_text(131, 255, input),
            Some(RowText {
                rank: RankText::TrainableAt(66),
                value: None
            })
        );
        assert_eq!(
            titanium.row_text(27, 0, input),
            Some(RowText {
                rank: RankText::TrainableAt(5),
                value: Some(0)
            })
        );
        for value in [254, 255] {
            assert_eq!(
                titanium.row_text(29, value, input),
                Some(RowText {
                    rank: RankText::String(0xca2),
                    value: Some(i32::try_from(value).unwrap())
                })
            );
        }
        assert_eq!(
            eqmac.row_text(10, 0xffff, input),
            Some(RowText {
                rank: RankText::String(0xc98),
                value: Some(-1)
            })
        );
        assert_eq!(eqmac.row_text(10, 0x10000, input), None);
        assert_eq!(titanium.row_text(132, 10, input), None);
    }

    #[test]
    fn language_rows_preserve_all_nonzero_slots_and_resolve_names_without_copied_words() {
        for language in 0..32 {
            assert!(!SkillRules::language_present(language, 0));
            for value in [1, 100, 255] {
                assert!(SkillRules::language_present(language, value));
            }
        }
        assert!(!SkillRules::language_present(32, 100));
        for (language, name) in [(1, 0xc80), (24, 0xc97), (25, 0xc2b), (31, 0xc2b)] {
            assert_eq!(SkillRules::language_name_string(language, None), Some(name));
        }
        assert_eq!(SkillRules::language_name_string(0, None), None);
        assert_eq!(
            SkillRules::language_name_string(0, Some(false)),
            Some(0xc2a)
        );
        assert_eq!(SkillRules::language_name_string(0, Some(true)), Some(0xc29));
        assert_eq!(SkillRules::language_name_string(32, None), None);
    }
}

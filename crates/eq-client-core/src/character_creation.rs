//! Editable character creation, independent of widgets and connection I/O.
use crate::creation::{self, NewCharacter, Rules};

/// Stat order shared by the form and the typed network request.
pub const STATS: [&str; 7] = [
    "Strength",
    "Stamina",
    "Agility",
    "Dexterity",
    "Wisdom",
    "Intelligence",
    "Charisma",
];

/// A local draft. Only [`Draft::request`] produces a command-ready character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    /// What the player has typed; normalized only when submitted.
    pub name: String,
    /// Current choices, including the editable final stats.
    pub character: NewCharacter,
    /// The current character list's creation policy.
    pub rules: Rules,
    /// Expansions reported by this world, if any.
    pub expansions: Option<crate::rules::Expansions>,
    /// World `PvP` ruleset used by deity and city choices.
    pub pvp: u32,
}

impl Draft {
    /// Starts a human warrior with all bonus points available to distribute.
    pub fn new(rules: Rules, expansions: Option<crate::rules::Expansions>, pvp: u32) -> Self {
        let mut draft = Self {
            name: String::new(),
            rules,
            expansions,
            pvp,
            character: NewCharacter {
                name: String::new(),
                race: 1,
                class: 1,
                gender: 0,
                deity: 396,
                start_zone: 1,
                stats: [0; 7],
                face: 0,
                cosmetics: creation::Cosmetics::default(),
                tutorial: false,
            },
        };
        draft.reconcile();
        draft.reset_stats();
        draft
    }

    /// Chooses an available race and reconciles dependent choices.
    pub fn race(&mut self, race: u32) {
        if race != self.character.race
            && self
                .rules
                .races(self.expansions)
                .iter()
                .any(|(id, _)| *id == race)
        {
            self.character.race = race;
            self.reconcile();
            self.reset_stats();
        }
    }

    /// Chooses an available class and resets its allocation.
    pub fn class(&mut self, class: u32) {
        if class != self.character.class
            && self
                .rules
                .classes(self.character.race, self.expansions)
                .iter()
                .any(|(id, _)| *id == class)
        {
            self.character.class = class;
            self.reconcile();
            self.reset_stats();
        }
    }

    /// Chooses a deity and reconciles the starting city.
    pub fn deity(&mut self, deity: u32) {
        if self
            .rules
            .deities(self.character.race, self.character.class, self.pvp)
            .iter()
            .any(|(id, _)| *id == deity)
        {
            self.character.deity = deity;
            self.reconcile();
        }
    }

    /// Rechecks choices after a new expansion or world-rules report.
    pub fn reconcile(&mut self) {
        let c = &mut self.character;
        let races = self.rules.races(self.expansions);
        if !races.iter().any(|(id, _)| *id == c.race) {
            c.race = 1;
        }
        let classes = self.rules.classes(c.race, self.expansions);
        if !classes.iter().any(|(id, _)| *id == c.class) {
            c.class = classes.first().map_or(1, |(id, _)| *id);
        }
        let deities = self.rules.deities(c.race, c.class, self.pvp);
        if !deities.iter().any(|(id, _)| *id == c.deity) {
            c.deity = deities.first().map_or(396, |(id, _)| *id);
        }
        let cities = self.rules.cities(c.race, c.class, c.deity, self.pvp);
        if !cities.iter().any(|city| city.start_zone == c.start_zone) {
            c.start_zone = cities.first().map_or(0, |city| city.start_zone);
        }
        self.rules.appearance(c.race, c.gender).reconcile(c);
        c.tutorial &= self.rules.offers_tutorial();
    }

    /// Base stats and the class's points to distribute.
    pub fn base(&self) -> ([u32; 7], u32) {
        creation::base_stats(self.character.race, self.character.class).unwrap_or(([0; 7], 0))
    }

    /// Bonus points not yet assigned.
    pub fn remaining(&self) -> u32 {
        let (base, points) = self.base();
        points.saturating_sub(
            self.character
                .stats
                .iter()
                .zip(base)
                .map(|(stat, base)| stat.saturating_sub(base))
                .sum(),
        )
    }

    /// Returns every stat to its race/class base.
    pub fn reset_stats(&mut self) {
        self.character.stats = self.base().0;
    }

    /// Moves one point within the official UI's base..min(base+25,150) range
    /// (`EQMac` 0xc76c0), without spending more points than remain.
    pub fn point(&mut self, index: usize, add: bool) {
        let base = self.base().0;
        let remaining = self.remaining();
        if let Some(stat) = self.character.stats.get_mut(index) {
            if add && remaining > 0 && *stat < (base[index] + 25).min(150) {
                *stat += 1;
            } else if !add && *stat > base[index] {
                *stat -= 1;
            }
        }
    }

    /// Spends points in a useful primary stat, then stamina, then other stats.
    pub fn allocate(&mut self) {
        self.reset_stats();
        let primary = match self.character.class {
            2 | 3 | 4 | 6 | 10 | 15 => 4,
            5 | 11..=14 => 5,
            8 | 9 => 3,
            _ => 1,
        };
        for index in [primary, 1, 0, 2, 3, 4, 5, 6] {
            for _ in 0..25 {
                self.point(index, true);
            }
        }
    }

    /// Validates and normalizes a draft without changing the editor.
    ///
    /// # Errors
    /// Returns a player-facing reason when a choice or allocation is invalid.
    pub fn request(&self) -> Result<NewCharacter, String> {
        let mut character = self.character.clone();
        let name = self.name.trim();
        if !(4..=15).contains(&name.len()) || !name.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err("Choose a name with 4 to 15 letters.".into());
        }
        character.name = name.to_ascii_lowercase();
        character.name[..1].make_ascii_uppercase();
        if !self
            .rules
            .classes(character.race, self.expansions)
            .iter()
            .any(|(id, _)| *id == character.class)
            || !self
                .rules
                .cities(character.race, character.class, character.deity, self.pvp)
                .iter()
                .any(|city| city.start_zone == character.start_zone)
        {
            return Err("Choose an available race, class, deity and starting city.".into());
        }
        if self.remaining() != 0 {
            return Err(format!(
                "Distribute the remaining {} stat points.",
                self.remaining()
            ));
        }
        if character
            .stats
            .iter()
            .zip(self.base().0)
            .any(|(stat, base)| *stat > (base + 25).min(150))
        {
            return Err("A stat exceeds its creation limit.".into());
        }
        if !self
            .rules
            .appearance(character.race, character.gender)
            .accepts(&character)
        {
            return Err("Choose an available appearance.".into());
        }
        if character.tutorial && !self.rules.offers_tutorial() {
            return Err("This server does not offer the tutorial.".into());
        }
        character.validate().map_err(|error| error.to_string())?;
        Ok(character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_combination_can_spend_its_points_and_encode() {
        for rules in [Rules::Classic, Rules::Titanium, Rules::EqMac] {
            for (race, _) in rules.races(None) {
                for (class, _) in rules.classes(race, None) {
                    let mut draft = Draft::new(rules, None, 0);
                    draft.name = "testhero".into();
                    draft.race(race);
                    draft.class(class);
                    draft.allocate();
                    let character = draft
                        .request()
                        .unwrap_or_else(|error| panic!("{rules:?}/{race}/{class}: {error}"));
                    assert_eq!(character.name, "Testhero");
                    if rules == Rules::EqMac {
                        character.eqmac_create_request().unwrap();
                    } else {
                        character.create_request().unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn edits_reconcile_dependencies_and_never_create_or_destroy_stat_points() {
        let mut draft = Draft::new(Rules::EqMac, None, 0);
        draft.name = "Testhero".into();
        draft.class(2);
        assert!(draft.request().is_err());
        for _ in 0..40 {
            draft.point(4, true);
        }
        assert_eq!(draft.remaining(), 5);
        draft.point(4, false);
        assert_eq!(draft.remaining(), 6);
        draft.allocate();
        assert!(draft.request().is_ok());
        draft.race(128);
        assert_ne!(draft.character.class, 2);
        assert_eq!(draft.character.deity, 203);
        assert_eq!(draft.character.start_zone, 106);
        assert!(draft.remaining() > 0);
        draft.class(11);
        assert_eq!(draft.character.start_zone, 82);
        draft.expansions = Some(crate::rules::Expansions(0));
        draft.reconcile();
        assert_eq!(draft.character.race, 1);
    }

    #[test]
    fn city_values_are_generation_specific_and_invalid_names_never_submit() {
        for (rules, zone) in [(Rules::Classic, 1), (Rules::Titanium, 1), (Rules::EqMac, 2)] {
            let mut draft = Draft::new(rules, None, 0);
            draft.class(2);
            draft.deity(212);
            draft.allocate();
            draft.name = "Testcleric".into();
            assert_eq!(draft.request().unwrap().start_zone, zone);
            for name in ["", "abc", "Test1234", "Tésthero", "Averylongcharactername"] {
                draft.name = name.into();
                assert!(draft.request().is_err());
            }
        }
    }
}

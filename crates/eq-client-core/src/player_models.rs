//! Which player models a viewer sees in the newer (Luclin) look.
//!
//! The official Titanium client lets the player choose, per player race and
//! gender, whether those characters draw with the Luclin models or the
//! classic ones; the server sends nothing that chooses. Where the server's
//! policy keeps the classic look (P99, whose creation rules preview classic
//! models only), every spawn draws classic whatever the choice.
//!
//! Inferred, unread (R49-44a): which spawns the choice covers. Here it is
//! players and their corpses only; non-players of player races and illusions
//! draw classic until the read says otherwise.

use crate::SpawnKind;

/// The player races that have Luclin models, in the order a choice's bits
/// keep them (male, then female, for each).
const RACES: [u32; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 128, 130, 330];

/// The look a spawn's model draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Generation {
    /// The original models, from a zone's or the global character archives.
    #[default]
    Classic,
    /// The newer models, from each race's own global archive.
    Luclin,
}

/// Which player races and genders the viewer chose to see with Luclin
/// models. None by default: every model draws classic until chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LuclinModels(u32);

impl LuclinModels {
    /// None chosen.
    pub const NONE: Self = Self(0);

    /// The bit a race and gender keep, if it has a Luclin model.
    fn bit(race: u32, gender: u32) -> Option<u32> {
        let index = u32::try_from(RACES.iter().position(|&r| r == race)?).ok()?;
        (gender <= 1).then(|| 1 << (index * 2 + gender))
    }

    /// Whether the race and gender has a Luclin model to choose.
    #[must_use]
    pub fn offered(race: u32, gender: u32) -> bool {
        Self::bit(race, gender).is_some()
    }

    /// Every race and gender with a Luclin model, male then female per race.
    pub fn every() -> impl Iterator<Item = (u32, u32)> {
        RACES.into_iter().flat_map(|race| [(race, 0), (race, 1)])
    }

    /// Whether the race and gender is chosen.
    #[must_use]
    pub fn chosen(self, race: u32, gender: u32) -> bool {
        Self::bit(race, gender).is_some_and(|bit| self.0 & bit != 0)
    }

    /// Chooses the race and gender, or not. A race and gender with no Luclin
    /// model is left as it is.
    pub fn set(&mut self, race: u32, gender: u32, on: bool) {
        if let Some(bit) = Self::bit(race, gender) {
            if on {
                self.0 |= bit;
            } else {
                self.0 &= !bit;
            }
        }
    }

    /// The choice as a file keeps it: each chosen race and gender's classic
    /// model code ([`crate::races::model`]), comma separated, in order.
    #[must_use]
    pub fn words(self) -> String {
        Self::every()
            .filter(|&(race, gender)| self.chosen(race, gender))
            .filter_map(|(race, gender)| crate::races::model(race, gender))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// A choice from a file's words; a code that names no race and gender
    /// with a Luclin model is skipped.
    #[must_use]
    pub fn parse(words: &str) -> Self {
        let mut models = Self::NONE;
        for code in words.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            if let Some((race, gender)) = Self::every().find(|&(race, gender)| {
                crate::races::model(race, gender).is_some_and(|own| own.eq_ignore_ascii_case(code))
            }) {
                models.set(race, gender, true);
            }
        }
        models
    }
}

/// The model generation a spawn draws with: Luclin for a player, or a
/// player's corpse, of a race and gender the viewer chose, where the
/// server's policy `allows` Luclin models; classic otherwise.
#[must_use]
pub fn generation(
    (race, gender, kind): (u32, u32, SpawnKind),
    chosen: LuclinModels,
    allows: bool,
) -> Generation {
    let player = matches!(kind, SpawnKind::Player | SpawnKind::PlayerCorpse);
    if allows && player && chosen.chosen(race, gender) {
        Generation::Luclin
    } else {
        Generation::Classic
    }
}

/// Whether the server's creation policy lets the world draw Luclin models:
/// not where it previews only classic models (P99's
/// [`crate::creation::Rules::classic_appearance`]), nor before any policy
/// is known.
#[must_use]
pub fn allowed_by(rules: Option<crate::creation::Rules>) -> bool {
    rules.is_some_and(|rules| !rules.classic_appearance())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creation::Rules;

    #[test]
    fn only_a_chosen_player_race_and_gender_draws_luclin_where_the_policy_allows() {
        let mut chosen = LuclinModels::NONE;
        chosen.set(1, 0, true);
        chosen.set(330, 1, true);
        let allows = allowed_by(Some(Rules::Titanium));
        assert_eq!(
            generation((1, 0, SpawnKind::Player), chosen, allows),
            Generation::Luclin
        );
        assert_eq!(
            generation((330, 1, SpawnKind::PlayerCorpse), chosen, allows),
            Generation::Luclin
        );
        // The other gender, another race, a guard of the race: classic.
        for spawn in [
            (1, 1, SpawnKind::Player),
            (2, 0, SpawnKind::Player),
            (1, 0, SpawnKind::Npc),
            (1, 0, SpawnKind::NpcCorpse),
        ] {
            assert_eq!(generation(spawn, chosen, allows), Generation::Classic);
        }
        // P99's policy, and none known yet, keep every model classic.
        for rules in [Some(Rules::Classic), None] {
            assert_eq!(
                generation((1, 0, SpawnKind::Player), chosen, allowed_by(rules)),
                Generation::Classic
            );
        }
        assert!(allowed_by(Some(Rules::EqMac)));
    }

    #[test]
    fn a_choice_keeps_to_its_words_and_skips_what_it_cannot_choose() {
        let mut chosen = LuclinModels::NONE;
        chosen.set(1, 0, true);
        chosen.set(128, 1, true);
        // A race without a Luclin model, and a neuter, choose nothing.
        chosen.set(13, 2, true);
        chosen.set(1, 2, true);
        assert_eq!(chosen.words(), "HUM,IKF");
        assert_eq!(LuclinModels::parse("HUM, ikf"), chosen);
        assert_eq!(LuclinModels::parse("HUM,AVI,ZZZ,,"), {
            let mut only = LuclinModels::NONE;
            only.set(1, 0, true);
            only
        });
        assert_eq!(LuclinModels::parse(""), LuclinModels::NONE);
        chosen.set(1, 0, false);
        assert_eq!(chosen.words(), "IKF");
        assert_eq!(LuclinModels::every().count(), 30);
        assert!(LuclinModels::offered(130, 1) && !LuclinModels::offered(13, 0));
    }
}

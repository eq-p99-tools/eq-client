//! Which player models a viewer sees in the newer (Luclin) look.
//!
//! The official Titanium client decides per player race and gender, as read
//! (R49-44 a, 0x43ab98 and 0x43ac64):
//! - **The server's expansion mask gates it.** Luclin models are offered
//!   only when bit 0x4 or 0x8 of the mask the server sends is set (0x45386d).
//!   A third case, the flag at 0x923d74, is unread as to meaning and not built.
//! - **`[Defaults] AllLuclinPcModelsOff`** turns every race but Vah Shir
//!   classic.
//! - **`[Defaults] UseLuclin<Race>Male` / `Female`** decides per race and
//!   gender. A key the viewer never set is on where the gate is open (the
//!   program then writes `TRUE` back to its ini; this client never writes
//!   the official ini, a stated departure), and off where it is shut.
//! - **Fallbacks** when a race's own key is off: Human also takes Erudite's;
//!   Wood Elf takes Dark Elf's, then Half Elf's, then High Elf's.
//! - **Vah Shir (130)** follows its own key alone, past the gate and the
//!   all-off key, and is on when unset.
//!
//! The read is Titanium's. The Mac client (TAKP) has the same keys, reader
//! and default (R49-44 e); that its expansion bits gate the same way is
//! inferred by analogy.
//!
//! The program reads the keys once, at start-up (R49-44 b); this client
//! reads them with the character's options at each entry into the world and
//! fixes the choice per zone, so a change to the options file shows from the
//! next entry into the world (a local look, 11-23), and one made in the
//! client from the next zone (a stated departure).
//!
//! Inferred, unread (R49-44 a's open half): which spawns the choice covers.
//! Here it is players and their corpses only; non-players of player races
//! and illusions draw classic until the read says otherwise.

use crate::{GameDialect, SpawnKind, rules::Expansions};

/// The player races the program's loader walks for Luclin models (0x43df32),
/// in the order a choice's bits keep them (male, then female, for each).
/// Froglok (330) isn't among them, so it stays classic.
const RACES: [u32; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 128, 130];

/// Vah Shir, whose key alone decides (0x43ac64).
const VAH_SHIR: u32 = 130;

/// The expansion mask's bits that offer Luclin models (0x45386d).
const OFFERING_BITS: u32 = 0x4 | 0x8;

/// The look a spawn's model draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Generation {
    /// The original models, from a zone's or the global character archives.
    #[default]
    Classic,
    /// The newer models, from each race's own global archive.
    Luclin,
}

/// Whether the server offers Luclin models: its expansion mask has bit 0x4
/// or 0x8 (read R49-44 a, Titanium). With no mask seen, nothing is offered.
#[must_use]
pub const fn offered(dialect: GameDialect, expansions: Option<Expansions>) -> bool {
    let Some(Expansions(mask)) = expansions else {
        return false;
    };
    match dialect {
        // Read for Titanium. The Mac client's keys are the same (R49-44 e);
        // that its mask gates the same way is inferred.
        GameDialect::Titanium | GameDialect::Rof2 | GameDialect::EqMac => mask & OFFERING_BITS != 0,
    }
}

/// The viewer's Luclin model keys: per race and gender on, off, or unset,
/// and the all-off key. Unset by default, so the server's offer decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LuclinModels {
    on: u32,
    off: u32,
    /// `[Defaults] AllLuclinPcModelsOff`, default false.
    pub all_off: bool,
}

impl LuclinModels {
    /// Every key unset, and all-off false.
    pub const NONE: Self = Self {
        on: 0,
        off: 0,
        all_off: false,
    };

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

    /// The race and gender's key: on, off, or unset. Where a file named it
    /// both ways, off wins.
    #[must_use]
    pub fn key(self, race: u32, gender: u32) -> Option<bool> {
        let bit = Self::bit(race, gender)?;
        if self.off & bit != 0 {
            Some(false)
        } else if self.on & bit != 0 {
            Some(true)
        } else {
            None
        }
    }

    /// Sets the race and gender's key on or off, or unsets it. A race and
    /// gender with no Luclin model is left as it is.
    pub fn set(&mut self, race: u32, gender: u32, key: Option<bool>) {
        if let Some(bit) = Self::bit(race, gender) {
            self.on &= !bit;
            self.off &= !bit;
            match key {
                Some(true) => self.on |= bit,
                Some(false) => self.off |= bit,
                None => {}
            }
        }
    }

    /// The keys set to `on` as a file keeps them: each race and gender's
    /// classic model code ([`crate::races::model`]), comma separated, in
    /// order.
    #[must_use]
    pub fn words(self, on: bool) -> String {
        Self::every()
            .filter(|&(race, gender)| self.key(race, gender) == Some(on))
            .filter_map(|(race, gender)| crate::races::model(race, gender))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Sets to `on` each race and gender a file's words name; a code that
    /// names no race and gender with a Luclin model is skipped. A key the
    /// words set off stays off whatever words later set it on.
    pub fn read(&mut self, words: &str, on: bool) {
        for code in words.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            if let Some(bit) = Self::every()
                .find(|&(race, gender)| {
                    crate::races::model(race, gender)
                        .is_some_and(|own| own.eq_ignore_ascii_case(code))
                })
                .and_then(|(race, gender)| Self::bit(race, gender))
            {
                if on {
                    self.on |= bit;
                } else {
                    self.off |= bit;
                }
            }
        }
    }

    /// These keys over `below`'s: each race and gender keeps its key here
    /// where set, else takes `below`'s. All-off is `below`'s.
    #[must_use]
    pub const fn over(self, below: Self) -> Self {
        let set = self.on | self.off;
        Self {
            on: self.on | (below.on & !set),
            off: self.off | (below.off & !set),
            all_off: below.all_off,
        }
    }

    /// Whether the race and gender's key counts as on: as set, or, unset,
    /// on where the server offers the models or for Vah Shir (0x43ab98).
    fn on(self, race: u32, gender: u32, offered: bool) -> bool {
        self.key(race, gender)
            .unwrap_or(offered || race == VAH_SHIR)
    }
}

/// The races whose keys a race also takes when its own is off (0x43ac64).
const fn fallbacks(race: u32) -> &'static [u32] {
    match race {
        1 => &[3],
        4 => &[6, 7, 5],
        _ => &[],
    }
}

/// The model generation a spawn draws with: Luclin for a player, or a
/// player's corpse, where the program's decision (0x43ac64) says so; classic
/// otherwise. `offered` is [`offered`] for the session.
#[must_use]
pub fn generation(
    (race, gender, kind): (u32, u32, SpawnKind),
    keys: LuclinModels,
    offered: bool,
) -> Generation {
    let player = matches!(kind, SpawnKind::Player | SpawnKind::PlayerCorpse);
    let luclin = player
        && LuclinModels::offered(race, gender)
        && if race == VAH_SHIR {
            keys.on(race, gender, offered)
        } else {
            offered
                && !keys.all_off
                && std::iter::once(race)
                    .chain(fallbacks(race).iter().copied())
                    .any(|race| keys.on(race, gender, offered))
        };
    if luclin {
        Generation::Luclin
    } else {
        Generation::Classic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: SpawnKind = SpawnKind::Player;

    fn keys(set: &[(u32, u32, bool)]) -> LuclinModels {
        let mut keys = LuclinModels::NONE;
        for &(race, gender, on) in set {
            keys.set(race, gender, Some(on));
        }
        keys
    }

    #[test]
    fn the_servers_mask_offers_the_models_by_bit_0x4_or_0x8() {
        for dialect in [GameDialect::Titanium, GameDialect::Rof2] {
            assert!(!offered(dialect, None));
            assert!(!offered(dialect, Some(Expansions(0x3))));
            assert!(offered(dialect, Some(Expansions(0x4))));
            assert!(offered(dialect, Some(Expansions(0x8))));
            assert!(offered(dialect, Some(Expansions(0xF))));
        }
    }

    #[test]
    fn the_mac_arm_gates_on_the_same_bits_inferred() {
        // A Mac server through Planes of Power sends Kunark to PoP (0xF);
        // one stopping at Velious sends 0x3.
        assert!(offered(GameDialect::EqMac, Some(Expansions(0xF))));
        assert!(!offered(GameDialect::EqMac, Some(Expansions(0x3))));
        assert!(!offered(GameDialect::EqMac, None));
    }

    #[test]
    fn an_unset_key_follows_the_offer() {
        let none = LuclinModels::NONE;
        assert_eq!(generation((1, 0, PLAYER), none, true), Generation::Luclin);
        assert_eq!(generation((1, 0, PLAYER), none, false), Generation::Classic);
        // An explicit key decides where offered, and nothing opens a shut gate.
        let off = keys(&[(2, 0, false)]);
        assert_eq!(generation((2, 0, PLAYER), off, true), Generation::Classic);
        let on = keys(&[(2, 1, true)]);
        assert_eq!(generation((2, 1, PLAYER), on, true), Generation::Luclin);
        assert_eq!(generation((2, 1, PLAYER), on, false), Generation::Classic);
    }

    #[test]
    fn only_players_and_their_corpses_draw_luclin() {
        let none = LuclinModels::NONE;
        assert_eq!(
            generation((1, 0, SpawnKind::PlayerCorpse), none, true),
            Generation::Luclin
        );
        for kind in [SpawnKind::Npc, SpawnKind::NpcCorpse] {
            assert_eq!(generation((1, 0, kind), none, true), Generation::Classic);
        }
        // Froglok isn't walked by the loader; a neuter has no model.
        assert_eq!(
            generation((330, 0, PLAYER), none, true),
            Generation::Classic
        );
        assert_eq!(
            generation((330, 0, PLAYER), keys(&[(330, 0, true)]), true),
            Generation::Classic
        );
        assert_eq!(generation((1, 2, PLAYER), none, true), Generation::Classic);
    }

    #[test]
    fn all_off_turns_every_race_classic_but_vah_shir() {
        let mut all_off = keys(&[(1, 0, true)]);
        all_off.all_off = true;
        assert_eq!(
            generation((1, 0, PLAYER), all_off, true),
            Generation::Classic
        );
        assert_eq!(
            generation((130, 0, PLAYER), all_off, true),
            Generation::Luclin
        );
    }

    #[test]
    fn vah_shir_follows_its_own_key_past_the_gate() {
        let none = LuclinModels::NONE;
        assert_eq!(
            generation((130, 1, PLAYER), none, false),
            Generation::Luclin
        );
        let off = keys(&[(130, 1, false)]);
        assert_eq!(generation((130, 1, PLAYER), off, true), Generation::Classic);
        // Iksar has no such exception.
        assert_eq!(
            generation((128, 1, PLAYER), none, false),
            Generation::Classic
        );
    }

    #[test]
    fn human_and_wood_elf_take_their_fallbacks_keys() {
        let human = keys(&[(1, 0, false), (3, 0, true)]);
        assert_eq!(generation((1, 0, PLAYER), human, true), Generation::Luclin);
        // The fallback is per gender, and only Human takes Erudite's.
        assert_eq!(
            generation(
                (1, 1, PLAYER),
                keys(&[(1, 1, false), (3, 1, false), (3, 0, true)]),
                true
            ),
            Generation::Classic
        );
        assert_eq!(
            generation((3, 0, PLAYER), keys(&[(3, 0, false), (1, 0, true)]), true),
            Generation::Classic
        );
        let elf_off = [(4, 1, false), (6, 1, false), (7, 1, false)];
        let mut high_elf = elf_off.to_vec();
        high_elf.push((5, 1, true));
        assert_eq!(
            generation((4, 1, PLAYER), keys(&high_elf), true),
            Generation::Luclin
        );
        let mut none = elf_off.to_vec();
        none.push((5, 1, false));
        assert_eq!(
            generation((4, 1, PLAYER), keys(&none), true),
            Generation::Classic
        );
        // An unset fallback counts as on where offered.
        assert_eq!(
            generation((4, 1, PLAYER), keys(&[(4, 1, false)]), true),
            Generation::Luclin
        );
    }

    #[test]
    fn keys_keep_to_their_words_and_off_wins() {
        let mut chosen = keys(&[(1, 0, true), (128, 1, true), (6, 1, false)]);
        // A race without a Luclin model, and a neuter, choose nothing.
        chosen.set(13, 2, Some(true));
        chosen.set(1, 2, Some(true));
        assert_eq!(chosen.words(true), "HUM,IKF");
        assert_eq!(chosen.words(false), "DAF");
        let mut read = LuclinModels::NONE;
        read.read("HUM, ikf", true);
        read.read("daf", false);
        assert_eq!(read, chosen);
        let mut skipped = LuclinModels::NONE;
        skipped.read("HUM,AVI,ZZZ,,", true);
        assert_eq!(skipped, keys(&[(1, 0, true)]));
        // A code in both lines is off, in either order.
        for order in [[true, false], [false, true]] {
            let mut both = LuclinModels::NONE;
            for on in order {
                both.read("HUM", on);
            }
            assert_eq!(both.key(1, 0), Some(false));
            assert_eq!(both.words(true), "");
        }
        // A file's keys lie over the ini's seed, key by key.
        let mut seed = keys(&[(1, 0, false), (2, 0, true)]);
        seed.all_off = true;
        let layered = keys(&[(1, 0, true)]).over(seed);
        assert_eq!(layered.key(1, 0), Some(true));
        assert_eq!(layered.key(2, 0), Some(true));
        assert_eq!(layered.key(3, 0), None);
        assert!(layered.all_off);
        chosen.set(1, 0, None);
        assert_eq!(chosen.key(1, 0), None);
        assert_eq!(chosen.words(true), "IKF");
        assert_eq!(LuclinModels::every().count(), 28);
        assert!(LuclinModels::offered(130, 1) && !LuclinModels::offered(330, 0));
    }
}

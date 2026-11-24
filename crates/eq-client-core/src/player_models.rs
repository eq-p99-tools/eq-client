//! Which player models a viewer sees in the newer (Luclin) look.
//!
//! The official Titanium client decides per player race and gender, as read
//! (R49-44 a, 0x43ab98 and 0x43ac64):
//! - **`[Defaults] AllLuclinPcModelsOff`** turns every race but Vah Shir
//!   classic.
//! - **`[Defaults] UseLuclin<Race>Male` / `Female`** decides per race and
//!   gender, on every server.
//! - **A key the viewer never set** follows the server's expansion mask, as
//!   the program's default does: on where bit 0x4 or 0x8 (Luclin or Planes
//!   of Power, 0x45386d) is set, and off (classic) where it isn't or no
//!   mask came. So on P99, whose mask is Kunark and Velious (0x3), a
//!   missing key is classic (Adam, 11-24). The program then writes `TRUE`
//!   back to its ini where it counts a key on; this client never writes
//!   the official ini, a stated departure.
//! - **Fallbacks** when a race's own key is off: Human also takes Erudite's;
//!   Wood Elf takes Dark Elf's, then Half Elf's, then High Elf's.
//! - **Vah Shir (130)** follows its own key alone, past the all-off key,
//!   and is on when unset whatever the mask.
//!
//! **A set key decides past the mask, a stated departure.** The program
//! offers the models only where the mask has bit 0x4 or 0x8, and with it
//! shut draws classic whatever the keys say. This client follows a set key
//! on every server instead (Adam's call, 11-24), so a viewer's ini decides
//! on P99 too; only a missing key reads the mask.
//!
//! The read is Titanium's. The Mac client (TAKP) has the same keys, reader
//! and default (R49-44 e); that its mask's bits count the same way is
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

use crate::{SpawnKind, rules::Expansions};

/// The player races the program's loader walks for Luclin models (0x43df32),
/// in the order a choice's bits keep them (male, then female, for each).
/// Froglok (330) isn't among them, so it stays classic.
const RACES: [u32; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 128, 130];

/// Vah Shir, whose key alone decides (0x43ac64).
const VAH_SHIR: u32 = 130;

/// The expansion mask's bits under which a missing key counts as on
/// (0x45386d).
const OFFERING_BITS: u32 = 0x4 | 0x8;

/// Whether the server's expansion mask has Luclin or Planes of Power (bit
/// 0x4 or 0x8), so that a key the viewer never set counts as on (read R49-44
/// a). With no mask seen, it hasn't.
#[must_use]
pub const fn offered(expansions: Option<Expansions>) -> bool {
    match expansions {
        Some(Expansions(mask)) => mask & OFFERING_BITS != 0,
        None => false,
    }
}

/// The look a spawn's model draws with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Generation {
    /// The original models, from a zone's or the global character archives.
    #[default]
    Classic,
    /// The newer models, from each race's own global archive.
    Luclin,
}

/// The viewer's Luclin model keys: per race and gender on, off, or unset,
/// and the all-off key. Unset by default, so the server's mask decides.
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
    /// on where the server's mask offers the models, and for Vah Shir
    /// (0x43ab98).
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
/// player's corpse, where the viewer's keys say so (0x43ac64), a set key on
/// every server; classic otherwise. `offered` is [`offered`] for the
/// session, which only a missing key reads.
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
            !keys.all_off
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
    fn the_servers_mask_offers_by_bit_0x4_or_0x8() {
        assert!(!offered(None));
        assert!(!offered(Some(Expansions(0x3))));
        assert!(offered(Some(Expansions(0x4))));
        assert!(offered(Some(Expansions(0x8))));
        assert!(offered(Some(Expansions(0xF))));
        assert!(offered(Some(Expansions(0x0007_f7ff))));
    }

    #[test]
    fn a_missing_key_follows_the_mask_and_a_set_key_decides_everywhere() {
        let none = LuclinModels::NONE;
        assert_eq!(generation((1, 0, PLAYER), none, true), Generation::Luclin);
        // P99's mask (0x3) or none: a missing key is classic.
        assert_eq!(generation((1, 0, PLAYER), none, false), Generation::Classic);
        let off = keys(&[(2, 0, false)]);
        let on = keys(&[(2, 1, true)]);
        for offered in [true, false] {
            assert_eq!(
                generation((2, 0, PLAYER), off, offered),
                Generation::Classic
            );
            assert_eq!(generation((2, 1, PLAYER), on, offered), Generation::Luclin);
        }
        // A missing fallback follows the mask too.
        let human = keys(&[(1, 0, false)]);
        assert_eq!(generation((1, 0, PLAYER), human, true), Generation::Luclin);
        assert_eq!(
            generation((1, 0, PLAYER), human, false),
            Generation::Classic
        );
        // Vah Shir is on when unset, whatever the mask.
        assert_eq!(
            generation((130, 0, PLAYER), none, false),
            Generation::Luclin
        );
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
    fn vah_shir_follows_its_own_key_past_all_off() {
        let mut all_off = LuclinModels::NONE;
        all_off.all_off = true;
        assert_eq!(
            generation((130, 1, PLAYER), all_off, true),
            Generation::Luclin
        );
        let mut off = keys(&[(130, 1, false)]);
        assert_eq!(generation((130, 1, PLAYER), off, true), Generation::Classic);
        off.all_off = true;
        assert_eq!(generation((130, 1, PLAYER), off, true), Generation::Classic);
        // Iksar has no such exception.
        assert_eq!(
            generation((128, 1, PLAYER), all_off, true),
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
        // An unset fallback counts as on.
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

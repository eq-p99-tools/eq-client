//! Names over heads: what the tag over a character says, and how much of a
//! player's name `/shownames` shows, in the official client's words.

pub use eq_network_game::names::NameParts;

use crate::{
    SpawnKind,
    listing::{Anonymity, Listing},
};

/// The official client's answer to a `/shownames` it cannot read
/// (`eqstr_us.txt`), which a front end shows where the installation has it.
pub const SHOW_NAMES_USAGE: u32 = 13298;
/// The same answer in this client's words.
pub const SHOW_NAMES_USAGE_TEXT: &str = "Use /shownames off, or /shownames and a level.";

/// How much of a player's name shows over their head, as `/shownames` sets
/// it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShowNames {
    /// No player's name (`/shownames off`).
    Off,
    /// First names only (`/shownames 1`).
    First,
    /// First and last names (`/shownames 2`).
    Last,
    /// First and last names, and guilds (`/shownames 3`).
    Guild,
    /// Everything: titles and suffixes too (`/shownames 4`).
    #[default]
    Everything,
}

impl ShowNames {
    /// Every level, from off up.
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::First,
        Self::Last,
        Self::Guild,
        Self::Everything,
    ];

    /// The word `/shownames` takes for it, which a file keeps too.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::First => "1",
            Self::Last => "2",
            Self::Guild => "3",
            Self::Everything => "4",
        }
    }

    /// The level by its number, as `ShowNamesLevel` in `eqclient.ini` keeps
    /// it: 0 for off.
    #[must_use]
    pub fn from_level(level: u32) -> Option<Self> {
        usize::try_from(level)
            .ok()
            .and_then(|level| Self::ALL.get(level).copied())
    }

    /// The level a `/shownames` word names: `off`, or 0 to 4. The official
    /// format also lists 5 and 6, which Titanium's string table has no words
    /// for.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        let word = word.trim();
        if word.eq_ignore_ascii_case("off") {
            return Some(Self::Off);
        }
        word.parse().ok().and_then(Self::from_level)
    }

    /// What the client says when `/shownames` sets it: the official
    /// client's string (`eqstr_us.txt`, 13293 to 13297) for a front end with
    /// the installed strings, and the same in this client's words.
    #[must_use]
    pub const fn announcement(self) -> (u32, &'static str) {
        match self {
            Self::Off => (13293, "Names over players are hidden."),
            Self::First => (13294, "Names over players show first names only."),
            Self::Last => (13295, "Names over players show first and last names."),
            Self::Guild => (13296, "Names over players show names and guilds."),
            Self::Everything => (13297, "Names over players show everything."),
        }
    }
}

/// Whether a spawn's tag is a player's, which Show PC Names and `/shownames`
/// govern: players and their corpses. Show NPC Names governs the rest.
#[must_use]
pub const fn is_pc(kind: SpawnKind) -> bool {
    matches!(kind, SpawnKind::Player | SpawnKind::PlayerCorpse)
}

/// What the tag over a spawn says, from the name it spawned with: the name
/// as the client shows names, and for a player as much more as `/shownames`
/// allows. A title goes before the name, a last name and a suffix after it,
/// and the guild on a line of its own under it, unless the player is
/// anonymous.
#[must_use]
pub fn label(
    name: &str,
    kind: SpawnKind,
    (parts, listing): (&NameParts, &Listing),
    guild: Option<&str>,
    level: ShowNames,
) -> String {
    let name = crate::entities::display_name(name);
    if kind != SpawnKind::Player {
        return name;
    }
    let everything = level >= ShowNames::Everything;
    let line = [
        (everything, parts.title.as_str()),
        (true, name.as_str()),
        (level >= ShowNames::Last, parts.last_name.as_str()),
        (everything, parts.suffix.as_str()),
    ]
    .into_iter()
    .filter(|(shown, word)| *shown && !word.is_empty())
    .map(|(_, word)| word)
    .collect::<Vec<_>>()
    .join(" ");
    match guild.filter(|_| level >= ShowNames::Guild && listing.anonymity != Anonymity::Anonymous) {
        Some(guild) => format!("{line}\n<{guild}>"),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_names_reads_its_words_and_says_the_official_lines() {
        assert_eq!(ShowNames::parse("OFF"), Some(ShowNames::Off));
        assert_eq!(ShowNames::parse(" 2 "), Some(ShowNames::Last));
        assert_eq!(ShowNames::parse("0"), Some(ShowNames::Off));
        for word in ["5", "6", "-1", "two", ""] {
            assert_eq!(ShowNames::parse(word), None, "{word}");
        }
        for level in ShowNames::ALL {
            assert_eq!(ShowNames::parse(level.word()), Some(level));
        }
        assert_eq!(ShowNames::from_level(4), Some(ShowNames::Everything));
        assert_eq!(ShowNames::default(), ShowNames::Everything);
        assert_eq!(ShowNames::Guild.announcement().0, 13296);
        assert_ne!(
            ShowNames::Guild.announcement().1,
            ShowNames::Last.announcement().1
        );
    }

    #[test]
    fn a_players_tag_grows_with_the_level() {
        let parts = NameParts {
            title: "Lord".into(),
            last_name: "Exemplum".into(),
            suffix: "of Veeshan".into(),
        };
        let listing = Listing {
            guild: Some(7),
            ..Listing::default()
        };
        let tag = |level| {
            label(
                "Examplar",
                SpawnKind::Player,
                (&parts, &listing),
                Some("Example Guild"),
                level,
            )
        };
        assert_eq!(tag(ShowNames::First), "Examplar");
        assert_eq!(tag(ShowNames::Last), "Examplar Exemplum");
        assert_eq!(tag(ShowNames::Guild), "Examplar Exemplum\n<Example Guild>");
        assert_eq!(
            tag(ShowNames::Everything),
            "Lord Examplar Exemplum of Veeshan\n<Example Guild>"
        );
        // An anonymous player hides the guild; one roleplaying shows it.
        for (anonymity, guild) in [
            (Anonymity::Anonymous, false),
            (Anonymity::Roleplaying, true),
        ] {
            let listing = Listing {
                anonymity,
                ..listing
            };
            let tag = label(
                "Examplar",
                SpawnKind::Player,
                (&parts, &listing),
                Some("Example Guild"),
                ShowNames::Guild,
            );
            assert_eq!(tag.contains("<Example Guild>"), guild, "{anonymity:?}");
        }
        // Empty parts leave no gaps.
        assert_eq!(
            label(
                "Examplar",
                SpawnKind::Player,
                (&NameParts::default(), &Listing::default()),
                None,
                ShowNames::Everything
            ),
            "Examplar"
        );
    }

    #[test]
    fn other_tags_are_the_name_alone() {
        let parts = NameParts {
            last_name: "Guard".into(),
            ..NameParts::default()
        };
        let listing = Listing {
            guild: Some(7),
            ..Listing::default()
        };
        for (name, kind, shown) in [
            ("a_gnoll001", SpawnKind::Npc, "a gnoll"),
            (
                "Examplar's_corpse0",
                SpawnKind::PlayerCorpse,
                "Examplar's corpse",
            ),
            (
                "a_gnoll's_corpse12",
                SpawnKind::NpcCorpse,
                "a gnoll's corpse",
            ),
        ] {
            assert_eq!(
                label(
                    name,
                    kind,
                    (&parts, &listing),
                    Some("Example Guild"),
                    ShowNames::Everything
                ),
                shown
            );
        }
        assert!(is_pc(SpawnKind::PlayerCorpse) && !is_pc(SpawnKind::NpcCorpse));
    }
}

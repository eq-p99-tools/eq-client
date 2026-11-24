//! The track window's options, as the Titanium program keeps them (read
//! R49-30 e and R14 section 2, checklist 6.638): a Ranger's sort and colour
//! filter, and anyone's choice of players. The program keeps the sort and
//! the players choice in `eqclient.ini` (`[Defaults]` `TrackSortType` and
//! `TrackPlayers`), saved as each command changes them, and reads the
//! filter mask from `TrackFilterType` at startup only; this client keeps
//! them with each character's options, seeded once from an installed
//! `eqclient.ini` (labelled departure: per character, not per
//! installation).
use crate::combat::ConColor;

/// How a Ranger's track list is ordered: `/tracksort` (0x47137e).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackSort {
    /// The server's order.
    #[default]
    Normal,
    /// Nearest first.
    Distance,
    /// By consider colour; the program's sort leaves the server's order.
    Consider,
    /// Farthest first.
    ReverseDistance,
    /// By consider colour, reversed; the program's sort leaves the server's
    /// order.
    ReverseConsider,
}

impl TrackSort {
    /// Each sort, by the program's number.
    pub const ALL: [Self; 5] = [
        Self::Normal,
        Self::Distance,
        Self::Consider,
        Self::ReverseDistance,
        Self::ReverseConsider,
    ];

    /// The word `eqclient.ini` keeps it as.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Distance => "DISTANCE",
            Self::Consider => "CONSIDER",
            Self::ReverseDistance => "RDISTANCE",
            Self::ReverseConsider => "RCONSIDER",
        }
    }

    /// The sort a saved word names, letter case aside.
    #[must_use]
    pub fn saved(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|sort| sort.word().eq_ignore_ascii_case(word.trim()))
    }

    /// The sort a typed word asks for, by its first letters: `RD` and `RC`
    /// before `N`, `D` and `C`. None for any other word.
    #[must_use]
    pub fn typed(word: &str) -> Option<Self> {
        let word = word.trim().to_ascii_uppercase();
        [
            ("RD", Self::ReverseDistance),
            ("RC", Self::ReverseConsider),
            ("N", Self::Normal),
            ("D", Self::Distance),
            ("C", Self::Consider),
        ]
        .into_iter()
        .find_map(|(start, sort)| word.starts_with(start).then_some(sort))
    }

    /// The program's line saying the sort is set.
    #[must_use]
    pub const fn line(self) -> u32 {
        match self {
            Self::Normal => 0x335d,
            Self::Distance => 0x335e,
            Self::Consider => 0x335f,
            Self::ReverseDistance => 0x3360,
            Self::ReverseConsider => 0x3361,
        }
    }

    /// Orders the list by the server's distances, as the program's sort
    /// 0x566ce6 does: only distance and reverse distance move a row; the
    /// rest keep the server's order. Ties keep the server's order.
    pub fn order<T>(self, rows: &mut [T], distance: impl Fn(&T) -> f32) {
        match self {
            Self::Distance => rows.sort_by(|a, b| distance(a).total_cmp(&distance(b))),
            Self::ReverseDistance => rows.sort_by(|a, b| distance(b).total_cmp(&distance(a))),
            Self::Normal | Self::Consider | Self::ReverseConsider => (),
        }
    }
}

/// The program's line for a `/tracksort` word it doesn't take.
pub const SORT_UNKNOWN: u32 = 0x3362;

/// Which players a track list shows: `/trackplayers` (0x468781).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackPlayers {
    /// No players.
    Off,
    /// Every player.
    #[default]
    On,
    /// Only the members of your group.
    Group,
}

impl TrackPlayers {
    /// Each choice, by the program's number.
    pub const ALL: [Self; 3] = [Self::Off, Self::On, Self::Group];

    /// The word `eqclient.ini` keeps it as.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Off => "FALSE",
            Self::On => "TRUE",
            Self::Group => "GROUP",
        }
    }

    /// The choice a saved word names, as the program reads it at startup:
    /// T or 1 lists players, G or 2 lists the group, anything else none.
    #[must_use]
    pub fn saved(word: &str) -> Self {
        match word.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
            Some('T' | '1') => Self::On,
            Some('G' | '2') => Self::Group,
            _ => Self::Off,
        }
    }

    /// The program's string-table word for the choice (0x3293 on, 0x3292
    /// off, 0x335c group), which a typed word is compared with.
    #[must_use]
    pub const fn word_line(self) -> u32 {
        match self {
            Self::Off => 0x3292,
            Self::On => 0x3293,
            Self::Group => 0x335c,
        }
    }

    /// This client's words for [`Self::word_line`] where the installation
    /// lacks them.
    #[must_use]
    pub const fn own_word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Group => "group",
        }
    }

    /// The program's line saying the choice is set.
    #[must_use]
    pub const fn line(self) -> u32 {
        match self {
            Self::On => 0x3354,
            Self::Off => 0x3355,
            Self::Group => 0x3356,
        }
    }
}

/// The program's line for a `/trackplayers` word it doesn't take.
pub const PLAYERS_UNKNOWN: u32 = 0x3357;

/// The colours `/trackfilter` hides from a Ranger's list, as the program's
/// mask (0x471607): green 0x1, blue 0x2, white 0x4, yellow 0x8, red 0x10
/// and light blue 0x20. A set bit hides the NPCs of that colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackFilter(pub u8);

/// Each colour the filter takes: its bit, the program's string-table word
/// for it, and this client's word where the installation lacks that.
pub const FILTER_COLOURS: [(ConColor, u8, u32, &str); 6] = [
    (ConColor::Green, 0x1, 0x3364, "green"),
    (ConColor::Blue, 0x2, 0x3365, "blue"),
    (ConColor::White, 0x4, 0x3366, "white"),
    (ConColor::Yellow, 0x8, 0x3368, "yellow"),
    (ConColor::Red, 0x10, 0x3367, "red"),
    (ConColor::LightBlue, 0x20, 0x3369, "light blue"),
];

/// The program's line when a colour's filter is set (it hides), with the
/// colour's word.
pub const FILTER_SET: u32 = 0x336c;
/// The program's line when a colour's filter is cleared.
pub const FILTER_CLEARED: u32 = 0x336b;

impl TrackFilter {
    /// Turns the colour's bit the other way; whether it is now set.
    pub fn toggle(&mut self, bit: u8) -> bool {
        self.0 ^= bit;
        self.0 & bit != 0
    }

    /// Whether an NPC of this colour is hidden. A colour the filter has no
    /// bit for is never hidden.
    #[must_use]
    pub fn hides(self, colour: ConColor) -> bool {
        FILTER_COLOURS
            .iter()
            .find(|(of, ..)| *of == colour)
            .is_some_and(|(_, bit, ..)| self.0 & bit != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sort_word_goes_by_its_first_letters() {
        for (word, sort) in [
            ("n", TrackSort::Normal),
            ("Normal", TrackSort::Normal),
            ("d", TrackSort::Distance),
            ("dist", TrackSort::Distance),
            ("C", TrackSort::Consider),
            ("rd", TrackSort::ReverseDistance),
            ("rdistance", TrackSort::ReverseDistance),
            ("RC", TrackSort::ReverseConsider),
        ] {
            assert_eq!(TrackSort::typed(word), Some(sort), "{word}");
        }
        for word in ["", "r", "x", "reverse"] {
            assert_eq!(TrackSort::typed(word), None, "{word}");
        }
        for sort in TrackSort::ALL {
            assert_eq!(TrackSort::saved(sort.word()), Some(sort));
        }
        assert_eq!(
            TrackSort::ALL.map(TrackSort::line),
            [0x335d, 0x335e, 0x335f, 0x3360, 0x3361]
        );
    }

    #[test]
    fn only_distance_and_reverse_distance_move_a_row() {
        let server = [(1, 30.0), (2, 10.0), (3, 20.0), (4, 10.0)];
        let ids = |sort: TrackSort| {
            let mut rows = server;
            sort.order(&mut rows, |row| row.1);
            rows.map(|row| row.0)
        };
        assert_eq!(ids(TrackSort::Normal), [1, 2, 3, 4]);
        assert_eq!(ids(TrackSort::Consider), [1, 2, 3, 4]);
        assert_eq!(ids(TrackSort::ReverseConsider), [1, 2, 3, 4]);
        assert_eq!(ids(TrackSort::Distance), [2, 4, 3, 1]);
        assert_eq!(ids(TrackSort::ReverseDistance), [1, 3, 2, 4]);
    }

    #[test]
    fn the_players_choice_reads_as_the_program_reads_its_key() {
        assert_eq!(TrackPlayers::saved("TRUE"), TrackPlayers::On);
        assert_eq!(TrackPlayers::saved("1"), TrackPlayers::On);
        assert_eq!(TrackPlayers::saved("group"), TrackPlayers::Group);
        assert_eq!(TrackPlayers::saved("2"), TrackPlayers::Group);
        assert_eq!(TrackPlayers::saved("FALSE"), TrackPlayers::Off);
        assert_eq!(TrackPlayers::saved(""), TrackPlayers::Off);
        assert_eq!(TrackPlayers::default(), TrackPlayers::On);
        for players in TrackPlayers::ALL {
            assert_eq!(TrackPlayers::saved(players.word()), players);
        }
    }

    #[test]
    fn a_filter_bit_hides_its_colour_and_toggles_back() {
        let mut filter = TrackFilter::default();
        assert!(!filter.hides(ConColor::Green));
        assert!(filter.toggle(0x1));
        assert!(filter.hides(ConColor::Green));
        assert!(!filter.hides(ConColor::LightBlue));
        assert!(filter.toggle(0x20));
        assert!(filter.hides(ConColor::LightBlue));
        assert!(!filter.toggle(0x1));
        assert!(!filter.hides(ConColor::Green));
        assert_eq!(filter, TrackFilter(0x20));
    }
}

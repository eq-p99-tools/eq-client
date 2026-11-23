//! The character's server filters: which kinds of message the player asks
//! the server not to send ([`eq_network_game::filters`]). The session sends
//! them at zone-in and, where the generation's client sends a change at
//! once, then too.
//!
//! The Titanium client keeps 28 filters (eqgame.exe table 0x66cd78, read
//! R49-2, recording checklist 6.494b and c), each under its own key in
//! `eqclient.ini` `[Options]` (`Channel00` to `Channel09`, `BadWord`,
//! `PCSpells` and the rest), as the index of the choice its Options row
//! shows. This client keeps the same keys and indices, but per character in
//! the character's options file: that is this client's choice, as the
//! original keeps one set per install. An index outside a row's choices
//! gives the row's default, as the original's (0x60cac0) does.

use std::fmt::Write as _;

use eq_network_game::{
    GameDialect,
    filters::{FilterCategory, FilterMode, ServerFilters},
};

/// A mode for every kind, kept by value with the other options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatFilters([FilterMode; FilterCategory::ALL.len()]);

impl Default for ChatFilters {
    /// Each kind's default: the packet the client sends today.
    fn default() -> Self {
        Self(FilterCategory::ALL.map(FilterCategory::default_mode))
    }
}

/// The Titanium client's ini key for each kind, in [`FilterCategory::ALL`]'s
/// order, which is the program's table order.
const KEYS: [&str; FilterCategory::ALL.len()] = [
    "Channel00",
    "Channel01",
    "Channel02",
    "Channel03",
    "Channel04",
    "Channel05",
    "BadWord",
    "PCSpells",
    "NPCSpells",
    "BardSongs",
    "CriticalSpells",
    "CriticalMelee",
    "SpellDamage",
    "Channel06",
    "Channel07",
    "Channel08",
    "Channel09",
    "HideDamageShield",
    "DotDamage",
    "PetAttacks",
    "PetMisses",
    "FocusEffects",
    "PetSpells",
    "HealOverTimeSpells",
    "ItemSpeech",
    "Strikethrough",
    "Stun",
    "SwarmPetDeath",
];

/// A kind's choices in the Titanium client's own order, so a mode's place
/// is the index a file keeps.
fn choices(category: FilterCategory) -> Vec<FilterMode> {
    category.modes(GameDialect::Titanium)
}

/// Where a kind sits in [`FilterCategory::ALL`].
fn place(category: FilterCategory) -> usize {
    FilterCategory::ALL
        .iter()
        .position(|listed| *listed == category)
        .unwrap_or_default()
}

impl ChatFilters {
    /// The mode for a kind.
    #[must_use]
    pub fn get(&self, category: FilterCategory) -> FilterMode {
        self.0[place(category)]
    }

    /// Sets the mode for a kind.
    pub fn set(&mut self, category: FilterCategory, mode: FilterMode) {
        self.0[place(category)] = mode;
    }

    /// The kinds the generation's packet has a word for and the Options
    /// page has a row for, each with the modes it can say, in the program's
    /// order; none for a generation that sends no filters yet. Swarm pets'
    /// deaths have no row in the original's Options window, so none here:
    /// the file keeps the kind and the packet sends it.
    #[must_use]
    pub fn kinds(dialect: GameDialect) -> Vec<(FilterCategory, Vec<FilterMode>)> {
        FilterCategory::ALL
            .into_iter()
            .filter(|category| has_row(*category))
            .map(|category| (category, category.modes(dialect)))
            .filter(|(_, modes)| !modes.is_empty())
            .collect()
    }

    /// The next mode the generation's word can say for this kind after the
    /// current one, as a page's click steps it; `None` where the packet has
    /// no word for the kind.
    #[must_use]
    pub fn next(&self, category: FilterCategory, dialect: GameDialect) -> Option<FilterMode> {
        let modes = category.modes(dialect);
        let now = modes.iter().position(|mode| *mode == self.get(category));
        let next = now.map_or(0, |now| (now + 1) % modes.len());
        modes.get(next).copied()
    }

    /// The set the session is told for the generation: a mode its word
    /// cannot say goes back to the kind's default.
    #[must_use]
    pub fn for_session(&self, dialect: GameDialect) -> ServerFilters {
        let mut filters = ServerFilters::default();
        for category in FilterCategory::ALL {
            filters.set(category, self.get(category));
        }
        filters.fitted(dialect)
    }

    /// Reads one options-file line's kind and index, if its key is one of
    /// the original's. An index outside the row's choices, or a value that is
    /// no index, gives the row's default.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let Some(place) = KEYS.iter().position(|known| *known == key) else {
            return false;
        };
        let category = FilterCategory::ALL[place];
        let mode = value
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| choices(category).get(index).copied())
            .unwrap_or_else(|| category.default_mode());
        self.set(category, mode);
        true
    }

    /// The options-file lines, one per kind, each the original's key and
    /// the index of the kind's choice.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (place, category) in FilterCategory::ALL.into_iter().enumerate() {
            let index = choices(category)
                .iter()
                .position(|mode| *mode == self.get(category))
                .unwrap_or_default();
            let _ = writeln!(text, "{} = {index}", KEYS[place]);
        }
        text
    }
}

/// The kinds the original's Options window has a row for, in its order.
pub fn rows() -> impl Iterator<Item = FilterCategory> {
    FilterCategory::ALL
        .into_iter()
        .filter(|category| has_row(*category))
}

/// Whether the original's Options window has a row for the kind: every kind
/// but swarm pets' deaths, which it keeps with no row.
#[must_use]
pub const fn has_row(category: FilterCategory) -> bool {
    !matches!(category, FilterCategory::SwarmPetDeath)
}

/// A mode's words on a page.
#[must_use]
pub const fn mode_words(mode: FilterMode) -> &'static str {
    match mode {
        FilterMode::Show => "Show",
        FilterMode::Hide => "Hide",
        FilterMode::MineOnly => "Mine only",
        FilterMode::GroupOnly => "Group only",
    }
}

/// A kind's words on a page: this client's own, as the original's row
/// labels are its own strings.
#[must_use]
pub const fn kind_words(category: FilterCategory) -> &'static str {
    match category {
        FilterCategory::GuildChat => "Guild chat",
        FilterCategory::Socials => "Socials",
        FilterCategory::GroupChat => "Group chat",
        FilterCategory::Shouts => "Shouts",
        FilterCategory::Auctions => "Auctions",
        FilterCategory::Ooc => "Out of character",
        FilterCategory::BadWords => "Bad words",
        FilterCategory::PcSpells => "Players' spells",
        FilterCategory::NpcSpells => "Creatures' spells",
        FilterCategory::BardSongs => "Bard songs",
        FilterCategory::SpellCrits => "Critical spells",
        FilterCategory::MeleeCrits => "Critical hits",
        FilterCategory::SpellDamage => "Spell damage",
        FilterCategory::MyMisses => "My misses",
        FilterCategory::OthersMisses => "Others' misses",
        FilterCategory::OthersHits => "Others' hits",
        FilterCategory::MissedMe => "Attacks that miss me",
        FilterCategory::DamageShields => "Damage shields",
        FilterCategory::DamageOverTime => "Damage over time",
        FilterCategory::PetHits => "Pet hits",
        FilterCategory::PetMisses => "Pet misses",
        FilterCategory::FocusEffects => "Focus effects",
        FilterCategory::PetSpells => "Pet spells",
        FilterCategory::HealOverTime => "Heals over time",
        FilterCategory::ItemSpeech => "Item speech",
        FilterCategory::Strikethrough => "Strikethrough",
        FilterCategory::Stuns => "Stuns",
        FilterCategory::SwarmPetDeath => "Swarm pet deaths",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_tables() {
        for dialect in [GameDialect::Titanium, GameDialect::EqMac] {
            assert_eq!(
                ChatFilters::default().for_session(dialect),
                ServerFilters::default()
            );
        }
        let text = ChatFilters::default().text();
        assert!(text.starts_with("Channel00 = 1\n"));
        assert!(text.contains("BadWord = 0\n"));
        assert!(text.contains("PCSpells = 0\n"));
        assert!(text.contains("Channel09 = 1\n"));
        assert!(text.ends_with("SwarmPetDeath = 0\n"));
        assert_eq!(text.lines().count(), 28);
    }

    #[test]
    fn filters_keep_the_originals_keys_and_indices() {
        let mut filters = ChatFilters::default();
        filters.set(FilterCategory::Ooc, FilterMode::Hide);
        filters.set(FilterCategory::BardSongs, FilterMode::GroupOnly);
        filters.set(FilterCategory::BadWords, FilterMode::Show);
        filters.set(FilterCategory::Stuns, FilterMode::Hide);
        let text = filters.text();
        assert!(text.contains("Channel05 = 0\n"));
        assert!(text.contains("BardSongs = 2\n"));
        assert!(text.contains("BadWord = 1\n"));
        assert!(text.contains("Stun = 1\n"));
        let mut read = ChatFilters::default();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read.read(key.trim(), value));
        }
        assert_eq!(read, filters);
    }

    #[test]
    fn an_index_outside_the_choices_gives_the_rows_default() {
        let mut bad = ChatFilters::default();
        bad.set(FilterCategory::Shouts, FilterMode::Hide);
        assert!(bad.read("Channel03", "7"));
        assert!(bad.read("CriticalMelee", "-1"));
        assert!(bad.read("BadWord", "loud"));
        assert!(!bad.read("filter_ooc", "hide"));
        assert!(!bad.read("show_helm", "true"));
        assert_eq!(bad, ChatFilters::default());
    }

    #[test]
    fn each_generation_lists_its_rows_in_the_programs_order() {
        let titanium = ChatFilters::kinds(GameDialect::Titanium);
        // 28 kinds, less swarm pets' deaths, which has no row.
        assert_eq!(titanium.len(), 27);
        assert_eq!(titanium[0].0, FilterCategory::GuildChat);
        assert_eq!(titanium[6].0, FilterCategory::BadWords);
        assert_eq!(titanium[26].0, FilterCategory::Stuns);
        assert_eq!(
            titanium[9].1,
            [
                FilterMode::Show,
                FilterMode::MineOnly,
                FilterMode::GroupOnly,
                FilterMode::Hide
            ]
        );
        let eqmac = ChatFilters::kinds(GameDialect::EqMac);
        assert_eq!(eqmac.len(), 16);
        assert!(
            !eqmac
                .iter()
                .any(|(category, _)| *category == FilterCategory::DamageOverTime)
        );
        assert_eq!(ChatFilters::kinds(GameDialect::Rof2), Vec::new());
        let mut filters = ChatFilters::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            let next = filters
                .next(FilterCategory::BardSongs, GameDialect::Titanium)
                .unwrap();
            filters.set(FilterCategory::BardSongs, next);
            seen.push(next);
        }
        assert_eq!(
            seen,
            [
                FilterMode::MineOnly,
                FilterMode::GroupOnly,
                FilterMode::Hide,
                FilterMode::Show
            ]
        );
        assert_eq!(
            filters.next(FilterCategory::DamageOverTime, GameDialect::EqMac),
            None
        );
        // A channel row steps from its default, shown, to hidden.
        assert_eq!(
            ChatFilters::default().next(FilterCategory::Ooc, GameDialect::Titanium),
            Some(FilterMode::Hide)
        );
    }

    #[test]
    fn a_mode_a_generation_cannot_say_goes_to_its_default_for_the_session() {
        let mut filters = ChatFilters::default();
        filters.set(FilterCategory::DamageOverTime, FilterMode::Hide);
        filters.set(FilterCategory::Shouts, FilterMode::Hide);
        let eqmac = filters.for_session(GameDialect::EqMac);
        assert_eq!(eqmac.get(FilterCategory::Shouts), FilterMode::Hide);
        assert!(eqmac.check(GameDialect::EqMac).is_ok());
        assert!(
            filters
                .for_session(GameDialect::Titanium)
                .check(GameDialect::Titanium)
                .is_ok()
        );
    }
}

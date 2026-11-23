//! The character's server filters: which kinds of message the player asks
//! the server not to send ([`eq_network_game::filters`]). The session sends
//! them at zone-in and, where the server type reads a change then, at once.
//!
//! Which of the official client's options sets each kind, the ini keys it
//! keeps them under, and its `/filter` forms are unread (recording
//! checklist 6.494a to c; read request R49 part 2). Until then this client
//! keeps one mode per kind in the character's options file, under its own
//! `filter_<kind>` names, and sets them only from its own Options page.

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

    /// The kinds the generation's packet has a word for, each with the
    /// modes it can say, in the order a page lists them; none for a
    /// generation that sends no filters yet.
    #[must_use]
    pub fn kinds(dialect: GameDialect) -> Vec<(FilterCategory, Vec<FilterMode>)> {
        FilterCategory::ALL
            .into_iter()
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

    /// Reads one options-file line's kind and mode, if it names one. A mode
    /// no generation can say for the kind is not read, so the line keeps
    /// the default.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let Some(category) = key.strip_prefix(PREFIX).and_then(FilterCategory::from_key) else {
            return false;
        };
        let taken = |mode: &FilterMode| {
            [GameDialect::Titanium, GameDialect::EqMac, GameDialect::Rof2]
                .into_iter()
                .any(|dialect| category.modes(dialect).contains(mode))
        };
        if let Some(mode) = FilterMode::from_key(value.trim()).filter(taken) {
            self.set(category, mode);
        }
        true
    }

    /// The options-file lines, one per kind.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for category in FilterCategory::ALL {
            let _ = writeln!(
                text,
                "{PREFIX}{} = {}",
                category.key(),
                self.get(category).key()
            );
        }
        text
    }
}

/// How an options-file line names a kind.
const PREFIX: &str = "filter_";

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

/// A kind's words on a page: this client's own, as the official options
/// are unread (6.494b).
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
    fn the_default_is_the_packet_sent_today() {
        for dialect in [GameDialect::Titanium, GameDialect::EqMac] {
            assert_eq!(
                ChatFilters::default().for_session(dialect),
                ServerFilters::default()
            );
        }
    }

    #[test]
    fn filters_keep_to_their_file_and_bad_lines_keep_the_default() {
        let mut filters = ChatFilters::default();
        filters.set(FilterCategory::Ooc, FilterMode::Hide);
        filters.set(FilterCategory::BardSongs, FilterMode::GroupOnly);
        let text = filters.text();
        assert!(text.contains("filter_ooc = hide\n"));
        let mut read = ChatFilters::default();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read.read(key.trim(), value));
        }
        assert_eq!(read, filters);
        let mut bad = ChatFilters::default();
        assert!(bad.read("filter_melee_crits", "group"));
        assert!(bad.read("filter_shouts", "loud"));
        assert!(!bad.read("filter_unknown", "hide"));
        assert!(!bad.read("show_helm", "true"));
        assert_eq!(bad, ChatFilters::default());
    }

    #[test]
    fn each_generation_lists_and_steps_only_what_its_packet_says() {
        let titanium = ChatFilters::kinds(GameDialect::Titanium);
        assert_eq!(titanium.len(), 24);
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
        // Damage over time's Titanium default is mine only; hidden next.
        assert_eq!(
            ChatFilters::default().next(FilterCategory::DamageOverTime, GameDialect::Titanium),
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

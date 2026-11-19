//! When a loot click on a NO DROP item asks first, as `/lootnodrop` sets it
//! (Titanium 0x47024f). The official client keeps it as `LootNoDrop`, 0 to
//! 2; its loot click (0x52b9d5) takes the item at once from the player's own
//! corpse, an item that isn't NO DROP, under 2, or under 1 when the player
//! can use the item, and asks otherwise.

/// How a loot click treats a NO DROP item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LootNoDrop {
    /// Asks before every one (0). The default where the installation keeps
    /// no choice, which the program read doesn't give (checklist 6.487).
    #[default]
    Ask,
    /// Asks only before one the player can't use (1).
    AskUnusable,
    /// Never asks (2).
    Take,
}

impl LootNoDrop {
    /// Every choice, by its number.
    pub const ALL: [Self; 3] = [Self::Ask, Self::AskUnusable, Self::Take];

    /// The choice by its number, clamped to 0-2 as the program clamps it.
    #[must_use]
    pub const fn from_level(level: i64) -> Self {
        match level {
            ..=0 => Self::Ask,
            1 => Self::AskUnusable,
            _ => Self::Take,
        }
    }

    /// Its number, as a file keeps it.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Ask => 0,
            Self::AskUnusable => 1,
            Self::Take => 2,
        }
    }

    /// Whether a loot click asks before taking a NO DROP item from someone
    /// else's corpse, by whether the player can use it.
    #[must_use]
    pub const fn asks(self, usable: bool) -> bool {
        match self {
            Self::Ask => true,
            Self::AskUnusable => !usable,
            Self::Take => false,
        }
    }

    /// The line `/lootnodrop` prints for it, which also names the word that
    /// picks it (0x32bb to 0x32bd, in that order as far as is known, checklist
    /// 6.487). The words are this client's where the installation lacks the
    /// string.
    #[must_use]
    pub const fn line(self) -> (u32, &'static str) {
        match self {
            Self::Ask => (0x32bb, "never: ask before looting any NO DROP item."),
            Self::AskUnusable => (
                0x32bc,
                "sometimes: ask before looting a NO DROP item you can't use.",
            ),
            Self::Take => (0x32bd, "always: loot NO DROP items without asking."),
        }
    }

    /// The choice a `/lootnodrop` word picks: the first word of its line,
    /// or the whole line, ignoring case. `lines` gives each choice's line as
    /// the player's client words it.
    #[must_use]
    pub fn typed(word: &str, lines: impl Fn(Self) -> String) -> Option<Self> {
        let word = word.trim();
        if word.is_empty() {
            return None;
        }
        Self::ALL.into_iter().find(|choice| {
            let line = lines(*choice);
            let first = line
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches(|c: char| !c.is_alphanumeric());
            line.trim().eq_ignore_ascii_case(word) || first.eq_ignore_ascii_case(word)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_is_clamped_and_each_choice_asks_as_the_loot_click_does() {
        assert_eq!(LootNoDrop::from_level(-3), LootNoDrop::Ask);
        assert_eq!(LootNoDrop::from_level(1), LootNoDrop::AskUnusable);
        assert_eq!(LootNoDrop::from_level(9), LootNoDrop::Take);
        for choice in LootNoDrop::ALL {
            assert_eq!(LootNoDrop::from_level(choice.level().into()), choice);
        }
        assert!(LootNoDrop::Ask.asks(true) && LootNoDrop::Ask.asks(false));
        assert!(!LootNoDrop::AskUnusable.asks(true) && LootNoDrop::AskUnusable.asks(false));
        assert!(!LootNoDrop::Take.asks(true) && !LootNoDrop::Take.asks(false));
    }

    #[test]
    fn a_word_picks_the_line_it_starts_or_is() {
        let ours = |choice: LootNoDrop| choice.line().1.to_owned();
        assert_eq!(LootNoDrop::typed("NEVER", ours), Some(LootNoDrop::Ask));
        assert_eq!(
            LootNoDrop::typed(" sometimes ", ours),
            Some(LootNoDrop::AskUnusable)
        );
        assert_eq!(LootNoDrop::typed("always", ours), Some(LootNoDrop::Take));
        assert_eq!(LootNoDrop::typed("", ours), None);
        assert_eq!(LootNoDrop::typed("2", ours), None);
        let installed =
            |choice: LootNoDrop| ["Zero", "One - x", "Two"][usize::from(choice.level())].to_owned();
        assert_eq!(
            LootNoDrop::typed("one", installed),
            Some(LootNoDrop::AskUnusable)
        );
        assert_eq!(LootNoDrop::typed("never", installed), None);
    }
}

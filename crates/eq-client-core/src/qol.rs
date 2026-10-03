//! Quality-of-life fixes: where this client departs on purpose from the
//! official client, or from a server's defaults, to be friendlier to the
//! player. Everything else copies the official client, so each fix is a
//! labelled exception, defined once here: what it changes, where it may run,
//! and whether the player can turn it off.
//!
//! Code that carries a fix out asks [`Settings::on`] whether it is on, even
//! for a fix that is always on, so a fix moves between always on and a
//! setting by its definition alone.

use crate::Capability;

/// What a fix changes, which decides where it may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Only what the player sees or hears: what the official client already
    /// shows, in a clearer form, or what the player could work out
    /// themselves, never what the official client keeps from them. It sends
    /// nothing different, so it runs on every server.
    Shows,
    /// Asks first, holds back or refuses before something the official
    /// client would do at once. It sends less or later, never more, so it
    /// runs on every server.
    Guards,
    /// Sends something on the player's behalf that the official client would
    /// not. Like every feature that sends, the session offers it only where
    /// the server type lists it ([`Fix::needs`]).
    Acts,
}

/// Whether the player can turn a fix off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Always on: it only adds information or blocks a plain mistake, takes
    /// no extra step, and nobody would want it off.
    AlwaysOn,
    /// A setting on the Options window's quality-of-life page, kept per
    /// character.
    Setting {
        /// Whether it is on for a character who has not chosen.
        default: bool,
    },
}

/// A quality-of-life fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fix {
    /// Leaves food and drink with modifiers for the player to eat or drink
    /// by hand when they turn hungry or thirsty; the official client eats
    /// and drinks whatever comes first.
    SkipModifiedFood,
    /// Draws a window the UI skin sizes to nothing, as the Velious skin does
    /// its windows from later expansions, as the default skin draws it.
    HiddenWindows,
}

impl Fix {
    /// Every fix, in the order the quality-of-life page lists the settings.
    pub const ALL: [Self; 2] = [Self::SkipModifiedFood, Self::HiddenWindows];

    /// The name an options file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "skip_modified_food",
            Self::HiddenWindows => "hidden_windows",
        }
    }

    /// The fix whose file name this is.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|fix| fix.key() == key)
    }

    /// Its checkbox's words on the quality-of-life page.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "Skip Food With Modifiers",
            Self::HiddenWindows => "Draw Windows the Skin Hides",
        }
    }

    /// What its checkbox says on hover.
    #[must_use]
    pub const fn tooltip(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => {
                "Leave food and drink with modifiers for you to eat or drink by hand when you get hungry or thirsty."
            }
            Self::HiddenWindows => {
                "Draw the windows your UI skin hides, such as the Velious skin's Raid window, as the default skin draws them."
            }
        }
    }

    /// What it changes.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::SkipModifiedFood => Kind::Guards,
            Self::HiddenWindows => Kind::Shows,
        }
    }

    /// What the session must offer for the fix to matter, if anything:
    /// eating on its own comes with the inventory.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::SkipModifiedFood => Some(Capability::Inventory),
            Self::HiddenWindows => None,
        }
    }

    /// Whether it is always on or the player's choice: windows the skin
    /// hides stay hidden unless the player asks for them, as Adam chose.
    #[must_use]
    pub const fn availability(self) -> Availability {
        match self {
            Self::SkipModifiedFood => Availability::Setting { default: true },
            Self::HiddenWindows => Availability::Setting { default: false },
        }
    }

    /// Whether the player can turn it off on the quality-of-life page.
    #[must_use]
    pub const fn is_setting(self) -> bool {
        matches!(self.availability(), Availability::Setting { .. })
    }

    /// The fixes the player can turn off, in the quality-of-life page's
    /// order.
    pub fn settings() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(|fix| fix.is_setting())
    }
}

/// Which fixes are on: every one that is always on, and each setting as the
/// player chose it or else as it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings([bool; Fix::ALL.len()]);

impl Default for Settings {
    fn default() -> Self {
        Self(Fix::ALL.map(|fix| match fix.availability() {
            Availability::AlwaysOn => true,
            Availability::Setting { default } => default,
        }))
    }
}

impl Settings {
    /// Whether the fix is on.
    #[must_use]
    pub const fn on(&self, fix: Fix) -> bool {
        match fix.availability() {
            Availability::AlwaysOn => true,
            Availability::Setting { .. } => self.0[fix as usize],
        }
    }

    /// Turns a setting on or off; one that is always on stays on.
    pub const fn set(&mut self, fix: Fix, on: bool) {
        if fix.is_setting() {
            self.0[fix as usize] = on;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_fix_is_defined_once_and_in_order() {
        for (index, fix) in Fix::ALL.into_iter().enumerate() {
            assert_eq!(fix as usize, index, "{fix:?} is out of order");
            assert_eq!(Fix::from_key(fix.key()), Some(fix));
            assert!(!fix.label().is_empty() && !fix.tooltip().is_empty());
            // A fix that sends on the player's behalf rides on a feature the
            // session offers only where its server type lists it.
            if fix.kind() == Kind::Acts {
                assert!(fix.needs().is_some(), "{fix:?} names no feature");
            }
        }
        let mut keys: Vec<_> = Fix::ALL.iter().map(|fix| fix.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Fix::ALL.len());
        assert_eq!(Fix::from_key("shiny"), None);
    }

    #[test]
    fn settings_start_as_defined_and_change_only_where_the_player_can() {
        let mut settings = Settings::default();
        assert!(settings.on(Fix::SkipModifiedFood));
        assert!(!settings.on(Fix::HiddenWindows));
        settings.set(Fix::HiddenWindows, true);
        settings.set(Fix::SkipModifiedFood, false);
        assert!(settings.on(Fix::HiddenWindows));
        assert!(!settings.on(Fix::SkipModifiedFood));
        assert_eq!(
            Fix::settings().collect::<Vec<_>>(),
            [Fix::SkipModifiedFood, Fix::HiddenWindows]
        );
        // A fix that is always on stays on whatever is asked.
        for fix in Fix::ALL.into_iter().filter(|fix| !fix.is_setting()) {
            settings.set(fix, false);
            assert!(settings.on(fix), "{fix:?} turned off");
        }
    }
}

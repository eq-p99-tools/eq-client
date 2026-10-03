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
    /// Asks Yes or No before a spell leaves the spell book for good; the
    /// official client's Delete key deletes the chosen spell at once.
    AskBeforeDeletingSpells,
    /// Starts the Options window's Max FPS at 60 frames a second rather than
    /// as fast as vsync allows; the slider and the installation's
    /// `eqclient.ini` still set another cap. What the official client does
    /// with no cap set is not checked.
    FrameCap,
    /// Greys out the controls for what the server type does not offer, and
    /// says on hover why a control is unavailable; the official client only
    /// ever knows its own server.
    GreyedControls,
    /// Counts down a camp's preparation on the action bar, besides what the
    /// chat says.
    CampCountdown,
    /// Says the same refusal at most once every 3 seconds, so one a held key
    /// repeats each frame does not fill the chat.
    QuietRepeats,
    /// Keeps a hotbar item button to the item it was made with, so another
    /// item later put in its place is not used by mistake (inferred: what
    /// the official client's button uses then is not checked).
    HotbarItemGuard,
    /// Counts down how long a buff has left in its tooltip, about, from what
    /// the server last said or from the spell's own duration.
    BuffTimeLeft,
    /// Shows a weapon's damage divided by its delay, the ratio players weigh
    /// weapons by, under those two in the item display.
    WeaponRatio,
}

impl Fix {
    /// Every fix: the settings in the order the quality-of-life page lists
    /// them, then the fixes that are always on.
    pub const ALL: [Self; 10] = [
        Self::SkipModifiedFood,
        Self::HiddenWindows,
        Self::AskBeforeDeletingSpells,
        Self::FrameCap,
        Self::GreyedControls,
        Self::CampCountdown,
        Self::QuietRepeats,
        Self::HotbarItemGuard,
        Self::BuffTimeLeft,
        Self::WeaponRatio,
    ];

    /// The name an options file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "skip_modified_food",
            Self::HiddenWindows => "hidden_windows",
            Self::AskBeforeDeletingSpells => "ask_before_deleting_spells",
            Self::FrameCap => "frame_cap",
            Self::GreyedControls => "greyed_controls",
            Self::CampCountdown => "camp_countdown",
            Self::QuietRepeats => "quiet_repeats",
            Self::HotbarItemGuard => "hotbar_item_guard",
            Self::BuffTimeLeft => "buff_time_left",
            Self::WeaponRatio => "weapon_ratio",
        }
    }

    /// The fix whose file name this is.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|fix| fix.key() == key)
    }

    /// Its checkbox's words on the quality-of-life page, and its name in
    /// the README.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "Skip Food With Modifiers",
            Self::HiddenWindows => "Draw Windows the Skin Hides",
            Self::AskBeforeDeletingSpells => "Ask Before Deleting Spells",
            Self::FrameCap => "Cap Frames at 60",
            Self::GreyedControls => "Grey Out What Is Unavailable",
            Self::CampCountdown => "Count Down Camping",
            Self::QuietRepeats => "Say Each Refusal Once",
            Self::HotbarItemGuard => "Keep Hotbar Items to Their Item",
            Self::BuffTimeLeft => "Show Buff Time Left",
            Self::WeaponRatio => "Show Weapon Ratio",
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
            Self::AskBeforeDeletingSpells => {
                "Ask Yes or No before a spell is deleted from your spell book for good."
            }
            Self::FrameCap => {
                "Start the Max FPS slider at 60 frames a second instead of drawing as fast as your monitor allows."
            }
            Self::GreyedControls => {
                "Grey out the buttons for what this server does not offer, and say why on hover."
            }
            Self::CampCountdown => "Count down the seconds until a camp ends on the action bar.",
            Self::QuietRepeats => {
                "Say the same refusal at most once every 3 seconds, so a held key does not fill the chat."
            }
            Self::HotbarItemGuard => {
                "Use a hotbar item button only for the item it was made with, never another item later put in its place."
            }
            Self::BuffTimeLeft => "Count down about how long each buff has left in its tooltip.",
            Self::WeaponRatio => {
                "Show a weapon's damage divided by its delay under them in the item display."
            }
        }
    }

    /// What it changes.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::SkipModifiedFood | Self::AskBeforeDeletingSpells | Self::HotbarItemGuard => {
                Kind::Guards
            }
            Self::HiddenWindows
            | Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::BuffTimeLeft
            | Self::WeaponRatio => Kind::Shows,
        }
    }

    /// What the session must offer for the fix to matter, if anything:
    /// eating on its own comes with the inventory, and only a session that
    /// deletes spells has one to ask about.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::SkipModifiedFood => Some(Capability::Inventory),
            Self::AskBeforeDeletingSpells => Some(Capability::DeletingSpells),
            Self::HiddenWindows
            | Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::HotbarItemGuard
            | Self::BuffTimeLeft
            | Self::WeaponRatio => None,
        }
    }

    /// Whether it is always on or the player's choice: windows the skin
    /// hides stay hidden unless the player asks for them, as Adam chose, and
    /// what only adds what the player could work out, or stops a plain
    /// mistake, is always on.
    #[must_use]
    pub const fn availability(self) -> Availability {
        match self {
            Self::SkipModifiedFood | Self::AskBeforeDeletingSpells => {
                Availability::Setting { default: true }
            }
            Self::HiddenWindows => Availability::Setting { default: false },
            Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::HotbarItemGuard
            | Self::BuffTimeLeft
            | Self::WeaponRatio => Availability::AlwaysOn,
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
        // The settings come first, in the page's order.
        assert!(Fix::ALL.is_sorted_by_key(|fix| !fix.is_setting()));
        let mut keys: Vec<_> = Fix::ALL.iter().map(|fix| fix.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Fix::ALL.len());
        assert_eq!(Fix::from_key("shiny"), None);
    }

    #[test]
    fn the_readme_names_every_fix() {
        let readme = include_str!("../../../README.md");
        for fix in Fix::ALL {
            assert!(
                readme.contains(&format!("**{}**", fix.label())),
                "the README's Quality of life section leaves out {fix:?}"
            );
        }
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
            [
                Fix::SkipModifiedFood,
                Fix::HiddenWindows,
                Fix::AskBeforeDeletingSpells
            ]
        );
        // A fix that is always on stays on whatever is asked.
        for fix in Fix::ALL.into_iter().filter(|fix| !fix.is_setting()) {
            settings.set(fix, false);
            assert!(settings.on(fix), "{fix:?} turned off");
        }
    }
}

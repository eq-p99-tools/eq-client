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
    /// Local presentation of installed assets and already received world
    /// state. It sends nothing different, so it runs on every server.
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
    /// Starts each chat line with the time it arrived, by the computer's
    /// clock, as the chat log keeps it.
    ShowTimesInChat,
    /// Asks for the player's attention when a tell arrives while the client
    /// is not the focused window: on Windows its taskbar button flashes
    /// until the client is focused again.
    FlashOnTells,
    /// Lays a corpse's body along the floor under it, pitched and rolled to
    /// the slope, rather than level with its soles, where it sinks into a
    /// hillside ([`crate::corpse_tilt`]). Adam's choice (10-20); what the
    /// official client does is inferred, checklist item 6.282.
    CorpsesAlongGround,
    /// Lets the chase camera zoom out far past the 200 units behind the
    /// player the Titanium program allows (program read N2 of 11-01); turned
    /// off, the camera keeps to the program's limit. Its height above the
    /// player stays the program's either way. Adam's choice (11-01).
    LongChaseCamera,
    /// Offers the free camera: a user-assigned key detaches the camera from
    /// the character to fly it anywhere, through walls and terrain, until the
    /// key returns it; the character never moves and nothing is sent. The
    /// official client has no such camera. Adam's choice (11-01).
    FreeCamera,
    /// Lets the player see the scene clearly regardless of the outdoor
    /// hour: nights are lit as noon lights them, the zone's distance fog
    /// stays off the view, and any rain or snow stays out of sight.
    /// Nothing the world sees changes.
    ClearView,
    /// Allows horizontal movement while stunned; other stun restrictions remain.
    WalkWhileWinded,
    /// Opens newly slain nearby NPC corpses naming the player as killer and
    /// uses reply-driven Loot All. NO DROP items remain for manual looting.
    AutoLoot,
    /// Draws received positive damage above known targets without changing chat.
    FloatingCombatText,
    /// Reads the spellbook without automatically sitting.
    ReadWhileStanding,
    /// Memorizes without automatic sitting or the client preparation pause.
    /// Existing source, admission and action validation still applies.
    MemorizeWhileStanding,
    /// Draws known invisible entities and marks names with parentheses.
    /// Reported visibility and interaction rules remain unchanged.
    ShowInvisible,
    /// Answers supported P99 presence polls after an application-selected delay.
    /// Joining a list and numeric challenges are separate actions.
    AnswerListPolls,
    /// Reports supported client-computed hazards. Disabling withholds reports;
    /// server-generated damage still applies.
    TakeEnvironmentalDamage,
    /// Disables local air depletion and the Air Remaining window.
    GillsForDays,
    /// Increases verified forward running speed by ten percent, as explicit
    /// client policy, without a guarantee of server acceptance.
    RunBoost,
    /// Bounded flight using Jump to ascend and Duck to descend, retaining collision.
    Fly,
    /// Lets ordinary ground movement stand on mapped water and lava.
    JesusMode,
    /// Requests logout without sitting or camp preparation, then waits for its outcome.
    ExitAtOnce,
    /// Makes the local map and known entity positions available on every
    /// server, independently of the original client's map support.
    MapEverywhere,
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
    /// Opens the windows always on screen, until the player places them here
    /// or in the official client, where none covers another: each in a
    /// corner or along an edge of the screen. The official client opens each
    /// where the UI skin puts it.
    WindowsApart,
}

impl Fix {
    /// Every fix: the settings in the order the quality-of-life page lists
    /// them, then the fixes that are always on.
    pub const ALL: [Self; 31] = [
        Self::SkipModifiedFood,
        Self::HiddenWindows,
        Self::AskBeforeDeletingSpells,
        Self::ShowTimesInChat,
        Self::FlashOnTells,
        Self::CorpsesAlongGround,
        Self::LongChaseCamera,
        Self::FreeCamera,
        Self::ClearView,
        Self::WalkWhileWinded,
        Self::AutoLoot,
        Self::FloatingCombatText,
        Self::ReadWhileStanding,
        Self::MemorizeWhileStanding,
        Self::ShowInvisible,
        Self::AnswerListPolls,
        Self::TakeEnvironmentalDamage,
        Self::GillsForDays,
        Self::RunBoost,
        Self::Fly,
        Self::JesusMode,
        Self::ExitAtOnce,
        Self::MapEverywhere,
        Self::FrameCap,
        Self::GreyedControls,
        Self::CampCountdown,
        Self::QuietRepeats,
        Self::HotbarItemGuard,
        Self::BuffTimeLeft,
        Self::WeaponRatio,
        Self::WindowsApart,
    ];

    /// The name an options file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "skip_modified_food",
            Self::HiddenWindows => "hidden_windows",
            // Retain the old settings key; an old false value is ignored
            // because this feature is now always on.
            Self::MapEverywhere => "map_where_off",
            Self::AskBeforeDeletingSpells => "ask_before_deleting_spells",
            Self::ShowTimesInChat => "chat_times",
            Self::FlashOnTells => "flash_on_tells",
            Self::TakeEnvironmentalDamage => "take_environmental_damage",
            Self::GillsForDays => "gills_for_days",
            Self::CorpsesAlongGround => "corpses_along_ground",
            Self::LongChaseCamera => "long_chase_camera",
            Self::FreeCamera => "free_camera",
            Self::ClearView => "clear_view",
            Self::WalkWhileWinded => "walk_while_winded",
            Self::RunBoost => "run_boost",
            Self::AutoLoot => "auto_loot",
            Self::FloatingCombatText => "floating_combat_text",
            Self::ReadWhileStanding => "read_while_standing",
            Self::MemorizeWhileStanding => "memorize_while_standing",
            Self::ShowInvisible => "show_invisible",
            Self::AnswerListPolls => "answer_list_polls",
            Self::Fly => "fly",
            Self::JesusMode => "jesus_mode",
            Self::ExitAtOnce => "exit_at_once",
            Self::FrameCap => "frame_cap",
            Self::GreyedControls => "greyed_controls",
            Self::CampCountdown => "camp_countdown",
            Self::QuietRepeats => "quiet_repeats",
            Self::HotbarItemGuard => "hotbar_item_guard",
            Self::BuffTimeLeft => "buff_time_left",
            Self::WeaponRatio => "weapon_ratio",
            Self::WindowsApart => "windows_apart",
        }
    }

    /// The fix whose file name this is.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|fix| fix.key() == key)
    }

    /// Its checkbox's words on the quality-of-life page and in the settings guide.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SkipModifiedFood => "Skip Food With Modifiers",
            Self::HiddenWindows => "Draw Windows the Skin Hides",
            Self::MapEverywhere => "Maps on Every Server",
            Self::AskBeforeDeletingSpells => "Ask Before Deleting Spells",
            Self::ShowTimesInChat => "Show Times in Chat",
            Self::FlashOnTells => "Flash on Tells",
            Self::TakeEnvironmentalDamage => "Take Environmental Damage",
            Self::GillsForDays => "Gills for days",
            Self::CorpsesAlongGround => "Corpses Lie Along the Ground",
            Self::LongChaseCamera => "Long Chase Camera",
            Self::FreeCamera => "Free Camera",
            Self::ClearView => "See the World Clearly",
            Self::WalkWhileWinded => "Walk While Winded",
            Self::RunBoost => "Run a Little Faster",
            Self::AutoLoot => "Loot Corpses Automatically",
            Self::FloatingCombatText => "Floating Combat Text",
            Self::ReadWhileStanding => "Read the Spellbook Standing",
            Self::MemorizeWhileStanding => "Memorize Without Sitting",
            Self::ShowInvisible => "Show Invisible Entities",
            Self::AnswerListPolls => "Answer List Presence Polls",
            Self::Fly => "Fly",
            Self::JesusMode => "Jesus Mode",
            Self::ExitAtOnce => "Exit at Once",
            Self::FrameCap => "Cap Frames at 60",
            Self::GreyedControls => "Grey Out What Is Unavailable",
            Self::CampCountdown => "Count Down Camping",
            Self::QuietRepeats => "Say Each Refusal Once",
            Self::HotbarItemGuard => "Keep Hotbar Items to Their Item",
            Self::BuffTimeLeft => "Show Buff Time Left",
            Self::WeaponRatio => "Show Weapon Ratio",
            Self::WindowsApart => "Open Windows Apart",
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
            Self::MapEverywhere => {
                "Show the zone map and all known entity positions on every server."
            }
            Self::AskBeforeDeletingSpells => {
                "Ask Yes or No before a spell is deleted from your spell book for good."
            }
            Self::ShowTimesInChat => {
                "Start each chat line with the time it arrived, by your computer's clock."
            }
            Self::FlashOnTells => {
                "Flash the client's taskbar button when a tell arrives while another window is in front."
            }
            Self::TakeEnvironmentalDamage => {
                "Report supported falls, drowning and lava heat. The air meter keeps running while damage reports are disabled."
            }
            Self::GillsForDays => {
                "Skip the local breath calculation and hide Air Remaining, so the client sends no drowning reports."
            }
            Self::CorpsesAlongGround => {
                "Lay each corpse along the slope of the ground under it, instead of level with its feet."
            }
            Self::LongChaseCamera => {
                "Let the mouse wheel zoom the chase camera out far past the 200 units the Titanium client allows. Turn it off to keep the Titanium client's limit."
            }
            Self::FreeCamera => {
                "Let the Toggle Free Camera key detach the camera from your character and fly it anywhere until the key returns it. Your character never moves. Turn it off to keep the camera on your character."
            }
            Self::ClearView => {
                "Light the night as if it were noon and hide distance fog, rain and snow in your view."
            }
            Self::WalkWhileWinded => {
                "Allow horizontal movement while stunned. Attacks, spells, items and jumping still obey stun restrictions."
            }
            Self::RunBoost => {
                "Increase forward running speed by 10 percent. Walking, strafing and backward movement are unchanged. Server acceptance is not guaranteed."
            }
            Self::AutoLoot => {
                "Open newly slain NPC corpses naming you as killer within 20 units and use Loot All. Leave NO DROP items for manual looting. Stop on refusal; supported item results are required."
            }
            Self::FloatingCombatText => {
                "Draw positive damage reported by the server above known targets. Chat remains available and no combat outcome is predicted."
            }
            Self::ReadWhileStanding => {
                "Read the spellbook without automatically sitting, and keep it open while standing. Scribing and memorization use their separate rules."
            }
            Self::MemorizeWhileStanding => {
                "Memorize without automatic sitting or the client preparation pause. Scribing keeps its usual posture and delay. Server validation still applies."
            }
            Self::ShowInvisible => {
                "Draw known invisible entities with parentheses around their names. This changes presentation only and does not permit targeting through invisibility."
            }
            Self::Fly => {
                "Fly using your Jump binding to ascend and Duck binding to descend. Movement remains bounded and collides with solid geometry. Turning this off restores gravity."
            }
            Self::JesusMode => {
                "Walk on mapped water and lava at your ordinary ground speed. Reach the surface from shore or above."
            }
            Self::ExitAtOnce => {
                "Request logout without automatic sitting or the camp countdown. Wait for the server response before leaving the session."
            }
            Self::AnswerListPolls => {
                "Answer supported P99 list presence polls after a local delay. This does not join a list or answer numeric challenges."
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
            Self::WindowsApart => "Open the windows you have not moved where none covers another.",
        }
    }

    /// What it changes.
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::SkipModifiedFood
            | Self::AskBeforeDeletingSpells
            | Self::TakeEnvironmentalDamage
            | Self::GillsForDays
            | Self::HotbarItemGuard => Kind::Guards,
            // Sends the official client would not make on its own: departures
            // the session takes only where the server type offers them, and
            // looting, replies and a logout nobody asked for at that moment.
            Self::AnswerListPolls
            | Self::MemorizeWhileStanding
            | Self::WalkWhileWinded
            | Self::RunBoost
            | Self::AutoLoot
            | Self::Fly
            | Self::JesusMode
            | Self::ExitAtOnce => Kind::Acts,
            Self::HiddenWindows
            | Self::MapEverywhere
            | Self::ShowTimesInChat
            | Self::FlashOnTells
            | Self::CorpsesAlongGround
            | Self::LongChaseCamera
            | Self::FreeCamera
            | Self::ClearView
            | Self::ReadWhileStanding
            | Self::ShowInvisible
            | Self::FloatingCombatText
            | Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::BuffTimeLeft
            | Self::WeaponRatio
            | Self::WindowsApart => Kind::Shows,
        }
    }

    /// What the session must offer for the fix to matter, if anything:
    /// eating on its own needs meal support, only a session that
    /// deletes spells has one to ask about, tells come with talking, and
    /// only a session that takes the world's damage from the client has any
    /// to hold back.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::SkipModifiedFood => Some(Capability::Meals),
            Self::AskBeforeDeletingSpells => Some(Capability::DeletingSpells),
            Self::FlashOnTells => Some(Capability::Talking),
            Self::TakeEnvironmentalDamage => Some(Capability::EnvironmentalDamage),
            // All current server policies offer these opt-ins for testing.
            // The controls can still reflect a policy that withholds them.
            Self::WalkWhileWinded
            | Self::RunBoost
            | Self::Fly
            | Self::JesusMode
            | Self::GillsForDays
            | Self::MemorizeWhileStanding => Some(Capability::GameplayDepartures),
            // A server with no looting has no corpse to open by itself.
            Self::AutoLoot => Some(Capability::Looting),
            Self::AnswerListPolls => Some(Capability::AnsweringListPolls),
            Self::ExitAtOnce => Some(Capability::Camping),
            Self::ReadWhileStanding => Some(Capability::Spellbook),
            Self::HiddenWindows
            | Self::MapEverywhere
            | Self::ShowTimesInChat
            | Self::CorpsesAlongGround
            | Self::LongChaseCamera
            | Self::FreeCamera
            | Self::ClearView
            | Self::ShowInvisible
            | Self::FloatingCombatText
            | Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::HotbarItemGuard
            | Self::BuffTimeLeft
            | Self::WeaponRatio
            | Self::WindowsApart => None,
        }
    }

    /// Whether it is always on or the player's choice: windows the skin
    /// hides stay hidden unless the player asks for them;
    /// chat lines read as the official client's do until the player asks
    /// for their times; the world's damage is taken until the player turns
    /// it off, as Adam chose; corpses lie along the ground until the player
    /// turns it off, as Adam chose; the chase camera goes far until the
    /// player turns it off, as Adam asked; windows open apart, as Adam asked; and what
    /// only adds what the player could work out, or stops a plain mistake,
    /// is always on.
    #[must_use]
    pub const fn availability(self) -> Availability {
        match self {
            Self::SkipModifiedFood
            | Self::AskBeforeDeletingSpells
            | Self::FlashOnTells
            | Self::TakeEnvironmentalDamage
            | Self::CorpsesAlongGround
            | Self::LongChaseCamera
            | Self::FreeCamera => Availability::Setting { default: true },
            // A comfort the player has to ask for starts as the official
            // client behaves: off.
            Self::HiddenWindows
            | Self::ShowTimesInChat
            | Self::ClearView
            | Self::WalkWhileWinded
            | Self::RunBoost
            | Self::AutoLoot
            | Self::ReadWhileStanding
            | Self::MemorizeWhileStanding
            | Self::ShowInvisible
            | Self::AnswerListPolls
            | Self::GillsForDays
            | Self::Fly
            | Self::JesusMode
            | Self::ExitAtOnce
            | Self::FloatingCombatText => Availability::Setting { default: false },
            Self::MapEverywhere
            | Self::FrameCap
            | Self::GreyedControls
            | Self::CampCountdown
            | Self::QuietRepeats
            | Self::HotbarItemGuard
            | Self::BuffTimeLeft
            | Self::WeaponRatio
            | Self::WindowsApart => Availability::AlwaysOn,
        }
    }

    /// Whether the player can turn it off on the quality-of-life page.
    #[must_use]
    pub const fn is_setting(self) -> bool {
        matches!(self.availability(), Availability::Setting { .. })
    }

    /// Whether this choice can raise an eyebrow, at the server or among the
    /// neighbours: what it withholds, reports, or does faster than the
    /// official client would. The page keeps these together under their own
    /// caption at its end, and the names stay clean.
    #[must_use]
    pub const fn is_unsafe(self) -> bool {
        matches!(
            self,
            Self::TakeEnvironmentalDamage
                | Self::GillsForDays
                | Self::RunBoost
                | Self::Fly
                | Self::JesusMode
                | Self::ExitAtOnce
        )
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
    fn the_settings_guide_names_every_fix() {
        let guide = include_str!("../../../docs/settings.md");
        for fix in Fix::ALL {
            assert!(
                guide.contains(&format!("| {} |", fix.label())),
                "the settings guide's Quality of life section leaves out {fix:?}"
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
                Fix::AskBeforeDeletingSpells,
                Fix::ShowTimesInChat,
                Fix::FlashOnTells,
                Fix::CorpsesAlongGround,
                Fix::LongChaseCamera,
                Fix::FreeCamera,
                Fix::ClearView,
                Fix::WalkWhileWinded,
                Fix::AutoLoot,
                Fix::FloatingCombatText,
                Fix::ReadWhileStanding,
                Fix::MemorizeWhileStanding,
                Fix::ShowInvisible,
                Fix::AnswerListPolls,
                Fix::TakeEnvironmentalDamage,
                Fix::GillsForDays,
                Fix::RunBoost,
                Fix::Fly,
                Fix::JesusMode,
                Fix::ExitAtOnce,
            ]
        );
        // A fix that is always on stays on whatever is asked.
        for fix in Fix::ALL.into_iter().filter(|fix| !fix.is_setting()) {
            settings.set(fix, false);
            assert!(settings.on(fix), "{fix:?} turned off");
        }
    }

    #[test]
    fn gameplay_departures_and_local_comforts_require_opt_in() {
        let settings = Settings::default();
        for fix in [
            Fix::ClearView,
            Fix::WalkWhileWinded,
            Fix::RunBoost,
            Fix::AutoLoot,
            Fix::FloatingCombatText,
            Fix::ReadWhileStanding,
            Fix::MemorizeWhileStanding,
            Fix::ShowInvisible,
            Fix::AnswerListPolls,
            Fix::GillsForDays,
            Fix::Fly,
            Fix::JesusMode,
            Fix::ExitAtOnce,
        ] {
            assert!(!settings.on(fix), "{fix:?}");
        }
        assert_eq!(Fix::JesusMode.needs(), Some(Capability::GameplayDepartures));
        // Skipping the air counter withholds drowning reports a session that
        // takes them expects, so it is a departure like the others.
        assert_eq!(
            Fix::GillsForDays.needs(),
            Some(Capability::GameplayDepartures)
        );
        assert!(settings.on(Fix::TakeEnvironmentalDamage));
    }

    #[test]
    fn the_worlds_damage_is_taken_until_the_player_turns_it_off() {
        let mut settings = Settings::default();
        assert!(settings.on(Fix::TakeEnvironmentalDamage));
        // Greyed where the session takes none from the client.
        assert_eq!(
            Fix::TakeEnvironmentalDamage.needs(),
            Some(Capability::EnvironmentalDamage)
        );
        assert_eq!(Fix::TakeEnvironmentalDamage.kind(), Kind::Guards);
        // The choices that can raise an eyebrow sit together at the page's end
        // under their caption; the flag lives in the predicate, never in a name.
        assert_eq!(
            Fix::settings()
                .filter(|fix| fix.is_unsafe())
                .collect::<Vec<_>>(),
            [
                Fix::TakeEnvironmentalDamage,
                Fix::GillsForDays,
                Fix::RunBoost,
                Fix::Fly,
                Fix::JesusMode,
                Fix::ExitAtOnce
            ]
        );
        assert!(!Fix::ALL.iter().any(|fix| fix.label().contains("unsafe")));
        settings.set(Fix::TakeEnvironmentalDamage, false);
        assert!(!settings.on(Fix::TakeEnvironmentalDamage));
    }

    #[test]
    fn maps_are_local_and_always_on_even_with_an_old_disabled_setting() {
        let mut settings = Settings::default();
        settings.set(Fix::MapEverywhere, false);
        assert!(settings.on(Fix::MapEverywhere));
        assert_eq!(Fix::MapEverywhere.needs(), None);
    }
}

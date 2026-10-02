//! The player's options: those of the official Options window this client
//! keeps itself, and the ones only this client has. A file keeps them as one
//! `name = value` line each, so a file from an older client still reads.

use crate::food::AutoEat;

/// The first line of an options file.
pub const HEADER: &str = "# eq-client options v1";

/// An option the player turns on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Toggle {
    /// Opens the pet's window when the player gets a pet (the General page's
    /// Pet Window Popup).
    PetWindowPopup,
    /// Draws a ring at the target's feet (the Display page's Show 3D Target
    /// Ring).
    TargetRing,
    /// Shows the player's own helm (the Display page's Show My Helm). As on
    /// P99, it changes only how the player sees themselves: other players'
    /// helms always show, and the server hears nothing of it.
    ShowHelm,
    /// Draws players' names over their heads (the Display page's Show PC
    /// Names).
    PcNames,
    /// Draws the names of creatures, merchants and every other non-player
    /// over their heads (the Display page's Show NPC Names).
    NpcNames,
    /// Turns the camera the other way when the mouse moves up or down (the
    /// Mouse page's Invert Y Axis).
    InvertY,
    /// Zooms the camera with the mouse wheel (the Mouse page's Mouse Wheel
    /// Zoom).
    WheelZoom,
    /// Leaves food and drink with modifiers for the player to eat or drink
    /// by hand; only this client has it.
    SkipModifiedFood,
    /// Logs the chat, as the official client's `/log` does; it keeps the
    /// choice in `eqclient.ini`, this client per character.
    Log,
}

impl Toggle {
    /// Every toggle, in the order a file lists them.
    pub const ALL: [Self; 9] = [
        Self::PetWindowPopup,
        Self::TargetRing,
        Self::ShowHelm,
        Self::PcNames,
        Self::NpcNames,
        Self::InvertY,
        Self::WheelZoom,
        Self::SkipModifiedFood,
        Self::Log,
    ];

    /// The name a file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PetWindowPopup => "pet_window_popup",
            Self::TargetRing => "target_ring",
            Self::ShowHelm => "show_helm",
            Self::PcNames => "pc_names",
            Self::NpcNames => "npc_names",
            Self::InvertY => "invert_y",
            Self::WheelZoom => "wheel_zoom",
            Self::SkipModifiedFood => "skip_modified_food",
            Self::Log => "log",
        }
    }
}

/// Every option the client keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one of the Options window's checkboxes"
)]
pub struct Options {
    /// See [`Toggle::PetWindowPopup`].
    pub pet_window_popup: bool,
    /// See [`Toggle::TargetRing`].
    pub target_ring: bool,
    /// See [`Toggle::ShowHelm`].
    pub show_helm: bool,
    /// See [`Toggle::PcNames`].
    pub pc_names: bool,
    /// See [`Toggle::NpcNames`].
    pub npc_names: bool,
    /// See [`Toggle::InvertY`].
    pub invert_y: bool,
    /// See [`Toggle::WheelZoom`].
    pub wheel_zoom: bool,
    /// See [`Toggle::SkipModifiedFood`].
    pub skip_modified_food: bool,
    /// See [`Toggle::Log`].
    pub log: bool,
}

impl Default for Options {
    /// The client's defaults: the pet's window pops up, the target ring, the
    /// player's helm and everyone's names show, the mouse is not inverted and its wheel
    /// zooms, food with modifiers waits for the player, and the chat is
    /// logged.
    fn default() -> Self {
        Self {
            pet_window_popup: true,
            target_ring: true,
            show_helm: true,
            pc_names: true,
            npc_names: true,
            invert_y: false,
            wheel_zoom: true,
            skip_modified_food: true,
            log: true,
        }
    }
}

impl Options {
    /// Whether the toggle is on.
    #[must_use]
    pub const fn get(&self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::PetWindowPopup => self.pet_window_popup,
            Toggle::TargetRing => self.target_ring,
            Toggle::ShowHelm => self.show_helm,
            Toggle::PcNames => self.pc_names,
            Toggle::NpcNames => self.npc_names,
            Toggle::InvertY => self.invert_y,
            Toggle::WheelZoom => self.wheel_zoom,
            Toggle::SkipModifiedFood => self.skip_modified_food,
            Toggle::Log => self.log,
        }
    }

    /// Turns the toggle on or off.
    pub const fn set(&mut self, toggle: Toggle, on: bool) {
        let option = match toggle {
            Toggle::PetWindowPopup => &mut self.pet_window_popup,
            Toggle::TargetRing => &mut self.target_ring,
            Toggle::ShowHelm => &mut self.show_helm,
            Toggle::PcNames => &mut self.pc_names,
            Toggle::NpcNames => &mut self.npc_names,
            Toggle::InvertY => &mut self.invert_y,
            Toggle::WheelZoom => &mut self.wheel_zoom,
            Toggle::SkipModifiedFood => &mut self.skip_modified_food,
            Toggle::Log => &mut self.log,
        };
        *option = on;
    }

    /// What the session may eat and drink on its own.
    #[must_use]
    pub const fn auto_eat(&self) -> AutoEat {
        if self.skip_modified_food {
            AutoEat::Plain
        } else {
            AutoEat::Anything
        }
    }

    /// The options a file's text holds, over these defaults: a line it cannot
    /// read, or an option it does not name, keeps the default.
    #[must_use]
    pub fn read(text: &str, defaults: Self) -> Self {
        let mut options = defaults;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let on = match value.trim() {
                "true" => true,
                "false" => false,
                _ => continue,
            };
            if let Some(toggle) = Toggle::ALL
                .into_iter()
                .find(|toggle| toggle.key() == key.trim())
            {
                options.set(toggle, on);
            }
        }
        options
    }

    /// The text a file keeps: the header, then every option.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        for toggle in Toggle::ALL {
            text.push_str(toggle.key());
            text.push_str(if self.get(toggle) {
                " = true\n"
            } else {
                " = false\n"
            });
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_keep_to_their_file_and_old_files_still_read() {
        let mut options = Options::default();
        options.set(Toggle::InvertY, true);
        options.set(Toggle::SkipModifiedFood, false);
        let text = options.text();
        assert!(text.starts_with(HEADER));
        assert!(text.contains("invert_y = true\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // Unknown names, bad values and missing options keep the defaults.
        let defaults = Options {
            pet_window_popup: false,
            ..Options::default()
        };
        let old = "# eq-client options v1\nwheel_zoom = false\nshiny = true\ntarget_ring = maybe\n";
        let read = Options::read(old, defaults);
        assert!(!read.wheel_zoom);
        assert!(read.target_ring);
        assert!(!read.pet_window_popup);
    }

    #[test]
    fn skipping_modified_food_is_what_the_session_eats() {
        let mut options = Options::default();
        assert_eq!(options.auto_eat(), AutoEat::Plain);
        options.set(Toggle::SkipModifiedFood, false);
        assert_eq!(options.auto_eat(), AutoEat::Anything);
        assert!(!options.get(Toggle::SkipModifiedFood));
    }
}

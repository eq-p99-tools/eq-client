//! The player's options: those of the official Options window this client
//! keeps itself, and its quality-of-life settings ([`crate::qol`]). A file
//! keeps them as one `name = value` line each, so a file from an older
//! client still reads.

use crate::{
    Capability,
    food::AutoEat,
    names::ShowNames,
    qol::{self, Fix},
};

/// The first line of an options file.
pub const HEADER: &str = "# eq-client options v1";

/// The name a file keeps [`Options::show_names`] under.
const SHOW_NAMES: &str = "show_names";

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
    /// Logs the chat, as the official client's `/log` does; it keeps the
    /// choice in `eqclient.ini`, this client per character.
    Log,
    /// A quality-of-life setting, on the Options window's quality-of-life
    /// page, which only this client has.
    Qol(Fix),
}

impl Toggle {
    /// The official client's own toggles, in the order a file lists them.
    pub const OFFICIAL: [Self; 8] = [
        Self::PetWindowPopup,
        Self::TargetRing,
        Self::ShowHelm,
        Self::PcNames,
        Self::NpcNames,
        Self::InvertY,
        Self::WheelZoom,
        Self::Log,
    ];

    /// Every toggle, in the order a file lists them: the official client's,
    /// then each quality-of-life setting.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::OFFICIAL
            .into_iter()
            .chain(Fix::settings().map(Self::Qol))
    }

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
            Self::Log => "log",
            Self::Qol(fix) => fix.key(),
        }
    }

    /// What the session must offer for the toggle to matter, if anything; a
    /// checkbox for one it does not offer is greyed out.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::PetWindowPopup
            | Self::TargetRing
            | Self::ShowHelm
            | Self::PcNames
            | Self::NpcNames
            | Self::InvertY
            | Self::WheelZoom
            | Self::Log => None,
            Self::Qol(fix) => fix.needs(),
        }
    }
}

/// An option the player sets along one of the Options window's sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// How far away spawns are drawn, as a percentage of the farthest the
    /// zone and the client allow (the Display page's Far Clip Plane).
    ClipPlane,
    /// The most frames drawn each second, or 0 to leave the rate to the
    /// display (the Display page's Max. Frames Per Second).
    MaxFps,
    /// How fast the camera turns as the mouse drags it, as a percentage
    /// where 50 is the client's own speed (the Mouse page's Mouselook
    /// Sensitivity).
    MouseSensitivity,
}

/// The fewest frames a second the Max FPS slider offers.
const MIN_FPS: u16 = 10;
/// The most frames a second the Max FPS slider offers below its far right,
/// which leaves the rate to the display.
const MAX_FPS: u16 = 200;
/// The steps between the Max FPS slider's rates.
const FPS_STEP: usize = 5;

impl Level {
    /// Every level, in the order a file lists them.
    pub const ALL: [Self; 3] = [Self::ClipPlane, Self::MaxFps, Self::MouseSensitivity];

    /// The name a file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ClipPlane => "clip_plane",
            Self::MaxFps => "max_fps",
            Self::MouseSensitivity => "mouse_sensitivity",
        }
    }

    /// The values its slider stops at, from its left.
    fn stops(self) -> Vec<u16> {
        match self {
            Self::ClipPlane | Self::MouseSensitivity => (0..=100).collect(),
            Self::MaxFps => (MIN_FPS..=MAX_FPS)
                .step_by(FPS_STEP)
                .chain(std::iter::once(0))
                .collect(),
        }
    }

    /// Where along its slider a value sits, from 0 at its left to 1 at its
    /// right.
    #[must_use]
    pub fn fraction(self, value: u16) -> f32 {
        match self {
            Self::ClipPlane | Self::MouseSensitivity => f32::from(value.min(100)) / 100.0,
            Self::MaxFps if value == 0 => 1.0,
            Self::MaxFps => {
                let span = f32::from(MAX_FPS - MIN_FPS);
                // The far right is the display's rate, so a rate the slider
                // does not offer stops just short of it.
                (f32::from(value.clamp(MIN_FPS, MAX_FPS) - MIN_FPS) / span) * 0.975
            }
        }
    }

    /// The value at a point along its slider, from 0 at its left to 1 at
    /// its right: the stop nearest it.
    #[must_use]
    pub fn at(self, fraction: f32) -> u16 {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.stops()
            .into_iter()
            .min_by(|a, b| {
                (self.fraction(*a) - fraction)
                    .abs()
                    .total_cmp(&(self.fraction(*b) - fraction).abs())
            })
            .unwrap_or_default()
    }

    /// The value as the slider's label shows it.
    #[must_use]
    pub fn words(self, value: u16) -> String {
        match self {
            Self::ClipPlane | Self::MouseSensitivity => format!("{value} %"),
            Self::MaxFps if value == 0 => "Display".to_owned(),
            Self::MaxFps => value.to_string(),
        }
    }

    /// Whether a file's value is one the option takes.
    fn takes(self, value: u16) -> bool {
        match self {
            Self::ClipPlane | Self::MouseSensitivity => value <= 100,
            Self::MaxFps => value == 0 || (1..=1000).contains(&value),
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
    /// See [`Toggle::Log`].
    pub log: bool,
    /// Which quality-of-life fixes are on ([`Toggle::Qol`]).
    pub qol: qol::Settings,
    /// How much of a player's name shows over their head, as `/shownames`
    /// sets it; the official client keeps it in `eqclient.ini`, this client
    /// per character.
    pub show_names: ShowNames,
    /// See [`Level::ClipPlane`].
    pub clip_plane: u16,
    /// See [`Level::MaxFps`].
    pub max_fps: u16,
    /// See [`Level::MouseSensitivity`].
    pub mouse_sensitivity: u16,
}

impl Default for Options {
    /// The client's defaults: the pet's window pops up, the target ring, the
    /// player's helm and everyone's names show, the mouse is not inverted and its wheel
    /// zooms, the chat is logged, each quality-of-life fix starts as it is
    /// defined, players' names show in full, spawns draw as far as the zone
    /// allows, at most 60 frames a second, and the camera turns at the
    /// client's own speed.
    fn default() -> Self {
        Self {
            pet_window_popup: true,
            target_ring: true,
            show_helm: true,
            pc_names: true,
            npc_names: true,
            invert_y: false,
            wheel_zoom: true,
            log: true,
            qol: qol::Settings::default(),
            show_names: ShowNames::Everything,
            clip_plane: 100,
            max_fps: 60,
            mouse_sensitivity: 50,
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
            Toggle::Log => self.log,
            Toggle::Qol(fix) => self.qol.on(fix),
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
            Toggle::Log => &mut self.log,
            Toggle::Qol(fix) => {
                self.qol.set(fix, on);
                return;
            }
        };
        *option = on;
    }

    /// A slider's value.
    #[must_use]
    pub const fn level(&self, level: Level) -> u16 {
        match level {
            Level::ClipPlane => self.clip_plane,
            Level::MaxFps => self.max_fps,
            Level::MouseSensitivity => self.mouse_sensitivity,
        }
    }

    /// Sets a slider's value.
    pub const fn set_level(&mut self, level: Level, value: u16) {
        let option = match level {
            Level::ClipPlane => &mut self.clip_plane,
            Level::MaxFps => &mut self.max_fps,
            Level::MouseSensitivity => &mut self.mouse_sensitivity,
        };
        *option = value;
    }

    /// The share of the farthest drawing distance the scene is drawn within:
    /// the clip plane, never below a twentieth.
    #[must_use]
    pub fn clip_share(&self) -> f32 {
        (f32::from(self.clip_plane.min(100)) / 100.0).max(0.05)
    }

    /// How far the scene is drawn, terrain, objects and spawns alike, in a
    /// zone with this far clip: the clip plane's share of it, but never
    /// nearer than 100 units. Provisional: the official slider's mapping is
    /// unverified. None where the zone gives no far clip.
    #[must_use]
    pub fn clip_distance(&self, far_clip: Option<f32>) -> Option<f32> {
        let far_clip = far_clip.filter(|far| far.is_finite() && *far > 0.0)?;
        Some((far_clip * self.clip_share()).max(far_clip.min(100.0)))
    }

    /// The most frames a second, if the client holds the rate down at all.
    #[must_use]
    pub fn frame_cap(&self) -> Option<u32> {
        (self.max_fps > 0).then(|| u32::from(self.max_fps))
    }

    /// How far the camera turns, in radians, for each pixel the mouse drags
    /// it: the client's own 0.005 at a sensitivity of 50, doubling with
    /// every 25 above and halving with every 25 below.
    #[must_use]
    pub fn turn_per_pixel(&self) -> f32 {
        0.005 * ((f32::from(self.mouse_sensitivity.min(100)) - 50.0) / 25.0).exp2()
    }

    /// What the session may eat and drink on its own.
    #[must_use]
    pub const fn auto_eat(&self) -> AutoEat {
        if self.qol.on(Fix::SkipModifiedFood) {
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
            if key.trim() == SHOW_NAMES {
                if let Some(level) = ShowNames::parse(value) {
                    options.show_names = level;
                }
                continue;
            }
            if let Some(level) = Level::ALL
                .into_iter()
                .find(|level| level.key() == key.trim())
            {
                if let Some(number) = value.trim().parse().ok().filter(|n| level.takes(*n)) {
                    options.set_level(level, number);
                }
                continue;
            }
            let on = match value.trim() {
                "true" => true,
                "false" => false,
                _ => continue,
            };
            if let Some(toggle) = Toggle::all().find(|toggle| toggle.key() == key.trim()) {
                options.set(toggle, on);
            }
        }
        options
    }

    /// The text a file keeps: the header, then every option.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = format!("{HEADER}\n");
        for toggle in Toggle::all() {
            text.push_str(toggle.key());
            text.push_str(if self.get(toggle) {
                " = true\n"
            } else {
                " = false\n"
            });
        }
        for part in [SHOW_NAMES, " = ", self.show_names.word(), "\n"] {
            text.push_str(part);
        }
        for level in Level::ALL {
            for part in [level.key(), " = ", &self.level(level).to_string(), "\n"] {
                text.push_str(part);
            }
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
        options.set(Toggle::Qol(Fix::SkipModifiedFood), false);
        options.show_names = ShowNames::Last;
        let text = options.text();
        assert!(text.starts_with(HEADER));
        assert!(text.contains("invert_y = true\n"));
        assert!(text.contains("skip_modified_food = false\n"));
        assert!(text.contains("hidden_windows = false\n"));
        assert!(text.contains("show_names = 2\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // A file from before the QoL page keeps its choices there.
        let before = Options::read(
            "# eq-client options v1\nskip_modified_food = false\nhidden_windows = true\n",
            Options::default(),
        );
        assert!(!before.qol.on(Fix::SkipModifiedFood));
        assert!(before.get(Toggle::Qol(Fix::HiddenWindows)));
        // Unknown names, bad values and missing options keep the defaults.
        let defaults = Options {
            pet_window_popup: false,
            ..Options::default()
        };
        let old = "# eq-client options v1\nwheel_zoom = false\nshiny = true\ntarget_ring = maybe\nshow_names = 9\n";
        let read = Options::read(old, defaults);
        assert!(!read.wheel_zoom);
        assert!(read.target_ring);
        assert!(!read.pet_window_popup);
        assert_eq!(read.show_names, ShowNames::Everything);
        let off = Options::read("show_names = off\n", defaults);
        assert_eq!(off.show_names, ShowNames::Off);
    }

    #[test]
    fn sliders_keep_their_values_and_stop_where_their_labels_say() {
        let mut options = Options::default();
        options.set_level(Level::ClipPlane, 40);
        options.set_level(Level::MaxFps, 0);
        let text = options.text();
        assert!(text.contains("clip_plane = 40\n") && text.contains("max_fps = 0\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // Values a slider cannot hold keep the default.
        let read = Options::read(
            "clip_plane = 300\nmouse_sensitivity = x\n",
            Options::default(),
        );
        assert_eq!((read.clip_plane, read.mouse_sensitivity), (100, 50));
        // The far right of Max FPS leaves the rate to the display.
        assert_eq!(Level::MaxFps.at(1.0), 0);
        assert_eq!(Level::MaxFps.at(0.0), 10);
        assert_eq!(Level::MaxFps.words(0), "Display");
        assert_eq!(Level::MaxFps.at(Level::MaxFps.fraction(60)), 60);
        assert_eq!(Level::ClipPlane.at(0.404), 40);
        assert_eq!(Level::ClipPlane.words(40), "40 %");
        assert_eq!(Options::default().frame_cap(), Some(60));
        assert_eq!(options.frame_cap(), None);
        // Sensitivity 50 is the client's own speed; 75 doubles it.
        let mut quick = Options::default();
        assert!((quick.turn_per_pixel() - 0.005).abs() < 1e-6);
        quick.set_level(Level::MouseSensitivity, 75);
        assert!((quick.turn_per_pixel() - 0.01).abs() < 1e-6);
        // The scene always draws within a twentieth of the distance, and no
        // nearer than 100 units.
        options.set_level(Level::ClipPlane, 0);
        assert!((options.clip_share() - 0.05).abs() < f32::EPSILON);
        assert_eq!(options.clip_distance(Some(2400.0)), Some(120.0));
        assert_eq!(options.clip_distance(Some(1000.0)), Some(100.0));
        assert_eq!(options.clip_distance(Some(80.0)), Some(80.0));
        options.set_level(Level::ClipPlane, 50);
        assert_eq!(options.clip_distance(Some(2400.0)), Some(1200.0));
        assert_eq!(options.clip_distance(None), None);
    }

    #[test]
    fn skipping_modified_food_is_what_the_session_eats() {
        let mut options = Options::default();
        assert_eq!(options.auto_eat(), AutoEat::Plain);
        options.set(Toggle::Qol(Fix::SkipModifiedFood), false);
        assert_eq!(options.auto_eat(), AutoEat::Anything);
        assert!(!options.get(Toggle::Qol(Fix::SkipModifiedFood)));
    }

    #[test]
    fn every_option_has_a_name_of_its_own() {
        // Every fix's name counts, a fix that is always on too, so it can
        // become a setting without taking another option's name.
        let mut keys: Vec<_> = Toggle::OFFICIAL
            .iter()
            .map(|toggle| toggle.key())
            .chain(Fix::ALL.iter().map(|fix| fix.key()))
            .chain(Level::ALL.iter().map(|level| level.key()))
            .chain([SHOW_NAMES])
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count);
        // Only a quality-of-life setting can need what the session offers.
        assert!(
            Toggle::OFFICIAL
                .iter()
                .all(|toggle| toggle.needs().is_none())
        );
        assert_eq!(
            Toggle::Qol(Fix::SkipModifiedFood).needs(),
            Some(Capability::Inventory)
        );
        assert_eq!(
            Toggle::all().count(),
            Toggle::OFFICIAL.len() + Fix::settings().count()
        );
    }
}

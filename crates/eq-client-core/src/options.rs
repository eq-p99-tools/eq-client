//! The player's options: those of the official Options window this client
//! keeps itself, and its quality-of-life settings ([`crate::qol`]). A file
//! keeps them as one `name = value` line each, so a file from an older
//! client still reads.

use crate::{
    Capability,
    food::AutoEat,
    movement::RunSpeedMultiplier,
    names::ShowNames,
    no_drop::LootNoDrop,
    player_models::LuclinModels,
    qol::{self, Fix},
};

/// The first line of an options file.
pub const HEADER: &str = "# eq-client options v1";

/// The name a file keeps [`Options::show_names`] under.
const SHOW_NAMES: &str = "show_names";

/// The name a file keeps [`Options::loot_no_drop`] under.
const LOOT_NO_DROP: &str = "loot_no_drop";
/// The name a file keeps [`Options::luclin_models`] under.
const LUCLIN_MODELS: &str = "luclin_models";

/// The name a file keeps [`Options::attack_on_assist`] under.
const ATTACK_ON_ASSIST: &str = "attack_on_assist";

/// An option the player turns on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Toggle {
    /// Opens the pet's window when the player gets a pet (the General page's
    /// Pet Window Popup).
    PetWindowPopup,
    /// Stands a sitting player up when they move forward (the General page's
    /// Allow Auto-Stand, under Allow Auto-Duck). The official client keeps it
    /// as `AllowAutoStand` in `eqclient.ini`, on unless set off (Titanium
    /// 0x4af32c, 0x4ae721); this client per character. Off, a sitting player
    /// stays put until they stand.
    AutoStand,
    /// Draws a ring at the target's feet (the Display page's Show 3D Target
    /// Ring).
    TargetRing,
    /// Shows the player's own helm (the Display page's Show My Helm). Where
    /// the session sends it on (`EQEmu`, from the Titanium program) the server
    /// hears each change; elsewhere, as on P99, it changes only how the
    /// player sees themselves. Other players' helms always show.
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
    /// Brings the first-person view back level when no look key or mouse
    /// look holds it. The official client keeps it as `LookSpring` in
    /// `[Options]` of `eqclient.ini`, off unless set on (Titanium 0x4af32c,
    /// read R13); this client per character.
    LookSpring,
    /// Logs the chat, as the official client's `/log` does; it keeps the
    /// choice in `eqclient.ini`, this client per character.
    Log,
    /// Plays sounds; the official client keeps the choice as `Sound` in
    /// `eqclient.ini`, this client per character.
    Sound,
    /// Plays the zone's music. The official client keeps its music volume
    /// as `Music` in `eqclient.ini` (read R49-7 c), a 0 there seeding this
    /// off [I: the scale is unread]; this client keeps it per character, on
    /// by default.
    Music,
    /// Plays the zone's ambient sounds, its emitters. The official client
    /// keeps `EnvSounds` in `eqclient.ini` (read R49-7 c), taken as a
    /// TRUE or FALSE [I: its reader is unread]; this client keeps it per
    /// character, on by default.
    EnvironmentSounds,
    /// Plays combat music, as the official client's `CombatMusic` in
    /// `eqclient.ini` (TRUE when missing) and its `/combatmusic` (read R49-7
    /// c; the command's forms R49-22c, 0x4689a2). This client keeps it per
    /// character; it plays no combat music yet (row 6.525a).
    CombatMusic,
    /// Plays the installation's spell-effect particles, as the official
    /// client's does; the official client keeps the choice as
    /// `ShowSpellEffects` in `eqclient.ini` (and as a zero
    /// `SpellParticleDensity`), this client per character. The Options
    /// window's Particles page is its official home.
    SpellEffects,
    /// A quality-of-life setting, on the Options window's quality-of-life
    /// page, which only this client has.
    Qol(Fix),
}

impl Toggle {
    /// The official client's own toggles, in the order a file lists them; a
    /// file is read back by name, so the order is only how it is written.
    pub const OFFICIAL: [Self; 15] = [
        Self::PetWindowPopup,
        Self::AutoStand,
        Self::TargetRing,
        Self::ShowHelm,
        Self::PcNames,
        Self::NpcNames,
        Self::InvertY,
        Self::WheelZoom,
        Self::LookSpring,
        Self::Log,
        Self::Sound,
        // Music and Environment Sounds are listed as the official client's
        // own, but their official names, form and defaults are unread
        // [inferred, item 6.503, pending R49-7 c].
        Self::Music,
        Self::EnvironmentSounds,
        Self::CombatMusic,
        Self::SpellEffects,
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
            Self::AutoStand => "allow_auto_stand",
            Self::TargetRing => "target_ring",
            Self::ShowHelm => "show_helm",
            Self::PcNames => "pc_names",
            Self::NpcNames => "npc_names",
            Self::InvertY => "invert_y",
            Self::WheelZoom => "wheel_zoom",
            Self::LookSpring => "look_spring",
            Self::Log => "log",
            Self::Sound => "sound",
            Self::Music => "music",
            Self::EnvironmentSounds => "environment_sounds",
            Self::CombatMusic => "combat_music",
            Self::SpellEffects => "spell_effects",
            Self::Qol(fix) => fix.key(),
        }
    }

    /// What the session must offer for the toggle to matter, if anything; a
    /// checkbox for one it does not offer is greyed out.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::PetWindowPopup
            | Self::AutoStand
            | Self::TargetRing
            | Self::ShowHelm
            | Self::PcNames
            | Self::NpcNames
            | Self::InvertY
            | Self::WheelZoom
            | Self::LookSpring
            | Self::Log
            | Self::Sound
            | Self::Music
            | Self::EnvironmentSounds
            | Self::CombatMusic
            | Self::SpellEffects => None,
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
    /// How fast the camera turns as the mouse drags it, from 0 to 100
    /// across the program's sensitivity 1 to 8 (the Mouse page's Mouselook
    /// Sensitivity; see [`Options::turn_per_pixel`]).
    MouseSensitivity,
    /// How large this client's windows are drawn, as a percentage of the
    /// desktop's scaling (the Interface scale on the quality-of-life page); the installation's
    /// own full-window screens keep the official client's size.
    UiScale,
    /// Forward running speed in hundredths of the calibrated speed.
    RunSpeed,
    /// How loud sounds play, as a percentage of their files' own loudness.
    /// The official client keeps a `SoundVolume` in `eqclient.ini` on a
    /// scale not yet known, so this client's own starts at 100 (inferred, item
    /// 6.259).
    SoundVolume,
    /// How loud the zone's music plays, as a percentage of the synthesizer's
    /// own loudness (the General page's music volume slider). The official
    /// client's key, scale and default are pending read R49-7 c (recording
    /// checklist 6.503); this client's own starts at 100.
    MusicVolume,
}

/// The fewest frames a second the Max FPS slider offers.
const MIN_FPS: u16 = 10;
/// The most frames a second the Max FPS slider offers below its far right,
/// which leaves the rate to the display.
const MAX_FPS: u16 = 200;
/// The steps between the Max FPS slider's rates.
const FPS_STEP: usize = 5;
/// The smallest and largest interface scales, in percent, and the steps
/// between them.
const MIN_UI_SCALE: u16 = 50;
const MAX_UI_SCALE: u16 = 200;
const UI_SCALE_STEP: usize = 5;
const MIN_RUN_SPEED: u16 = RunSpeedMultiplier::NORMAL.percent();
const MAX_RUN_SPEED: u16 = RunSpeedMultiplier::MAX.percent();

impl Level {
    /// Every level, in the order a file lists them.
    pub const ALL: [Self; 7] = [
        Self::ClipPlane,
        Self::MaxFps,
        Self::MouseSensitivity,
        Self::UiScale,
        Self::RunSpeed,
        Self::SoundVolume,
        Self::MusicVolume,
    ];

    /// The name a file keeps it under.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ClipPlane => "clip_plane",
            Self::MaxFps => "max_fps",
            Self::MouseSensitivity => "mouse_sensitivity",
            Self::UiScale => "ui_scale",
            Self::RunSpeed => "run_boost",
            Self::SoundVolume => "sound_volume",
            Self::MusicVolume => "music_volume",
        }
    }

    /// The session capability required to use the slider, if any.
    #[must_use]
    pub const fn needs(self) -> Option<Capability> {
        match self {
            Self::RunSpeed => Some(Capability::GameplayDepartures),
            _ => None,
        }
    }

    /// Whether setting it lays the interface out again at another size, so
    /// a window and its slider move under a pointer that holds still: its
    /// slider keeps what a drag chooses until the press ends rather than
    /// feeding each frame's choice back into the layout it reads.
    #[must_use]
    pub const fn rescales_layout(self) -> bool {
        matches!(self, Self::UiScale)
    }

    /// The values its slider stops at, from its left.
    fn stops(self) -> Vec<u16> {
        match self {
            Self::ClipPlane | Self::MouseSensitivity | Self::SoundVolume | Self::MusicVolume => {
                (0..=100).collect()
            }
            Self::MaxFps => (MIN_FPS..=MAX_FPS)
                .step_by(FPS_STEP)
                .chain(std::iter::once(0))
                .collect(),
            Self::UiScale => (MIN_UI_SCALE..=MAX_UI_SCALE)
                .step_by(UI_SCALE_STEP)
                .collect(),
            Self::RunSpeed => (MIN_RUN_SPEED..=MAX_RUN_SPEED).collect(),
        }
    }

    /// Where along its slider a value sits, from 0 at its left to 1 at its
    /// right.
    #[must_use]
    pub fn fraction(self, value: u16) -> f32 {
        match self {
            Self::ClipPlane | Self::MouseSensitivity | Self::SoundVolume | Self::MusicVolume => {
                f32::from(value.min(100)) / 100.0
            }
            Self::MaxFps if value == 0 => 1.0,
            Self::MaxFps => {
                let span = f32::from(MAX_FPS - MIN_FPS);
                // The far right is the display's rate, so a rate the slider
                // does not offer stops just short of it.
                (f32::from(value.clamp(MIN_FPS, MAX_FPS) - MIN_FPS) / span) * 0.975
            }
            Self::UiScale => {
                let span = f32::from(MAX_UI_SCALE - MIN_UI_SCALE);
                f32::from(value.clamp(MIN_UI_SCALE, MAX_UI_SCALE) - MIN_UI_SCALE) / span
            }
            Self::RunSpeed => {
                let span = f32::from(MAX_RUN_SPEED - MIN_RUN_SPEED);
                f32::from(value.clamp(MIN_RUN_SPEED, MAX_RUN_SPEED) - MIN_RUN_SPEED) / span
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
            Self::ClipPlane
            | Self::MouseSensitivity
            | Self::UiScale
            | Self::SoundVolume
            | Self::MusicVolume => {
                format!("{value} %")
            }
            Self::MaxFps if value == 0 => "Display".to_owned(),
            Self::MaxFps => value.to_string(),
            Self::RunSpeed => format!("{}.{:02}x", value / 100, value % 100),
        }
    }

    /// Whether a file's value is one the option takes.
    fn takes(self, value: u16) -> bool {
        match self {
            Self::ClipPlane | Self::MouseSensitivity | Self::SoundVolume | Self::MusicVolume => {
                value <= 100
            }
            Self::MaxFps => value == 0 || (1..=1000).contains(&value),
            Self::UiScale => (MIN_UI_SCALE..=MAX_UI_SCALE).contains(&value),
            Self::RunSpeed => RunSpeedMultiplier::from_percent(value).is_some(),
        }
    }

    /// Reads the saved value, including the original run speed checkbox.
    fn parse(self, value: &str) -> Option<u16> {
        match (self, value.trim()) {
            (Self::RunSpeed, "true") => Some(110),
            (Self::RunSpeed, "false") => Some(MIN_RUN_SPEED),
            (_, value) => value.parse().ok().filter(|n| self.takes(*n)),
        }
    }
}

/// Every option the client keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one of the Options window's checkboxes"
)]
pub struct Options {
    /// See [`Toggle::PetWindowPopup`].
    pub pet_window_popup: bool,
    /// See [`Toggle::AutoStand`].
    pub allow_auto_stand: bool,
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
    /// See [`Toggle::LookSpring`].
    pub look_spring: bool,
    /// See [`Toggle::Log`].
    pub log: bool,
    /// See [`Toggle::Sound`].
    pub sound: bool,
    /// See [`Toggle::Music`].
    pub music: bool,
    /// See [`Toggle::EnvironmentSounds`].
    pub environment_sounds: bool,
    /// See [`Toggle::CombatMusic`].
    pub combat_music: bool,
    /// See [`Toggle::SpellEffects`].
    pub spell_effects: bool,
    /// Which quality-of-life fixes are on ([`Toggle::Qol`]).
    pub qol: qol::Settings,
    /// How much of a player's name shows over their head, as `/shownames`
    /// sets it; the official client keeps it in `eqclient.ini`, this client
    /// per character.
    pub show_names: ShowNames,
    /// When a loot click on a NO DROP item asks first, as `/lootnodrop`
    /// sets it; the official client keeps it in `eqclient.ini`, this client
    /// per character.
    pub loot_no_drop: LootNoDrop,
    /// Whether the answer to an assist starts auto attack as it targets, as
    /// `/assist on` and `/assist off` set it (Titanium 0x45f40a, read
    /// R49-8 d). The official client keeps it in `eqclient.ini`
    /// (`[Defaults]` `AttackOnAssist`), this client per character. Its
    /// default there is unread, so it starts off (inferred, checklist
    /// 6.490).
    pub attack_on_assist: bool,
    /// See [`Level::ClipPlane`].
    pub clip_plane: u16,
    /// See [`Level::MaxFps`].
    pub max_fps: u16,
    /// See [`Level::MouseSensitivity`].
    pub mouse_sensitivity: u16,
    /// See [`Level::UiScale`].
    pub ui_scale: u16,
    /// See [`Level::SoundVolume`].
    pub sound_volume: u16,
    /// The chat's font size, 0 to 6, as `/chatfontsize` sets it; none
    /// keeps the one the official UI file names. Labelled departure: the
    /// official client sets only the active chat window's size and keeps it
    /// with that window's layout (read R49-22c, 0x46b099); this client sets
    /// one size for the whole chat and keeps it per character.
    pub chat_font_size: Option<u8>,
    /// The pointer speed modifier `/mousespeed` sets, kept as typed; the
    /// official client keeps it as `MousePointerSpeedMod` in `eqclient.ini`
    /// (read R49-22c, 0x471974). This client uses the system's pointer, so
    /// it is kept and reported but moves nothing.
    pub mouse_pointer_speed: Option<f64>,
    /// Each colour kind's colour as `/usercolor` set it, by the kind's
    /// number; none keeps the installation's. The official client keeps
    /// them as `User_<kind>_Red`, `_Green` and `_Blue` in `[TextColors]` of
    /// `eqclient.ini` (read R49-22c, 0x46b1b2), this client per character.
    pub user_colors: [Option<[u8; 3]>; USER_COLOR_KINDS],
    /// See [`Level::MusicVolume`].
    pub music_volume: u16,
    /// Which player races and genders draw with the Luclin models, as the
    /// official client's Options window chooses them (inferred, R49-44a);
    /// none by default.
    pub luclin_models: LuclinModels,
    /// The server filters, per character ([`crate::chat_filters`]).
    pub chat_filters: crate::chat_filters::ChatFilters,
}

/// How many colour kinds `/usercolor` takes: 0 to 86, as the program's
/// range check reads (0x46b87b).
pub const USER_COLOR_KINDS: usize = 87;

/// The names a file keeps the chat font size and the pointer speed under.
const CHAT_FONT_SIZE: &str = "chat_font_size";
const MOUSE_POINTER_SPEED: &str = "mouse_pointer_speed";
/// The prefix a file keeps each colour kind's colour under.
const USER_COLOR: &str = "user_color_";

impl Default for Options {
    /// The client's defaults: the pet's window pops up, a sitting player
    /// stands to move, the target ring, the
    /// player's helm and everyone's names show, the mouse is not inverted and its wheel
    /// zooms, the view does not spring back level, the chat is logged, sounds play at full volume, each
    /// quality-of-life fix starts as it is
    /// defined, players' names show in full, spawns draw as far as the zone
    /// allows, at most 60 frames a second ([`Fix::FrameCap`]), the camera
    /// turns at the client's own speed, and windows are drawn at the
    /// desktop's scaling.
    fn default() -> Self {
        let qol = qol::Settings::default();
        Self {
            pet_window_popup: true,
            allow_auto_stand: true,
            target_ring: true,
            show_helm: true,
            pc_names: true,
            npc_names: true,
            invert_y: false,
            wheel_zoom: true,
            look_spring: false,
            log: true,
            sound: true,
            music: true,
            environment_sounds: true,
            combat_music: true,
            spell_effects: true,
            qol,
            show_names: ShowNames::Everything,
            loot_no_drop: LootNoDrop::Ask,
            attack_on_assist: false,
            clip_plane: 100,
            max_fps: if qol.on(Fix::FrameCap) { 60 } else { 0 },
            mouse_sensitivity: 50,
            ui_scale: 100,
            sound_volume: 100,
            chat_font_size: None,
            mouse_pointer_speed: None,
            user_colors: [None; USER_COLOR_KINDS],
            music_volume: 100,
            luclin_models: LuclinModels::NONE,
            chat_filters: crate::chat_filters::ChatFilters::default(),
        }
    }
}

impl Options {
    /// Whether the toggle is on.
    #[must_use]
    pub const fn get(&self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::PetWindowPopup => self.pet_window_popup,
            Toggle::AutoStand => self.allow_auto_stand,
            Toggle::TargetRing => self.target_ring,
            Toggle::ShowHelm => self.show_helm,
            Toggle::PcNames => self.pc_names,
            Toggle::NpcNames => self.npc_names,
            Toggle::InvertY => self.invert_y,
            Toggle::WheelZoom => self.wheel_zoom,
            Toggle::LookSpring => self.look_spring,
            Toggle::Log => self.log,
            Toggle::Sound => self.sound,
            Toggle::Music => self.music,
            Toggle::EnvironmentSounds => self.environment_sounds,
            Toggle::CombatMusic => self.combat_music,
            Toggle::SpellEffects => self.spell_effects,
            Toggle::Qol(fix) => self.qol.on(fix),
        }
    }

    /// Turns the toggle on or off.
    pub const fn set(&mut self, toggle: Toggle, on: bool) {
        let option = match toggle {
            Toggle::PetWindowPopup => &mut self.pet_window_popup,
            Toggle::AutoStand => &mut self.allow_auto_stand,
            Toggle::TargetRing => &mut self.target_ring,
            Toggle::ShowHelm => &mut self.show_helm,
            Toggle::PcNames => &mut self.pc_names,
            Toggle::NpcNames => &mut self.npc_names,
            Toggle::InvertY => &mut self.invert_y,
            Toggle::WheelZoom => &mut self.wheel_zoom,
            Toggle::LookSpring => &mut self.look_spring,
            Toggle::Log => &mut self.log,
            Toggle::Sound => &mut self.sound,
            Toggle::Music => &mut self.music,
            Toggle::EnvironmentSounds => &mut self.environment_sounds,
            Toggle::CombatMusic => &mut self.combat_music,
            Toggle::SpellEffects => &mut self.spell_effects,
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
            Level::UiScale => self.ui_scale,
            Level::RunSpeed => self.qol.run_speed_multiplier.percent(),
            Level::SoundVolume => self.sound_volume,
            Level::MusicVolume => self.music_volume,
        }
    }

    /// Sets a slider's value.
    pub const fn set_level(&mut self, level: Level, value: u16) {
        let option = match level {
            Level::ClipPlane => &mut self.clip_plane,
            Level::MaxFps => &mut self.max_fps,
            Level::MouseSensitivity => &mut self.mouse_sensitivity,
            Level::UiScale => &mut self.ui_scale,
            Level::RunSpeed => {
                if let Some(multiplier) = RunSpeedMultiplier::from_percent(value) {
                    self.qol.run_speed_multiplier = multiplier;
                }
                return;
            }
            Level::SoundVolume => &mut self.sound_volume,
            Level::MusicVolume => &mut self.music_volume,
        };
        *option = value;
    }

    /// The share of the farthest drawing distance the scene is drawn within:
    /// the clip plane, never below a twentieth.
    #[must_use]
    pub fn clip_share(&self) -> f32 {
        (f32::from(self.clip_plane.min(100)) / 100.0).max(0.05)
    }

    /// How loud sounds play, from 0 for silent to 1 for their files' own
    /// loudness: nothing when sounds are off.
    #[must_use]
    pub fn sound_volume(&self) -> f32 {
        if self.sound {
            f32::from(self.sound_volume.min(100)) / 100.0
        } else {
            0.0
        }
    }

    /// How loud the zone's ambient sounds play: the sound volume, nothing
    /// when sounds or environment sounds are off.
    #[must_use]
    pub fn environment_volume(&self) -> f32 {
        if self.environment_sounds {
            self.sound_volume()
        } else {
            0.0
        }
    }

    /// How loud the zone's music plays, from 0 for silent to 1 for the
    /// synthesizer's own loudness: nothing when music is off.
    #[must_use]
    pub fn music_volume(&self) -> f32 {
        if self.music {
            f32::from(self.music_volume.min(100)) / 100.0
        } else {
            0.0
        }
    }

    /// The most frames a second, if the client holds the rate down at all.
    #[must_use]
    pub fn frame_cap(&self) -> Option<u32> {
        (self.max_fps > 0).then(|| u32::from(self.max_fps))
    }

    /// How far the camera turns, in radians, for each pixel the mouse drags
    /// it on a screen `width` pixels wide. The program turns
    /// (0.5 + 1.5 × (s − 1) / 7) × dx / width × 512 heading units, a whole
    /// turn being 512, for its sensitivity s from 1 to 8 (Titanium
    /// 0x4929dd). The setting's 0 to 100 spans s from 1 to 8 (inferred,
    /// recording checklist 6.418), so a drag across the whole screen turns
    /// from half a turn to two. A width that is no width turns nothing.
    #[must_use]
    pub fn turn_per_pixel(&self, width: f32) -> f32 {
        if !(width.is_finite() && width >= 1.0) {
            return 0.0;
        }
        let share = f32::from(self.mouse_sensitivity.min(100)) / 100.0;
        (0.5 + 1.5 * share) * std::f32::consts::TAU / width
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
            if options.read_chat_setting(key.trim(), value.trim()) {
                continue;
            }
            if key.trim() == LUCLIN_MODELS {
                options.luclin_models = LuclinModels::parse(value);
                continue;
            }
            if key.trim() == SHOW_NAMES {
                if let Some(level) = ShowNames::parse(value) {
                    options.show_names = level;
                }
                continue;
            }
            if key.trim() == LOOT_NO_DROP {
                if let Ok(level) = value.trim().parse() {
                    options.loot_no_drop = LootNoDrop::from_level(level);
                }
                continue;
            }
            if key.trim() == ATTACK_ON_ASSIST {
                if let Ok(on) = value.trim().parse() {
                    options.attack_on_assist = on;
                }
                continue;
            }
            if options.chat_filters.read(key.trim(), value) {
                continue;
            }
            if let Some(level) = Level::ALL
                .into_iter()
                .find(|level| level.key() == key.trim())
            {
                if let Some(number) = level.parse(value) {
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

    /// Reads a chat font size, pointer speed or colour line; whether the key
    /// was one of those.
    fn read_chat_setting(&mut self, key: &str, value: &str) -> bool {
        if key == CHAT_FONT_SIZE {
            if let Some(size) = value.parse().ok().filter(|size| *size <= 6) {
                self.chat_font_size = Some(size);
            }
        } else if key == MOUSE_POINTER_SPEED {
            if let Some(speed) = value.parse::<f64>().ok().filter(|speed| speed.is_finite()) {
                self.mouse_pointer_speed = Some(speed);
            }
        } else if let Some(kind) = key.strip_prefix(USER_COLOR) {
            let rgb: Vec<u8> = value
                .split_whitespace()
                .filter_map(|part| part.parse().ok())
                .collect();
            if let (Some(slot), &[red, green, blue]) = (
                kind.parse::<usize>()
                    .ok()
                    .and_then(|kind| self.user_colors.get_mut(kind)),
                rgb.as_slice(),
            ) {
                *slot = Some([red, green, blue]);
            }
        } else {
            return false;
        }
        true
    }

    /// The text a file keeps: the header, then every option.
    #[must_use]
    pub fn text(&self) -> String {
        use std::fmt::Write as _;
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
        let loot_no_drop = self.loot_no_drop.level().to_string();
        for part in [LOOT_NO_DROP, " = ", &loot_no_drop, "\n"] {
            text.push_str(part);
        }
        let attack_on_assist = self.attack_on_assist.to_string();
        for part in [ATTACK_ON_ASSIST, " = ", &attack_on_assist, "\n"] {
            text.push_str(part);
        }
        for level in Level::ALL {
            for part in [level.key(), " = ", &self.level(level).to_string(), "\n"] {
                text.push_str(part);
            }
        }
        if let Some(size) = self.chat_font_size {
            let _ = writeln!(text, "{CHAT_FONT_SIZE} = {size}");
        }
        if let Some(speed) = self.mouse_pointer_speed {
            let _ = writeln!(text, "{MOUSE_POINTER_SPEED} = {speed}");
        }
        for (kind, color) in self.user_colors.iter().enumerate() {
            if let Some([red, green, blue]) = color {
                let _ = writeln!(text, "{USER_COLOR}{kind} = {red} {green} {blue}");
            }
        }
        for part in [LUCLIN_MODELS, " = ", &self.luclin_models.words(), "\n"] {
            text.push_str(part);
        }
        text.push_str(&self.chat_filters.text());
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
        options.set(Toggle::Qol(Fix::JesusMode), true);
        options.show_names = ShowNames::Last;
        options.loot_no_drop = LootNoDrop::AskUnusable;
        options.luclin_models.set(6, 1, true);
        options.chat_filters.set(
            eq_network_game::filters::FilterCategory::Auctions,
            eq_network_game::filters::FilterMode::Hide,
        );
        options.attack_on_assist = true;
        let text = options.text();
        assert!(text.contains("luclin_models = DAF\n"));
        assert!(text.starts_with(HEADER));
        assert!(text.contains("invert_y = true\n"));
        assert!(text.contains("skip_modified_food = false\n"));
        assert!(text.contains("jesus_mode = true\n"));
        assert!(text.contains("hidden_windows = false\n"));
        assert!(text.contains("show_names = 2\n"));
        assert!(text.contains("loot_no_drop = 1\n"));
        assert!(text.contains("Channel04 = 0\n"));
        assert!(text.contains("attack_on_assist = true\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // A file from before the QoL page keeps its choices there.
        let before = Options::read(
            "# eq-client options v1\nskip_modified_food = false\nhidden_windows = true\n",
            Options::default(),
        );
        assert!(!before.qol.on(Fix::SkipModifiedFood));
        assert!(!before.qol.on(Fix::JesusMode));
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

    /// The chat font size, the pointer speed and the colours `/chatfontsize`,
    /// `/mousespeed` and `/usercolor` set keep to the file (6.525).
    #[test]
    fn chat_settings_keep_to_their_file() {
        let mut options = Options::default();
        options.set(Toggle::CombatMusic, false);
        options.chat_font_size = Some(4);
        options.mouse_pointer_speed = Some(1.5);
        options.user_colors[12] = Some([1, 2, 3]);
        options.user_colors[86] = Some([255, 0, 9]);
        let text = options.text();
        assert!(text.contains("combat_music = false\n"));
        assert!(text.contains("chat_font_size = 4\n"));
        assert!(text.contains("mouse_pointer_speed = 1.5\n"));
        assert!(text.contains("user_color_12 = 1 2 3\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // What no slot or range takes keeps the default.
        let read = Options::read(
            "chat_font_size = 9\nmouse_pointer_speed = inf\nuser_color_87 = 1 2 3\nuser_color_5 = 1 2\n",
            Options::default(),
        );
        assert_eq!(read, Options::default());
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
        // The program's curve: a drag across the whole screen turns half a
        // turn at sensitivity 1 (0) and two at 8 (100); 50 is s = 4.5.
        let mut quick = Options::default();
        let across = |options: &Options| options.turn_per_pixel(1_000.0) * 1_000.0;
        let turn = std::f32::consts::TAU;
        assert!((across(&quick) - 1.25 * turn).abs() < 1e-4);
        quick.set_level(Level::MouseSensitivity, 0);
        assert!((across(&quick) - 0.5 * turn).abs() < 1e-4);
        quick.set_level(Level::MouseSensitivity, 100);
        assert!((across(&quick) - 2.0 * turn).abs() < 1e-4);
        // Program sensitivity 4 is 3/7 of the way: 0.5 + 1.5 × 3/7.
        quick.set_level(Level::MouseSensitivity, 43);
        assert!((across(&quick) - (0.5 + 1.5 * 0.43) * turn).abs() < 1e-4);
        // The same drag on a screen twice as wide turns half as far.
        assert!((quick.turn_per_pixel(2_000.0) * 2.0 - quick.turn_per_pixel(1_000.0)).abs() < 1e-7);
        assert!(quick.turn_per_pixel(0.0).abs() < f32::EPSILON);
        // The scene always draws at least a twentieth of the way; how far
        // that is, each client generation's far plane says.
        options.set_level(Level::ClipPlane, 0);
        assert!((options.clip_share() - 0.05).abs() < f32::EPSILON);
        options.set_level(Level::ClipPlane, 50);
        assert!((options.clip_share() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn the_interface_scale_runs_from_half_to_double_in_fives_and_starts_at_the_desktops() {
        assert_eq!(Options::default().ui_scale, 100);
        assert_eq!(Level::UiScale.at(0.0), 50);
        assert_eq!(Level::UiScale.at(1.0), 200);
        assert_eq!(Level::UiScale.at(Level::UiScale.fraction(150)), 150);
        // Between stops, the nearest.
        assert_eq!(Level::UiScale.at(Level::UiScale.fraction(100) + 0.01), 100);
        assert_eq!(Level::UiScale.words(125), "125 %");
        assert!(Level::UiScale.rescales_layout());
        assert!(
            Level::ALL
                .into_iter()
                .filter(|level| *level != Level::UiScale)
                .all(|level| !level.rescales_layout())
        );
        let mut options = Options::default();
        options.set_level(Level::UiScale, 175);
        let text = options.text();
        assert!(text.contains("ui_scale = 175\n"));
        assert_eq!(Options::read(&text, Options::default()), options);
        // A scale the slider cannot hold keeps the default.
        for bad in ["ui_scale = 20\n", "ui_scale = 500\n", "ui_scale = big\n"] {
            assert_eq!(Options::read(bad, Options::default()).ui_scale, 100);
        }
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
    fn run_speed_migrates_the_checkbox_and_round_trips_in_hundredths() {
        let level = Level::RunSpeed;
        let defaults = Options::default();
        assert_eq!(defaults.level(level), 100);
        assert_eq!(level.at(0.0), 100);
        assert_eq!(level.at(0.5), 120);
        assert_eq!(level.at(1.0), 140);
        assert_eq!(level.at(f32::NAN), 100);
        assert_eq!(level.at(f32::INFINITY), 100);
        assert_eq!(level.at(level.fraction(121)), 121);
        assert_eq!(level.words(100), "1.00x");
        assert_eq!(level.words(121), "1.21x");
        assert_eq!(level.words(140), "1.40x");
        for (saved, percent) in [("false", 100), ("true", 110), ("120", 120), ("140", 140)] {
            let read = Options::read(&format!("run_boost = {saved}\n"), defaults);
            assert_eq!(read.level(level), percent);
            assert!(read.text().contains(&format!("run_boost = {percent}\n")));
            assert_eq!(Options::read(&read.text(), defaults), read);
        }
        let mut selected = defaults;
        selected.set_level(level, 120);
        for invalid in ["99", "141", "-100", "NaN", "inf", "1.40"] {
            let text = format!("run_boost = {invalid}\n");
            assert_eq!(Options::read(&text, defaults).level(level), 100);
            assert_eq!(Options::read(&text, selected).level(level), 120);
        }
        selected.set_level(level, 141);
        assert_eq!(selected.level(level), 120);
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
            Some(Capability::Meals)
        );
        assert_eq!(
            Toggle::all().count(),
            Toggle::OFFICIAL.len() + Fix::settings().count()
        );
    }
}

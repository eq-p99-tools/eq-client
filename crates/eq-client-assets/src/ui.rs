//! Window layouts from the installed client's UI skin, read at runtime so none of
//! the skin's data is copied into this project and a custom skin carries over.
use std::{
    collections::{BTreeSet, HashMap},
    io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// The skin a character uses until it chooses another, and the one the official
/// client reads any window file from that a custom skin leaves out.
pub const DEFAULT_SKIN: &str = "default";

/// Titanium equipment slots, from charm to ammo.
const EQUIPMENT: std::ops::RangeInclusive<u16> = 0..=21;

/// Where a skin's inventory window puts the equipment and the character, in that
/// window's pixels, relative to the top-left of the two together.
#[derive(Clone, Debug, PartialEq)]
pub struct EquipmentLayout {
    /// The equipment slots the window shows, in the order it draws them; a skin
    /// may leave some out, as classic-era skins do with the charm slot.
    pub slots: Vec<SlotPlacement>,
    /// Where the window draws the character, when it draws one.
    pub character: Option<Area>,
}

/// Where an equipment slot sits in a skin's inventory window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotPlacement {
    /// Titanium inventory slot, the skin's `EQType`.
    pub slot: u16,
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Slot width.
    pub width: f32,
    /// Slot height.
    pub height: f32,
}

/// A rectangle in a window's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Why a skin's inventory layout could not be used.
#[derive(Debug, Error)]
pub enum UiLayoutError {
    /// The skin name could name a path outside the UI directory.
    #[error("invalid UI skin name: {0}")]
    InvalidSkin(String),
    /// The window file could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// Window file path.
        path: PathBuf,
        /// Operating-system error.
        source: io::Error,
    },
    /// The window file is not well-formed XML.
    #[error("invalid UI XML: {0}")]
    Xml(#[from] roxmltree::Error),
    /// No window in the file shows an equipment slot.
    #[error("the inventory window shows no equipment slots")]
    NoEquipment,
    /// The window shows an equipment slot twice.
    #[error("the inventory window shows equipment slot {0} twice")]
    DuplicateSlot(u16),
    /// A slot's location or size is missing or not a number.
    #[error("equipment slot {0} has no usable location or size")]
    Geometry(u16),
    /// The file name could name a path outside the skin's directory.
    #[error("invalid UI file name: {0}")]
    InvalidFile(String),
    /// A texture sheet could not be decoded.
    #[error("could not decode {path}: {source}")]
    Image {
        /// Sheet path.
        path: PathBuf,
        /// Decoder error.
        source: image::ImageError,
    },
    /// The window file defines no window of that name.
    #[error("the skin defines no window {0}")]
    MissingWindow(String),
    /// The set of interface files has no file of that name.
    #[error("the skin's set of interface files has no {0}")]
    MissingFile(String),
    /// The file is not a cursor: an icon, or no Windows icon file at all.
    #[error("{0} is not a cursor")]
    NotCursor(PathBuf),
    /// A texture sheet is not the size its cells assume.
    #[error("{path} is {width}x{height}, not {SHEET_SIZE}x{SHEET_SIZE}")]
    SheetSize {
        /// Sheet path.
        path: PathBuf,
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
}

/// Width and height of the texture sheets that hold icons, such as
/// `dragitem1.tga` and `spells01.tga`.
pub const SHEET_SIZE: u32 = 256;

/// The file a skin uses for `file`: its own, or the default skin's when it
/// has none, as the official client reads it.
///
/// # Errors
/// Rejects skin and file names that could leave the skin's directory.
pub fn skin_file(eq_directory: &Path, skin: &str, file: &str) -> Result<PathBuf, UiLayoutError> {
    if !valid_skin(skin) {
        return Err(UiLayoutError::InvalidSkin(skin.to_owned()));
    }
    if !plain_name(file) || file.chars().all(|c| c == '.') {
        return Err(UiLayoutError::InvalidFile(file.to_owned()));
    }
    let skins = eq_directory.join("uifiles");
    let own = skins.join(skin).join(file);
    Ok(if own.is_file() {
        own
    } else {
        skins.join(DEFAULT_SKIN).join(file)
    })
}

/// Reads one of a skin's textures, such as `window_pieces01.tga`, as RGBA
/// pixels, whatever its size.
///
/// # Errors
/// Rejects unsafe names and unreadable or undecodable files.
pub fn texture(
    eq_directory: &Path,
    skin: &str,
    file: &str,
) -> Result<image::RgbaImage, UiLayoutError> {
    let path = skin_file(eq_directory, skin, file)?;
    Ok(image::open(&path)
        .map_err(|source| UiLayoutError::Image { path, source })?
        .into_rgba8())
}

/// A skin's mouse cursor, such as `arrow.cur`: its picture and the pixel in
/// it that points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinCursor {
    /// The picture, as RGBA pixels.
    pub pixels: image::RgbaImage,
    /// The pixel that points, across and down from the picture's top left.
    pub hotspot: (u16, u16),
}

/// Reads one of a skin's cursors, such as `arrow.cur`, from the picture the
/// image crate picks among the sizes the file holds, with that picture's
/// hotspot.
///
/// # Errors
/// Rejects unsafe names, unreadable files, files that are not cursors, icons
/// among them, and pictures that cannot be decoded.
pub fn cursor(eq_directory: &Path, skin: &str, file: &str) -> Result<SkinCursor, UiLayoutError> {
    let path = skin_file(eq_directory, skin, file)?;
    let bytes = std::fs::read(&path).map_err(|source| UiLayoutError::Read {
        path: path.clone(),
        source,
    })?;
    let hotspot = cursor_hotspot(&bytes).ok_or_else(|| UiLayoutError::NotCursor(path.clone()))?;
    let pixels = image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico)
        .map_err(|source| UiLayoutError::Image { path, source })?
        .into_rgba8();
    Ok(SkinCursor { pixels, hotspot })
}

/// The hotspot of the picture the image crate decodes from a cursor file,
/// which it does not report: a cursor's header names it in each picture's
/// directory entry. The image crate reads a cursor as an icon and decodes the
/// entry it scores highest by the field an icon keeps its bits per pixel in,
/// then by its area, where a side of 0 is 256; the last entry is taken first
/// and an earlier one only when it scores strictly higher. In a cursor that
/// field is the hotspot's y, so the same rule finds the same entry here.
/// None when the file is no cursor or its directory is cut short.
fn cursor_hotspot(bytes: &[u8]) -> Option<(u16, u16)> {
    let word = |at: usize| Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
    // The reserved word, then the type: 1 for an icon, 2 for a cursor.
    if word(0)? != 0 || word(2)? != 2 {
        return None;
    }
    let entries = (0..usize::from(word(4)?))
        .map(|index| bytes.get(6 + 16 * index..22 + 16 * index))
        .collect::<Option<Vec<_>>>()?;
    let field = |entry: &[u8], at: usize| u16::from_le_bytes([entry[at], entry[at + 1]]);
    let side = |length: u8| if length == 0 { 256 } else { u32::from(length) };
    let score = |entry: &[u8]| (field(entry, 6), side(entry[0]) * side(entry[1]));
    let (last, earlier) = entries.split_last()?;
    let mut best = *last;
    for entry in earlier {
        if score(entry) > score(best) {
            best = entry;
        }
    }
    Some((field(best, 4), field(best, 6)))
}

/// Reads one of a skin's icon sheets, such as `dragitem1.tga`, as RGBA pixels.
///
/// # Errors
/// Rejects unsafe names, unreadable or undecodable files, and sheets that are
/// not [`SHEET_SIZE`] pixels square.
pub fn texture_sheet(
    eq_directory: &Path,
    skin: &str,
    file: &str,
) -> Result<image::RgbaImage, UiLayoutError> {
    let path = skin_file(eq_directory, skin, file)?;
    let pixels = image::open(&path)
        .map_err(|source| UiLayoutError::Image {
            path: path.clone(),
            source,
        })?
        .into_rgba8();
    if pixels.width() != SHEET_SIZE || pixels.height() != SHEET_SIZE {
        return Err(UiLayoutError::SheetSize {
            path,
            width: pixels.width(),
            height: pixels.height(),
        });
    }
    Ok(pixels)
}

/// Which official client an installation holds. Its own settings files
/// (`eqclient.ini`, `UI_<character>_<world>.ini` and `<character>_<world>.ini`)
/// are read only by the rules of the generation they were checked against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstalledClient {
    /// Titanium, the client P99 and stock `EQEmu` servers expect.
    #[default]
    Titanium,
    /// The Mac-era client Project Quarm and TAKP servers expect.
    EqMac,
}

impl InstalledClient {
    /// Whether this client has the short effects window, for songs and
    /// other short buffs; unchecked for the Mac-era client.
    #[must_use]
    pub const fn short_effects(self) -> bool {
        matches!(self, Self::Titanium)
    }

    /// Whether this client shows casts in its own casting window;
    /// unchecked for the Mac-era client.
    #[must_use]
    pub const fn casting_window(self) -> bool {
        matches!(self, Self::Titanium)
    }

    /// The face this client writes its windows in, by its file among
    /// Windows' fonts; None until it is checked for this client. The
    /// Mac-era client's face is unchecked, and its installation carries no
    /// font files, so it writes in the Titanium client's face rather than
    /// a monospace placeholder: inferred, recording checklist item 6.152.
    #[must_use]
    pub const fn font_file(self) -> Option<&'static str> {
        match self {
            Self::Titanium | Self::EqMac => Some("arial.ttf"),
        }
    }

    /// What this client shows while it loads a zone, from its own files;
    /// None until that is checked for this client. The Mac-era client has no
    /// such picture among its files.
    #[must_use]
    pub const fn loading_screen(self) -> Option<LoadingScreen> {
        match self {
            Self::Titanium => Some(LoadingScreen {
                picture: &["Resources", "loadscreen.jpg"],
                status: 12387,
            }),
            Self::EqMac => None,
        }
    }

    /// The spell particle effects this client draws, from its files in
    /// `eq_directory`; None until they are checked for this client.
    #[must_use]
    pub fn spell_effects(
        self,
        eq_directory: &Path,
    ) -> Option<Result<crate::spells::effects::SpellEffects, crate::spells::effects::EffectsError>>
    {
        match self {
            Self::Titanium => Some(crate::spells::effects::SpellEffects::read_titanium(
                eq_directory,
            )),
            Self::EqMac => None,
        }
    }

    /// Where this client keeps its sound files; None until that is checked
    /// for this client.
    #[must_use]
    pub const fn sound_files(self) -> Option<crate::sounds::SoundFiles> {
        match self {
            Self::Titanium => Some(crate::sounds::SoundFiles {
                archive_stem: "snd",
                folders: &["sounds"],
                numbers: "SoundAssets.txt",
            }),
            Self::EqMac => None,
        }
    }

    /// How this client tells the time, from the program (checklist items
    /// 1.2, 6.115 and 6.155).
    #[must_use]
    pub const fn clock(self) -> crate::clock::ClientClock {
        match self {
            Self::Titanium => crate::clock::TITANIUM,
            Self::EqMac => crate::clock::EQMAC,
        }
    }

    /// How this client's player falls and what a landing costs, from the
    /// program; None until read for this client (Titanium: recording
    /// checklist items 1.5, 6.147 and 6.167).
    #[must_use]
    pub const fn falls(self) -> Option<crate::falls::ClientFalls> {
        match self {
            Self::Titanium => None,
            Self::EqMac => Some(crate::falls::EQMAC),
        }
    }

    /// The least time between two jumps, from the program; None until read
    /// for this client. The Titanium client refuses a jump within 500 ms of
    /// the last (recording checklist item 1.3).
    #[must_use]
    pub const fn jump_spacing(self) -> Option<std::time::Duration> {
        match self {
            Self::Titanium => Some(std::time::Duration::from_millis(500)),
            Self::EqMac => None,
        }
    }

    /// The spell effects whose buffs this client takes off the player
    /// before a cast, a song or an item's click, in the order it looks for
    /// them, from the program; None where it drops nothing or that is
    /// unread. The Mac-era client looks for effect 94, then invisibility
    /// (12), invisibility versus undead (28) and versus animals (29) (TAKP
    /// Batch 1 R1, recording checklist item 6.163); the Titanium client's
    /// cast sends nothing before it (sitting 8).
    #[must_use]
    pub const fn invisibility_drop(self) -> Option<&'static [u32]> {
        match self {
            Self::Titanium => None,
            Self::EqMac => Some(&[94, 12, 28, 29]),
        }
    }

    /// How this client's attack key turns auto attack on and off, from the
    /// program; None until read for this client (Titanium: sittings 6 and
    /// 7). The Mac-era client's is TAKP Batch 1 R2 (recording checklist
    /// item 6.157).
    #[must_use]
    pub const fn attack_toggle(self) -> Option<AttackToggle> {
        match self {
            Self::Titanium => None,
            Self::EqMac => Some(AttackToggle {
                frozen: 12368,
                not_standing: 12369,
                on: 12370,
                off: 12371,
                line_type: 13,
            }),
        }
    }

    /// How this client prices what its merchant window shows, from the
    /// program; None until read for this client (Titanium: sitting 8's
    /// T18).
    #[must_use]
    pub const fn merchant_prices(self) -> Option<crate::merchants::MerchantPrices> {
        match self {
            Self::Titanium => None,
            Self::EqMac => Some(crate::merchants::EQMAC),
        }
    }

    /// The settings files of this client, installed in `eq_directory`.
    pub fn settings(self, eq_directory: &Path) -> Box<dyn OfficialSettings + '_> {
        match self {
            Self::Titanium => Box::new(Titanium(eq_directory)),
            Self::EqMac => Box::new(Unchecked),
        }
    }
}

/// How a client's attack key works: it needs no target and never turns
/// the player, refuses to turn auto attack on for a player who is not
/// standing, without standing them up, turns it off instead when it is on,
/// and turns it off by itself when the player stops standing. Its lines
/// are by their ids in the client's string table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackToggle {
    /// The refusal for a player the server froze in place.
    pub frozen: u32,
    /// The refusal for any other player not standing, such as one sitting.
    pub not_standing: u32,
    /// The line when auto attack turns on.
    pub on: u32,
    /// The line when it turns off, by the key or by itself.
    pub off: u32,
    /// The chat line type the on and off lines take their colour from.
    pub line_type: u32,
}

/// The picture an installed client covers the screen with while it loads a
/// zone, and the line it writes over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadingScreen {
    /// The picture's file, folder by folder under the installation.
    pub picture: &'static [&'static str],
    /// The line, by its id in the client's string table.
    pub status: u32,
}

impl LoadingScreen {
    /// Reads the picture from the installation as RGBA pixels.
    ///
    /// # Errors
    /// Fails if the file cannot be read or decoded.
    pub fn read_picture(&self, eq_directory: &Path) -> Result<image::RgbaImage, UiLayoutError> {
        let path = self
            .picture
            .iter()
            .fold(eq_directory.to_path_buf(), |path, part| path.join(part));
        Ok(image::open(&path)
            .map_err(|source| UiLayoutError::Image { path, source })?
            .into_rgba8())
    }
}

/// What this client reads, read only, from the settings an installed official
/// client keeps for itself and its characters. Each reader reads nothing by
/// default, so a client generation's files stay unread until a reader for
/// them has been checked against that client.
pub trait OfficialSettings {
    /// The options in `eqclient.ini`.
    fn options(&self) -> OfficialOptions {
        OfficialOptions::default()
    }

    /// The skin a character last chose, by its name. None when the client
    /// says nothing usable.
    fn chosen_skin(&self, _character: &str, _world: &str) -> Option<String> {
        None
    }

    /// The first page of a character's hotbuttons on a world, as each
    /// button's number from 1 and its code (such as `H2`).
    fn hotbuttons(&self, _character: &str, _world: &str) -> Vec<(u8, String)> {
        Vec::new()
    }

    /// Where the client last put a character's windows on a world, at every
    /// screen size it was played at.
    fn window_positions(&self, _character: &str, _world: &str) -> Vec<WindowPosition> {
        Vec::new()
    }

    /// How the client last drew a character's windows' backgrounds on a
    /// world, and how they fade.
    fn window_looks(&self, _character: &str, _world: &str) -> Vec<WindowLook> {
        Vec::new()
    }

    /// The colours the player chose for the chat's kinds of text on the
    /// Options window's Colors page, by the number of the kind: 256 for
    /// what is said, 257 for tells, and so on, as servers number them.
    fn text_colors(&self) -> Vec<(u16, [u8; 3])> {
        Vec::new()
    }

    /// The font size, by the client's font number, of a character's main
    /// chat window on a world.
    fn chat_font(&self, _character: &str, _world: &str) -> Option<u8> {
        None
    }

    /// The account and world the official login screen last used.
    fn login(&self) -> LoginMemory {
        LoginMemory::default()
    }
}

/// What the official login screen remembers between runs; each None when
/// the file or setting is missing or empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginMemory {
    /// The login server the official client logs in at, as `host:port` or
    /// a bare host: the one the account and world belong to.
    pub server: Option<String>,
    /// The account name last logged in with.
    pub account: Option<String>,
    /// The name of the world last played on.
    pub world: Option<String>,
}

/// An installed Titanium client's settings files, in its installation.
struct Titanium<'a>(&'a Path);

impl OfficialSettings for Titanium<'_> {
    /// `eqclient.ini`, the options the client keeps for every character.
    fn options(&self) -> OfficialOptions {
        std::fs::read(self.0.join("eqclient.ini"))
            .map(|bytes| options_from_ini(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// `UISkin` in the `[Main]` section of `UI_<character>_<world>.ini`, where
    /// `world` is the world's short name. None when the file or setting is
    /// missing or names no usable skin.
    fn chosen_skin(&self, character: &str, world: &str) -> Option<String> {
        if !plain_name(character) || !plain_name(world) {
            return None;
        }
        let bytes = std::fs::read(self.0.join(format!("UI_{character}_{world}.ini"))).ok()?;
        ini_value(&String::from_utf8_lossy(&bytes), "Main", "UISkin")
            .filter(|skin| valid_skin(skin))
    }

    /// `[HotButtons]` in `<character>_<world>.ini`. Empty when the file, the
    /// section or a sane name is missing.
    fn hotbuttons(&self, character: &str, world: &str) -> Vec<(u8, String)> {
        let plain =
            |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric());
        if !plain(character) || !plain(world) {
            return Vec::new();
        }
        std::fs::read(self.0.join(format!("{character}_{world}.ini")))
            .map(|bytes| hotbuttons_from_ini(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// The sections of `UI_<character>_<world>.ini`; none when the file is
    /// missing.
    fn window_positions(&self, character: &str, world: &str) -> Vec<WindowPosition> {
        if !plain_name(character) || !plain_name(world) {
            return Vec::new();
        }
        std::fs::read(self.0.join(format!("UI_{character}_{world}.ini")))
            .map(|bytes| positions_from_ini(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// The same sections' backgrounds and fading; none when the file is
    /// missing.
    fn window_looks(&self, character: &str, world: &str) -> Vec<WindowLook> {
        if !plain_name(character) || !plain_name(world) {
            return Vec::new();
        }
        std::fs::read(self.0.join(format!("UI_{character}_{world}.ini")))
            .map(|bytes| looks_from_ini(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// `[TextColors]` of `eqclient.ini`: `User_<n>_Red`, `_Green` and
    /// `_Blue` for the kind numbered 255 + n. That the numbers line up so
    /// is inferred from the 86 colours the file keeps and the kinds servers
    /// number from 256, not checked against the Colors page.
    fn text_colors(&self) -> Vec<(u16, [u8; 3])> {
        std::fs::read(self.0.join("eqclient.ini"))
            .map(|bytes| text_colors_from_ini(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// `ChatWindow0_FontStyle` in `[ChatManager]` of the character's UI
    /// file: window 0 is the main chat, as its name there says.
    fn chat_font(&self, character: &str, world: &str) -> Option<u8> {
        if !plain_name(character) || !plain_name(world) {
            return None;
        }
        let bytes = std::fs::read(self.0.join(format!("UI_{character}_{world}.ini"))).ok()?;
        ini_value(
            &String::from_utf8_lossy(&bytes),
            "ChatManager",
            "ChatWindow0_FontStyle",
        )?
        .parse()
        .ok()
    }

    /// `Username` in `[PLAYER]` and `LastServerName` in `[MISC]` of
    /// `eqlsPlayerData.ini`, which the official login screen keeps, and the
    /// login server in `eqhost.txt`, which those belong to.
    fn login(&self) -> LoginMemory {
        let value = |file: &str, section, key| {
            let bytes = std::fs::read(self.0.join(file)).ok()?;
            ini_value(&String::from_utf8_lossy(&bytes), section, key)
                .filter(|value| !value.is_empty())
        };
        LoginMemory {
            server: value("eqhost.txt", "LoginServer", "Host"),
            account: value("eqlsPlayerData.ini", "PLAYER", "Username"),
            world: value("eqlsPlayerData.ini", "MISC", "LastServerName"),
        }
    }
}

/// A client whose settings files no reader has been checked against yet.
struct Unchecked;

impl OfficialSettings for Unchecked {}

/// What the official client's `eqclient.ini` says of the options this
/// client keeps per character; each None when the file or setting is missing
/// or says neither on nor off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OfficialOptions {
    /// Whether the chat is logged from login: `Log` in `[Defaults]`, which
    /// the official `/log` saves.
    pub log: Option<bool>,
    /// Whether sounds play: `Sound` in `[Defaults]`.
    pub sound: Option<bool>,
    /// The Display page's Show PC Names: `PCNames` in `[Options]`.
    pub pc_names: Option<bool>,
    /// The Display page's Show NPC Names: `NPCNames` in `[Options]`.
    pub npc_names: Option<bool>,
    /// How much of a player's name `/shownames` shows, 0 for none:
    /// `ShowNamesLevel` in `[Defaults]`.
    pub show_names_level: Option<u32>,
    /// The Display page's Far Clip Plane: `ClipPlane` in `[Options]`, from 0
    /// to 20 as far as is known.
    pub clip_plane: Option<u32>,
    /// The Display page's Max. Frames Per Second, 0 for no cap: `MaxFPS` in
    /// `[Options]`.
    pub max_fps: Option<u32>,
    /// The Mouse page's Mouselook Sensitivity: `MouseSensitivity` in
    /// `[Options]`, from 0 to 10 as far as is known.
    pub mouse_sensitivity: Option<u32>,
}

fn options_from_ini(text: &str) -> OfficialOptions {
    let flag = |section, key| match ini_value(text, section, key)?.to_ascii_uppercase().as_str() {
        "TRUE" | "1" => Some(true),
        "FALSE" | "0" => Some(false),
        _ => None,
    };
    OfficialOptions {
        log: flag("Defaults", "Log"),
        sound: flag("Defaults", "Sound"),
        pc_names: flag("Options", "PCNames"),
        npc_names: flag("Options", "NPCNames"),
        show_names_level: ini_value(text, "Defaults", "ShowNamesLevel")
            .and_then(|level| level.trim().parse().ok()),
        clip_plane: ini_value(text, "Options", "ClipPlane").and_then(|v| v.trim().parse().ok()),
        max_fps: ini_value(text, "Options", "MaxFPS").and_then(|v| v.trim().parse().ok()),
        mouse_sensitivity: ini_value(text, "Options", "MouseSensitivity")
            .and_then(|v| v.trim().parse().ok()),
    }
}

fn hotbuttons_from_ini(text: &str) -> Vec<(u8, String)> {
    (1..=10u8)
        .filter_map(|button| {
            let code = ini_value(text, "HotButtons", &format!("Page1Button{button}"))?;
            (!code.is_empty()).then_some((button, code))
        })
        .collect()
}

/// Where the official client last put one of a character's windows, at one
/// screen size: a section of `UI_<character>_<world>.ini`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowPosition {
    /// The window, by the official client's name, such as `PlayerWindow`.
    pub window: String,
    /// The screen's width and height the position is for.
    pub screen: (u32, u32),
    /// Left edge, in that screen's pixels.
    pub x: i32,
    /// Top edge.
    pub y: i32,
}

/// `XPos<width>x<height>` and `YPos<width>x<height>` pairs, by section.
fn positions_from_ini(text: &str) -> Vec<WindowPosition> {
    let mut section = "";
    let mut lefts: Vec<(String, (u32, u32), i32)> = Vec::new();
    let mut tops: HashMap<(String, (u32, u32)), i32> = HashMap::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<i32>() else {
            continue;
        };
        let screen = |suffix: &str| {
            let (width, height) = suffix.split_once('x')?;
            Some((width.parse().ok()?, height.parse().ok()?))
        };
        if let Some(size) = key.trim().strip_prefix("XPos").and_then(screen) {
            lefts.push((section.to_owned(), size, value));
        } else if let Some(size) = key.trim().strip_prefix("YPos").and_then(screen) {
            tops.insert((section.to_owned(), size), value);
        }
    }
    lefts
        .into_iter()
        .filter_map(|(window, screen, x)| {
            let y = *tops.get(&(window.clone(), screen))?;
            Some(WindowPosition {
                window,
                screen,
                x,
                y,
            })
        })
        .collect()
}

/// How the official client draws a window's background, and how the
/// background fades while the pointer is away: the keys of the window's
/// section in `UI_<character>_<world>.ini`. What each key means is inferred
/// from the client's window settings and the values it writes, not checked
/// against a recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowLook {
    /// The window, by its section's name, such as `MainChat`.
    pub window: String,
    /// The background's opacity while the window is in use, from 0 to 255
    /// (`Alpha`).
    pub alpha: u8,
    /// Whether the background fades while the pointer is away (`Fades`).
    pub fades: bool,
    /// The opacity it fades to (`FadeToAlpha`).
    pub fade_to_alpha: u8,
    /// How long the pointer is away before it fades, in milliseconds
    /// (`Delay`).
    pub delay_ms: u32,
    /// How long the fade takes, in milliseconds (`Duration`).
    pub duration_ms: u32,
    /// Whether the background is a flat colour, its tint (`BGType` 2),
    /// rather than the skin's own (`BGType` 1).
    pub flat: bool,
    /// The background's tint (`BGTint.red`, `BGTint.green`, `BGTint.blue`).
    pub tint: [u8; 3],
}

impl WindowLook {
    /// A window as the client draws one it saved nothing for: the skin's
    /// background, opaque, faded to opaque after two seconds in half a
    /// second. The delay and duration are what it wrote for every window in
    /// the 284 UI files on the PC this was written on (counted); the rest
    /// is what it wrote for nearly all of them.
    fn new(window: &str) -> Self {
        Self {
            window: window.to_owned(),
            alpha: u8::MAX,
            fades: true,
            fade_to_alpha: u8::MAX,
            delay_ms: 2000,
            duration_ms: 500,
            flat: false,
            tint: [u8::MAX; 3],
        }
    }
}

/// Each section's background and fading keys; a section without any is no
/// window look.
fn looks_from_ini(text: &str) -> Vec<WindowLook> {
    let mut looks: Vec<WindowLook> = Vec::new();
    let mut section = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let number = || value.parse::<u32>().ok();
        let byte = || value.parse::<u8>().ok();
        let known = matches!(
            key,
            "Alpha"
                | "Fades"
                | "FadeToAlpha"
                | "Delay"
                | "Duration"
                | "BGType"
                | "BGTint.red"
                | "BGTint.green"
                | "BGTint.blue"
        );
        if !known {
            continue;
        }
        if looks.last().is_none_or(|look| look.window != section) {
            looks.push(WindowLook::new(section));
        }
        let Some(look) = looks.last_mut() else {
            continue;
        };
        match key {
            "Alpha" => look.alpha = byte().unwrap_or(look.alpha),
            "Fades" => {
                look.fades = match value.to_ascii_lowercase().as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => look.fades,
                };
            }
            "FadeToAlpha" => look.fade_to_alpha = byte().unwrap_or(look.fade_to_alpha),
            "Delay" => look.delay_ms = number().unwrap_or(look.delay_ms),
            "Duration" => look.duration_ms = number().unwrap_or(look.duration_ms),
            "BGType" => look.flat = number() == Some(2),
            "BGTint.red" => look.tint[0] = byte().unwrap_or(look.tint[0]),
            "BGTint.green" => look.tint[1] = byte().unwrap_or(look.tint[1]),
            _ => look.tint[2] = byte().unwrap_or(look.tint[2]),
        }
    }
    looks
}

/// The colours `[TextColors]` keeps whole, red, green and blue, by the kind
/// of text each is for (255 + n for `User_<n>`).
fn text_colors_from_ini(text: &str) -> Vec<(u16, [u8; 3])> {
    let mut colors: std::collections::BTreeMap<u16, [Option<u8>; 3]> =
        std::collections::BTreeMap::new();
    let mut section = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        if !section.eq_ignore_ascii_case("TextColors") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some((number, part)) = key
            .trim()
            .strip_prefix("User_")
            .and_then(|rest| rest.split_once('_'))
        else {
            continue;
        };
        let (Ok(number), Ok(value)) = (number.parse::<u16>(), value.trim().parse::<u8>()) else {
            continue;
        };
        let index = match part {
            "Red" => 0,
            "Green" => 1,
            "Blue" => 2,
            _ => continue,
        };
        if let Some(kind) = number.checked_add(255) {
            colors.entry(kind).or_default()[index] = Some(value);
        }
    }
    colors
        .into_iter()
        .filter_map(|(kind, [red, green, blue])| Some((kind, [red?, green?, blue?])))
        .collect()
}

/// Reads the equipment layout of a skin's inventory window, from the default
/// skin's file when the skin has none of its own, as the official client does.
///
/// # Errors
/// Rejects unsafe skin names, unreadable or malformed files, and windows that
/// show no equipment slot, show one twice, or place one without a positive size.
pub fn equipment_layout(eq_directory: &Path, skin: &str) -> Result<EquipmentLayout, UiLayoutError> {
    let path = skin_file(eq_directory, skin, "EQUI_Inventory.xml")?;
    let bytes = std::fs::read(&path).map_err(|source| UiLayoutError::Read { path, source })?;
    // Skins declare an ASCII encoding, but hand-edited ones may carry Latin-1 text.
    equipment_from_xml(&String::from_utf8_lossy(&bytes))
}

fn equipment_from_xml(text: &str) -> Result<EquipmentLayout, UiLayoutError> {
    let document = roxmltree::Document::parse(text)?;
    // Window files define each element once at the top level, then list elements
    // as the pieces of the screens and pages that show them.
    let defined: Vec<(&str, roxmltree::Node<'_, '_>)> = document
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("item")?, node)))
        .collect();
    let elements: HashMap<&str, roxmltree::Node<'_, '_>> = defined.iter().copied().collect();
    let slot_of = |name: &str| {
        elements
            .get(name)
            .filter(|node| node.has_tag_name("InvSlot"))
            .and_then(|node| equipment_slot(*node))
    };
    // Coordinates are relative to the showing container, so use the one container
    // that shows the equipment; slots defined but shown nowhere are not drawn.
    let equipment_shown = |node: roxmltree::Node<'_, '_>| {
        pieces(node)
            .filter(|piece| slot_of(piece).is_some())
            .count()
    };
    let container = defined
        .iter()
        .map(|(_, node)| *node)
        .filter(|node| equipment_shown(*node) > 0)
        .max_by_key(|node| equipment_shown(*node))
        .ok_or(UiLayoutError::NoEquipment)?;
    let mut placed = BTreeSet::new();
    let mut slots = Vec::new();
    for piece in pieces(container) {
        let Some(slot) = slot_of(piece) else {
            continue;
        };
        if !placed.insert(slot) {
            return Err(UiLayoutError::DuplicateSlot(slot));
        }
        let bounds = area(elements[piece])
            .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
            .ok_or(UiLayoutError::Geometry(slot))?;
        slots.push(SlotPlacement {
            slot,
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        });
    }
    let mut character = pieces(container)
        .filter_map(|piece| elements.get(piece))
        .find(|node| screen_id(**node) == Some("IW_CharacterView"))
        .and_then(|view| {
            let origin = area(*view)?;
            let art = pieces(*view)
                .filter_map(|piece| elements.get(piece))
                .find(|node| screen_id(**node) == Some("ClassAnim"))
                .and_then(|node| area(*node))?;
            Some(Area {
                x: origin.x + art.x,
                y: origin.y + art.y,
                ..art
            })
        })
        .filter(|area| area.width > 0.0 && area.height > 0.0);
    let left = slots
        .iter()
        .map(|p| p.x)
        .chain(character.map(|area| area.x))
        .fold(f32::INFINITY, f32::min);
    let top = slots
        .iter()
        .map(|p| p.y)
        .chain(character.map(|area| area.y))
        .fold(f32::INFINITY, f32::min);
    for placement in &mut slots {
        placement.x -= left;
        placement.y -= top;
    }
    if let Some(area) = &mut character {
        area.x -= left;
        area.y -= top;
    }
    Ok(EquipmentLayout { slots, character })
}

/// The equipment slot an `InvSlot` element shows, if any.
fn equipment_slot(node: roxmltree::Node<'_, '_>) -> Option<u16> {
    let value = number(node, &["EQType"])?;
    // Equipment types are small whole numbers.
    let whole = value.fract() == 0.0 && value >= 0.0 && value <= f32::from(u16::MAX);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked above.
    let slot = whole.then_some(value as u16)?;
    EQUIPMENT.contains(&slot).then_some(slot)
}

/// The elements a screen or page shows, by name; a piece may carry its element
/// type as a prefix, as in `Screen:IW_CharacterView`.
fn pieces<'a>(node: roxmltree::Node<'a, 'a>) -> impl Iterator<Item = &'a str> {
    node.children()
        .filter(|child| child.has_tag_name("Pieces"))
        .filter_map(|child| child.text())
        .map(|text| text.trim().rsplit(':').next().unwrap_or_default())
}

fn screen_id<'a>(node: roxmltree::Node<'a, 'a>) -> Option<&'a str> {
    node.children()
        .find(|child| child.has_tag_name("ScreenID"))?
        .text()
        .map(str::trim)
}

fn area(node: roxmltree::Node<'_, '_>) -> Option<Area> {
    Some(Area {
        x: number(node, &["Location", "X"])?,
        y: number(node, &["Location", "Y"])?,
        width: number(node, &["Size", "CX"])?,
        height: number(node, &["Size", "CY"])?,
    })
}

/// The finite number in the text of the element reached by `path` below `node`.
fn number(node: roxmltree::Node<'_, '_>, path: &[&str]) -> Option<f32> {
    let mut node = node;
    for name in path {
        node = node.children().find(|child| child.has_tag_name(*name))?;
    }
    node.text()?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A skin name that stays inside the UI directory.
fn valid_skin(skin: &str) -> bool {
    !skin.is_empty()
        && skin
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
}

/// A character or world name that keeps a file name inside the installation.
pub(crate) fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '/' | '\\' | ':'))
}

/// The value of `key` in `section` of an INI text, compared without case.
fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            current = name.trim();
        } else if current.eq_ignore_ascii_case(section)
            && let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case(key)
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colors_page_keeps_each_kinds_colour_whole() {
        let colors = text_colors_from_ini(
            "[Defaults]\nUser_1_Red=9\n[TextColors]\nUser_1_Red=255\nUser_1_Green=10\n\
             User_1_Blue=0\nUser_2_Red=1\nUser_2_Green=2\nUser_80_Red=7\nUser_80_Green=8\n\
             User_80_Blue=9\n[Next]\nUser_3_Red=1\n",
        );
        // Say is 256, and System 335; a colour missing a part is none.
        assert_eq!(colors, [(256, [255, 10, 0]), (335, [7, 8, 9])]);
    }

    #[test]
    fn a_windows_section_gives_its_background_and_fading() {
        let looks = looks_from_ini(
            "[Main]\nUISkin=default\n[MainChat]\nXPos1280x720=10\nAlpha=230\nFades=false\n\
             FadeToAlpha=200\nDelay=1000\nDuration=250\nBGType=2\nBGTint.red=0\n\
             BGTint.green=0\nBGTint.blue=0\n[PlayerWindow]\nAlpha=255\nBGType=1\n",
        );
        assert_eq!(
            looks,
            [
                WindowLook {
                    window: "MainChat".into(),
                    alpha: 230,
                    fades: false,
                    fade_to_alpha: 200,
                    delay_ms: 1000,
                    duration_ms: 250,
                    flat: true,
                    tint: [0, 0, 0],
                },
                // What a window does not say is the client's default.
                WindowLook::new("PlayerWindow"),
            ]
        );
    }

    #[test]
    fn eqclient_ini_says_whether_the_chat_is_logged() {
        let log = |text| options_from_ini(text).log;
        assert_eq!(
            log("[Defaults]
Sound=TRUE
Log=TRUE
"),
            Some(true)
        );
        assert_eq!(
            log("[defaults]
log = false
"),
            Some(false)
        );
        // Another section's Log, or none at all, says nothing.
        assert_eq!(
            log("[Other]
Log=TRUE
"),
            None
        );
        assert_eq!(
            log("[Defaults]
Log=maybe
"),
            None
        );
    }

    #[test]
    fn eqclient_ini_says_whether_sounds_play() {
        let sound = |text| options_from_ini(text).sound;
        assert_eq!(sound("[Defaults]\nSound=TRUE\nSoundVolume=0\n"), Some(true));
        assert_eq!(sound("[Defaults]\nSound=FALSE\n"), Some(false));
        assert_eq!(sound("[Options]\nSound=TRUE\n"), None);
    }

    #[test]
    fn eqclient_ini_says_whose_names_show_and_how_far_and_fast_the_view_goes() {
        assert_eq!(
            options_from_ini(
                "[Defaults]
ShowNamesLevel=4
[Options]
PCNames=1
NPCNames=0
ClipPlane=15
MaxFPS=100
MouseSensitivity=4
"
            ),
            OfficialOptions {
                log: None,
                sound: None,
                pc_names: Some(true),
                npc_names: Some(false),
                show_names_level: Some(4),
                clip_plane: Some(15),
                max_fps: Some(100),
                mouse_sensitivity: Some(4),
            }
        );
        assert_eq!(options_from_ini(""), OfficialOptions::default());
    }

    #[test]
    fn a_characters_ini_holds_the_first_page_of_hotbuttons() {
        let text = "[Socials]\nPage1Button1Name=Wave\n[HotButtons]\nPage1Button1=H0\nPage1Button3=J29\nPage2Button1=H4\nPage1Button10=\n";
        assert_eq!(
            hotbuttons_from_ini(text),
            [(1, "H0".to_owned()), (3, "J29".to_owned())]
        );
        assert_eq!(hotbuttons_from_ini("").len(), 0);
        assert_eq!(
            InstalledClient::Titanium
                .settings(Path::new("."))
                .hotbuttons("../x", "World")
                .len(),
            0
        );
    }

    #[test]
    fn only_a_checked_client_generation_has_its_settings_read() {
        let install = std::env::temp_dir().join(format!("eq-ui-generation-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("eqclient.ini"), "[Defaults]\nLog=TRUE\n").unwrap();
        std::fs::write(
            install.join("UI_Example_ExampleWorld.ini"),
            "[Main]\nUISkin=velious\n[PlayerWindow]\nXPos1280x720=10\nYPos1280x720=20\n",
        )
        .unwrap();
        std::fs::write(
            install.join("Example_ExampleWorld.ini"),
            "[HotButtons]\nPage1Button1=H0\n",
        )
        .unwrap();
        std::fs::write(
            install.join("eqlsPlayerData.ini"),
            "[MISC]\nLastServerID=1\nLastServerName=ExampleWorld\n[PLAYER]\nUsername=example\n",
        )
        .unwrap();
        std::fs::write(
            install.join("eqhost.txt"),
            "[LoginServer]\nHost=localhost:5998\n",
        )
        .unwrap();
        let read = |client: InstalledClient| {
            let settings = client.settings(&install);
            (
                settings.options(),
                settings.chosen_skin("Example", "ExampleWorld"),
                settings.hotbuttons("Example", "ExampleWorld").len(),
                settings.window_positions("Example", "ExampleWorld").len(),
                settings.login(),
            )
        };
        let titanium = read(InstalledClient::Titanium);
        let eqmac = read(InstalledClient::EqMac);
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(titanium.0.log, Some(true));
        assert_eq!(
            (titanium.1.as_deref(), titanium.2, titanium.3),
            (Some("velious"), 1, 1)
        );
        assert_eq!(
            titanium.4,
            LoginMemory {
                server: Some("localhost:5998".into()),
                account: Some("example".into()),
                world: Some("ExampleWorld".into()),
            }
        );
        // The same files in an EQMac installation are left unread.
        assert_eq!(
            eqmac,
            (
                OfficialOptions::default(),
                None,
                0,
                0,
                LoginMemory::default()
            )
        );
    }

    #[test]
    fn window_positions_pair_left_and_top_edges_by_screen_size() {
        let positions = positions_from_ini(
            "[Main]\nUISkin=default\n[PlayerWindow]\nXPos2560x1600=2022\nYPos2560x1600=2\n\
             XPos1280x720=600\n[TargetWindow]\nXPos1280x720=500\nYPos1280x720=16\nAlpha=255\n",
        );
        assert_eq!(
            positions,
            [
                WindowPosition {
                    window: "PlayerWindow".into(),
                    screen: (2560, 1600),
                    x: 2022,
                    y: 2,
                },
                WindowPosition {
                    window: "TargetWindow".into(),
                    screen: (1280, 720),
                    x: 500,
                    y: 16,
                },
            ]
        );
    }

    /// A synthetic window: slot `n` at (10 + 50n, 5 + 7n), 40 by 40, shown by one
    /// page with a general inventory slot and an unrelated element to ignore.
    fn window(slots: impl Iterator<Item = u16> + Clone) -> String {
        use std::fmt::Write;
        let mut xml = String::from("<?xml version=\"1.0\"?><XML><Schema xmlns=\"EverQuestData\"/>");
        for slot in slots.clone() {
            write!(
                xml,
                "<InvSlot item=\"S{slot}\"><Location><X>{}</X><Y>{}</Y></Location>\
                 <Size><CX>40</CX><CY>40</CY></Size><EQType>{slot}</EQType></InvSlot>",
                10 + 50 * u32::from(slot),
                5 + 7 * u32::from(slot)
            )
            .unwrap();
        }
        xml.push_str(
            "<InvSlot item=\"G\"><Location><X>1</X><Y>1</Y></Location>\
             <Size><CX>40</CX><CY>40</CY></Size><EQType>22</EQType></InvSlot>\
             <Label item=\"L\"><EQType>3</EQType></Label><Page item=\"P\">",
        );
        for slot in slots {
            write!(xml, "<Pieces>S{slot}</Pieces>").unwrap();
        }
        xml.push_str("<Pieces>G</Pieces><Pieces>L</Pieces></Page></XML>");
        xml
    }

    /// Adds a character view with class art at (100, 60) + (3, 11), 75 by 142.
    fn with_character(xml: &str, art: bool) -> String {
        let art_piece = if art {
            "<Pieces>ClassAnim</Pieces>"
        } else {
            ""
        };
        xml.replacen(
            "<Page item=\"P\">",
            &format!(
                "<StaticAnimation item=\"ClassAnim\"><ScreenID>ClassAnim</ScreenID>\
                 <Location><X>3</X><Y>11</Y></Location><Size><CX>75</CX><CY>142</CY></Size>\
                 </StaticAnimation><Screen item=\"IW_CharacterView\">\
                 <ScreenID>IW_CharacterView</ScreenID><Location><X>100</X><Y>60</Y></Location>\
                 <Size><CX>85</CX><CY>168</CY></Size>{art_piece}</Screen>\
                 <Page item=\"P\"><Pieces>Screen:IW_CharacterView</Pieces>"
            ),
            1,
        )
    }

    #[test]
    fn shown_equipment_slots_are_placed_relative_to_the_group() {
        let layout = equipment_from_xml(&window(0..=21)).unwrap();
        assert_eq!(layout.slots.len(), 22);
        assert_eq!(
            layout.slots[0],
            SlotPlacement {
                slot: 0,
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0
            }
        );
        assert_eq!((layout.slots[21].x, layout.slots[21].y), (1050.0, 147.0));
        assert_eq!(layout.character, None);
    }

    #[test]
    fn skins_may_leave_slots_out_and_define_slots_they_never_show() {
        let layout = equipment_from_xml(&window(1..=21)).unwrap();
        assert_eq!(layout.slots.first().map(|p| p.slot), Some(1));
        // Defined but listed by no screen: the official client never draws it.
        let unshown = window(1..=21).replacen(
            "<Label",
            "<InvSlot item=\"S0\"><Location><X>0</X><Y>0</Y></Location>\
             <Size><CX>40</CX><CY>40</CY></Size><EQType>0</EQType></InvSlot><Label",
            1,
        );
        assert_eq!(equipment_from_xml(&unshown).unwrap().slots.len(), 21);
    }

    #[test]
    fn the_character_goes_where_the_skin_draws_its_class_art() {
        let layout = equipment_from_xml(&with_character(&window(0..=21), true)).unwrap();
        // Slot 0 sits at (10, 5), so the group's origin moves there.
        assert_eq!(
            layout.character,
            Some(Area {
                x: 93.0,
                y: 66.0,
                width: 75.0,
                height: 142.0
            })
        );
        // A character view without its art (as in the classic velious skin) draws none.
        let blank = equipment_from_xml(&with_character(&window(0..=21), false)).unwrap();
        assert_eq!(blank.character, None);
    }

    #[test]
    fn duplicated_unsized_or_missing_equipment_is_refused() {
        assert!(matches!(
            equipment_from_xml(&window((0..=21).chain([5]))),
            Err(UiLayoutError::DuplicateSlot(5))
        ));
        let zero_width = window(0..=21).replacen("<CX>40</CX>", "<CX>0</CX>", 1);
        assert!(matches!(
            equipment_from_xml(&zero_width),
            Err(UiLayoutError::Geometry(0))
        ));
        assert!(matches!(
            equipment_from_xml(&window(std::iter::empty())),
            Err(UiLayoutError::NoEquipment)
        ));
        assert!(matches!(
            equipment_from_xml("<XML><InvSlot>"),
            Err(UiLayoutError::Xml(_))
        ));
        assert!(matches!(
            equipment_layout(Path::new("."), "../default"),
            Err(UiLayoutError::InvalidSkin(_))
        ));
    }

    #[test]
    fn a_skin_without_its_own_inventory_file_uses_the_default_one() {
        let install = std::env::temp_dir().join(format!("eq-ui-fallback-{}", std::process::id()));
        std::fs::create_dir_all(install.join("uifiles/default")).unwrap();
        std::fs::create_dir_all(install.join("uifiles/sparse")).unwrap();
        std::fs::write(
            install.join("uifiles/default/EQUI_Inventory.xml"),
            window(0..=21),
        )
        .unwrap();
        let layout = equipment_layout(&install, "sparse");
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(layout.unwrap().slots.len(), 22);
    }

    #[test]
    fn the_loading_screen_is_titaniums_picture_and_line_and_none_on_the_mac_client() {
        assert_eq!(InstalledClient::EqMac.loading_screen(), None);
        let screen = InstalledClient::Titanium.loading_screen().unwrap();
        assert_eq!(screen.status, 12387);
        let install = std::env::temp_dir().join(format!("eq-ui-loading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        assert!(matches!(
            screen.read_picture(&install),
            Err(UiLayoutError::Image { .. })
        ));
        std::fs::create_dir_all(install.join("Resources")).unwrap();
        image::RgbImage::new(64, 48)
            .save(install.join("Resources/loadscreen.jpg"))
            .unwrap();
        let picture = screen
            .read_picture(&install)
            .map(|pixels| pixels.dimensions());
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(picture.unwrap(), (64, 48));
    }

    #[test]
    fn icon_sheets_come_from_the_skin_then_the_default_one() {
        let install = std::env::temp_dir().join(format!("eq-ui-sheets-{}", std::process::id()));
        std::fs::create_dir_all(install.join("uifiles/default")).unwrap();
        std::fs::create_dir_all(install.join("uifiles/painted")).unwrap();
        let sheet = |red| {
            image::RgbaImage::from_pixel(SHEET_SIZE, SHEET_SIZE, image::Rgba([red, 0, 0, 255]))
        };
        sheet(1)
            .save(install.join("uifiles/default/dragitem1.tga"))
            .unwrap();
        sheet(2)
            .save(install.join("uifiles/default/spells01.tga"))
            .unwrap();
        sheet(3)
            .save(install.join("uifiles/painted/dragitem1.tga"))
            .unwrap();
        image::RgbaImage::new(8, 8)
            .save(install.join("uifiles/default/dragitem2.tga"))
            .unwrap();
        let red = |skin, file| {
            texture_sheet(&install, skin, file).map(|pixels| pixels.get_pixel(0, 0).0[0])
        };
        let own = red("painted", "dragitem1.tga");
        let fallback = red("painted", "spells01.tga");
        let small = red("default", "dragitem2.tga");
        let escaping = red("painted", "..");
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!((own.unwrap(), fallback.unwrap()), (3, 2));
        assert!(matches!(small, Err(UiLayoutError::SheetSize { .. })));
        assert!(matches!(escaping, Err(UiLayoutError::InvalidFile(_))));
    }

    #[test]
    fn the_chosen_skin_comes_from_the_characters_ui_settings() {
        let install = std::env::temp_dir().join(format!("eq-ui-choice-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(
            install.join("UI_Example_ExampleWorld.ini"),
            "[ChatWindow]\r\nUISkin=wrong\r\n[MAIN]\r\n uiskin = velious \r\n",
        )
        .unwrap();
        std::fs::write(
            install.join("UI_Other_ExampleWorld.ini"),
            "[Main]\nUISkin=../x\n",
        )
        .unwrap();
        let settings = InstalledClient::Titanium.settings(&install);
        let chosen = settings.chosen_skin("Example", "ExampleWorld");
        let unsafe_skin = settings.chosen_skin("Other", "ExampleWorld");
        let missing = settings.chosen_skin("Nobody", "ExampleWorld");
        let escaping = settings.chosen_skin("../Example", "ExampleWorld");
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!(chosen.as_deref(), Some("velious"));
        assert_eq!((unsafe_skin, missing, escaping), (None, None, None));
    }

    /// A picture in a Windows icon file: its side, the two words of its
    /// directory entry that a cursor keeps its hotspot in and an icon its
    /// planes and bits per pixel, and its colour by position.
    struct Picture {
        side: u8,
        words: (u16, u16),
        colour: fn(u32, u32) -> [u8; 4],
    }

    /// A Windows icon file of `kind`, 1 for an icon and 2 for a cursor, with
    /// each picture a bitmap: a header of twice its height, its 32-bit BGRA
    /// rows and its 1-bit transparency mask, each from the bottom up.
    fn icon_file(kind: u16, pictures: &[Picture]) -> Vec<u8> {
        let count = u16::try_from(pictures.len()).unwrap();
        let mut file = [0, kind, count]
            .iter()
            .flat_map(|word: &u16| word.to_le_bytes())
            .collect::<Vec<_>>();
        let mut offset = 6 + 16 * u32::from(count);
        let mut bitmaps = Vec::new();
        for picture in pictures {
            let side = u32::from(picture.side);
            let mask_row = (side.div_ceil(8)).div_ceil(4) * 4;
            let mut bitmap = Vec::new();
            for word in [40, side, 2 * side] {
                bitmap.extend(word.to_le_bytes());
            }
            bitmap.extend(1_u16.to_le_bytes());
            bitmap.extend(32_u16.to_le_bytes());
            bitmap.extend([0; 24]);
            for y in (0..side).rev() {
                for x in 0..side {
                    let [r, g, b, a] = (picture.colour)(x, y);
                    bitmap.extend([b, g, r, a]);
                }
            }
            for y in (0..side).rev() {
                let mut row = vec![0_u8; usize::try_from(mask_row).unwrap()];
                for x in 0..side {
                    if (picture.colour)(x, y)[3] == 0 {
                        row[usize::try_from(x / 8).unwrap()] |= 0x80 >> (x % 8);
                    }
                }
                bitmap.extend(row);
            }
            let length = u32::try_from(bitmap.len()).unwrap();
            file.extend([picture.side, picture.side, 0, 0]);
            file.extend(picture.words.0.to_le_bytes());
            file.extend(picture.words.1.to_le_bytes());
            file.extend(length.to_le_bytes());
            file.extend(offset.to_le_bytes());
            offset += length;
            bitmaps.extend(bitmap);
        }
        file.extend(bitmaps);
        file
    }

    /// A cursor's large picture: shaded, and clear below its diagonal.
    fn large(x: u32, y: u32) -> [u8; 4] {
        if x + y > 40 {
            [0, 0, 0, 0]
        } else {
            [
                u8::try_from(x * 8).unwrap(),
                u8::try_from(y * 8).unwrap(),
                100,
                255,
            ]
        }
    }

    /// A cursor's small picture, all white.
    fn small(_: u32, _: u32) -> [u8; 4] {
        [255, 255, 255, 255]
    }

    /// Writes `bytes` as `file` of `skin` in a new installation.
    fn install_file(install: &Path, skin: &str, file: &str, bytes: &[u8]) {
        std::fs::create_dir_all(install.join("uifiles").join(skin)).unwrap();
        std::fs::write(install.join("uifiles").join(skin).join(file), bytes).unwrap();
    }

    #[test]
    fn a_cursor_gives_the_picture_the_image_crate_picks_and_its_hotspot() {
        let install = std::env::temp_dir().join(format!("eq-ui-cursor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        let two = |small_hotspot| {
            icon_file(
                2,
                &[
                    Picture {
                        side: 16,
                        words: small_hotspot,
                        colour: small,
                    },
                    Picture {
                        side: 32,
                        words: (3, 5),
                        colour: large,
                    },
                ],
            )
        };
        install_file(&install, DEFAULT_SKIN, "arrow.cur", &two((1, 2)));
        // The image crate scores a cursor's hotspot y as an icon's bits per
        // pixel, so a small picture with a lower hotspot wins the pick.
        install_file(&install, DEFAULT_SKIN, "low.cur", &two((1, 9)));
        let read = cursor(&install, DEFAULT_SKIN, "arrow.cur");
        let low = cursor(&install, DEFAULT_SKIN, "low.cur");
        std::fs::remove_dir_all(&install).unwrap();
        let read = read.unwrap();
        assert_eq!(read.pixels.dimensions(), (32, 32));
        assert_eq!(read.hotspot, (3, 5));
        assert_eq!(read.pixels.get_pixel(0, 0).0, [0, 0, 100, 255]);
        assert_eq!(read.pixels.get_pixel(2, 30).0, [16, 240, 100, 255]);
        assert_eq!(read.pixels.get_pixel(31, 0).0, [248, 0, 100, 255]);
        assert_eq!(read.pixels.get_pixel(31, 31).0[3], 0);
        let low = low.unwrap();
        assert_eq!((low.pixels.dimensions(), low.hotspot), ((16, 16), (1, 9)));
        assert_eq!(low.pixels.get_pixel(8, 8).0, [255, 255, 255, 255]);
    }

    #[test]
    fn a_skin_without_its_own_cursor_uses_the_default_ones() {
        let install =
            std::env::temp_dir().join(format!("eq-ui-cursor-skin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        let one = |hotspot| {
            icon_file(
                2,
                &[Picture {
                    side: 16,
                    words: hotspot,
                    colour: small,
                }],
            )
        };
        install_file(&install, DEFAULT_SKIN, "arrow.cur", &one((1, 1)));
        std::fs::create_dir_all(install.join("uifiles/plain")).unwrap();
        let fallback = cursor(&install, "plain", "arrow.cur").map(|cursor| cursor.hotspot);
        install_file(&install, "plain", "arrow.cur", &one((7, 4)));
        let own = cursor(&install, "plain", "arrow.cur").map(|cursor| cursor.hotspot);
        let missing = cursor(&install, "plain", "hand.cur");
        std::fs::remove_dir_all(&install).unwrap();
        assert_eq!((fallback.unwrap(), own.unwrap()), ((1, 1), (7, 4)));
        assert!(matches!(missing, Err(UiLayoutError::Read { .. })));
    }

    #[test]
    fn an_icon_is_not_read_as_a_cursor() {
        let install =
            std::env::temp_dir().join(format!("eq-ui-cursor-icon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        let icon = icon_file(
            1,
            &[Picture {
                side: 16,
                words: (1, 32),
                colour: small,
            }],
        );
        install_file(&install, DEFAULT_SKIN, "arrow.cur", &icon);
        let read = cursor(&install, DEFAULT_SKIN, "arrow.cur");
        std::fs::remove_dir_all(&install).unwrap();
        assert!(matches!(read, Err(UiLayoutError::NotCursor(_))));
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn reads_the_installed_skin() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let skin = std::env::var("EQ_PROBE_SKIN").unwrap_or_else(|_| DEFAULT_SKIN.into());
        let layout = equipment_layout(Path::new(&install), &skin).unwrap();
        for placement in &layout.slots {
            println!("{placement:?}");
        }
        println!("character: {:?}", layout.character);
    }
}

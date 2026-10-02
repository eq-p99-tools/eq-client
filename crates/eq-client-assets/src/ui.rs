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
    /// Windows' fonts; None until it is checked for this client.
    #[must_use]
    pub const fn font_file(self) -> Option<&'static str> {
        match self {
            Self::Titanium => Some("arial.ttf"),
            Self::EqMac => None,
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
fn plain_name(name: &str) -> bool {
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
        let read = |client: InstalledClient| {
            let settings = client.settings(&install);
            (
                settings.options(),
                settings.chosen_skin("Example", "ExampleWorld"),
                settings.hotbuttons("Example", "ExampleWorld").len(),
                settings.window_positions("Example", "ExampleWorld").len(),
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
        // The same files in an EQMac installation are left unread.
        assert_eq!(eqmac, (OfficialOptions::default(), None, 0, 0));
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

//! Window layouts from the installed client's UI skin, read at runtime so none of
//! the skin's data is copied into this project and a custom skin carries over.
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Titanium equipment slots, from charm to ammo.
const EQUIPMENT: std::ops::RangeInclusive<u16> = 0..=21;

/// Where an equipment slot sits in a skin's inventory window, in that window's
/// pixels, relative to the top-left of the equipment slots as a group.
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
    /// An equipment slot is placed twice, or not at all.
    #[error("the inventory window places equipment slot {slot} {count} times")]
    SlotCount {
        /// Titanium slot.
        slot: u16,
        /// How many placements it has.
        count: usize,
    },
    /// A slot's location or size is missing or not a number.
    #[error("equipment slot {0} has no usable location or size")]
    Geometry(u16),
}

/// Reads the equipment slots of `uifiles/<skin>/EQUI_Inventory.xml`.
///
/// # Errors
/// Rejects unsafe skin names, unreadable or malformed files, and layouts that do
/// not place every equipment slot exactly once with a positive size.
pub fn equipment_layout(
    eq_directory: &Path,
    skin: &str,
) -> Result<Vec<SlotPlacement>, UiLayoutError> {
    if skin.is_empty()
        || !skin
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
    {
        return Err(UiLayoutError::InvalidSkin(skin.to_owned()));
    }
    let path = eq_directory
        .join("uifiles")
        .join(skin)
        .join("EQUI_Inventory.xml");
    let bytes = std::fs::read(&path).map_err(|source| UiLayoutError::Read { path, source })?;
    // Skins declare an ASCII encoding, but hand-edited ones may carry Latin-1 text.
    equipment_from_xml(&String::from_utf8_lossy(&bytes))
}

fn equipment_from_xml(text: &str) -> Result<Vec<SlotPlacement>, UiLayoutError> {
    let document = roxmltree::Document::parse(text)?;
    let mut placements: BTreeMap<u16, Vec<SlotPlacement>> = BTreeMap::new();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("InvSlot"))
    {
        let Some(slot) = number(node, &["EQType"]).and_then(|value| {
            // Equipment types are small whole numbers.
            (value.fract() == 0.0 && value >= 0.0 && value <= f32::from(u16::MAX)).then_some(value)
        }) else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked above.
        let slot = slot as u16;
        if !EQUIPMENT.contains(&slot) {
            continue;
        }
        let geometry = [
            number(node, &["Location", "X"]),
            number(node, &["Location", "Y"]),
            number(node, &["Size", "CX"]),
            number(node, &["Size", "CY"]),
        ];
        let [Some(x), Some(y), Some(width), Some(height)] = geometry else {
            return Err(UiLayoutError::Geometry(slot));
        };
        if width <= 0.0 || height <= 0.0 {
            return Err(UiLayoutError::Geometry(slot));
        }
        placements.entry(slot).or_default().push(SlotPlacement {
            slot,
            x,
            y,
            width,
            height,
        });
    }
    let mut layout = Vec::new();
    for slot in EQUIPMENT {
        match placements.remove(&slot).unwrap_or_default().as_slice() {
            [placement] => layout.push(*placement),
            others => {
                return Err(UiLayoutError::SlotCount {
                    slot,
                    count: others.len(),
                });
            }
        }
    }
    let left = layout.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let top = layout.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    for placement in &mut layout {
        placement.x -= left;
        placement.y -= top;
    }
    Ok(layout)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic window: slot `n` at (10 + 50n, 5 + 7n), 40 by 40, plus a
    /// general inventory slot and an unrelated element that must be ignored.
    fn window(slots: impl Iterator<Item = u16>) -> String {
        use std::fmt::Write;
        let mut xml = String::from("<?xml version=\"1.0\"?><XML><Schema xmlns=\"EverQuestData\"/>");
        for slot in slots {
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
             <Label item=\"L\"><EQType>3</EQType></Label></XML>",
        );
        xml
    }

    #[test]
    fn every_equipment_slot_is_placed_relative_to_the_group() {
        let layout = equipment_from_xml(&window(0..=21)).unwrap();
        assert_eq!(layout.len(), 22);
        assert_eq!(
            layout[0],
            SlotPlacement {
                slot: 0,
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0
            }
        );
        assert_eq!((layout[21].x, layout[21].y), (1050.0, 147.0));
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn reads_the_installed_default_skin() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        for placement in equipment_layout(Path::new(&install), "default").unwrap() {
            println!("{placement:?}");
        }
    }

    #[test]
    fn missing_duplicated_or_unsized_slots_are_refused() {
        assert!(matches!(
            equipment_from_xml(&window(1..=21)),
            Err(UiLayoutError::SlotCount { slot: 0, count: 0 })
        ));
        assert!(matches!(
            equipment_from_xml(&window((0..=21).chain([5]))),
            Err(UiLayoutError::SlotCount { slot: 5, count: 2 })
        ));
        let zero_width = window(0..=21).replacen("<CX>40</CX>", "<CX>0</CX>", 1);
        assert!(matches!(
            equipment_from_xml(&zero_width),
            Err(UiLayoutError::Geometry(0))
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
}

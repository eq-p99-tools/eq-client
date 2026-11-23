//! The installed classic zone's placed point lights, and the installed
//! program's day ambient table.
use glam::Vec3;
use libeq::wld::parser::{PointLight, WldDoc};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// A resolved WLD 0x28 -> 0x1c -> 0x1b chain, in renderer coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedLight {
    /// Position in the zone.
    pub position: [f32; 3],
    /// Authored radius in zone units.
    pub radius: f32,
    /// First authored RGB frame (or white for an explicitly colorless light).
    pub rgb: [f32; 3],
    /// First authored brightness frame.
    pub level: f32,
}

pub(crate) fn placed(doc: &WldDoc) -> Vec<PlacedLight> {
    doc.fragment_iter::<PointLight>()
        .filter_map(|point| {
            let reference = doc.get(&point.reference)?;
            let light = doc.get(&reference.reference)?;
            let level = *light.light_levels.as_ref()?.first()?;
            let rgb = match &light.colors {
                Some(colors) => {
                    let &(red, green, blue) = colors.first()?;
                    [red, green, blue]
                }
                None => [1.0; 3],
            };
            let position = eq_client_axes::from_wld(Vec3::new(point.x, point.y, point.z));
            if !position.is_finite()
                || !point.radius.is_finite()
                || point.radius <= 0.0
                || !level.is_finite()
                || level <= 0.0
                || !rgb
                    .iter()
                    .all(|channel| channel.is_finite() && *channel >= 0.0)
            {
                return None;
            }
            Some(PlacedLight {
                position: position.to_array(),
                radius: point.radius,
                rgb,
                level,
            })
        })
        .collect()
}

/// eqgame.exe's day ambient, one RGB row an hour from 12 AM, which the
/// zone ambient follows in zones of time type 1, 2 or 5 (0x441295).
pub type DayAmbient = [[f32; 3]; 24];

/// The Titanium program's three columns of the table, 24 floats each.
const DAY_AMBIENT_COLUMNS: [u32; 3] = [0x0078_f708, 0x0078_f768, 0x0078_f7c8];

/// The installed Titanium program's day ambient table, read from its
/// eqgame.exe once per installation; None where the program doesn't hold
/// a plausible one there (another client, or another build).
#[must_use]
pub fn day_ambient(eq_directory: &Path) -> Option<DayAmbient> {
    static READ: OnceLock<Mutex<HashMap<PathBuf, Option<DayAmbient>>>> = OnceLock::new();
    let read = READ.get_or_init(Mutex::default);
    if let Some(known) = read.lock().ok()?.get(eq_directory) {
        return *known;
    }
    let table = std::fs::read(eq_directory.join("eqgame.exe"))
        .ok()
        .and_then(|bytes| day_ambient_from_pe(&bytes));
    read.lock().ok()?.insert(eq_directory.to_owned(), table);
    table
}

fn day_ambient_from_pe(bytes: &[u8]) -> Option<DayAmbient> {
    let mut table = [[0.0_f32; 3]; 24];
    for (channel, address) in DAY_AMBIENT_COLUMNS.into_iter().enumerate() {
        let column = crate::pe::data(bytes, address, 96)?;
        for (row, value) in column.as_chunks::<4>().0.iter().enumerate() {
            let value = f32::from_le_bytes(*value);
            if !(0.0..=1.0).contains(&value) {
                return None;
            }
            table[row][channel] = value;
        }
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_ambient_table_is_read_from_the_program_and_checked() {
        let mut columns = Vec::new();
        for channel in 0..3_u8 {
            for row in 0..24_u8 {
                columns.extend((f32::from(row) / 24.0 + f32::from(channel) / 100.0).to_le_bytes());
            }
        }
        let bytes = crate::pe::tests::image(0x0018_f708, &columns);
        let table = day_ambient_from_pe(&bytes).unwrap();
        assert_eq!(table[0], [0.0, 0.01, 0.02]);
        assert_eq!(table[12][0], 0.5);
        // A value out of range means this isn't the table.
        let mut wrong = columns.clone();
        wrong[..4].copy_from_slice(&2.0_f32.to_le_bytes());
        assert!(day_ambient_from_pe(&crate::pe::tests::image(0x0018_f708, &wrong)).is_none());
        assert!(day_ambient_from_pe(&bytes[..bytes.len() - 1]).is_none());
    }
    use crate::test_world::{OLD_FORMAT, WorldFile};

    #[test]
    fn placed_lights_resolve_the_wld_chain_and_skip_unresolved_or_invalid_ones() {
        let mut file = WorldFile::new(OLD_FORMAT);
        let words = |words: &[u32]| words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let rgb = [1.0_f32, 0.5, 0.25];
        let source = file.fragment(
            0x1b,
            words(&[
                0,
                0x14,
                1,
                0.75_f32.to_bits(),
                rgb[0].to_bits(),
                rgb[1].to_bits(),
                rgb[2].to_bits(),
            ]),
        );
        let reference = file.fragment(0x1c, words(&[0, source.cast_unsigned(), 0]));
        for (link, radius) in [(reference, 30.0_f32), (reference, 0.0), (999, 30.0)] {
            file.fragment(
                0x28,
                words(&[
                    0,
                    link.cast_unsigned(),
                    0x100,
                    2.0_f32.to_bits(),
                    3.0_f32.to_bits(),
                    4.0_f32.to_bits(),
                    radius.to_bits(),
                ]),
            );
        }
        let doc = WldDoc::parse(&file.bytes()).unwrap();
        assert_eq!(
            placed(&doc),
            [PlacedLight {
                position: eq_client_axes::from_wld(Vec3::new(2.0, 3.0, 4.0)).to_array(),
                radius: 30.0,
                rgb,
                level: 0.75,
            }]
        );
    }
}

//! Static data read from the installed program (`eqgame.exe`), by the
//! program's own addresses, for the tables it keeps there rather than in
//! its data files. No values are kept here; each reader checks the shape
//! of what it found before handing it on.

use std::{fs, path::Path};

/// The bytes the installed 32-bit program keeps at `address` (an address
/// in the program as loaded), `length` long; None when the file is not
/// such a program or no section holds all of them.
#[must_use]
pub(crate) fn at(bytes: &[u8], address: u32, length: usize) -> Option<&[u8]> {
    let word = |at: usize| {
        Some(u16::from_le_bytes(
            bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let dword = |at: usize| {
        Some(u32::from_le_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = usize::try_from(dword(0x3c)?).ok()?;
    if bytes.get(pe..pe.checked_add(4)?)? != b"PE\0\0" || word(pe + 24)? != 0x10b {
        return None;
    }
    let rva = address.checked_sub(dword(pe + 52)?)?;
    let sections = pe.checked_add(24 + usize::from(word(pe + 20)?))?;
    let length32 = u32::try_from(length).ok()?;
    (0..usize::from(word(pe + 6)?)).find_map(|i| {
        let section = sections.checked_add(i.checked_mul(40)?)?;
        let offset = rva.checked_sub(dword(section + 12)?)?;
        if offset.checked_add(length32)? > dword(section + 16)? {
            return None;
        }
        let start = usize::try_from(dword(section + 20)?.checked_add(offset)?).ok()?;
        bytes.get(start..start.checked_add(length)?)
    })
}

/// How many languages the `EQMac` program's minimum-level table covers.
pub const LANGUAGES: usize = 25;
/// How many races it has a column for: 1 to 12, Iksar (128), Vah Shir
/// (130).
pub const RACES: usize = 14;

/// Where the TAKP `EQMac` program keeps its language minimum levels, which
/// its training gate checks (0x4ef555, read 6.597): a level for each
/// language, then for each race column, as 32-bit values.
const EQMAC_LANGUAGE_LEVELS: u32 = 0x0060_b0b0;

/// The `EQMac` program's language minimum levels from the installed
/// program in `directory`, by language and then race column; None when the
/// program is not one that keeps the table there, or what is there does
/// not look like it.
#[must_use]
pub fn installed_language_levels(directory: &Path) -> Option<[[u32; RACES]; LANGUAGES]> {
    language_levels(&fs::read(directory.join("eqgame.exe")).ok()?)
}

/// The table out of the program's bytes, if it has the program's shape:
/// every race speaks the first language from level 1, and no race learns
/// languages 10 or 23 below 99.
#[must_use]
pub fn language_levels(bytes: &[u8]) -> Option<[[u32; RACES]; LANGUAGES]> {
    let data = at(bytes, EQMAC_LANGUAGE_LEVELS, LANGUAGES * RACES * 4)?;
    let rows: [[u32; RACES]; LANGUAGES] = std::array::from_fn(|language| {
        std::array::from_fn(|race| {
            let start = (language * RACES + race) * 4;
            data.get(start..start + 4)
                .and_then(|cell| cell.try_into().ok())
                .map_or(0, u32::from_le_bytes)
        })
    });
    let all = |row: &[u32; RACES], value: u32| row.iter().all(|cell| *cell == value);
    (all(&rows[0], 1) && all(&rows[10], 99) && all(&rows[23], 99)).then_some(rows)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A made-up 32-bit program image with one section at `address`
    /// holding `data`: no bytes of any real program.
    pub(crate) fn image(address: u32, data: &[u8]) -> Vec<u8> {
        let base = 0x0040_0000_u32;
        let pe = 0x40_usize;
        let sections = pe + 24 + 0xe0;
        let raw = 0x400_usize;
        let mut bytes = vec![0; raw + data.len()];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&u32::try_from(pe).unwrap().to_le_bytes());
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        bytes[pe + 20..pe + 22].copy_from_slice(&0xe0u16.to_le_bytes());
        bytes[pe + 24..pe + 26].copy_from_slice(&0x10bu16.to_le_bytes());
        bytes[pe + 52..pe + 56].copy_from_slice(&base.to_le_bytes());
        let size = u32::try_from(data.len()).unwrap();
        bytes[sections + 12..sections + 16].copy_from_slice(&(address - base).to_le_bytes());
        bytes[sections + 16..sections + 20].copy_from_slice(&size.to_le_bytes());
        bytes[sections + 20..sections + 24]
            .copy_from_slice(&u32::try_from(raw).unwrap().to_le_bytes());
        bytes[raw..].copy_from_slice(data);
        bytes
    }

    /// A made-up table of the right shape: 1s for the first language, 99s
    /// for 10 and 23, and each other cell its own made-up number.
    fn made_up_table() -> Vec<u8> {
        let mut data = Vec::new();
        for language in 0..LANGUAGES {
            for race in 0..RACES {
                let cell = match language {
                    0 => 1,
                    10 | 23 => 99,
                    _ => u32::try_from(language * 2 + race).unwrap(),
                };
                data.extend_from_slice(&cell.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn the_language_table_is_read_by_language_then_race() {
        let rows = language_levels(&image(EQMAC_LANGUAGE_LEVELS, &made_up_table())).unwrap();
        assert_eq!(rows[0], [1; RACES]);
        assert_eq!(rows[5][0], 10);
        assert_eq!(rows[5][13], 23);
        assert_eq!(rows[24][12], 60);
    }

    #[test]
    fn a_table_without_the_programs_shape_is_not_taken() {
        let mut data = made_up_table();
        data[10 * RACES * 4] = 98;
        assert_eq!(language_levels(&image(EQMAC_LANGUAGE_LEVELS, &data)), None);
        // Elsewhere in the program, or not a program at all.
        assert_eq!(language_levels(&image(0x0060_0000, &made_up_table())), None);
        assert_eq!(language_levels(&made_up_table()), None);
        // Too short for the whole table.
        let short = image(EQMAC_LANGUAGE_LEVELS, &made_up_table()[..1000]);
        assert_eq!(language_levels(&short), None);
    }
}

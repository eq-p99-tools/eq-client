//! Reading data at a virtual address of an installed 32-bit Windows
//! program, such as a table in eqgame.exe, from the file alone.

/// The `length` bytes at virtual address `address` of the PE32 image in
/// `bytes`, when one section holds them all in the file.
pub(crate) fn data(bytes: &[u8], address: u32, length: usize) -> Option<&[u8]> {
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
    let length_word = u32::try_from(length).ok()?;
    (0..usize::from(word(pe + 6)?)).find_map(|i| {
        let section = sections.checked_add(i.checked_mul(40)?)?;
        let offset = rva.checked_sub(dword(section + 12)?)?;
        if offset.checked_add(length_word)? > dword(section + 16)? {
            return None;
        }
        let at = usize::try_from(dword(section + 20)?.checked_add(offset)?).ok()?;
        bytes.get(at..at.checked_add(length)?)
    })
}

#[cfg(test)]
pub(crate) mod tests {
    /// A PE32 image with base `0x0060_0000` and one section whose data, at
    /// file offset 512, starts at `rva` and holds `data`.
    pub(crate) fn image(rva: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[134..136].copy_from_slice(&1u16.to_le_bytes());
        bytes[148..150].copy_from_slice(&224u16.to_le_bytes());
        bytes[152..154].copy_from_slice(&0x10bu16.to_le_bytes());
        bytes[180..184].copy_from_slice(&0x0060_0000u32.to_le_bytes());
        bytes[388..392].copy_from_slice(&rva.to_le_bytes());
        bytes[392..396].copy_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
        bytes[396..400].copy_from_slice(&512u32.to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn data_is_found_by_address_only_inside_its_section() {
        let bytes = image(0x1000, &[1, 2, 3, 4]);
        assert_eq!(super::data(&bytes, 0x0060_1001, 2), Some(&[2, 3][..]));
        assert_eq!(super::data(&bytes, 0x0060_1002, 3), None);
        assert_eq!(super::data(&bytes, 0x0050_0000, 1), None);
        for length in 0..bytes.len() {
            assert_eq!(super::data(&bytes[..length], 0x0060_1000, 4), None);
        }
    }
}

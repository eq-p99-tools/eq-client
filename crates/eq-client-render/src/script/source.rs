//! Bounded reads shared by CLI, follow mode and the workbench.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Read a UTF-8 scenario without allocating beyond the source size limit.
///
/// # Errors
/// Returns an I/O error, invalid UTF-8, or a source-size limit violation.
pub fn read_source(path: &Path) -> io::Result<String> {
    String::from_utf8(read_bytes(path)?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Script must be UTF-8 text"))
}

pub(super) fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(super) fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(super::MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > super::MAX_SOURCE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Script exceeds {} bytes", super::MAX_SOURCE_BYTES),
        ));
    }
    Ok(bytes)
}

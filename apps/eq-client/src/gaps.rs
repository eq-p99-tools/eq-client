//! The session's gap file on disk, and the report over the saved ones
//! (`--gap-report`): see [`eq_client_core::gaps`].

use eq_client_core::gaps::{self, GapFile, NetworkGap};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The folder in the settings directory the gap files live in.
pub const FOLDER: &str = "gaps";

/// A session's file as JSON.
///
/// # Errors
/// Returns an error when the file cannot be serialized.
pub fn to_json(file: &GapFile) -> io::Result<String> {
    serde_json::to_string_pretty(file).map_err(io::Error::other)
}

/// Reads a gap the network library tells of from its own serialization,
/// so the client follows the library's record without naming its type.
pub fn network_gap<T: serde::Serialize + ?Sized>(gap: &T) -> Option<NetworkGap> {
    serde_json::to_value(gap)
        .and_then(serde_json::from_value)
        .inspect_err(|error| tracing::warn!("A gap record was not readable: {error}"))
        .ok()
}

/// Starts the session's gap file under `settings`, when there is a
/// settings directory; says where it will be written.
pub fn begin(settings: Option<&Path>) {
    let Some(settings) = settings else {
        tracing::warn!("Server message gaps are not recorded: no settings directory");
        return;
    };
    match gaps::begin(&settings.join(FOLDER), jiff::Timestamp::now()) {
        Ok(path) => tracing::info!("Server message gaps go to {}", path.display()),
        Err(error) => tracing::warn!("Server message gaps are not recorded: {error}"),
    }
}

/// Counts a network gap and writes the file when due.
pub fn record<T: serde::Serialize + ?Sized>(gap: &T) {
    if let Some(gap) = network_gap(gap) {
        gaps::network(gap);
    }
}

/// Writes the session's file when it is due.
pub fn write_when_due() {
    if let Err(error) = gaps::write_when_due(&to_json) {
        tracing::warn!("The gap file was not written: {error}");
    }
}

/// Writes the session's file a last time and ends it.
pub fn end() {
    if let Err(error) = gaps::end(&to_json) {
        tracing::warn!("The gap file was not written: {error}");
    }
}

/// The report over `paths` (files, or folders of them), or over the
/// settings directory's gap folder when none are named.
///
/// # Errors
/// Returns an error when no file can be found or one cannot be read.
pub fn report(paths: &[PathBuf], settings: Option<&Path>, samples: bool) -> Result<String, String> {
    let default;
    let paths = if paths.is_empty() {
        default = [settings
            .ok_or("--gap-report: no settings directory; name the gap files or folder")?
            .join(FOLDER)];
        &default[..]
    } else {
        paths
    };
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let listed = gaps::files_in(path)
                .map_err(|error| format!("--gap-report: {}: {error}", path.display()))?;
            files.extend(listed);
        } else {
            files.push(path.clone());
        }
    }
    if files.is_empty() {
        return Err("--gap-report: no gap files found".into());
    }
    let mut read = Vec::with_capacity(files.len());
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("--gap-report: {}: {error}", file.display()))?;
        read.push(
            serde_json::from_str::<GapFile>(&text)
                .map_err(|error| format!("--gap-report: {}: {error}", file.display()))?,
        );
    }
    Ok(gaps::report(&read, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record as eq-network's `Gap` serializes it.
    fn library_record() -> serde_json::Value {
        serde_json::json!({
            "stage": "zone",
            "dialect": "titanium",
            "server": "eq_emu",
            "zone": "qeynos",
            "opcode": 0x497c,
            "name": "OP_Action",
            "count": 3,
            "min_length": 31,
            "max_length": 31,
            "kind": {"type": "unknown_kind", "kind": 8},
            "sample": "00ff",
        })
    }

    #[test]
    fn the_library_s_record_reads_as_a_network_gap() {
        let gap = network_gap(&library_record()).unwrap();
        assert_eq!(gap.opcode, 0x497c);
        assert_eq!(gap.kind, gaps::Kind::UnknownKind { kind: 8 });
        assert_eq!(gap.sample.as_deref(), Some("00ff"));
        let mut newer = library_record();
        newer["kind"] = serde_json::json!({"type": "something_new", "x": 1});
        assert_eq!(network_gap(&newer).unwrap().kind, gaps::Kind::Other);
    }

    #[test]
    fn a_written_file_reads_back_into_the_report() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-gap-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut log = gaps::GapLog::new(jiff::Timestamp::UNIX_EPOCH);
        log.network(
            network_gap(&library_record()).unwrap(),
            jiff::Timestamp::UNIX_EPOCH,
        );
        let file = log.file();
        std::fs::write(
            directory.join("gaps-19700101T000000Z.json"),
            to_json(&file).unwrap(),
        )
        .unwrap();
        let plain = report(std::slice::from_ref(&directory), None, false).unwrap();
        assert!(plain.contains("| 3 | 1 | eq_emu | titanium | zone | 0x497c | OP_Action |"));
        assert!(!plain.contains("00ff"));
        assert!(
            report(std::slice::from_ref(&directory), None, true)
                .unwrap()
                .contains("`00ff`")
        );
        assert!(report(&[], None, false).is_err());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

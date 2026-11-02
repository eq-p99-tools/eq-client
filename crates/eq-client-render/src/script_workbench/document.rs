//! UTF-8 documents and bounded file operations, independent of the UI and runner.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::script::{MAX_SOURCE_BYTES, ParseError, Program};

pub(super) const EXAMPLE: &str = "# Run in your current game session\n# I uses your current key bindings\npress I\nwait 1000\npress I\n";

/// The editable buffer and the exact disk snapshot it was loaded from.
#[derive(Debug)]
pub(super) struct Document {
    pub text: String,
    pub path: Option<PathBuf>,
    saved: String,
    disk: Option<String>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new(EXAMPLE.to_owned(), None)
    }
}

impl Document {
    pub fn new(text: String, path: Option<PathBuf>) -> Self {
        Self {
            saved: text.clone(),
            text,
            path,
            disk: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    /// Compile the buffer without changing the current executor or disk snapshot.
    pub fn program(&self) -> Result<Program, ParseError> {
        Program::parse(
            &self.text,
            self.path
                .as_deref()
                .and_then(Path::parent)
                .unwrap_or(Path::new(".")),
        )
    }

    /// Prepare a save against the snapshot of this path, or a new-file save.
    pub fn save(&self, path: PathBuf) -> Save {
        Save {
            expected: if self.path.as_ref() == Some(&path) {
                self.disk.clone()
            } else {
                None
            },
            path,
            text: self.text.clone(),
        }
    }

    pub fn loaded(path: PathBuf, raw: String) -> Self {
        // Parley's editor treats CR and LF independently. Keep one visual line
        // per Windows newline while retaining the raw disk snapshot for saves.
        let text = raw
            .strip_prefix('\u{feff}')
            .unwrap_or(&raw)
            .replace("\r\n", "\n");
        Self {
            saved: text.clone(),
            text,
            path: Some(path),
            disk: Some(raw),
        }
    }

    /// A save may finish after more typing; those newer edits remain dirty.
    pub fn saved(&mut self, path: PathBuf, text: String) {
        self.path = Some(path);
        self.saved.clone_from(&text);
        self.disk = Some(text);
    }
}

pub(super) struct Save {
    path: PathBuf,
    text: String,
    expected: Option<String>,
}

/// Read at most the source limit, including files that grow while being read.
pub(super) fn read(path: &Path) -> Result<String, String> {
    crate::script::read_source(path).map_err(|error| error.to_string())
}

impl Save {
    /// Refuse external changes before replacing a loaded file. A new path must
    /// not exist; Save As never overwrites an unread document.
    pub fn write(self) -> Result<(PathBuf, String), String> {
        if self.text.len() > MAX_SOURCE_BYTES {
            return Err(format!("Script exceeds {MAX_SOURCE_BYTES} bytes"));
        }
        if let Some(expected) = self.expected {
            if read(&self.path)? != expected {
                return Err(
                    "File changed outside the client. Save to a new path, or reload it.".to_owned(),
                );
            }
            // Write beside the document, then replace it; a failed write leaves
            // the original intact. Recheck after writing, before replacement.
            let (temporary, mut file) = temporary(&self.path)?;
            let result = (|| {
                file.write_all(self.text.as_bytes())
                    .map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                drop(file);
                if read(&self.path)? != expected {
                    return Err(
                        "File changed during save. Reload or save to a new path.".to_owned()
                    );
                }
                fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result?;
        } else {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.path)
                .map_err(|error| {
                    format!(
                        "Cannot create file: {error}. Open an existing file before replacing it."
                    )
                })?;
            let written = file
                .write_all(self.text.as_bytes())
                .and_then(|()| file.sync_all());
            drop(file);
            if let Err(error) = written {
                let _ = fs::remove_file(&self.path);
                return Err(error.to_string());
            }
        }
        Ok((self.path, self.text))
    }
}

fn temporary(path: &Path) -> Result<(PathBuf, fs::File), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary =
            path.with_extension(format!("script-save-{}-{suffix}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not reserve a temporary save file".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_preserve_newer_buffer_edits() {
        let mut doc = Document::loaded("sample.eqscript".into(), "wait 10\n".into());
        doc.text = "wait 20\n".into();
        doc.saved("sample.eqscript".into(), "wait 15\n".into());
        assert!(doc.dirty());
        assert_eq!(doc.text, "wait 20\n");
    }

    #[test]
    fn external_changes_and_unread_existing_files_are_never_overwritten() {
        let path =
            std::env::temp_dir().join(format!("script-conflict-{}.eqscript", std::process::id()));
        fs::write(&path, "wait 10\n").unwrap();
        let mut doc = Document::loaded(path.clone(), read(&path).unwrap());
        doc.text = "wait 20\n".into();
        fs::write(&path, "wait 30\n").unwrap();
        assert!(doc.save(path.clone()).write().is_err());
        assert!(Document::default().save(path.clone()).write().is_err());
        assert_eq!(read(&path).unwrap(), "wait 30\n");
        let mut doc = Document::loaded(path.clone(), read(&path).unwrap());
        doc.text = "wait 40\n".into();
        doc.save(path.clone()).write().unwrap();
        assert_eq!(read(&path).unwrap(), "wait 40\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn bom_and_relative_screenshot_paths_survive_loading() {
        let doc = Document::loaded(
            "scenarios/demo.eqscript".into(),
            "\u{feff}# note\nscreenshot look.png\n".into(),
        );
        assert!(!doc.dirty());
        let program = doc.program().unwrap();
        assert_eq!(program.instructions()[0].line(), 2);
        assert_eq!(
            program.instructions()[0].step(),
            &crate::script::Step::Screenshot("scenarios/look.png".into())
        );
    }

    #[test]
    fn windows_newlines_display_once_and_keep_the_original_disk_snapshot() {
        let raw = "\u{feff}report first\r\nwait 1\r\n";
        let doc = Document::loaded("windows.eqscript".into(), raw.into());
        assert_eq!(doc.text, "report first\nwait 1\n");
        assert!(!doc.dirty());
        assert_eq!(
            doc.save("windows.eqscript".into()).expected.as_deref(),
            Some(raw)
        );
    }
}

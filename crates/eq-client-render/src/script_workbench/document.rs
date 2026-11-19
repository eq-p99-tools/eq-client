//! UTF-8 documents and bounded file operations, independent of the UI and runner.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::script::{MAX_SOURCE_BYTES, ParseError, Program};

pub(super) const EXAMPLE: &str = "# Run in your current game session\n# I uses your current key bindings\npress(\"I\")\nwait(1000)\npress(\"I\")\n";

/// The editable buffer and the exact disk snapshot it was loaded from.
#[derive(Debug)]
pub(super) struct Document {
    pub text: String,
    pub path: Option<PathBuf>,
    saved: String,
    disk: Option<String>,
    /// The file came with Windows newlines, which a save writes back.
    crlf: bool,
    check: Option<Check>,
}

/// Check generations, unique across documents so a replaced one never matches.
static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The parser's verdict on one exact buffer and path. The editor's marks and
/// the status line share it; Run still compiles the buffer it runs.
#[derive(Debug)]
pub(super) struct Check {
    text: String,
    path: Option<PathBuf>,
    /// Statements and actions of a valid program.
    pub counts: Option<(usize, usize)>,
    /// Every error, in source order.
    pub errors: Vec<ParseError>,
    /// Unique to each recomputation, across documents.
    pub generation: u64,
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
            crlf: false,
            check: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    /// Screenshot paths are relative to the document's folder.
    fn base(&self) -> &Path {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
    }

    /// Compile the buffer without changing the current executor or disk snapshot.
    pub fn program(&self) -> Result<Program, ParseError> {
        Program::parse(&self.text, self.base())
    }

    /// The verdict for the current text and path, compiled once per change.
    pub fn check(&mut self) -> &Check {
        let current = self
            .check
            .as_ref()
            .is_some_and(|check| check.text == self.text && check.path == self.path);
        if !current {
            let (counts, errors) = match Program::check(&self.text, self.base()) {
                Ok(program) => (
                    Some((program.statement_count(), program.instructions().len())),
                    Vec::new(),
                ),
                Err(errors) => (None, errors),
            };
            let generation = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.check = Some(Check {
                text: self.text.clone(),
                path: self.path.clone(),
                counts,
                errors,
                generation,
            });
        }
        self.check.as_ref().expect("checked above")
    }

    /// Prepare a save against the snapshot of this path, or a new-file save.
    pub fn save(&self, path: PathBuf) -> Save {
        Save {
            crlf: self.crlf,
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
        let crlf = raw.contains("\r\n");
        let text = raw
            .strip_prefix('\u{feff}')
            .unwrap_or(&raw)
            .replace("\r\n", "\n");
        Self {
            saved: text.clone(),
            text,
            path: Some(path),
            disk: Some(raw),
            crlf,
            check: None,
        }
    }

    /// The bytes a save writes for this buffer: its newlines as the file had them.
    fn on_disk(crlf: bool, text: &str) -> String {
        if crlf {
            text.replace('\n', "\r\n")
        } else {
            text.to_owned()
        }
    }

    /// A save may finish after more typing; those newer edits remain dirty.
    pub fn saved(&mut self, path: PathBuf, text: String) {
        self.path = Some(path);
        self.disk = Some(Self::on_disk(self.crlf, &text));
        self.saved = text;
    }
}

pub(super) struct Save {
    path: PathBuf,
    text: String,
    expected: Option<String>,
    crlf: bool,
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
                file.write_all(Document::on_disk(self.crlf, &self.text).as_bytes())
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
                .write_all(Document::on_disk(self.crlf, &self.text).as_bytes())
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
    fn a_check_compiles_once_per_text_and_path() {
        use crate::script::COMPILES;
        let mut doc = Document::new("wait(1)\npres(\"I\")\n".into(), None);
        let before = COMPILES.get();
        assert_eq!(doc.check().errors.len(), 1);
        let generation = doc.check().generation;
        assert_eq!(doc.check().errors[0].line, Some(2));
        assert_eq!(COMPILES.get(), before + 1);
        doc.text = "wait(1)\n".into();
        assert_eq!(doc.check().counts, Some((1, 1)));
        assert_ne!(doc.check().generation, generation);
        assert_eq!(COMPILES.get(), before + 2);
        doc.path = Some("scenarios/demo.eqscript".into());
        doc.check();
        assert_eq!(COMPILES.get(), before + 3);
    }

    #[test]
    fn saves_preserve_newer_buffer_edits() {
        let mut doc = Document::loaded("sample.eqscript".into(), "wait(10)\n".into());
        doc.text = "wait(20)\n".into();
        doc.saved("sample.eqscript".into(), "wait(15)\n".into());
        assert!(doc.dirty());
        assert_eq!(doc.text, "wait(20)\n");
    }

    #[test]
    fn external_changes_and_unread_existing_files_are_never_overwritten() {
        let path =
            std::env::temp_dir().join(format!("script-conflict-{}.eqscript", std::process::id()));
        fs::write(&path, "wait(10)\n").unwrap();
        let mut doc = Document::loaded(path.clone(), read(&path).unwrap());
        doc.text = "wait(20)\n".into();
        fs::write(&path, "wait(30)\n").unwrap();
        assert!(doc.save(path.clone()).write().is_err());
        assert!(Document::default().save(path.clone()).write().is_err());
        assert_eq!(read(&path).unwrap(), "wait(30)\n");
        let mut doc = Document::loaded(path.clone(), read(&path).unwrap());
        doc.text = "wait(40)\n".into();
        doc.save(path.clone()).write().unwrap();
        assert_eq!(read(&path).unwrap(), "wait(40)\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn bom_and_relative_screenshot_paths_survive_loading() {
        let doc = Document::loaded(
            "scenarios/demo.eqscript".into(),
            "\u{feff}# note\nscreenshot(\"look.png\")\n".into(),
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
        let raw = "\u{feff}report(\"first\")\r\nwait(1)\r\n";
        let doc = Document::loaded("windows.eqscript".into(), raw.into());
        assert_eq!(doc.text, "report(\"first\")\nwait(1)\n");
        assert!(!doc.dirty());
        assert_eq!(
            doc.save("windows.eqscript".into()).expected.as_deref(),
            Some(raw)
        );
    }

    #[test]
    fn a_windows_file_keeps_its_newlines_through_a_save() {
        let path = std::env::temp_dir().join(format!(
            "eq-client-crlf-{}-{}.eqscript",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "wait(1)\r\nwait(2)\r\n").unwrap();
        let mut doc = Document::loaded(path.clone(), read(&path).unwrap());
        assert_eq!(doc.text, "wait(1)\nwait(2)\n");
        doc.text.push_str("wait(3)\n");
        let (saved_path, text) = doc.save(path.clone()).write().unwrap();
        doc.saved(saved_path, text);
        assert_eq!(read(&path).unwrap(), "wait(1)\r\nwait(2)\r\nwait(3)\r\n");
        assert!(!doc.dirty());
        // The snapshot matches the disk, so the next save sees no outside change.
        doc.text.push_str("wait(4)\n");
        doc.save(path.clone()).write().unwrap();
        assert_eq!(
            read(&path).unwrap(),
            "wait(1)\r\nwait(2)\r\nwait(3)\r\nwait(4)\r\n"
        );
        fs::remove_file(path).unwrap();
    }
}

//! The sound files an installed client plays, found by name. Where a client
//! generation keeps them is its [`SoundFiles`], from
//! [`crate::ui::InstalledClient::sound_files`]; a [`SoundBank`] opened over
//! them reads a file's bytes only when it is asked for, and says what kind of
//! WAV they hold so the player can refuse a kind it cannot decode.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use libeq::pfs::PfsReader;
use thiserror::Error;

/// Where a client generation keeps its sound files. A name some of them
/// share is read from the first that has it: the archives in the order of
/// their numbers, then the folders in their order (inferred, item 6.260;
/// the installation surveyed holds such names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundFiles {
    /// The archives' names: this, then a number, then `.pfs`, such as
    /// `snd1.pfs`.
    pub archive_stem: &'static str,
    /// Folders under the installation holding loose WAV files.
    pub folders: &'static [&'static str],
}

impl SoundFiles {
    /// The number of an archive this generation keeps sounds in, from its
    /// file name; None for any other file.
    fn archive_number(self, file_name: &str) -> Option<u32> {
        let lower = file_name.to_ascii_lowercase();
        let number = lower
            .strip_prefix(self.archive_stem)?
            .strip_suffix(".pfs")?;
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        number.parse().ok()
    }
}

/// Why sounds could not be read.
#[derive(Debug, Error)]
pub enum SoundError {
    /// The installation's folder or one of its archives could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// The folder or archive.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// An archive could not be read.
    #[error(transparent)]
    Archive(#[from] libeq::pfs::Error),
    /// A file is not a WAV whose format can be named.
    #[error("{0} is not a readable WAV file")]
    NotWav(String),
}

/// What a WAV file's format chunk says it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WavFormat {
    /// The encoding, by the format chunk's code: [`WavFormat::PCM`] for
    /// plain samples, others for compressed ones.
    pub code: u16,
    /// How many channels.
    pub channels: u16,
    /// Samples each second, per channel.
    pub rate: u32,
    /// Bits in each sample.
    pub bits: u16,
}

impl WavFormat {
    /// The format code of plain samples.
    pub const PCM: u16 = 1;

    /// Reads the format chunk of a RIFF WAVE file; None when the bytes are
    /// not one or the chunk is missing or cut short.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let chunk = chunk(bytes, *b"fmt ")?;
        let word = |at: usize| Some(u16::from_le_bytes(chunk.get(at..at + 2)?.try_into().ok()?));
        Some(Self {
            code: word(0)?,
            channels: word(2)?,
            rate: u32::from_le_bytes(chunk.get(4..8)?.try_into().ok()?),
            bits: word(14)?,
        })
    }
}

/// The body of a RIFF WAVE file's first chunk of this kind; None when the
/// bytes are not one, or the chunk is missing or runs past their end.
fn chunk(bytes: &[u8], kind: [u8; 4]) -> Option<&[u8]> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = usize::try_from(u32::from_le_bytes(header[4..8].try_into().ok()?)).ok()?;
        let body = at + 8;
        if header[0..4] == kind {
            return bytes.get(body..body.checked_add(size)?);
        }
        // Chunks are padded to an even length.
        at = body.checked_add(size)?.checked_add(size % 2)?;
    }
    None
}

/// One sound file, read whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    /// Its name, lower-cased, such as `swing.wav`.
    pub name: String,
    /// What its format chunk says.
    pub format: WavFormat,
    /// The whole file.
    pub bytes: Vec<u8>,
}

impl Sound {
    /// How long it plays, in seconds, from its samples' length; None for a
    /// compressed format, or a file with no samples.
    #[must_use]
    pub fn seconds(&self) -> Option<f32> {
        if self.format.code != WavFormat::PCM {
            return None;
        }
        let frame = u32::from(self.format.channels) * u32::from(self.format.bits.div_ceil(8));
        let per_second = self
            .format
            .rate
            .checked_mul(frame)
            .filter(|rate| *rate > 0)?;
        let samples = u32::try_from(chunk(&self.bytes, *b"data")?.len()).ok()?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "a sound file's length in seconds"
        )]
        Some(samples as f32 / per_second as f32)
    }
}

/// The sound files of an installation, by name.
pub struct SoundBank {
    archives: Vec<PfsReader<File>>,
    /// Each lower-cased name, and where it is.
    files: HashMap<String, Place>,
}

/// Where a sound file is.
enum Place {
    /// In an archive, by the archive's index and the file's name there as
    /// the archive spells it.
    Archive(usize, String),
    /// Loose, at this path.
    Loose(PathBuf),
}

impl SoundBank {
    /// Opens the sound archives a client generation keeps in `eq_directory`
    /// and lists their files. An installation without any has an empty bank.
    ///
    /// # Errors
    /// Fails when the folder or an archive in it cannot be read.
    pub fn open(eq_directory: &Path, files: SoundFiles) -> Result<Self, SoundError> {
        let io_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| SoundError::Io { path, source }
        };
        let mut numbered = Vec::new();
        for entry in std::fs::read_dir(eq_directory).map_err(io_error(eq_directory))? {
            let entry = entry.map_err(io_error(eq_directory))?;
            let name = entry.file_name();
            if let Some(number) = name.to_str().and_then(|name| files.archive_number(name)) {
                numbered.push((number, entry.path()));
            }
        }
        numbered.sort();
        let mut bank = Self {
            archives: Vec::new(),
            files: HashMap::new(),
        };
        for (_, path) in numbered {
            let file = File::open(&path).map_err(io_error(&path))?;
            let mut archive = PfsReader::open(file)?;
            let index = bank.archives.len();
            for name in archive.filenames()? {
                bank.files
                    .entry(name.to_ascii_lowercase())
                    .or_insert(Place::Archive(index, name));
            }
            bank.archives.push(archive);
        }
        for folder in files.folders {
            let folder = eq_directory.join(folder);
            let entries = match std::fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(SoundError::Io {
                        path: folder,
                        source,
                    });
                }
            };
            for entry in entries {
                let entry = entry.map_err(io_error(&folder))?;
                let Some(name) = entry.file_name().to_str().map(str::to_ascii_lowercase) else {
                    continue;
                };
                if Path::new(&name)
                    .extension()
                    .is_some_and(|extension| extension == "wav")
                {
                    bank.files.entry(name).or_insert(Place::Loose(entry.path()));
                }
            }
        }
        Ok(bank)
    }

    /// How many sound files the bank holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the installation had no sound files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the bank holds a file of this name, in any case.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_ascii_lowercase())
    }

    /// The names of the files, lower-cased, in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Reads a file by its name, in any case; None when the bank has none of
    /// that name.
    ///
    /// # Errors
    /// Fails when its archive cannot be read or the file is not a WAV.
    pub fn read(&mut self, name: &str) -> Result<Option<Sound>, SoundError> {
        let name = name.to_ascii_lowercase();
        let bytes = match self.files.get(&name) {
            None => return Ok(None),
            Some(Place::Archive(archive, stored)) => {
                let Some(bytes) = self.archives[*archive].get(stored)? else {
                    return Ok(None);
                };
                bytes
            }
            Some(Place::Loose(path)) => std::fs::read(path).map_err(|source| SoundError::Io {
                path: path.clone(),
                source,
            })?,
        };
        let format = WavFormat::read(&bytes).ok_or_else(|| SoundError::NotWav(name.clone()))?;
        Ok(Some(Sound {
            name,
            format,
            bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libeq::pfs::PfsWriter;
    use std::io::Cursor;

    const FILES: SoundFiles = SoundFiles {
        archive_stem: "snd",
        folders: &["sounds"],
    };

    /// A WAV file with a format chunk and `samples` bytes of 8-bit samples,
    /// after a padded odd-length chunk the reader must step over.
    fn wav(code: u16, samples: usize) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        bytes.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        bytes.extend_from_slice(b"fmt \x10\0\0\0");
        for word in [code, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&11_025_u32.to_le_bytes());
        bytes.extend_from_slice(&11_025_u32.to_le_bytes());
        for word in [1_u16, 8] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::try_from(samples).unwrap().to_le_bytes());
        bytes.resize(bytes.len() + samples, 0x80);
        let riff = u32::try_from(bytes.len() - 8).unwrap();
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        bytes
    }

    fn archive(path: &Path, files: &[(&str, &[u8])]) {
        let mut writer = PfsWriter::create(Cursor::new(Vec::new())).unwrap();
        for (name, bytes) in files {
            writer.insert(*name, *bytes).unwrap();
        }
        std::fs::write(path, writer.finish().unwrap().into_inner()).unwrap();
    }

    #[test]
    fn a_wav_says_its_format_after_other_chunks() {
        assert_eq!(
            WavFormat::read(&wav(WavFormat::PCM, 4)),
            Some(WavFormat {
                code: 1,
                channels: 1,
                rate: 11_025,
                bits: 8,
            })
        );
        assert_eq!(
            WavFormat::read(&wav(2, 4)).map(|format| format.code),
            Some(2)
        );
        // Not a WAV, or one cut off inside its format chunk.
        assert_eq!(WavFormat::read(b"OggS"), None);
        assert_eq!(WavFormat::read(&wav(1, 0)[..30]), None);
        // A chunk whose size runs past the end.
        let mut long = wav(1, 0);
        long[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(WavFormat::read(&long), None);
    }

    #[test]
    fn only_numbered_archives_of_the_stem_are_sound_archives() {
        assert_eq!(FILES.archive_number("snd1.pfs"), Some(1));
        assert_eq!(FILES.archive_number("SND12.PFS"), Some(12));
        for other in ["snd.pfs", "sndx.pfs", "snd1.s3d", "gequip.pfs", "snd1a.pfs"] {
            assert_eq!(FILES.archive_number(other), None, "{other}");
        }
    }

    #[test]
    fn the_bank_reads_files_by_name_from_the_first_archive_that_has_them() {
        let install = std::env::temp_dir().join(format!("eq-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        let first = wav(WavFormat::PCM, 4);
        let later = wav(WavFormat::PCM, 8);
        // Numbered order, not the order of the names: 2 before 10.
        archive(
            &install.join("snd2.pfs"),
            &[("Swing.wav", &first), ("broken.wav", b"not a wav")],
        );
        archive(
            &install.join("SND10.pfs"),
            &[("swing.wav", &later), ("other.wav", &later)],
        );
        archive(&install.join("gequip.pfs"), &[("ignored.wav", &first)]);
        // Loose files in the folders, after the archives.
        let folder = install.join("sounds");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Loose.WAV"), &later).unwrap();
        std::fs::write(folder.join("other.wav"), &first).unwrap();
        std::fs::write(folder.join("theme.mp3"), b"ID3").unwrap();

        let mut bank = SoundBank::open(&install, FILES).unwrap();
        assert_eq!(bank.len(), 4);
        assert_eq!(bank.read("loose.wav").unwrap().unwrap().bytes, later);
        assert!(!bank.contains("theme.mp3"));
        assert!(bank.contains("SWING.WAV"));
        assert!(!bank.contains("ignored.wav"));
        let swing = bank.read("SWING.wav").unwrap().unwrap();
        assert_eq!(swing.name, "swing.wav");
        // 4 one-byte samples at 11,025 a second.
        assert_eq!(swing.seconds(), Some(4.0 / 11_025.0));
        assert_eq!(swing.bytes, first);
        assert_eq!(bank.read("other.wav").unwrap().unwrap().bytes, later);
        assert!(bank.read("missing.wav").unwrap().is_none());
        assert!(
            matches!(bank.read("broken.wav"), Err(SoundError::NotWav(name)) if name == "broken.wav")
        );

        // A folder with no sound archives has an empty bank.
        let empty = install.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(SoundBank::open(&empty, FILES).unwrap().is_empty());
        std::fs::remove_dir_all(&install).unwrap();
    }
}

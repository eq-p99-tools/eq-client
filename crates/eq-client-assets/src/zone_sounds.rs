//! A zone's sound emitters and music regions, from the loose files an
//! installation keeps beside its zone archives: `<zone>_sounds.eff` places
//! them, `<zone>_sndbnk.eff` names the zone's own sounds, and `<zone>.xmi`
//! holds its music ([`crate::xmi`]).
//!
//! The record layout, the kinds and the id rules here are inferred from the
//! files and public format knowledge; what the Titanium program reads from
//! them is pending read R49-7 a (recording checklist 6.503). Each inferred
//! field is named once, below, so the read replaces it in one place.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::sounds::SoundNumbers;

/// The length of one `_sounds.eff` record [I].
pub const RECORD: usize = 84;
/// The most records an emitter file may hold; a larger one is refused
/// (this client's own bound).
pub const MAX_RECORDS: usize = 4096;
/// The largest bank or music file read; a larger one is refused (this
/// client's own bound).
pub const MAX_FILE: u64 = 8 * 1024 * 1024;
/// Ids up to this name the zone's own EMIT sounds, from 1 [I].
const LAST_EMIT: i32 = 31;
/// Ids up to this name the installation's numbered sounds [I].
const LAST_NUMBERED: i32 = 161;
/// Ids from this name the zone's own LOOP sounds [I].
const FIRST_LOOP: i32 = 162;

/// Why a zone's sound files could not be read.
#[derive(Debug, Error)]
pub enum ZoneSoundError {
    /// A file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The emitter file is not whole records, or holds too many.
    #[error("{0} bytes is not a whole number of {RECORD}-byte sound records, or too many")]
    Length(usize),
    /// A file is larger than this client reads.
    #[error("{path} is larger than {MAX_FILE} bytes")]
    TooLarge {
        /// The file.
        path: PathBuf,
    },
}

/// What an emitter record does, by its kind byte (+0x38) [I].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitterKind {
    /// A sound effect with a day and a night sound (0).
    DayNightEffect,
    /// A region of background music, day and night sequences (1).
    Music,
    /// One sound effect, day or night (2).
    StaticEffect,
    /// Day and night music by another route (3).
    DayNightMusic,
    /// Any other kind, kept.
    Unknown(u8),
}

impl EmitterKind {
    const fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::DayNightEffect,
            1 => Self::Music,
            2 => Self::StaticEffect,
            3 => Self::DayNightMusic,
            other => Self::Unknown(other),
        }
    }

    /// Whether its ids name music sequences rather than sounds.
    #[must_use]
    pub const fn is_music(self) -> bool {
        matches!(self, Self::Music | Self::DayNightMusic)
    }
}

/// One record of `<zone>_sounds.eff` [I, every offset pending R49-7 a].
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    /// Its place in the zone's own coordinates, as the file keeps them
    /// (+0x10, +0x14, +0x18).
    pub position: [f32; 3],
    /// How far it is heard (+0x1c).
    pub radius: f32,
    /// The wait after a play before the next, day and night, in ms; 0
    /// loops (+0x20, +0x24).
    pub cooldowns: [i32; 2],
    /// A random wait added to the cooldown, in ms (+0x28).
    pub random_delay: i32,
    /// The day and night sound or sequence ids (+0x30, +0x34).
    pub ids: [i32; 2],
    /// What it does (+0x38).
    pub kind: EmitterKind,
    /// How long a music switch fades out, in ms (+0x44).
    pub fade_out_ms: i32,
    /// How far from its centre it plays at full volume (+0x4c).
    pub full_volume_range: i32,
    /// The whole record, for fields not named yet.
    pub raw: [u8; RECORD],
}

fn word(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn float(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
}

impl Emitter {
    /// Reads one record.
    #[must_use]
    pub fn read(record: &[u8; RECORD]) -> Self {
        Self {
            position: [
                float(record, 0x10),
                float(record, 0x14),
                float(record, 0x18),
            ],
            radius: float(record, 0x1c),
            cooldowns: [word(record, 0x20), word(record, 0x24)],
            random_delay: word(record, 0x28),
            ids: [word(record, 0x30), word(record, 0x34)],
            kind: EmitterKind::from_byte(record[0x38]),
            fade_out_ms: word(record, 0x44),
            full_volume_range: word(record, 0x4c),
            raw: *record,
        }
    }

    /// Whether its numbers can be used: a finite place and a positive,
    /// finite reach.
    #[must_use]
    pub fn usable(&self) -> bool {
        self.position.iter().all(|value| value.is_finite())
            && self.radius.is_finite()
            && self.radius > 0.0
    }

    /// The day or night id.
    #[must_use]
    pub const fn id(&self, night: bool) -> i32 {
        self.ids[night as usize]
    }

    /// The day or night cooldown; 0 loops.
    #[must_use]
    pub const fn cooldown(&self, night: bool) -> i32 {
        self.cooldowns[night as usize]
    }

    /// How loud it plays at a distance from its centre, from 1 within the
    /// full-volume range down to 0 at the radius, falling in a line between
    /// [I, pending R49-7 a].
    #[must_use]
    pub fn loudness(&self, distance: f32) -> f32 {
        if !self.usable() || !distance.is_finite() || distance >= self.radius {
            return 0.0;
        }
        #[expect(clippy::cast_precision_loss, reason = "ranges are small")]
        let full = (self.full_volume_range.max(0) as f32).min(self.radius);
        if distance <= full {
            return 1.0;
        }
        1.0 - (distance - full) / (self.radius - full)
    }
}

/// Reads every record of a `_sounds.eff` file.
///
/// # Errors
/// Refuses a file that is not whole records.
pub fn read_emitters(bytes: &[u8]) -> Result<Vec<Emitter>, ZoneSoundError> {
    if !bytes.len().is_multiple_of(RECORD) || bytes.len() / RECORD > MAX_RECORDS {
        return Err(ZoneSoundError::Length(bytes.len()));
    }
    Ok(bytes
        .as_chunks::<RECORD>()
        .0
        .iter()
        .map(Emitter::read)
        .collect())
}

/// The zone's own sounds from `<zone>_sndbnk.eff`: its sections in order,
/// each a heading line (`EMIT`, `LOOP`, any other kept by name) and a sound
/// file name a line [I].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneBank {
    sections: Vec<(String, Vec<String>)>,
}

impl ZoneBank {
    /// Reads the text. A name before any heading is kept under an empty one.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let heading = line.to_ascii_uppercase();
            if !line.contains('.') && heading.bytes().all(|byte| byte.is_ascii_uppercase()) {
                sections.push((heading, Vec::new()));
                continue;
            }
            if sections.is_empty() {
                sections.push((String::new(), Vec::new()));
            }
            if let Some((_, names)) = sections.last_mut() {
                names.push(line.to_owned());
            }
        }
        Self { sections }
    }

    /// The names under a heading, first section of that name.
    #[must_use]
    pub fn section(&self, heading: &str) -> &[String] {
        self.sections
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(heading))
            .map_or(&[], |(_, names)| names.as_slice())
    }
}

/// Where an emitter's sound id points [I, pending R49-7 a].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundRef {
    /// Nothing plays.
    None,
    /// A sound file by name.
    Named(String),
    /// An id the rules don't resolve (out of a list's range, negative, or a
    /// number the installation doesn't name), kept.
    Unresolved(i32),
}

/// Resolves a sound id: 0 is none, 1 to 31 the zone's EMIT list, 32 to 161
/// the installation's numbered sounds, 162 up the zone's LOOP list.
#[must_use]
pub fn sound_name(id: i32, bank: &ZoneBank, numbers: &SoundNumbers) -> SoundRef {
    let listed = |list: &[String], index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|index| list.get(index))
            .map_or(SoundRef::Unresolved(id), |name| {
                SoundRef::Named(name.clone())
            })
    };
    match id {
        0 => SoundRef::None,
        1..=LAST_EMIT => listed(bank.section("EMIT"), id - 1),
        32..=LAST_NUMBERED => u32::try_from(id)
            .ok()
            .and_then(|number| numbers.name(number))
            .map_or(SoundRef::Unresolved(id), |name| {
                SoundRef::Named(name.to_owned())
            }),
        FIRST_LOOP.. => listed(bank.section("LOOP"), id - FIRST_LOOP),
        _ => SoundRef::Unresolved(id),
    }
}

/// A zone's sound files, as read.
#[derive(Clone, Debug, Default)]
pub struct ZoneSounds {
    /// Every emitter and music record, in file order.
    pub emitters: Vec<Emitter>,
    /// The zone's own sound names.
    pub bank: ZoneBank,
    /// The zone's music file's bytes, when it has one.
    pub music: Option<Vec<u8>>,
}

fn optional(path: &Path) -> Result<Option<Vec<u8>>, ZoneSoundError> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE => {
            return Err(ZoneSoundError::TooLarge {
                path: path.to_owned(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ZoneSoundError::Io {
                path: path.to_owned(),
                source,
            });
        }
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ZoneSoundError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Finds a file in a folder by name, ignoring case, as installations copied
/// between systems name them either way.
fn find(directory: &Path, name: &str) -> PathBuf {
    let exact = directory.join(name);
    if exact.exists() {
        return exact;
    }
    fs::read_dir(directory)
        .ok()
        .and_then(|entries| {
            entries.filter_map(Result::ok).find(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|found| found.eq_ignore_ascii_case(name))
            })
        })
        .map_or(exact, |entry| entry.path())
}

impl ZoneSounds {
    /// Reads a zone's files by its short name from the installation root; a
    /// file it lacks leaves its part empty.
    ///
    /// # Errors
    /// Refuses a file that exists but can't be read, or an emitter file that
    /// is not whole records.
    pub fn read(eq_directory: &Path, zone: &str) -> Result<Self, ZoneSoundError> {
        let zone = zone.to_ascii_lowercase();
        let emitters = optional(&find(eq_directory, &format!("{zone}_sounds.eff")))?
            .map(|bytes| read_emitters(&bytes))
            .transpose()?
            .unwrap_or_default();
        let bank = optional(&find(eq_directory, &format!("{zone}_sndbnk.eff")))?
            .map(|bytes| ZoneBank::parse(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default();
        let music = optional(&find(eq_directory, &format!("{zone}.xmi")))?;
        Ok(Self {
            emitters,
            bank,
            music,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn record(
        position: [f32; 3],
        radius: f32,
        ids: [i32; 2],
        kind: u8,
        full: i32,
    ) -> [u8; RECORD] {
        let mut record = [0; RECORD];
        for (index, value) in position.into_iter().enumerate() {
            record[0x10 + index * 4..0x14 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        record[0x1c..0x20].copy_from_slice(&radius.to_le_bytes());
        record[0x20..0x24].copy_from_slice(&5000_i32.to_le_bytes());
        record[0x28..0x2c].copy_from_slice(&700_i32.to_le_bytes());
        record[0x30..0x34].copy_from_slice(&ids[0].to_le_bytes());
        record[0x34..0x38].copy_from_slice(&ids[1].to_le_bytes());
        record[0x38] = kind;
        record[0x44..0x48].copy_from_slice(&2500_i32.to_le_bytes());
        record[0x4c..0x50].copy_from_slice(&full.to_le_bytes());
        record
    }

    #[test]
    fn records_read_their_fields_and_a_partial_file_is_refused() {
        let bytes = [
            record([1.0, 2.0, 3.0], 50.0, [3, 4], 2, 10),
            record([0.0; 3], 300.0, [1, 2], 1, 0),
        ]
        .concat();
        let emitters = read_emitters(&bytes).unwrap();
        assert_eq!(emitters.len(), 2);
        let first = &emitters[0];
        assert_eq!(first.position, [1.0, 2.0, 3.0]);
        assert!((first.radius - 50.0).abs() < f32::EPSILON);
        assert_eq!(first.cooldowns, [5000, 0]);
        assert_eq!(first.random_delay, 700);
        assert_eq!(first.ids, [3, 4]);
        assert_eq!(first.id(true), 4);
        assert_eq!(first.kind, EmitterKind::StaticEffect);
        assert_eq!(first.fade_out_ms, 2500);
        assert_eq!(first.full_volume_range, 10);
        assert!(emitters[1].kind.is_music());
        assert_eq!(EmitterKind::from_byte(9), EmitterKind::Unknown(9));
        assert!(matches!(
            read_emitters(&bytes[..=RECORD]),
            Err(ZoneSoundError::Length(85))
        ));
        assert_eq!(read_emitters(&[]).unwrap().len(), 0);
        assert!(matches!(
            read_emitters(&vec![0; RECORD * (MAX_RECORDS + 1)]),
            Err(ZoneSoundError::Length(_))
        ));
    }

    #[test]
    fn loudness_is_full_inside_its_range_and_falls_to_nothing_at_the_radius() {
        let emitter = Emitter::read(&record([0.0; 3], 100.0, [1, 1], 2, 20));
        assert!((emitter.loudness(5.0) - 1.0).abs() < f32::EPSILON);
        assert!((emitter.loudness(20.0) - 1.0).abs() < f32::EPSILON);
        assert!((emitter.loudness(60.0) - 0.5).abs() < 1e-6);
        assert!(emitter.loudness(100.0).abs() < f32::EPSILON);
        assert!(emitter.loudness(f32::NAN).abs() < f32::EPSILON);
        let broken = Emitter::read(&record([f32::NAN, 0.0, 0.0], 100.0, [1, 1], 2, 20));
        assert!(!broken.usable());
        assert!(broken.loudness(0.0).abs() < f32::EPSILON);
        // A full-volume range past the radius is the radius.
        let wide = Emitter::read(&record([0.0; 3], 10.0, [1, 1], 2, 50));
        assert!((wide.loudness(9.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ids_resolve_through_the_zone_lists_and_the_numbered_sounds() {
        let bank = ZoneBank::parse("EMIT\r\nwater1.wav\nbird.wav\n\nLOOP\nwind.wav\nRAND\nx.wav\n");
        assert_eq!(bank.section("emit"), ["water1.wav", "bird.wav"]);
        assert_eq!(bank.section("RAND"), ["x.wav"]);
        assert_eq!(bank.section("NONE").len(), 0);
        let numbers = SoundNumbers::parse("100^thunder.wav\n");
        assert_eq!(sound_name(0, &bank, &numbers), SoundRef::None);
        assert_eq!(
            sound_name(2, &bank, &numbers),
            SoundRef::Named("bird.wav".into())
        );
        assert_eq!(sound_name(3, &bank, &numbers), SoundRef::Unresolved(3));
        assert_eq!(
            sound_name(100, &bank, &numbers),
            SoundRef::Named("thunder.wav".into())
        );
        assert_eq!(sound_name(101, &bank, &numbers), SoundRef::Unresolved(101));
        assert_eq!(
            sound_name(162, &bank, &numbers),
            SoundRef::Named("wind.wav".into())
        );
        assert_eq!(sound_name(163, &bank, &numbers), SoundRef::Unresolved(163));
        assert_eq!(sound_name(-1, &bank, &numbers), SoundRef::Unresolved(-1));
        // Names before any heading are kept.
        assert_eq!(ZoneBank::parse("a.wav\nEMIT\nb.wav").section(""), ["a.wav"]);
    }

    #[test]
    fn a_zone_without_files_reads_empty() {
        let directory = std::env::temp_dir().join(format!("zone-sounds-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let empty = ZoneSounds::read(&directory, "nowhere").unwrap();
        assert!(empty.emitters.is_empty() && empty.music.is_none());
        fs::write(
            directory.join("Somewhere_SOUNDS.EFF"),
            record([0.0; 3], 10.0, [1, 1], 0, 0),
        )
        .unwrap();
        fs::write(directory.join("somewhere_sndbnk.eff"), "EMIT\nwave.wav\n").unwrap();
        fs::write(directory.join("somewhere.xmi"), b"FORM").unwrap();
        let found = ZoneSounds::read(&directory, "SomeWhere").unwrap();
        assert_eq!(found.emitters.len(), 1);
        assert_eq!(found.bank.section("EMIT"), ["wave.wav"]);
        assert_eq!(found.music.as_deref(), Some(&b"FORM"[..]));
        fs::remove_dir_all(&directory).unwrap();
    }
}

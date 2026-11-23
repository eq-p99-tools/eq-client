//! A zone's sound emitters and music regions, from the loose files an
//! installation keeps beside its zone archives: `<zone>_sounds.eff` places
//! them, `<zone>_sndbnk.eff` names the zone's own sounds, and `<zone>.xmi`
//! holds its music ([`crate::xmi`]).
//!
//! The record layout, the day and night split, the radius, the repeat and
//! the volume follow the Titanium program's loader 0x44bed1 and 0x44ba79
//! and its millibel conversion 0x4c4fa1 (read R49-7 a, first pass, and its
//! engine follow-up; recording checklist 6.503). The follow-up's section 2
//! supersedes the first pass on the volume's sign: a positive record value
//! is the attenuation, taken negated. What the reads leave open
//! is marked \[I\] where it is used: the falloff, the bank's default
//! extension, the delays' unit, a delay of 0 looping, and the ids between
//! the two banks.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::sounds::SoundNumbers;

/// The length of one `_sounds.eff` record (0x54, read R49-7 a).
pub const RECORD: usize = 84;
/// The most records an emitter file may hold; a larger one is refused
/// (this client's own bound).
pub const MAX_RECORDS: usize = 4096;
/// The largest bank or music file read; a larger one is refused (this
/// client's own bound).
pub const MAX_FILE: u64 = 8 * 1024 * 1024;
/// Ids up to this name the zone's own EMIT sounds, from 1 (read R49-7 a);
/// where the EMIT bank ends is inferred \[I\].
const LAST_EMIT: i32 = 31;
/// Ids up to this name the installation's numbered sounds [I: the read
/// numbers only the two banks].
const LAST_NUMBERED: i32 = 161;
/// Ids from this name the zone's own LOOP sounds: 0xa2 (read R49-7 a).
const FIRST_LOOP: i32 = 162;
/// The reach the program gives a record whose radius is below 0.0 (read
/// R49-7 a); a radius of exactly 0 stays 0, heard nowhere.
pub const UNBOUNDED_RADIUS: f32 = 10_000.0;
/// What the program adds to a repeating emitter's delay (read R49-7 a), in
/// ms [I: the unit].
pub const REPEAT_FLOOR_MS: i32 = 500;
/// The volume of a type-0 record whose volume value is 0 or less (read
/// R49-7 a); any other type plays at 1.0.
pub const TYPE_ZERO_VOLUME: f32 = 0.2;
/// The extension a bank name without '.' takes [I: the read names the
/// string at 0x644178 but not its text].
pub const DEFAULT_EXTENSION: &str = ".wav";

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
    /// The zone's short name is not plain (letters, digits and underscore),
    /// so no file name is built from it.
    #[error("the zone's name is not a plain short name")]
    ZoneName,
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

/// What an emitter does, by its type byte (+0x38 day, +0x39 night; read
/// R49-7 a and its engine follow-up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitterKind {
    /// A bank sound (0), at [`TYPE_ZERO_VOLUME`] unless attenuated.
    DayNightEffect,
    /// A region of music (1): a sound number of 0 or more is that sequence
    /// of `<zone>.xmi`, and a negative one -N is entry N of the
    /// `mp3index.txt` list (read on the music sign, 0x44ba79, 0x4c6a99,
    /// 0x44b502).
    Music,
    /// A sound with the first distance setting (2): the program gives it a
    /// minimum of 3 and 100 beside its +0x4c value; always one emitter.
    StaticEffect,
    /// A sound with the second distance setting (3): the program gives it 8
    /// and its +0x4c value plus 150.
    DistanceEffect,
    /// Any other type, kept.
    Unknown(u8),
}

impl EmitterKind {
    const fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::DayNightEffect,
            1 => Self::Music,
            2 => Self::StaticEffect,
            3 => Self::DistanceEffect,
            other => Self::Unknown(other),
        }
    }

    /// Whether its ids name music sequences rather than sounds.
    #[must_use]
    pub const fn is_music(self) -> bool {
        matches!(self, Self::Music)
    }
}

/// One record of `<zone>_sounds.eff` (read R49-7 a). Each pair is the day
/// field, then the night one.
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    /// Its place in the zone's own coordinates, as the file keeps them
    /// (+0x10, +0x14, +0x18). The program stores z negated on the emitter
    /// (+0x2c); whether its player position is negated alike is unread, so
    /// the file's z is compared with the server's as is \[I\].
    pub position: [f32; 3],
    /// How far it is heard (+0x1c); below 0.0 it is [`UNBOUNDED_RADIUS`].
    pub radius: f32,
    /// The repeat delay, day and night (+0x20, +0x24): above 0 it repeats,
    /// in ms [I: the unit]; 0 or below loops \[I\].
    pub cooldowns: [i32; 2],
    /// The most extra wait added to a repeat, in ms (+0x28).
    pub random_delay: i32,
    /// The day and night sound or sequence ids (+0x30, +0x34).
    pub ids: [i32; 2],
    /// What it does by day (+0x38).
    pub kind: EmitterKind,
    /// The day and night type bytes (+0x38, +0x39).
    pub kinds: [u8; 2],
    /// The day and night volume values (+0x3c, +0x40): positive is an
    /// attenuation in millibels, otherwise the type's own volume.
    pub volumes: [i32; 2],
    /// The day value at +0x44, which the program keeps as the emitter's
    /// +0x1c and, for music, clamps to 20000 and passes with 1500 to the
    /// music start (engine follow-up to R49-7); the music half takes it as
    /// a switch's fade-out in ms [I: the engine follow-up's section 2 reads
    /// the clamp and the 1500 but not what the music start does with them].
    pub fade_out_ms: i32,
    /// The whole record. The pair at +0x4c is a distance setting the
    /// program combines with the type (engine follow-up to R49-7); how the
    /// per-frame falloff uses it is unread.
    pub raw: [u8; RECORD],
}

/// When one of a record's emitters plays (read R49-7 a, its +0x4c mark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Day and night alike: a type-2 record, or one whose day and night
    /// fields match.
    Always,
    /// By day, with the day fields.
    Day,
    /// By night, with the night fields.
    Night,
}

impl Phase {
    /// Whether it plays at this hour.
    #[must_use]
    pub const fn plays(self, night: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Day => !night,
            Self::Night => night,
        }
    }
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
            radius: match float(record, 0x1c) {
                radius if radius < 0.0 => UNBOUNDED_RADIUS,
                radius => radius,
            },
            cooldowns: [word(record, 0x20), word(record, 0x24)],
            random_delay: word(record, 0x28),
            ids: [word(record, 0x30), word(record, 0x34)],
            kind: EmitterKind::from_byte(record[0x38]),
            kinds: [record[0x38], record[0x39]],
            volumes: [word(record, 0x3c), word(record, 0x40)],
            fade_out_ms: word(record, 0x44),
            raw: *record,
        }
    }

    /// The emitters the program makes of this record, each with the side
    /// (0 day, 1 night) whose fields it takes: one always for a type-2
    /// record or one whose day and night fields match, else a day one and
    /// a night one (read R49-7 a).
    #[must_use]
    pub fn split(&self) -> Vec<(Phase, usize)> {
        let same = self.kinds[0] == self.kinds[1]
            && self.volumes[0] == self.volumes[1]
            && self.cooldowns[0] == self.cooldowns[1]
            && self.ids[0] == self.ids[1];
        if self.kinds[0] == 2 || same {
            vec![(Phase::Always, 0)]
        } else {
            vec![(Phase::Day, 0), (Phase::Night, 1)]
        }
    }

    /// The type of one side's emitter.
    #[must_use]
    pub const fn kind_of(&self, side: usize) -> EmitterKind {
        EmitterKind::from_byte(self.kinds[side & 1])
    }

    /// One side's volume (engine follow-up to R49-7, section 2, in
    /// 0x44ba79): a positive value v is an attenuation in millibels, which
    /// 0x4c4fa1(-v) turns into amplitude as 10^(-v/2000), so 2000 is a
    /// tenth and above 10000 is silent; 0 or less is 1.0, or
    /// [`TYPE_ZERO_VOLUME`] for a type-0 emitter.
    #[must_use]
    pub fn volume(&self, side: usize) -> f32 {
        let side = side & 1;
        match self.volumes[side] {
            value if value > 10_000 => 0.0,
            #[expect(clippy::cast_precision_loss, reason = "attenuations are small")]
            value if value > 0 => 10_f32.powf(-(value as f32) / 2000.0),
            _ if self.kinds[side] == 0 => TYPE_ZERO_VOLUME,
            _ => 1.0,
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

    /// One side's repeat delay; 0 or below loops \[I\].
    #[must_use]
    pub const fn cooldown(&self, side: usize) -> i32 {
        self.cooldowns[side & 1]
    }

    /// How loud it plays at a distance from its centre, from 1 there down
    /// to 0 at the radius in a line [I, pending a read of 0x44ba79].
    #[must_use]
    pub fn loudness(&self, distance: f32) -> f32 {
        if !self.usable() || !distance.is_finite() || distance >= self.radius {
            return 0.0;
        }
        1.0 - distance / self.radius
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

/// The zone's own sounds from `<zone>_sndbnk.eff`, as the program reads it
/// (read R49-7 a): a line `EMIT` starts the emitter bank and `LOOP` the
/// loop bank, and every other line is a sound name, with carriage returns,
/// line feeds and spaces taken out and [`DEFAULT_EXTENSION`] added to one
/// without '.'. Headings are matched without regard to case \[I\].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneBank {
    sections: Vec<(String, Vec<String>)>,
}

impl ZoneBank {
    /// Reads the text. A name before any heading is kept under an empty one.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        for line in text.split('\n') {
            let line: String = line
                .chars()
                .filter(|c| !matches!(c, '\r' | '\n' | ' '))
                .collect();
            if line.is_empty() {
                continue;
            }
            if line.eq_ignore_ascii_case("EMIT") || line.eq_ignore_ascii_case("LOOP") {
                sections.push((line.to_ascii_uppercase(), Vec::new()));
                continue;
            }
            if sections.is_empty() {
                sections.push((String::new(), Vec::new()));
            }
            let name = if line.contains('.') {
                line
            } else {
                format!("{line}{DEFAULT_EXTENSION}")
            };
            if let Some((_, names)) = sections.last_mut() {
                names.push(name);
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
/// the installation's numbered sounds \[I\], 162 up the zone's LOOP list
/// (read R49-7 a numbers EMIT from 1 and LOOP from 162).
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
    /// Refuses a zone name that is not plain (empty, or anything but ASCII
    /// letters, digits and underscore) before any path is built, a file
    /// that exists but can't be read, or an emitter file that is not whole
    /// records.
    pub fn read(eq_directory: &Path, zone: &str) -> Result<Self, ZoneSoundError> {
        // The name comes from the server; it names files only when plain,
        // by the rule the zone's own archives are loaded with.
        let zone = zone.to_ascii_lowercase();
        let zone = crate::zone_name(&zone).map_err(|_| ZoneSoundError::ZoneName)?;
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
        record[0x39] = kind;
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
        assert_eq!(first.kinds, [2, 2]);
        assert_eq!(first.fade_out_ms, 2500);
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
    fn loudness_falls_in_a_line_to_nothing_at_the_radius() {
        let emitter = Emitter::read(&record([0.0; 3], 100.0, [1, 1], 2, 20));
        assert!((emitter.loudness(0.0) - 1.0).abs() < f32::EPSILON);
        assert!((emitter.loudness(60.0) - 0.4).abs() < 1e-6);
        assert!(emitter.loudness(100.0).abs() < f32::EPSILON);
        assert!(emitter.loudness(f32::NAN).abs() < f32::EPSILON);
        let broken = Emitter::read(&record([f32::NAN, 0.0, 0.0], 100.0, [1, 1], 2, 20));
        assert!(!broken.usable());
        assert!(broken.loudness(0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_radius_below_zero_reaches_ten_thousand_and_zero_stays_zero() {
        let wide = Emitter::read(&record([0.0; 3], -1.0, [1, 1], 2, 0));
        assert!((wide.radius - UNBOUNDED_RADIUS).abs() < f32::EPSILON);
        assert!(wide.loudness(5000.0) > 0.0);
        let none = Emitter::read(&record([0.0; 3], 0.0, [1, 1], 2, 0));
        assert!(none.radius.abs() < f32::EPSILON);
        assert!(!none.usable());
        assert!(none.loudness(0.0).abs() < f32::EPSILON);
    }

    fn pair(record: &mut [u8; RECORD], at: usize, day: i32, night: i32) {
        record[at..at + 4].copy_from_slice(&day.to_le_bytes());
        record[at + 4..at + 8].copy_from_slice(&night.to_le_bytes());
    }

    #[test]
    fn a_record_makes_one_emitter_or_a_day_and_a_night_one() {
        // Day and night fields alike: one emitter, always.
        let mut alike = record([0.0; 3], 50.0, [3, 3], 0, 0);
        alike[0x39] = 0;
        pair(&mut alike, 0x20, 4000, 4000);
        assert_eq!(Emitter::read(&alike).split(), [(Phase::Always, 0)]);
        // Type 2 is one emitter whatever its night fields say.
        let mut fixed = record([0.0; 3], 50.0, [3, 4], 2, 0);
        fixed[0x39] = 0;
        assert_eq!(Emitter::read(&fixed).split(), [(Phase::Always, 0)]);
        // Any field apart: a day emitter and a night one, each its own.
        for (at, day, night) in [(0x20, 4000, 9000), (0x30, 3, 4), (0x3c, -100, 0)] {
            let mut apart = alike;
            pair(&mut apart, at, day, night);
            let emitter = Emitter::read(&apart);
            assert_eq!(emitter.split(), [(Phase::Day, 0), (Phase::Night, 1)]);
        }
        let mut kinds = alike;
        kinds[0x39] = 2;
        let emitter = Emitter::read(&kinds);
        assert_eq!(emitter.split(), [(Phase::Day, 0), (Phase::Night, 1)]);
        assert_eq!(emitter.kind_of(0), EmitterKind::DayNightEffect);
        assert_eq!(emitter.kind_of(1), EmitterKind::StaticEffect);
        assert!(Phase::Always.plays(true) && Phase::Always.plays(false));
        assert!(Phase::Day.plays(false) && !Phase::Day.plays(true));
        assert!(Phase::Night.plays(true) && !Phase::Night.plays(false));
    }

    #[test]
    fn a_positive_volume_is_an_attenuation_otherwise_the_types_own() {
        let mut quiet = record([0.0; 3], 50.0, [3, 3], 0, 0);
        quiet[0x39] = 2;
        pair(&mut quiet, 0x3c, 0, 2000);
        let emitter = Emitter::read(&quiet);
        assert!((emitter.volume(0) - TYPE_ZERO_VOLUME).abs() < f32::EPSILON);
        // 2000 millibels is a tenth, and above 10000 is silent.
        assert!((emitter.volume(1) - 0.1).abs() < 1e-6);
        let mut silent = quiet;
        pair(&mut silent, 0x3c, 0, 10_001);
        assert!(Emitter::read(&silent).volume(1).abs() < f32::EPSILON);
        pair(&mut silent, 0x3c, 0, 10_000);
        assert!((Emitter::read(&silent).volume(1) - 1e-5).abs() < 1e-7);
        // 0 or less is the type's own: 1.0, or 0.2 for type 0.
        let mut loud = quiet;
        pair(&mut loud, 0x3c, -5, -2000);
        let loud = Emitter::read(&loud);
        assert!((loud.volume(0) - TYPE_ZERO_VOLUME).abs() < f32::EPSILON);
        assert!((loud.volume(1) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ids_resolve_through_the_zone_lists_and_the_numbered_sounds() {
        let bank = ZoneBank::parse("EMIT\r\nwater1.wav\nbi rd.wav \n\nloop\nwind\nrain.wav\n");
        // Carriage returns and spaces go, and a name without '.' takes the
        // default extension.
        assert_eq!(bank.section("emit"), ["water1.wav", "bird.wav"]);
        assert_eq!(bank.section("LOOP"), ["wind.wav", "rain.wav"]);
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
        assert_eq!(sound_name(164, &bank, &numbers), SoundRef::Unresolved(164));
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

    #[test]
    fn a_zone_name_that_is_not_plain_names_no_file() {
        let directory =
            std::env::temp_dir().join(format!("zone-sounds-plain-{}", std::process::id()));
        fs::create_dir_all(directory.join("x")).unwrap();
        fs::write(directory.join("x").join("a.xmi"), b"FORM").unwrap();
        for zone in ["../x", "a/b", "a\\b", "", "a.b", "a b", "zone\0"] {
            assert!(
                matches!(
                    ZoneSounds::read(&directory, zone),
                    Err(ZoneSoundError::ZoneName)
                ),
                "{zone:?}"
            );
        }
        assert!(ZoneSounds::read(&directory, "qeynos2").is_ok());
        assert!(ZoneSounds::read(&directory, "tox_2").is_ok());
        fs::remove_dir_all(&directory).unwrap();
    }
}

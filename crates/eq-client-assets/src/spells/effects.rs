//! The particles an installed Titanium client draws for spells, from its
//! `spellsnew.eff` (each spell effect's stages and their emitters) and
//! `spellsnew.edd` (each emitter's image and motion), with the images in its
//! `SpellEffects` folder. A spell names its effect in field 145 of
//! `spells_us.txt`.
//!
//! Only the names, images and slot numbers are certain. What each emitter
//! number means, where a placement sits on the body and what a slot's level
//! gates were inferred by comparing records, so they are read here and
//! nowhere else.
use std::{io, path::Path, path::PathBuf};
use thiserror::Error;

/// One effect record: a 64-byte name, then three stages.
const EFFECT_RECORD: usize = 268;
/// A stage: its sound's number, then four slots.
const STAGE_WORDS: usize = 17;
/// Where an emitter file's records start, after "EDD\0" and its version.
const EMITTER_HEADER: usize = 8;
/// One emitter record: a 64-byte name, a 64-byte image name, then numbers.
const EMITTER_RECORD: usize = 416;
/// The most particles an emitter starts in a second that is taken as sane.
const MAX_RATE: f32 = 1_000.0;
/// The longest particle life, in seconds, that is taken as sane.
const MAX_LIFE: f32 = 30.0;

/// Why the effect files could not be read.
#[derive(Debug, Error)]
pub enum EffectsError {
    /// A file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The emitter file does not start the way the checked one does.
    #[error("the emitter file does not start with an EDD header")]
    Header,
    /// An image name would leave the effects folder.
    #[error("invalid effect image name: {0}")]
    InvalidImage(String),
    /// An image could not be decoded.
    #[error("could not decode {path}: {source}")]
    Image {
        /// The file.
        path: PathBuf,
        /// Why.
        source: image::ImageError,
    },
}

/// Where on a body a slot's emitter sits (inferred from which slots pair up
/// and from the emitters' names; a recording item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// The left hand.
    LeftHand,
    /// The right hand.
    RightHand,
    /// Over the head.
    Head,
    /// The middle of the body.
    Chest,
    /// The ground under the body.
    Feet,
}

impl Placement {
    /// The placement a slot's number names.
    const fn from_number(number: u32) -> Self {
        match number {
            0 | 3 => Self::Feet,
            1 => Self::Head,
            4 => Self::LeftHand,
            5 => Self::RightHand,
            _ => Self::Chest,
        }
    }
}

/// One emitter an effect stage runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The emitter, by its number in the emitter file.
    pub emitter: u32,
    /// The lowest caster level the slot runs for (inferred).
    pub from_level: u16,
    /// Where it sits.
    pub placement: Placement,
}

/// A spell effect: what runs on the caster while casting and on the target
/// when the spell lands (the first and third stages; inferred from the
/// emitters' names, the second stage is unread).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effect {
    /// The record's name.
    pub name: String,
    /// On the caster during the cast.
    pub caster: Vec<Slot>,
    /// On the target as the spell lands.
    pub target: Vec<Slot>,
    /// The caster stage's sound, by its number in the installation's sound
    /// numbers file ([`crate::sounds::SoundNumbers`]); None for 0. Every
    /// non-zero stage number on the installation checked is one (10-19).
    pub caster_sound: Option<u32>,
    /// The target stage's sound, by its number as `caster_sound`'s.
    pub target_sound: Option<u32>,
}

impl Effect {
    /// The caster stage's slots a caster of `level` runs.
    pub fn caster_slots(&self, level: u16) -> impl Iterator<Item = &Slot> {
        self.caster
            .iter()
            .filter(move |slot| slot.from_level <= level)
    }

    /// The target stage's slots a caster of `level` runs.
    pub fn target_slots(&self, level: u16) -> impl Iterator<Item = &Slot> {
        self.target
            .iter()
            .filter(move |slot| slot.from_level <= level)
    }
}

/// How one emitter starts and moves its particles. Every number's meaning
/// is inferred; positions and motions are in world axes, with up third.
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    /// The record's name.
    pub name: String,
    /// Its image in the `SpellEffects` folder.
    pub image: String,
    /// Particles started a second.
    pub rate: f32,
    /// How long each particle lives, in seconds.
    pub life: f32,
    /// How far from the emitter a particle may start.
    pub spread: f32,
    /// A particle's start velocity.
    pub velocity: [f32; 3],
    /// What pulls on it as it lives.
    pub acceleration: [f32; 3],
    /// Its size.
    pub size: f32,
    /// Its colour at birth and at the end of its life.
    pub colors: [[u8; 3]; 2],
    /// The cells of the image when it is a square sheet, played over the
    /// particle's life; 1 for a single picture.
    pub cells: u32,
}

/// The installed client's spell effects and emitters, by number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpellEffects {
    effects: Vec<Effect>,
    emitters: Vec<Option<Emitter>>,
}

impl SpellEffects {
    /// Reads a Titanium installation's effect files.
    ///
    /// # Errors
    ///
    /// Fails if either file cannot be read or the emitter file's header is
    /// not the checked one's.
    pub fn read_titanium(eq_directory: &Path) -> Result<Self, EffectsError> {
        let read = |name: &str| {
            let path = eq_directory.join(name);
            std::fs::read(&path).map_err(|source| EffectsError::Io { path, source })
        };
        Self::parse(&read("spellsnew.eff")?, &read("spellsnew.edd")?)
    }

    /// Parses the two files' bytes. A short tail is ignored, and a record
    /// whose numbers are not sane is left out rather than guessed at.
    ///
    /// # Errors
    ///
    /// Fails if the emitter file's header is not the checked one's.
    pub fn parse(effects: &[u8], emitters: &[u8]) -> Result<Self, EffectsError> {
        if !emitters.starts_with(b"EDD\0") || emitters.len() < EMITTER_HEADER {
            return Err(EffectsError::Header);
        }
        Ok(Self {
            effects: effects
                .as_chunks::<EFFECT_RECORD>()
                .0
                .iter()
                .map(|record| parse_effect(record))
                .collect(),
            emitters: emitters[EMITTER_HEADER..]
                .as_chunks::<EMITTER_RECORD>()
                .0
                .iter()
                .map(|record| parse_emitter(record))
                .collect(),
        })
    }

    /// The effect a spell names; none for 0, which is the empty record.
    #[must_use]
    pub fn effect(&self, number: u32) -> Option<&Effect> {
        if number == 0 {
            return None;
        }
        self.effects.get(usize::try_from(number).ok()?)
    }

    /// An emitter by its number; none for 0, which is the empty record.
    #[must_use]
    pub fn emitter(&self, number: u32) -> Option<&Emitter> {
        if number == 0 {
            return None;
        }
        self.emitters.get(usize::try_from(number).ok()?)?.as_ref()
    }
}

/// Reads an effect image from the installation's `SpellEffects` folder.
///
/// # Errors
///
/// Rejects names that would leave the folder and unreadable or undecodable
/// images.
pub fn effect_image(eq_directory: &Path, name: &str) -> Result<image::RgbaImage, EffectsError> {
    if !crate::ui::plain_name(name) || name.chars().all(|c| c == '.') {
        return Err(EffectsError::InvalidImage(name.to_owned()));
    }
    let path = eq_directory.join("SpellEffects").join(name);
    // By path, so the extension names the format: TGA has no magic bytes.
    image::open(&path)
        .map(image::DynamicImage::into_rgba8)
        .map_err(|source| EffectsError::Image { path, source })
}

/// A NUL-ended name in a fixed field; text past the NUL is an editor's
/// leftover.
fn name(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).trim().to_owned()
}

fn word(record: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        record[offset],
        record[offset + 1],
        record[offset + 2],
        record[offset + 3],
    ])
}

fn float(record: &[u8], offset: usize) -> f32 {
    f32::from_bits(word(record, offset))
}

fn parse_effect(record: &[u8]) -> Effect {
    let stage = |index: usize| -> Vec<Slot> {
        let start = 64 + index * STAGE_WORDS * 4;
        (0..4)
            .filter_map(|slot| {
                let at = start + 4 + slot * 16;
                let emitter = word(record, at);
                (emitter != 0).then(|| Slot {
                    emitter,
                    from_level: u16::try_from(word(record, at + 4)).unwrap_or(u16::MAX),
                    placement: Placement::from_number(word(record, at + 12)),
                })
            })
            .collect()
    };
    let sound = |index: usize| Some(word(record, 64 + index * STAGE_WORDS * 4)).filter(|n| *n != 0);
    Effect {
        name: name(&record[..64]),
        caster: stage(0),
        target: stage(2),
        caster_sound: sound(0),
        target_sound: sound(2),
    }
}

fn parse_emitter(record: &[u8]) -> Option<Emitter> {
    let image = name(&record[64..128]);
    let vector = |offset| {
        [
            float(record, offset),
            float(record, offset + 4),
            float(record, offset + 8),
        ]
    };
    let channel = |offset| u8::try_from(word(record, offset).min(255)).unwrap_or(u8::MAX);
    let color = |offset| [channel(offset), channel(offset + 4), channel(offset + 8)];
    let cells = word(record, 0x130);
    let emitter = Emitter {
        name: name(&record[..64]),
        image,
        rate: float(record, 0x88),
        life: float(record, 0x94),
        spread: float(record, 0xac),
        acceleration: vector(0xb8),
        size: float(record, 0xcc),
        colors: [color(0xd4), color(0xe0)],
        velocity: vector(0xec),
        cells: if matches!(cells, 4 | 16) { cells } else { 1 },
    };
    let sane = [emitter.rate, emitter.life, emitter.spread, emitter.size]
        .into_iter()
        .chain(emitter.velocity)
        .chain(emitter.acceleration)
        .all(f32::is_finite)
        && (0.0..=MAX_RATE).contains(&emitter.rate)
        && (0.0..=MAX_LIFE).contains(&emitter.life)
        && emitter.size >= 0.0
        && emitter.spread >= 0.0;
    (sane && !emitter.image.is_empty()).then_some(emitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(record: &mut [u8], offset: usize, value: u32) {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_float(record: &mut [u8], offset: usize, value: f32) {
        put(record, offset, value.to_bits());
    }

    fn put_name(record: &mut [u8], offset: usize, text: &str) {
        record[offset..offset + text.len()].copy_from_slice(text.as_bytes());
    }

    /// A synthetic effect file: the empty record, then one whose caster
    /// stage has a level-0 slot on the left hand and a level-24 slot on the
    /// right, and whose target stage has one slot on the chest.
    fn effect_file() -> Vec<u8> {
        let mut bytes = vec![0; EFFECT_RECORD * 2 + 30];
        let record = &mut bytes[EFFECT_RECORD..EFFECT_RECORD * 2];
        put_name(record, 0, "Test effect\0leftover");
        let stage = |index: usize, slot: usize| 64 + index * STAGE_WORDS * 4 + 4 + slot * 16;
        put(record, stage(0, 0), 1);
        put(record, stage(0, 0) + 8, 1);
        put(record, stage(0, 0) + 12, 4);
        put(record, stage(0, 1), 1);
        put(record, stage(0, 1) + 4, 24);
        put(record, stage(0, 1) + 12, 5);
        put(record, stage(1, 0), 2);
        put(record, stage(2, 0), 2);
        put(record, stage(2, 0) + 12, 2);
        // The stages' sounds: the caster's and the target's, none between.
        put(record, stage(0, 0) - 4, 104);
        put(record, stage(2, 0) - 4, 137);
        bytes
    }

    fn emitter_file() -> Vec<u8> {
        let mut bytes = b"EDD\x00110\x00".to_vec();
        bytes.resize(EMITTER_HEADER + EMITTER_RECORD * 3, 0);
        let mut emitter = |index: usize, image: &str, rate: f32| {
            let start = EMITTER_HEADER + index * EMITTER_RECORD;
            let record = &mut bytes[start..start + EMITTER_RECORD];
            put_name(record, 0, "Test emitter");
            put_name(record, 64, image);
            put_float(record, 0x88, rate);
            put_float(record, 0x94, 0.5);
            put_float(record, 0xc0, -3.0);
            put_float(record, 0xcc, 1.5);
            for (offset, value) in [(0xd4, 255), (0xd8, 300), (0xdc, 63), (0xe0, 1)] {
                put(record, offset, value);
            }
            put_float(record, 0xf4, 6.5);
            put(record, 0x130, 16);
        };
        emitter(1, "sparkle.tga", 40.0);
        emitter(2, "broken.tga", f32::NAN);
        bytes
    }

    #[test]
    fn an_effect_keeps_its_caster_and_target_stages_and_skips_empty_slots() {
        let effects = SpellEffects::parse(&effect_file(), &emitter_file()).expect("parsed");
        assert_eq!(effects.effect(0), None);
        assert_eq!(effects.effect(2), None);
        let effect = effects.effect(1).expect("effect 1");
        assert_eq!(effect.name, "Test effect");
        assert_eq!(
            effect.caster,
            [
                Slot {
                    emitter: 1,
                    from_level: 0,
                    placement: Placement::LeftHand,
                },
                Slot {
                    emitter: 1,
                    from_level: 24,
                    placement: Placement::RightHand,
                },
            ]
        );
        assert_eq!(effect.target.len(), 1);
        assert_eq!(effect.target[0].placement, Placement::Chest);
        assert_eq!(effect.caster_sound, Some(104));
        assert_eq!(effect.target_sound, Some(137));
    }

    #[test]
    fn a_slot_runs_from_its_level() {
        let effects = SpellEffects::parse(&effect_file(), &emitter_file()).expect("parsed");
        let effect = effects.effect(1).expect("effect 1");
        assert_eq!(effect.caster_slots(23).count(), 1);
        assert_eq!(effect.caster_slots(24).count(), 2);
        assert_eq!(effect.target_slots(1).count(), 1);
    }

    #[test]
    fn an_emitter_reads_its_numbers_and_an_insane_one_is_left_out() {
        let effects = SpellEffects::parse(&effect_file(), &emitter_file()).expect("parsed");
        let emitter = effects.emitter(1).expect("emitter 1");
        assert_eq!(emitter.image, "sparkle.tga");
        assert!((emitter.rate - 40.0).abs() < f32::EPSILON);
        assert_eq!(emitter.acceleration, [0.0, 0.0, -3.0]);
        assert_eq!(emitter.velocity, [0.0, 0.0, 6.5]);
        assert_eq!(emitter.colors, [[255, 255, 63], [1, 0, 0]]);
        assert_eq!(emitter.cells, 16);
        assert_eq!(effects.emitter(2), None);
        assert_eq!(effects.emitter(0), None);
        assert_eq!(effects.emitter(9), None);
    }

    #[test]
    fn an_emitter_file_without_its_header_is_refused() {
        assert!(matches!(
            SpellEffects::parse(&effect_file(), b"XYZ\0"),
            Err(EffectsError::Header)
        ));
    }

    #[test]
    fn an_image_name_cannot_leave_the_effects_folder() {
        for bad in ["../x.tga", "a\\b.tga", "..", ""] {
            assert!(matches!(
                effect_image(Path::new("."), bad),
                Err(EffectsError::InvalidImage(_))
            ));
        }
    }

    #[test]
    fn a_tga_image_reads_by_its_extension() {
        let install = std::env::temp_dir().join(format!("eq-spell-effects-{}", std::process::id()));
        std::fs::create_dir_all(install.join("SpellEffects")).unwrap();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([9, 8, 7, 255]))
            .save(install.join("SpellEffects/spark.tga"))
            .unwrap();
        let read = effect_image(&install, "spark.tga");
        std::fs::remove_dir_all(&install).unwrap();
        let pixels = read.expect("the tga reads");
        assert_eq!(pixels.dimensions(), (8, 4));
        assert_eq!(pixels.get_pixel(0, 0).0, [9, 8, 7, 255]);
    }
}

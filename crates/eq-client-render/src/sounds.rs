//! Sounds, from the installed client's own sound files
//! ([`eq_client_assets::sounds`]), played through Bevy's audio. Whatever hears
//! of an event asks [`Sounds::play`] for a file by its name, at a place in the
//! world or on the listener; nothing here sends anything.
//!
//! A file is read and test-decoded off the frame thread the first time it is
//! asked for, then kept while it fits [`BUDGET`]. One the decoder cannot read
//! is logged once and never played: Bevy's audio would end the app on it.
use bevy::{
    audio::{AudioSink, PlaybackMode, SpatialAudioSink, SpatialScale, Volume},
    prelude::*,
};
use eq_client_assets::sounds::{SoundBank, SoundError, SoundNumbers};
use std::{
    collections::HashMap,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::background::Background;

/// The most bytes of sound files kept between plays; past it the least
/// recently played are let go, and read again when next asked for.
const BUDGET: usize = 32 * 1024 * 1024;
/// How long, in seconds, a sound may wait for its file before it is too late
/// to play.
const STALE: f32 = 0.5;
/// The most sounds playing at once; past it a new one is not started
/// (inferred, item 6.257).
const VOICES: usize = 32;
/// How far from the listener, in world units, a sound plays at its full
/// volume; past it, it falls off with the square of the distance (inferred,
/// item 6.257).
const FULL_VOLUME: f32 = 20.0;
/// How far from the listener, in world units, a sound is still played
/// (inferred, item 6.257).
const FARTHEST: f32 = 200.0;
/// The distance, in world units, between the listener's ears.
const EAR_GAP: f32 = 4.0;
/// How long, in seconds, a sound may go unstarted past its own length
/// before the audio output is taken to be missing.
const SILENT: f32 = 1.0;
/// How long, in seconds, a sound whose length its file does not say is
/// taken to play.
const UNKNOWN_LENGTH: f32 = 30.0;

/// The installation's sound files, those read so far, and the sounds asked
/// for this frame.
#[derive(Resource)]
pub(super) struct Sounds {
    /// The installation, once its sounds are asked for.
    directory: Option<PathBuf>,
    /// The bank's opening, under way.
    open: Background<Option<(SoundBank, SoundNumbers)>>,
    /// The bank, once open; None while opening, or when this client has no
    /// sound files checked or they could not be read.
    bank: Option<Arc<Mutex<SoundBank>>>,
    /// Each sound's file name by its number; empty until the bank is open.
    numbers: SoundNumbers,
    /// Each file asked for, by its lower-cased name.
    clips: HashMap<String, Clip>,
    /// The bytes of the files kept ready.
    kept: usize,
    /// Counts plays, so the least recently played file is known.
    plays: u64,
    /// The sounds asked for and not yet started.
    asked: Vec<Asked>,
    /// How loud sounds play, from 0 for silent to 1 for as loud as the file.
    pub(super) volume: f32,
    /// Whether there is no audio output to play on, as found when a sound
    /// never started; nothing more is asked for then.
    no_output: bool,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            directory: None,
            open: Background::default(),
            bank: None,
            numbers: SoundNumbers::default(),
            clips: HashMap::new(),
            kept: 0,
            plays: 0,
            asked: Vec::new(),
            volume: 1.0,
            no_output: false,
        }
    }
}

/// One file asked for.
enum Clip {
    /// Being read and test-decoded off the frame thread.
    Reading(Background<Read>),
    /// Ready to play.
    Ready {
        source: Handle<AudioSource>,
        bytes: usize,
        /// How long it plays, in seconds.
        seconds: f32,
        /// The play count when it last played.
        played: u64,
    },
    /// Not in the bank, or unreadable (logged once); kept as such for the
    /// installation's life, which has a few thousand names at most.
    Missing,
}

/// A file's read: its bytes and how long it plays, None when the bank has
/// no such file, or why it cannot be played.
type Read = Result<Option<(Vec<u8>, f32)>, String>;

/// A sound asked for.
struct Asked {
    name: String,
    /// Where in the world it sounds; None for on the listener.
    at: Option<Vec3>,
    /// When it was first looked at, in seconds since the app started.
    when: Option<f32>,
}

impl Sounds {
    /// Asks for a sound file to play once, by its name, at a place in the
    /// world or, with None, on the listener. It plays as soon as its file is
    /// read, or not at all if that takes too long.
    pub(super) fn play(&mut self, name: &str, at: Option<Vec3>) {
        if self.bank.is_some() && self.volume > 0.0 && !self.no_output {
            self.asked.push(Asked {
                name: name.to_ascii_lowercase(),
                at,
                when: None,
            });
        }
    }

    /// Asks for a sound by its number in the installation's numbers file, as
    /// [`Sounds::play`] asks by name; a number that names no file is not
    /// heard.
    pub(super) fn play_number(&mut self, number: u32, at: Option<Vec3>) {
        if let Some(name) = self.numbers.name(number).map(str::to_owned) {
            self.play(&name, at);
        }
    }

    /// Lets go of the least recently played files until those kept fit the
    /// budget. A sound playing keeps its own hold on its file.
    fn trim(&mut self) {
        while self.kept > BUDGET {
            let oldest = self
                .clips
                .iter()
                .filter_map(|(name, clip)| match clip {
                    Clip::Ready { played, .. } => Some((*played, name.clone())),
                    Clip::Reading(_) | Clip::Missing => None,
                })
                .min();
            let Some((_, name)) = oldest else { break };
            if let Some(Clip::Ready { bytes, .. }) = self.clips.remove(&name) {
                self.kept -= bytes;
            }
        }
    }
}

/// Opens the installation's sound files off the frame thread, once.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn load(settings: Res<super::ViewerSettings>, mut sounds: ResMut<Sounds>) {
    let config = &settings.0;
    if sounds.directory.is_none() {
        let Some(directory) = config.eq_directory.clone() else {
            return;
        };
        if let Some(files) = config.installed_client.sound_files() {
            let path = directory.clone();
            sounds
                .open
                .start(0, move || match SoundBank::open(&path, files) {
                    Ok(bank) => {
                        // Without the numbers, sounds named by number are
                        // not heard; those named by file still are.
                        let numbers = SoundNumbers::read(&path, files).unwrap_or_else(|error| {
                            warn!("sounds named by number are off: {error}");
                            SoundNumbers::default()
                        });
                        Some((bank, numbers))
                    }
                    Err(error) => {
                        warn!("sounds are off: {error}");
                        None
                    }
                });
        }
        sounds.directory = Some(directory);
    }
    if let Some(Some((bank, numbers))) = sounds.open.poll(Some(0)) {
        info!(files = bank.len(), numbered = numbers.len(), "Sound files");
        sounds.bank = Some(Arc::new(Mutex::new(bank)));
        sounds.numbers = numbers;
    }
}

/// Reads a file from the bank and checks that the decoder Bevy's audio uses
/// can start on it; the file's bytes and how long it plays, None when the
/// bank has no such file.
fn read(bank: &Mutex<SoundBank>, name: &str) -> Read {
    let sound = bank
        .lock()
        .map_err(|_| "the sound bank was poisoned".to_owned())?
        .read(name)
        .map_err(|error: SoundError| error.to_string())?;
    let Some(sound) = sound else {
        return Ok(None);
    };
    decodes(&sound.bytes).map_err(|error| {
        format!(
            "{name} cannot be decoded (format {}, {} channels, {} Hz, {} bits): {error}",
            sound.format.code, sound.format.channels, sound.format.rate, sound.format.bits
        )
    })?;
    let seconds = sound.seconds().unwrap_or(UNKNOWN_LENGTH);
    Ok(Some((sound.bytes, seconds)))
}

/// Whether the decoder Bevy's audio builds for a source can start on these
/// bytes, built the same way; Bevy's audio ends the app on one it cannot.
fn decodes(bytes: &[u8]) -> Result<(), rodio::decoder::DecoderError> {
    rodio::Decoder::builder()
        .with_byte_len(bytes.len() as u64)
        .with_data(Cursor::new(bytes.to_vec()))
        .build()
        .map(drop)
}

/// Marks a sound this module started, with when it should have finished, in
/// seconds since the app started.
#[derive(Component)]
pub(super) struct Playing {
    until: f32,
}

/// Starts the sounds asked for whose files are ready, reads those not yet
/// read, and lets go of those waiting too long.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    mut sounds: ResMut<Sounds>,
    options: Res<super::options::OptionsState>,
    mut sources: ResMut<Assets<AudioSource>>,
    playing: Query<(Entity, &Playing, Has<AudioSink>, Has<SpatialAudioSink>)>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
) {
    sounds.volume = options.options.sound_volume();
    let Some(bank) = sounds.bank.clone() else {
        sounds.asked.clear();
        return;
    };
    let sounds = &mut *sounds;
    let mut ready = 0;
    for clip in sounds.clips.values_mut() {
        let Clip::Reading(job) = clip else { continue };
        let Some(result) = job.poll(Some(0)) else {
            continue;
        };
        *clip = match result {
            Ok(Some((bytes, seconds))) => {
                ready += bytes.len();
                Clip::Ready {
                    bytes: bytes.len(),
                    seconds,
                    source: sources.add(AudioSource {
                        bytes: bytes.into(),
                    }),
                    played: 0,
                }
            }
            Ok(None) => Clip::Missing,
            Err(error) => {
                warn!("a sound will not play: {error}");
                Clip::Missing
            }
        };
    }
    sounds.kept += ready;
    let now = time.elapsed_secs();
    let ears = listener.iter().next().map(GlobalTransform::translation);
    let mut voices = 0;
    for (entity, sound, started, started_spatial) in &playing {
        voices += 1;
        // Bevy's audio despawns a sound when it ends; one still here long
        // after was never started, so there is no output to play on.
        if now > sound.until + SILENT {
            commands.entity(entity).try_despawn();
            voices -= 1;
            if !(started || started_spatial || sounds.no_output) {
                warn!("Sounds are off: there is no audio output to play them on.");
                sounds.no_output = true;
            }
        }
    }
    if sounds.no_output {
        sounds.asked.clear();
    }
    let asked = std::mem::take(&mut sounds.asked);
    for mut asked in asked {
        let when = *asked.when.get_or_insert(now);
        // A sound too far away, or with no listener to hear it, is not heard.
        if let Some(at) = asked.at
            && ears.is_none_or(|ears| ears.distance(at) > FARTHEST)
        {
            continue;
        }
        match sounds.clips.get_mut(&asked.name) {
            Some(Clip::Ready {
                source,
                played,
                seconds,
                ..
            }) => {
                if voices >= VOICES {
                    continue;
                }
                sounds.plays += 1;
                *played = sounds.plays;
                voices += 1;
                commands.spawn((
                    Playing {
                        until: now + *seconds,
                    },
                    AudioPlayer::new(source.clone()),
                    settings(asked.at.is_some(), sounds.volume),
                    Transform::from_translation(asked.at.unwrap_or_default()),
                ));
            }
            Some(Clip::Missing) => {}
            Some(Clip::Reading(_)) if now - when > STALE => {}
            Some(Clip::Reading(_)) => sounds.asked.push(asked),
            None => {
                let mut job = Background::default();
                let bank = Arc::clone(&bank);
                let name = asked.name.clone();
                job.start(0, move || read(&bank, &name));
                sounds.clips.insert(asked.name.clone(), Clip::Reading(job));
                sounds.asked.push(asked);
            }
        }
    }
    sounds.trim();
}

/// How a sound plays: once, gone when done, at the volume chosen, and placed
/// in the world when it has a place.
fn settings(spatial: bool, volume: f32) -> PlaybackSettings {
    PlaybackSettings {
        mode: PlaybackMode::Despawn,
        volume: Volume::Linear(volume),
        spatial,
        spatial_scale: Some(SpatialScale::new(1.0 / FULL_VOLUME)),
        ..PlaybackSettings::ONCE
    }
}

/// What the camera hears with: sounds pan as the camera turns (inferred,
/// item 6.258).
pub(super) fn listener() -> SpatialListener {
    SpatialListener::new(EAR_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(code: u16) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        for word in [code, 1] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&11_025_u32.to_le_bytes());
        bytes.extend_from_slice(&11_025_u32.to_le_bytes());
        for word in [1_u16, 8] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(b"data\x04\0\0\0\x80\x80\x80\x80");
        let riff = u32::try_from(bytes.len() - 8).unwrap();
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        bytes
    }

    /// A folder holding one sound archive with a short PCM `swing.wav`.
    fn install(tag: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("eq-render-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut writer = libeq::pfs::PfsWriter::create(std::io::Cursor::new(Vec::new())).unwrap();
        writer.insert("Swing.wav", wav(1).as_slice()).unwrap();
        std::fs::write(
            folder.join("snd1.pfs"),
            writer.finish().unwrap().into_inner(),
        )
        .unwrap();
        folder
    }

    /// With an audio output a sound plays and is gone; with none, as in a
    /// container, the app keeps running, says once that sounds are off, and
    /// asks for no more.
    #[test]
    fn a_sound_plays_or_the_missing_output_is_logged_without_ending_the_app() {
        use bevy::time::TimeUpdateStrategy;
        let output = rodio::DeviceSinkBuilder::open_default_sink().is_ok();
        let folder = install("output");
        let files = eq_client_assets::sounds::SoundFiles {
            archive_stem: "snd",
            folders: &[],
            numbers: "SoundAssets.txt",
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::audio::AudioPlugin::default(),
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ))
        .insert_resource(Sounds {
            bank: Some(Arc::new(Mutex::new(
                SoundBank::open(&folder, files).unwrap(),
            ))),
            ..Sounds::default()
        })
        .init_resource::<super::super::options::OptionsState>()
        .add_systems(Update, update);
        app.update();
        app.world_mut()
            .resource_mut::<Sounds>()
            .play("SWING.WAV", None);
        // The file is read off the frame; wait for it in real time.
        let mut started = false;
        for _ in 0..200 {
            app.update();
            let mut playing = app.world_mut().query::<&Playing>();
            if playing.iter(app.world()).next().is_some() {
                started = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(started, "the sound was never started");
        // Well past the sound's length and the grace after it.
        for _ in 0..30 {
            app.update();
        }
        let mut playing = app.world_mut().query::<&Playing>();
        assert_eq!(playing.iter(app.world()).count(), 0);
        let sounds = app.world().resource::<Sounds>();
        assert_eq!(sounds.no_output, !output);
        if !output {
            let mut sounds = app.world_mut().resource_mut::<Sounds>();
            sounds.play("swing.wav", None);
            assert!(sounds.asked.is_empty());
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn only_what_the_decoder_starts_on_is_played() {
        assert!(decodes(&wav(1)).is_ok());
        // A compressed WAV the decoder lacks, and something not a WAV, would
        // end the app inside Bevy's audio.
        assert!(decodes(&wav(2)).is_err());
        assert!(decodes(b"not a sound").is_err());
    }

    #[test]
    fn the_least_recently_played_files_go_first_past_the_budget() {
        let mut sounds = Sounds::default();
        let mut sources = Assets::<AudioSource>::default();
        let half = BUDGET / 2;
        for (name, played) in [("old.wav", 1), ("new.wav", 3), ("mid.wav", 2)] {
            let source = sources.add(AudioSource {
                bytes: Vec::new().into(),
            });
            sounds.clips.insert(
                name.to_owned(),
                Clip::Ready {
                    source,
                    bytes: half,
                    seconds: 1.0,
                    played,
                },
            );
            sounds.kept += half;
        }
        sounds.clips.insert("gone.wav".to_owned(), Clip::Missing);
        sounds.trim();
        assert_eq!(sounds.kept, BUDGET);
        assert!(!sounds.clips.contains_key("old.wav"));
        assert!(sounds.clips.contains_key("mid.wav"));
        assert!(sounds.clips.contains_key("new.wav"));
        assert!(sounds.clips.contains_key("gone.wav"));
    }

    #[test]
    fn nothing_is_asked_for_without_a_bank_or_with_the_volume_down() {
        let mut sounds = Sounds::default();
        sounds.play("Swing.wav", None);
        assert!(sounds.asked.is_empty());
        let empty = std::env::temp_dir().join(format!("eq-render-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let files = eq_client_assets::sounds::SoundFiles {
            archive_stem: "snd",
            folders: &[],
            numbers: "SoundAssets.txt",
        };
        sounds.bank = Some(Arc::new(Mutex::new(
            SoundBank::open(&empty, files).unwrap(),
        )));
        sounds.play("Swing.wav", None);
        assert_eq!(sounds.asked.len(), 1);
        assert_eq!(sounds.asked[0].name, "swing.wav");
        sounds.volume = 0.0;
        sounds.play("Swing.wav", None);
        assert_eq!(sounds.asked.len(), 1);
        std::fs::remove_dir_all(&empty).unwrap();
    }
}

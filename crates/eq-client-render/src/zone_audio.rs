//! A zone's music and ambient sound, from its own files
//! ([`eq_client_assets::zone_sounds`], [`eq_client_assets::xmi`]): the
//! emitters near the player loop or repeat by distance, and the music region
//! the player stands in plays its sequence through a synthesizer and the
//! sound font the player names.
//!
//! The emitters follow the Titanium program's loader (read R49-7 a, first
//! pass; recording checklist 6.503): each record is one emitter or a day
//! and a night one, with its own sound, delay and volume. A music record's
//! sound number, 0 or more, is the sequence of `<zone>.xmi` it plays:
//! 0x44c1bc loads the file through 0x44b33b, and the type-1 branch
//! 0x44bc19 to 0x4c6a99 plays the record's value as its sequence (read on
//! the music sign). A negative number -N names entry N of the
//! `mp3index.txt` list (0x44a8df, 0x44b502), which this client does not
//! play yet; how the program chooses among regions, fades and switches
//! music is not read (the follow-up reads only the 20000 clamp and the
//! 1500 it passes to the music start); every
//! inferred rule is a named constant or function here, marked \[I\]. The instruments are the
//! sound font the player names (a stated departure: how the program plays
//! its music is unread, and the synthesizer here refuses the `.sf2` files
//! an installation may ship as they stand).
//!
//! Emitters and music keep their own voices ([`EMITTER_VOICES`]) apart from
//! the cues [`super::sounds`] plays, so a cue never waits for an emitter
//! (reviewer M1). Nothing outlives its zone: a zone change, a camp, a
//! logout or a lost connection stops every voice.
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, Decodable, PlaybackMode, Volume},
    prelude::*,
};
use eq_client_assets::zone_sounds::{Emitter, EmitterKind, SoundRef, ZoneSounds, sound_name};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use std::{
    collections::{HashMap, HashSet},
    num::NonZero,
    path::Path,
    sync::Arc,
};

use super::background::Background;

/// The most emitter voices at once, nearest first; this client's own bound
/// (reviewer M1).
pub(super) const EMITTER_VOICES: usize = 8;
/// The synthesizer's output rate.
const SAMPLE_RATE: u32 = 44_100;
/// Frames the synthesizer renders at a time.
const BLOCK: usize = 512;
/// The largest sound font read; a larger one is refused (this client's own
/// bound).
const MAX_SOUNDFONT: u64 = 512 * 1024 * 1024;
/// A music switch's fade-out when its record gives none, in ms [I, pending
/// R49-7 b].
const DEFAULT_FADE_MS: i32 = 1000;
/// Below this share of daylight the night ids play [I, pending R49-7 a].
const NIGHT_BELOW: f32 = 0.5;
/// The longest an emitter's one-shot keeps its voice, in seconds, as cues
/// are bounded; this client's own bound.
const ONE_SHOT_SECONDS: f32 = 30.0;

/// One of the zone's music sequences, played on the player's sound font.
#[derive(Asset, TypePath, Clone)]
pub(super) struct MidiMusic {
    font: Arc<SoundFont>,
    midi: Arc<MidiFile>,
}

/// The synthesizer's samples, left and right in turn, looping the sequence.
pub(super) struct MidiDecoder {
    sequencer: Option<MidiFileSequencer>,
    left: Vec<f32>,
    right: Vec<f32>,
    /// The frame next given, and whether its right sample is next.
    at: usize,
    right_next: bool,
}

impl Iterator for MidiDecoder {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.right_next {
            self.right_next = false;
            let sample = self.right[self.at];
            self.at += 1;
            return Some(sample);
        }
        if self.at >= BLOCK {
            if let Some(sequencer) = &mut self.sequencer {
                sequencer.render(&mut self.left, &mut self.right);
            } else {
                self.left.fill(0.0);
                self.right.fill(0.0);
            }
            self.at = 0;
        }
        self.right_next = true;
        Some(self.left[self.at])
    }
}

impl rodio::Source for MidiDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(SAMPLE_RATE).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl Decodable for MidiMusic {
    type Decoder = MidiDecoder;

    fn decoder(&self) -> Self::Decoder {
        #[expect(clippy::cast_possible_wrap, reason = "a fixed rate")]
        let settings = SynthesizerSettings::new(SAMPLE_RATE as i32);
        let sequencer = Synthesizer::new(&self.font, &settings)
            .map_err(|error| warn!("music will not play: {error}"))
            .ok()
            .map(|synthesizer| {
                let mut sequencer = MidiFileSequencer::new(synthesizer);
                sequencer.play(&self.midi, true);
                sequencer
            });
        MidiDecoder {
            sequencer,
            left: vec![0.0; BLOCK],
            right: vec![0.0; BLOCK],
            at: BLOCK,
            right_next: false,
        }
    }
}

/// A zone's files as read for playing: each emitter with its day and night
/// sound names, and the music sequences ready for the synthesizer.
struct Loaded {
    emitters: Vec<Emitter>,
    names: Vec<[SoundRef; 2]>,
    sequences: Vec<Arc<MidiFile>>,
}

/// Reads a zone's files off the frame: a missing or broken file is one log
/// line and leaves its part silent (reviewer condition 2).
fn load(directory: &Path, zone: &str, numbers: &eq_client_assets::sounds::SoundNumbers) -> Loaded {
    let sounds = ZoneSounds::read(directory, zone).unwrap_or_else(|error| {
        warn!(zone, "the zone's sounds are off: {error}");
        ZoneSounds::default()
    });
    let names = sounds
        .emitters
        .iter()
        .map(|emitter| emitter.ids.map(|id| sound_name(id, &sounds.bank, numbers)))
        .collect();
    let sequences = sounds
        .music
        .as_deref()
        .map(|bytes| match eq_client_assets::xmi::to_midi(bytes) {
            Ok(files) => files
                .into_iter()
                .filter_map(|file| match MidiFile::new(&mut file.as_slice()) {
                    Ok(midi) => Some(Arc::new(midi)),
                    Err(error) => {
                        warn!(zone, "a music sequence will not play: {error}");
                        None
                    }
                })
                .collect(),
            Err(error) => {
                warn!(zone, "the zone's music is off: {error}");
                Vec::new()
            }
        })
        .unwrap_or_default();
    Loaded {
        emitters: sounds.emitters,
        names,
        sequences,
    }
}

/// Reads the player's sound font off the frame; one log line and silence
/// when it is missing, too large or unreadable (reviewer condition 3).
fn load_font(path: &Path) -> Option<Arc<SoundFont>> {
    let size = std::fs::metadata(path)
        .map_err(|error| warn!("music is off: the SoundFont cannot be read: {error}"))
        .ok()?
        .len();
    if size > MAX_SOUNDFONT {
        warn!("music is off: the SoundFont is larger than {MAX_SOUNDFONT} bytes");
        return None;
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| warn!("music is off: the SoundFont cannot be read: {error}"))
        .ok()?;
    SoundFont::new(&mut file)
        .map(Arc::new)
        .map_err(|error| warn!("music is off: the SoundFont cannot be read: {error}"))
        .ok()
}

/// The sound font, once asked for.
#[derive(Default)]
enum Font {
    #[default]
    Unasked,
    Loading(Background<Option<Arc<SoundFont>>>),
    Ready(Arc<SoundFont>),
    Absent,
}

/// The zone's music and ambient sound, as loaded and playing.
#[derive(Resource, Default)]
pub(super) struct ZoneAudio {
    font: Font,
    /// The admission and zone loaded or loading.
    zone: Option<(u64, String)>,
    job: Background<Loaded>,
    loaded: Option<Loaded>,
    /// Counts zones, so a voice from an earlier one is let go.
    generation: u64,
    /// The music region and sequence playing.
    music: Option<(usize, usize)>,
    /// When each repeating emitter may play next, in seconds since start.
    next: HashMap<usize, f32>,
    /// Kinds and ids not played, logged once a zone (reviewer M2).
    told: HashSet<String>,
}

/// Marks an emitter's voice.
#[derive(Component)]
pub(super) struct ZoneVoice {
    generation: u64,
    emitter: usize,
    name: String,
    looping: bool,
    /// When a one-shot gives its voice back at the latest, in seconds since
    /// the app started.
    until: f32,
}

#[cfg(test)]
impl ZoneVoice {
    /// A looping voice of an emitter in the first zone, for tests.
    pub(super) fn looping(emitter: usize) -> Self {
        Self {
            generation: 0,
            emitter,
            name: format!("emitter{emitter}.wav"),
            looping: true,
            until: f32::INFINITY,
        }
    }
}

/// Marks the music's voice, and a fading one's start and length.
#[derive(Component)]
pub(super) struct ZoneMusic {
    generation: u64,
    fading: Option<(f32, f32)>,
}

/// Whether the zone clock says night: daylight below [`NIGHT_BELOW`]; a
/// time not yet given is day [I, pending R49-7 a].
fn is_night(daylight: Option<f32>) -> bool {
    daylight.is_some_and(|light| light < NIGHT_BELOW)
}

fn distance(emitter: &Emitter, at: [f32; 3]) -> f32 {
    let [x, y, z] = emitter.position;
    ((x - at[0]).powi(2) + (y - at[1]).powi(2) + (z - at[2]).powi(2)).sqrt()
}

/// The music region the player stands in, nearest centre first, and the
/// sequence it plays: the day or night sound number, counted from 0, as
/// 0x4c6a99 plays `<zone>.xmi` (read on the music sign); None where no
/// region holds the player, or its number is negative (the mp3 list, not
/// played yet) or names no sequence.
/// Which region wins where they overlap is this client's choice [I].
fn choose_music(
    emitters: &[Emitter],
    at: [f32; 3],
    night: bool,
    sequences: usize,
) -> Option<(usize, usize)> {
    emitters
        .iter()
        .enumerate()
        .filter(|(_, emitter)| emitter.kind.is_music() && emitter.usable())
        .map(|(index, emitter)| (index, emitter, distance(emitter, at)))
        .filter(|(_, emitter, distance)| *distance < emitter.radius)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .and_then(|(index, emitter, _)| {
            usize::try_from(emitter.id(night))
                .ok()
                .filter(|sequence| *sequence < sequences)
                .map(|sequence| (index, sequence))
        })
}

/// Whether an emitter of this type plays a sound, rather than music or a
/// type this client does not know.
const fn plays_sound(kind: EmitterKind) -> bool {
    matches!(
        kind,
        EmitterKind::DayNightEffect | EmitterKind::StaticEffect | EmitterKind::DistanceEffect
    )
}

/// An emitter's voice key: its record and the side (0 day, 1 night) whose
/// fields it plays.
const fn key(index: usize, side: usize) -> usize {
    index * 2 + side
}

/// The emitters heard from where the player stands at this hour, nearest
/// first, at most [`EMITTER_VOICES`]: each one's key ([`key`]), sound name
/// and loudness, its distance falloff times its own volume.
fn heard<'a>(
    emitters: &[Emitter],
    names: &'a [[SoundRef; 2]],
    at: [f32; 3],
    night: bool,
) -> Vec<(usize, &'a str, f32)> {
    let mut heard: Vec<_> = emitters
        .iter()
        .zip(names)
        .enumerate()
        .flat_map(|(index, (emitter, names))| {
            emitter
                .split()
                .into_iter()
                .filter(move |(phase, side)| {
                    phase.plays(night) && plays_sound(emitter.kind_of(*side))
                })
                .filter_map(move |(_, side)| {
                    let SoundRef::Named(name) = &names[side] else {
                        return None;
                    };
                    let distance = distance(emitter, at);
                    let loudness = emitter.loudness(distance) * emitter.volume(side);
                    (loudness > 0.0).then_some((
                        key(index, side),
                        name.as_str(),
                        loudness,
                        distance,
                    ))
                })
        })
        .collect();
    heard.sort_by(|a, b| a.3.total_cmp(&b.3));
    heard.truncate(EMITTER_VOICES);
    heard
        .into_iter()
        .map(|(index, name, loudness, _)| (index, name, loudness))
        .collect()
}

/// A repeating emitter's next wait, in seconds: its delay and the program's
/// 500 (read R49-7 a), and a share of its random delay, the share from a
/// hash of the emitter and the time [I: the share's draw; the ms unit].
fn wait(emitter: &Emitter, side: usize, salt: u64) -> f32 {
    let mix = salt
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .rotate_left(17)
        .wrapping_mul(0xbf58_476d_1ce4_e5b9);
    #[expect(clippy::cast_precision_loss, reason = "a share in [0, 1)")]
    let share = (mix >> 11) as f32 / (1_u64 << 53) as f32;
    #[expect(clippy::cast_precision_loss, reason = "waits are small")]
    let (cooldown, delay) = (
        emitter
            .cooldown(side)
            .max(0)
            .saturating_add(eq_client_assets::zone_sounds::REPEAT_FLOOR_MS) as f32,
        emitter.random_delay.max(0) as f32,
    );
    (cooldown + share * delay) / 1000.0
}

/// A typed `/combatmusic`, waiting to be carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CombatMusicTyped;

/// Turns combat music on or off for a typed `/combatmusic`, as the official
/// client's toggle does (read R49-7 c, 0x4689a2), and keeps the choice with
/// the character's options. The line is this client's own: the program's
/// is unread, and this client plays no combat music yet.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn combat_music_typed(
    mut chat: ResMut<super::chat::ChatState>,
    mut options: ResMut<super::options::OptionsState>,
) {
    if chat.combat_music_toggle.take().is_none() {
        return;
    }
    let on = !options.options.combat_music;
    options.options.combat_music = on;
    chat.history
        .push(super::chat::system_line(super::chat::Said::own(
            combat_music_line(on),
        )));
}

/// This client's words for a `/combatmusic`.
const fn combat_music_line(on: bool) -> &'static str {
    if on {
        "Combat music is on. This client does not play combat music yet."
    } else {
        "Combat music is off. This client does not play combat music yet."
    }
}

/// How loud a fading track plays, from 1 at its start down to 0.
fn fade(elapsed: f32, length: f32) -> f32 {
    if length <= 0.0 {
        0.0
    } else {
        (1.0 - elapsed / length).clamp(0.0, 1.0)
    }
}

type Voices<'w, 's> =
    Query<'w, 's, (Entity, &'static ZoneVoice, Option<&'static mut AudioSink>), Without<ZoneMusic>>;
type Tracks<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut ZoneMusic,
        Option<&'static mut AudioSink>,
    ),
>;

/// Loads the zone's files when the player enters it, and plays its
/// emitters and music from where the player stands.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn update(
    mut commands: Commands,
    time: Res<Time<bevy::time::Real>>,
    online: Res<super::online::OnlineState>,
    settings: Res<super::ViewerSettings>,
    options: Res<super::options::OptionsState>,
    mut audio: ResMut<ZoneAudio>,
    mut sounds: ResMut<super::sounds::Sounds>,
    music_assets: Option<ResMut<Assets<MidiMusic>>>,
    mut voices: Voices,
    mut tracks: Tracks,
) {
    let audio = &mut *audio;
    let now = time.elapsed_secs();
    let world = online.world();
    // The admission and zone the player stands in, if any.
    let here = world
        .session_id()
        .filter(|_| world.connected() && !world.zone().is_empty())
        .map(|session| (session, world.zone().to_ascii_lowercase()));
    if here != audio.zone {
        audio.generation = audio.generation.wrapping_add(1);
        audio.loaded = None;
        audio.job.clear();
        audio.music = None;
        audio.next.clear();
        audio.told.clear();
        audio.zone.clone_from(&here);
        if let (Some((session, zone)), Some(directory)) = (&here, settings.0.eq_directory.clone()) {
            let numbers = sounds.numbers().clone();
            let zone = zone.clone();
            audio
                .job
                .start(*session, move || load(&directory, &zone, &numbers));
        }
    }
    let generation = audio.generation;
    // Nothing outlives its zone (reviewer condition 7).
    for (entity, voice, _) in &voices {
        if voice.generation != generation {
            commands.entity(entity).try_despawn();
        }
    }
    for (entity, track, _) in &tracks {
        if track.generation != generation {
            commands.entity(entity).try_despawn();
        }
    }
    if let Some(loaded) = audio.job.poll(here.as_ref().map(|(session, _)| *session)) {
        info!(
            emitters = loaded.emitters.len(),
            sequences = loaded.sequences.len(),
            "Zone sounds"
        );
        audio.loaded = Some(loaded);
    }
    let Some(loaded) = &audio.loaded else {
        return;
    };
    let Some(player) = world.player() else {
        return;
    };
    let at = [player.position.x, player.position.y, player.position.z];
    let clock = settings.0.installed_client.clock();
    let daylight = super::daylight::sky_minute(
        clock,
        eq_client_core::zones::number(world.zone()),
        world.game_time(std::time::Instant::now()),
    )
    .map(|minute| super::daylight::day_night(clock).daylight(minute));
    let night = is_night(daylight);
    for (index, emitter) in loaded.emitters.iter().enumerate() {
        let unplayed = match emitter.kind {
            EmitterKind::Unknown(kind) => Some(format!("kind {kind}")),
            kind if kind.is_music() && emitter.ids.iter().any(|id| *id < 0) => {
                Some(format!("music id {:?} (the mp3 list)", emitter.ids))
            }
            _ => None,
        };
        if let Some(what) = unplayed
            && audio.told.insert(what.clone())
        {
            warn!(index, "a zone sound record is not played: {what}");
        }
    }

    // Emitters.
    // An installation's nosound.txt silences emitters and music alike
    // (read R49-7 c; music too is inferred).
    let silent = settings.0.no_sound;
    let volume = if silent {
        0.0
    } else {
        options.options.environment_volume()
    };
    let wanted = if volume > 0.0 {
        heard(&loaded.emitters, &loaded.names, at, night)
    } else {
        Vec::new()
    };
    let mut playing: HashSet<usize> = HashSet::new();
    for (entity, voice, sink) in &mut voices {
        if voice.generation != generation {
            continue;
        }
        match wanted
            .iter()
            .find(|(index, name, _)| *index == voice.emitter && *name == voice.name)
        {
            Some((_, _, loudness)) if now <= voice.until => {
                playing.insert(voice.emitter);
                if let Some(mut sink) = sink {
                    sink.set_volume(Volume::Linear(loudness * volume));
                }
            }
            // A looping voice stops when it is no longer heard, and every
            // voice at once when the emitters are turned off; a one-shot
            // otherwise plays out, within its bound.
            _ if voice.looping || volume <= 0.0 || now > voice.until => {
                commands.entity(entity).try_despawn();
            }
            _ => {
                playing.insert(voice.emitter);
            }
        }
    }
    let mut count = playing.len();
    for (index, name, loudness) in &wanted {
        if playing.contains(index) || count >= EMITTER_VOICES {
            continue;
        }
        let (record, side) = (*index / 2, *index % 2);
        let emitter = &loaded.emitters[record];
        // Only a delay above 0 repeats (read R49-7 a); 0 or below loops [I].
        let looping = emitter.cooldown(side) <= 0;
        if !looping && audio.next.get(index).is_some_and(|next| now < *next) {
            continue;
        }
        let Some(source) = sounds.source(name) else {
            continue;
        };
        count += 1;
        if !looping {
            let salt = u64::try_from(*index).unwrap_or(0) ^ u64::from(now.to_bits());
            audio.next.insert(*index, now + wait(emitter, side, salt));
        }
        commands.spawn((
            ZoneVoice {
                generation,
                emitter: *index,
                name: (*name).to_owned(),
                looping,
                until: if looping {
                    f32::INFINITY
                } else {
                    now + ONE_SHOT_SECONDS
                },
            },
            AudioPlayer::new(source),
            PlaybackSettings {
                mode: if looping {
                    PlaybackMode::Loop
                } else {
                    PlaybackMode::Despawn
                },
                volume: Volume::Linear(loudness * volume),
                ..PlaybackSettings::LOOP
            },
        ));
    }

    // Music.
    let music_volume = if silent {
        0.0
    } else {
        options.options.music_volume()
    };
    let font = match &mut audio.font {
        Font::Unasked => {
            if let Some(path) = settings.0.music_soundfont.clone() {
                let mut job = Background::default();
                job.start(0, move || load_font(&path));
                audio.font = Font::Loading(job);
            } else {
                info!("Music is off: no SoundFont is named");
                audio.font = Font::Absent;
            }
            None
        }
        Font::Loading(job) => match job.poll(Some(0)) {
            Some(Some(font)) => {
                audio.font = Font::Ready(Arc::clone(&font));
                Some(font)
            }
            Some(None) => {
                audio.font = Font::Absent;
                None
            }
            None => None,
        },
        Font::Ready(font) => Some(Arc::clone(font)),
        Font::Absent => None,
    };
    let chosen = (music_volume > 0.0)
        .then(|| choose_music(&loaded.emitters, at, night, loaded.sequences.len()))
        .flatten();
    let fade_ms = audio
        .music
        .and_then(|(index, _)| loaded.emitters.get(index))
        .map_or(DEFAULT_FADE_MS, |emitter| {
            if emitter.fade_out_ms > 0 {
                emitter.fade_out_ms
            } else {
                DEFAULT_FADE_MS
            }
        });
    let switched = chosen != audio.music;
    for (entity, mut track, sink) in &mut tracks {
        if track.generation != generation {
            continue;
        }
        if switched && track.fading.is_none() {
            #[expect(clippy::cast_precision_loss, reason = "fades are short")]
            let length = fade_ms as f32 / 1000.0;
            track.fading = Some((now, length));
        }
        let gain = track
            .fading
            .map_or(1.0, |(start, length)| fade(now - start, length));
        if track.fading.is_some() && gain <= 0.0 {
            commands.entity(entity).try_despawn();
        } else if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(gain * music_volume));
        }
    }
    if switched {
        audio.music = chosen;
        if let (Some((_, sequence)), Some(font), Some(mut assets)) = (chosen, font, music_assets) {
            let handle = assets.add(MidiMusic {
                font,
                midi: Arc::clone(&loaded.sequences[sequence]),
            });
            commands.spawn((
                ZoneMusic {
                    generation,
                    fading: None,
                },
                AudioPlayer(handle),
                PlaybackSettings {
                    volume: Volume::Linear(music_volume),
                    ..PlaybackSettings::LOOP
                },
            ));
        } else if chosen.is_some() && !matches!(audio.font, Font::Loading(_)) {
            // Without a sound font nothing plays; ask again on the next
            // switch.
        } else if matches!(audio.font, Font::Loading(_)) {
            // The sound font is still loading: choose again next frame.
            audio.music = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter(position: [f32; 3], radius: f32, ids: [i32; 2], kind: u8, full: i32) -> Emitter {
        let mut record = [0; eq_client_assets::zone_sounds::RECORD];
        for (index, value) in position.into_iter().enumerate() {
            record[0x10 + index * 4..0x14 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        record[0x1c..0x20].copy_from_slice(&radius.to_le_bytes());
        record[0x28..0x2c].copy_from_slice(&1000_i32.to_le_bytes());
        record[0x30..0x34].copy_from_slice(&ids[0].to_le_bytes());
        record[0x34..0x38].copy_from_slice(&ids[1].to_le_bytes());
        record[0x38] = kind;
        record[0x39] = kind;
        record[0x4c..0x50].copy_from_slice(&full.to_le_bytes());
        Emitter::read(&record)
    }

    #[test]
    fn the_nearest_music_region_holding_the_player_plays_its_hours_sequence() {
        let regions = [
            emitter([0.0; 3], 1000.0, [0, 1], 1, 0),
            emitter([100.0, 0.0, 0.0], 50.0, [2, 2], 1, 0),
            emitter([0.0; 3], 10.0, [0, 0], 2, 0),
        ];
        assert_eq!(choose_music(&regions, [0.0; 3], false, 3), Some((0, 0)));
        assert_eq!(choose_music(&regions, [0.0; 3], true, 3), Some((0, 1)));
        assert_eq!(
            choose_music(&regions, [90.0, 0.0, 0.0], false, 3),
            Some((1, 2))
        );
        // A sequence the file lacks, or a negative id, plays nothing; id 0
        // is the file's first sequence.
        assert_eq!(choose_music(&regions, [90.0, 0.0, 0.0], false, 2), None);
        let none = [emitter([0.0; 3], 100.0, [-1, i32::MIN], 1, 0)];
        assert_eq!(choose_music(&none, [0.0; 3], false, 3), None);
        assert_eq!(choose_music(&none, [0.0; 3], true, 3), None);
        let past = [emitter([0.0; 3], 100.0, [3, 0], 1, 0)];
        assert_eq!(choose_music(&past, [0.0; 3], false, 3), None);
        assert_eq!(choose_music(&past, [0.0; 3], true, 3), Some((0, 0)));
        assert_eq!(choose_music(&regions, [5000.0, 0.0, 0.0], false, 3), None);
    }

    #[test]
    fn emitters_are_heard_nearest_first_within_their_own_voice_bound() {
        let named = |name: &str| {
            [
                SoundRef::Named(name.into()),
                SoundRef::Named(format!("night-{name}")),
            ]
        };
        let mut emitters = Vec::new();
        let mut sounds = Vec::new();
        for step in 0..12_u8 {
            emitters.push(emitter(
                [f32::from(step) * 5.0, 0.0, 0.0],
                100.0,
                [1, 2],
                0,
                0,
            ));
            sounds.push(named(&format!("s{step}.wav")));
        }
        // Music, an unknown kind and an unresolved name are not emitters.
        emitters.push(emitter([0.0; 3], 100.0, [1, 1], 1, 0));
        sounds.push(named("music.wav"));
        emitters.push(emitter([0.0; 3], 100.0, [1, 1], 9, 0));
        sounds.push(named("odd.wav"));
        emitters.push(emitter([0.0; 3], 100.0, [1, 1], 2, 0));
        sounds.push([SoundRef::Unresolved(1), SoundRef::None]);
        let day = heard(&emitters, &sounds, [0.0; 3], false);
        assert_eq!(day.len(), EMITTER_VOICES);
        assert_eq!(day[0].1, "s0.wav");
        assert!(day.windows(2).all(|pair| pair[0].2 >= pair[1].2));
        assert_eq!(
            heard(&emitters, &sounds, [0.0; 3], true)[0].1,
            "night-s0.wav"
        );
        // A static effect plays its day sound at night too.
        let fixed = [emitter([0.0; 3], 100.0, [1, 2], 2, 0)];
        assert_eq!(
            heard(&fixed, &[named("a.wav")], [0.0; 3], true)[0].1,
            "a.wav"
        );
        assert_eq!(
            heard(&fixed, &[named("a.wav")], [500.0, 0.0, 0.0], false).len(),
            0
        );
    }

    #[test]
    fn a_day_and_a_night_emitter_each_play_their_own_hour_and_volume() {
        let mut record = [0; eq_client_assets::zone_sounds::RECORD];
        record[0x1c..0x20].copy_from_slice(&100.0_f32.to_le_bytes());
        record[0x30..0x34].copy_from_slice(&1_i32.to_le_bytes());
        record[0x34..0x38].copy_from_slice(&2_i32.to_le_bytes());
        // By day a type-0 effect at 0.2; by night a type-2 one at 1.0.
        record[0x38] = 0;
        record[0x39] = 2;
        let split = [Emitter::read(&record)];
        let names = [[
            SoundRef::Named("day.wav".into()),
            SoundRef::Named("night.wav".into()),
        ]];
        let day = heard(&split, &names, [50.0, 0.0, 0.0], false);
        assert_eq!(day.len(), 1);
        assert_eq!((day[0].0, day[0].1), (key(0, 0), "day.wav"));
        assert!((day[0].2 - 0.1).abs() < 1e-6, "{}", day[0].2);
        let night = heard(&split, &names, [50.0, 0.0, 0.0], true);
        assert_eq!((night[0].0, night[0].1), (key(0, 1), "night.wav"));
        assert!((night[0].2 - 0.5).abs() < 1e-6, "{}", night[0].2);
    }

    #[test]
    fn combatmusic_toggles_and_keeps_the_choice_saying_it_is_not_built() {
        let mut app = App::new();
        app.init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::options::OptionsState>()
            .add_systems(Update, combat_music_typed);
        assert!(
            app.world()
                .resource::<super::super::options::OptionsState>()
                .options
                .combat_music
        );
        for on in [false, true] {
            app.world_mut()
                .resource_mut::<super::super::chat::ChatState>()
                .combat_music_toggle = Some(CombatMusicTyped);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<super::super::options::OptionsState>()
                    .options
                    .combat_music,
                on
            );
            let chat = app.world().resource::<super::super::chat::ChatState>();
            assert!(chat.combat_music_toggle.is_none());
            let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
            assert!(
                lines
                    .last()
                    .is_some_and(|(_, line)| line.message.text.contains(combat_music_line(on))),
                "{lines:?}"
            );
        }
        // Nothing typed, nothing changes.
        app.update();
        assert!(
            app.world()
                .resource::<super::super::options::OptionsState>()
                .options
                .combat_music
        );
    }

    #[test]
    fn nights_waits_and_fades_follow_their_rules() {
        assert!(!is_night(None));
        assert!(!is_night(Some(0.8)));
        assert!(is_night(Some(0.2)));
        // The program's 500 ms comes first, then a share of the random
        // delay (here 1000 ms).
        let repeat = emitter([0.0; 3], 10.0, [1, 1], 2, 0);
        for salt in 0..50 {
            let wait = wait(&repeat, 0, salt);
            assert!((0.5..1.5).contains(&wait), "{wait}");
        }
        assert!((fade(0.0, 2.0) - 1.0).abs() < f32::EPSILON);
        assert!((fade(1.0, 2.0) - 0.5).abs() < f32::EPSILON);
        assert!(fade(3.0, 2.0).abs() < f32::EPSILON);
        assert!(fade(0.0, 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_decoder_without_a_synthesizer_is_silent_and_never_ends() {
        let mut decoder = MidiDecoder {
            sequencer: None,
            left: vec![0.0; BLOCK],
            right: vec![0.0; BLOCK],
            at: BLOCK,
            right_next: false,
        };
        assert!(decoder.by_ref().take(BLOCK * 5).all(|sample| sample == 0.0));
        assert!(decoder.next().is_some());
        assert_eq!(rodio::Source::channels(&decoder).get(), 2);
        assert_eq!(rodio::Source::sample_rate(&decoder).get(), SAMPLE_RATE);
    }

    /// A camp, a logout or a lost connection, like a zone change, lets go
    /// of every emitter and music voice the zone started (reviewer
    /// condition 7).
    #[test]
    fn no_voice_outlives_its_zone() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(super::super::online::OnlineState::new(false))
            .init_resource::<crate::ViewerSettings>()
            .init_resource::<super::super::options::OptionsState>()
            .init_resource::<super::super::sounds::Sounds>()
            .insert_resource(ZoneAudio {
                zone: Some((1, "qeynos".into())),
                generation: 5,
                music: Some((0, 0)),
                ..ZoneAudio::default()
            })
            .add_systems(Update, update);
        let voice = app
            .world_mut()
            .spawn(ZoneVoice {
                generation: 5,
                ..ZoneVoice::looping(0)
            })
            .id();
        let music = app
            .world_mut()
            .spawn(ZoneMusic {
                generation: 5,
                fading: None,
            })
            .id();
        app.update();
        assert!(app.world().get_entity(voice).is_err());
        assert!(app.world().get_entity(music).is_err());
        let audio = app.world().resource::<ZoneAudio>();
        assert_eq!(audio.zone, None);
        assert_eq!(audio.generation, 6);
        assert_eq!(audio.music, None);
        assert!(audio.loaded.is_none());
    }
}

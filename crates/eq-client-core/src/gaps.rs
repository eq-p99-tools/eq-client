//! What a session dropped or could not read of the servers' messages, and
//! the world news this client hears but does not show yet, counted into one
//! file per session so live runs become a list of gaps to close.
//!
//! The network library tells of each gap as it happens, the first of its
//! kind at once and repeats in counted batches; [`GapLog`] adds them up by
//! server type, generation, stage, zone, opcode and kind, with when each
//! was first and last seen and up to [`MAX_SAMPLES`] body samples. The
//! world counts the news it leaves unshown by its kind's name alone
//! ([`variant_name`]), never by what it holds.
//!
//! The files live in the per-user settings directory, never in a
//! repository: a zone sample may hold what other players said or their
//! names. Only the newest [`KEEP_SESSIONS`] files are kept. A report
//! ([`report`]) prints no sample unless asked.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

/// The most samples a file keeps of one gap.
pub const MAX_SAMPLES: usize = 3;
/// The most hex characters a kept sample holds: 256 bytes.
pub const MAX_SAMPLE_CHARS: usize = 512;
/// How many session files the folder keeps, the newest.
pub const KEEP_SESSIONS: usize = 50;
/// The least time between two writes of a session's file.
pub const WRITE_EVERY: Duration = Duration::from_secs(10);
/// The stage the client's own unshown news is counted under.
pub const CLIENT_STAGE: &str = "client";
/// The kind of the client's own unshown news.
pub const UNSHOWN: &str = "unshown";

/// A gap as the network library tells of it, read from its JSON.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct NetworkGap {
    /// The stage of the session that received the message.
    pub stage: String,
    /// The client generation the server speaks.
    pub dialect: String,
    /// The configured server type.
    pub server: String,
    /// The short zone name, empty before a zone.
    pub zone: String,
    /// The message's opcode.
    pub opcode: u16,
    /// Its name in the generation's table, when it lists it.
    pub name: Option<String>,
    /// The arrivals this record counts.
    pub count: u64,
    /// The shortest body among them.
    pub min_length: usize,
    /// The longest.
    pub max_length: usize,
    /// What was left unread, and why.
    pub kind: Kind,
    /// A hex sample of the body, at the zone's first record only.
    pub sample: Option<String>,
}

/// What was left unread, and why, as the network library's `GapKind`
/// serializes it, and the client's own [`Kind::Unshown`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// No table row lists the opcode.
    Unlisted,
    /// Listed and left unread, for this reason.
    Unread {
        /// Why.
        reason: String,
    },
    /// Read only at another stage.
    ReadElsewhere,
    /// Holds nothing worth reading: counted, but no gap.
    Skipped {
        /// Why.
        reason: String,
    },
    /// Read all but these parts.
    PartlyRead {
        /// The parts left unread.
        parts: Vec<String>,
    },
    /// A kind of the message no server is known to send.
    UnknownKind {
        /// The kind.
        kind: u32,
    },
    /// The decoders could not read it.
    Unreadable {
        /// What it was about.
        part: String,
        /// Why.
        error: String,
    },
    /// News the client heard but does not show yet.
    Unshown,
    /// A kind a newer network library tells of.
    #[serde(other)]
    Other,
}

impl Kind {
    /// Its key without the free text that changes from one record to the
    /// next, as the network library keys it.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Unlisted => "unlisted".to_owned(),
            Self::Unread { .. } => "unread".to_owned(),
            Self::ReadElsewhere => "read_elsewhere".to_owned(),
            Self::Skipped { .. } => "skipped".to_owned(),
            Self::PartlyRead { .. } => "partly_read".to_owned(),
            Self::UnknownKind { kind } => format!("unknown_kind:{kind}"),
            Self::Unreadable { part, .. } => format!("unreadable:{part}"),
            Self::Unshown => UNSHOWN.to_owned(),
            Self::Other => "other".to_owned(),
        }
    }

    /// Whether it is a gap to close.
    #[must_use]
    pub const fn is_gap(&self) -> bool {
        match self {
            Self::Skipped { .. } => false,
            Self::Unlisted
            | Self::Unread { .. }
            | Self::ReadElsewhere
            | Self::PartlyRead { .. }
            | Self::UnknownKind { .. }
            | Self::Unreadable { .. }
            | Self::Unshown
            | Self::Other => true,
        }
    }

    /// Its reason, error or parts, in words.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Unread { reason } | Self::Skipped { reason } => reason.clone(),
            Self::PartlyRead { parts } => parts.join("; "),
            Self::Unreadable { error, .. } => error.clone(),
            Self::UnknownKind { kind } => format!("kind {kind}"),
            Self::Unlisted | Self::ReadElsewhere | Self::Unshown | Self::Other => String::new(),
        }
    }
}

/// One gap, as a session's file keeps it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Entry {
    /// The stage that received it: login, world or zone, or [`CLIENT_STAGE`]
    /// for news this client hears but does not show.
    pub stage: String,
    /// The client generation the server speaks.
    pub dialect: String,
    /// The configured server type.
    pub server: String,
    /// The short zone name, empty before a zone and for the client's own.
    pub zone: String,
    /// The message's opcode; none for the client's own.
    pub opcode: Option<u16>,
    /// The table's name for the message, or the news's kind.
    pub name: Option<String>,
    /// The gap's kind, without free text.
    pub kind: String,
    /// The first record's whole kind: its reason, error or parts.
    pub detail: Kind,
    /// Whether it is a gap to close; a skipped message is not.
    pub is_gap: bool,
    /// How many arrivals.
    pub count: u64,
    /// When the first arrived, in UTC.
    pub first_seen: String,
    /// When the last record came, in UTC.
    pub last_seen: String,
    /// The shortest body.
    pub min_length: usize,
    /// The longest.
    pub max_length: usize,
    /// Up to [`MAX_SAMPLES`] hex samples of bodies.
    #[serde(default)]
    pub samples: Vec<String>,
}

/// What tells one entry from another in a session.
type Key = (
    String,
    String,
    String,
    String,
    Option<u16>,
    Option<String>,
    String,
);

impl Entry {
    fn key(&self) -> Key {
        (
            self.server.clone(),
            self.dialect.clone(),
            self.stage.clone(),
            self.zone.clone(),
            self.opcode,
            if self.opcode.is_none() {
                self.name.clone()
            } else {
                None
            },
            self.kind.clone(),
        )
    }
}

/// A session's file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GapFile {
    /// The file's layout.
    pub version: u32,
    /// When the session began, in UTC.
    pub started: String,
    /// Each gap, most counted first.
    pub entries: Vec<Entry>,
}

/// The gaps one session has counted.
#[derive(Debug, Default)]
pub struct GapLog {
    started: String,
    entries: BTreeMap<Key, Entry>,
    /// The server and generation of the last network gap, which the
    /// client's own unshown news is counted under.
    server: Option<(String, String)>,
}

impl GapLog {
    /// Nothing counted yet, for a session that began at `started`.
    #[must_use]
    pub fn new(started: jiff::Timestamp) -> Self {
        Self {
            started: started.to_string(),
            ..Self::default()
        }
    }

    /// Adds a network gap's record, told at `at`.
    pub fn network(&mut self, gap: NetworkGap, at: jiff::Timestamp) {
        self.server = Some((gap.server.clone(), gap.dialect.clone()));
        let kind = gap.kind.key();
        let is_gap = gap.kind.is_gap();
        let entry = Entry {
            stage: gap.stage,
            dialect: gap.dialect,
            server: gap.server,
            zone: gap.zone,
            opcode: Some(gap.opcode),
            name: gap.name,
            kind,
            detail: gap.kind,
            is_gap,
            count: gap.count,
            first_seen: at.to_string(),
            last_seen: at.to_string(),
            min_length: gap.min_length,
            max_length: gap.max_length,
            samples: gap
                .sample
                .map(|sample| sample.chars().take(MAX_SAMPLE_CHARS).collect())
                .into_iter()
                .collect(),
        };
        self.add(entry);
    }

    /// Counts one piece of news of kind `name` the client heard at `at` but
    /// does not show yet.
    pub fn unshown(&mut self, name: &str, at: jiff::Timestamp) {
        let (server, dialect) = self.server.clone().unwrap_or_default();
        self.add(Entry {
            stage: CLIENT_STAGE.to_owned(),
            dialect,
            server,
            zone: String::new(),
            opcode: None,
            name: Some(name.to_owned()),
            kind: UNSHOWN.to_owned(),
            detail: Kind::Unshown,
            is_gap: true,
            count: 1,
            first_seen: at.to_string(),
            last_seen: at.to_string(),
            min_length: 0,
            max_length: 0,
            samples: Vec::new(),
        });
    }

    fn add(&mut self, entry: Entry) {
        match self.entries.get_mut(&entry.key()) {
            Some(kept) => merge(kept, entry),
            None => {
                self.entries.insert(entry.key(), entry);
            }
        }
    }

    /// The session's file as it stands.
    #[must_use]
    pub fn file(&self) -> GapFile {
        let mut entries: Vec<Entry> = self.entries.values().cloned().collect();
        entries.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key().cmp(&b.key())));
        GapFile {
            version: 1,
            started: self.started.clone(),
            entries,
        }
    }

    /// Whether nothing has been counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Adds `more` of the same gap into `kept`.
fn merge(kept: &mut Entry, more: Entry) {
    kept.count += more.count;
    kept.min_length = kept.min_length.min(more.min_length);
    kept.max_length = kept.max_length.max(more.max_length);
    if more.first_seen < kept.first_seen {
        kept.first_seen = more.first_seen;
    }
    if more.last_seen > kept.last_seen {
        kept.last_seen = more.last_seen;
    }
    for sample in more.samples {
        if kept.samples.len() < MAX_SAMPLES && !kept.samples.contains(&sample) {
            kept.samples.push(sample);
        }
    }
}

/// A kind's name, without its payload: `T`'s serializer is stopped at the
/// variant's name and never reaches what the variant holds.
#[must_use]
pub fn variant_name<T: Serialize + ?Sized>(value: &T) -> Option<&'static str> {
    match value.serialize(NameOnly) {
        Err(Stop(name)) => name,
        Ok(()) => None,
    }
}

/// The error that stops [`NameOnly`], carrying the name it found.
#[derive(Debug)]
struct Stop(Option<&'static str>);

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stopped at the variant's name")
    }
}

impl std::error::Error for Stop {}

impl serde::ser::Error for Stop {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Self(None)
    }
}

/// A serializer that takes an enum variant's name and stops.
struct NameOnly;

type Never = serde::ser::Impossible<(), Stop>;

macro_rules! stop {
    ($($method:ident($($arg:ty),*)),* $(,)?) => {
        $(fn $method(self, $(_: $arg),*) -> Result<(), Stop> { Err(Stop(None)) })*
    };
}

impl serde::Serializer for NameOnly {
    type Ok = ();
    type Error = Stop;
    type SerializeSeq = Never;
    type SerializeTuple = Never;
    type SerializeTupleStruct = Never;
    type SerializeTupleVariant = Never;
    type SerializeMap = Never;
    type SerializeStruct = Never;
    type SerializeStructVariant = Never;

    stop!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
        serialize_none(),
        serialize_unit(),
        serialize_unit_struct(&'static str),
    );

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<(), Stop> {
        Err(Stop(None))
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Stop> {
        Err(Stop(Some(variant)))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        Err(Stop(None))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        Err(Stop(Some(variant)))
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Never, Stop> {
        Err(Stop(None))
    }

    fn serialize_tuple(self, _: usize) -> Result<Never, Stop> {
        Err(Stop(None))
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Never, Stop> {
        Err(Stop(None))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Never, Stop> {
        Err(Stop(Some(variant)))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Never, Stop> {
        Err(Stop(None))
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Never, Stop> {
        Err(Stop(None))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Never, Stop> {
        Err(Stop(Some(variant)))
    }
}

/// The session's gap file being written, while a session runs.
struct Recorder {
    log: GapLog,
    path: PathBuf,
    written: Option<Instant>,
    changed: bool,
}

/// The running session's recorder: the session's thread tells it the
/// network's gaps, the world's the news it leaves unshown.
static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

/// Starts a session's gap file in `directory`, keeping only the newest
/// files there. Nothing is written until a gap is counted.
///
/// # Errors
/// Returns an error when the directory cannot be made or read.
pub fn begin(directory: &Path, now: jiff::Timestamp) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    prune(directory, KEEP_SESSIONS.saturating_sub(1))?;
    let stamp = now.strftime("%Y%m%dT%H%M%SZ").to_string();
    let mut path = directory.join(format!("gaps-{stamp}.json"));
    let mut again = 1;
    while path.exists() {
        again += 1;
        path = directory.join(format!("gaps-{stamp}-{again}.json"));
    }
    if let Ok(mut recorder) = RECORDER.lock() {
        *recorder = Some(Recorder {
            log: GapLog::new(now),
            path: path.clone(),
            written: None,
            changed: false,
        });
    }
    Ok(path)
}

/// Counts a network gap in the running session's file, when one runs.
pub fn network(gap: NetworkGap) {
    with_recorder(|recorder| {
        recorder.log.network(gap, jiff::Timestamp::now());
        recorder.changed = true;
    });
}

/// Counts news of kind `name` the world heard but does not show, when a
/// session runs.
pub fn unshown(name: &str) {
    with_recorder(|recorder| {
        recorder.log.unshown(name, jiff::Timestamp::now());
        recorder.changed = true;
    });
}

/// What a session's file is written with: the host's serializer, since
/// this crate keeps no JSON writer of its own.
pub type Writer<'a> = &'a dyn Fn(&GapFile) -> io::Result<String>;

/// Writes the running session's file when it changed and
/// [`WRITE_EVERY`] has passed since the last write.
///
/// # Errors
/// Returns an error when the file cannot be written.
pub fn write_when_due(text: Writer<'_>) -> io::Result<()> {
    let mut result = Ok(());
    with_recorder(|recorder| {
        if recorder.changed
            && recorder
                .written
                .is_none_or(|written| written.elapsed() >= WRITE_EVERY)
        {
            result = write(recorder, text);
        }
    });
    result
}

/// Writes the running session's file, if anything was counted, and ends
/// it.
///
/// # Errors
/// Returns an error when the file cannot be written.
pub fn end(text: Writer<'_>) -> io::Result<()> {
    let recorder = RECORDER
        .lock()
        .ok()
        .and_then(|mut recorder| recorder.take());
    match recorder {
        Some(mut recorder) if recorder.changed => write(&mut recorder, text),
        _ => Ok(()),
    }
}

fn with_recorder(run: impl FnOnce(&mut Recorder)) {
    if let Ok(mut recorder) = RECORDER.lock()
        && let Some(recorder) = recorder.as_mut()
    {
        run(recorder);
    }
}

/// Writes the file whole, through a temporary one beside it.
fn write(recorder: &mut Recorder, text: Writer<'_>) -> io::Result<()> {
    let text = text(&recorder.log.file())?;
    let temporary = recorder.path.with_extension("json.part");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, &recorder.path)?;
    recorder.written = Some(Instant::now());
    recorder.changed = false;
    Ok(())
}

/// The session files in `directory`, oldest first.
///
/// # Errors
/// Returns an error when the directory cannot be read.
pub fn files_in(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("gaps-") && name.ends_with(".json"))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Removes all but the newest `keep` session files in `directory`.
fn prune(directory: &Path, keep: usize) -> io::Result<()> {
    let files = files_in(directory)?;
    let extra = files.len().saturating_sub(keep);
    for old in &files[..extra] {
        fs::remove_file(old)?;
    }
    Ok(())
}

/// One gap across sessions, as a report gathers it.
#[derive(Debug, Default)]
struct Gathered {
    count: u64,
    zones: BTreeSet<String>,
    sessions: usize,
    first_seen: String,
    last_seen: String,
    min_length: Option<usize>,
    max_length: usize,
    detail: String,
    samples: Vec<String>,
}

/// A Markdown report of the gaps in `files`: the gaps to close ranked by
/// how often they came, then what was counted as skipped. Samples are left
/// out unless `samples` asks for them, since one may hold what another
/// player said.
#[must_use]
pub fn report(files: &[GapFile], samples: bool) -> String {
    let mut gathered: BTreeMap<(bool, String, String, String, String, String, String), Gathered> =
        BTreeMap::new();
    for file in files {
        let mut seen = BTreeSet::new();
        for entry in &file.entries {
            let opcode = entry
                .opcode
                .map_or_else(String::new, |opcode| format!("0x{opcode:04x}"));
            let key = (
                entry.is_gap,
                entry.server.clone(),
                entry.dialect.clone(),
                entry.stage.clone(),
                opcode,
                entry.name.clone().unwrap_or_default(),
                entry.kind.clone(),
            );
            let row = gathered.entry(key.clone()).or_default();
            row.count += entry.count;
            if !entry.zone.is_empty() {
                row.zones.insert(entry.zone.clone());
            }
            if seen.insert(key) {
                row.sessions += 1;
            }
            if row.first_seen.is_empty() || entry.first_seen < row.first_seen {
                row.first_seen.clone_from(&entry.first_seen);
            }
            if entry.last_seen > row.last_seen {
                row.last_seen.clone_from(&entry.last_seen);
            }
            row.min_length = Some(
                row.min_length
                    .map_or(entry.min_length, |min| min.min(entry.min_length)),
            );
            row.max_length = row.max_length.max(entry.max_length);
            if row.detail.is_empty() {
                row.detail = entry.detail.detail();
            }
            for sample in &entry.samples {
                if row.samples.len() < MAX_SAMPLES && !row.samples.contains(sample) {
                    row.samples.push(sample.clone());
                }
            }
        }
    }
    let mut rows: Vec<_> = gathered.into_iter().collect();
    rows.sort_by(|(a_key, a), (b_key, b)| {
        b_key
            .0
            .cmp(&a_key.0)
            .then(b.count.cmp(&a.count))
            .then(a_key.cmp(b_key))
    });
    let mut out = String::new();
    let _ = writeln!(out, "# Server message gaps\n");
    let _ = writeln!(out, "{} session file(s).\n", files.len());
    for (heading, gaps) in [("Gaps to close", true), ("Counted, nothing to read", false)] {
        let _ = writeln!(out, "## {heading}\n");
        let mut any = false;
        for ((is_gap, server, dialect, stage, opcode, name, kind), row) in &rows {
            if *is_gap != gaps {
                continue;
            }
            if !any {
                let _ = writeln!(
                    out,
                    "| Count | Sessions | Server | Generation | Stage | Opcode | Name | Kind | Detail | Bytes | Zones | First seen | Last seen |"
                );
                let _ = writeln!(
                    out,
                    "|---:|---:|---|---|---|---|---|---|---|---|---|---|---|"
                );
                any = true;
            }
            let zones: Vec<_> = row.zones.iter().map(String::as_str).collect();
            let bytes = match row.min_length {
                Some(min) if min == row.max_length => min.to_string(),
                Some(min) => format!("{min}-{}", row.max_length),
                None => String::new(),
            };
            let _ = writeln!(
                out,
                "| {} | {} | {server} | {dialect} | {stage} | {opcode} | {} | {kind} | {} | {bytes} | {} | {} | {} |",
                row.count,
                row.sessions,
                cell(name),
                cell(&row.detail),
                cell(&zones.join(", ")),
                row.first_seen,
                row.last_seen,
            );
        }
        if !any {
            let _ = writeln!(out, "None.");
        }
        let _ = writeln!(out);
    }
    if samples {
        let _ = writeln!(out, "## Samples\n");
        for ((_, server, _, stage, opcode, name, kind), row) in &rows {
            for sample in &row.samples {
                let _ = writeln!(
                    out,
                    "- {server} {stage} {opcode} {} {kind}: `{sample}`",
                    cell(name)
                );
            }
        }
    }
    out
}

/// Text safe inside a Markdown table cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests;

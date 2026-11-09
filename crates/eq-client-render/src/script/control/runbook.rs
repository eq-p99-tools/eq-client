//! Checklist state reported by the local harness, independent of the executor.
use std::collections::HashSet;

use serde_json::{Value, json};

use crate::online::OnlineState;

pub(super) mod export;
pub(super) mod view;

const MAX_ITEMS: usize = 64;
const MAX_ID_BYTES: usize = 48;
const MAX_TITLE_BYTES: usize = 96;
const MAX_LABEL_BYTES: usize = 120;
const MAX_NOTE_BYTES: usize = 180;
const MAX_REFERENCE_BYTES: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Definition {
    title: String,
    items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    id: String,
    label: String,
}

/// Only the producer can report progress or an outcome. Pending has no report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Running,
    Blocked,
    Skipped,
    Passed,
    Failed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Skipped => "skipped",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }

    fn terminal(self) -> bool {
        matches!(self, Self::Skipped | Self::Passed | Self::Failed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Evidence {
    kind: EvidenceKind,
    reference: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvidenceKind {
    ScriptedObservation,
    ManualWitness,
    ReceivedData,
}

impl EvidenceKind {
    fn name(self) -> &'static str {
        match self {
            Self::ScriptedObservation => "scripted_observation",
            Self::ManualWitness => "manual_witness",
            Self::ReceivedData => "received_data",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ScriptedObservation => "Scripted observation",
            Self::ManualWitness => "Manual witness",
            Self::ReceivedData => "Received data",
        }
    }
}

impl Evidence {
    fn parse(value: &Value) -> Result<Self, &'static str> {
        let object = object(value, &["kind", "reference"])?;
        let kind = match object.get("kind").and_then(Value::as_str) {
            Some("scripted_observation") => EvidenceKind::ScriptedObservation,
            Some("manual_witness") => EvidenceKind::ManualWitness,
            Some("received_data") => EvidenceKind::ReceivedData,
            _ => {
                return Err(
                    "Runbook evidence kind must be scripted_observation, manual_witness or received_data",
                );
            }
        };
        let reference = identifier(object.get("reference"), MAX_REFERENCE_BYTES).ok_or(
            "Runbook evidence reference must be 1–96 ASCII letters, digits, hyphens or underscores",
        )?;
        Ok(Self { kind, reference })
    }

    fn json(&self) -> Value {
        json!({"kind":self.kind.name(),"reference":self.reference})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    state: State,
    note: Option<String>,
    evidence: Option<Evidence>,
    admission_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResultUpdate {
    runbook_id: u64,
    item_id: String,
    after_request_id: Option<u64>,
    report: Report,
}

impl Definition {
    pub(super) fn parse(value: &Value) -> Result<Self, &'static str> {
        let object = object(value, &["title", "items"])?;
        let title = plain_text(object.get("title"), MAX_TITLE_BYTES)
            .ok_or("Runbook title must be 1–96 bytes of single-line text")?;
        let items = object
            .get("items")
            .and_then(Value::as_array)
            .filter(|items| (1..=MAX_ITEMS).contains(&items.len()))
            .ok_or("Runbook requires 1–64 items")?;
        let items = items
            .iter()
            .map(Item::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let mut ids = HashSet::new();
        if items.iter().any(|item| !ids.insert(item.id.as_str())) {
            return Err("Runbook item IDs must be unique");
        }
        Ok(Self { title, items })
    }
}

impl Item {
    fn parse(value: &Value) -> Result<Self, &'static str> {
        let object = object(value, &["id", "label"])?;
        let id = item_id(object.get("id"))?;
        let label = plain_text(object.get("label"), MAX_LABEL_BYTES)
            .ok_or("Runbook item label must be 1–120 bytes of single-line text")?;
        Ok(Self { id, label })
    }
}

impl ResultUpdate {
    pub(super) fn parse(value: &Value) -> Result<Self, &'static str> {
        let object = object(
            value,
            &[
                "runbook_id",
                "item_id",
                "status",
                "note",
                "evidence",
                "admission_id",
                "after_request_id",
            ],
        )?;
        let runbook_id = object
            .get("runbook_id")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or("Runbook result requires a positive runbook_id from status")?;
        let item_id = item_id(object.get("item_id"))?;
        let state = match object.get("status").and_then(Value::as_str) {
            Some("running") => State::Running,
            Some("blocked") => State::Blocked,
            Some("skipped") => State::Skipped,
            Some("passed") => State::Passed,
            Some("failed") => State::Failed,
            _ => {
                return Err(
                    "Runbook result status must be running, blocked, skipped, passed or failed",
                );
            }
        };
        // Status emits null for an absent note, including on terminal reports.
        let note = object
            .get("note")
            .filter(|value| !value.is_null())
            .map(|value| {
                plain_text(Some(value), MAX_NOTE_BYTES)
                    .ok_or("Runbook note must be 1–180 bytes of single-line text")
            })
            .transpose()?;
        let evidence = object
            .get("evidence")
            .filter(|value| !value.is_null())
            .map(Evidence::parse)
            .transpose()?;
        let admission_id = optional_id(
            object.get("admission_id"),
            "Runbook admission_id must be a positive integer from status",
        )?;
        let after_request_id = optional_id(
            object.get("after_request_id"),
            "Runbook after_request_id must be a positive integer from the row",
        )?;
        Ok(Self {
            runbook_id,
            item_id,
            after_request_id,
            report: Report {
                state,
                note,
                evidence,
                admission_id,
            },
        })
    }
}

fn object<'a>(
    value: &'a Value,
    fields: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, &'static str> {
    let object = value.as_object().ok_or("Runbook data must be an object")?;
    if object.keys().any(|field| !fields.contains(&field.as_str())) {
        return Err("Runbook data contains an unexpected field");
    }
    Ok(object)
}

fn item_id(value: Option<&Value>) -> Result<String, &'static str> {
    identifier(value, MAX_ID_BYTES)
        .ok_or("Runbook item ID must be 1–48 ASCII letters, digits, hyphens or underscores")
}

fn identifier(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| {
            (1..=max).contains(&text.len())
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .map(str::to_owned)
}

fn optional_id(value: Option<&Value>, error: &'static str) -> Result<Option<u64>, &'static str> {
    value
        .filter(|value| !value.is_null())
        .map(|value| value.as_u64().filter(|id| *id != 0).ok_or(error))
        .transpose()
}

fn plain_text(value: Option<&Value>, max: usize) -> Option<String> {
    let text = value?.as_str()?;
    if text.len() > max
        || text.trim().is_empty()
        || text.chars().any(|ch| {
            ch.is_control()
                || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return None;
    }
    Some(text.to_owned())
}

pub(super) struct Run {
    pub(super) id: u64,
    title: String,
    pub(super) revision: u64,
    rows: Vec<Row>,
}

struct Row {
    item: Item,
    recorded: Option<(u64, Report)>,
}

impl Row {
    fn report(&self) -> Option<&Report> {
        self.recorded.as_ref().map(|(_, report)| report)
    }
}

#[derive(Default)]
struct Counts {
    pending: usize,
    running: usize,
    blocked: usize,
    skipped: usize,
    passed: usize,
    failed: usize,
}

impl Counts {
    fn health(&self) -> &'static str {
        if self.pending + self.running + self.blocked == 0 {
            "complete"
        } else if self.running != 0 {
            "running"
        } else if self.blocked != 0 {
            "blocked"
        } else if self.passed + self.failed + self.skipped == 0 {
            "pending"
        } else {
            "incomplete"
        }
    }

    fn json(&self) -> Value {
        json!({"state":self.health(),"pending":self.pending,"running":self.running,
            "blocked":self.blocked,"skipped":self.skipped,"passed":self.passed,"failed":self.failed})
    }
}

impl Run {
    pub(super) fn new(id: u64, definition: Definition) -> Self {
        Self {
            id,
            title: definition.title,
            revision: 0,
            rows: definition
                .items
                .into_iter()
                .map(|item| Row {
                    item,
                    recorded: None,
                })
                .collect(),
        }
    }

    pub(super) fn record(
        &mut self,
        request_id: u64,
        update: &ResultUpdate,
        admission_id: Option<u64>,
    ) -> Result<(), &'static str> {
        if self.id != update.runbook_id {
            return Err("runbook_changed");
        }
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.item.id == update.item_id)
            .ok_or("unknown_runbook_item")?;
        if row.report() == Some(&update.report) {
            return Ok(());
        }
        if update.report.admission_id.is_some() && update.report.admission_id != admission_id {
            return Err("runbook_admission_changed");
        }
        if row.report().is_some_and(|report| report.state.terminal()) {
            return Err("runbook_item_already_reported");
        }
        if update.after_request_id != row.recorded.as_ref().map(|(id, _)| *id) {
            return Err("runbook_item_changed");
        }
        row.recorded = Some((request_id, update.report.clone()));
        self.revision = request_id;
        Ok(())
    }

    fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for row in &self.rows {
            match row.report().map(|report| report.state) {
                None => counts.pending += 1,
                Some(State::Running) => counts.running += 1,
                Some(State::Blocked) => counts.blocked += 1,
                Some(State::Skipped) => counts.skipped += 1,
                Some(State::Passed) => counts.passed += 1,
                Some(State::Failed) => counts.failed += 1,
            }
        }
        counts
    }

    pub(super) fn json(&self) -> Value {
        let items: Vec<_> = self
            .rows
            .iter()
            .map(|row| {
                let report = row.report();
                json!({
                    "id":row.item.id,"label":row.item.label,
                    "status":report.map_or("pending", |report| report.state.name()),
                    "note":report.and_then(|report| report.note.as_deref()),
                    "evidence":report.and_then(|report| report.evidence.as_ref().map(Evidence::json)),
                    "admission_id":report.and_then(|report| report.admission_id),
                    "report_request_id":row.recorded.as_ref().map(|(id, _)| *id),
                })
            })
            .collect();
        json!({"id":self.id,"title":self.title,"revision":self.revision,
            "reported_by":"producer","health":self.counts().json(),"items":items})
    }
}

/// Only a currently connected, non-transferring online admission can scope a report.
pub(super) fn admission_id(online: &OnlineState) -> Option<u64> {
    let world = online.world();
    world
        .session_id()
        .filter(|_| online.enabled && world.connected() && world.pending_transfer().is_none())
}

//! Checklist state reported by the local harness, independent of the executor.
use std::collections::HashSet;

use serde_json::{Value, json};

pub(super) mod view;

const MAX_ITEMS: usize = 64;
const MAX_ID_BYTES: usize = 48;
const MAX_TITLE_BYTES: usize = 96;
const MAX_LABEL_BYTES: usize = 120;
const MAX_NOTE_BYTES: usize = 180;

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

/// Only the harness can supply a terminal result. Pending is the absence of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    outcome: Outcome,
    note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResultUpdate {
    runbook_id: u64,
    item_id: String,
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
        let object = object(value, &["runbook_id", "item_id", "status", "note"])?;
        let runbook_id = object
            .get("runbook_id")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
            .ok_or("Runbook result requires a positive runbook_id from status")?;
        let item_id = item_id(object.get("item_id"))?;
        let outcome = match object.get("status").and_then(Value::as_str) {
            Some("passed") => Outcome::Passed,
            Some("failed") => Outcome::Failed,
            _ => return Err("Runbook result status must be passed or failed"),
        };
        let note = object
            .get("note")
            .map(|value| {
                plain_text(Some(value), MAX_NOTE_BYTES)
                    .ok_or("Runbook note must be 1–180 bytes of single-line text")
            })
            .transpose()?;
        Ok(Self {
            runbook_id,
            item_id,
            report: Report { outcome, note },
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
    value
        .and_then(Value::as_str)
        .filter(|text| {
            (1..=MAX_ID_BYTES).contains(&text.len())
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .map(str::to_owned)
        .ok_or("Runbook item ID must be 1–48 ASCII letters, digits, hyphens or underscores")
}

fn plain_text(value: Option<&Value>, max: usize) -> Option<String> {
    let text = value?.as_str()?;
    if text.len() > max
        || text.trim().is_empty()
        || text.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return None;
    }
    Some(text.to_owned())
}

pub(super) struct Run {
    id: u64,
    title: String,
    rows: Vec<Row>,
}

struct Row {
    item: Item,
    report: Option<Report>,
}

impl Run {
    pub(super) fn new(id: u64, definition: Definition) -> Self {
        Self {
            id,
            title: definition.title,
            rows: definition
                .items
                .into_iter()
                .map(|item| Row { item, report: None })
                .collect(),
        }
    }

    pub(super) fn record(&mut self, update: &ResultUpdate) -> Result<(), &'static str> {
        if self.id != update.runbook_id {
            return Err("runbook_changed");
        }
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.item.id == update.item_id)
            .ok_or("unknown_runbook_item")?;
        if let Some(report) = &row.report {
            return if *report == update.report {
                Ok(())
            } else {
                Err("runbook_item_already_reported")
            };
        }
        row.report = Some(update.report.clone());
        Ok(())
    }

    pub(super) fn json(&self) -> Value {
        let items: Vec<_> = self
            .rows
            .iter()
            .map(|row| {
                json!({
                    "id":row.item.id,"label":row.item.label,
                    "status":row.report.as_ref().map_or("pending", |report| report.outcome.name()),
                    "note":row.report.as_ref().and_then(|report| report.note.as_deref()),
                })
            })
            .collect();
        json!({"id":self.id,"title":self.title,"items":items})
    }
}

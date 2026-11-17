//! Bounded local requests. Receipts describe the executor, never server acceptance.
use serde_json::Value;

use super::super::ExpectedWorld;
use super::runbook::{Definition, ResultUpdate};

pub(super) const VERSION: u64 = 1;
pub(super) const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub(super) const MAX_MESSAGE_BYTES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Envelope {
    pub instance: String,
    pub id: u64,
    pub operation: Operation,
    pub expected_world: Option<ExpectedWorld>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Start(String),
    Stop(u64),
    Status,
    RunbookStart(Definition),
    RunbookResult(ResultUpdate),
    RunbookExport(u64),
}

/// Only these fixed fields may enter the local execution boundary.
pub(super) fn parse(bytes: &[u8]) -> Result<Envelope, &'static str> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err("Request exceeds the 256 KiB limit");
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "Request must be valid JSON")?;
    let object = value.as_object().ok_or("Request must be an object")?;
    if object.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err("Request version must be 1");
    }
    let instance = object
        .get("instance")
        .and_then(Value::as_str)
        .filter(|text| {
            !text.is_empty()
                && text.len() <= 64
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        .ok_or("Instance must be the identifier published in status")?;
    let id = object
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id != 0)
        .ok_or("Request id must be a positive unsigned integer")?;
    let operation = match object.get("op").and_then(Value::as_str) {
        Some("start") => Operation::Start(
            object
                .get("source")
                .and_then(Value::as_str)
                .ok_or("Start requires inline source text")?
                .to_owned(),
        ),
        Some("stop") => Operation::Stop(
            object
                .get("run_id")
                .and_then(Value::as_u64)
                .filter(|id| *id != 0)
                .ok_or("Stop requires a positive run_id from status")?,
        ),
        Some("status") => Operation::Status,
        Some("runbook_start") => Operation::RunbookStart(Definition::parse(
            object
                .get("runbook")
                .ok_or("Runbook start requires a runbook")?,
        )?),
        Some("runbook_result") => Operation::RunbookResult(ResultUpdate::parse(
            object
                .get("result")
                .ok_or("Runbook result requires a result")?,
        )?),
        Some("runbook_export") => Operation::RunbookExport(
            object
                .get("runbook_id")
                .and_then(Value::as_u64)
                .filter(|id| *id != 0)
                .ok_or("Runbook export requires a positive runbook_id from status")?,
        ),
        _ => {
            return Err(
                "Operation must be start, stop, status, runbook_start, runbook_result or runbook_export",
            );
        }
    };
    let extra = match &operation {
        Operation::Start(_) => Some("source"),
        Operation::Stop(_) => Some("run_id"),
        Operation::Status => None,
        Operation::RunbookStart(_) => Some("runbook"),
        Operation::RunbookResult(_) => Some("result"),
        Operation::RunbookExport(_) => Some("runbook_id"),
    };
    let expected_world = object
        .get("expected_world")
        .map(expected_world)
        .transpose()?;
    if expected_world.is_some() && !matches!(operation, Operation::Start(_)) {
        return Err("Only start accepts expected_world");
    }
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "version" | "instance" | "id" | "op" | "expected_world"
        ) && Some(key.as_str()) != extra
    }) {
        return Err("Request contains an unexpected field");
    }
    Ok(Envelope {
        instance: instance.to_owned(),
        id,
        operation,
        expected_world,
    })
}

fn expected_world(value: &Value) -> Result<ExpectedWorld, &'static str> {
    let expected = value
        .as_object()
        .ok_or("expected_world must be an object")?;
    if expected.len() != 2 {
        return Err("expected_world requires only admission_id and zone");
    }
    let admission_id = expected
        .get("admission_id")
        .and_then(Value::as_u64)
        .filter(|id| *id != 0)
        .ok_or("expected_world requires a positive admission_id")?;
    let zone = expected
        .get("zone")
        .and_then(Value::as_str)
        .filter(|zone| !zone.is_empty() && zone.len() <= 128 && !zone.chars().any(char::is_control))
        .ok_or(
            "expected_world requires the nonempty zone short name from status (up to 128 bytes)",
        )?;
    Ok(ExpectedWorld {
        admission_id,
        zone: zone.to_owned(),
    })
}

/// Truncate diagnostics at a UTF-8 boundary; source text is never a status field.
pub(super) fn message(text: &str) -> String {
    let mut end = text.len().min(MAX_MESSAGE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_typed_versioned_requests_with_valid_ids_cross_the_boundary() {
        let valid = json!({"version":1,"instance":"client-1","id":1,"op":"status"});
        assert_eq!(
            parse(&serde_json::to_vec(&valid).unwrap())
                .unwrap()
                .operation,
            Operation::Status
        );
        for (field, value) in [
            ("version", json!(2)),
            ("id", json!(0)),
            ("id", json!(-1)),
            ("id", json!(1.5)),
            ("instance", json!("../other")),
            ("op", json!("shell")),
            ("source", json!("ignored")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(
                parse(&serde_json::to_vec(&invalid).unwrap()).is_err(),
                "{invalid}"
            );
        }
        assert!(parse(b"[]").is_err());
        assert!(parse(b"{").is_err());
        assert!(parse(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    }

    #[test]
    fn starts_are_inline_and_stops_name_a_run_without_replacement_options() {
        let start = br#"{"version":1,"instance":"one","id":2,"op":"start","source":"wait 1\n"}"#;
        assert_eq!(
            parse(start).unwrap().operation,
            Operation::Start("wait 1\n".into())
        );
        let path = br#"{"version":1,"instance":"one","id":2,"op":"start","path":"unread"}"#;
        assert!(parse(path).is_err());
        let stop = br#"{"version":1,"instance":"one","id":3,"op":"stop","run_id":1}"#;
        assert_eq!(parse(stop).unwrap().operation, Operation::Stop(1));
        let replace =
            br#"{"version":1,"instance":"one","id":3,"op":"stop","run_id":1,"then":"start"}"#;
        assert!(parse(replace).is_err());
    }

    #[test]
    fn diagnostics_are_bounded_without_splitting_utf8() {
        let bounded = message(&"é".repeat(MAX_MESSAGE_BYTES));
        assert_eq!(bounded.len(), MAX_MESSAGE_BYTES);
        assert!(bounded.chars().all(|character| character == 'é'));
    }

    #[test]
    fn world_scope_is_optional_but_requires_both_identifiers_only_on_start() {
        let start = json!({"version":1,"instance":"one","id":1,"op":"start","source":"wait 1\n"});
        assert!(
            parse(&serde_json::to_vec(&start).unwrap())
                .unwrap()
                .expected_world
                .is_none()
        );
        for expected in [
            json!(null),
            json!({"admission_id":7}),
            json!({"zone":"samplezone"}),
            json!({"admission_id":0,"zone":"samplezone"}),
            json!({"admission_id":7,"zone":""}),
            json!({"admission_id":7,"zone":"samplezone","extra":true}),
        ] {
            let mut invalid = start.clone();
            invalid["expected_world"] = expected;
            assert!(parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        let mut valid = start;
        valid["expected_world"] = json!({"admission_id":7,"zone":"SampleZone"});
        assert_eq!(
            parse(&serde_json::to_vec(&valid).unwrap())
                .unwrap()
                .expected_world
                .unwrap()
                .zone,
            "SampleZone"
        );
        valid["op"] = json!("status");
        valid.as_object_mut().unwrap().remove("source");
        assert!(parse(&serde_json::to_vec(&valid).unwrap()).is_err());
    }
}

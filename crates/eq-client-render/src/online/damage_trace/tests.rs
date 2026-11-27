use super::*;
use bevy::log::tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct Capture {
    enabled: bool,
    events: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
}

impl Subscriber for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.enabled && metadata.target() == "eq_client_render::damage_lines"
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.events.lock().unwrap().push(fields.0);
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}

fn damage() -> WorldUpdate {
    WorldUpdate::Game(WorldEvent::Damage(Damage {
        source_id: 7,
        target_id: 7,
        kind: 1,
        outcome: eq_client_core::combat::DamageOutcome::Hit(33),
        spell_id: None,
        push: None,
    }))
}

fn push(history: &mut ChatHistory) {
    history.push(crate::chat::system_line(
        "private body omitted from provenance",
    ));
}

#[test]
fn disabled_provenance_keeps_no_receipt_or_counter() {
    let capture = Capture::default();
    bevy::log::tracing::subscriber::with_default(capture.clone(), || {
        let mut ordinal = 0;
        let mut history = ChatHistory::default();
        push(&mut history);
        assert!(begin(&mut ordinal, &damage(), Some(7), &history).is_none());
        assert_eq!(ordinal, 0);
        assert_eq!(history.lines(ChatTab::All).len(), 1);
    });
    assert!(capture.events.lock().unwrap().is_empty());
}

#[test]
fn separate_identical_updates_keep_separate_ordinals_and_history_ids() {
    let capture = Capture {
        enabled: true,
        ..Capture::default()
    };
    bevy::log::tracing::subscriber::with_default(capture.clone(), || {
        let mut ordinal = 0;
        let mut history = ChatHistory::default();
        for _ in 0..2 {
            let receipt = begin(&mut ordinal, &damage(), Some(7), &history).unwrap();
            push(&mut history);
            receipt.finish(&Changes::default(), &history);
        }
        assert_eq!(history.lines(ChatTab::All).len(), 2);
    });
    let events = capture.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["update_ordinal"], (index + 1).to_string());
        assert_eq!(event["history_ids"], format!("[{}]", index + 1));
        assert_eq!(event["source"], "damage");
    }
}

#[test]
fn one_update_with_two_rows_is_distinct_from_two_updates() {
    let capture = Capture {
        enabled: true,
        ..Capture::default()
    };
    bevy::log::tracing::subscriber::with_default(capture.clone(), || {
        let mut ordinal = 0;
        let mut history = ChatHistory::default();
        let receipt = begin(&mut ordinal, &damage(), Some(7), &history).unwrap();
        push(&mut history);
        push(&mut history);
        receipt.finish(&Changes::default(), &history);
    });
    let events = capture.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["history_ids"], "[1, 2]");
}

#[test]
fn alternative_chat_sources_record_ids_without_bodies_or_arguments() {
    let capture = Capture {
        enabled: true,
        ..Capture::default()
    };
    bevy::log::tracing::subscriber::with_default(capture.clone(), || {
        let mut ordinal = 0;
        let mut history = ChatHistory::default();
        let chat = WorldUpdate::Chat(crate::chat::system_line("secret chat payload"));
        let message = WorldUpdate::ServerMessage {
            string_id: 12162,
            arguments: vec!["secret string argument".into()],
            message_type: None,
        };
        for update in [chat, message] {
            let receipt = begin(&mut ordinal, &update, Some(7), &history).unwrap();
            push(&mut history);
            receipt.finish(&Changes::default(), &history);
        }
    });
    let events = capture.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["source"], "chat");
    assert_eq!(events[1]["source"], "server_message");
    assert_eq!(events[1]["string_id"], "Some(12162)");
    let logged = format!("{events:?}");
    for secret in [
        "secret chat payload",
        "secret string argument",
        "private body omitted",
    ] {
        assert!(!logged.contains(secret));
    }
}

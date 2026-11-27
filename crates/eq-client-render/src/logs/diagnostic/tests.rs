use super::*;
use bevy::log::tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use eq_client_core::chat::{ChannelName, Source};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture {
    opt_in: bool,
    texts: Arc<Mutex<Vec<String>>>,
}

impl Subscriber for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.opt_in && metadata.target() == "eq_client_render::chat_lines"
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut text = Text::default();
        event.record(&mut text);
        self.texts.lock().unwrap().extend(text.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[derive(Default)]
struct Text(Option<String>);
impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "text" {
            self.0 = Some(format!("{value:?}"));
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "text" {
            self.0 = Some(value.to_owned());
        }
    }
}

fn trace(capture: &Capture, history: &ChatHistory, player: Option<&str>, seen: &mut u64) {
    bevy::log::tracing::subscriber::with_default(capture.clone(), || {
        trace_history(
            history,
            player,
            Some(&crate::hud::messages::Messages::default()),
            seen,
        );
    });
}

#[test]
fn ordinary_chat_and_formatted_combat_lines_trace_once_without_a_script_or_character_log() {
    let capture = Capture {
        opt_in: true,
        ..default()
    };
    let mut history = ChatHistory::default();
    let mut seen = 0;
    let mut chat = crate::chat::system_line(crate::chat::Said::official("Hail"));
    chat.channel = ChannelName::Say;
    chat.sender = Some("Example".into());
    chat.source = Source::Server;
    history.push(chat);
    let damage = eq_client_core::combat::Damage {
        source_id: 7,
        target_id: 9,
        kind: 1,
        outcome: eq_client_core::combat::DamageOutcome::Miss,
        spell_id: None,
        push: None,
    };
    let messages = crate::hud::messages::Messages::parse(
        "EQST0002\n0 0\n12185 You\n12179 slash\n12156 miss!\n12158 %1 try to %2 %3, but %4\n",
    );
    history.push(crate::chat::system_line(
        crate::combat::damage_text(&messages, 7, |_| "a bat".into(), &damage).unwrap(),
    ));
    trace(&capture, &history, Some("Example"), &mut seen);
    trace(&capture, &history, Some("Example"), &mut seen);
    let texts = capture.texts.lock().unwrap();
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0], "You (say): Hail");
    assert_eq!(texts[1], "You try to slash a bat, but miss!");
}

#[test]
fn disabled_and_pre_admission_chat_is_never_dumped_on_later_opt_in() {
    let off = Capture::default();
    let on = Capture {
        opt_in: true,
        ..default()
    };
    let mut history = ChatHistory::default();
    let mut seen = 0;
    history.push(crate::chat::system_line("Before admission"));
    trace(&on, &history, None, &mut seen);
    history.push(crate::chat::system_line("Disabled diagnostic"));
    trace(&off, &history, Some("Example"), &mut seen);
    trace(&on, &history, Some("Example"), &mut seen);
    assert_eq!(*on.texts.lock().unwrap(), Vec::<String>::new());
    assert_eq!(*off.texts.lock().unwrap(), Vec::<String>::new());
    history.push(crate::chat::system_line("New visible line"));
    trace(&on, &history, Some("Example"), &mut seen);
    assert_eq!(*on.texts.lock().unwrap(), ["New visible line"]);
}

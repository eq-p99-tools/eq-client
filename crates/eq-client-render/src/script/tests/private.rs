//! Tells a script reads stay on the player's screen (rule O-b): a run that
//! reads `chat.last_tell` and `chat.last_tell_from` writes neither to a
//! log, a `report()` line, the run's events (which the control status and
//! Runbook export carry) or anything else it records.

use super::{frame_at, pointing, set_focus};
use crate::{
    online::{OnlineState, testing as session},
    script::{Phase, Program, RunStatus, Script},
};
use bevy::{
    ecs::schedule::SingleThreadedExecutor,
    log::{
        tracing::{
            Event,
            field::{Field, Visit},
            subscriber,
        },
        tracing_subscriber::{
            Layer, Registry,
            layer::{Context, SubscriberExt},
        },
    },
    prelude::*,
    window::PrimaryWindow,
};
use eq_client_core::chat::{ChannelName, ChatLine, Message, Source};
use std::{
    fmt::Write as _,
    path::Path,
    sync::{Arc, Mutex},
};

const WORDS: &str = "meet me by the docks";
const SENDER: &str = "Whisperer";

/// Every field of every event logged, as text.
struct Everything(Arc<Mutex<Vec<String>>>);

impl Layer<Registry> for Everything {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, Registry>) {
        let mut line = String::new();
        event.record(&mut Fields(&mut line));
        self.0.lock().unwrap().push(line);
    }
}

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let _ = write!(self.0, " {}={value}", field.name());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

#[test]
fn a_run_reading_tells_logs_and_reports_neither() {
    let mut app = pointing(Vec::new());
    app.init_resource::<crate::script::RunControl>()
        .init_resource::<RunStatus>()
        .add_systems(Update, crate::script::observe.after(crate::script::drive))
        .edit_schedule(Update, |schedule| {
            schedule.set_executor(SingleThreadedExecutor::new());
        });
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    set_focus(&mut app, window, true);
    let mut online = OnlineState::new(true);
    session::admit(&mut online, 9, session::player(7));
    app.insert_resource(online);
    app.world_mut()
        .resource_mut::<crate::chat::ChatState>()
        .history
        .push(ChatLine {
            channel: ChannelName::Tell,
            message_type: None,
            sender: Some(SENDER.into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: WORDS.into(),
                item_links: Vec::new(),
            },
            source: Source::Server,
        });
    let source = format!(
        "said: text = chat.last_tell\nfrom: text = chat.last_tell_from\nif said == \"{WORDS}\" and from == \"{SENDER}\":\n    report(\"heard\")\n"
    );
    let program = Program::parse(&source, Path::new(".")).unwrap();
    app.insert_resource(Script::from_program(program));
    let logged = Arc::new(Mutex::new(Vec::new()));
    let capture = Registry::default().with(Everything(Arc::clone(&logged)));
    subscriber::with_default(capture, || {
        for frame in 0..40 {
            frame_at(&mut app, frame * 120);
        }
    });
    let status = app.world().resource::<RunStatus>();
    assert_eq!(status.phase(), Phase::Completed, "{:?}", status.events());
    let logged = logged.lock().unwrap();
    // The run did read them: its report ran.
    assert!(
        logged.iter().any(|line| line.contains("heard")),
        "{logged:#?}"
    );
    for private in [WORDS, SENDER] {
        assert!(
            !logged.iter().any(|line| line.contains(private)),
            "a log carried {private:?}"
        );
        assert!(
            !status
                .events()
                .iter()
                .any(|event| event.message.contains(private)),
            "an event carried {private:?}"
        );
    }
}

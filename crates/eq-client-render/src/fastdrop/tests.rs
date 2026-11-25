use super::*;
use crate::chat::submit_typed_line;
use crate::online::{OnlineState, testing};
use eq_client_core::options::{NoSafeDrop, Options};

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ChatState>()
        .init_resource::<crate::options::OptionsState>()
        .add_systems(Update, answers);
    app
}

fn ask(app: &mut App, command: &str) -> Option<String> {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(
        crate::chat::client_request_in_scope(command, &mut chat, None),
        Some(Ok(()))
    );
    app.update();
    app.world()
        .resource::<ChatState>()
        .history
        .lines(eq_client_core::chat::ChatTab::System)
        .last()
        .map(|(_, line)| line.message.text.clone())
}

#[test]
fn original_command_requires_a_recognized_word() {
    assert_eq!(Asked::typed("/fastdrop"), Some(Asked(None)));
    assert_eq!(
        Asked::typed("/FASTDROP on"),
        Some(Asked(Some(NoSafeDrop::Fast)))
    );
    assert!(Asked::typed("/fastdropx").is_none());
    assert!(Asked::typed("fastdrop").is_none());
    assert_eq!(
        crate::commands::spelled("/fa").as_deref(),
        Some("/fastdrop")
    );
}

#[test]
fn original_modes_persist_and_invalid_input_leaves_the_choice() {
    let mut app = app();
    assert_eq!(
        app.world()
            .resource::<crate::options::OptionsState>()
            .options
            .no_safe_drop,
        NoSafeDrop::Ask
    );
    for (command, expected) in [
        ("/fastdrop on", NoSafeDrop::Fast),
        ("/fastdrop", NoSafeDrop::Fast),
        ("/fastdrop invalid", NoSafeDrop::Fast),
        ("/fastdrop never", NoSafeDrop::Never),
        ("/fastdrop off", NoSafeDrop::Ask),
    ] {
        assert!(ask(&mut app, command).is_some());
        let options = &app
            .world()
            .resource::<crate::options::OptionsState>()
            .options;
        assert_eq!(options.no_safe_drop, expected);
        assert_eq!(
            Options::read(&options.text(), Options::default()).no_safe_drop,
            expected
        );
    }
}

#[test]
fn a_social_or_script_line_turns_it_over_as_typing_does() {
    // A social, a hotbutton or a script's slash runs the line as typed
    // (6.584): the drop's question is turned over the same way.
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(7));
    let outbox = crate::outbox::Outbox::new(None);
    let mut chat = ChatState::default();
    submit_typed_line(
        "/fastdrop never",
        &mut chat,
        &mut online,
        &outbox,
        &eq_client_core::qol::Settings::default(),
    )
    .unwrap();
    assert!(chat.fastdrop_asked.is_some());
}

#[test]
fn invalid_saved_values_safely_ask() {
    for value in ["2", "-2", "invalid"] {
        let defaults = Options {
            no_safe_drop: NoSafeDrop::Fast,
            ..Options::default()
        };
        assert_eq!(
            Options::read(&format!("NoSafeDrop = {value}"), defaults).no_safe_drop,
            NoSafeDrop::Ask
        );
    }
}

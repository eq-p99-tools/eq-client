use super::*;
use crate::online::testing;
use eq_client_core::{ClientCommand, OutboundChat, chat::ChatTab};
use std::sync::mpsc::Receiver;

/// A player in admission 5 who knows the common tongue fully and language 3
/// at 25, with `/language` answered each frame.
fn app() -> (App, Receiver<ClientCommand>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let mut online = OnlineState::new(false);
    let mut player = testing::player(7);
    player.languages = Some(vec![100, 0, 0, 25]);
    testing::admit(&mut online, 5, player);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<OptionsState>()
        .init_resource::<ChatState>()
        .init_resource::<Messages>()
        .add_systems(Update, (command, speak).chain());
    (app, receiver)
}

fn ask(app: &mut App, typed: &str) {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(crate::chat::client_request(typed, &mut chat), Some(Ok(())));
    app.update();
}

fn lines(app: &App) -> Vec<String> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(ChatTab::All)
        .into_iter()
        .map(|(_, line)| line.message.text.clone())
        .collect()
}

fn say(app: &App) {
    let online = app.world().resource::<OnlineState>();
    app.world()
        .resource::<Outbox>()
        .send(
            online.world(),
            ClientCommand::SendChat(OutboundChat::Say("mae govannen".into())),
        )
        .unwrap();
}

#[test]
fn language_alone_lists_the_languages_the_character_knows() {
    let (mut app, _sent) = app();
    ask(&mut app, "/language");
    assert_eq!(
        lines(&app),
        ["1 - Common Tongue (100) (speaking)", "4 - Language 3 (25)"]
    );
}

#[test]
fn a_number_chooses_a_known_language_which_every_typed_line_then_goes_out_in() {
    let (mut app, sent) = app();
    // Before a choice, a line goes out as before.
    say(&app);
    assert!(matches!(sent.try_recv(), Ok(ClientCommand::SendChat(_))));
    ask(&mut app, "/language 4");
    assert_eq!(app.world().resource::<OptionsState>().options.language, 3);
    assert_eq!(lines(&app), ["You are now speaking Language 3."]);
    say(&app);
    let player = testing::player(7);
    let mut knows = player;
    knows.languages = Some(vec![100, 0, 0, 25]);
    assert_eq!(
        sent.try_recv().unwrap(),
        ClientCommand::SendChatInAdmission {
            session_id: 5,
            message: OutboundChat::Say("mae govannen".into()),
            tongue: Tongue::spoken(&knows, 3).unwrap(),
        }
    );
    ask(&mut app, "/language 1");
    assert_eq!(app.world().resource::<OptionsState>().options.language, 0);
    say(&app);
    assert!(matches!(sent.try_recv(), Ok(ClientCommand::SendChat(_))));
}

#[test]
fn a_language_the_character_lacks_or_a_word_that_names_none_is_refused() {
    let (mut app, _sent) = app();
    ask(&mut app, "/language 2");
    assert_eq!(
        app.world().resource::<ChatState>().newest(),
        "You do not know that language."
    );
    for typed in ["/language 0", "/language 26", "/language elvish"] {
        ask(&mut app, typed);
        assert!(
            app.world()
                .resource::<ChatState>()
                .newest()
                .starts_with("Use /language")
        );
    }
    assert_eq!(app.world().resource::<OptionsState>().options.language, 0);
}

#[test]
fn a_saved_language_the_character_no_longer_knows_speaks_the_common_tongue() {
    let (mut app, sent) = app();
    app.world_mut()
        .resource_mut::<OptionsState>()
        .options
        .language = 9;
    app.update();
    say(&app);
    assert!(matches!(sent.try_recv(), Ok(ClientCommand::SendChat(_))));
}

#[test]
fn heard_tongues_are_named_by_the_installation_or_in_this_clients_words() {
    let messages = Messages::parse("EQST0002\n0 2\n3203 Elvish\n3115 Old Speech\n");
    assert_eq!(tongue_name(Some(&messages), 4), "Elvish");
    assert_eq!(tongue_name(Some(&messages), 27), "Old Speech");
    assert_eq!(tongue_name(None, 4), "Language 4");
    assert_eq!(tongue_name(None, 27), "an unknown tongue");
    assert_eq!(tongue_name(None, 200), "an unknown tongue");
}

use super::*;
use crate::online::testing;
use eq_client_core::{
    ClientCommand, OutboundChat,
    chat::{ChannelName, ChatLine, ChatTab, Message, Source},
};
use std::sync::mpsc::Receiver;

/// Installed lines in a made-up wording, by the ids the program reads.
const STRINGS: &str = "EQST0002
0 2
1411  in %1
1412  in an unknown tongue
1417 AFK:
1418 AFK Message:
1419 Sorry, I am AFK right now.
3114 Common Tongue
3202 Dwarvish
3205 Gnomish
13260 Languages:
13261 Not a language number.
13262 Speaking %1 (%2).
13300 help
";

/// A player in admission 5 who knows the common tongue fully, Dwarvish
/// (language 3) at 25 and Gnomish (6) at 40.
fn app() -> (App, Receiver<ClientCommand>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let mut online = OnlineState::new(false);
    let mut player = testing::player(7);
    player.languages = Some(vec![100, 0, 0, 25, 0, 0, 40]);
    testing::admit(&mut online, 5, player);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(sender)))
        .init_resource::<ChatState>()
        .insert_resource(Messages::parse(STRINGS))
        .add_systems(Update, (command, speak, away).chain());
    (app, receiver)
}

fn ask(app: &mut App, typed: &str) {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(crate::chat::client_request(typed, &mut chat), Some(Ok(())));
    app.update();
}

fn lines(app: &App) -> Vec<(String, Option<u32>)> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(ChatTab::All)
        .into_iter()
        .map(|(_, line)| (line.message.text.clone(), line.message_type))
        .collect()
}

fn texts(app: &App) -> Vec<String> {
    lines(app).into_iter().map(|(text, _)| text).collect()
}

fn say(app: &App) {
    let online = app.world().resource::<OnlineState>();
    app.world()
        .resource::<Outbox>()
        .send(
            online.world(),
            ClientCommand::SendChat(OutboundChat::Say("hail".into())),
        )
        .unwrap();
}

fn player() -> PlayerState {
    let mut player = testing::player(7);
    player.languages = Some(vec![100, 0, 0, 25, 0, 0, 40]);
    player
}

#[test]
fn the_help_word_lists_the_known_languages_and_empty_says_the_current_one() {
    let (mut app, _sent) = app();
    ask(&mut app, "/language HELP");
    assert_eq!(
        texts(&app),
        [
            "Languages:",
            "1 - Common Tongue",
            "2 - Dwarvish",
            "3 - Gnomish",
            "Speaking Common Tongue (1).",
        ]
    );
    // Every line is message type 0x111.
    assert!(lines(&app).iter().all(|(_, kind)| *kind == Some(0x111)));
    ask(&mut app, "/language");
    assert_eq!(texts(&app).last().unwrap(), "Speaking Common Tongue (1).");
}

#[test]
fn a_number_is_an_index_into_the_known_list_and_the_send_carries_its_tongue() {
    let (mut app, sent) = app();
    say(&app);
    assert!(matches!(sent.try_recv(), Ok(ClientCommand::SendChat(_))));
    // The second known language is Dwarvish, language 3, not language 1.
    ask(&mut app, "/language 2");
    assert_eq!(texts(&app).last().unwrap(), "Speaking Dwarvish (2).");
    say(&app);
    assert_eq!(
        sent.try_recv().unwrap(),
        ClientCommand::SendChatInAdmission {
            session_id: 5,
            message: OutboundChat::Say("hail".into()),
            tongue: Tongue::spoken(&player(), 3, eq_client_core::chat::Speaker::default()).unwrap(),
        }
    );
    ask(&mut app, "/language 3");
    assert_eq!(texts(&app).last().unwrap(), "Speaking Gnomish (3).");
}

#[test]
fn a_number_outside_the_list_or_no_number_is_refused() {
    let (mut app, _sent) = app();
    for typed in [
        "/language 0",
        "/language 4",
        "/language abc",
        "/language dwarvish",
    ] {
        ask(&mut app, typed);
        assert_eq!(texts(&app).last().unwrap(), "Not a language number.");
    }
    ask(&mut app, "/language");
    assert_eq!(texts(&app).last().unwrap(), "Speaking Common Tongue (1).");
}

#[test]
fn each_chat_window_keeps_its_own_choice_for_the_session() {
    let (mut app, _sent) = app();
    ask(&mut app, "/language 2");
    let main = app.world().resource::<ChatState>().active_window();
    let other = crate::windows::WindowId::ChatGroup(1);
    {
        let mut chat = app.world_mut().resource_mut::<ChatState>();
        assert_eq!(chat.languages.index(main), 1);
        assert_eq!(chat.languages.index(other), 0);
        chat.languages.choose(other, 2);
        assert_eq!(chat.languages.index(main), 1);
    }
    let online = app.world().resource::<OnlineState>();
    let chat = app.world().resource::<ChatState>();
    assert_eq!(chat.languages.language(main, online.world().player()), 3);
    assert_eq!(chat.languages.language(other, online.world().player()), 6);
    // An index the list no longer reaches speaks language 0.
    let mut forgot = player();
    forgot.languages = Some(vec![100]);
    assert_eq!(chat.languages.language(main, Some(&forgot)), 0);
    // The session's end forgets every choice.
    app.world_mut().resource_mut::<ChatState>().end_session();
    let chat = app.world().resource::<ChatState>();
    assert_eq!(chat.languages.index(main), 0);
}

fn tell(sender: &str, text: &str) -> ChatLine {
    ChatLine {
        channel: ChannelName::Tell,
        sender: Some(sender.into()),
        target: None,
        message: Message {
            message: None,
            message_hex: None,
            text: text.into(),
            item_links: Vec::new(),
        },
        source: Source::Server,
        message_type: None,
        tongue: Some(Heard {
            language: 0,
            skill: 100,
            channel: 7,
            named: false,
        }),
    }
}

fn hear(app: &mut App, line: ChatLine) {
    let online = app.world_mut().remove_resource::<OnlineState>().unwrap();
    app.world_mut()
        .resource_mut::<ChatState>()
        .receive(line, &online);
    app.world_mut().insert_resource(online);
    app.update();
}

fn away_now(app: &mut App) {
    let mut online = app.world_mut().resource_mut::<OnlineState>();
    let mut player = player();
    player.listing.away = true;
    testing::admit(&mut online, 5, player);
}

#[test]
fn a_tell_heard_while_away_gets_the_away_reply_unless_it_is_one() {
    let (mut app, sent) = app();
    // Not away: no reply.
    hear(&mut app, tell("Friend", "hello"));
    assert!(sent.try_recv().is_err());
    away_now(&mut app);
    hear(&mut app, tell("Friend", "hello"));
    assert_eq!(
        sent.try_recv().unwrap(),
        ClientCommand::SendChatInAdmission {
            session_id: 5,
            message: OutboundChat::Tell {
                recipient: "Friend".into(),
                message: "Sorry, I am AFK right now.".into(),
            },
            tongue: Tongue::COMMON,
        }
    );
    // Another's away reply gets none back.
    hear(&mut app, tell("Friend", "AFK: Sorry, I am AFK right now."));
    hear(&mut app, tell("Friend", "AFK Message: out to lunch"));
    assert!(sent.try_recv().is_err());
}

#[test]
fn an_ignored_sender_gets_no_away_reply() {
    let (mut app, sent) = app();
    away_now(&mut app);
    app.world_mut()
        .resource_mut::<ChatState>()
        .contacts
        .lists
        .ignored
        .toggle("Pest", 100);
    hear(&mut app, tell("Pest", "hello"));
    assert!(sent.try_recv().is_err());
    assert!(!texts(&app).iter().any(|text| text.contains("hello")));
}

#[test]
fn heard_tongues_are_tagged_by_the_installed_lines() {
    let messages = Messages::parse(STRINGS);
    let heard = |named| Heard {
        language: 3,
        skill: 100,
        channel: 8,
        named,
    };
    assert_eq!(tongue_place(Some(&messages), heard(true)), " in Dwarvish");
    assert_eq!(
        tongue_place(Some(&messages), heard(false)),
        " in an unknown tongue"
    );
    assert_eq!(tongue_place(None, heard(true)), ", in Language 3");
    assert_eq!(tongue_place(None, heard(false)), ", in an unknown tongue");
}

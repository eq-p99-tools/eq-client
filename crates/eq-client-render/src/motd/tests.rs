use super::*;
use crate::online::{OnlineState, testing};
use crate::outbox::Outbox;
use eq_client_core::chat::{ChannelName, ChatLine, ChatTab, Message, Source};

fn motd_line(text: &str) -> ChatLine {
    ChatLine {
        channel: ChannelName::Motd,
        message_type: None,
        sender: None,
        target: None,
        message: Message {
            message: None,
            message_hex: None,
            text: text.into(),
            item_links: Vec::new(),
        },
        source: Source::Server,
        tongue: None,
    }
}

/// An app with a session that sent nothing yet, and an outbox whose queue
/// the test holds.
fn app() -> (
    App,
    std::sync::mpsc::Receiver<eq_client_core::ClientCommand>,
) {
    let mut online = OnlineState::new(true);
    // The Mac-era program's `/motd` is unread: the message said again as it
    // came. Titanium's is the program's line (below).
    testing::generation(&mut online, eq_client_core::GameDialect::EqMac);
    testing::connect(&mut online, true);
    let (queue, received) = std::sync::mpsc::sync_channel(4);
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(Outbox::new(Some(queue)))
        .init_resource::<ChatState>()
        .add_systems(Update, answers);
    (app, received)
}

fn hear(app: &mut App, line: ChatLine) {
    let world = app.world_mut();
    world.resource_scope(|world, mut chat: Mut<ChatState>| {
        chat.receive(line, world.resource::<OnlineState>());
    });
}

fn ask(app: &mut App) {
    let mut chat = app.world_mut().resource_mut::<ChatState>();
    assert_eq!(
        crate::chat::client_request_in_scope("/motd", &mut chat, None),
        Some(Ok(()))
    );
    app.update();
}

fn motd_texts(app: &App) -> Vec<String> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(ChatTab::for_channel(ChannelName::Motd))
        .into_iter()
        .filter(|(_, line)| line.channel == ChannelName::Motd)
        .map(|(_, line)| line.message.text.clone())
        .collect()
}

fn system_texts(app: &App) -> Vec<String> {
    app.world()
        .resource::<ChatState>()
        .history
        .lines(ChatTab::System)
        .into_iter()
        .filter(|(_, line)| line.channel == ChannelName::System)
        .map(|(_, line)| line.message.text.clone())
        .collect()
}

#[test]
fn the_words_are_motd_spelled_out_and_ignore_what_follows() {
    assert!(typed("/motd"));
    assert!(typed("/MOTD again please"));
    assert!(!typed("/motdx"));
    assert!(!typed("motd"));
    // A typed abbreviation reaches here spelled out.
    assert_eq!(crate::commands::spelled("/mo").as_deref(), Some("/motd"));
}

#[test]
fn the_message_kept_is_said_again_each_time_and_nothing_is_sent() {
    let (mut app, received) = app();
    hear(&mut app, motd_line("Welcome to the test server."));
    ask(&mut app);
    ask(&mut app);
    // The same line, on the MOTD's own channel, said again twice: the line
    // said again is not kept in its place.
    assert_eq!(motd_texts(&app), ["Welcome to the test server."; 3]);
    assert_eq!(system_texts(&app), Vec::<String>::new());
    assert!(received.try_recv().is_err());
    // A later message of the day is the one said again.
    hear(&mut app, motd_line("Second message."));
    ask(&mut app);
    assert_eq!(
        motd_texts(&app).last().map(String::as_str),
        Some("Second message.")
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn without_a_message_this_clients_line_is_said_and_nothing_is_sent() {
    let (mut app, received) = app();
    // Another channel's line is no message of the day.
    let mut other = motd_line("A broadcast.");
    other.channel = ChannelName::Broadcast;
    hear(&mut app, other);
    ask(&mut app);
    assert_eq!(system_texts(&app), ["There is no message of the day."]);
    assert_eq!(motd_texts(&app), Vec::<String>::new());
    assert!(received.try_recv().is_err());
}

#[test]
fn a_new_connection_never_says_the_old_servers_message() {
    let (mut app, received) = app();
    hear(&mut app, motd_line("The first server's message."));
    app.world_mut().resource_mut::<OnlineState>().restart();
    ask(&mut app);
    assert_eq!(system_texts(&app), ["There is no message of the day."]);
    assert_eq!(motd_texts(&app), ["The first server's message."]);
    // The new connection's own message is said.
    hear(&mut app, motd_line("The second server's message."));
    ask(&mut app);
    assert_eq!(
        motd_texts(&app),
        [
            "The first server's message.",
            "The second server's message.",
            "The second server's message."
        ]
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn on_titanium_motd_says_the_kept_message_in_the_programs_line_even_empty() {
    let (mut app, received) = app();
    testing::generation(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        eq_client_core::GameDialect::Titanium,
    );
    app.insert_resource(crate::hud::messages::Messages::parse(
        "EQST0002\n0 1\n12965 MOTD: %1\n",
    ));
    // None came: the line still says, with nothing in it.
    ask(&mut app);
    assert_eq!(system_texts(&app), ["MOTD: "]);
    hear(&mut app, motd_line("Welcome."));
    ask(&mut app);
    assert_eq!(system_texts(&app), ["MOTD: ", "MOTD: Welcome."]);
    assert!(received.try_recv().is_err());
}

//! The chat, logged as the official client logs it, for parsers and timers
//! that read its logs: on unless the character's options or else the
//! installation's `eqclient.ini` turn it off, `/log` turns it on and off and
//! the character's options keep that, and each line goes to
//! `Logs\eqlog_<character>_<server>.txt` in the installation.
use bevy::prelude::*;
use eq_client_core::logs;
use std::io::Write as _;

/// The file the chat goes to, and how far it has gone.
#[derive(Resource, Default)]
pub(crate) struct ChatLog {
    /// The newest chat line written or passed over.
    seen: u64,
    /// The open log, by its file name.
    file: Option<(String, std::fs::File)>,
}

/// Turns logging on or off for a `/log`, saying so as the official client
/// does, and writes each new chat line to the character's log.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn write(
    (settings, messages): (
        Res<crate::ViewerSettings>,
        Option<Res<crate::hud::messages::Messages>>,
    ),
    online: Res<crate::online::OnlineState>,
    mut chat: ResMut<crate::chat::ChatState>,
    (mut log, mut options): (ResMut<ChatLog>, ResMut<crate::options::OptionsState>),
) {
    let mut writes = options.options.log;
    if chat.log_toggle {
        chat.log_toggle = false;
        let on = !options.options.log;
        options.options.log = on;
        let (id, fallback) = if on {
            (logs::LOGGING_ON, logs::LOGGING_ON_TEXT)
        } else {
            (logs::LOGGING_OFF, logs::LOGGING_OFF_TEXT)
        };
        let words = messages.as_deref().map_or_else(
            || fallback.to_owned(),
            |messages| messages.text(id, fallback),
        );
        // The official client writes these lines itself, so the log takes
        // them even in this client's words, where the installation lacks
        // the string.
        chat.history
            .push(crate::chat::system_line(crate::chat::Said::official(words)));
        // The official client writes its *OFF* line before it stops, as it
        // writes its *ON* line as it starts.
        writes = true;
    }
    let lines = chat.history.lines(eq_client_core::chat::ChatTab::All);
    let Some(newest) = lines.last().map(|(id, _)| *id) else {
        return;
    };
    if newest <= log.seen {
        return;
    }
    let seen = std::mem::replace(&mut log.seen, newest);
    // Lines before a character is in the world, or while logging is off,
    // are passed over.
    let (true, Some(directory), Some(player), Some(server)) = (
        writes,
        settings.0.eq_directory.as_deref(),
        online.world().player(),
        online.world().world_name(),
    ) else {
        return;
    };
    let now = chrono::Local::now().naive_local();
    let mut text = String::new();
    // A line in this client's own words is never the official client's,
    // so the log leaves it out (`logs::words`).
    for (_, line) in lines.iter().filter(|(id, _)| *id > seen) {
        if let Some(words) = logs::words(line, &player.name) {
            text.push_str(&logs::line(now, &read(words, messages.as_deref())));
        }
    }
    if text.is_empty() {
        return;
    }
    let name = logs::file_name(&player.name, server);
    if log.file.as_ref().is_none_or(|(open, _)| *open != name) {
        let folder = directory.join("Logs");
        let opened = std::fs::create_dir_all(&folder).and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(folder.join(&name))
        });
        match opened {
            Ok(file) => log.file = Some((name, file)),
            Err(error) => {
                warn!("Could not open the chat log {name}: {error}");
                log.file = None;
                return;
            }
        }
    }
    if let Some((name, file)) = log.file.as_mut()
        && let Err(error) = file.write_all(text.as_bytes())
    {
        warn!("Could not write the chat log {name}: {error}");
        log.file = None;
    }
}

/// A log line's words: the installed client's string where the line has
/// one and the installation has the table, and this client's words
/// otherwise.
fn read(words: logs::Words, messages: Option<&crate::hud::messages::Messages>) -> String {
    match (words.string_id, messages) {
        (Some(id), Some(messages)) => messages.official(id, &words.arguments, &words.text),
        _ => words.text,
    }
}

/// A chat line as the chat window shows it: in the words its log writes,
/// without the time, with its item links moved to where its message lands
/// in those words. A line in this client's own words, which the log leaves
/// out, shows as it reads.
pub(crate) fn shown(
    line: &eq_client_core::chat::ChatLine,
    player: &str,
    messages: Option<&crate::hud::messages::Messages>,
) -> eq_client_core::chat::Message {
    // The words are read with a mark in the message's place, so its links
    // can follow it wherever the installed string puts it.
    const MARK: &str = "\u{1}";
    let mut marked = line.clone();
    MARK.clone_into(&mut marked.message.text);
    marked.message.item_links.clear();
    let mut message = line.message.clone();
    let Some(words) = logs::words(&marked, player) else {
        return message;
    };
    let words = read(words, messages);
    if let Some((before, after)) = words.split_once(MARK) {
        message.text = format!("{before}{}{after}", line.message.text);
        for link in &mut message.item_links {
            link.text_start += before.len();
            link.text_end += before.len();
        }
    } else {
        // A string that leaves the message out leaves its links out too.
        message.text = words;
        message.item_links.clear();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    #[test]
    fn speech_reads_by_the_installed_string() {
        let messages = crate::hud::messages::Messages::parse(
            "EQST0002
0 1
1410 %1 speaks%2: %3
",
        );
        let words = || logs::Words {
            string_id: Some(1410),
            arguments: vec!["Examplar".into(), String::new(), "Hail".into()],
            text: "Examplar (say): Hail".into(),
        };
        assert_eq!(read(words(), Some(&messages)), "Examplar speaks: Hail");
        assert_eq!(read(words(), None), "Examplar (say): Hail");
        // A string the table lacks falls back to this client's words.
        let missing = logs::Words {
            string_id: Some(1416),
            ..words()
        };
        assert_eq!(read(missing, Some(&messages)), "Examplar (say): Hail");
    }

    #[test]
    fn raid_speech_keeps_the_empty_tongues_place() {
        use eq_client_core::chat::{ChannelName, ChatLine, Message, Source};
        let messages = crate::hud::messages::Messages::parse(
            "EQST0002
0 2
5109 Mine, '%1'
5112 %1 raid, %2 '%3'
",
        );
        let line = |sender: &str| ChatLine {
            channel: ChannelName::Raid,
            message_type: None,
            sender: Some(sender.into()),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: "go".into(),
                item_links: Vec::new(),
            },
            source: Source::Server,
        };
        // Another's line leaves the tongue's place empty, so two spaces stay
        // before the quote, as the installed string has them.
        assert_eq!(
            shown(&line("Examplar"), "Tester", Some(&messages)).text,
            "Examplar raid,  'go'"
        );
        // The player's own line, which the session records as the others
        // hear it.
        assert_eq!(
            shown(&line("Tester"), "Tester", Some(&messages)).text,
            "Mine, 'go'"
        );
    }

    #[test]
    fn the_chat_shows_a_line_in_its_logs_words_with_its_links() {
        use eq_client_core::chat::{ChannelName, ChatLine, Message, Source};
        let messages = crate::hud::messages::Messages::parse(
            "EQST0002
0 1
1410 %1 speaks%2: %3
",
        );
        let dagger = eq_client_core::ItemLink {
            body: "synthetic".into(),
            text: "Rusty Dagger".into(),
            item_id: 7,
            start: 0,
            end: 0,
            text_start: 4,
            text_end: 16,
        };
        let line = |channel, sender: Option<&str>, source| ChatLine {
            channel,
            message_type: None,
            sender: sender.map(str::to_owned),
            target: None,
            message: Message {
                message: None,
                message_hex: None,
                text: "WTS Rusty Dagger".into(),
                item_links: vec![dagger.clone()],
            },
            source,
        };
        // The link's words, wherever the line's words put them.
        let linked = |message: &Message| {
            let [only] = &message.item_links[..] else {
                panic!("one link");
            };
            message.text[only.text_start..only.text_end].to_owned()
        };
        let say = line(ChannelName::Say, Some("Examplar"), Source::Server);
        let heard = shown(&say, "Other", Some(&messages));
        assert_eq!(heard.text, "Examplar speaks: WTS Rusty Dagger");
        assert_eq!(linked(&heard), "Rusty Dagger");
        // Without the table, the line is in this client's words.
        let fallback = shown(&say, "Other", None);
        assert_eq!(fallback.text, "Examplar (say): WTS Rusty Dagger");
        assert_eq!(linked(&fallback), "Rusty Dagger");
        // The game's lines and this client's own read as they are.
        for source in [Source::Server, Source::Client] {
            let system = shown(
                &line(ChannelName::System, None, source),
                "Other",
                Some(&messages),
            );
            assert_eq!(system.text, "WTS Rusty Dagger");
            assert_eq!(linked(&system), "Rusty Dagger");
        }
    }

    #[test]
    fn the_chat_goes_to_the_characters_log_until_log_turns_it_off() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-logs-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&directory);
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some(directory.clone());
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        let mut player = testing::player(7);
        player.name = "Examplar".into();
        testing::admit(&mut online, 1, player);
        app.insert_resource(online)
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<ChatLog>()
            .add_systems(Update, write);
        let say = |app: &mut App, said: crate::chat::Said| {
            app.world_mut()
                .resource_mut::<crate::chat::ChatState>()
                .history
                .push(crate::chat::system_line(said));
            app.update();
        };
        let official = crate::chat::Said::official;
        say(&mut app, official("Arrived in The Qeynos Hills."));
        // This client's own words are never the official client's, so the
        // log leaves them out.
        say(&mut app, "Nobody here by that name.".into());
        say(&mut app, official("\u{c9}p\u{e9}e \u{2026}"));
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .log_toggle = true;
        app.update();
        say(&mut app, official("Not logged."));
        let file = directory
            .join("Logs")
            .join("eqlog_Examplar_ExampleWorld.txt");
        let written = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 3, "{written}");
        assert!(lines[0].starts_with('['));
        assert!(lines[0].ends_with("] Arrived in The Qeynos Hills."));
        // Non-ASCII text goes as the UTF-8 it came in.
        assert!(lines[1].ends_with("] \u{c9}p\u{e9}e \u{2026}"));
        // The official client writes its *OFF* line, then nothing more.
        assert!(lines[2].ends_with(logs::LOGGING_OFF_TEXT));
        // Its *ON* line starts the log again.
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .log_toggle = true;
        app.update();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(
            written
                .lines()
                .nth(3)
                .is_some_and(|line| line.ends_with(logs::LOGGING_ON_TEXT)),
            "{written}"
        );
        // Every line ends as the official client ends its lines.
        assert!(written.ends_with("\r\n"));
        assert_eq!(
            written.matches("\r\n").count(),
            written.matches('\n').count()
        );
        assert_eq!(written.matches('\n').count(), 4);
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .log_toggle = true;
        app.update();
        // And the character's options keep the choice.
        assert!(
            !app.world()
                .resource::<crate::options::OptionsState>()
                .options
                .log
        );
        // The chat says logging went off.
        let chat = app.world().resource::<crate::chat::ChatState>();
        assert!(
            chat.history
                .lines(eq_client_core::chat::ChatTab::All)
                .iter()
                .any(|(_, line)| line.message.text == logs::LOGGING_OFF_TEXT)
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}

//! The chat, logged as the official client logs it, for parsers and timers
//! that read its logs: on from login unless the installation's
//! `eqclient.ini` turns it off, `/log` turns it on and off, and each line
//! goes to `Logs\eqlog_<character>_<server>.txt` in the installation.
use bevy::prelude::*;
use eq_client_core::logs;
use std::io::Write as _;

/// Whether the chat is logged, and the file it goes to.
#[derive(Resource)]
pub(crate) struct ChatLog {
    /// Whether new lines are written.
    on: bool,
    /// The newest chat line written or passed over.
    seen: u64,
    /// The open log, by its file name.
    file: Option<(String, std::fs::File)>,
}

impl ChatLog {
    /// A log that starts on or off; the client starts it as `eqclient.ini`
    /// says, or on.
    pub(crate) const fn new(on: bool) -> Self {
        Self {
            on,
            seen: 0,
            file: None,
        }
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new(true)
    }
}

/// Turns logging on or off for a `/log`, saying so as the official client
/// does, and writes each new chat line to the character's log.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn write(
    settings: Res<crate::ViewerSettings>,
    online: Res<crate::online::OnlineState>,
    mut chat: ResMut<crate::chat::ChatState>,
    mut log: ResMut<ChatLog>,
) {
    if chat.log_toggle {
        chat.log_toggle = false;
        log.on = !log.on;
        let words = if log.on {
            logs::LOGGING_ON
        } else {
            logs::LOGGING_OFF
        };
        chat.history
            .push(crate::chat::system_line(words.to_owned()));
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
        log.on,
        settings.0.eq_directory.as_deref(),
        online.world().player(),
        online.world().world_name(),
    ) else {
        return;
    };
    let now = chrono::Local::now().naive_local();
    let mut text = String::new();
    for (_, line) in lines.iter().filter(|(id, _)| *id > seen) {
        text.push_str(&logs::line(now, &logs::words(line, &player.name)));
        text.push('\n');
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};

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
            .insert_resource(ChatLog::new(true))
            .add_systems(Update, write);
        let say = |app: &mut App, text: &str| {
            app.world_mut()
                .resource_mut::<crate::chat::ChatState>()
                .history
                .push(crate::chat::system_line(text.to_owned()));
            app.update();
        };
        say(&mut app, "You have entered The Qeynos Hills.");
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .log_toggle = true;
        app.update();
        say(&mut app, "Not logged.");
        let file = directory
            .join("Logs")
            .join("eqlog_Examplar_ExampleWorld.txt");
        let written = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 1, "{written}");
        assert!(lines[0].starts_with('['));
        assert!(lines[0].ends_with("] You have entered The Qeynos Hills."));
        // The chat says logging went off.
        let chat = app.world().resource::<crate::chat::ChatState>();
        assert!(
            chat.history
                .lines(eq_client_core::chat::ChatTab::All)
                .iter()
                .any(|(_, line)| line.message.text == logs::LOGGING_OFF)
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}

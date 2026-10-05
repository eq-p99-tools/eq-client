//! The sessions the graphical client runs, each on its own thread: what one
//! logs in with, and the thread's ownership.
mod movement;

use anyhow::Result;
use eq_client_core::{MotionCalibration, WorldUpdate, food::AutoEat, world::Link};
use eq_network::{
    assets::Assets,
    client::{
        CancellationToken, Client, ClientConfig, ClientEvent, ClientIdentity, ConnectionState,
        LoginError, RecordEvent, RunOptions, ServerProtocol,
    },
};
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};
use zeroize::Zeroizing;

/// Chat lines keep their text; string-table messages are resolved by presentation.
fn chat_update(event: eq_network::chat::ChatEvent) -> Option<WorldUpdate> {
    use eq_client_core::chat::Spoken;
    let spoken =
        eq_client_core::chat::spoken(event.speak_mode, event.language, event.sender.as_deref());
    // An NPC's special say, shout or emote reads as the server's own
    // formatted NPC line of that kind, so the chat and the log word it alike.
    if let (Some(Spoken::String(string_id)), Some(speaker), Some(message)) =
        (spoken, &event.sender, &event.message)
    {
        return Some(WorldUpdate::ServerMessage {
            string_id,
            arguments: vec![speaker.clone().into(), message.clone().into()],
            message_type: event.message_type,
        });
    }
    if let Some(string_id) = event.string_id {
        return Some(WorldUpdate::ServerMessage {
            string_id,
            arguments: event
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            message_type: event.message_type,
        });
    }
    event
        .message
        .filter(|message| !message.text.is_empty())
        .map(|message| {
            WorldUpdate::Chat(eq_client_core::chat::ChatLine {
                // A special word to the group reads as a group line.
                channel: match spoken {
                    Some(Spoken::Channel(channel)) => channel,
                    _ => event.channel_name,
                },
                sender: event.sender,
                target: event.target,
                message,
                source: eq_client_core::chat::Source::Server,
                message_type: event.message_type,
            })
        })
}

/// What a session logs in with.
pub struct Login {
    /// The server type.
    pub protocol: ServerProtocol,
    /// The login server's address.
    pub host: String,
    /// The login server's port.
    pub port: u16,
    /// The account's name.
    pub account: String,
    /// The password, wiped once the session's configuration holds it.
    pub password: Zeroizing<String>,
    /// The world to play on; empty lists the login server's worlds.
    pub server: String,
    /// The character to enter; empty lists the world's characters.
    pub character: String,
}

/// What every session of a run shares.
#[derive(Clone)]
pub struct SessionOptions {
    /// The installation, whose files a Titanium session's login checks.
    pub install: PathBuf,
    /// Ends each session after this many seconds, its login included.
    pub seconds: Option<u64>,
    /// Independently measured movement speeds.
    pub calibration: Option<MotionCalibration>,
    /// Refuses servers outside this machine's network (a scripted test run).
    pub local_only: bool,
    /// What the session eats and drinks on its own.
    pub auto_eat: AutoEat,
}

/// Owns a session's thread: dropping it cancels the session and waits for
/// the thread to end.
pub struct SessionWorker {
    cancel: CancellationToken,
    worker: Option<JoinHandle<()>>,
    /// Why the session ended, once it has, in the player's words.
    reason: Arc<Mutex<Option<String>>>,
}

impl SessionWorker {
    /// Starts a session on its own thread. The configuration is checked
    /// here; the installation's files are read, and the login made, on the
    /// thread, so starting never holds up the window.
    ///
    /// # Errors
    /// Fails when the login's details cannot make a session, such as an
    /// account too long for the server type.
    pub fn start(login: Login, options: &SessionOptions) -> Result<eq_client_render::Session> {
        let protocol = login.protocol;
        let client = client(login, options)?;
        let install = options.install.clone();
        let cancel = CancellationToken::default();
        let worker_cancel = cancel.clone();
        // The limit covers the whole session: login, character select and every zone.
        if let Some(seconds) = options.seconds {
            let deadline = cancel.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(seconds));
                deadline.cancel();
            });
        }
        let reason = Arc::new(Mutex::new(None));
        let ended = reason.clone();
        let (sender, receiver) = mpsc::sync_channel(1024);
        let (commands, command_queue) = mpsc::sync_channel(32);
        let configuration = commands.clone();
        let calibration = options.calibration;
        let worker = thread::spawn(move || {
            let mut movement = movement::Continuity::new(calibration);
            let mut options = RunOptions::default();
            options.reconnect = false;
            let result = (|| {
                // A Titanium login sends what the installation's files hold.
                let client = if protocol.is_titanium() {
                    client.with_assets(Assets::scan_all(&install)?)?
                } else {
                    client
                };
                client.run_with_commands(&worker_cancel, options, &command_queue, |event| {
                    let update = match event {
                        ClientEvent::World(event) => {
                            if let Some(command) =
                                movement.observe(&event, std::time::Instant::now())
                                && let Err(error) = configuration.try_send(command)
                            {
                                // Movement stays disabled; the session itself is fine.
                                tracing::warn!(
                                    "Movement calibration was not carried over: {error}"
                                );
                            }
                            Some(WorldUpdate::Game(event))
                        }
                        ClientEvent::Status(status) => {
                            Some(WorldUpdate::Connection(link(status.state)))
                        }
                        ClientEvent::Progress(stage) => Some(progress_update(stage)),
                        ClientEvent::Record(record) => match record.event {
                            RecordEvent::Chat(event) => chat_update(event),
                            _ => None,
                        },
                        ClientEvent::Diagnostic(message) => {
                            tracing::info!("{message}");
                            None
                        }
                        // A reconnect is a fresh login; this viewer asks for
                        // none, but says so if one comes.
                        ClientEvent::Reconnecting {
                            error,
                            delay_seconds,
                        } => {
                            tracing::warn!("Reconnecting in {delay_seconds} s: {error}");
                            Some(WorldUpdate::Connection(Link::LoggingIn))
                        }
                    };
                    if let Some(update) = update {
                        match sender.try_send(update) {
                            Ok(()) => (),
                            Err(mpsc::TrySendError::Disconnected(_)) => worker_cancel.cancel(),
                            Err(mpsc::TrySendError::Full(_)) => {
                                anyhow::bail!("viewer event queue full")
                            }
                        }
                    }
                    Ok(())
                })
            })();
            if let Err(error) = result {
                tracing::error!("Session ended: {error:#}");
                // A session asked to stop ends without a reason to show.
                if !worker_cancel.is_cancelled()
                    && let Ok(mut reason) = ended.lock()
                {
                    *reason = Some(player_words(&error));
                }
                // Waits for room if the queue is full; fails only once the viewer is gone.
                let _ = sender.send(WorldUpdate::Connection(Link::Ended));
            }
        });
        Ok(eq_client_render::Session {
            updates: receiver,
            commands,
            worker: Box::new(Self {
                cancel,
                worker: Some(worker),
                reason,
            }),
        })
    }
}

impl eq_client_render::Worker for SessionWorker {
    fn stop(&self) {
        self.cancel.cancel();
    }

    fn finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn reason(&self) -> Option<String> {
        self.reason.lock().ok()?.clone()
    }
}

/// Why a session ended, as the login screen says it: the login server's
/// refusal of the account, or what went wrong, in the library's words.
fn player_words(error: &anyhow::Error) -> String {
    match error.downcast_ref::<LoginError>() {
        Some(refusal) => refusal.to_string(),
        None => format!("The session ended: {error}"),
    }
}

/// Ready is connected; treating it as a disconnect would discard admission data.
fn progress_update(stage: eq_network::client::ConnectionStage) -> WorldUpdate {
    use eq_network::client::ConnectionStage;
    WorldUpdate::Connection(match stage {
        ConnectionStage::Ready => Link::Connected,
        ConnectionStage::ConnectingZone
        | ConnectionStage::LoadingCharacter
        | ConnectionStage::EnteringWorld => Link::Entering,
        // The login and world servers, and the stages a later protocol adds
        // before the zone.
        _ => Link::LoggingIn,
    })
}

/// The session's coarse state in the player's terms.
fn link(state: ConnectionState) -> Link {
    match state {
        ConnectionState::Connected => Link::Connected,
        ConnectionState::Zoning => Link::Zoning,
        ConnectionState::Disconnected | ConnectionState::Stopped => Link::Ended,
        _ => Link::LoggingIn,
    }
}

/// The session's client, configured from the login: the login server, the
/// account, and the world and character it goes straight to where named.
fn client(login: Login, options: &SessionOptions) -> Result<Client> {
    let mut config = ClientConfig::for_protocol(
        login.protocol,
        login.account,
        login.password.as_str(),
        login.server,
        login.character,
    );
    config.host = login.host;
    config.port = login.port;
    config.local_only = options.local_only;
    config.auto_eat = options.auto_eat;
    let hostname = env::var("COMPUTERNAME").or_else(|_| env::var("HOSTNAME"));
    let username = env::var("USERNAME").or_else(|_| env::var("USER"));
    Client::new(
        config,
        ClientIdentity::new(
            identity_field(hostname.ok(), "EQCLIENT"),
            identity_field(username.ok(), "PLAYER"),
        ),
    )
}

/// Up to 15 printable ASCII bytes of an identity field (the login identity is
/// ASCII), or the fallback when none are left.
fn identity_field(value: Option<String>, fallback: &str) -> String {
    let field: String = value
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .take(15)
        .collect();
    if field.trim().is_empty() {
        fallback.into()
    } else {
        field
    }
}

impl Drop for SessionWorker {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_npcs_special_message_reads_as_its_kind_of_npc_line() {
        use eq_client_core::chat::{NPC_EMOTE, NPC_SAY, NPC_SHOUT};
        // A Titanium special message: the speak mode, the journal mode and
        // the language, the type, the target, the speaker, twelve unused
        // bytes, then the text.
        let special = |mode: u8, speaker: &[u8]| {
            let mut body = vec![mode, 0, 0];
            body.extend_from_slice(&10u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(speaker);
            body.push(0);
            body.extend_from_slice(&[0; 12]);
            body.extend_from_slice(b"Welcome, traveler.\0");
            let event = eq_network::chat::parse_for(
                eq_network::GameDialect::Titanium,
                0x2372,
                &body,
                false,
            )
            .unwrap()
            .unwrap();
            chat_update(event)
        };
        let worded = |update| match update {
            Some(WorldUpdate::ServerMessage {
                string_id,
                arguments,
                message_type,
            }) => {
                // The type the server gave the line comes with it.
                assert_eq!(message_type, Some(10));
                Some((string_id, arguments))
            }
            _ => None,
        };
        let named = |id| Some((id, vec!["Quest giver".into(), "Welcome, traveler.".into()]));
        assert_eq!(worded(special(1, b"Quest giver")), named(NPC_SAY));
        assert_eq!(worded(special(2, b"Quest giver")), named(NPC_SHOUT));
        assert_eq!(worded(special(4, b"Quest giver")), named(NPC_EMOTE));
        // A word to the group is a group line under its speaker's name.
        let Some(WorldUpdate::Chat(group)) = special(5, b"Quest giver") else {
            panic!("a group line");
        };
        assert_eq!(
            (group.channel, group.sender.as_deref()),
            (
                eq_client_core::chat::ChannelName::Group,
                Some("Quest giver")
            )
        );
        // A plain server line reads as its text.
        let Some(WorldUpdate::Chat(plain)) = special(0, b"") else {
            panic!("a plain line");
        };
        assert_eq!(
            (plain.channel, plain.sender, plain.message.text.as_str()),
            (
                eq_client_core::chat::ChannelName::System,
                None,
                "Welcome, traveler."
            )
        );
    }

    #[test]
    fn ready_progress_keeps_the_just_delivered_admission_snapshot() {
        use eq_network::client::ConnectionStage;
        assert!(matches!(
            progress_update(ConnectionStage::Ready),
            WorldUpdate::Connection(Link::Connected)
        ));
        assert!(matches!(
            progress_update(ConnectionStage::ConnectingZone),
            WorldUpdate::Connection(Link::Entering)
        ));
        assert!(matches!(
            progress_update(ConnectionStage::Authenticating),
            WorldUpdate::Connection(Link::LoggingIn)
        ));
    }

    #[test]
    fn identity_fields_are_short_ascii() {
        assert_eq!(
            super::identity_field(Some("Пользователь".into()), "PLAYER"),
            "PLAYER"
        );
        assert_eq!(
            super::identity_field(Some("DESKTOP-ABCDEFGHIJK".into()), "EQCLIENT"),
            "DESKTOP-ABCDEFG"
        );
        assert_eq!(super::identity_field(None, "EQCLIENT"), "EQCLIENT");
    }
}

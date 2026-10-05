//! Worker ownership and environment-only credentials for the graphical consumer.
mod movement;

use anyhow::{Context, Result};
use eq_client_core::{ClientCommand, MotionCalibration, WorldUpdate, food::AutoEat, world::Link};
use eq_network::{
    assets::Assets,
    client::{
        CancellationToken, Client, ClientConfig, ClientEvent, ClientIdentity, ConnectionState,
        RecordEvent, RunOptions, ServerProtocol,
    },
};
use std::{
    env,
    path::Path,
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

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

/// Owns shutdown: closing the viewer cancels and joins its network worker.
pub struct SessionWorker {
    cancel: CancellationToken,
    worker: Option<JoinHandle<()>>,
    commands: mpsc::SyncSender<ClientCommand>,
}

impl SessionWorker {
    /// Returns the bounded command sender owned by this session.
    pub fn commands(&self) -> mpsc::SyncSender<ClientCommand> {
        self.commands.clone()
    }

    /// Starts one selected server session. Secrets come from the process environment.
    /// A local-only session refuses servers outside this machine's network.
    pub fn start(
        install: &Path,
        protocol: ServerProtocol,
        seconds: Option<u64>,
        calibration: Option<MotionCalibration>,
        local_only: bool,
        auto_eat: AutoEat,
    ) -> Result<(Self, Receiver<WorldUpdate>)> {
        let client = client_from_environment(install, protocol, local_only, auto_eat)?;
        let cancel = CancellationToken::default();
        let worker_cancel = cancel.clone();
        // The limit covers the whole session: login, character select and every zone.
        if let Some(seconds) = seconds {
            let deadline = cancel.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(seconds));
                deadline.cancel();
            });
        }
        let (sender, receiver) = mpsc::sync_channel(1024);
        let (commands, command_queue) = mpsc::sync_channel(32);
        let configuration = commands.clone();
        let worker = thread::spawn(move || {
            let mut movement = movement::Continuity::new(calibration);
            let mut options = RunOptions::default();
            options.reconnect = false;
            let result =
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
                });
            if let Err(error) = result {
                // Waits for room if the queue is full; fails only once the viewer is gone.
                let _ = sender.send(WorldUpdate::Connection(Link::Ended));
                tracing::error!("Session ended: {error:#}");
            }
        });
        Ok((
            Self {
                cancel,
                worker: Some(worker),
                commands,
            },
            receiver,
        ))
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

/// Builds a protocol-specific client without loading P99 checksums for Quarm.
fn client_from_environment(
    install: &Path,
    protocol: ServerProtocol,
    local_only: bool,
    auto_eat: AutoEat,
) -> Result<Client> {
    let value = |name| env::var(name).with_context(|| format!("missing {name}"));
    let mut config = ClientConfig::for_protocol(
        protocol,
        value("EQ_ACCOUNT")?,
        value("EQ_PASSWORD")?,
        value("EQ_SERVER")?,
        env::var("EQ_CHARACTER").unwrap_or_default(),
    );
    // Local servers (for example stock EQEmu) name their own login endpoint.
    if let Ok(host) = env::var("EQ_LOGIN_HOST") {
        config.host = host;
    }
    if let Ok(port) = env::var("EQ_LOGIN_PORT") {
        config.port = port.parse().context("EQ_LOGIN_PORT is not a port number")?;
    }
    config.local_only = local_only;
    config.auto_eat = auto_eat;
    let hostname = env::var("COMPUTERNAME").or_else(|_| env::var("HOSTNAME"));
    let username = env::var("USERNAME").or_else(|_| env::var("USER"));
    let client = Client::new(
        config,
        ClientIdentity::new(
            identity_field(hostname.ok(), "EQCLIENT"),
            identity_field(username.ok(), "PLAYER"),
        ),
    )?;
    let client = if protocol.is_titanium() {
        client.with_assets(Assets::scan_all(install)?)?
    } else {
        client
    };
    Ok(client)
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

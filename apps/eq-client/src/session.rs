//! Worker ownership and environment-only credentials for the graphical consumer.
mod movement;

use anyhow::{Context, Result};
use eq_client_core::{ClientCommand, MotionCalibration, WorldUpdate, world::Link};
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
    if let Some(string_id) = event.string_id {
        return Some(WorldUpdate::ServerMessage {
            string_id,
            arguments: event
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(|argument| argument.text)
                .collect(),
        });
    }
    event
        .message
        .filter(|message| !message.text.is_empty())
        .map(|message| {
            WorldUpdate::Chat(eq_client_core::chat::ChatLine {
                channel: event.channel_name,
                sender: event.sender,
                target: event.target,
                message,
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
    ) -> Result<(Self, Receiver<WorldUpdate>)> {
        anyhow::ensure!(
            calibration.is_none() || protocol.is_titanium(),
            "calibrated movement requires the Titanium protocol"
        );
        let client = client_from_environment(install, protocol, local_only)?;
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

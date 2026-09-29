//! Worker ownership and environment-only credentials for the graphical consumer.
mod movement;

use anyhow::{Context, Result};
use eq_client_core::{ClientCommand, MotionCalibration, WorldUpdate};
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
    pub fn start(
        install: &Path,
        seconds: Option<u64>,
        calibration: Option<MotionCalibration>,
    ) -> Result<(Self, Receiver<WorldUpdate>)> {
        if calibration.is_some() {
            let protocol: ServerProtocol = env::var("EQ_PROTOCOL")
                .unwrap_or_else(|_| "p99".into())
                .parse()?;
            anyhow::ensure!(
                protocol.is_titanium(),
                "calibrated movement requires the Titanium protocol"
            );
        }
        let client = client_from_environment(install)?;
        let cancel = CancellationToken::default();
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1024);
        let (commands, command_queue) = mpsc::sync_channel(32);
        let configuration = commands.clone();
        let worker = thread::spawn(move || {
            let mut movement = movement::Continuity::new(calibration);
            let mut options = RunOptions::default();
            options.reconnect = false;
            options.zone_duration = seconds.map(Duration::from_secs);
            let result =
                client.run_with_commands(&worker_cancel, options, &command_queue, |event| {
                    let update = match event {
                        ClientEvent::World(event) => {
                            if let Some(command) =
                                movement.observe(&event, std::time::Instant::now())
                            {
                                configuration
                                    .try_send(command)
                                    .context("cannot queue movement calibration")?;
                            }
                            Some(WorldUpdate::Game(event))
                        }
                        ClientEvent::Status(status) => Some(WorldUpdate::Connection {
                            connected: status.state == ConnectionState::Connected,
                            terminal: matches!(
                                status.state,
                                ConnectionState::Stopped | ConnectionState::Disconnected
                            ),
                            label: format!("{:?}", status.state),
                        }),
                        ClientEvent::Progress(stage) => Some(progress_update(stage)),
                        ClientEvent::Record(record) => match record.event {
                            RecordEvent::Chat(event) => chat_update(event),
                            _ => None,
                        },
                        ClientEvent::Diagnostic(message) => {
                            eprintln!("{message}");
                            None
                        }
                        _ => None,
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
                let _ = sender.try_send(WorldUpdate::Connection {
                    connected: false,
                    terminal: true,
                    label: "Disconnected".into(),
                });
                eprintln!("Session ended: {error:#}");
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
    WorldUpdate::Connection {
        connected: matches!(stage, eq_network::client::ConnectionStage::Ready),
        terminal: false,
        label: format!("{stage:?}"),
    }
}

/// Builds a protocol-specific client without loading P99 checksums for Quarm.
fn client_from_environment(install: &Path) -> Result<Client> {
    let value = |name| env::var(name).with_context(|| format!("missing {name}"));
    let protocol: ServerProtocol = env::var("EQ_PROTOCOL")
        .unwrap_or_else(|_| "p99".into())
        .parse()?;
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
    let hostname = env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_else(|_| "EQCLIENT".into());
    let username = env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "PLAYER".into());
    let client = Client::new(
        config,
        ClientIdentity::new(
            hostname.chars().take(15).collect::<String>(),
            username.chars().take(15).collect::<String>(),
        ),
    )?;
    let client = if protocol.is_titanium() {
        client.with_assets(Assets::scan_all(install)?)?
    } else {
        client
    };
    Ok(client)
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
            WorldUpdate::Connection {
                connected: true,
                terminal: false,
                ..
            }
        ));
        assert!(matches!(
            progress_update(ConnectionStage::ConnectingZone),
            WorldUpdate::Connection {
                connected: false,
                terminal: false,
                ..
            }
        ));
    }
}

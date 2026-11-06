//! The sessions the graphical client runs, each on its own thread: what one
//! logs in with, and the thread's ownership.
mod machine;

use anyhow::Result;
use eq_client_core::{
    WorldEvent, WorldUpdate, food::AutoEat, machine::DisplaySnapshot, world::Link,
};
use eq_network::{
    assets::Assets,
    client::{
        CancellationToken, Client, ClientConfig, ClientEvent, ClientIdentity, ConnectionState,
        LoginError, PlayerAdmissionError, RecordEvent, RunOptions, ServerProtocol,
    },
};
use std::{
    env,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
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
    /// This client's settings directory, where it keeps what it must
    /// remember between runs, such as its last machine report's date.
    pub settings: Option<PathBuf>,
    /// The window's display, for the machine's weekly report.
    pub display: DisplaySnapshot,
    /// Ends each session after this many seconds, its login included.
    pub seconds: Option<u64>,
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
    /// Whether the thread closed its updates itself as it ended.
    closed: Arc<AtomicBool>,
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
        let inventory_move_rules = client.inventory_move_rules();
        let spell_provider =
            eq_client_core::spell_tables::Binding::new(client.spell_table_policy());
        let install = options.install.clone();
        let settings = options.settings.clone();
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
        let closed = Arc::new(AtomicBool::new(false));
        let closing = closed.clone();
        let (sender, receiver) = mpsc::sync_channel(1024);
        let (commands, command_queue) = mpsc::sync_channel(32);
        let worker = thread::spawn(move || {
            let mut spell_bridge = SpellBridge::new(spell_provider);
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
                    if let Some(fact) = spell_bridge.capture(&event)? {
                        enqueue(&sender, &worker_cancel, WorldUpdate::SpellTable(fact))?;
                    }
                    let update = match event {
                        ClientEvent::World(WorldEvent::SpellTable(_)) => None,
                        ClientEvent::World(event) => {
                            // The machine's account went out: keep its
                            // date in this client's own settings, so the
                            // week's wait survives this run ending.
                            if let WorldEvent::SystemInfoReported { at } = &event {
                                keep_report_date(settings.as_deref(), *at);
                            }
                            Some(WorldUpdate::Game(event))
                        }
                        ClientEvent::Status(status) => status_update(status.state),
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
                        enqueue(&sender, &worker_cancel, update)?;
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
                end_updates(sender, &closing);
            } else {
                closing.store(true, Ordering::Release);
            }
        });
        Ok(eq_client_render::Session {
            inventory_move_rules,
            spell_provider,
            updates: receiver,
            commands,
            worker: Box::new(Self {
                cancel,
                worker: Some(worker),
                reason,
                closed,
            }),
        })
    }
}

/// Captures provider identity and start ordering in the actual producer,
/// before queueing. No renderer drain can relabel these as a newer provider.
struct SpellBridge {
    binding: eq_client_core::spell_tables::Binding,
    run_id: Option<u64>,
    attempt: u64,
}

impl SpellBridge {
    const fn new(binding: eq_client_core::spell_tables::Binding) -> Self {
        Self {
            binding,
            run_id: None,
            attempt: 0,
        }
    }

    fn capture(
        &mut self,
        event: &ClientEvent,
    ) -> Result<Option<eq_client_core::spell_tables::Envelope>> {
        use eq_client_core::spell_tables::Fact;
        use eq_network::client::ConnectionStage;
        if !self.binding.policy().requires_server_tables() {
            return Ok(None);
        }
        let fact = match event {
            ClientEvent::Status(status) if status.state == ConnectionState::Connecting => {
                // Events.reset creates this ID immediately before the actual
                // login attempt. World/zone handoffs do not call that reset.
                self.attempt = self
                    .attempt
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("spell provider attempt sequence exhausted"))?;
                self.run_id = status.run_id;
                match status.run_id {
                    Some(run_id) => Fact::RunStarted {
                        run_id,
                        attempt: self.attempt,
                    },
                    None => Fact::RunUnavailable {
                        attempt: self.attempt,
                    },
                }
            }
            ClientEvent::Progress(ConnectionStage::ConnectingZone) => {
                let Some(run_id) = self.run_id else {
                    return Ok(None);
                };
                Fact::ZonePending { run_id }
            }
            ClientEvent::World(WorldEvent::Entered { session_id, .. }) => {
                let Some(run_id) = self.run_id else {
                    return Ok(None);
                };
                Fact::ZoneEntered {
                    run_id,
                    session_id: *session_id,
                }
            }
            ClientEvent::World(WorldEvent::SpellTable(event)) => Fact::Table(event.clone()),
            _ => return Ok(None),
        };
        Ok(Some(self.binding.envelope(fact)))
    }
}

/// Losing a provider fact to overload is a session error, just as for the
/// other ordered game updates. A closed renderer cancels the producer.
fn enqueue(
    sender: &mpsc::SyncSender<WorldUpdate>,
    cancel: &CancellationToken,
    update: WorldUpdate,
) -> Result<()> {
    match sender.try_send(update) {
        Ok(()) => Ok(()),
        Err(mpsc::TrySendError::Disconnected(_)) => {
            cancel.cancel();
            Ok(())
        }
        Err(mpsc::TrySendError::Full(_)) => anyhow::bail!("viewer event queue full"),
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

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

/// Why a session ended, as the login screen says it: the login server's
/// refusal of the account, or what went wrong, in the library's words.
fn player_words(error: &anyhow::Error) -> String {
    if let Some(refusal) = error.downcast_ref::<LoginError>() {
        refusal.to_string()
    } else if let Some(admission) = error.downcast_ref::<PlayerAdmissionError>() {
        format!("{admission}. Log in again to retry entering the character.")
    } else {
        format!("The session ended: {error}")
    }
}

/// Ready is connected; treating it as a disconnect would discard admission data.
fn progress_update(stage: eq_network::client::ConnectionStage) -> WorldUpdate {
    use eq_network::client::ConnectionStage;
    WorldUpdate::Connection(match stage {
        ConnectionStage::Ready => Link::Connected,
        ConnectionStage::ConnectingZone
        | ConnectionStage::LoadingCharacter
        | ConnectionStage::EnteringWorld
        | ConnectionStage::Recording => Link::Entering,
        // The login and world servers, and the stages a later protocol adds
        // before the zone.
        _ => Link::LoggingIn,
    })
}

/// The worker sends the terminal update for an error after storing the reason.
/// The network reports Disconnected before returning that error.
fn status_update(state: ConnectionState) -> Option<WorldUpdate> {
    Some(WorldUpdate::Connection(match state {
        ConnectionState::Connected => Link::Connected,
        ConnectionState::Zoning => Link::Zoning,
        ConnectionState::Disconnected => return None,
        ConnectionState::Stopped => Link::Ended,
        _ => Link::LoggingIn,
    }))
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
    // A Titanium client entering a watched world tells it what machine the
    // player entered it on, once a week: this machine's own account, made
    // when it is due and dated by the last one this client or the installed
    // client remembers. Which worlds hear it is the server type's to say
    // (`sends_system_info`).
    if login.protocol.is_titanium() {
        config.sysinfo = Some(Arc::new(machine::HostMachine::new(
            crate::logins::installed_client(login.protocol),
            options.install.clone(),
            options.settings.clone(),
            options.display.clone(),
        )));
    }
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

/// Keeps the date a world heard this machine's account in this client's
/// own settings directory, as the official key `LastSysInfoDate` in its
/// `eqclient.ini`, where the next week's wait reads it back from first; the
/// installed client's files are only ever read. The running session
/// remembers the report itself, so a date that cannot be kept only leaves
/// the next run to report again, and the log to say why.
fn keep_report_date(settings: Option<&std::path::Path>, at: u64) {
    let Some(settings) = settings else {
        tracing::warn!("The machine's report date was not kept: no settings directory");
        return;
    };
    if let Err(error) = eq_client_assets::ui::keep_last_sysinfo_date(settings, at) {
        tracing::warn!("The machine's report date was not kept: {error}");
    }
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

/// Sends the terminal status when there is room, then closes the event stream.
/// A full queue drains in order before the receiver observes disconnection and
/// ends the session. Joining the worker must never depend on that drain.
/// `closed` is set first, so a viewer that finds the stream closed knows the
/// worker ended it, and reads why from the worker's reason.
fn end_updates(sender: mpsc::SyncSender<WorldUpdate>, closed: &AtomicBool) {
    closed.store(true, Ordering::Release);
    let _ = sender.try_send(WorldUpdate::Connection(Link::Ended));
    drop(sender);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_bridge_filters_lifecycle_and_tables_by_network_requirement() {
        use eq_client_core::spell_tables::{Binding, Context, Event, Policy, Stage, Table, Update};
        use eq_network::client::{ConnectionStage, SessionStatus};
        for policy in [Policy::Installed, Policy::Project1999] {
            let mut bridge = SpellBridge::new(Binding::new(policy));
            for event in [
                ClientEvent::Status(
                    SessionStatus::new(ConnectionState::Connecting, "display").with_run_id(7),
                ),
                ClientEvent::Progress(ConnectionStage::ConnectingZone),
                ClientEvent::World(WorldEvent::SpellTable(Event::new(
                    Context::new(7, 1, Stage::World),
                    Update::Table(Table::Registration(Vec::new())),
                ))),
                ClientEvent::Status(SessionStatus::new(ConnectionState::Connecting, "legacy")),
            ] {
                assert_eq!(
                    bridge.capture(&event).unwrap().is_some(),
                    policy.requires_server_tables()
                );
            }
            if !policy.requires_server_tables() {
                assert_eq!(bridge.run_id, None);
                assert_eq!(bridge.attempt, 0);
            }
        }
    }

    #[test]
    fn spell_bridge_captures_creation_binding_and_actual_attempt_order_before_enqueue() {
        use eq_client_core::spell_tables::{Binding, Fact, Policy};
        use eq_network::client::SessionStatus;
        let binding = Binding::new(Policy::Project1999);
        let mut bridge = SpellBridge::new(binding);
        let mut status =
            SessionStatus::new(ConnectionState::Connecting, "original display").with_run_id(7);
        status.session_id = "a display label, not hexadecimal".into();
        let first = bridge
            .capture(&ClientEvent::Status(status))
            .unwrap()
            .unwrap();
        let second = bridge
            .capture(&ClientEvent::Status(
                SessionStatus::new(ConnectionState::Connecting, "another display").with_run_id(8),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(first.binding(), binding);
        assert_eq!(second.binding(), binding);
        assert!(matches!(
            first.fact(),
            Fact::RunStarted {
                run_id: 7,
                attempt: 1
            }
        ));
        assert!(matches!(
            second.fact(),
            Fact::RunStarted {
                run_id: 8,
                attempt: 2
            }
        ));
        let other = Binding::new(Policy::Project1999);
        assert_ne!(first.binding(), other);
        let mut installed = SpellBridge::new(Binding::new(Policy::Installed));
        assert!(
            installed
                .capture(&ClientEvent::Status(SessionStatus::new(
                    ConnectionState::Connecting,
                    "uninterpreted"
                )))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_overloaded_spell_event_bridge_fails_instead_of_dropping_ordered_facts() {
        use eq_client_core::spell_tables::{Binding, Fact, Policy};
        let binding = Binding::new(Policy::Project1999);
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = CancellationToken::default();
        enqueue(&sender, &cancel, WorldUpdate::Connection(Link::LoggingIn)).unwrap();
        let fact = binding.envelope(Fact::RunStarted {
            run_id: 7,
            attempt: 1,
        });
        assert!(enqueue(&sender, &cancel, WorldUpdate::SpellTable(fact.clone())).is_err());
        assert!(matches!(
            receiver.try_recv(),
            Ok(WorldUpdate::Connection(Link::LoggingIn))
        ));
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        enqueue(&sender, &cancel, WorldUpdate::SpellTable(fact)).unwrap();
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn spell_bridge_unknown_identity_does_not_abort_or_reuse_an_old_run() {
        use eq_client_core::spell_tables::{Binding, Fact, Policy};
        use eq_network::client::{ConnectionStage, SessionStatus};
        let mut bridge = SpellBridge::new(Binding::new(Policy::Project1999));
        bridge
            .capture(&ClientEvent::Status(
                SessionStatus::new(ConnectionState::Connecting, "known").with_run_id(7),
            ))
            .unwrap();
        let legacy = bridge
            .capture(&ClientEvent::Status(SessionStatus::new(
                ConnectionState::Connecting,
                "legacy non-hex label",
            )))
            .unwrap()
            .unwrap();
        assert!(matches!(legacy.fact(), Fact::RunUnavailable { attempt: 2 }));
        assert!(
            bridge
                .capture(&ClientEvent::Progress(ConnectionStage::ConnectingZone))
                .unwrap()
                .is_none()
        );
        let known = bridge
            .capture(&ClientEvent::Status(
                SessionStatus::new(ConnectionState::Connecting, "another label").with_run_id(8),
            ))
            .unwrap()
            .unwrap();
        assert!(matches!(
            known.fact(),
            Fact::RunStarted {
                run_id: 8,
                attempt: 3
            }
        ));
    }

    #[test]
    fn full_event_queue_does_not_block_worker_shutdown() {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(WorldUpdate::Connection(Link::LoggingIn))
            .unwrap();
        let (finished, completion) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let closing = closed.clone();
        let worker = thread::spawn(move || {
            end_updates(sender, &closing);
            finished.send(()).unwrap();
        });
        // The receiver stays alive and full while the actual terminal path runs.
        let ended = completion.recv_timeout(Duration::from_secs(2));
        if ended.is_err() {
            drop(receiver);
            worker.join().unwrap();
            panic!("terminal notification blocked on an undrained event queue");
        }
        let owner = SessionWorker {
            cancel: CancellationToken::default(),
            worker: Some(worker),
            reason: Arc::new(Mutex::new(Some("The zone server stopped answering".into()))),
            closed,
        };
        // The viewer finds the stream closed with no end said: the worker
        // closed it, and its own words say why.
        assert!(eq_client_render::Worker::closed(&owner));
        assert_eq!(
            eq_client_render::Worker::reason(&owner).as_deref(),
            Some("The zone server stopped answering")
        );
        drop(owner);
        assert!(matches!(
            receiver.try_recv(),
            Ok(WorldUpdate::Connection(Link::LoggingIn))
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn failed_player_admission_maps_to_an_actionable_terminal_reason() {
        use eq_network::client::ConnectionStage;

        assert!(matches!(
            progress_update(ConnectionStage::Recording),
            WorldUpdate::Connection(Link::Entering)
        ));
        assert!(status_update(ConnectionState::Disconnected).is_none());
        assert_eq!(
            player_words(&PlayerAdmissionError.into()),
            "The zone connected, but your character data could not be read. \
             Log in again to retry entering the character."
        );
    }

    #[test]
    fn terminal_status_is_delivered_when_the_queue_has_room() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let closed = AtomicBool::new(false);
        end_updates(sender, &closed);
        assert!(closed.load(Ordering::Acquire));
        assert!(matches!(
            receiver.try_recv(),
            Ok(WorldUpdate::Connection(Link::Ended))
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn terminal_status_does_not_require_a_receiver() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        end_updates(sender, &AtomicBool::new(false));
    }

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

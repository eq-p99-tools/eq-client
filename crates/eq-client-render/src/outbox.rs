//! The one way the client's commands leave for the session. The outbox
//! stamps a command with the admission it belongs to and the time it was
//! made, refuses what the session cannot take (no session, something the
//! server does not offer, a full or closed queue) with one reason. By default
//! refusals go to chat. A caller with its own status can take the refusal and
//! report it there. Windows grey out
//! what the session does not offer from the same answer, the world's
//! capability report.
mod book;

pub(crate) use self::book::{BOOK_TIMEOUT, BookEditReceipt};
use self::book::{BookQueue, Expected};
use crate::theme;
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, WorldEvent, world::ClientWorld};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    time::Instant,
};

/// What a command must carry: the admission it was made in, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub session_id: u64,
    pub created: Instant,
}

/// Why a command did not leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// There is no session to send it to: the viewer is offline.
    Offline,
    /// The session has not admitted the player to a zone yet.
    NotAdmitted,
    /// The player is between zones or the connection dropped.
    Away,
    /// The player is dead, awaiting their return home.
    Dead,
    /// The session does not let the player do this on this server.
    Unavailable(Capability),
    /// This provider has no complete, usable spell replay for the current zone.
    SpellData,
    /// A saved layout owns the spellbook until its current step settles.
    SpellSet,
    /// The previous local edit is still awaiting its result.
    BookPending,
    /// An edit may have reached the server and its result is unknown.
    BookUnconfirmed,
    /// The queue to the session is full.
    Busy,
    /// The session has ended.
    Ended,
}

impl Refusal {
    /// What the player reads.
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::Offline => "Not connected to a server",
            Self::NotAdmitted => "Enter the world first",
            Self::Away => "You can't do that right now",
            Self::Dead => "You can't do that while dead",
            Self::Unavailable(_) => UNAVAILABLE,
            Self::SpellData => "Spell data is not ready for casting",
            Self::SpellSet => "Wait for the spell set to finish",
            Self::BookPending => "Wait for the current spellbook change",
            Self::BookUnconfirmed => {
                "Spellbook change unconfirmed. Wait for the session or re-enter the zone."
            }
            Self::Busy => "Too many requests at once; try again",
            Self::Ended => "The connection has ended",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.text())
    }
}

impl std::error::Error for Refusal {}

/// The line a window shows for a failed action of its own. A refusal from the
/// outbox is already said in the chat, so the window shows nothing for it.
pub(crate) fn window_line(error: &anyhow::Error) -> String {
    if error.downcast_ref::<Refusal>().is_some() {
        String::new()
    } else {
        error.to_string()
    }
}

/// The reason a control is greyed out, and the line a refused key shows.
pub(crate) const UNAVAILABLE: &str = "Not available on this server";

/// The reason a skin's control this client does not have yet is greyed out,
/// in the client's own words.
pub(crate) const MISSING: &str = "Not in this client yet";

/// The session's command queue, and the refusals not yet shown.
#[derive(Resource)]
pub(crate) struct Outbox {
    queue: Option<SyncSender<ClientCommand>>,
    spell_ready: AtomicBool,
    refused: Mutex<Vec<Refusal>>,
    book: Mutex<BookQueue>,
}

fn interrupts_apply(command: &ClientCommand) -> bool {
    matches!(
        command,
        ClientCommand::Move(_)
            | ClientCommand::Jump { .. }
            | ClientCommand::SetPosture { .. }
            | ClientCommand::CastSpell { .. }
            | ClientCommand::UseItem(_)
            | ClientCommand::Camp { .. }
            | ClientCommand::ExitAtOnce { .. }
    )
}

impl Outbox {
    pub(crate) fn new(queue: Option<SyncSender<ClientCommand>>) -> Self {
        Self {
            queue,
            spell_ready: AtomicBool::new(true),
            refused: Mutex::new(Vec::new()),
            book: Mutex::default(),
        }
    }

    pub(crate) fn set_spell_ready(&self, ready: bool) {
        self.spell_ready.store(ready, Ordering::Relaxed);
    }

    /// Sends to another session's queue from now on, or to none.
    pub(crate) fn connect(&mut self, queue: Option<SyncSender<ClientCommand>>) {
        self.queue = queue;
        self.book = Mutex::default();
    }

    /// Claims the book between set steps only after any manual edit settles.
    pub(crate) fn begin_spell_set(&self, world: &ClientWorld, now: Instant) -> Result<(), Refusal> {
        let mut book = self.book.lock().map_err(|_| Refusal::Busy)?;
        book.can_edit(world.session_id(), false, now)?;
        if crate::spellbook::action_pending(world) {
            return Err(Refusal::BookPending);
        }
        book.applying = true;
        book.interrupted = false;
        Ok(())
    }

    /// Feed ordered results before input can hand the spellbook to Apply.
    pub(crate) fn book_news(&self, event: &WorldEvent, session: Option<u64>) {
        if matches!(event, WorldEvent::BookAction(_) | WorldEvent::Spell(_))
            && let Ok(mut book) = self.book.lock()
        {
            book.observe(event, session);
        }
    }

    /// A fresh admission supplies a new authoritative book and gem layout.
    pub(crate) fn reset_book(&self) {
        if let Ok(mut book) = self.book.lock() {
            book.reset();
        }
    }

    /// An interrupted link can resume with the same worker and book request.
    pub(crate) fn interrupt_book(&self, session: Option<u64>) {
        let newly_unconfirmed = self
            .book
            .lock()
            .is_ok_and(|mut book| book.interrupt(session));
        if newly_unconfirmed {
            self.refused(Refusal::BookUnconfirmed);
        }
    }

    pub(crate) fn applying_spell_set(&self) -> bool {
        self.book.lock().is_ok_and(|book| book.applying)
    }

    pub(crate) fn spell_set_interrupted(&self) -> bool {
        self.book
            .lock()
            .map_or(true, |book| !book.applying || book.interrupted)
    }

    pub(crate) fn end_spell_set(&self) {
        if let Ok(mut book) = self.book.lock() {
            book.applying = false;
        }
    }

    /// The stamp for a command made now in the current admission.
    ///
    /// # Errors
    /// Refuses, and shows why, when there is no session or admission.
    pub(crate) fn stamp(&self, world: &ClientWorld) -> Result<Stamp, Refusal> {
        self.try_stamp(world)
            .inspect_err(|refusal| self.refused(*refusal))
    }

    fn try_stamp(&self, world: &ClientWorld) -> Result<Stamp, Refusal> {
        self.queue.as_ref().ok_or(Refusal::Offline)?;
        let session_id = world.session_id().ok_or(Refusal::NotAdmitted)?;
        Ok(Stamp {
            session_id,
            created: Instant::now(),
        })
    }

    /// The stamp a command made now would carry, for checking a request
    /// before the player makes it; nothing is refused or shown.
    pub(crate) fn peek(&self, world: &ClientWorld) -> Option<Stamp> {
        self.queue.as_ref()?;
        world.in_world().then_some(())?;
        Some(Stamp {
            session_id: world.session_id()?,
            created: Instant::now(),
        })
    }

    /// Sends a command, or refuses it with one reason, which the chat says
    /// ([`show`]).
    ///
    /// # Errors
    /// Refuses a command the session cannot take now.
    pub(crate) fn send(&self, world: &ClientWorld, command: ClientCommand) -> Result<(), Refusal> {
        self.try_send(world, command, false, None)
            .inspect_err(|refusal| self.refused(*refusal))
    }

    /// Stamps and sends the active set's single step through the same checks
    /// as a click. The set owns the refusal feedback, so nothing is shown here.
    ///
    /// # Errors
    /// Refuses a command the session cannot take now.
    pub(crate) fn spell_set_step(
        &self,
        world: &ClientWorld,
        command: impl FnOnce(Stamp) -> ClientCommand,
    ) -> Result<BookEditReceipt, Refusal> {
        let stamp = self.try_stamp(world)?;
        self.try_book_edit(world, command(stamp), true)
    }

    /// Stamps a manual book edit and gives its caller its progress and result.
    pub(crate) fn post_book_edit(
        &self,
        world: &ClientWorld,
        command: impl FnOnce(Stamp) -> ClientCommand,
    ) -> Result<BookEditReceipt, Refusal> {
        let stamp = self.stamp(world)?;
        self.send_book_edit(world, command(stamp))
    }

    /// Sends a prepared manual book edit and gives its caller its progress and result.
    pub(crate) fn send_book_edit(
        &self,
        world: &ClientWorld,
        command: ClientCommand,
    ) -> Result<BookEditReceipt, Refusal> {
        self.try_book_edit(world, command, false)
            .inspect_err(|refusal| self.refused(*refusal))
    }

    fn try_book_edit(
        &self,
        world: &ClientWorld,
        command: ClientCommand,
        set_step: bool,
    ) -> Result<BookEditReceipt, Refusal> {
        debug_assert!(Expected::from_command(&command).is_some());
        let receipt = BookEditReceipt::new(command.session_id().ok_or(Refusal::NotAdmitted)?);
        self.try_send(world, command, set_step, Some(&receipt))?;
        Ok(receipt)
    }

    fn try_send(
        &self,
        world: &ClientWorld,
        command: ClientCommand,
        set_step: bool,
        receipt: Option<&BookEditReceipt>,
    ) -> Result<(), Refusal> {
        let edit = Expected::from_command(&command);
        let interrupt = interrupts_apply(&command);
        self.check(world, &command).and_then(|queue| {
            let mut book = (edit.is_some() || interrupt)
                .then(|| self.book.lock().map_err(|_| Refusal::Busy))
                .transpose()?;
            let edit = edit.map(|expected| (expected, command.session_id(), Instant::now()));
            if let (Some((_, session, now)), Some(book)) = (edit, book.as_mut()) {
                if session != world.session_id() {
                    return Err(Refusal::Away);
                }
                book.can_edit(session, set_step, now)?;
            }
            queue.try_send(command).map_err(|error| match error {
                TrySendError::Full(_) => Refusal::Busy,
                TrySendError::Disconnected(_) => Refusal::Ended,
            })?;
            if let Some(book) = book.as_mut() {
                if let Some((expected, session, now)) = edit {
                    book.sent(expected, session, now, receipt.cloned());
                }
                book.interrupted |= book.applying && interrupt;
            }
            Ok(())
        })
    }

    /// Sends a command the player did not ask for, such as a setting the
    /// session must know. A refusal is not shown; the caller tries again.
    pub(crate) fn tell(&self, world: &ClientWorld, command: ClientCommand) -> bool {
        self.try_send(world, command, false, None).is_ok()
    }

    /// Stamps a command made now and sends it.
    ///
    /// # Errors
    /// Refuses a command the session cannot take now.
    pub(crate) fn post(
        &self,
        world: &ClientWorld,
        command: impl FnOnce(Stamp) -> ClientCommand,
    ) -> Result<(), Refusal> {
        self.try_post(world, command)
            .inspect_err(|refusal| self.refused(*refusal))
    }

    /// Stamps and sends a command when the caller owns refusal feedback.
    ///
    /// # Errors
    /// Refuses a command the session cannot take now.
    pub(super) fn try_post(
        &self,
        world: &ClientWorld,
        command: impl FnOnce(Stamp) -> ClientCommand,
    ) -> Result<(), Refusal> {
        let stamp = self.try_stamp(world)?;
        self.try_send(world, command(stamp), false, None)
    }

    /// The queue, if the session can take this command now.
    fn check(
        &self,
        world: &ClientWorld,
        command: &ClientCommand,
    ) -> Result<&SyncSender<ClientCommand>, Refusal> {
        let queue = self.queue.as_ref().ok_or(Refusal::Offline)?;
        // The world server's commands need no zone; every other one needs a
        // player in the world and what the admission offers.
        if let Some(capability) = command.capability() {
            if world.session_id().is_none() {
                return Err(Refusal::NotAdmitted);
            }
            let preference = matches!(
                command,
                ClientCommand::SetMovementOptions { .. }
                    | ClientCommand::SetBookOptions { .. }
                    | ClientCommand::AnswerListPolls { .. }
            );
            if preference && command.session_id() != world.session_id() {
                return Err(Refusal::Away);
            }
            if world.is_dead() && !preference {
                return Err(Refusal::Dead);
            }
            if !(world.in_world() || preference && world.connected()) {
                return Err(Refusal::Away);
            }
            if !world.can(capability) {
                return Err(Refusal::Unavailable(capability));
            }
            if matches!(
                command,
                ClientCommand::CastSpell { .. }
                    | ClientCommand::UseItem(_)
                    | ClientCommand::CancelBuff(_)
            ) && !self.spell_ready.load(Ordering::Relaxed)
            {
                return Err(Refusal::SpellData);
            }
        }
        Ok(queue)
    }

    /// Queues a refusal for the normal chat feedback.
    pub(super) fn refused(&self, refusal: Refusal) {
        debug!(?refusal, "Command refused");
        if let Ok(mut refused) = self.refused.lock() {
            refused.push(refusal);
        }
    }

    /// The refusals since the last frame.
    pub(crate) fn take_refused(&self) -> Vec<Refusal> {
        self.refused
            .lock()
            .map(|mut refused| std::mem::take(&mut *refused))
            .unwrap_or_default()
    }
}

/// Says this frame's last refusal queued for chat.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(outbox: Res<Outbox>, mut chat: ResMut<super::chat::ChatState>) {
    if let Some(refusal) = outbox.take_refused().pop() {
        chat.refuse(refusal.text());
    }
}

/// What a control needs of the session: greyed out under a veil, with the
/// reason on hover, while the session does not offer it. One veil serves
/// every control, whatever it needs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
    /// Something the session lets the player do, such as casting.
    Capability(Capability),
    /// Automatic looting requires item results, not receipt-only echoes.
    AutomaticLoot,
    /// The local air simulation belongs to Titanium.
    TitaniumBreath,
    /// An ability, which the server type must also list.
    Ability(eq_client_core::abilities::Ability),
    /// Nothing of the session, as an empty action bar slot needs: never
    /// veiled.
    Nothing,
    /// What this client does not have yet, as a skin's button for a window
    /// it lacks: never offered, and drawn in the skin's own disabled look
    /// rather than under a veil.
    Missing,
}

impl Needs {
    /// What an Options window checkbox needs of the session, if anything.
    pub(crate) fn of(toggle: eq_client_core::options::Toggle) -> Option<Self> {
        use eq_client_core::{options::Toggle, qol::Fix};
        match toggle {
            Toggle::Qol(Fix::AutoLoot) => Some(Self::AutomaticLoot),
            Toggle::Qol(Fix::GillsForDays) => Some(Self::TitaniumBreath),
            _ => toggle.needs().map(Self::Capability),
        }
    }

    /// Whether the session offers what the control needs.
    pub(crate) fn offered(self, world: &ClientWorld) -> bool {
        match self {
            Self::Capability(capability) => offered(world, capability),
            Self::AutomaticLoot => {
                offered(world, Capability::Looting) && world.loot_continuation().automatic()
            }
            Self::Ability(ability) => {
                offered(world, Capability::Abilities) && world.ability_offered(ability)
            }
            Self::TitaniumBreath => world.generation() == eq_client_core::GameDialect::Titanium,
            Self::Nothing => true,
            Self::Missing => false,
        }
    }

    /// Why the control is greyed out while it is not offered: shown on hover.
    pub(crate) fn reason(self) -> String {
        match self {
            Self::Missing => MISSING.to_owned(),
            Self::AutomaticLoot => {
                "Automatic looting needs supported item results on this session.".to_owned()
            }
            Self::TitaniumBreath => "Local breath simulation uses the Titanium client.".to_owned(),
            Self::Capability(_) | Self::Ability(_) | Self::Nothing => UNAVAILABLE.to_owned(),
        }
    }
}

/// The veil over a control the session does not offer.
#[derive(Component)]
pub(crate) struct Veil;

/// Puts a veil over each new control that needs something, shown only while
/// the session does not offer it. A control the client does not have yet
/// is already drawn disabled, so it gets none.
pub(crate) fn veil(mut commands: Commands, added: Query<(Entity, &Needs), Added<Needs>>) {
    for (control, needs) in &added {
        if *needs == Needs::Missing {
            continue;
        }
        commands.entity(control).with_child((
            Veil,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme::VEIL),
            // Above the control's own icons and labels.
            ZIndex(10),
            bevy::ui::FocusPolicy::Pass,
        ));
    }
}

/// Shows each control's veil while the session does not offer what it needs
/// ([`eq_client_core::qol::Fix::GreyedControls`]). Offline, the preview
/// offers everything.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn grey_out(
    (online, options): (
        Res<super::online::OnlineState>,
        Res<super::options::OptionsState>,
    ),
    controls: Query<(&Needs, &Children)>,
    mut veils: Query<&mut Node, With<Veil>>,
) {
    let greys = greys(&options);
    for (needs, children) in &controls {
        let display = if !greys || needs.offered(online.world()) {
            Display::None
        } else {
            Display::Flex
        };
        for child in children {
            if let Ok(mut node) = veils.get_mut(*child)
                && node.display != display
            {
                node.display = display;
            }
        }
    }
}

/// Whether a control for what the session does not offer is greyed out,
/// with the reason on hover.
pub(crate) const fn greys(options: &super::options::OptionsState) -> bool {
    options
        .options
        .qol
        .on(eq_client_core::qol::Fix::GreyedControls)
}

/// Whether the session lets the player do this; before any admission the
/// question does not arise, so nothing is greyed out.
pub(crate) fn offered(world: &ClientWorld, capability: Capability) -> bool {
    world.session_id().is_none() || world.can(capability)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{OutboundChat, WorldEvent, WorldUpdate, world::NoSpells};

    fn admitted(capabilities: Vec<Capability>) -> ClientWorld {
        admitted_with(capabilities, Vec::new())
    }

    /// An admission that offers some things and leaves others to the player.
    fn admitted_with(capabilities: Vec<Capability>, choices: Vec<Capability>) -> ClientWorld {
        let mut world = ClientWorld::default();
        for update in [
            WorldUpdate::Game(WorldEvent::Entered {
                capabilities,
                choices,
                session_id: 4,
                zone: "qeytoqrg".into(),
                player: Box::new(crate::preview::player(
                    eq_client_core::WorldPosition::default(),
                    [None; 8],
                )),
                far_clip: None,
                spell_landing_flag: None,
            }),
            WorldUpdate::Connection(eq_client_core::world::Link::Connected),
        ] {
            world.apply(&update, Instant::now(), &NoSpells);
        }
        world
    }

    fn say() -> ClientCommand {
        ClientCommand::SendChat(OutboundChat::Say("Hail".into()))
    }

    #[test]
    fn a_carried_combine_offer_never_authorizes_station_commands() {
        use eq_client_core::{inventory::InventorySlot, tradeskills::WORLD_CONTAINER};
        let (queue, received) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(queue));
        let world = admitted(vec![Capability::Tradeskills]);
        let combine = |container| ClientCommand::Combine {
            session_id: 4,
            container,
            created: Instant::now(),
        };
        outbox.send(&world, combine(InventorySlot(23))).unwrap();
        for command in [
            combine(WORLD_CONTAINER),
            ClientCommand::OpenContainer {
                session_id: 4,
                drop_id: 71,
                created: Instant::now(),
            },
            ClientCommand::CloseContainer { session_id: 4 },
        ] {
            assert_eq!(
                outbox.send(&world, command),
                Err(Refusal::Unavailable(Capability::WorldContainers))
            );
        }
        assert!(matches!(
            received.try_recv().unwrap(),
            ClientCommand::Combine {
                container: InventorySlot(23),
                ..
            }
        ));
        assert!(received.try_recv().is_err());
        assert!(world.container().is_none());
        assert!(world.combining().is_none());
    }

    #[test]
    fn unavailable_spell_data_refuses_casting_while_chat_keeps_working() {
        let (queue, received) = std::sync::mpsc::sync_channel(2);
        let outbox = Outbox::new(Some(queue));
        let world = admitted(vec![Capability::Casting, Capability::Talking]);
        let cast = || ClientCommand::CastSpell {
            session_id: 4,
            gem: 0,
            spell_id: 1,
            target_id: 7,
            created: Instant::now(),
        };
        outbox.set_spell_ready(false);
        assert_eq!(outbox.send(&world, cast()), Err(Refusal::SpellData));
        outbox.send(&world, say()).unwrap();
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::SendChat(_))
        ));
        assert!(received.try_recv().is_err());
        // Dropping an existing invisibility state needs no local spell cost,
        // timing or projection, even though it shares the Casting capability.
        outbox
            .send(
                &world,
                ClientCommand::DropInvisibility {
                    session_id: 4,
                    steps: Vec::new(),
                },
            )
            .unwrap();
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::DropInvisibility { .. })
        ));
        outbox.set_spell_ready(true);
        outbox.send(&world, cast()).unwrap();
        assert!(matches!(
            received.try_recv(),
            Ok(ClientCommand::CastSpell { .. })
        ));
    }

    #[test]
    fn unavailable_spell_data_preserves_existing_refusal_priority() {
        let (queue, _received) = std::sync::mpsc::sync_channel(1);
        let outbox = Outbox::new(Some(queue));
        outbox.set_spell_ready(false);
        let cast = || ClientCommand::CastSpell {
            session_id: 4,
            gem: 0,
            spell_id: 1,
            target_id: 7,
            created: Instant::now(),
        };
        assert_eq!(
            outbox.send(&ClientWorld::default(), cast()),
            Err(Refusal::NotAdmitted)
        );
        let mut zoning = admitted(vec![Capability::Casting]);
        zoning.apply(
            &WorldUpdate::Connection(eq_client_core::world::Link::Zoning),
            Instant::now(),
            &NoSpells,
        );
        assert_eq!(outbox.send(&zoning, cast()), Err(Refusal::Away));
        assert_eq!(
            outbox.send(&admitted(Vec::new()), cast()),
            Err(Refusal::Unavailable(Capability::Casting))
        );
    }

    #[test]
    fn commands_leave_only_for_what_the_session_offers() {
        let (queue, received) = std::sync::mpsc::sync_channel(1);
        let outbox = Outbox::new(Some(queue));
        let world = admitted(vec![Capability::Talking]);
        let jump = |stamp: Stamp| ClientCommand::Jump {
            session_id: stamp.session_id,
            created: stamp.created,
        };
        assert_eq!(
            outbox.post(&world, jump),
            Err(Refusal::Unavailable(Capability::Jumping))
        );
        assert_eq!(outbox.send(&world, say()), Ok(()));
        assert_eq!(received.try_recv().ok(), Some(say()));
        // A full queue refuses rather than waits.
        outbox.send(&world, say()).unwrap();
        assert_eq!(outbox.send(&world, say()), Err(Refusal::Busy));
        drop(received);
        assert_eq!(outbox.send(&world, say()), Err(Refusal::Ended));
        assert_eq!(
            outbox.take_refused(),
            [
                Refusal::Unavailable(Capability::Jumping),
                Refusal::Busy,
                Refusal::Ended
            ]
        );
    }

    #[test]
    fn a_stamp_needs_a_session_and_an_admission() {
        let world = admitted(Capability::ALL.to_vec());
        assert_eq!(Outbox::new(None).stamp(&world), Err(Refusal::Offline));
        let (queue, _received) = std::sync::mpsc::sync_channel(1);
        let outbox = Outbox::new(Some(queue));
        assert_eq!(
            outbox.stamp(&ClientWorld::default()),
            Err(Refusal::NotAdmitted)
        );
        // Between zones, nothing that needs the zone leaves.
        let mut zoning = admitted(Capability::ALL.to_vec());
        zoning.apply(
            &WorldUpdate::Connection(eq_client_core::world::Link::Zoning),
            Instant::now(),
            &NoSpells,
        );
        assert_eq!(outbox.send(&zoning, say()), Err(Refusal::Away));
        assert_eq!(outbox.stamp(&world).map(|stamp| stamp.session_id), Ok(4));
        // The world server's commands need no admission.
        let choose = ClientCommand::SelectCharacter {
            selection_id: 1,
            slot: 0,
        };
        assert_eq!(outbox.send(&ClientWorld::default(), choose), Ok(()));
    }

    #[test]
    fn a_control_greys_out_while_the_session_lacks_what_it_needs() {
        let mut app = crate::testing::app();
        app.add_systems(Update, (veil, grey_out).chain());
        let control = app
            .world_mut()
            .spawn((Node::default(), Needs::Capability(Capability::Casting)))
            .id();
        let veil_display = |app: &mut App| {
            let children = app.world().get::<Children>(control).unwrap().to_vec();
            app.world().get::<Node>(children[0]).unwrap().display
        };
        // Before any admission nothing is greyed out.
        app.update();
        app.update();
        assert_eq!(veil_display(&mut app), Display::None);
        crate::online::testing::set_world(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            admitted(vec![Capability::Talking]),
        );
        app.update();
        assert_eq!(veil_display(&mut app), Display::Flex);
        crate::online::testing::set_world(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            admitted(Capability::ALL.to_vec()),
        );
        app.update();
        assert_eq!(veil_display(&mut app), Display::None);
    }

    #[test]
    fn an_ability_the_server_does_not_list_is_veiled_like_any_other_need() {
        use eq_client_core::{WorldEvent, WorldUpdate, abilities::Ability};
        let mut world = admitted(Capability::ALL.to_vec());
        assert!(Needs::Ability(Ability::Fishing).offered(&world));
        world.apply(
            &WorldUpdate::Game(WorldEvent::AbilitiesOffered(vec![Ability::Kick])),
            Instant::now(),
            &eq_client_core::world::NoSpells,
        );
        assert!(Needs::Ability(Ability::Kick).offered(&world));
        assert!(!Needs::Ability(Ability::Fishing).offered(&world));
        // An ability also needs the abilities capability.
        let without = admitted(vec![Capability::Talking]);
        assert!(!Needs::Ability(Ability::Kick).offered(&without));
    }

    #[test]
    fn a_control_the_client_lacks_keeps_the_skins_look_and_its_own_reason() {
        let mut app = crate::testing::app();
        app.add_systems(Update, (veil, grey_out).chain());
        let control = app
            .world_mut()
            .spawn((Node::default(), Needs::Missing))
            .id();
        app.update();
        app.update();
        // The skin draws it disabled, so it wears no veil.
        assert!(app.world().get::<Children>(control).is_none());
        // No session offers it, nor the preview offline.
        assert!(!Needs::Missing.offered(&ClientWorld::default()));
        assert!(!Needs::Missing.offered(&admitted(Capability::ALL.to_vec())));
        assert_eq!(Needs::Missing.reason(), MISSING);
        assert_eq!(Needs::Capability(Capability::Casting).reason(), UNAVAILABLE);
    }

    #[test]
    fn what_the_session_leaves_to_the_player_waits_for_their_setting() {
        use eq_client_core::{options::Toggle, qol::Fix};
        let mut world = admitted_with(vec![Capability::Talking], vec![Capability::Map]);
        // The transport's optional capabilities still need a choice. The
        // local map UI no longer needs or requests that capability.
        let map = Needs::Capability(Capability::Map);
        assert!(!map.offered(&world));
        assert_eq!(Needs::of(Toggle::Qol(Fix::MapEverywhere)), None);
        world.choose(vec![Capability::Map]);
        assert!(map.offered(&world));
        assert!(map.offered(&admitted(vec![Capability::Map])));
        // The saved food preference needs the session's meal capability.
        assert_eq!(
            Needs::of(Toggle::Qol(Fix::SkipModifiedFood)),
            Some(Needs::Capability(Capability::Meals))
        );
        assert_eq!(Needs::of(Toggle::Qol(Fix::HiddenWindows)), None);
    }
}

#[cfg(test)]
mod hud_tests {
    use super::*;

    #[test]
    fn hud_gems_grey_out_without_casting() {
        let mut app = crate::testing::app();
        app.add_systems(Startup, |mut commands: Commands| {
            crate::hud::spawn(&mut commands);
        })
        .add_systems(Update, (veil, grey_out).chain());
        let mut world = ClientWorld::default();
        for update in [
            eq_client_core::WorldUpdate::Game(eq_client_core::WorldEvent::Entered {
                capabilities: vec![Capability::Talking],
                choices: Vec::new(),
                session_id: 4,
                zone: "qeytoqrg".into(),
                player: Box::new(crate::preview::player(
                    eq_client_core::WorldPosition::default(),
                    [None; 8],
                )),
                far_clip: None,
                spell_landing_flag: None,
            }),
            eq_client_core::WorldUpdate::Connection(eq_client_core::world::Link::Connected),
        ] {
            world.apply(&update, Instant::now(), &eq_client_core::world::NoSpells);
        }
        crate::online::testing::set_world(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            world,
        );
        app.update();
        app.update();
        let mut gems = app
            .world_mut()
            .query_filtered::<&Children, With<crate::hud::SpellGem>>();
        let children: Vec<Vec<Entity>> = gems
            .iter(app.world())
            .map(|children| children.to_vec())
            .collect();
        assert_eq!(children.len(), 8);
        for children in children {
            let veils: Vec<_> = children
                .iter()
                .filter_map(|child| {
                    app.world()
                        .get::<Veil>(*child)
                        .map(|_| app.world().get::<Node>(*child).unwrap().display)
                })
                .collect();
            assert_eq!(veils, [Display::Flex]);
        }
    }
}

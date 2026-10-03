//! The one way the client's commands leave for the session. The outbox
//! stamps a command with the admission it belongs to and the time it was
//! made, refuses what the session cannot take (no session, something the
//! server does not offer, a full or closed queue) with one reason, and says
//! every refusal in one place: the chat, as the official client says its
//! refusals. Windows grey out
//! what the session does not offer from the same answer, the world's
//! capability report.
use crate::theme;
use bevy::prelude::*;
use eq_client_core::{Capability, ClientCommand, world::ClientWorld};
use std::{
    sync::{
        Mutex,
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

/// The reason a setting that turns on what a session may leave to the player
/// is greyed out where the session does not, in the client's own words.
pub(crate) const NOT_LEFT: &str = "Only for servers that keep this off";

/// The reason a skin's control this client does not have yet is greyed out,
/// in the client's own words.
pub(crate) const MISSING: &str = "Not in this client yet";

/// The session's command queue, and the refusals not yet shown.
#[derive(Resource)]
pub(crate) struct Outbox {
    queue: Option<SyncSender<ClientCommand>>,
    refused: Mutex<Vec<Refusal>>,
}

impl Outbox {
    pub(crate) fn new(queue: Option<SyncSender<ClientCommand>>) -> Self {
        Self {
            queue,
            refused: Mutex::new(Vec::new()),
        }
    }

    /// The stamp for a command made now in the current admission.
    ///
    /// # Errors
    /// Refuses, and shows why, when there is no session or admission.
    pub(crate) fn stamp(&self, world: &ClientWorld) -> Result<Stamp, Refusal> {
        let stamp = if self.queue.is_none() {
            Err(Refusal::Offline)
        } else {
            world
                .session_id()
                .map(|session_id| Stamp {
                    session_id,
                    created: Instant::now(),
                })
                .ok_or(Refusal::NotAdmitted)
        };
        stamp.inspect_err(|refusal| self.refused(*refusal))
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
        let sent = self.check(world, &command).and_then(|queue| {
            queue.try_send(command).map_err(|error| match error {
                TrySendError::Full(_) => Refusal::Busy,
                TrySendError::Disconnected(_) => Refusal::Ended,
            })
        });
        sent.inspect_err(|refusal| self.refused(*refusal))
    }

    /// Sends a command the player did not ask for, such as a setting the
    /// session must know. A refusal is not shown; the caller tries again.
    pub(crate) fn tell(&self, world: &ClientWorld, command: ClientCommand) -> bool {
        self.check(world, &command)
            .is_ok_and(|queue| queue.try_send(command).is_ok())
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
        let stamp = self.stamp(world)?;
        self.send(world, command(stamp))
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
            if world.death().is_some() {
                return Err(Refusal::Dead);
            }
            if !world.in_world() {
                return Err(Refusal::Away);
            }
            if !world.can(capability) {
                return Err(Refusal::Unavailable(capability));
            }
        }
        Ok(queue)
    }

    fn refused(&self, refusal: Refusal) {
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

/// Says this frame's last refusal in the chat, where every refusal is said.
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
    /// A setting that turns on what the session leaves to the player, which
    /// matters only where the session does.
    Choice(Capability),
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
        let unlocks = match toggle {
            eq_client_core::options::Toggle::Qol(fix) => fix.unlocks(),
            _ => None,
        };
        match unlocks {
            Some(capability) => Some(Self::Choice(capability)),
            None => toggle.needs().map(Self::Capability),
        }
    }

    /// Whether the session offers what the control needs.
    pub(crate) fn offered(self, world: &ClientWorld) -> bool {
        match self {
            Self::Capability(capability) => offered(world, capability),
            Self::Choice(capability) => world.session_id().is_none() || world.leaves(capability),
            Self::Ability(ability) => {
                offered(world, Capability::Abilities) && world.ability_offered(ability)
            }
            Self::Nothing => true,
            Self::Missing => false,
        }
    }

    /// Why the control is greyed out while it is not offered: shown on hover.
    /// What the session leaves to the player names the setting that turns it
    /// on.
    pub(crate) fn reason(self, world: &ClientWorld) -> String {
        let setting = match self {
            Self::Capability(capability) if world.leaves(capability) => {
                eq_client_core::qol::Fix::unlocking(capability)
            }
            _ => None,
        };
        match (self, setting) {
            (_, Some(fix)) => format!(
                "Turn on \"{}\" on the Options window's QoL page",
                fix.label()
            ),
            (Self::Missing, None) => MISSING.to_owned(),
            (Self::Choice(_), None) => NOT_LEFT.to_owned(),
            (Self::Capability(_) | Self::Ability(_) | Self::Nothing, None) => {
                UNAVAILABLE.to_owned()
            }
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

/// Shows each control's veil while the session does not offer what it needs.
/// Offline, the preview offers everything.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn grey_out(
    online: Res<super::online::OnlineState>,
    controls: Query<(&Needs, &Children)>,
    mut veils: Query<&mut Node, With<Veil>>,
) {
    for (needs, children) in &controls {
        let display = if needs.offered(online.world()) {
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
            Err(Refusal::Unavailable(Capability::Falling))
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
                Refusal::Unavailable(Capability::Falling),
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
        let world = ClientWorld::default();
        assert_eq!(Needs::Missing.reason(&world), MISSING);
        assert_eq!(
            Needs::Capability(Capability::Casting).reason(&world),
            UNAVAILABLE
        );
    }

    #[test]
    fn what_the_session_leaves_to_the_player_waits_for_their_setting() {
        use eq_client_core::{
            options::Toggle,
            qol::{Fix, Settings},
        };
        let mut world = admitted_with(vec![Capability::Talking], vec![Capability::Map]);
        // The map is greyed, with the setting that turns it on named.
        let map = Needs::Capability(Capability::Map);
        assert!(!map.offered(&world));
        assert_eq!(
            map.reason(&world),
            "Turn on \"Use the Map Where It's Off\" on the Options window's QoL page"
        );
        // The setting itself is offered where the session leaves the map to
        // the player.
        let setting = Needs::of(Toggle::Qol(Fix::MapWhereOff));
        assert_eq!(setting, Some(Needs::Choice(Capability::Map)));
        assert!(setting.is_some_and(|needs| needs.offered(&world)));
        // Turned on, the map is the player's.
        let mut settings = Settings::default();
        settings.set(Fix::MapWhereOff, true);
        world.choose(settings.unlocked());
        assert!(map.offered(&world));
        // Where the session offers the map, or nothing, the setting is
        // greyed, with its own reason.
        let offering = admitted(vec![Capability::Map]);
        assert!(!Needs::Choice(Capability::Map).offered(&offering));
        assert_eq!(Needs::Choice(Capability::Map).reason(&offering), NOT_LEFT);
        assert!(Needs::Capability(Capability::Map).offered(&offering));
        // Other checkboxes need what they always did.
        assert_eq!(
            Needs::of(Toggle::Qol(Fix::SkipModifiedFood)),
            Some(Needs::Capability(Capability::Inventory))
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

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
    /// An ability, which the server type must also list.
    Ability(eq_client_core::abilities::Ability),
    /// Nothing of the session, as an empty action bar slot needs: never
    /// veiled.
    Nothing,
}

impl Needs {
    /// Whether the session offers what the control needs.
    pub(crate) fn offered(self, world: &ClientWorld) -> bool {
        match self {
            Self::Capability(capability) => offered(world, capability),
            Self::Ability(ability) => {
                offered(world, Capability::Abilities) && world.ability_offered(ability)
            }
            Self::Nothing => true,
        }
    }
}

/// The veil over a control the session does not offer.
#[derive(Component)]
pub(crate) struct Veil;

/// Puts a veil over each new control that needs something, shown only while
/// the session does not offer it.
pub(crate) fn veil(mut commands: Commands, added: Query<Entity, Added<Needs>>) {
    for control in &added {
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
        let mut world = ClientWorld::default();
        for update in [
            WorldUpdate::Game(WorldEvent::Entered {
                capabilities,
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

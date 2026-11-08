//! Scene right-clicks reuse the trade/loot paths and otherwise consider the
//! clicked entity. Neither a click nor consideration changes the target.
use super::{BANKER_CLASS, MERCHANT_CLASS, TradeState, merchant_reach};
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use eq_client_assets::ui::InstalledClient;
use eq_client_core::{Capability, ClientCommand, SpawnKind, world::ClientWorld};

#[derive(Clone, Copy)]
pub(super) enum Use {
    Loot(u16),
    Trade(u16),
}

impl Use {
    pub(super) const fn id(self) -> u16 {
        match self {
            Self::Loot(id) | Self::Trade(id) => id,
        }
    }
}

fn eligible(world: &ClientWorld, client: InstalledClient, id: u16) -> Result<Use, &'static str> {
    let spawn = world
        .spawn(id)
        .map(|spawn| &spawn.state)
        .filter(|spawn| !spawn.invisible)
        .ok_or("Interaction target is no longer available")?;
    let player = world.player().ok_or("Enter the world first")?;
    if matches!(spawn.kind, SpawnKind::NpcCorpse | SpawnKind::PlayerCorpse) {
        // The session's loot guard owns corpse reach.
        return world
            .can(Capability::Looting)
            .then_some(Use::Loot(id))
            .ok_or(crate::outbox::UNAVAILABLE);
    }
    let capability = match (spawn.kind, spawn.class) {
        (SpawnKind::Npc, Some(MERCHANT_CLASS)) => Capability::Trading,
        (SpawnKind::Npc, Some(BANKER_CLASS)) => Capability::Inventory,
        (SpawnKind::Npc, class) if crate::training::is_guildmaster(class) => Capability::Training,
        _ => return Err("Target a merchant, banker, trainer or corpse"),
    };
    if !world.can(capability) {
        return Err(crate::outbox::UNAVAILABLE);
    }
    if merchant_reach(client).is_some_and(|reach| !reach.admits(player.position, spawn.position)) {
        return Err("Interaction target is out of reach");
    }
    Ok(Use::Trade(id))
}

impl TradeState {
    /// Opens an ordinary service without retargeting, toggling another
    /// service closed, or falling back to consideration.
    pub(crate) fn script_interact(
        &mut self,
        online: &mut OnlineState,
        outbox: &Outbox,
        shown: &mut Shown,
        client: InstalledClient,
        (admission, id, revision): (u64, u16, u64),
    ) -> Result<(), &'static str> {
        let world = online.world();
        if !world.in_world() || world.session_id() != Some(admission) {
            return Err("Interaction needs the current admission");
        }
        let spawn = world
            .spawn(id)
            .filter(|spawn| {
                world.target().selected == Some(id)
                    && !world.target_stale()
                    && spawn.revision == revision
            })
            .ok_or("Interaction target changed or is no longer available")?;
        if self.loot.is_some()
            || world.loot().is_some()
            || self.merchant.is_some()
            || world.merchant().is_some()
            || self.merchant_pending.is_some()
            || self.opening_shop(world).is_some()
            || world.exchange().is_some()
            || world.container().is_some()
            || world.pending_container_open().is_some()
            || world.combining().is_some()
            || world.training().is_some()
            || world.inventory_moves_pending()
            || shown.is_open(WindowId::Bank)
        {
            return Err("Close or settle the current transaction before opening another");
        }
        let service = eligible(world, client, id)?;
        let stamp = outbox.stamp(world).map_err(crate::outbox::Refusal::text)?;
        match (service, spawn.state.class) {
            (Use::Loot(id), _) => {
                let name = eq_client_core::entities::display_name(&spawn.state.name);
                self.open_loot(online, outbox, stamp, id, name)
            }
            (Use::Trade(id), Some(MERCHANT_CLASS)) => self.open_shop(
                world,
                outbox,
                stamp,
                id,
                world.player().ok_or("Enter the world first")?.spawn_id,
            ),
            (Use::Trade(_), Some(BANKER_CLASS)) => {
                shown.open(WindowId::Bank);
                Ok(())
            }
            (Use::Trade(trainer), class) if crate::training::is_guildmaster(class) => outbox
                .send(
                    world,
                    ClientCommand::Training {
                        session_id: stamp.session_id,
                        request: eq_client_core::training::TrainingRequest::Open { trainer },
                        created: stamp.created,
                    },
                )
                .map_err(crate::outbox::Refusal::text),
            (Use::Trade(_), _) => Err("Target a merchant, banker, guildmaster or corpse"),
        }
    }

    /// Queues an explicit scene click in the admission where it was made.
    pub(crate) fn right_click(&mut self, world: &ClientWorld, id: u16) {
        self.right_clicked = world.session_id().map(|session| (session, id));
    }

    /// Consumes before any early return, so a dead/old/removed click cannot
    /// act after recovery or against another admission's reused spawn ID.
    pub(super) fn take_right_click(
        &mut self,
        world: &ClientWorld,
        client: InstalledClient,
        outbox: &Outbox,
    ) -> Option<Use> {
        let (session, id) = self.right_clicked.take()?;
        if world.session_id() != Some(session) || !world.in_world() {
            return None;
        }
        if let Ok(service) = eligible(world, client, id) {
            return Some(service);
        }
        let spawn = &world.spawn(id)?.state;
        if spawn.invisible {
            return None;
        }
        let player = world.player()?;
        // Consider has no interaction-radius requirement. The normal
        // outbox/session path owns capability, admission and spawn checks.
        if matches!(spawn.kind, SpawnKind::Npc | SpawnKind::Player) {
            let _ = outbox.post(world, |stamp| ClientCommand::Consider {
                session_id: stamp.session_id,
                own_id: player.spawn_id,
                target_id: id,
                created: stamp.created,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        online::{OnlineState, testing},
        windows::{Shown, WindowId},
    };
    use bevy::prelude::*;
    use eq_client_core::{SpawnState, WorldEvent};
    use std::sync::mpsc::{self, Receiver};

    fn actor(class: Option<u8>, distance: f32) -> SpawnState {
        let mut actor = testing::pet(8, 7);
        actor.pet_owner = None;
        actor.class = class;
        actor.position.x = distance;
        actor
    }

    fn app(actor: SpawnState, without: Option<Capability>) -> (App, Receiver<ClientCommand>) {
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        if let Some(without) = without {
            testing::news(
                &mut online,
                [WorldEvent::Entered {
                    capabilities: Capability::ALL
                        .iter()
                        .copied()
                        .filter(|cap| *cap != without)
                        .collect(),
                    choices: Vec::new(),
                    session_id: 1,
                    zone: "test".into(),
                    player: Box::new(testing::player(7)),
                    far_clip: None,
                    spell_landing_flag: None,
                }],
            );
        }
        testing::connect(&mut online, true);
        testing::spawn_entry(&mut online, 8, actor);
        testing::spawn_entry(&mut online, 9, testing::pet(9, 7));
        online.select_target(Some(9));
        let (tx, rx) = mpsc::sync_channel(8);
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, super::super::input);
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = InstalledClient::EqMac;
        (app, rx)
    }

    fn queue(app: &mut App) {
        app.world_mut()
            .resource_scope(|world, mut trade: Mut<TradeState>| {
                trade.right_click(world.resource::<OnlineState>().world(), 8);
            });
    }

    fn unchanged_target(app: &App) {
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            Some(9)
        );
    }

    #[test]
    fn right_click_noninteractive_and_unsupported_npcs_considers_without_retargeting() {
        for (class, distance, without) in [
            (Some(1), 500.0, None),
            (None, 10.0, None),
            (Some(MERCHANT_CLASS), 10.0, Some(Capability::Trading)),
        ] {
            let (mut app, rx) = app(actor(class, distance), without);
            queue(&mut app);
            app.update();
            assert!(matches!(
                rx.try_recv(),
                Ok(ClientCommand::Consider {
                    session_id: 1,
                    own_id: 7,
                    target_id: 8,
                    ..
                })
            ));
            unchanged_target(&app);
            assert!(!app.world().resource::<Shown>().is_open(WindowId::Bank));
            app.update();
            assert!(rx.try_recv().is_err(), "a click is consumed only once");
        }
    }

    #[test]
    fn right_click_uses_in_reach_interactions_and_considers_beyond_the_existing_boundary() {
        for class in [MERCHANT_CLASS, BANKER_CLASS, 20] {
            let (mut near, rx) = app(actor(Some(class), 20.0), None);
            queue(&mut near);
            near.update();
            match class {
                MERCHANT_CLASS => assert!(matches!(
                    rx.try_recv(),
                    Ok(ClientCommand::Shop {
                        session_id: 1,
                        merchant_id: 8,
                        open: true,
                        ..
                    })
                )),
                BANKER_CLASS => {
                    assert!(near.world().resource::<Shown>().is_open(WindowId::Bank));
                    assert!(rx.try_recv().is_err());
                }
                _ => assert!(matches!(
                    rx.try_recv(),
                    Ok(ClientCommand::Training {
                        session_id: 1,
                        request: eq_client_core::training::TrainingRequest::Open { trainer: 8 },
                        ..
                    })
                )),
            }
            unchanged_target(&near);
            let (mut far, rx) = app(actor(Some(class), 20.01), None);
            queue(&mut far);
            far.update();
            assert!(matches!(
                rx.try_recv(),
                Ok(ClientCommand::Consider { target_id: 8, .. })
            ));
            assert!(!far.world().resource::<Shown>().is_open(WindowId::Bank));
            unchanged_target(&far);
        }
    }

    #[test]
    fn script_interact_requests_training_for_the_selected_guildmaster_revision() {
        let (mut app, rx) = app(actor(Some(20), 10.0), None);
        let world = app.world_mut();
        world.resource_scope(|world, mut online: Mut<OnlineState>| {
            online.select_target(Some(8));
            let revision = online.world().spawn(8).unwrap().revision;
            let outbox = world.resource::<Outbox>();
            let mut shown = Shown::default();
            let mut trade = TradeState::default();
            assert!(
                trade
                    .script_interact(
                        &mut online,
                        outbox,
                        &mut shown,
                        InstalledClient::Titanium,
                        (1, 8, revision + 1),
                    )
                    .is_err()
            );
            assert!(rx.try_recv().is_err());
            assert!(
                trade
                    .script_interact(
                        &mut online,
                        outbox,
                        &mut shown,
                        InstalledClient::Titanium,
                        (1, 8, revision),
                    )
                    .is_ok()
            );
        });
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Training {
                session_id: 1,
                request: eq_client_core::training::TrainingRequest::Open { trainer: 8 },
                ..
            })
        ));
    }

    #[test]
    fn right_click_does_not_guess_a_titanium_interaction_radius() {
        let (mut app, rx) = app(actor(Some(MERCHANT_CLASS), 500.0), None);
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = InstalledClient::Titanium;
        queue(&mut app);
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Shop {
                merchant_id: 8,
                open: true,
                ..
            })
        ));
        unchanged_target(&app);
    }

    #[test]
    fn right_click_corpse_reuses_the_loot_request() {
        let mut corpse = actor(Some(1), 3.0);
        corpse.kind = SpawnKind::NpcCorpse;
        let (mut app, rx) = app(corpse, None);
        queue(&mut app);
        app.update();
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientCommand::Loot {
                session_id: 1,
                corpse_id: 8,
                ..
            })
        ));
        assert!(app.world().resource::<TradeState>().loot.is_some());
        unchanged_target(&app);
    }

    #[test]
    fn queued_right_click_does_not_cross_admissions_or_survive_entity_loss() {
        for lost in [0, 1, 2, 3] {
            let (mut app, rx) = app(actor(Some(1), 10.0), None);
            queue(&mut app);
            {
                let mut online = app.world_mut().resource_mut::<OnlineState>();
                match lost {
                    0 => {
                        testing::admit(&mut online, 2, testing::player(7));
                        testing::spawn_entry(&mut online, 8, actor(Some(1), 10.0));
                    }
                    1 => testing::news(&mut online, [WorldEvent::Despawn(8)]),
                    2 => {
                        let mut hidden = actor(Some(1), 10.0);
                        hidden.invisible = true;
                        testing::spawn_entry(&mut online, 8, hidden);
                    }
                    _ => testing::connect(&mut online, false),
                }
            }
            app.update();
            assert!(rx.try_recv().is_err());
            assert!(app.world().resource::<TradeState>().right_clicked.is_none());
            app.update();
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn right_click_consider_keeps_the_normal_combat_capability_guard() {
        let (mut app, rx) = app(actor(Some(1), 10.0), Some(Capability::Combat));
        queue(&mut app);
        app.update();
        assert!(rx.try_recv().is_err());
        unchanged_target(&app);
    }
}

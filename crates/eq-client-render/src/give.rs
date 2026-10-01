//! Handing items to an NPC, as the official client does: clicking an NPC
//! with an item on the cursor asks it to trade, its answer opens the skin's
//! give window, and the item moves into the window's first slot. More items
//! go in from the cursor; Give hands them over, and closing the window (its
//! Cancel button or Escape) takes them back. The session decides what may be
//! asked and moved; this module keeps the window with the world's exchange.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{ClientCommand, SpawnKind, inventory::InventorySlot, world::ClientWorld};

/// The give window's Give button.
#[derive(Component)]
pub(crate) struct GiveButton;

/// Asks a clicked NPC to take what the player holds on the cursor, an item
/// or coins; nothing happens with an empty cursor, as in the official
/// client. A refusal shows in the feedback line.
pub(crate) fn offer(spawn_id: u16, online: &mut OnlineState, outbox: &Outbox) {
    let world = online.world();
    let holding = world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
        || crate::coins::on_cursor(world).is_some();
    let npc = world
        .spawn(spawn_id)
        .is_some_and(|spawn| spawn.state.kind == SpawnKind::Npc);
    if !holding || !npc || world.exchange().is_some() {
        return;
    }
    let sent = outbox
        .post(world, |stamp| ClientCommand::OfferTrade {
            session_id: stamp.session_id,
            with_id: spawn_id,
            created: stamp.created,
        })
        .is_ok();
    if sent {
        online.offer_trade(spawn_id);
    }
}

/// The name of the character the give window hands items to.
pub(crate) fn partner(world: &ClientWorld) -> String {
    world
        .exchange()
        .and_then(|exchange| world.spawn(exchange.with))
        .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
        .unwrap_or_default()
}

/// Closes the exchange the player walked away from; what it held comes back.
fn cancel(online: &mut OnlineState, outbox: &Outbox) {
    // A refusal shows in the feedback line; the window closes either way.
    let _ = outbox.post(online.world(), |stamp| ClientCommand::CancelTrade {
        session_id: stamp.session_id,
    });
    online.close_trade();
}

/// Keeps the give window with the exchange: it opens when the NPC answers,
/// taking what is on the cursor (an item into its first slot, or coins), and
/// closes when the server ends the exchange. A window the player closes
/// cancels it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut shown: ResMut<Shown>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
    mut opened: Local<Option<u16>>,
) {
    let open = online
        .world()
        .exchange()
        .filter(|exchange| exchange.open)
        .map(|exchange| exchange.with);
    match (open, *opened) {
        (Some(with), mine) if mine != Some(with) => {
            shown.open(WindowId::Give);
            *opened = Some(with);
            if let Some((coin, amount)) = crate::coins::on_cursor(online.world()) {
                let transfer = eq_client_core::money::CoinTransfer {
                    from: eq_client_core::money::CoinPlace::Cursor,
                    to: eq_client_core::money::CoinPlace::Trade,
                    coin,
                    into: coin,
                    amount,
                };
                crate::coins::send(transfer, &online, &outbox);
            } else {
                inventory.hand_over_cursor(&online, &outbox);
            }
        }
        (Some(_), Some(_)) if !shown.is_open(WindowId::Give) => {
            cancel(&mut online, &outbox);
            *opened = None;
        }
        (None, Some(_)) => {
            shown.close(WindowId::Give);
            *opened = None;
        }
        _ => (),
    }
}

/// Hands what the give window holds over when Give is pressed.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<GiveButton>)>,
) {
    if !buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }
    let open = online
        .world()
        .exchange()
        .is_some_and(|exchange| exchange.open && !exchange.given);
    if open
        && outbox
            .post(online.world(), |stamp| ClientCommand::AcceptTrade {
                session_id: stamp.session_id,
                created: stamp.created,
            })
            .is_ok()
    {
        online.give();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{
        WorldEvent,
        exchange::ExchangeUpdate,
        inventory::{InventoryUpdate, MoveQuantity},
    };
    use std::sync::mpsc;

    const NPC: u16 = 42;

    /// An admitted player beside an NPC, holding a sword on the cursor.
    fn app() -> (App, mpsc::Receiver<ClientCommand>) {
        let mut app = crate::testing::app();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::spawns(
            &mut online,
            vec![eq_client_core::SpawnState {
                class: Some(1),
                spawn_id: NPC,
                name: "Guard_Example000".into(),
                kind: SpawnKind::Npc,
                race: 1,
                gender: 0,
                position: eq_client_core::WorldPosition::default(),
                velocity: [0.0; 3],
                size: 6.0,
                invisible: false,
                appearance: eq_client_core::outfit::Appearance::default(),
                level: 0,
                listing: eq_client_core::listing::Listing::default(),
            }],
        );
        let mut sword = crate::preview::items()
            .into_iter()
            .find(|item| item.slot == InventorySlot(13))
            .unwrap();
        sword.slot = InventorySlot::CURSOR;
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![sword]));
        let (tx, rx) = mpsc::sync_channel(8);
        app.insert_resource(online)
            .insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, window);
        (app, rx)
    }

    fn online(app: &mut App) -> Mut<'_, OnlineState> {
        app.world_mut().resource_mut::<OnlineState>()
    }

    #[test]
    fn the_npcs_answer_opens_the_window_with_the_cursor_item_in_it() {
        let (mut app, rx) = app();
        // Without a session, nothing is asked.
        offer(NPC, &mut online(&mut app), &Outbox::new(None));
        assert!(online(&mut app).world().exchange().is_none());
        app.world_mut()
            .resource_scope(|world, mut online: Mut<OnlineState>| {
                offer(NPC, &mut online, world.resource::<Outbox>());
            });
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::OfferTrade { with_id: NPC, .. }
        ));
        assert_eq!(online(&mut app).world().exchange().unwrap().with, NPC);
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Give));
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Opened {
                with: u32::from(NPC),
            })],
        );
        app.update();
        assert!(app.world().resource::<Shown>().is_open(WindowId::Give));
        let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
            panic!("the cursor item moves into the window");
        };
        assert_eq!(
            (request.from, request.to, request.quantity),
            (
                InventorySlot::CURSOR,
                InventorySlot(3000),
                MoveQuantity::Whole
            )
        );
        // The server ends it: the window closes and nothing is cancelled.
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Finished)],
        );
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Give));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn closing_the_window_cancels_the_exchange() {
        let (mut app, rx) = app();
        online(&mut app).offer_trade(NPC);
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Opened {
                with: u32::from(NPC),
            })],
        );
        app.update();
        while rx.try_recv().is_ok() {}
        app.world_mut()
            .resource_mut::<Shown>()
            .close(WindowId::Give);
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::CancelTrade { session_id: 1 }
        ));
        assert!(online(&mut app).world().exchange().is_none());
    }

    #[test]
    fn the_window_names_the_npc() {
        let (mut app, _rx) = app();
        online(&mut app).offer_trade(NPC);
        assert_eq!(partner(online(&mut app).world()), "Guard Example");
    }
}

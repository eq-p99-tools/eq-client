//! Handing items to another character, as the official client does:
//! clicking an NPC or another player with an item or coins on the cursor
//! asks them to trade, their answer opens the skin's give window (an NPC) or
//! trade window (a player), and what the cursor held moves into the
//! window's first slot. Another player's request opens the trade window by
//! itself. More items go in from the cursor; Give hands them to an NPC, and
//! a trade goes through once both players click Trade (anything put in
//! undoes both clicks, and a side that clicked shows its name lit). Closing
//! the window (its Cancel button or Escape) takes everything back. The
//! session decides what may be asked and moved; this module keeps the
//! window with the world's exchange.
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, SpawnKind,
    exchange::Partner,
    inventory::InventorySlot,
    world::{Asker, ClientWorld, Exchange},
};

/// The give window's Give button.
#[derive(Component)]
pub(crate) struct GiveButton;

/// Asks a clicked NPC or another player to take what the player holds on
/// the cursor, an item or coins; nothing happens with an empty cursor, as in
/// the official client. The outbox says a refusal itself.
pub(crate) fn offer(spawn_id: u16, online: &mut OnlineState, outbox: &Outbox) {
    let world = online.world();
    let holding = world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
        || crate::coins::on_cursor(world).is_some();
    let other = world
        .player()
        .is_some_and(|player| player.spawn_id != spawn_id);
    let takes = world
        .spawn(spawn_id)
        .is_some_and(|spawn| matches!(spawn.state.kind, SpawnKind::Npc | SpawnKind::Player));
    if !holding || !other || !takes || world.exchange().is_some() {
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

/// A side of a trade.
#[derive(Clone, Copy)]
pub(crate) enum Side {
    Mine,
    Theirs,
}

/// The colour a side's name shows in: lit once that side clicked Trade.
pub(crate) fn ink(world: &ClientWorld, side: Side) -> Color {
    let clicked = world.exchange().is_some_and(|exchange| match side {
        Side::Mine => exchange.given,
        Side::Theirs => exchange.partner_accepted,
    });
    if clicked {
        crate::theme::AGREED
    } else {
        crate::theme::INK_BRIGHT
    }
}

/// The window an exchange shows in: the give window for an NPC, the trade
/// window for another player.
const fn window_of(exchange: &Exchange) -> WindowId {
    match exchange.partner {
        Partner::Npc => WindowId::Give,
        Partner::Player => WindowId::Trade,
    }
}

/// The name of the character the give or trade window is with.
pub(crate) fn partner(world: &ClientWorld) -> String {
    world
        .exchange()
        .and_then(|exchange| world.spawn(exchange.with))
        .map(|spawn| eq_client_core::entities::display_name(&spawn.state.name))
        .unwrap_or_default()
}

/// Closes the exchange the player walked away from; what it held comes back.
fn cancel(online: &mut OnlineState, outbox: &Outbox) {
    // The outbox says a refusal itself; the window closes either way.
    let _ = outbox.post(online.world(), |stamp| ClientCommand::CancelTrade {
        session_id: stamp.session_id,
    });
    online.close_trade();
}

/// Keeps the give or trade window with the exchange: it opens when the other
/// side answers, taking what is on the cursor (an item into its first slot,
/// or coins) when the player asked, and closes when the server ends the
/// exchange. A window the player closes cancels it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut shown: ResMut<Shown>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
    mut opened: Local<Option<(u16, WindowId)>>,
) {
    let open = online
        .world()
        .exchange()
        .filter(|exchange| exchange.open)
        .map(|exchange| ((exchange.with, window_of(exchange)), exchange.asker));
    match (open, *opened) {
        (Some((window, asker)), mine) if mine != Some(window) => {
            if let Some((_, before)) = mine
                && before != window.1
            {
                shown.close(before);
            }
            shown.open(window.1);
            *opened = Some(window);
            if asker == Asker::Partner {
                return;
            }
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
        (Some(_), Some((_, id))) if !shown.is_open(id) => {
            cancel(&mut online, &outbox);
            *opened = None;
        }
        (None, Some((_, id))) => {
            shown.close(id);
            *opened = None;
        }
        _ => (),
    }
}

/// Shows what an item the other player put in is, on a right click, as the
/// player's own items show.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn inspect_theirs(
    mouse: Res<ButtonInput<MouseButton>>,
    online: Res<OnlineState>,
    slots: Query<(&Interaction, &crate::skinned::TheirSlot)>,
    mut items: ResMut<crate::items::ItemState>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(exchange) = online.world().exchange() else {
        return;
    };
    for (interaction, slot) in &slots {
        if *interaction != Interaction::None
            && let Some(item) = exchange.theirs.get(&slot.0)
        {
            items.open_held(item);
        }
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
                name_parts: eq_client_core::names::NameParts::default(),
                pet_owner: None,
                hp_percent: None,
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
    fn another_players_request_opens_the_trade_window_with_nothing_moved() {
        let (mut app, rx) = app();
        testing::spawns(
            &mut online(&mut app),
            vec![eq_client_core::SpawnState {
                class: Some(1),
                spawn_id: 50,
                name: "Trader".into(),
                kind: SpawnKind::Player,
                race: 1,
                gender: 0,
                position: eq_client_core::WorldPosition::default(),
                velocity: [0.0; 3],
                size: 0.0,
                invisible: false,
                appearance: eq_client_core::outfit::Appearance::default(),
                level: 0,
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
                pet_owner: None,
                hp_percent: None,
            }],
        );
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Taken { from: 50 })],
        );
        app.update();
        let shown = app.world().resource::<Shown>();
        assert!(shown.is_open(WindowId::Trade) && !shown.is_open(WindowId::Give));
        assert!(
            rx.try_recv().is_err(),
            "the sword stays on the cursor until the player puts it in"
        );
        assert_eq!(partner(online(&mut app).world()), "Trader");
        // Their click lights their name; anything put in puts it out.
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Accepted { by: 50 })],
        );
        assert_eq!(
            ink(online(&mut app).world(), Side::Theirs),
            crate::theme::AGREED
        );
        assert_eq!(
            ink(online(&mut app).world(), Side::Mine),
            crate::theme::INK_BRIGHT
        );
        // The other player closing it closes it here, without a cancel.
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Cancelled { by: 7 })],
        );
        app.update();
        assert!(!app.world().resource::<Shown>().is_open(WindowId::Trade));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn the_window_names_the_npc() {
        let (mut app, _rx) = app();
        online(&mut app).offer_trade(NPC);
        assert_eq!(partner(online(&mut app).world()), "Guard Example");
    }
}

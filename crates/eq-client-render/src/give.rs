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
use crate::inventory::Handed;
use crate::{
    online::OnlineState,
    outbox::Outbox,
    windows::{Shown, WindowId},
};
use bevy::prelude::*;
use eq_client_assets::ui::GiveAtOpen;
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
/// side answers, taking what is on the cursor when the player asked (coins
/// to the trade's coins, an item where the installed generation puts it:
/// [`GiveAtOpen`], a merge's remainder once the merge settles), and closes
/// when the server ends the exchange. A window the player closes cancels
/// it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn window(
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut shown: ResMut<Shown>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
    (mut opened, mut merging): (Local<Option<(u16, WindowId)>>, Local<bool>),
    (settings, messages, mut chat): (
        Res<crate::ViewerSettings>,
        Res<crate::hud::messages::Messages>,
        ResMut<crate::chat::ChatState>,
    ),
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
                let rule = settings.0.installed_client.give_at_open();
                let handed = inventory.hand_over_cursor(&online, &outbox, rule);
                *merging = handed == Handed::Stacked;
                if handed == Handed::NoRoom && rule == GiveAtOpen::StackFirst {
                    no_room(&messages, &mut chat);
                }
            }
        }
        (Some(_), Some((_, id))) if !shown.is_open(id) => {
            cancel(&mut online, &outbox);
            *opened = None;
            *merging = false;
        }
        (None, Some((_, id))) => {
            shown.close(id);
            *opened = None;
            *merging = false;
        }
        // Once a merge settles, what it left on the cursor goes on: onto
        // the next stack with room, then into the first empty slot (N4 2).
        (Some(_), Some(_)) if *merging && inventory.settled() => {
            let handed = inventory.hand_over_cursor(&online, &outbox, GiveAtOpen::StackFirst);
            *merging = handed == Handed::Stacked;
            if handed == Handed::NoRoom {
                no_room(&messages, &mut chat);
            }
        }
        _ => (),
    }
}

/// The program's line when no trade slot can take the cursor's item, in its
/// colour (N4 section 2).
fn no_room(messages: &crate::hud::messages::Messages, chat: &mut crate::chat::ChatState) {
    chat.history.push(crate::chat::system_line(
        messages.said(12319, &[]).typed(10),
    ));
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
            items.open_received(item.details.clone());
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
                walk_speed: 0.0,
                run_speed: 0.0,
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
                walk_speed: 0.0,
                run_speed: 0.0,
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

    /// Opens the NPC's give window on `client` with `items` in the
    /// inventory and `gold` on the cursor; what the opening sends.
    fn open_with(
        client: eq_client_assets::ui::InstalledClient,
        items: Vec<eq_client_core::inventory::InventoryItem>,
        gold: u32,
    ) -> (App, Option<ClientCommand>, mpsc::Receiver<ClientCommand>) {
        let (mut app, rx) = app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .installed_client = client;
        testing::inventory(&mut online(&mut app), InventoryUpdate::Snapshot(items));
        let cursor = eq_client_core::Coins {
            gold,
            ..eq_client_core::Coins::default()
        };
        testing::news(
            &mut online(&mut app),
            [WorldEvent::CoinsElsewhere {
                cursor,
                bank: eq_client_core::Coins::default(),
                given: eq_client_core::Coins::default(),
                offered: eq_client_core::Coins::default(),
            }],
        );
        online(&mut app).offer_trade(NPC);
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Exchange(ExchangeUpdate::Opened {
                with: u32::from(NPC),
            })],
        );
        app.update();
        let sent = rx.try_recv().ok();
        (app, sent, rx)
    }

    #[test]
    fn takp_puts_the_cursor_item_on_a_matching_stack_first_and_says_when_full() {
        use eq_client_assets::ui::InstalledClient;
        let stack = |slot, count| {
            let mut item = crate::preview::items()
                .into_iter()
                .find(|item| item.slot == InventorySlot(13))
                .unwrap();
            item.slot = InventorySlot(slot);
            item.stack_count = Some(count);
            item.rules.stack_size = 20;
            item
        };
        let moved = |sent: Option<ClientCommand>| match sent {
            Some(ClientCommand::MoveInventory(request)) => Some((request.from, request.to)),
            _ => None,
        };
        // Onto the same item's stack in the second trade slot, past the
        // free first one; Titanium keeps today's first free slot.
        let items = vec![stack(InventorySlot::CURSOR.0, 2), stack(3001, 5)];
        let (_, sent, _) = open_with(InstalledClient::EqMac, items.clone(), 0);
        assert_eq!(
            moved(sent),
            Some((InventorySlot::CURSOR, InventorySlot(3001)))
        );
        let (_, sent, _) = open_with(InstalledClient::Titanium, items, 0);
        assert_eq!(
            moved(sent),
            Some((InventorySlot::CURSOR, InventorySlot(3000)))
        );
        // A full stack is passed over for the first free slot.
        let items = vec![stack(InventorySlot::CURSOR.0, 2), stack(3000, 20)];
        let (_, sent, _) = open_with(InstalledClient::EqMac, items, 0);
        assert_eq!(
            moved(sent),
            Some((InventorySlot::CURSOR, InventorySlot(3001)))
        );
        // An NPC's four slots all taken: nothing moves, and the line says so
        // in colour 10.
        let mut items: Vec<_> = (3000..3004).map(|slot| stack(slot, 20)).collect();
        items.push(stack(InventorySlot::CURSOR.0, 2));
        let (app, sent, _) = open_with(InstalledClient::EqMac, items, 0);
        assert!(sent.is_none());
        let line = app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .last()
            .map(|(_, line)| (*line).clone())
            .unwrap();
        assert_eq!(line.message_type, Some(10));
        assert!(line.message.text.contains("12319"), "{}", line.message.text);
    }

    #[test]
    fn what_a_takp_merge_leaves_on_the_cursor_goes_to_the_first_free_slot() {
        let stack = |slot, count| {
            let mut item = crate::preview::items()
                .into_iter()
                .find(|item| item.slot == InventorySlot(13))
                .unwrap();
            item.slot = InventorySlot(slot);
            item.stack_count = Some(count);
            item.rules.stack_size = 20;
            item
        };
        // 15 on the cursor onto a stack of 10 out of 20: the stack fills.
        let (mut app, sent, rx) = open_with(
            eq_client_assets::ui::InstalledClient::EqMac,
            vec![stack(InventorySlot::CURSOR.0, 15), stack(3000, 10)],
            0,
        );
        let Some(ClientCommand::MoveInventory(merge)) = sent else {
            panic!("the merge goes first: {sent:?}");
        };
        assert_eq!(
            (merge.from, merge.to, merge.quantity),
            (
                InventorySlot::CURSOR,
                InventorySlot(3000),
                MoveQuantity::Count(std::num::NonZeroU32::new(10).unwrap())
            )
        );
        // Nothing more goes until the merge settles.
        app.update();
        assert!(rx.try_recv().is_err());
        testing::inventory(
            &mut online(&mut app),
            InventoryUpdate::Snapshot(vec![stack(InventorySlot::CURSOR.0, 5), stack(3000, 20)]),
        );
        app.world_mut().resource_scope(
            |world, mut state: Mut<crate::inventory::InventoryState>| {
                let online = world.resource::<OnlineState>();
                state.action_result(1, merge.revision, None, online.world().inventory());
            },
        );
        app.update();
        let Ok(ClientCommand::MoveInventory(rest)) = rx.try_recv() else {
            panic!("the rest goes on");
        };
        assert_eq!(
            (rest.from, rest.to, rest.quantity),
            (
                InventorySlot::CURSOR,
                InventorySlot(3001),
                MoveQuantity::Whole
            )
        );
        // And once that settles, nothing else is sent.
        app.world_mut().resource_scope(
            |world, mut state: Mut<crate::inventory::InventoryState>| {
                let online = world.resource::<OnlineState>();
                state.action_result(1, rest.revision, None, online.world().inventory());
            },
        );
        app.update();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn coins_on_the_cursor_go_to_the_trade_as_the_window_opens() {
        let (_, sent, _) = open_with(eq_client_assets::ui::InstalledClient::EqMac, Vec::new(), 3);
        let Some(ClientCommand::MoveCoins { transfer, .. }) = sent else {
            panic!("the cursor's coins move in: {sent:?}");
        };
        assert_eq!(
            (transfer.from, transfer.to, transfer.coin, transfer.amount),
            (
                eq_client_core::money::CoinPlace::Cursor,
                eq_client_core::money::CoinPlace::Trade,
                eq_client_core::money::Coin::Gold,
                3
            )
        );
    }

    #[test]
    fn the_window_names_the_npc() {
        let (mut app, _rx) = app();
        online(&mut app).offer_trade(NPC);
        assert_eq!(partner(online(&mut app).world()), "Guard Example");
    }
}

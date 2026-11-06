use super::*;
use crate::{
    inventory::{InventoryState, SplitAction},
    online::{OnlineState, testing},
};
use eq_client_core::{
    Coins,
    inventory::{InventoryItem, InventorySlot, InventoryUpdate},
    merchant::{MerchantItem, MerchantUpdate},
};
use std::sync::mpsc::{self, Receiver};

fn stock(item: InventoryItem, quantity: u32, price: u32) -> WorldEvent {
    WorldEvent::Merchant(MerchantUpdate::Item(Box::new(MerchantItem {
        slot: 3,
        price,
        quantity,
        item,
    })))
}

fn fixture() -> (OnlineState, TradeState, Outbox, Receiver<ClientCommand>) {
    let mut online = OnlineState::new(true);
    testing::admit(&mut online, 1, testing::player(7));
    let mut item = crate::preview::items().remove(4);
    item.slot = InventorySlot(23);
    item.stack_count = Some(5);
    item.rules.stack_size = 20;
    item.details.price = Some(20);
    testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item.clone()]));
    testing::news(
        &mut online,
        [
            WorldEvent::Coins(Coins::from_copper(1_000)),
            WorldEvent::Merchant(MerchantUpdate::Opened {
                merchant_id: 8,
                accepted: true,
                rate: 1.0,
            }),
            stock(item, 0, 10),
        ],
    );
    let mut trade = TradeState::default();
    trade.follow(online.world());
    choose(&mut trade, online.world(), Chosen::Ware(3));
    let (tx, rx) = mpsc::sync_channel(8);
    (online, trade, Outbox::new(Some(tx)), rx)
}

fn choose(trade: &mut TradeState, world: &ClientWorld, chosen: Chosen) -> Selection {
    trade.merchant.as_mut().unwrap().chosen = Some(chosen);
    trade.changed();
    let selection = trade.quantity_selection(world).unwrap();
    trade.merchant_choice = Some(selection);
    selection
}

fn submit(
    trade: &mut TradeState,
    online: &OnlineState,
    outbox: &Outbox,
    selection: Selection,
    count: u32,
) -> Result<(), &'static str> {
    trade.submit_quantity(
        online.world(),
        outbox,
        outbox.peek(online.world()).unwrap(),
        selection,
        count,
    )
}

#[test]
fn merchant_quantity_bounds_use_stack_stock_money_and_one_for_charged_items() {
    let (mut online, trade, _, _) = fixture();
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Ware(3)),
        Ok(20)
    );
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Carried(23)),
        Ok(5)
    );
    let item = online.world().merchant().unwrap().stock[&3].item.clone();
    testing::news(&mut online, [stock(item.clone(), 4, 10)]);
    assert_eq!(trade.quantity_limit(online.world(), Chosen::Ware(3)), Ok(4));
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(29))]);
    assert_eq!(trade.quantity_limit(online.world(), Chosen::Ware(3)), Ok(2));
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(9))]);
    assert!(
        trade
            .quantity_limit(online.world(), Chosen::Ware(3))
            .is_err()
    );
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(1_000))]);
    let mut unknown = item.clone();
    unknown.rules.stack_size = 0;
    testing::news(&mut online, [stock(unknown, u32::MAX, 10)]);
    assert_eq!(trade.quantity_limit(online.world(), Chosen::Ware(3)), Ok(1));
    let mut charged = item;
    charged.stack_count = None;
    charged.charges = 10;
    charged.activation.maximum_charges = 10;
    testing::news(&mut online, [stock(charged, 10, 10)]);
    assert_eq!(trade.quantity_limit(online.world(), Chosen::Ware(3)), Ok(1));
}

#[test]
fn merchant_quantity_zero_excess_and_changed_purse_never_send() {
    let (mut online, mut trade, outbox, rx) = fixture();
    let selection = trade.quantity_selection(online.world()).unwrap();
    let before = online.world().inventory().clone();
    for count in [0, 21, u32::MAX] {
        assert!(submit(&mut trade, &online, &outbox, selection, count).is_err());
    }
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(10))]);
    assert!(selection.current(&trade, online.world()));
    assert!(submit(&mut trade, &online, &outbox, selection, 2).is_err());
    assert!(rx.try_recv().is_err());
    assert_eq!(*online.world().inventory(), before);
    assert!(trade.merchant_pending.is_none());
}

#[test]
fn merchant_quantity_partial_buy_and_sale_only_queue_exact_intent() {
    for chosen in [Chosen::Ware(3), Chosen::Carried(23)] {
        let (mut online, mut trade, outbox, rx) = fixture();
        let selection = choose(&mut trade, online.world(), chosen);
        let before = online.world().inventory().clone();
        let coins = online.world().coins().copied();
        let mut picker = InventoryState::default();
        picker.select_merchant(
            selection,
            trade.quantity_limit(online.world(), chosen).unwrap(),
        );
        picker.erase_digit();
        picker.type_digit(2);
        picker.split_action(SplitAction::Confirm, &mut online, &outbox);
        let (selection, count) = picker.take_merchant().unwrap();
        assert_eq!(count, 2);
        submit(&mut trade, &online, &outbox, selection, count).unwrap();
        match (chosen, rx.try_recv().unwrap()) {
            (
                Chosen::Ware(3),
                ClientCommand::Buy {
                    slot: 3,
                    quantity: 2,
                    ..
                },
            )
            | (
                Chosen::Carried(23),
                ClientCommand::Sell {
                    slot: 23,
                    quantity: 2,
                    ..
                },
            ) => (),
            other => panic!("wrong merchant intent: {other:?}"),
        }
        assert!(picker.take_merchant().is_none());
        assert!(submit(&mut trade, &online, &outbox, selection, count).is_err());
        assert!(rx.try_recv().is_err());
        assert_eq!(*online.world().inventory(), before);
        assert_eq!(online.world().coins(), coins.as_ref());
    }
}

#[test]
fn merchant_quantity_stale_catalog_inventory_choice_and_admission_are_rejected() {
    for cause in 0..5 {
        let (mut online, mut trade, outbox, rx) = fixture();
        let selection = trade.quantity_selection(online.world()).unwrap();
        let mut picker = InventoryState::default();
        picker.select_merchant(selection, 20);
        match cause {
            0 => testing::news(
                &mut online,
                [WorldEvent::Merchant(MerchantUpdate::Removed { slot: 3 })],
            ),
            1 => testing::inventory(&mut online, InventoryUpdate::Remove(InventorySlot(23))),
            2 => {
                choose(&mut trade, online.world(), Chosen::Carried(23));
            }
            3 => online.close_shop(),
            _ => testing::admit(&mut online, 2, testing::player(7)),
        }
        picker.refresh_merchant(&trade, online.world());
        assert!(!picker.picking_merchant());
        assert!(!selection.current(&trade, online.world()));
        assert!(submit(&mut trade, &online, &outbox, selection, 1).is_err());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn merchant_quantity_cancel_zero_and_disconnect_discard_unsent_intent() {
    for cause in 0..4 {
        let (mut online, trade, outbox, rx) = fixture();
        let selection = trade.quantity_selection(online.world()).unwrap();
        let mut picker = InventoryState::default();
        picker.select_merchant(selection, 20);
        match cause {
            0 => picker.split_action(SplitAction::Cancel, &mut online, &outbox),
            1 => {
                picker.erase_digit();
                picker.split_action(SplitAction::Confirm, &mut online, &outbox);
            }
            2 => {
                picker.split_action(SplitAction::Confirm, &mut online, &outbox);
                picker.cancel_merchant();
            }
            _ => {
                testing::connect(&mut online, false);
                picker.refresh_merchant(&trade, online.world());
            }
        }
        assert!(picker.take_merchant().is_none());
        assert!(!picker.picking_merchant());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn merchant_quantity_refusal_and_uncertainty_preserve_holdings_and_never_retry() {
    let (online, mut trade, outbox, rx) = fixture();
    let selection = trade.quantity_selection(online.world()).unwrap();
    let before = online.world().inventory().clone();
    let coins = online.world().coins();
    submit(&mut trade, &online, &outbox, selection, 2).unwrap();
    rx.try_recv().unwrap();
    trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::MerchantUnanswered {
            session_id: 9,
            reason: "old".into(),
        }),
        online.world(),
    );
    assert_eq!(trade.merchant_status(), Some("Waiting for merchant"));
    trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::MerchantUnanswered {
            session_id: 1,
            reason: "unknown".into(),
        }),
        online.world(),
    );
    assert_eq!(trade.merchant_status(), Some("Outcome unknown"));
    trade.merchant_news(
        &WorldUpdate::Game(stock(
            online.world().merchant().unwrap().stock[&3].item.clone(),
            0,
            10,
        )),
        online.world(),
    );
    trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::Merchant(MerchantUpdate::Bought {
            slot: 4,
            quantity: 2,
            price: 20,
        })),
        online.world(),
    );
    assert!(
        trade
            .quantity_limit(online.world(), Chosen::Ware(3))
            .is_err()
    );
    assert!(rx.try_recv().is_err());
    trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::MerchantRefused {
            session_id: 1,
            reason: "refused".into(),
        }),
        online.world(),
    );
    assert!(trade.merchant_status().is_none());
    assert!(trade.chosen().is_none());
    assert_eq!(*online.world().inventory(), before);
    assert_eq!(online.world().coins(), coins);
}

#[test]
fn merchant_quantity_matching_received_result_releases_only_its_action() {
    for chosen in [Chosen::Ware(3), Chosen::Carried(23)] {
        let (online, mut trade, outbox, rx) = fixture();
        let selection = choose(&mut trade, online.world(), chosen);
        submit(&mut trade, &online, &outbox, selection, 2).unwrap();
        rx.try_recv().unwrap();
        let event = match chosen {
            Chosen::Ware(slot) => MerchantUpdate::Bought {
                slot,
                quantity: 2,
                price: 20,
            },
            Chosen::Carried(slot) => MerchantUpdate::Sold {
                slot,
                quantity: 2,
                price: 38,
            },
        };
        trade.merchant_news(
            &WorldUpdate::Game(WorldEvent::Merchant(event)),
            online.world(),
        );
        assert!(trade.merchant_pending.is_none());
        assert!(trade.chosen().is_none());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn merchant_quantity_zero_result_or_received_close_releases_ui_without_delivery() {
    for (chosen, result) in [
        (
            Chosen::Ware(3),
            MerchantUpdate::Bought {
                slot: 0,
                quantity: 0,
                price: 0,
            },
        ),
        (
            Chosen::Carried(23),
            MerchantUpdate::Sold {
                slot: -1,
                quantity: 0,
                price: 0,
            },
        ),
        (Chosen::Ware(3), MerchantUpdate::Closed),
    ] {
        let (online, mut trade, outbox, rx) = fixture();
        let before = online.world().inventory().clone();
        let selection = choose(&mut trade, online.world(), chosen);
        submit(&mut trade, &online, &outbox, selection, 2).unwrap();
        rx.try_recv().unwrap();
        trade.merchant_news(
            &WorldUpdate::Game(WorldEvent::Merchant(result)),
            online.world(),
        );
        assert!(trade.merchant_pending.is_none());
        assert_eq!(*online.world().inventory(), before);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn merchant_quantity_sale_can_select_the_remaining_received_stack() {
    let (mut online, mut trade, outbox, rx) = fixture();
    let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
    submit(&mut trade, &online, &outbox, selection, 2).unwrap();
    rx.try_recv().unwrap();
    // The session supplies the deduction; the merchant UI does not infer it.
    testing::inventory(
        &mut online,
        InventoryUpdate::Deduct {
            slot: InventorySlot(23),
            quantity: 2,
        },
    );
    trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::Merchant(MerchantUpdate::Sold {
            slot: 23,
            quantity: 2,
            price: 38,
        })),
        online.world(),
    );
    let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
    let mut picker = InventoryState::default();
    picker.select_merchant(
        selection,
        trade
            .quantity_limit(online.world(), Chosen::Carried(23))
            .unwrap(),
    );
    picker.split_action(SplitAction::Maximum, &mut online, &outbox);
    assert_eq!(picker.picked(), Some((3, 3)));
    picker.split_action(SplitAction::Confirm, &mut online, &outbox);
    let (selection, count) = picker.take_merchant().unwrap();
    submit(&mut trade, &online, &outbox, selection, count).unwrap();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::Sell {
            slot: 23,
            quantity: 3,
            ..
        })
    ));
}

#[test]
fn merchant_quantity_no_drop_full_bags_and_stale_inventory_are_not_offered() {
    let (mut online, trade, _, _) = fixture();
    let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
    item.details.flags.push("NO DROP".into());
    testing::inventory(&mut online, InventoryUpdate::Set(vec![item.clone()]));
    assert!(
        trade
            .quantity_limit(online.world(), Chosen::Carried(23))
            .is_err()
    );
    item.details.flags.clear();
    item.bag_slots = 2;
    let mut inside = item.clone();
    inside.slot = InventorySlot(23).child(0).unwrap();
    testing::inventory(&mut online, InventoryUpdate::Set(vec![item, inside]));
    assert!(
        trade
            .quantity_limit(online.world(), Chosen::Carried(23))
            .is_err()
    );
    testing::inventory(&mut online, InventoryUpdate::Invalidated);
    assert!(
        trade
            .quantity_limit(online.world(), Chosen::Ware(3))
            .is_err()
    );
}

fn click<T: bevy::prelude::Component>(app: &mut bevy::prelude::App, action: T) {
    use bevy::prelude::*;
    let button = app
        .world_mut()
        .spawn((Button, Interaction::None, action))
        .id();
    crate::clicks::testing::click(app, button);
    app.world_mut().despawn(button);
}

#[test]
fn merchant_quantity_product_buttons_share_the_picker_and_close_discards_it() {
    use bevy::prelude::*;
    let (online, trade, outbox, rx) = fixture();
    let mut app = crate::testing::app();
    app.insert_resource(online)
        .insert_resource(trade)
        .insert_resource(outbox)
        .add_systems(
            Update,
            (crate::inventory::input, super::super::input).chain(),
        );
    click(&mut app, super::super::Action::BuyChosen);
    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.world().resource::<InventoryState>().picked(),
        Some((1, 20))
    );
    click(&mut app, SplitAction::More);
    assert_eq!(
        app.world().resource::<InventoryState>().picked(),
        Some((2, 20))
    );
    assert_eq!(
        app.world()
            .resource::<TradeState>()
            .quantity_quote(app.world().resource::<OnlineState>().world(), 2),
        Some(20)
    );
    click(&mut app, SplitAction::Confirm);
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::Buy {
            slot: 3,
            quantity: 2,
            ..
        })
    ));
    click(&mut app, super::super::Action::BuyChosen);
    assert!(rx.try_recv().is_err());
    assert!(!app.world().resource::<InventoryState>().picking_merchant());
    // Closing while a mutation is queued sends only the explicit close.
    click(&mut app, super::super::Action::EndShop);
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::Shop { open: false, .. })
    ));
    assert!(rx.try_recv().is_err());

    let (online, trade, outbox, rx) = fixture();
    app.insert_resource(online)
        .insert_resource(trade)
        .insert_resource(outbox);
    click(&mut app, super::super::Action::BuyChosen);
    click(&mut app, super::super::Action::EndShop);
    assert!(!app.world().resource::<InventoryState>().picking_merchant());
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::Shop { open: false, .. })
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn merchant_quantity_pending_transfer_and_terminal_end_never_repeat_the_action() {
    for terminal in [false, true] {
        let (mut online, mut trade, outbox, rx) = fixture();
        let selection = trade.quantity_selection(online.world()).unwrap();
        submit(&mut trade, &online, &outbox, selection, 2).unwrap();
        rx.try_recv().unwrap();
        let update = if terminal {
            WorldUpdate::Connection(eq_client_core::world::Link::Ended)
        } else {
            WorldUpdate::Game(WorldEvent::ZoneTransfer(eq_client_core::ZoneOffer {
                zone_id: 2,
                instance_id: 0,
                position: eq_client_core::WorldPosition::default(),
                reason: 0,
                to_bind: false,
                solicited: true,
            }))
        };
        online.tell(
            &update,
            std::time::Instant::now(),
            &eq_client_core::world::NoSpells,
        );
        trade.merchant_news(&update, online.world());
        assert!(trade.merchant_pending.is_none());
        assert!(!selection.current(&trade, online.world()));
        assert!(rx.try_recv().is_err());
    }
}

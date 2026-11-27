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
) -> Result<(), Refused> {
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
    let _ = trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::MerchantUnanswered {
            session_id: 9,
            reason: "old".into(),
        }),
        online.world(),
    );
    assert_eq!(trade.merchant_status(), Some("Waiting for merchant"));
    let _ = trade.merchant_news(
        &WorldUpdate::Game(WorldEvent::MerchantUnanswered {
            session_id: 1,
            reason: "unknown".into(),
        }),
        online.world(),
    );
    assert_eq!(trade.merchant_status(), Some("Outcome unknown"));
    let _ = trade.merchant_news(
        &WorldUpdate::Game(stock(
            online.world().merchant().unwrap().stock[&3].item.clone(),
            0,
            10,
        )),
        online.world(),
    );
    let _ = trade.merchant_news(
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
    let _ = trade.merchant_news(
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
        let _ = trade.merchant_news(
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
        let _ = trade.merchant_news(
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
    let _ = trade.merchant_news(
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
        let _ = trade.merchant_news(&update, online.world());
        assert!(trade.merchant_pending.is_none());
        assert!(!selection.current(&trade, online.world()));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn a_buy_follows_the_programs_order_and_lines() {
    let (mut online, mut trade, outbox, rx) = fixture();
    let selection = trade.quantity_selection(online.world()).unwrap();
    // A quantity below one, and a ware the merchant no longer lists, pass
    // silently (read R49-35 a).
    assert_eq!(
        submit(&mut trade, &online, &outbox, selection, 0),
        Err(Refused::Silent)
    );
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Ware(99)),
        Err(Refused::Silent)
    );
    // A silent refusal can't read as a buy that went: nothing is sent and
    // the merchant's action status, which a script's report shows as
    // `merchant.action_status`, stays empty.
    assert!(rx.try_recv().is_err());
    assert_eq!(trade.merchant_status(), None);
    // Something on the cursor: the program's line, before the coins.
    let mut held = crate::preview::items().remove(4);
    held.slot = InventorySlot::CURSOR;
    testing::inventory(&mut online, InventoryUpdate::Set(vec![held]));
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(1))]);
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Ware(3)),
        Err(Refused::CursorItem)
    );
    // Selling keeps this client's words for the cursor.
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Carried(23)),
        Err(Refused::Words("Place the cursor item before trading"))
    );
    // With the cursor clear, coins short of one ware: line 0x2f1c.
    testing::inventory(&mut online, InventoryUpdate::Remove(InventorySlot::CURSOR));
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Ware(3)),
        Err(Refused::Short(3))
    );
    assert!(rx.try_recv().is_err());
    // A pending action passes silently.
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(1_000))]);
    let selection = choose(&mut trade, online.world(), Chosen::Ware(3));
    submit(&mut trade, &online, &outbox, selection, 1).unwrap();
    assert!(matches!(rx.try_recv(), Ok(ClientCommand::Buy { .. })));
    // A buy that went shows as waiting; the silent refusal of a second
    // press neither sends nor changes that.
    assert_eq!(trade.merchant_status(), Some("Waiting for merchant"));
    assert_eq!(
        trade.quantity_limit(online.world(), Chosen::Ware(3)),
        Err(Refused::Silent)
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(trade.merchant_status(), Some("Waiting for merchant"));
}

#[test]
fn titanium_sale_words_wait_for_a_matched_result_after_the_item_has_left() {
    use eq_client_core::world::Notice;
    let (mut online, mut trade, outbox, rx) = fixture();
    let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
    item.details.name = "Bat Fur".into();
    item.details.price = Some(15);
    item.stack_count = Some(1);
    testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
    testing::news(
        &mut online,
        [WorldEvent::Merchant(MerchantUpdate::Opened {
            merchant_id: 8,
            accepted: true,
            rate: f32::from_bits(0x3f86_bca2),
        })],
    );
    trade.follow(online.world());
    trade.prices = Some(eq_client_assets::merchants::TITANIUM);
    let mut merchant = testing::pet(8, 7);
    merchant.name = "Merchant_Iludarae".into();
    testing::spawns(&mut online, vec![merchant]);
    trade.merchant.as_mut().unwrap().name = "Stale window label".into();
    let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
    assert_eq!(trade.quantity_quote(online.world(), 1), Some(14));
    submit(&mut trade, &online, &outbox, selection, 1).unwrap();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::Sell {
            slot: 23,
            quantity: 1,
            ..
        })
    ));
    let result = |slot, quantity| {
        WorldUpdate::Game(WorldEvent::Merchant(MerchantUpdate::Sold {
            slot,
            quantity,
            price: 13,
        }))
    };
    assert_eq!(trade.merchant_news(&result(24, 1), online.world()), None);
    assert_eq!(trade.merchant_news(&result(23, 2), online.world()), None);
    assert_eq!(
        trade.merchant_news(
            &WorldUpdate::Game(WorldEvent::MerchantUnanswered {
                session_id: 1,
                reason: "unknown".into(),
            }),
            online.world()
        ),
        None
    );
    // The session's deduction can reach the host before the sale event.
    testing::inventory(&mut online, InventoryUpdate::Remove(InventorySlot(23)));
    // The worker credits the server-paid amount independently of the quote.
    testing::news(&mut online, [WorldEvent::Coins(Coins::from_copper(13))]);
    let purse = online.world().coins();
    assert_eq!(
        trade.merchant_news(&result(23, 1), online.world()),
        Some(Notice::MerchantSold {
            merchant: "Merchant Iludarae".into(),
            item: "Bat Fur".into(),
            price: 14,
        })
    );
    assert_eq!(trade.merchant_news(&result(23, 1), online.world()), None);
    assert_eq!(
        online.world().coins(),
        purse,
        "caption never changes custody"
    );
    assert!(rx.try_recv().is_err(), "receipt/duplicate never retries");
}

#[test]
fn failed_send_refusal_close_and_other_generations_never_say_native_sale_success() {
    for outcome in [0, 1, 2, 3] {
        let (online, mut trade, outbox, rx) = fixture();
        trade.prices = Some(if outcome == 3 {
            eq_client_assets::merchants::EQMAC
        } else {
            eq_client_assets::merchants::TITANIUM
        });
        let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
        if outcome == 0 {
            // The admitted selection is stamped before the worker disconnects.
            // Keep the sender so this exercises send, rather than missing-queue setup.
            let (tx, disconnected) = mpsc::sync_channel(1);
            let failed = Outbox::new(Some(tx));
            let stamp = failed.peek(online.world()).unwrap();
            drop(disconnected);
            assert_eq!(
                trade.submit_quantity(online.world(), &failed, stamp, selection, 1),
                Ok(())
            );
            assert_eq!(failed.take_refused(), [crate::outbox::Refusal::Ended]);
            assert!(trade.merchant_pending.is_none());
            assert!(trade.merchant_sale.is_none());
            assert!(rx.try_recv().is_err(), "failed send queues no sale");
        } else {
            submit(&mut trade, &online, &outbox, selection, 1).unwrap();
            assert!(matches!(
                rx.try_recv(),
                Ok(ClientCommand::Sell {
                    slot: 23,
                    quantity: 1,
                    ..
                })
            ));
            assert!(rx.try_recv().is_err(), "one successful submission only");
        }
        let update = match outcome {
            1 => MerchantUpdate::Sold {
                slot: 23,
                quantity: 0,
                price: 0,
            },
            2 => MerchantUpdate::Closed,
            _ => MerchantUpdate::Sold {
                slot: 23,
                quantity: 1,
                price: 20,
            },
        };
        assert_eq!(
            trade.merchant_news(
                &WorldUpdate::Game(WorldEvent::Merchant(update)),
                online.world()
            ),
            None
        );
    }
}

#[test]
fn titanium_zero_value_and_unretained_augment_prices_are_not_invented() {
    let (mut online, mut trade, _, _) = fixture();
    trade.prices = Some(eq_client_assets::merchants::TITANIUM);
    for flag in ["NO RENT", "NO DROP"] {
        let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
        item.details.flags = vec![flag.into()];
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        choose(&mut trade, online.world(), Chosen::Carried(23));
        assert_eq!(trade.quantity_quote(online.world(), 1), Some(0));
    }
    let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
    item.details.flags.clear();
    item.augments[0] = Some(100);
    testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
    choose(&mut trade, online.world(), Chosen::Carried(23));
    assert_eq!(trade.quantity_quote(online.world(), 1), None);
}

#[test]
fn titanium_eligible_zero_value_sale_waits_for_the_server() {
    use eq_client_core::world::Notice;
    for (stack, quantity) in [(None, 1), (Some(3), 2)] {
        for accepted in [false, true] {
            let (mut online, mut trade, outbox, rx) = fixture();
            let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
            item.details.name = "Worthless item".into();
            item.details.price = Some(0);
            item.details.flags.clear();
            item.bag_slots = 0;
            item.stack_count = stack;
            testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
            trade.prices = Some(eq_client_assets::merchants::TITANIUM);
            let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
            let inventory = online.world().inventory().clone();
            let purse = online.world().coins().copied();
            assert_eq!(trade.quantity_quote(online.world(), quantity), Some(0));
            assert_eq!(
                trade.quantity_limit(online.world(), Chosen::Carried(23)),
                Ok(stack.unwrap_or(1))
            );
            submit(&mut trade, &online, &outbox, selection, quantity).unwrap();
            assert!(matches!(
                rx.try_recv(),
                Ok(ClientCommand::Sell { slot: 23, quantity: sent, .. }) if sent == quantity
            ));
            assert!(rx.try_recv().is_err());
            assert!(trade.merchant_pending.is_some());
            assert!(submit(&mut trade, &online, &outbox, selection, quantity).is_err());
            assert!(rx.try_recv().is_err(), "pending sale never retries");
            assert_eq!(*online.world().inventory(), inventory);
            assert_eq!(online.world().coins(), purse.as_ref());
            let reply = WorldUpdate::Game(WorldEvent::Merchant(MerchantUpdate::Sold {
                slot: if accepted { 23 } else { -1 },
                quantity: if accepted { quantity } else { 0 },
                price: 0,
            }));
            if accepted {
                testing::inventory(
                    &mut online,
                    InventoryUpdate::Deduct {
                        slot: InventorySlot(23),
                        quantity,
                    },
                );
            }
            assert_eq!(
                trade.merchant_news(&reply, online.world()),
                accepted.then(|| Notice::MerchantSold {
                    merchant: String::new(),
                    item: "Worthless item".into(),
                    price: 0,
                })
            );
            assert!(trade.merchant_pending.is_none());
            assert_eq!(trade.merchant_news(&reply, online.world()), None);
            assert_eq!(online.world().coins(), purse.as_ref());
            if !accepted {
                assert_eq!(*online.world().inventory(), inventory);
            }
            assert!(rx.try_recv().is_err(), "reply never sends another sale");
        }
    }
}

#[test]
fn titanium_zero_quote_does_not_bypass_known_item_restrictions() {
    let (mut online, mut trade, outbox, rx) = fixture();
    let mut item = online.world().inventory().items()[&InventorySlot(23)].clone();
    item.details.price = Some(0);
    item.details.flags = vec!["NO DROP".into()];
    testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
    trade.prices = Some(eq_client_assets::merchants::TITANIUM);
    let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
    assert_eq!(trade.quantity_quote(online.world(), 1), Some(0));
    assert_eq!(
        submit(&mut trade, &online, &outbox, selection, 1),
        Err(Refused::Words("The merchant will not buy NO DROP items."))
    );
    assert!(trade.merchant_pending.is_none());
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_different_merchant_or_admission_drops_the_old_sale_caption() {
    for fresh_admission in [false, true] {
        let (mut online, mut trade, outbox, _rx) = fixture();
        trade.prices = Some(eq_client_assets::merchants::TITANIUM);
        let selection = choose(&mut trade, online.world(), Chosen::Carried(23));
        submit(&mut trade, &online, &outbox, selection, 1).unwrap();
        if fresh_admission {
            testing::admit(&mut online, 2, testing::player(7));
        } else {
            testing::news(
                &mut online,
                [WorldEvent::Merchant(MerchantUpdate::Opened {
                    merchant_id: 9,
                    accepted: true,
                    rate: 1.0,
                })],
            );
        }
        assert_eq!(
            trade.merchant_news(
                &WorldUpdate::Game(WorldEvent::Merchant(MerchantUpdate::Sold {
                    slot: 23,
                    quantity: 1,
                    price: 20,
                })),
                online.world()
            ),
            None
        );
        assert!(trade.merchant_sale.is_none());
    }
}

//! Read-only projections for checking scripted transactions.
use crate::trade::{Chosen, TradeState};
use eq_client_core::{money::CoinPlace, world::ClientWorld};
use serde_json::{Value, json};

pub(super) fn state(world: &ClientWorld, trade: &TradeState) -> Value {
    let merchant = world.merchant().map(|merchant| {
        let rows = merchant
            .stock
            .values()
            .take(80)
            .map(|entry| {
                json!({
                    "slot": entry.slot,
                    "item_id": entry.item.details.id,
                    "listed_copper": entry.price,
                    "displayed_copper": trade.ware_price(world, entry.price),
                    "quantity": entry.quantity,
                })
            })
            .collect::<Vec<_>>();
        let selection = trade.chosen().map(|chosen| {
            let (side, slot, item_id) = match chosen {
                Chosen::Ware(slot) => (
                    "buy",
                    i64::from(slot),
                    merchant.stock.get(&slot).map(|entry| entry.item.details.id),
                ),
                Chosen::Carried(slot) => (
                    "sell",
                    i64::from(slot),
                    world
                        .inventory()
                        .items()
                        .get(&eq_client_core::inventory::InventorySlot(slot))
                        .map(|item| item.details.id),
                ),
            };
            json!({
                "side": side,
                "slot": slot,
                "item_id": item_id,
                "displayed_one_copper": trade.quantity_quote(world, 1),
            })
        });
        json!({
            "merchant_id": merchant.merchant_id,
            "revision": world.merchant_revision(),
            "row_count": merchant.stock.len(),
            "rows": rows,
            "selection": selection,
            "action_status": trade.merchant_status(),
        })
    });
    let loot = world.loot().map(|loot| {
        let rows = loot
            .items
            .iter()
            .take(32)
            .map(|(place, item)| {
                json!({
                    "place": place,
                    "item_id": item.details.id,
                    "stack_count": item.stack_count,
                })
            })
            .collect::<Vec<_>>();
        json!({
            "corpse_id": loot.corpse_id,
            "listed": loot.listed,
            "row_count": loot.items.len(),
            "rows": rows,
        })
    });
    json!({
        "version": 1,
        "session_id": world.session_id(),
        "merchant": merchant,
        "merchant_opening_unconfirmed": trade.opening_shop(world),
        "loot": loot,
        "given_coins": world.coins_in(CoinPlace::Trade),
        "offered_coins": world.offered_coins(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{
        WorldEvent,
        loot::LootUpdate,
        merchant::{MerchantItem, MerchantUpdate},
    };

    #[test]
    fn report_keeps_stock_and_corpse_rows_separate_from_delivery() {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        online.open_loot(10);
        let item = crate::preview::items().remove(0);
        let item_id = item.details.id;
        testing::news(
            &mut online,
            [
                WorldEvent::Merchant(MerchantUpdate::Opened {
                    merchant_id: 9,
                    accepted: true,
                    rate: 1.0,
                }),
                WorldEvent::Merchant(MerchantUpdate::Item(Box::new(MerchantItem {
                    slot: 3,
                    price: 42,
                    quantity: 2,
                    item: item.clone(),
                }))),
                WorldEvent::Loot(LootUpdate::Item {
                    place: 2,
                    item: Box::new(item),
                }),
                WorldEvent::Loot(LootUpdate::Listed { corpse_id: 10 }),
            ],
        );
        let report = state(online.world(), &TradeState::default());
        assert_eq!(report["session_id"], 1);
        assert_eq!(report["merchant"]["merchant_id"], 9);
        assert_eq!(
            report["merchant"]["rows"][0],
            json!({
                "slot": 3, "item_id": item_id, "listed_copper": 42,
                "displayed_copper": 42, "quantity": 2,
            })
        );
        assert_eq!(report["loot"]["corpse_id"], 10);
        assert_eq!(report["loot"]["rows"][0]["item_id"], item_id);
        assert_eq!(report["loot"]["listed"], true);
        assert_eq!(report["given_coins"], Value::Null);
        assert!(online.world().inventory().items().is_empty());
    }
}

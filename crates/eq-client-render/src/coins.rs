//! Coins in the windows, moved as the official client moves them: a click on
//! a coin box in the purse or the bank picks coins up onto the cursor (the
//! quantity picker asks how many, a shifted click takes them all), and a
//! click with coins on the cursor puts them down in the box clicked, the
//! give window's among them, changing kind as servers do. Servers answer no
//! coin move, so the world keeps the coins where the player put them.
use crate::{inventory::InventoryState, online::OnlineState, outbox::Outbox};
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    inventory::InventorySlot,
    money::{Coin, CoinPlace, CoinTransfer},
    world::ClientWorld,
};

/// A box that holds coins of one kind in a place.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CoinBox {
    pub place: CoinPlace,
    pub coin: Coin,
}

/// The coins on the cursor, the most valuable kind first; the cursor holds
/// one kind at a time.
pub(crate) fn on_cursor(world: &ClientWorld) -> Option<(Coin, u32)> {
    let coins = world.coins_in(CoinPlace::Cursor)?;
    Coin::ALL
        .into_iter()
        .map(|coin| (coin, coins.of(coin)))
        .find(|(_, count)| *count > 0)
}

/// Sends a coin move and keeps the coins where they went; a refusal shows in
/// the feedback line.
pub(crate) fn send(transfer: CoinTransfer, online: &mut OnlineState, outbox: &Outbox) {
    let sent = outbox
        .post(online.world(), |stamp| ClientCommand::MoveCoins {
            session_id: stamp.session_id,
            transfer,
            created: stamp.created,
        })
        .is_ok();
    if sent {
        online.move_coins(transfer);
    }
}

/// What a click on a coin box does: put down the cursor's coins, or pick
/// some up. None when it does nothing, as with an item on the cursor or a
/// box of a window that is not open.
pub(crate) fn click(
    target: CoinBox,
    world: &ClientWorld,
    bank_open: bool,
    all: bool,
) -> Option<Click> {
    let held = world.coins_in(target.place)?;
    if target.place == CoinPlace::Bank && !bank_open {
        return None;
    }
    if let Some((coin, amount)) = on_cursor(world) {
        return Some(Click::Move(CoinTransfer {
            from: CoinPlace::Cursor,
            to: target.place,
            coin,
            into: target.coin,
            amount,
        }));
    }
    // Coins given stay given until the window closes, as servers keep them.
    if world
        .inventory()
        .items()
        .contains_key(&InventorySlot::CURSOR)
        || target.place == CoinPlace::Trade
    {
        return None;
    }
    let available = held.of(target.coin);
    if available == 0 {
        return None;
    }
    Some(if all {
        Click::Move(CoinTransfer {
            from: target.place,
            to: CoinPlace::Cursor,
            coin: target.coin,
            into: target.coin,
            amount: available,
        })
    } else {
        Click::Pick(available)
    })
}

/// What a click on a coin box comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Click {
    /// Coins move now.
    Move(CoinTransfer),
    /// The quantity picker asks how many of this many to take.
    Pick(u32),
}

/// Picks coins up from a box, or puts the cursor's down in it, and carries
/// out what the quantity picker took.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(
    keys: crate::keys::Keys,
    mut online: ResMut<OnlineState>,
    outbox: Res<Outbox>,
    mut inventory: ResMut<InventoryState>,
    boxes: Query<(&Interaction, &CoinBox), Changed<Interaction>>,
) {
    if let Some(transfer) = inventory.take_coins() {
        send(transfer, &mut online, &outbox);
    }
    let Some((_, target)) = boxes
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
    else {
        return;
    };
    let all = keys
        .input
        .any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    match click(*target, online.world(), inventory.bank_open(), all) {
        Some(Click::Move(transfer)) => send(transfer, &mut online, &outbox),
        Some(Click::Pick(available)) => {
            inventory.select_coins(target.place, target.coin, available);
        }
        None => (),
    }
}

/// The coins shown in a box.
pub(crate) fn shown(world: &ClientWorld, place: CoinPlace, coin: Coin) -> String {
    world
        .coins_in(place)
        .map_or_else(String::new, |coins| coins.of(coin).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{Coins, WorldEvent, exchange::ExchangeUpdate};

    fn world() -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        testing::news(
            &mut online,
            [
                WorldEvent::Coins(Coins {
                    platinum: 2,
                    gold: 5,
                    silver: 0,
                    copper: 0,
                }),
                WorldEvent::CoinsElsewhere {
                    cursor: Coins::default(),
                    bank: Coins::default(),
                },
            ],
        );
        online
    }

    #[test]
    fn a_click_picks_coins_up_and_a_click_with_coins_puts_them_down() {
        let mut online = world();
        let purse_gold = CoinBox {
            place: CoinPlace::Purse,
            coin: Coin::Gold,
        };
        // The picker asks how many; a shifted click takes them all.
        assert_eq!(
            click(purse_gold, online.world(), false, false),
            Some(Click::Pick(5))
        );
        let Some(Click::Move(take)) = click(purse_gold, online.world(), false, true) else {
            panic!("all of them move");
        };
        assert_eq!(
            (take.from, take.to, take.amount),
            (CoinPlace::Purse, CoinPlace::Cursor, 5)
        );
        assert!(online.move_coins(take));
        // With coins on the cursor, a click on any box puts them down there.
        let bank_platinum = CoinBox {
            place: CoinPlace::Bank,
            coin: Coin::Platinum,
        };
        assert_eq!(click(bank_platinum, online.world(), false, false), None);
        assert_eq!(
            click(bank_platinum, online.world(), true, false),
            Some(Click::Move(CoinTransfer {
                from: CoinPlace::Cursor,
                to: CoinPlace::Bank,
                coin: Coin::Gold,
                into: Coin::Platinum,
                amount: 5,
            }))
        );
        // The give window's boxes take coins only while it is open, and give
        // none back.
        let given = CoinBox {
            place: CoinPlace::Trade,
            coin: Coin::Gold,
        };
        assert_eq!(click(given, online.world(), false, false), None);
        online.offer_trade(42);
        testing::news(
            &mut online,
            [WorldEvent::Exchange(ExchangeUpdate::Opened { with: 42 })],
        );
        let Some(Click::Move(give)) = click(given, online.world(), false, false) else {
            panic!("the cursor's coins go in");
        };
        assert!(online.move_coins(give));
        assert_eq!(shown(online.world(), CoinPlace::Trade, Coin::Gold), "5");
        assert_eq!(click(given, online.world(), false, true), None);
    }
}

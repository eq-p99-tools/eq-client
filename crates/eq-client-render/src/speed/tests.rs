use super::*;
use crate::online::testing;
use eq_client_core::{
    BaseAttributes, Buff, BuffUpdate, Coins, WorldEvent,
    inventory::{InventoryItem, InventorySlot, InventoryUpdate},
};

fn online() -> OnlineState {
    let mut state = OnlineState::new(true);
    let mut player = testing::player(7);
    player.base_attributes = Some(BaseAttributes {
        strength: 90,
        ..BaseAttributes::default()
    });
    player.level = 20;
    testing::admit(&mut state, 1, player);
    testing::news(
        &mut state,
        [
            WorldEvent::BuffSnapshot(Vec::new()),
            WorldEvent::SpeedInputsRequired {
                session_id: 1,
                revision: 1,
            },
        ],
    );
    state
}

fn ready(state: &mut OnlineState) {
    testing::news(
        state,
        [
            WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new())),
            WorldEvent::Coins(Coins::default()),
            WorldEvent::CoinsElsewhere {
                cursor: Coins::default(),
                bank: Coins::default(),
                given: Coins::default(),
                offered: Coins::default(),
            },
        ],
    );
}

fn item(slot: i32, weight: u32, strength: i32) -> InventoryItem {
    let mut item = crate::preview::items().remove(0);
    item.slot = InventorySlot(slot);
    item.details.weight_tenths = weight;
    item.details.bonuses = Some(eq_client_core::ItemBonuses {
        strength,
        ..Default::default()
    });
    item.details.equipment = Some(eq_client_core::EquipmentRules::default());
    item.details.slots = u32::MAX;
    item.details.classes = u32::MAX;
    item.details.races = u32::MAX;
    item.bag_slots = 0;
    item.rules = eq_client_core::inventory::ItemPlacement::default();
    item.stack_count = None;
    item
}

fn effects(spell: u32) -> Option<Vec<Effect>> {
    let (id, base, maximum, formula) = match spell {
        1 => (3, 10, 50, 102),
        2 => (3, -20, -60, 102),
        3 => (99, 0, 0, 100),
        4 => (4, 5, 50, 102),
        5 => (4, -200, 0, 100),
        6 => (3, 10, 50, 123),
        _ => return None,
    };
    Some(vec![Effect {
        id,
        base,
        maximum,
        formula,
        limit: 0,
    }])
}

fn buff(slot: u32, spell_id: u32) -> WorldEvent {
    WorldEvent::Buff(BuffUpdate {
        entity_id: 7,
        slot,
        spell_id,
        buff: Some(Buff {
            spell_id,
            caster_level: 20,
            bard_modifier: 10,
            effect_type: 2,
            duration_ticks: 10,
            counters: 0,
            caster_id: 7,
        }),
    })
}

#[test]
fn absent_empty_invalidated_and_recovered_state_are_distinct() {
    let mut state = online();
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(Vec::new()))],
    );
    assert_eq!(calculate(state.world(), effects), Err(Missing::Coins));
    ready(&mut state);
    assert_eq!(
        calculate(state.world(), effects),
        Ok(SpeedInputs {
            rate: 0,
            weight: 0,
            capacity: 90
        })
    );
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Invalidated)],
    );
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    ready(&mut state);
    assert!(calculate(state.world(), effects).is_ok());
    testing::admit(&mut state, 2, testing::player(7));
    assert_eq!(state.world().speed_input_revision(), None);
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    testing::news(
        &mut state,
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 99,
        }],
    );
    assert_eq!(state.world().speed_input_revision(), None);
}

#[test]
fn worn_strength_coin_changes_and_weight_preserving_predictions_recalculate() {
    let mut state = online();
    ready(&mut state);
    let worn = item(17, 100, 15);
    let carried = item(22, 40, 200);
    testing::news(
        &mut state,
        [
            WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                worn.clone(),
                carried.clone(),
            ])),
            WorldEvent::Coins(Coins {
                copper: 80,
                ..Default::default()
            }),
        ],
    );
    assert_eq!(
        calculate(state.world(), effects),
        Ok(SpeedInputs {
            rate: 0,
            weight: 16,
            capacity: 105
        })
    );
    let mut moved = carried.clone();
    moved.slot = InventorySlot(23);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Prediction(vec![
            worn.clone(),
            moved,
        ]))],
    );
    assert_eq!(calculate(state.world(), effects).unwrap().weight, 16);
    // Moving weight into the bank must not make an unconfirmed faster grant.
    let mut banked = carried;
    banked.slot = InventorySlot(2000);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Prediction(vec![
            worn.clone(),
            banked,
        ]))],
    );
    assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
    testing::news(
        &mut state,
        [
            WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![worn])),
            WorldEvent::Coins(Coins::default()),
            WorldEvent::CoinsElsewhere {
                cursor: Coins::default(),
                bank: Coins::default(),
                given: Coins::default(),
                offered: Coins::default(),
            },
        ],
    );
    assert_eq!(calculate(state.world(), effects).unwrap().weight, 10);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Remove(
            InventorySlot(17),
        ))],
    );
    assert_eq!(calculate(state.world(), effects).unwrap().capacity, 90);
}

#[test]
fn scaled_snare_overrides_speed_and_root_fade_restores_without_inflation() {
    let mut state = online();
    ready(&mut state);
    testing::news(&mut state, [buff(0, 1), buff(1, 4)]);
    assert_eq!(
        calculate(state.world(), effects),
        Ok(SpeedInputs {
            rate: 30,
            weight: 0,
            capacity: 115
        })
    );
    testing::news(&mut state, [buff(2, 2)]);
    assert_eq!(calculate(state.world(), effects).unwrap().rate, -40);
    testing::news(&mut state, [buff(3, 3)]);
    assert_eq!(calculate(state.world(), effects).unwrap().rate, -1000);
    testing::news(
        &mut state,
        [WorldEvent::Buff(BuffUpdate {
            entity_id: 7,
            slot: 3,
            spell_id: 3,
            buff: None,
        })],
    );
    assert_eq!(calculate(state.world(), effects).unwrap().rate, -40);
    testing::news(&mut state, [buff(4, 5)]);
    assert_eq!(calculate(state.world(), effects).unwrap().capacity, 1);
    testing::news(&mut state, [buff(5, 6)]);
    assert_eq!(calculate(state.world(), effects), Err(Missing::Effects));
    testing::news(&mut state, [buff(5, 999)]);
    assert_eq!(calculate(state.world(), effects), Err(Missing::Effects));
}

#[test]
fn reports_withdrawal_recovery_each_revision_and_retries_full_queue() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut app = crate::testing::app();
    app.insert_resource(Outbox::new(Some(tx)))
        .insert_resource(online())
        .add_systems(Update, report);
    app.update();
    // Fill the queue with the first unavailable report, then supply a snapshot.
    ready(&mut app.world_mut().resource_mut::<OnlineState>());
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(_),
            ..
        })
    ));
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            revision: 1,
            state: SpeedInputState::Ready(SpeedInputs { weight: 0, .. }),
            ..
        })
    ));
    app.update();
    assert!(rx.try_recv().is_err());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 2,
        }],
    );
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            revision: 2,
            state: SpeedInputState::Ready(_),
            ..
        })
    ));
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Inventory(InventoryUpdate::Invalidated)],
    );
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Inventory),
            ..
        })
    ));
}

#[test]
fn a_large_snare_cannot_masquerade_as_the_root_sentinel() {
    let mut state = online();
    ready(&mut state);
    testing::news(&mut state, [buff(0, 1)]);
    let extreme = |_| {
        Some(vec![Effect {
            id: 3,
            base: -2000,
            maximum: 0,
            formula: 100,
            limit: 0,
        }])
    };
    assert_eq!(calculate(state.world(), extreme).unwrap().rate, -999);
}

#[test]
fn eqmac_fatigue_recalculates_effective_strength_from_reported_state() {
    use eq_client_core::{GameDialect, WorldUpdate, food::Nourishment, world::NoSpells};
    let source = online();
    let mut world = ClientWorld::default();
    world.carry_as(GameDialect::EqMac);
    let mut player = source.world().player().unwrap().clone();
    player.base_attributes.as_mut().unwrap().stamina = 60;
    let tell = |world: &mut ClientWorld, event| {
        world.apply(
            &WorldUpdate::Game(event),
            std::time::Instant::now(),
            &NoSpells,
        );
    };
    for event in [
        WorldEvent::Entered {
            session_id: 1,
            capabilities: vec![],
            choices: vec![],
            zone: "synthetic".into(),
            player: Box::new(player),
            far_clip: None,
            spell_landing_flag: None,
        },
        WorldEvent::BuffSnapshot(vec![]),
        WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![])),
        WorldEvent::Coins(Coins::default()),
        WorldEvent::CoinsElsewhere {
            cursor: Coins::default(),
            bank: Coins::default(),
            given: Coins::default(),
            offered: Coins::default(),
        },
    ] {
        tell(&mut world, event);
    }
    // Missing fatigue has a source-defined bounded penalty, not an invented zero.
    assert_eq!(calculate(&world, effects).unwrap().capacity, 80);
    for (fatigue, strength) in [(0, 90), (65, 85), (100, 80), (0, 90)] {
        tell(
            &mut world,
            WorldEvent::Nourishment(Nourishment {
                food: 6000,
                water: 6000,
                fatigue: Some(fatigue),
            }),
        );
        assert_eq!(calculate(&world, effects).unwrap().capacity, strength);
    }
}

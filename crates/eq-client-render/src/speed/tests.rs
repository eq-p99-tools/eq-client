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

/// Queue through the actual host owner while retaining the worker result.
fn queue_equipment_move(
    state: &mut OnlineState,
    from: i32,
    to: i32,
) -> (eq_client_core::inventory::InventoryMove, InventoryUpdate) {
    queue_inventory_quantity(
        state,
        from,
        to,
        eq_client_core::inventory::MoveQuantity::Whole,
    )
}

fn queue_inventory_quantity(
    state: &mut OnlineState,
    from: i32,
    to: i32,
    quantity: eq_client_core::inventory::MoveQuantity,
) -> (eq_client_core::inventory::InventoryMove, InventoryUpdate) {
    use eq_client_core::inventory::{InventoryActor, InventoryMove, MoveRules};
    state.plan_inventory_as(Some(MoveRules::Quarm));
    let before = state.world().inventory().clone();
    let player = state.world().player().unwrap();
    let actor = InventoryActor {
        bank_access: false,
        deity: player.deity,
        dual_wield: None,
        class: player.class,
        race: player.race,
        level: player.level,
        trade_slots: 0,
        trade_no_drop: false,
        world_container: false,
    };
    let request = InventoryMove {
        session_id: state.world().session_id().unwrap(),
        revision: before.revision(),
        sequence: None,
        from: InventorySlot(from),
        to: InventorySlot(to),
        quantity,
        created: std::time::Instant::now(),
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let outbox = Outbox::new(Some(tx));
    state.queue_inventory_move(request, actor, &outbox).unwrap();
    let ClientCommand::MoveInventory(request) = rx.try_recv().unwrap() else {
        panic!("Expected one inventory request");
    };
    let update = before
        .plan_move_for(MoveRules::Quarm, &request, actor)
        .unwrap();
    (request, update)
}

fn equipment_move_result(
    request: &eq_client_core::inventory::InventoryMove,
    update: Option<InventoryUpdate>,
    error: Option<String>,
) -> WorldEvent {
    WorldEvent::InventoryAction {
        session_id: request.session_id,
        revision: request.revision,
        sequence: request.sequence,
        update,
        error,
    }
}

fn submit_equipment_move(state: &mut OnlineState, from: i32, to: i32) {
    let (request, update) = queue_equipment_move(state, from, to);
    testing::news(state, [equipment_move_result(&request, Some(update), None)]);
}

#[test]
fn root_stack_mass_bound_uses_worker_results_without_confirming_placement() {
    use eq_client_core::inventory::MoveQuantity;
    let mut state = online();
    ready(&mut state);
    let mut milk = item(22, 1, 0);
    milk.stack_count = Some(19);
    milk.rules.stack_size = 20;
    milk.rules.item_type = 15;
    let mut other = item(24, 43, 0);
    other.details.id += 1;
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            milk, other,
        ]))],
    );
    assert_eq!(state.world().movement_weight(), Ok(44));
    let count = MoveQuantity::Count(std::num::NonZeroU32::new(1).unwrap());
    for cycle in 0..3 {
        for (from, to, amount) in [
            (22, 30, count),
            (30, 29, MoveQuantity::Whole),
            (29, 30, MoveQuantity::Whole),
            (30, 22, count),
        ] {
            let before = calculate(state.world(), effects).unwrap();
            let (request, update) = queue_inventory_quantity(&mut state, from, to, amount);
            assert_eq!(calculate(state.world(), effects), Ok(before));
            assert_ne!(state.world().inventory(), state.world().worker_inventory());
            testing::news(
                &mut state,
                [equipment_move_result(&request, Some(update), None)],
            );
            let bound = if cycle == 0 && from == 22 { 45 } else { 46 };
            assert_eq!(state.world().movement_weight(), Ok(bound));
            assert_eq!(calculate(state.world(), effects).unwrap().weight, 4);
            assert!(state.world().worker_inventory().predicted());
            assert!(
                state
                    .world()
                    .worker_inventory()
                    .carried_weight_is_conservative()
            );
            assert!(!state.world().worker_inventory().equipment_stats_uncertain());
            assert_eq!(state.world().accepted_speed_inputs(), None);
        }
    }
    assert!(state.world().worker_inventory().unresolved_predictions() > 0);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Invalidated)],
    );
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
}

#[test]
fn a_stack_mass_bound_does_not_certify_stat_food() {
    use eq_client_core::inventory::MoveQuantity;
    let mut state = online();
    ready(&mut state);
    let mut milk = item(22, 7, 5);
    milk.stack_count = Some(4);
    milk.rules.stack_size = 20;
    milk.rules.item_type = 15;
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![milk]))],
    );
    let (request, update) = queue_inventory_quantity(
        &mut state,
        22,
        30,
        MoveQuantity::Count(std::num::NonZeroU32::new(1).unwrap()),
    );
    testing::news(
        &mut state,
        [equipment_move_result(&request, Some(update), None)],
    );
    assert_eq!(state.world().movement_weight(), Ok(14));
    assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
    assert!(state.world().worker_inventory().predicted());
}

#[test]
fn known_stat_neutral_equipment_keeps_readiness_without_confirming_placement() {
    let mut state = online();
    ready(&mut state);
    let original = item(24, 37, 0);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original.clone(),
        ]))],
    );
    let expected = SpeedInputs {
        rate: 0,
        weight: 3,
        capacity: 90,
        eqmac_fatigue: None,
    };
    assert_eq!(calculate(state.world(), effects), Ok(expected));
    for (from, to) in [(24, 30), (30, 13), (13, 30), (30, 24)] {
        submit_equipment_move(&mut state, from, to);
        assert_eq!(calculate(state.world(), effects), Ok(expected));
        assert!(state.world().inventory().predicted());
    }
    assert_eq!(
        state.world().inventory().items()[&InventorySlot(24)],
        original
    );
    assert_eq!(state.world().inventory().unresolved_predictions(), 4);
    assert!(
        state
            .world()
            .inventory()
            .prediction_origins()
            .any(|(slot, origin)| slot == InventorySlot(13) && origin.is_none())
    );
    assert_eq!(state.world().accepted_speed_inputs(), None);
}

#[test]
fn affecting_intermediate_equipment_stays_unavailable_after_a_net_zero_round_trip() {
    for strength in [-7, 7] {
        let mut state = online();
        ready(&mut state);
        let original = item(24, 37, strength);
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                original.clone(),
            ]))],
        );
        submit_equipment_move(&mut state, 24, 30);
        submit_equipment_move(&mut state, 30, 13);
        assert_eq!(state.world().movement_weight(), Ok(37));
        assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
        submit_equipment_move(&mut state, 13, 30);
        submit_equipment_move(&mut state, 30, 24);
        assert_eq!(
            state.world().inventory().items()[&InventorySlot(24)],
            original
        );
        assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
        assert!(state.world().inventory().predicted());
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                original,
            ]))],
        );
        assert!(calculate(state.world(), effects).is_ok());
    }
}

#[test]
fn authoritative_equipment_with_missing_metadata_remains_an_inventory_failure() {
    for missing_rules in [false, true] {
        let mut state = online();
        ready(&mut state);
        let mut incomplete = item(13, 37, 0);
        if missing_rules {
            incomplete.details.equipment = None;
        } else {
            incomplete.details.bonuses = None;
        }
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                incomplete,
            ]))],
        );
        assert_eq!(state.world().movement_weight(), Ok(37));
        assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    }
}

#[test]
fn original_strength_equipment_out_and_back_cannot_regrant_from_equal_endpoints() {
    for strength in [-7, 7] {
        let mut state = online();
        ready(&mut state);
        let original = item(13, 37, strength);
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                original.clone(),
            ]))],
        );
        assert!(calculate(state.world(), effects).is_ok());
        submit_equipment_move(&mut state, 13, 30);
        assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
        submit_equipment_move(&mut state, 30, 13);
        assert_eq!(
            state.world().inventory().items()[&InventorySlot(13)],
            original
        );
        assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
        assert_eq!(state.world().movement_weight(), Ok(37));
        assert_eq!(state.world().accepted_speed_inputs(), None);
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                original,
            ]))],
        );
        assert!(calculate(state.world(), effects).is_ok());
    }
}

#[test]
fn unversioned_partial_coverage_does_not_certify_affecting_equipment_inputs() {
    let mut state = online();
    ready(&mut state);
    let original = item(24, 37, 7);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original.clone(),
        ]))],
    );
    for (from, to) in [(24, 30), (30, 13), (13, 30), (30, 24)] {
        submit_equipment_move(&mut state, from, to);
    }
    testing::news(
        &mut state,
        [
            WorldEvent::Inventory(InventoryUpdate::Remove(InventorySlot(13))),
            WorldEvent::Inventory(InventoryUpdate::Set(vec![original.clone()])),
            WorldEvent::Inventory(InventoryUpdate::Remove(InventorySlot::CURSOR)),
        ],
    );
    assert!(!state.world().inventory().predicted());
    assert!(!state.world().inventory().stale());
    assert!(state.world().inventory().equipment_stats_uncertain());
    assert_eq!(state.world().movement_weight(), Ok(37));
    assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
    assert_eq!(state.world().accepted_speed_inputs(), None);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original,
        ]))],
    );
    assert!(calculate(state.world(), effects).is_ok());
}

#[test]
fn projected_affecting_tail_keeps_its_history_through_partial_correction_and_drain() {
    let mut state = online();
    ready(&mut state);
    let original = item(24, 37, -7);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original.clone(),
        ]))],
    );
    let (first, first_update) = queue_equipment_move(&mut state, 24, 30);
    let (second, second_update) = queue_equipment_move(&mut state, 30, 13);
    let (third, _) = queue_equipment_move(&mut state, 13, 30);
    let (fourth, _) = queue_equipment_move(&mut state, 30, 24);
    let projected = state.world().inventory().clone();
    assert_eq!(projected.items()[&InventorySlot(24)], original);
    assert!(projected.equipment_stats_uncertain());
    assert_eq!(
        calculate(state.world(), effects),
        Ok(SpeedInputs {
            rate: 0,
            weight: 3,
            capacity: 90,
            eqmac_fatigue: None,
        })
    );
    testing::news(
        &mut state,
        [
            equipment_move_result(&first, Some(first_update), None),
            equipment_move_result(&second, Some(second_update), None),
        ],
    );
    assert!(state.world().inventory().equipment_stats_uncertain());
    assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
    // The worker prefix is equipped, so this older empty slot record is a
    // correction. The local overlay must drop; later queued requests drain.
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Remove(
            InventorySlot(13),
        ))],
    );
    assert!(state.world().inventory().stale());
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    testing::news(
        &mut state,
        [
            equipment_move_result(&third, None, Some("Revision changed".into())),
            equipment_move_result(&fourth, None, Some("Predecessor refused".into())),
        ],
    );
    assert!(!state.world().inventory_moves_pending());
    assert!(state.world().inventory().equipment_stats_uncertain());
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original,
        ]))],
    );
    assert!(!state.world().inventory().equipment_stats_uncertain());
    assert!(projected.equipment_stats_uncertain());
    assert!(calculate(state.world(), effects).is_ok());
    assert_eq!(state.world().accepted_speed_inputs(), None);
}

#[test]
fn new_admission_and_partial_items_cannot_reuse_unrecorded_neutrality() {
    let mut state = online();
    ready(&mut state);
    let original = item(24, 37, 7);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            original,
        ]))],
    );
    submit_equipment_move(&mut state, 24, 30);
    submit_equipment_move(&mut state, 30, 13);
    let old = state.world().inventory().clone();
    testing::admit(&mut state, 2, testing::player(7));
    assert!(!state.world().inventory().received());
    assert!(!state.world().inventory().equipment_stats_uncertain());
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Set(vec![item(
            13, 37, 0,
        )]))],
    );
    assert!(!state.world().inventory().received());
    assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
    assert!(old.equipment_stats_uncertain());
}

#[test]
fn queued_food_projection_does_not_mix_with_worker_movement_weight_or_capacity() {
    let mut state = online();
    ready(&mut state);
    let mut first = item(22, 0, 7);
    let mut second = item(23, 0, 1);
    first.rules.item_type = 14;
    second.rules.item_type = 14;
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            first, second,
        ]))],
    );
    let expected = SpeedInputs {
        rate: 0,
        weight: 0,
        capacity: 97,
        eqmac_fatigue: None,
    };
    assert_eq!(calculate(state.world(), effects), Ok(expected));
    let (request, update) = queue_equipment_move(&mut state, 22, 30);
    assert_ne!(state.world().inventory(), state.world().worker_inventory());
    assert_eq!(calculate(state.world(), effects), Ok(expected));
    assert_eq!(state.world().accepted_speed_inputs(), None);
    testing::news(
        &mut state,
        [equipment_move_result(&request, Some(update), None)],
    );
    assert_eq!(calculate(state.world(), effects), Err(Missing::Prediction));
    assert_eq!(state.world().accepted_speed_inputs(), None);
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
        replaces_slot: false,
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
fn titanium_instrument_scales_capped_signed_effects_and_preserves_unknowns() {
    let project = |spell, level, modifier, dialect| {
        super::project(&effects(spell).unwrap(), level, modifier, dialect)
    };
    let titanium = GameDialect::Titanium;
    assert_eq!(project(1, 60, 21, titanium).unwrap().rate, 105);
    assert_eq!(project(2, 60, 21, titanium).unwrap().rate, -126);
    assert_eq!(project(4, 20, 21, titanium).unwrap().strength, 52);
    for neutral in [0, 10] {
        assert_eq!(project(1, 20, neutral, titanium).unwrap().rate, 30);
    }
    for unresolved in [9, 46, 0x0100_0015] {
        assert!(matches!(
            project(1, 60, unresolved, titanium),
            Err(Missing::Effects)
        ));
    }
    assert!(matches!(project(1, 0, 21, titanium), Err(Missing::Effects)));
    assert!(matches!(
        project(6, 60, 21, titanium),
        Err(Missing::Effects)
    ));
    assert!(matches!(
        project(1, 60, 21, GameDialect::EqMac),
        Err(Missing::Effects)
    ));
}

fn synthetic_speed_song() -> crate::spellbook::SpellNames {
    let mut fields = vec!["0"; 183];
    fields[0] = "42";
    fields[1] = "Synthetic speed song";
    fields[16] = "5";
    fields[17] = "3";
    fields[20] = "15";
    fields[44] = "65";
    fields[70] = "102";
    fields[86..98].fill("254");
    fields[86] = "3";
    crate::spellbook::SpellNames::parse(&fields.join("^"))
}

#[test]
fn bard_speed_reports_follow_landing_refresh_and_fade_across_equipment_changes() {
    let mut state = online();
    let mut player = state.world().player().unwrap().clone();
    player.class = Some(8);
    player.level = 60;
    testing::admit(&mut state, 2, player);
    ready(&mut state);
    let mut drum = item(14, 0, 0);
    drum.rules.item_type = 26;
    let drum_slot = drum.slot;

    let spells = synthetic_speed_song();
    let song = Buff {
        spell_id: 42,
        caster_level: 60,
        bard_modifier: 21,
        effect_type: 2,
        duration_ticks: 0,
        counters: 0,
        caster_id: 7,
    };
    let landing = |instrument_modifier, target_id, effect_flag| {
        WorldEvent::SpellEffect(eq_client_core::SpellEffect {
            target_id,
            caster_id: 7,
            caster_level: 60,
            instrument_modifier,
            spell_id: 42,
            spell_level: 60,
            effect_flag,
            push: None,
        })
    };
    let slot = |buff, entity_id| {
        WorldEvent::Buff(BuffUpdate {
            replaces_slot: false,
            entity_id,
            slot: 24,
            spell_id: 42,
            buff,
        })
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = crate::testing::app();
    app.insert_resource(state)
        .insert_resource(spells)
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, report);
    let mut revision = 0;
    let mut last_report_id = 0;
    let mut check = |events: Vec<_>, rate, require_revision: bool| {
        let required = require_revision.then(|| {
            revision += 1;
            WorldEvent::SpeedInputsRequired {
                session_id: 2,
                revision,
            }
        });
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            events.into_iter().chain(required),
        );
        app.update();
        let ClientCommand::SpeedInputs {
            session_id,
            revision: report_revision,
            report_id,
            state: SpeedInputState::Ready(inputs),
        } = rx.try_recv().unwrap()
        else {
            panic!("expected a ready speed report");
        };
        assert_eq!(
            (session_id, report_revision, inputs.rate),
            (2, revision, rate)
        );
        assert!(report_id > last_report_id);
        last_report_id = report_id;
    };

    check(
        vec![
            WorldEvent::BuffSnapshot(vec![]),
            WorldEvent::Inventory(InventoryUpdate::Set(vec![drum])),
            landing(21, 7, 0),
            landing(21, 8, 4),
        ],
        0,
        true,
    );
    // P99 does not use a spell action to advance the session's revision.
    check(vec![landing(21, 7, 4)], 136, false);
    check(
        vec![WorldEvent::Inventory(InventoryUpdate::Remove(drum_slot))],
        136,
        true,
    );
    check(vec![landing(10, 7, 4)], 65, false);
    check(vec![slot(Some(song), 7)], 136, true);
    check(vec![slot(None, 7)], 0, true);
    // The explicit fade also clears a landing that never received a slot.
    check(vec![landing(21, 7, 4)], 136, false);
    check(vec![slot(None, 7)], 0, true);
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
            capacity: 90,
            eqmac_fatigue: None,
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

fn movement_notice(app: &App) -> &str {
    app.world()
        .resource::<crate::notices::Lines>()
        .movement
        .text()
}

#[test]
fn readiness_tracks_receipts_even_when_the_unavailable_report_is_unchanged() {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = crate::testing::app();
    app.insert_resource(online())
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, report);
    app.update();
    assert_eq!(
        movement_notice(&app),
        "Movement paused: waiting for inventory information."
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Inventory),
            ..
        })
    ));
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![]))],
    );
    app.update();
    assert_eq!(
        movement_notice(&app),
        "Movement paused: waiting for carried coin information."
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Coins),
            ..
        })
    ));
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Coins(Coins::default())],
    );
    for _ in 0..3 {
        app.update();
        assert_eq!(
            movement_notice(&app),
            "Movement paused: waiting for cursor coin information."
        );
        // The more useful wording does not send duplicate unavailable reports.
        assert!(rx.try_recv().is_err());
    }
    assert_eq!(
        app.world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::System)
            .len(),
        0
    );
    ready(&mut app.world_mut().resource_mut::<OnlineState>());
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Ready(_),
            ..
        })
    ));
    // Local readiness and successful enqueue do not clear a pending notice.
    assert_eq!(
        movement_notice(&app),
        "Movement paused: waiting for the session to accept the movement inputs."
    );
}

fn reporting_ready() -> (App, std::sync::mpsc::Receiver<ClientCommand>) {
    let mut state = online();
    ready(&mut state);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = crate::testing::app();
    app.insert_resource(state)
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, report);
    app.update();
    (app, rx)
}

fn ready_spell_provider(
    names: &mut crate::spellbook::SpellNames,
    binding: eq_client_core::spell_tables::Binding,
) {
    use eq_client_core::spell_tables::{Context, Event, Fact, SpellId, Stage, Table, Update};
    names.bind(binding);
    for fact in [
        Fact::RunStarted {
            run_id: 7,
            attempt: 1,
        },
        Fact::Table(Event::new(
            Context::new(7, 1, Stage::World),
            Update::Table(Table::Registration(vec![SpellId::new(1).unwrap()])),
        )),
        Fact::ZonePending { run_id: 7 },
        Fact::Table(Event::new(
            Context::new(7, 2, Stage::Zone { session_id: 1 }),
            Update::Table(Table::Replay(Vec::new())),
        )),
        Fact::ZoneEntered {
            run_id: 7,
            session_id: 1,
        },
    ] {
        names.receive(&binding.envelope(fact));
    }
}

#[test]
fn spell_provider_recovery_requires_a_fresh_matching_movement_receipt() {
    use eq_client_core::spell_tables::{Binding, Context, Event, Fact, Policy, Stage, Update};
    use eq_network_game::spell_tables::{DecodeError, Kind, Unavailable};
    let (mut app, rx) = reporting_ready();
    let original = receipt(&rx.try_recv().unwrap(), true);
    let mut fields = vec!["0"; 183];
    fields[0] = "1";
    fields[1] = "Synthetic affecting spell";
    fields[86..98].fill("254");
    let binding = Binding::new(Policy::Project1999);
    let mut names = crate::spellbook::SpellNames::parse(&fields.join("^"));
    ready_spell_provider(&mut names, binding);
    app.insert_resource(names);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [buff(0, 1)],
    );
    app.update();
    // The known non-speed spell retains the previous numeric inputs, so the
    // receipt owner may retain the already-posted report identity.
    assert!(rx.try_recv().is_err());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(original),
        ],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    let failed = Event::new(
        Context::new(7, 3, Stage::Zone { session_id: 1 }),
        Update::Unavailable {
            kind: Kind::Replay,
            reason: Unavailable::Malformed(DecodeError::MissingCount),
        },
    );
    app.world_mut()
        .resource_mut::<crate::spellbook::SpellNames>()
        .receive(&binding.envelope(Fact::Table(failed)));
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Effects),
            ..
        })
    ));
    assert_ne!(movement_notice(&app), "");
    // Provider replacement and real registration/replay restore the same input
    // values without changing this report owner's admission or revision.
    ready_spell_provider(
        &mut app
            .world_mut()
            .resource_mut::<crate::spellbook::SpellNames>(),
        Binding::new(Policy::Project1999),
    );
    app.update();
    let recovered = receipt(&rx.try_recv().unwrap(), true);
    assert_eq!(
        (recovered.revision, recovered.inputs),
        (original.revision, original.inputs)
    );
    assert_ne!(recovered.report_id, original.report_id);
    assert_ne!(movement_notice(&app), "");
    for accepted in [original, recovered] {
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::SpeedInputsAccepted(accepted)],
        );
        app.update();
        assert_eq!(movement_notice(&app).is_empty(), accepted == recovered);
    }
}

fn receipt(command: &ClientCommand, granted: bool) -> eq_client_core::SpeedInputAcceptance {
    let ClientCommand::SpeedInputs {
        session_id,
        revision,
        report_id,
        state: SpeedInputState::Ready(inputs),
    } = *command
    else {
        panic!("expected the actual ready report")
    };
    eq_client_core::SpeedInputAcceptance {
        session_id,
        revision,
        report_id,
        inputs,
        granted,
    }
}

fn motion_state(speed: Option<f32>) -> WorldEvent {
    WorldEvent::MotionState {
        diagonal: eq_client_core::DiagonalRates::default(),
        session_id: 1,
        units_per_second: speed,
        backward_units_per_second: None,
        walk_units_per_second: None,
        strafe_units_per_second: None,
        falls: true,
    }
}

#[test]
fn only_the_exact_posted_ready_report_with_a_positive_grant_clears_the_wait() {
    let (mut app, rx) = reporting_ready();
    let command = rx.try_recv().unwrap();
    let accepted = receipt(&command, true);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [motion_state(Some(6.0))],
    );
    app.update();
    assert!(
        !movement_notice(&app).is_empty(),
        "cached speed is insufficient"
    );
    let mut different_inputs = accepted.inputs;
    different_inputs.weight += 1;
    for stale in [
        eq_client_core::SpeedInputAcceptance {
            session_id: 2,
            ..accepted
        },
        eq_client_core::SpeedInputAcceptance {
            revision: 2,
            ..accepted
        },
        eq_client_core::SpeedInputAcceptance {
            report_id: accepted.report_id.wrapping_sub(1),
            ..accepted
        },
        eq_client_core::SpeedInputAcceptance {
            inputs: different_inputs,
            ..accepted
        },
        eq_client_core::SpeedInputAcceptance {
            granted: false,
            ..accepted
        },
    ] {
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::SpeedInputsAccepted(stale)],
        );
        app.update();
        assert_ne!(movement_notice(&app), "");
    }
    app.world_mut()
        .resource_mut::<crate::notices::Lines>()
        .status
        .set("Zoning");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::SpeedInputsAccepted(accepted)],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    assert_eq!(
        app.world()
            .resource::<crate::notices::Lines>()
            .status
            .text(),
        "Zoning"
    );
}

#[test]
fn identical_value_recovery_uses_a_fresh_identity_and_rejects_the_old_receipt() {
    let (mut app, rx) = reporting_ready();
    let original = receipt(&rx.try_recv().unwrap(), true);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(original),
        ],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Inventory(InventoryUpdate::Invalidated)],
    );
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(_),
            ..
        })
    ));
    assert_ne!(movement_notice(&app), "");
    ready(&mut app.world_mut().resource_mut::<OnlineState>());
    app.update();
    let recovered = receipt(&rx.try_recv().unwrap(), true);
    assert_eq!(
        (recovered.revision, recovered.inputs),
        (original.revision, original.inputs)
    );
    assert_ne!(recovered.report_id, original.report_id);
    assert!(!movement_notice(&app).is_empty(), "cached receipt is stale");
    for accepted in [original, recovered] {
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::SpeedInputsAccepted(accepted)],
        );
        app.update();
        assert_eq!(movement_notice(&app).is_empty(), accepted == recovered);
    }
}

#[test]
fn pending_drop_keeps_movement_bounded_until_inventory_is_invalidated() {
    use std::num::NonZeroU32;

    let (mut app, rx) = reporting_ready();
    receipt(&rx.try_recv().unwrap(), true);
    let serial = NonZeroU32::new(17).unwrap();
    let mut original = item(InventorySlot::CURSOR.0, 50, 0);
    original.rules.ground_drop_serial = Some(serial);
    let heavier = item(InventorySlot::CURSOR.0, 1_500, 0);
    testing::inventory(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        InventoryUpdate::Snapshot(vec![original.clone()]),
    );
    app.update();
    let ready = receipt(&rx.try_recv().unwrap(), true);
    assert_eq!(ready.inputs.weight, 5);

    {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        let revision = online.world().worker_inventory().revision();
        testing::news(
            &mut online,
            [
                WorldEvent::InventoryAction {
                    session_id: 1,
                    revision,
                    sequence: None,
                    update: Some(InventoryUpdate::GroundDropPending(serial)),
                    error: None,
                },
                WorldEvent::Inventory(InventoryUpdate::Cursor(vec![heavier.clone()])),
            ],
        );
        let world = online.world();
        assert!(!world.worker_inventory().stale());
        assert!(world.worker_inventory().ground_drop_pending());
        assert_eq!(world.worker_inventory().queued().count(), 1);
        assert_eq!(
            world.worker_inventory().items().get(&InventorySlot::CURSOR),
            Some(&original)
        );
        assert_eq!(world.movement_weight(), Ok(1_500));
        assert!((world.fall_weight().unwrap() - 150.0).abs() < f32::EPSILON);
    }
    app.update();
    let bounded = receipt(&rx.try_recv().unwrap(), true);
    assert_eq!(bounded.inputs.weight, 150);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(bounded),
        ],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");

    testing::inventory(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        InventoryUpdate::Remove(InventorySlot::CURSOR),
    );
    app.update();
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Inventory),
            ..
        })
    ));
    assert!(movement_notice(&app).contains("fresh inventory snapshot"));
    let retained = app
        .world()
        .resource::<OnlineState>()
        .world()
        .fall_weight()
        .unwrap();
    assert!((retained - 150.0).abs() < f32::EPSILON);

    testing::inventory(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        InventoryUpdate::Snapshot(vec![heavier]),
    );
    app.update();
    let recovered = receipt(&rx.try_recv().unwrap(), true);
    assert_eq!(recovered.inputs.weight, 150);
    assert_ne!(ready.report_id, recovered.report_id);
    let restored = app
        .world()
        .resource::<OnlineState>()
        .world()
        .fall_weight()
        .unwrap();
    assert!((restored - 150.0).abs() < f32::EPSILON);
    assert_ne!(movement_notice(&app), "");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(recovered),
        ],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
}

#[test]
fn withdrawal_and_new_input_requirements_invalidate_the_previous_acceptance() {
    let (mut app, rx) = reporting_ready();
    let original = receipt(&rx.try_recv().unwrap(), true);
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(original),
        ],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [motion_state(None), motion_state(Some(6.0))],
    );
    app.update();
    assert!(
        !movement_notice(&app).is_empty(),
        "an unacknowledged regrant is insufficient"
    );
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 2,
        }],
    );
    app.update();
    let current = receipt(&rx.try_recv().unwrap(), true);
    assert_ne!(current.report_id, original.report_id);
    for accepted in [original, current] {
        testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [WorldEvent::SpeedInputsAccepted(accepted)],
        );
        app.update();
        assert_eq!(movement_notice(&app).is_empty(), accepted == current);
    }
}

#[test]
fn a_full_queue_retries_one_identity_and_new_states_supersede_it() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let mut app = crate::testing::app();
    app.insert_resource(online())
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, report);
    app.update();
    ready(&mut app.world_mut().resource_mut::<OnlineState>());
    app.update();
    let ClientCommand::SpeedInputs {
        report_id: missing_id,
        ..
    } = rx.try_recv().unwrap()
    else {
        panic!("expected the queued unavailable report")
    };
    app.update();
    let accepted = receipt(&rx.try_recv().unwrap(), true);
    assert_ne!(accepted.report_id, missing_id);
    for _ in 0..3 {
        app.update();
        assert!(rx.try_recv().is_err());
        assert_ne!(movement_notice(&app), "");
    }
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [
            motion_state(Some(6.0)),
            WorldEvent::SpeedInputsAccepted(accepted),
        ],
    );
    app.update();
    assert!(
        movement_notice(&app).is_empty(),
        "the retry retained its identity"
    );
}

#[test]
fn report_identity_wrap_and_admission_changes_do_not_match_the_previous_report() {
    let mut reports = Reports {
        next_id: u64::MAX - 1,
        current: None,
    };
    let state = SpeedInputState::Ready(SpeedInputs {
        rate: 0,
        weight: 0,
        capacity: 90,
        eqmac_fatigue: None,
    });
    let previous_id = reports.prepare(1, 1, state).id;
    assert_eq!(previous_id, u64::MAX);
    reports.prepare(1, 1, SpeedInputState::Unavailable(Missing::Effects));
    let recovered = reports.prepare(1, 1, state);
    assert_ne!(recovered.id, previous_id);
    let recovered_id = recovered.id;
    assert_eq!(reports.prepare(1, 1, state).id, recovered_id);
    let new_admission = reports.prepare(2, 1, state);
    assert_ne!(
        (new_admission.session_id, new_admission.id),
        (1, recovered_id)
    );
    assert!(!new_admission.posted);
}

#[test]
fn readiness_explains_missing_profile_buffs_spells_and_item_details() {
    let run = |state| {
        let mut app = crate::testing::app();
        app.insert_resource(state).add_systems(Update, report);
        app.update();
        movement_notice(&app).to_owned()
    };
    let mut state = online();
    let mut player = testing::player(7);
    player.base_attributes = None;
    testing::admit(&mut state, 1, player);
    ready(&mut state);
    testing::news(
        &mut state,
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 1,
        }],
    );
    assert_eq!(
        run(state),
        "Movement paused: waiting for character information."
    );

    let mut state = online();
    let player = state.world().player().unwrap().clone();
    testing::admit(&mut state, 1, player);
    ready(&mut state);
    testing::news(
        &mut state,
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 1,
        }],
    );
    assert_eq!(run(state), "Movement paused: waiting for buff information.");

    let mut state = online();
    ready(&mut state);
    testing::news(&mut state, [buff(0, 999)]);
    assert_eq!(
        run(state),
        "Movement paused: spell or item effect details are unavailable."
    );

    let mut state = online();
    ready(&mut state);
    let mut food = item(22, 10, 5);
    food.rules.item_type = 14;
    food.details.equipment = None;
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![food]))],
    );
    assert_eq!(run(state), "Movement paused: item details are unavailable.");
}

#[test]
fn unresolved_strength_and_predicted_load_keep_their_actual_refusal() {
    let mut state = online();
    ready(&mut state);
    let mut fields = vec!["0"; 183];
    fields[0] = "42";
    fields[1] = "Synthetic strength spell";
    fields[20] = "5";
    fields[70] = "100";
    fields[86] = "4";
    let names = crate::spellbook::SpellNames::parse(&fields.join("^"));
    testing::news(
        &mut state,
        [WorldEvent::SpellEffect(eq_client_core::SpellEffect {
            spell_id: 42,
            caster_level: 20,
            effect_flag: 4,
            instrument_modifier: 10,
            target_id: 7,
            caster_id: 7,
            spell_level: 1,
            push: None,
        })],
    );
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut app = crate::testing::app();
    app.insert_resource(state)
        .insert_resource(names)
        .insert_resource(Outbox::new(Some(tx)))
        .add_systems(Update, report);
    app.update();
    assert_eq!(
        movement_notice(&app),
        "Movement paused: spell or item effect details are unavailable."
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Effects),
            ..
        })
    ));

    let mut state = online();
    ready(&mut state);
    let mut carried = item(22, 70, 0);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
            carried.clone(),
        ]))],
    );
    carried.slot = InventorySlot(2000);
    testing::news(
        &mut state,
        [WorldEvent::Inventory(InventoryUpdate::Prediction(vec![
            carried,
        ]))],
    );
    app.insert_resource(state);
    app.update();
    assert_eq!(
        movement_notice(&app),
        "Movement paused: waiting for item changes to be confirmed."
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(ClientCommand::SpeedInputs {
            state: SpeedInputState::Unavailable(Missing::Prediction),
            ..
        })
    ));
}

#[test]
fn a_wait_is_admission_scoped_and_hidden_outside_the_connected_world() {
    let mut app = crate::testing::app();
    app.insert_resource(online()).add_systems(Update, report);
    app.update();
    assert_ne!(movement_notice(&app), "");
    app.world_mut().resource_mut::<OnlineState>().enabled = false;
    app.update();
    assert_eq!(movement_notice(&app), "");
    app.world_mut().resource_mut::<OnlineState>().enabled = true;
    app.update();
    assert_ne!(movement_notice(&app), "");

    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        2,
        testing::player(7),
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::SpeedInputsRequired {
            session_id: 1,
            revision: 99,
        }],
    );
    app.update();
    assert_eq!(movement_notice(&app), "");
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::SpeedInputsRequired {
            session_id: 2,
            revision: 1,
        }],
    );
    app.update();
    assert_eq!(
        movement_notice(&app),
        "Movement paused: waiting for inventory information."
    );
    testing::connect(&mut app.world_mut().resource_mut::<OnlineState>(), false);
    app.update();
    assert_eq!(movement_notice(&app), "");
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
            capacity: 105,
            eqmac_fatigue: None,
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
            capacity: 115,
            eqmac_fatigue: None,
        })
    );
    testing::news(&mut state, [buff(2, 2)]);
    assert_eq!(calculate(state.world(), effects).unwrap().rate, -40);
    testing::news(&mut state, [buff(3, 3)]);
    assert_eq!(calculate(state.world(), effects).unwrap().rate, -1000);
    testing::news(
        &mut state,
        [WorldEvent::Buff(BuffUpdate {
            replaces_slot: false,
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
fn eqmac_fatigue_changes_effective_capacity_without_changing_the_posted_basis() {
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
    let basis = calculate(&world, effects).unwrap();
    assert_eq!(basis.capacity, 80);
    assert_eq!(
        basis.eqmac_fatigue,
        Some(EqMacFatigueInputs {
            strength: 90,
            stamina: Some(60)
        })
    );
    assert_eq!(effective_capacity(&world, basis), 80);
    let mut reports = Reports::default();
    let report_id = reports.prepare(1, 1, SpeedInputState::Ready(basis)).id;
    for (fatigue, strength) in [(0, 90), (65, 85), (100, 80), (0, 90)] {
        tell(
            &mut world,
            WorldEvent::Nourishment(Nourishment {
                food: 6000,
                water: 6000,
                fatigue: Some(fatigue),
            }),
        );
        assert_eq!(calculate(&world, effects), Ok(basis));
        assert_eq!(effective_capacity(&world, basis), strength);
        assert_eq!(
            reports.prepare(1, 1, SpeedInputState::Ready(basis)).id,
            report_id
        );
    }
}

#[test]
fn eqmac_host_basis_preserves_signed_strength_and_known_stamina_bounds() {
    for (base_str, bonus_str, base_sta, bonus_sta, strength, stamina, fallback) in [
        (0, -7, 0, 0, -7, 1, 1),
        (0, 0, 0, 0, 0, 1, 1),
        (1, 0, 1, 0, 1, 1, 1),
        (255, 0, 100, 0, 255, 100, 245),
        (260, 0, 100, 0, 260, 100, 250),
        (265, 0, 100, 0, 265, 100, 255),
        (300, 0, 250, 100, 300, 255, 255),
        (90, 0, 5, -100, 90, 1, 80),
    ] {
        let mut state = online();
        let mut player = state.world().player().unwrap().clone();
        let base = player.base_attributes.as_mut().unwrap();
        base.strength = base_str;
        base.stamina = base_sta;
        let mut world = ClientWorld::default();
        world.carry_as(eq_client_core::GameDialect::EqMac);
        testing::set_world(&mut state, world);
        testing::admit(&mut state, 1, player);
        ready(&mut state);
        let mut equipment = item(17, 0, bonus_str);
        equipment.details.bonuses.as_mut().unwrap().stamina = bonus_sta;
        testing::news(
            &mut state,
            [
                WorldEvent::BuffSnapshot(Vec::new()),
                WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![equipment])),
            ],
        );
        let basis = calculate(state.world(), effects).unwrap();
        assert_eq!(
            basis.eqmac_fatigue,
            Some(EqMacFatigueInputs {
                strength,
                stamina: Some(stamina)
            })
        );
        assert_eq!(basis.capacity, fallback);
        assert_ne!(basis.capacity, 0);
        assert_eq!(basis.eqmac_capacity(None), fallback);
        // Known fatigue is applied BEFORE255, independently of the fallback.
        if strength == 260 {
            assert_eq!(basis.eqmac_capacity(Some(110)), 250);
            assert_eq!(basis.eqmac_capacity(Some(105)), 255);
        }
        testing::news(
            &mut state,
            [WorldEvent::Inventory(InventoryUpdate::Invalidated)],
        );
        assert_eq!(calculate(state.world(), effects), Err(Missing::Inventory));
        assert_eq!(state.world().accepted_speed_inputs(), None);
    }
}

fn reported(
    app: &mut App,
    rx: &std::sync::mpsc::Receiver<ClientCommand>,
    revision: u64,
    updates: impl IntoIterator<Item = InventoryUpdate>,
) -> SpeedInputState {
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        updates
            .into_iter()
            .map(WorldEvent::Inventory)
            .chain([WorldEvent::SpeedInputsRequired {
                session_id: 1,
                revision,
            }]),
    );
    app.update();
    let Ok(ClientCommand::SpeedInputs {
        revision: actual,
        state,
        ..
    }) = rx.try_recv()
    else {
        panic!("inventory change must report the current input revision")
    };
    assert_eq!(actual, revision);
    state
}

#[test]
fn scribed_scroll_consumption_reports_known_load_without_hiding_an_unresolved_bank_move() {
    for banked in [false, true] {
        let mut state = online();
        let player = state.world().player().unwrap().clone();
        let mut world = ClientWorld::default();
        world.carry_as(eq_client_core::GameDialect::EqMac);
        testing::set_world(&mut state, world);
        testing::admit(&mut state, 1, player);
        ready(&mut state);
        let mut scroll = item(22, 30, 0);
        scroll.scroll_spell = Some(73);
        let mut other = item(23, 70, 0);
        other.details.id += 1;
        testing::news(
            &mut state,
            [
                WorldEvent::BuffSnapshot(vec![]),
                WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![
                    scroll.clone(),
                    other.clone(),
                ])),
            ],
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut app = crate::testing::app();
        app.insert_resource(state)
            .insert_resource(Outbox::new(Some(tx)))
            .add_systems(Update, report);
        assert!(matches!(
            reported(&mut app, &rx, 1, []),
            SpeedInputState::Ready(SpeedInputs { weight: 10, .. })
        ));
        scroll.slot = InventorySlot::CURSOR;
        assert!(matches!(
            reported(
                &mut app,
                &rx,
                2,
                [InventoryUpdate::Prediction(vec![
                    scroll.clone(),
                    other.clone()
                ])]
            ),
            SpeedInputState::Ready(SpeedInputs { weight: 10, .. })
        ));
        if banked {
            other.slot = InventorySlot(2000);
            testing::news(
                &mut app.world_mut().resource_mut::<OnlineState>(),
                [WorldEvent::Inventory(InventoryUpdate::Prediction(vec![
                    scroll, other,
                ]))],
            );
        }
        // These are the session's normalized events after its matched scribe reply.
        let input = reported(
            &mut app,
            &rx,
            3,
            [InventoryUpdate::Consume {
                slot: InventorySlot::CURSOR,
                charge: false,
            }],
        );
        let state = app.world().resource::<OnlineState>();
        assert!(
            !state
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        assert!(!state.world().inventory().stale());
        assert_eq!(state.world().inventory().predicted(), banked);
        let load = load(
            state.world(),
            app.world().resource::<crate::spellbook::SpellNames>(),
        );
        if banked {
            assert_eq!(input, SpeedInputState::Unavailable(Missing::Prediction));
            assert!(load.is_none());
        } else {
            assert!(matches!(
                input,
                SpeedInputState::Ready(SpeedInputs { weight: 7, .. })
            ));
            assert_eq!(load.map(|(weight, _)| weight), Some(7));
        }
    }
}

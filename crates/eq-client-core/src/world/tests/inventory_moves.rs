//! Queued production moves and ordered worker results, wholly synthetic.
use super::*;
use crate::inventory::{
    InventoryActor, InventoryMove, InventorySlot, InventoryUpdate, MoveQuantity,
};
use crate::outfit::{TextureSlot, WearChange};
use std::num::NonZeroU16;

fn setup() -> ClientWorld {
    let mut world = admitted();
    let mut item = chest();
    item.slot = InventorySlot(22);
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![item])),
    );
    world
}

fn actor() -> InventoryActor {
    InventoryActor {
        bank_access: false,
        deity: None,
        dual_wield: None,
        class: Some(1),
        race: 1,
        level: 60,
        trade_slots: 0,
        trade_no_drop: false,
        world_container: false,
    }
}

fn request(world: &ClientWorld, from: i32, to: i32) -> InventoryMove {
    InventoryMove {
        session_id: world.session_id().unwrap(),
        revision: world.inventory().revision(),
        sequence: None,
        from: InventorySlot(from),
        to: InventorySlot(to),
        quantity: MoveQuantity::Whole,
        created: Instant::now(),
    }
}

fn queue(world: &mut ClientWorld, from: i32, to: i32) -> InventoryMove {
    let request = request(world, from, to);
    let mut sent = None;
    world
        .queue_inventory_move(request, actor(), |_, request| {
            sent = Some(request.clone());
            Ok(())
        })
        .unwrap();
    sent.unwrap()
}

fn result(
    world: &mut ClientWorld,
    request: &InventoryMove,
    update: Option<InventoryUpdate>,
    error: Option<&str>,
) -> Changes {
    game(
        world,
        WorldEvent::InventoryAction {
            session_id: request.session_id,
            revision: request.revision,
            sequence: request.sequence,
            update,
            error: error.map(str::to_owned),
        },
    )
}

fn held_item(slot: InventorySlot, id: u32, model: u16) -> crate::inventory::InventoryItem {
    let mut item = chest();
    item.slot = slot;
    item.rules.item_type = 0; // One-handed slash.
    item.details.id = id;
    item.details.slots = 1 << 13;
    item.details.bonuses = Some(crate::ItemBonuses::default());
    item.details.base_held_model = NonZeroU16::new(model);
    item
}

fn own_primary_wear(world: &mut ClientWorld, material: u32) {
    game(
        world,
        WorldEvent::WearChange(WearChange {
            spawn_id: 9,
            slot: TextureSlot::Primary,
            material,
            tint: None,
        }),
    );
}

fn held_swap(model: u16) -> ClientWorld {
    let mut world = admitted();
    own_primary_wear(&mut world, 37);
    game(
        &mut world,
        inventory(vec![
            held_item(InventorySlot(13), 101, 37),
            held_item(InventorySlot::CURSOR, 102, model),
        ]),
    );
    world
}

fn submit_primary_swap(world: &mut ClientWorld) {
    let request = queue(world, 30, 13);
    assert!(world.held_model_estimate(TextureSlot::Primary).is_none());
    let prediction = world
        .worker_inventory()
        .plan_move(&request, actor())
        .unwrap();
    result(world, &request, Some(prediction), None);
}

#[test]
fn same_zone_held_swap_keeps_submitted_provenance_through_silence() {
    use HeldModelEstimate::{ReceivedInventory, Submitted};
    let mut world = held_swap(38);
    submit_primary_swap(&mut world);
    let primary = |world: &ClientWorld| world.held_model_estimate(TextureSlot::Primary);
    assert_eq!(primary(&world), Some(Submitted(38)));
    game(&mut world, WorldEvent::Inventory(InventoryUpdate::Settled));
    assert!(!world.worker_inventory().predicted());
    assert_eq!(primary(&world), Some(Submitted(38)));
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Set(vec![held_item(
            InventorySlot::CURSOR,
            101,
            37,
        )])),
    );
    assert_eq!(primary(&world), Some(Submitted(38)));
    assert_eq!(
        world
            .player()
            .unwrap()
            .appearance
            .material(TextureSlot::Primary),
        37
    );

    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Set(vec![held_item(
            InventorySlot(13),
            102,
            38,
        )])),
    );
    assert_eq!(primary(&world), Some(ReceivedInventory(38)));
    own_primary_wear(&mut world, 93);
    assert_eq!(primary(&world), None);
    // A later slot echo cannot replace the received appearance with a base model.
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Set(vec![held_item(
            InventorySlot(13),
            102,
            38,
        )])),
    );
    assert_eq!(primary(&world), None);
    assert_eq!(
        world
            .player()
            .unwrap()
            .appearance
            .material(TextureSlot::Primary),
        93
    );
}

#[test]
fn refused_or_corrected_held_swaps_leave_received_wear_in_charge() {
    let primary = |world: &ClientWorld| world.held_model_estimate(TextureSlot::Primary);
    let mut refused = held_swap(38);
    let request = queue(&mut refused, 30, 13);
    result(&mut refused, &request, None, Some("Refused locally"));
    assert_eq!(primary(&refused), None);

    for settled in [false, true] {
        let mut corrected = held_swap(38);
        submit_primary_swap(&mut corrected);
        if settled {
            game(
                &mut corrected,
                WorldEvent::Inventory(InventoryUpdate::Settled),
            );
        }
        game(
            &mut corrected,
            WorldEvent::Inventory(InventoryUpdate::Set(vec![held_item(
                InventorySlot(13),
                101,
                37,
            )])),
        );
        assert_eq!(primary(&corrected), None);
        assert_eq!(
            corrected.worker_inventory().items()[&InventorySlot(13)]
                .details
                .id,
            101
        );
        assert_eq!(
            corrected
                .player()
                .unwrap()
                .appearance
                .material(TextureSlot::Primary),
            37
        );
    }
}

#[test]
fn a_new_admission_or_unknown_base_model_uses_received_appearance() {
    let mut world = held_swap(38);
    submit_primary_swap(&mut world);
    let mut admission = entered(2);
    let WorldEvent::Entered { player, .. } = &mut admission else {
        unreachable!();
    };
    player.appearance.materials[TextureSlot::Primary.index()] = 44;
    game(&mut world, admission);
    connection(&mut world, true, false);
    assert!(world.held_model_estimate(TextureSlot::Primary).is_none());
    assert_eq!(
        world
            .player()
            .unwrap()
            .appearance
            .material(TextureSlot::Primary),
        44
    );

    let mut unknown = held_swap(0);
    submit_primary_swap(&mut unknown);
    assert!(unknown.held_model_estimate(TextureSlot::Primary).is_none());
    assert_eq!(
        unknown.worker_inventory().items()[&InventorySlot(13)]
            .details
            .id,
        102
    );
}

#[test]
fn destroy_submission_result_applies_once_and_cannot_remove_a_newer_cursor() {
    for newer in [false, true] {
        let mut world = setup();
        let mut item = world.inventory().items()[&InventorySlot(22)].clone();
        item.slot = InventorySlot::CURSOR;
        item.bag_slots = 0;
        game(
            &mut world,
            WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![item.clone()])),
        );
        world.plan_inventory_as(Some(crate::inventory::MoveRules::EqEmu));
        let request = world.prepare_cursor_destruction().unwrap();
        if newer {
            item.details.id += 1;
            game(
                &mut world,
                WorldEvent::Inventory(InventoryUpdate::Set(vec![item.clone()])),
            );
        }
        let submitted = WorldEvent::InventoryAction {
            session_id: request.session_id,
            revision: request.revision,
            sequence: None,
            update: Some(InventoryUpdate::DestroyCursor),
            error: None,
        };
        game(&mut world, submitted.clone());
        if newer {
            assert_eq!(world.inventory().items()[&InventorySlot::CURSOR], item);
        } else {
            assert!(
                !world
                    .inventory()
                    .items()
                    .contains_key(&InventorySlot::CURSOR)
            );
            assert!(world.inventory().predicted());
        }
        let after = world.inventory().clone();
        game(&mut world, submitted);
        assert_eq!(world.inventory(), &after);
    }
}

#[test]
fn successive_moves_project_before_replies_and_results_never_apply_twice() {
    let mut world = setup();
    let mut worker = world.inventory().clone();
    let first = queue(&mut world, 22, 30);
    let second = queue(&mut world, 30, 23);
    let projected = world.inventory().clone();
    assert_eq!(
        second.sequence.unwrap().after,
        Some(first.sequence.unwrap().id)
    );
    assert!(world.inventory_moves_pending());
    assert!(projected.predicted());
    for request in [&first, &second] {
        let update = worker.plan_move(request, actor()).unwrap();
        worker.apply(update.clone());
        let changes = result(&mut world, request, Some(update.clone()), None);
        assert!(!changes.ignored);
        assert_eq!(world.inventory(), &projected);
        assert!(result(&mut world, request, Some(update), None).ignored);
        assert_eq!(world.inventory(), &projected);
    }
    assert!(!world.inventory_moves_pending());
    assert_eq!(world.inventory(), &worker);
    assert_eq!(world.inventory().unresolved_predictions(), 2);
}

#[test]
fn failed_enqueue_keeps_snapshot_and_rejection_drains_descendants_without_replay() {
    let mut world = setup();
    let original = world.inventory().clone();
    let pick = request(&world, 22, 30);
    assert!(
        world
            .queue_inventory_move(pick, actor(), |_, _| anyhow::bail!("Full queue"))
            .is_err()
    );
    assert_eq!(world.inventory(), &original);
    assert!(!world.inventory_moves_pending());
    let first = queue(&mut world, 22, 30);
    let second = queue(&mut world, 30, 23);
    result(&mut world, &first, None, Some("Refused locally"));
    assert_eq!(world.inventory(), &original);
    let fresh = request(&world, 22, 30);
    assert!(
        world
            .queue_inventory_move(fresh, actor(), |_, _| panic!(
                "must not enqueue while draining"
            ))
            .is_err()
    );
    result(&mut world, &second, None, Some("Predecessor refused"));
    assert!(!world.inventory_moves_pending());
    let recovered = queue(&mut world, 22, 30);
    assert!(recovered.sequence.unwrap().id > second.sequence.unwrap().id);
    assert_eq!(recovered.sequence.unwrap().after, None);
}

#[test]
fn authoritative_changes_discard_overlay_and_snapshot_recovers_after_results_drain() {
    let mut world = setup();
    let snapshot = world
        .inventory()
        .items()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let first = queue(&mut world, 22, 30);
    let second = queue(&mut world, 30, 23);
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Invalidated),
    );
    assert!(world.inventory().stale());
    assert!(world.inventory().items().contains_key(&InventorySlot(22)));
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Snapshot(snapshot)),
    );
    let fresh = request(&world, 22, 30);
    assert!(
        world
            .queue_inventory_move(fresh, actor(), |_, _| panic!("drain first"))
            .is_err()
    );
    result(&mut world, &first, None, Some("Revision changed"));
    result(&mut world, &second, None, Some("Predecessor refused"));
    queue(&mut world, 22, 30);
}

#[test]
fn new_admission_discards_pending_projection_and_ignores_older_results() {
    let mut world = setup();
    let old = queue(&mut world, 22, 30);
    game(&mut world, entered(2));
    assert!(!world.inventory_moves_pending());
    assert!(result(&mut world, &old, None, Some("Old admission")).ignored);
    assert!(!world.inventory().received());
}

#[test]
fn stale_selection_and_out_of_order_result_cannot_change_the_projection() {
    let mut world = setup();
    let stale = request(&world, 22, 30);
    let mut worker = world.inventory().clone();
    let first = queue(&mut world, 22, 30);
    let picked = world.inventory().clone();
    assert!(
        world
            .queue_inventory_move(stale, actor(), |_, _| panic!(
                "stale intent must not enqueue"
            ))
            .is_err()
    );
    assert_eq!(world.inventory(), &picked);
    let first_update = worker.plan_move(&first, actor()).unwrap();
    worker.apply(first_update.clone());
    let second = queue(&mut world, 30, 23);
    let second_update = worker.plan_move(&second, actor()).unwrap();
    let projected = world.inventory().clone();
    assert!(result(&mut world, &second, Some(second_update.clone()), None).ignored);
    assert_eq!(world.inventory(), &projected);
    result(&mut world, &first, Some(first_update), None);
    result(&mut world, &second, Some(second_update), None);
    assert_eq!(world.inventory(), &projected);
    assert!(!world.inventory_moves_pending());
}

#[test]
fn refused_tail_preserves_the_submitted_prefix_as_uncertain() {
    let mut world = setup();
    let mut worker = world.inventory().clone();
    let first = queue(&mut world, 22, 30);
    let second = queue(&mut world, 30, 23);
    let update = worker.plan_move(&first, actor()).unwrap();
    worker.apply(update.clone());
    result(&mut world, &first, Some(update), None);
    result(&mut world, &second, None, Some("Equipment access changed"));
    assert_eq!(world.inventory(), &worker);
    assert!(world.inventory().predicted());
    assert_eq!(world.inventory().unresolved_predictions(), 1);
    let fresh = queue(&mut world, 30, 24);
    assert_eq!(fresh.sequence.unwrap().after, None);
}

#[test]
fn host_planning_uses_selected_rules_and_never_falls_back_when_policy_is_missing() {
    use crate::inventory::MoveRules;
    for rules in [
        None,
        Some(MoveRules::EqEmu),
        Some(MoveRules::Takp),
        Some(MoveRules::Quarm),
    ] {
        let mut world = setup();
        let mut item = world.inventory().items()[&InventorySlot(22)].clone();
        item.slot = InventorySlot::CURSOR;
        item.details.slots = u32::MAX;
        item.details.classes = u32::MAX;
        item.details.races = u32::MAX;
        game(
            &mut world,
            WorldEvent::Inventory(InventoryUpdate::Snapshot(vec![item])),
        );
        world.plan_inventory_as(rules);
        let before = world.inventory().clone();
        let request = request(&world, 30, 0);
        let mut called = false;
        let result = world.queue_inventory_move(request, actor(), |_, _| {
            called = true;
            Ok(())
        });
        let valid = rules == Some(MoveRules::EqEmu);
        assert_eq!(result.is_ok(), valid, "{rules:?}: {result:?}");
        assert_eq!(called, valid);
        if !valid {
            assert_eq!(world.inventory(), &before);
            assert!(!world.inventory_moves_pending());
        }
    }
}

#[test]
fn same_admission_transfer_reset_reconciles_a_late_success_once_before_recovery() {
    let mut world = setup();
    let mut worker = world.worker_inventory().clone();
    let first = queue(&mut world, 22, 30);
    let update = worker.plan_move(&first, actor()).unwrap();
    worker.apply(update.clone());
    game(
        &mut world,
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
    );
    assert!(!world.in_world());
    assert!(world.inventory_moves_pending());
    assert!(world.inventory().items().contains_key(&InventorySlot(22)));
    assert!(!result(&mut world, &first, Some(update.clone()), None).ignored);
    assert_eq!(world.worker_inventory(), &worker);
    assert!(!world.inventory_moves_pending());
    assert!(result(&mut world, &first, Some(update), None).ignored);
    assert_eq!(world.accepted_speed_inputs(), None);
    game(
        &mut world,
        WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        },
    );
    assert!(world.in_world());
    let fresh = queue(&mut world, 30, 23);
    assert_eq!(fresh.sequence.unwrap().after, None);
    assert!(fresh.sequence.unwrap().id > first.sequence.unwrap().id);
}

#[test]
fn retained_resets_drain_same_admission_results_but_new_admissions_discard_them() {
    for reset in [
        Reset::Died,
        Reset::Lost {
            ended: false,
            transferring: false,
        },
    ] {
        let mut world = setup();
        let mut worker = world.worker_inventory().clone();
        let first = queue(&mut world, 22, 30);
        let update = worker.plan_move(&first, actor()).unwrap();
        worker.apply(update.clone());
        if matches!(reset, Reset::Lost { .. }) {
            connection(&mut world, false, false);
            assert!(!world.accepts_reply(first.session_id));
        } else {
            game(
                &mut world,
                WorldEvent::Death(crate::Death {
                    spawn_id: 9,
                    killer_id: 5,
                    corpse_id: 0,
                    bind_zone_id: 0,
                    corpse_name: None,
                }),
            );
        }
        assert!(world.inventory_moves_pending());
        assert!(!result(&mut world, &first, Some(update), None).ignored);
        assert_eq!(world.worker_inventory(), &worker);
        assert_eq!(world.accepted_speed_inputs(), None);
    }
    let mut world = setup();
    let worker = world.worker_inventory().clone();
    let first = queue(&mut world, 22, 30);
    let update = worker.plan_move(&first, actor()).unwrap();
    game(&mut world, entered(2));
    assert!(result(&mut world, &first, Some(update), None).ignored);
    assert!(!world.worker_inventory().received());
}

#[test]
fn newer_authoritative_state_cannot_be_overwritten_by_a_tracked_old_success() {
    let mut world = setup();
    let worker = world.worker_inventory().clone();
    let first = queue(&mut world, 22, 30);
    let update = worker.plan_move(&first, actor()).unwrap();
    world.reset(Reset::Zoning { to_bind: false });
    let snapshot = worker.items().values().cloned().collect();
    game(
        &mut world,
        WorldEvent::Inventory(InventoryUpdate::Snapshot(snapshot)),
    );
    let authoritative = world.worker_inventory().clone();
    result(&mut world, &first, Some(update.clone()), None);
    assert_eq!(world.worker_inventory(), &authoritative);
    assert!(!world.inventory_moves_pending());
    assert!(result(&mut world, &first, Some(update), None).ignored);
    assert_eq!(world.accepted_speed_inputs(), None);
}

#[test]
fn dependent_refusals_keep_typed_errors_but_present_only_the_root() {
    let mut world = setup();
    let requests = [
        queue(&mut world, 22, 30),
        queue(&mut world, 30, 23),
        queue(&mut world, 23, 30),
    ];
    for (index, request) in requests.iter().enumerate() {
        let reason = if index == 0 {
            "Access changed"
        } else {
            "Predecessor refused"
        };
        let changes = result(&mut world, request, None, Some(reason));
        assert!(matches!(changes.replies.as_slice(), [Reply::InventoryMove {
            error: Some(error), present_error, ..
        }] if error == reason && *present_error == (index == 0)));
    }
    assert!(!world.inventory_moves_pending());
    let fresh = queue(&mut world, 22, 30);
    let changes = result(&mut world, &fresh, None, Some("Different root"));
    assert!(matches!(
        changes.replies.as_slice(),
        [Reply::InventoryMove {
            present_error: true,
            ..
        }]
    ));
}

#[test]
fn ambiguous_or_empty_result_payloads_do_not_fabricate_a_submission() {
    let mut world = setup();
    let worker = world.worker_inventory().clone();
    let first = queue(&mut world, 22, 30);
    let update = worker.plan_move(&first, actor()).unwrap();
    assert!(result(&mut world, &first, None, None).ignored);
    assert!(
        result(
            &mut world,
            &first,
            Some(update.clone()),
            Some("Both present")
        )
        .ignored
    );
    assert!(world.inventory_moves_pending());
    assert_eq!(world.worker_inventory(), &worker);
    assert!(!result(&mut world, &first, Some(update), None).ignored);
    assert!(!world.inventory_moves_pending());
}

#[test]
fn a_terminal_connection_end_discards_even_a_matching_late_success() {
    let mut world = setup();
    let worker = world.worker_inventory().clone();
    let first = queue(&mut world, 22, 30);
    let update = worker.plan_move(&first, actor()).unwrap();
    connection(&mut world, false, true);
    assert!(!world.inventory_moves_pending());
    assert!(result(&mut world, &first, Some(update), None).ignored);
    assert!(!world.worker_inventory().received());
    assert_eq!(world.accepted_speed_inputs(), None);
}

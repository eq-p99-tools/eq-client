//! Queued production moves and ordered worker results, wholly synthetic.
use super::*;
use crate::inventory::{
    InventoryActor, InventoryMove, InventorySlot, InventoryUpdate, MoveQuantity,
};

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

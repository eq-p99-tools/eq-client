use super::*;
use eq_network_game::movement::Carriers;

#[test]
fn carrier_receipt_clock_survives_frontend_delay_and_replacement_clears_before_the_next_event() {
    let mut world = admitted();
    let now = Instant::now();
    let mut boat = spawn(8);
    boat.race = 72;
    boat.velocity = [10.0, 0.0, 0.0];
    let spawned = WorldEvent::Spawns(vec![boat]);
    let mut session = Carriers::default();
    let updates = session.observe(&spawned, crate::GameDialect::Titanium, now);
    game(&mut world, spawned);
    world.apply(
        &WorldUpdate::Game(WorldEvent::Carriers(updates)),
        now + std::time::Duration::from_secs(1),
        &NoSpells,
    );
    let received = world.carriers().latest(8).unwrap();
    assert_eq!(received.sample().reported, now);
    assert_eq!(
        received.pose(now + std::time::Duration::from_secs(1)),
        session
            .latest(8)
            .unwrap()
            .pose(now + std::time::Duration::from_secs(1))
    );
    // The ordered pair may straddle frontend frame batches. Replacement
    // alone must already retire the old actor's collision/carry identity.
    game(&mut world, WorldEvent::Spawns(vec![spawn(8)]));
    assert!(world.carriers().latest(8).is_none());
}

#[test]
fn despawn_and_a_new_admission_forget_carriers() {
    let mut world = admitted();
    let now = Instant::now();
    let mut boat = spawn(8);
    boat.race = 73;
    let mut session = Carriers::default();
    let updates = session.observe(
        &WorldEvent::Spawns(vec![boat]),
        crate::GameDialect::Titanium,
        now,
    );
    game(&mut world, WorldEvent::Carriers(updates.clone()));
    assert!(world.carriers().latest(8).is_some());
    game(&mut world, WorldEvent::Despawn(8));
    assert!(world.carriers().latest(8).is_none());
    game(&mut world, WorldEvent::Carriers(updates));
    game(&mut world, entered(2));
    assert!(world.carriers().latest(8).is_none());
}

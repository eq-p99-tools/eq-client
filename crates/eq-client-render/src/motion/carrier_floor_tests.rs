use super::*;
use eq_client_core::{
    GameDialect, SpawnKind, WorldEvent, WorldPosition,
    movement::{Carriers, CollisionInstance, CollisionMesh},
};
use std::sync::Arc;

#[test]
fn controller_floor_records_the_deep_sweep_at_carried_feet_and_clears_on_reset() {
    let now = Instant::now();
    let player = WorldPosition {
        z: 5.0,
        ..default()
    };
    let mut online = online::OnlineState::new(true);
    let mut admitted = online::testing::player(4);
    admitted.position = player;
    online::testing::admit(&mut online, 11, admitted);
    let mut ship = online::testing::pet(8, 4);
    ship.race = 72;
    ship.gender = 2;
    ship.position = WorldPosition::default();
    let spawned = WorldEvent::Spawns(vec![ship]);
    let mut carriers = Carriers::default();
    let initial = carriers.observe(&spawned, GameDialect::Titanium, now);
    let accepted = boats::Riding {
        sample: carriers.latest(8).unwrap().sample(),
        pose: WorldPosition::default(),
        position: player,
    };
    let pose = world_position([100.0, 0.0, 0.0], 0.0);
    let moved = carriers.observe(
        &WorldEvent::Position {
            spawn_id: 8,
            position: pose,
            velocity: [0.0; 3],
            forward: 0.0,
        },
        GameDialect::Titanium,
        now,
    );
    online::testing::news(
        &mut online,
        [
            spawned,
            WorldEvent::Carriers(initial),
            WorldEvent::Carriers(moved),
        ],
    );
    let mut controls = Controls {
        boat: Some(accepted),
        ..default()
    };
    let rider = controls
        .boat
        .unwrap()
        .carried(online.world().carriers(), now)
        .unwrap();
    let feet = Vec3::from_array(eq_client_core::render_position(rider.position)) - Vec3::Y * 3.0;
    let deck = Arc::new(
        CollisionMesh::new([
            [[-2.0, 0.0, -2.0], [2.0, 0.0, 2.0], [2.0, 0.0, -2.0]],
            [[-2.0, 0.0, -2.0], [-2.0, 0.0, 2.0], [2.0, 0.0, 2.0]],
        ])
        .unwrap(),
    );
    let mut collision = CollisionWorld::new([]).unwrap();
    collision.set_spawn_instances(vec![(
        8,
        CollisionInstance::new(deck, Vec3::X * 100.0, Quat::IDENTITY)
            .unwrap()
            .with_spawn_kind(SpawnKind::Npc),
    )]);
    let session_feet = Vec3::from_array(eq_client_core::render_position(player)) - Vec3::Y * 3.0;
    assert!(collision.ground_spawn(session_feet, 0.1, 0.1).is_none());
    assert!(collision.ground_spawn(feet, 0.1, 0.1).is_none());
    let support = controls.probe_carrier_floor(&collision, online.world(), feet, Some(8), now);
    assert_eq!(support.unwrap().0.spawn_id, 8);
    let query = controls.carrier_floor().unwrap();
    assert_eq!(query.admission, 11);
    assert_eq!(query.queried, now);
    assert_eq!(query.feet, feet);
    assert_eq!(query.current, Some(8));
    assert_eq!(query.support.unwrap().1, pose);
    // Observing support does not settle a proposal or replace the accepted link.
    assert_eq!(controls.boat.unwrap().pose, accepted.pose);
    assert!(controls.boat_pending.is_none());
    assert!(
        controls
            .probe_carrier_floor(
                &collision,
                online.world(),
                feet + Vec3::X * 10.0,
                Some(8),
                now
            )
            .is_none()
    );
    assert!(controls.carrier_floor().unwrap().support.is_none());
    controls.reset(None);
    assert!(controls.carrier_floor().is_none());
}

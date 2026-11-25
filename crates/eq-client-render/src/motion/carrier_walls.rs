use super::*;
use eq_client_core::{
    SpawnKind,
    movement::{CollisionInstance, CollisionMesh},
};
use std::sync::Arc;

#[test]
fn carrier_walls_are_filtered_only_for_titanium_dry_walking() {
    let floor = [
        [[-10.0, 0.0, -10.0], [10.0, 0.0, 10.0], [10.0, 0.0, -10.0]],
        [[-10.0, 0.0, -10.0], [-10.0, 0.0, 10.0], [10.0, 0.0, 10.0]],
    ];
    let mut world = CollisionWorld::new(floor).unwrap();
    let wall = CollisionMesh::new([
        [[0.0, 0.0, -8.0], [0.0, 12.0, -8.0], [0.0, 12.0, 8.0]],
        [[0.0, 0.0, -8.0], [0.0, 12.0, 8.0], [0.0, 0.0, 8.0]],
    ])
    .unwrap();
    world.set_spawn_instances(vec![(
        8,
        CollisionInstance::new(Arc::new(wall), Vec3::ZERO, Quat::IDENTITY)
            .unwrap()
            .with_spawn_kind(SpawnKind::Npc),
    )]);
    for client in [
        InstalledClient::Titanium,
        InstalledClient::EqMac,
        InstalledClient::Rof2,
    ] {
        for (flying, swimming) in [(false, false), (true, false), (false, true)] {
            let view = carrier_walking_world(&world, client, flying, swimming);
            let next = view
                .as_ref()
                .unwrap_or(&world)
                .step(-Vec3::X * 3.0, Vec3::X * 6.0, 3.0);
            let crosses = match client {
                InstalledClient::Titanium => !flying && !swimming,
                InstalledClient::EqMac | InstalledClient::Rof2 => false,
            };
            assert_eq!(
                next.x > 2.99,
                crosses,
                "{client:?}, fly={flying}, swim={swimming}: {next:?}"
            );
        }
    }
}

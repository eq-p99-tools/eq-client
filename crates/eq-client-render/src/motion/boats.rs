//! The rider's floor-sweep link to a received carrier.
use bevy::prelude::*;
use eq_client_core::{WorldPosition, movement::CollisionWorld};
use eq_network_game::movement::{
    CARRIER_REACH, CarrierSample, Carriers, carrier_local, carrier_world,
};
use std::time::Instant;

/// The frame and world point used by the last locally submitted sample.
#[derive(Clone, Copy, Debug)]
pub(super) struct Riding {
    pub sample: CarrierSample,
    pub pose: WorldPosition,
    pub position: WorldPosition,
}

impl Riding {
    /// Applies the carrier's motion since the previous pass before the
    /// rider's own step, including yaw. Replaced actors never inherit riders.
    pub(super) fn carried(self, carriers: &Carriers, now: Instant) -> Option<Self> {
        let report = carriers.latest(self.sample.spawn_id)?;
        let sample = report.sample();
        if sample.incarnation != self.sample.incarnation {
            return None;
        }
        let local = carrier_local(self.pose, self.position);
        if local.x.hypot(local.y).hypot(local.z) > CARRIER_REACH {
            return None;
        }
        let pose = report.pose(now);
        Some(Self {
            sample,
            pose,
            position: carrier_world(pose, local),
        })
    }
}

/// Which boat frame the accepted display queue uses. Its pose is updated
/// after drawing, so detaching can put the whole queue back in world space.
#[derive(Clone, Copy, Debug)]
pub(super) struct DisplayFrame {
    pub sample: CarrierSample,
    pub pose: WorldPosition,
}

fn position(transform: Transform) -> WorldPosition {
    let forward = transform.rotation * Vec3::Z;
    let heading = eq_client_core::world_heading(forward.x.atan2(forward.z));
    eq_client_core::world_position(transform.translation.to_array(), heading)
}

pub(super) fn to_local(pose: WorldPosition, transform: Transform) -> Transform {
    let local = carrier_local(pose, position(transform));
    Transform {
        translation: Vec3::from_array(eq_client_core::render_position(local)),
        rotation: Quat::from_rotation_y(eq_client_core::render_heading(local.heading)),
        ..transform
    }
}

pub(super) fn to_world(pose: WorldPosition, transform: Transform) -> Transform {
    let world = carrier_world(pose, position(transform));
    Transform {
        translation: Vec3::from_array(eq_client_core::render_position(world)),
        rotation: Quat::from_rotation_y(eq_client_core::render_heading(world.heading)),
        ..transform
    }
}

/// Refreshes the carrier link on each floor probe. A swept landing on its
/// model links the rider; walking off, despawn or unavailable geometry unlinks.
pub(super) fn under(
    world: &CollisionWorld,
    carriers: &Carriers,
    feet: Vec3,
    now: Instant,
) -> Option<(CarrierSample, WorldPosition)> {
    let id = world.ground_spawn(feet, 0.01, 0.05)?;
    let report = carriers.latest(id)?;
    let pose = report.pose(now);
    let point = eq_client_core::world_position(feet.to_array(), pose.heading);
    let local = carrier_local(pose, point);
    (local.x.hypot(local.y).hypot(local.z) <= CARRIER_REACH).then_some((report.sample(), pose))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{GameDialect, SpawnKind, SpawnState, WorldEvent};
    fn carrier(id: u16, position: WorldPosition) -> SpawnState {
        // Test fixture construction stays in the network's public record
        // shape, without any installed asset or route data.
        SpawnState {
            spawn_id: id,
            race: 72,
            kind: SpawnKind::Npc,
            position,
            class: None,
            name: String::new(),
            gender: 2,
            velocity: [0.0; 3],
            forward: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            size: 1.0,
            invisible: false,
            appearance: Default::default(),
            level: 1,
            listing: Default::default(),
            pet_owner: None,
            hp_percent: None,
            posture: None,
            name_parts: Default::default(),
        }
    }
    #[test]
    fn a_received_turn_carries_position_and_heading_and_despawn_ends_the_link() {
        let start = Instant::now();
        let origin = WorldPosition::default();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, origin)]),
            GameDialect::Titanium,
            start,
        );
        let riding = Riding {
            sample: carriers.latest(8).unwrap().sample(),
            pose: origin,
            position: WorldPosition {
                x: 20.0,
                y: 0.0,
                z: 5.0,
                heading: 500.0,
            },
        };
        let later = start + std::time::Duration::from_secs(1);
        carriers.observe(
            &WorldEvent::Position {
                spawn_id: 8,
                position: WorldPosition {
                    x: 100.0,
                    y: 30.0,
                    z: 0.0,
                    heading: 128.0,
                },
                velocity: [0.0; 3],
                forward: 0.0,
            },
            GameDialect::Titanium,
            later,
        );
        let carried = riding.carried(&carriers, later).unwrap();
        assert!((carried.position.x - 100.0).abs() < 0.001);
        assert!((carried.position.y - 10.0).abs() < 0.001);
        assert_eq!(carried.position.heading, 116.0);
        carriers.observe(&WorldEvent::Despawn(8), GameDialect::Titanium, later);
        assert!(carried.carried(&carriers, later).is_none());
    }
    #[test]
    fn only_the_floor_under_the_feet_keeps_the_link() {
        use eq_client_core::movement::CollisionMesh;
        let start = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, WorldPosition::default())]),
            GameDialect::Titanium,
            start,
        );
        let deck = [[[-10.0, 5.0, -10.0], [10.0, 5.0, -10.0], [0.0, 5.0, 10.0]]];
        let mut collision = CollisionWorld::new([[
            [-100.0, -10.0, -100.0],
            [100.0, -10.0, -100.0],
            [0.0, -10.0, 100.0],
        ]])
        .unwrap();
        collision.set_spawn_obstacles(vec![(
            8,
            std::sync::Arc::new(CollisionMesh::new(deck).unwrap()),
        )]);
        assert_eq!(
            under(&collision, &carriers, Vec3::new(0.0, 5.0, 0.0), start)
                .unwrap()
                .0
                .spawn_id,
            8
        );
        assert!(under(&collision, &carriers, Vec3::new(30.0, 5.0, 0.0), start).is_none());
        collision.set_spawn_obstacles(Vec::new());
        assert!(under(&collision, &carriers, Vec3::new(0.0, 5.0, 0.0), start).is_none());
        carriers.clear();
        assert!(carriers.latest(8).is_none());
    }
    #[test]
    fn a_rejected_or_unconfirmed_endpoint_never_replaces_the_accepted_rider() {
        let now = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, WorldPosition::default())]),
            GameDialect::Titanium,
            now,
        );
        let sample = carriers.latest(8).unwrap().sample();
        let accepted = Riding {
            sample,
            pose: WorldPosition::default(),
            position: WorldPosition {
                x: 20.0,
                ..WorldPosition::default()
            },
        };
        let proposed = Riding {
            position: WorldPosition {
                x: 50.0,
                ..accepted.position
            },
            ..accepted
        };
        let mut controls = super::super::Controls {
            boat: Some(accepted),
            boat_pending: Some((now, Some(proposed))),
            ..Default::default()
        };
        controls.wait_for(Some(11), now, super::super::PendingKind::Move);
        assert_eq!(controls.boat.unwrap().position, accepted.position);
        assert_eq!(
            controls.liquid_basis(Some(11), now + std::time::Duration::from_secs(3)),
            super::super::LiquidBasis::Unconfirmed
        );
        assert_eq!(controls.boat.unwrap().position, accepted.position);
        controls.move_refused(Some(11), now, "rejected");
        assert_eq!(controls.boat.unwrap().position, accepted.position);
        assert!(controls.boat_pending.is_none());
        let next = now + std::time::Duration::from_secs(4);
        controls.boat_pending = Some((next, Some(proposed)));
        controls.wait_for(Some(11), next, super::super::PendingKind::Move);
        assert!(controls.moved(Some(11), next));
        controls.resolve_carrier(next, false);
        assert_eq!(controls.boat.unwrap().position, proposed.position);
    }

    #[test]
    fn accepted_walking_interpolates_in_the_boat_frame_before_world_carry() {
        let now = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, WorldPosition::default())]),
            GameDialect::Titanium,
            now,
        );
        let riding = Riding {
            sample: carriers.latest(8).unwrap().sample(),
            pose: WorldPosition::default(),
            position: WorldPosition {
                x: 22.0,
                ..WorldPosition::default()
            },
        };
        let mut controls = super::super::Controls {
            boat: Some(riding),
            sent_span: 0.1,
            ..Default::default()
        };
        let from = Transform::from_translation(Vec3::from_array(eq_client_core::render_position(
            WorldPosition {
                x: 20.0,
                ..WorldPosition::default()
            },
        )));
        controls.display_carrier_sample(from, riding.position, &carriers);
        let mut shown = Transform::default();
        controls.visual.as_mut().unwrap().advance(0.05, &mut shown);
        let boat = WorldPosition {
            x: 100.0,
            heading: 128.0,
            ..WorldPosition::default()
        };
        let shown = to_world(boat, shown);
        let point = position(shown);
        assert!((point.x - 100.0).abs() < 0.001);
        assert!((point.y + 21.0).abs() < 0.001);
    }

    #[test]
    fn a_footprint_edge_keeps_the_same_carrier_floor_as_grounded_physics() {
        let now = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, WorldPosition::default())]),
            GameDialect::Titanium,
            now,
        );
        let mut collision =
            CollisionWorld::new([[[-10.0, 4.98, -10.0], [10.0, 4.98, -10.0], [0.0, 4.98, 10.0]]])
                .unwrap();
        let deck = [
            [[-5.0, 5.0, -5.0], [0.0, 5.0, -5.0], [0.0, 5.0, 5.0]],
            [[-5.0, 5.0, -5.0], [0.0, 5.0, 5.0], [-5.0, 5.0, 5.0]],
        ];
        collision.set_spawn_obstacles(vec![(
            8,
            std::sync::Arc::new(eq_client_core::movement::CollisionMesh::new(deck).unwrap()),
        )]);
        let feet = Vec3::new(0.2, 5.0, 0.0);
        assert!((collision.support(feet).unwrap() - 5.0).abs() < 0.001);
        assert_eq!(
            under(&collision, &carriers, feet, now).unwrap().0.spawn_id,
            8
        );
        assert!(under(&collision, &carriers, Vec3::new(0.5, 5.0, 0.0), now).is_none());
    }
}

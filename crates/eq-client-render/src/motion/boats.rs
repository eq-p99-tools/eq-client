//! The rider's floor-sweep link to a received carrier.
use bevy::prelude::*;
use eq_client_core::movement::{
    CARRIER_REACH, CarrierSample, Carriers, carrier_local, carrier_world,
};
use eq_client_core::{WorldPosition, movement::CollisionWorld};
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

/// Where the program's floor sweep ends, its target z of -32767
/// (0x48a0fc's 0xc6fffe00, read R49-27 (d)).
const SWEEP_END: f32 = -32767.0;

/// How far below `feet` the floor sweep looks for the first floor: down to
/// [`SWEEP_END`].
pub(crate) fn sweep_depth(feet: Vec3) -> f32 {
    (feet.y - SWEEP_END).max(0.05)
}

/// Refreshes the carrier link on each floor probe, as the program's floor
/// sweep does on every move step: the first floor under the feet, however
/// far down, links the rider when it is a carrier's (0x48a0fc, 0x42cf54,
/// read R49-27 (d)). So a body in the air over the deck stays linked, and
/// one over the water or the dock is not; walking off, despawn or
/// unavailable geometry unlinks. The read gives the sweep no reach; the
/// link's bound of [`CARRIER_REACH`] from the boat's origin is ours, the
/// stricter side, and the session's guard holds the same (6.469, 6.611).
/// The read is Titanium's; the Mac-era client takes the same sweep,
/// inferred until its twin is read (6.611). The carrier `current` links
/// stays the link while any part of the footprint is over it
/// ([`CollisionWorld::spawn_floor_keeping`]).
pub(super) fn under(
    world: &CollisionWorld,
    carriers: &Carriers,
    feet: Vec3,
    current: Option<u16>,
    now: Instant,
) -> Option<(CarrierSample, WorldPosition)> {
    let (id, _) = world.spawn_floor_keeping(feet, 0.01, sweep_depth(feet), current)?;
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
            body_type: None,
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
            appearance: eq_client_core::outfit::Appearance::default(),
            level: 1,
            listing: eq_client_core::listing::Listing::default(),
            pet_owner: None,
            hp_percent: None,
            posture: None,
            name_parts: eq_client_core::names::NameParts::default(),
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
        assert!((carried.position.heading - 116.0).abs() < 0.001);
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
            under(&collision, &carriers, Vec3::new(0.0, 5.0, 0.0), None, start)
                .unwrap()
                .0
                .spawn_id,
            8
        );
        assert!(
            under(
                &collision,
                &carriers,
                Vec3::new(30.0, 5.0, 0.0),
                None,
                start
            )
            .is_none()
        );
        collision.set_spawn_obstacles(Vec::new());
        assert!(under(&collision, &carriers, Vec3::new(0.0, 5.0, 0.0), None, start).is_none());
        carriers.clear();
        assert!(carriers.latest(8).is_none());
    }
    #[test]
    fn a_ship_passing_alongside_keeps_the_rider_on_its_own_deck() {
        use eq_client_core::movement::CollisionMesh;
        // The #631 rerun (6.611): a ship coming alongside met part of the
        // rider's footprint and took the link. The linked ship keeps it
        // while any part of the footprint is over its deck.
        let start = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![
                carrier(8, WorldPosition::default()),
                carrier(9, WorldPosition::default()),
            ]),
            GameDialect::Titanium,
            start,
        );
        let slab = |from: f32, to: f32| {
            std::sync::Arc::new(
                CollisionMesh::new([
                    [[from, 5.0, -10.0], [to, 5.0, -10.0], [to, 5.0, 10.0]],
                    [[from, 5.0, -10.0], [to, 5.0, 10.0], [from, 5.0, 10.0]],
                ])
                .unwrap(),
            )
        };
        let mut collision = CollisionWorld::new([[
            [-500.0, -30.0, -500.0],
            [500.0, -30.0, -500.0],
            [0.0, -30.0, 500.0],
        ]])
        .unwrap();
        collision.set_spawn_obstacles(vec![(9, slab(-0.1, 20.0)), (8, slab(-20.0, 0.2))]);
        let linked = |feet: Vec3, current| {
            under(&collision, &carriers, feet, current, start).map(|(sample, _)| sample.spawn_id)
        };
        let feet = Vec3::new(0.0, 5.0, 0.0);
        assert_eq!(linked(feet, None), Some(9));
        assert_eq!(linked(feet, Some(8)), Some(8));
        assert_eq!(linked(Vec3::new(1.0, 5.0, 0.0), Some(8)), Some(9));
    }
    #[test]
    fn the_first_floor_below_links_the_rider_however_far_down() {
        use eq_client_core::movement::CollisionMesh;
        // The program's floor sweep runs to z -32767 and links the first
        // carrier it meets (read R49-27 (d), 6.611): a body high in the
        // air over the deck is linked; over the water or the dock, or
        // under the deck, it isn't.
        let start = Instant::now();
        let mut carriers = Carriers::default();
        carriers.observe(
            &WorldEvent::Spawns(vec![carrier(8, WorldPosition::default())]),
            GameDialect::Titanium,
            start,
        );
        let deck = [
            [[-10.0, 5.0, -10.0], [10.0, 5.0, -10.0], [10.0, 5.0, 10.0]],
            [[-10.0, 5.0, -10.0], [10.0, 5.0, 10.0], [-10.0, 5.0, 10.0]],
        ];
        let mut collision = CollisionWorld::new([
            // The sea floor, then a dock beside the boat, 1 over its deck.
            [
                [-500.0, -30.0, -500.0],
                [500.0, -30.0, -500.0],
                [0.0, -30.0, 500.0],
            ],
            [[12.0, 6.0, -10.0], [30.0, 6.0, -10.0], [30.0, 6.0, 10.0]],
            [[12.0, 6.0, -10.0], [30.0, 6.0, 10.0], [12.0, 6.0, 10.0]],
        ])
        .unwrap();
        collision.set_spawn_obstacles(vec![(
            8,
            std::sync::Arc::new(CollisionMesh::new(deck).unwrap()),
        )]);
        let linked = |feet: Vec3| {
            under(&collision, &carriers, feet, None, start).map(|(sample, _)| sample.spawn_id)
        };
        assert_eq!(linked(Vec3::new(0.0, 5.0, 0.0)), Some(8));
        assert_eq!(linked(Vec3::new(0.0, 9.0, 0.0)), Some(8));
        assert_eq!(linked(Vec3::new(0.0, 400.0, 0.0)), Some(8));
        assert_eq!(linked(Vec3::new(-40.0, 9.0, 0.0)), None);
        assert_eq!(linked(Vec3::new(20.0, 6.0, 0.0)), None);
        assert_eq!(linked(Vec3::new(20.0, 9.0, 0.0)), None);
        assert_eq!(linked(Vec3::new(0.0, 4.0, 0.0)), None);
        // Past the guard's bound of 600 from the boat's origin, ours
        // unlinks, the stricter side (6.469).
        assert_eq!(linked(Vec3::new(0.0, 700.0, 0.0)), None);
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
            ..super::super::Controls::default()
        };
        controls.wait_for(Some(11), now, super::super::PendingKind::Move);
        assert_eq!(controls.boat.unwrap().position, accepted.position);
        assert!(matches!(
            controls.liquid_basis(Some(11), now + std::time::Duration::from_secs(3)),
            super::super::LiquidBasis::Unconfirmed
        ));
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
            ..super::super::Controls::default()
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
            under(&collision, &carriers, feet, None, now)
                .unwrap()
                .0
                .spawn_id,
            8
        );
        assert!(under(&collision, &carriers, Vec3::new(0.5, 5.0, 0.0), None, now).is_none());
    }
}

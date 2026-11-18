//! The host's part of an arrival aboard a boat (Titanium 0x4aba14; reads
//! R36 §3, R36b §6). When the session says the boat the profile names is
//! here, the program stands the player at the profile's place in the boat's
//! frame and sweeps for the floor from one unit above it. If that finds no
//! floor linking the boat, it tries again at (0, 0, 25) in the frame. The
//! session hears the result once and takes it, or uses the zone's safe
//! point when nothing was found.
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand, WorldPosition,
    boats::riders::Arrival,
    movement::{CarrierSample, CollisionWorld, carrier_world},
};
use std::time::{Duration, Instant};

/// The program sweeps for the floor from one unit above the restored point.
const SWEEP_LIFT: f32 = 1.0;
/// How far below the restored point the sweep may find the deck (inferred,
/// checklist 6.478): the session refuses anything lower.
const SWEEP_DROP: f32 = 30.0;
/// The program's second try, in the boat's frame.
const RETRY: [f32; 3] = [0.0, 0.0, 25.0];
/// How long the host waits for the boat's collider to load before it
/// sweeps anyway (inferred, 6.478). The program has its model at once.
const COLLIDER_WAIT: Duration = Duration::from_secs(10);

/// Where the sweep stands the player, from the restored point `at`: on a
/// floor that links `carrier`, found from [`SWEEP_LIFT`] above the feet
/// down to [`SWEEP_DROP`] below them. Only a floor that is the carrier's
/// counts, as the program keeps a placement only when it links the boat.
fn sweep(
    collision: &CollisionWorld,
    at: WorldPosition,
    feet_offset: f32,
    carrier: u16,
) -> Option<WorldPosition> {
    let feet = Vec3::from_array(eq_client_core::render_position(at)) - Vec3::Y * feet_offset;
    let (floor, height) = collision.spawn_floor(feet, SWEEP_LIFT, SWEEP_DROP)?;
    if floor != carrier {
        return None;
    }
    let center = Vec3::new(feet.x, height + feet_offset, feet.z);
    Some(eq_client_core::world_position(
        center.to_array(),
        at.heading,
    ))
}

/// The placement the host offers: the restored point's floor, else the
/// retry point's, else none.
fn placement(
    collision: Option<&CollisionWorld>,
    pose: Option<WorldPosition>,
    local: WorldPosition,
    feet_offset: f32,
    carrier: u16,
) -> Option<WorldPosition> {
    let (collision, pose) = (collision?, pose?);
    let retry = WorldPosition {
        x: RETRY[0],
        y: RETRY[1],
        z: RETRY[2],
        heading: local.heading,
    };
    [local, retry]
        .into_iter()
        .find_map(|point| sweep(collision, carrier_world(pose, point), feet_offset, carrier))
}

/// The arrival answered last, and when the current one was first seen.
#[derive(Default)]
pub(super) struct Answered {
    seen: Option<((u64, CarrierSample), Instant)>,
    answered: Option<(u64, CarrierSample)>,
}

/// Answers the session's arrival aboard once, after the scene has placed
/// the boat's collider.
#[allow(
    clippy::needless_pass_by_value,
    reason = "Bevy supplies system parameters by value"
)]
pub(super) fn place(
    online: Res<crate::online::OnlineState>,
    collision: Option<Res<crate::Collision>>,
    bodies: Query<&crate::PlayerBody>,
    outbox: Res<crate::outbox::Outbox>,
    mut state: Local<Answered>,
) {
    let world = online.world();
    let (Some(Arrival::Aboard { sample, local, .. }), Some(session_id)) =
        (world.arrival(), world.session_id())
    else {
        *state = Answered::default();
        return;
    };
    let key = (session_id, *sample);
    if state.answered == Some(key) {
        return;
    }
    let now = Instant::now();
    let since = match state.seen {
        Some((seen, since)) if seen == key => since,
        _ => {
            state.seen = Some((key, now));
            now
        }
    };
    let collision = collision
        .as_deref()
        .and_then(|collision| collision.0.as_ref());
    let ready = !online.scene_loading()
        && collision.is_some_and(|collision| collision.has_spawn(sample.spawn_id));
    if !ready && now.duration_since(since) < COLLIDER_WAIT {
        return;
    }
    let Ok(body) = bodies.single() else {
        return;
    };
    let Some(stamp) = outbox.peek(world) else {
        return;
    };
    let position = placement(
        collision,
        world.carriers().pose(*sample, stamp.created),
        *local,
        body.feet_offset,
        sample.spawn_id,
    );
    if position.is_none() {
        warn!(
            spawn_id = sample.spawn_id,
            "Arrival aboard found no deck under the restored point or the retry point"
        );
    }
    if outbox
        .send(
            world,
            ClientCommand::ArrivalPlacement {
                session_id: stamp.session_id,
                position,
                created: stamp.created,
            },
        )
        .is_ok()
    {
        state.answered = Some(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::movement::CollisionMesh;
    use std::sync::Arc;

    /// A deck 20 across at height `deck` (world z) around the origin, owned
    /// by spawn 8, over sea floor far below.
    fn world(deck: f32) -> CollisionWorld {
        let corner = |x: f32, y: f32, z: f32| {
            eq_client_core::render_position(WorldPosition {
                x,
                y,
                z,
                heading: 0.0,
            })
        };
        let mut collision = CollisionWorld::new([[
            corner(-500.0, -500.0, -200.0),
            corner(500.0, -500.0, -200.0),
            corner(0.0, 500.0, -200.0),
        ]])
        .unwrap();
        collision.set_spawn_obstacles(vec![(
            8,
            Arc::new(
                CollisionMesh::new([[
                    corner(-20.0, -20.0, deck),
                    corner(20.0, -20.0, deck),
                    corner(0.0, 20.0, deck),
                ]])
                .unwrap(),
            ),
        )]);
        collision
    }

    const AT: WorldPosition = WorldPosition {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        heading: 0.0,
    };

    fn local(z: f32) -> WorldPosition {
        WorldPosition {
            x: 1.0,
            y: 2.0,
            z,
            heading: 40.0,
        }
    }

    #[test]
    fn the_restored_point_lands_on_the_deck_below_it() {
        let collision = world(5.0);
        // Feet 3 below the center; the restored center 4 above the deck.
        let placed = placement(Some(&collision), Some(AT), local(12.0), 3.0, 8).unwrap();
        assert!((placed.x - 1.0).abs() < 0.001 && (placed.y - 2.0).abs() < 0.001);
        assert!((placed.z - 8.0).abs() < 0.001, "{placed:?}");
        assert!((placed.heading - 40.0).abs() < 0.001);
    }

    #[test]
    fn no_deck_under_the_restored_point_tries_the_retry_point() {
        let collision = world(5.0);
        // Far below the deck: the sweep from one above finds no floor within
        // its reach, so the retry at 25 up in the frame lands on the deck.
        let placed = placement(Some(&collision), Some(AT), local(-150.0), 3.0, 8).unwrap();
        assert!(placed.x.abs() < 0.001 && placed.y.abs() < 0.001);
        assert!((placed.z - 8.0).abs() < 0.001, "{placed:?}");
    }

    #[test]
    fn a_floor_that_is_not_the_named_carrier_is_no_placement() {
        let collision = world(5.0);
        assert_eq!(
            placement(Some(&collision), Some(AT), local(12.0), 3.0, 9),
            None
        );
        assert_eq!(placement(None, Some(AT), local(12.0), 3.0, 8), None);
        assert_eq!(placement(Some(&collision), None, local(12.0), 3.0, 8), None);
    }
}

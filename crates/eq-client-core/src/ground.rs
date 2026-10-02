//! Items on the ground as the client finds and picks them. The server's object
//! table, the pickup rules and the reach live in the network session; this
//! module only chooses which object a key press or a click means.
pub use eq_network_game::objects::{
    ContainerView, GroundObject, ObjectKind, ObjectUpdate, Objects,
};

use crate::WorldPosition;
use glam::Vec3;

/// Small items are hard to click, so each axis of a pick box is at least this
/// long, in EQ units, around the model's own bounds.
pub const MIN_PICK_SIZE: f32 = 1.5;

/// The nearest object within reach that the player can use, measured as
/// the session measures it, with its distance: an item to pick up or a world
/// container such as a forge to open. Other fixtures are left out.
#[must_use]
pub fn nearest_usable(objects: &Objects, player: WorldPosition) -> Option<(u32, f32)> {
    objects
        .entries()
        .values()
        .filter(|object| object.kind() == ObjectKind::Item || object.is_tradeskill_container())
        .filter_map(|object| {
            let distance = (object.position.x - player.x)
                .hypot(object.position.y - player.y)
                .hypot(object.position.z - player.z);
            (distance.is_finite() && distance <= Objects::USE_DISTANCE)
                .then_some((object.drop_id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// A model's bounds grown to at least [`MIN_PICK_SIZE`] on each axis, around
/// the same center.
#[must_use]
pub fn pick_box(min: Vec3, max: Vec3) -> (Vec3, Vec3) {
    let center = (min + max) / 2.0;
    let half = ((max - min) / 2.0).max(Vec3::splat(MIN_PICK_SIZE / 2.0));
    (center - half, center + half)
}

/// Distance along a ray to where it enters a box, or None when it misses or
/// the box is behind it. `direction` need not be normalized; the distance is
/// in units of its length.
#[must_use]
pub fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let (start, step) = (origin[axis], direction[axis]);
        if step.abs() < f32::EPSILON {
            if start < min[axis] || start > max[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = ((min[axis] - start) / step, (max[axis] - start) / step);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    near.is_finite().then_some(near)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn object(drop_id: u32, model: &str, x: f32) -> GroundObject {
        GroundObject {
            drop_id,
            model: model.into(),
            position: WorldPosition {
                x,
                ..WorldPosition::default()
            },
            object_type: 0,
        }
    }

    #[test]
    fn the_nearest_usable_object_within_reach_is_chosen() {
        let mut objects = Objects::default();
        // A fixture that is not a container, nearest of all, is skipped.
        for update in [
            object(1, "IT63_ACTORDEF", 12.0),
            object(2, "IT10_ACTORDEF", 5.0),
            object(3, "FORGE", 1.0),
            object(4, "IT63_ACTORDEF", Objects::USE_DISTANCE + 1.0),
        ] {
            objects.apply(&ObjectUpdate::Spawn(update));
        }
        let player = WorldPosition::default();
        let (id, distance) = nearest_usable(&objects, player).unwrap();
        assert_eq!(id, 2);
        assert!(close(distance, 5.0));
        objects.apply(&ObjectUpdate::Remove {
            drop_id: 2,
            taken_by: None,
        });
        assert_eq!(nearest_usable(&objects, player).map(|hit| hit.0), Some(1));
        // A forge, a tradeskill container, is used as an item is.
        let mut forge = object(5, "FORGE", 2.0);
        forge.object_type = 17;
        objects.apply(&ObjectUpdate::Spawn(forge));
        assert_eq!(nearest_usable(&objects, player).map(|hit| hit.0), Some(5));
        objects.apply(&ObjectUpdate::Snapshot(Vec::new()));
        assert_eq!(nearest_usable(&objects, player), None);
    }

    #[test]
    fn small_models_get_a_clickable_box_around_their_center() {
        let (min, max) = pick_box(Vec3::new(-0.3, 0.0, -0.3), Vec3::new(0.3, 0.8, 0.3));
        assert!(close(max.x - min.x, MIN_PICK_SIZE) && close(max.y - min.y, MIN_PICK_SIZE));
        assert!(close(f32::midpoint(min.y, max.y), 0.4));
        let (min, max) = pick_box(Vec3::ZERO, Vec3::new(4.0, 1.0, 2.0));
        assert!(close(max.x - min.x, 4.0) && close(max.z - min.z, 2.0));
    }

    #[test]
    fn rays_enter_boxes_in_front_and_miss_the_rest() {
        let (min, max) = (Vec3::splat(-1.0), Vec3::splat(1.0));
        let hit = ray_box(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z, min, max).unwrap();
        assert!(close(hit, 9.0));
        // A longer direction measures in its own units.
        let hit = ray_box(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z * 2.0, min, max).unwrap();
        assert!(close(hit, 4.5));
        assert!(ray_box(Vec3::new(3.0, 0.0, 10.0), Vec3::NEG_Z, min, max).is_none());
        assert!(ray_box(Vec3::new(0.0, 0.0, 10.0), Vec3::Z, min, max).is_none());
        // A ray starting inside enters at once; one parallel to a face misses outside it.
        assert!(close(ray_box(Vec3::ZERO, Vec3::X, min, max).unwrap(), 0.0));
        assert!(ray_box(Vec3::new(0.0, 2.0, 0.0), Vec3::X, min, max).is_none());
    }
}

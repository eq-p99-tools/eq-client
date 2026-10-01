//! Where EQ's axes meet the renderer's.
//!
//! EQ places things in two related frames:
//!
//! - **World** coordinates, as servers send positions: X (west), Y (north) and
//!   Z (up), with headings from 0 up to 512 where a step along heading `h`
//!   moves `(sin h, cos h)` in X and Y.
//! - **WLD** coordinates, as zone and model files store geometry: the world's
//!   X and Y swapped, with Z still up. Stored geometry faces heading 0.
//!
//! The renderer is right-handed with Y up. EQ's world frame is the mirror
//! image of a right-handed one, so reaching the renderer takes a reflection;
//! drawn without it, the world comes out mirrored, with signpost text reading
//! backwards and right hands on the left. `WORLD_AXES` is the only statement
//! of that. Positions, directions, rotations, headings and triangle winding are
//! all derived from it here, so nothing else carries a flip of its own.

use std::f32::consts::TAU;

use glam::{Mat3, Mat4, Quat, Vec3};

/// Where the world's X, Y and up axes land in the renderer: -Z, X and Y. Its
/// determinant is -1, the reflection between EQ's frame and the renderer's.
const WORLD_AXES: Mat3 = Mat3::from_cols(Vec3::NEG_Z, Vec3::X, Vec3::Y);

/// WLD files store the world's X and Y swapped.
const WLD_TO_WORLD: Mat3 = Mat3::from_cols(Vec3::Y, Vec3::X, Vec3::Z);

/// A full turn in EQ heading units.
pub const FULL_TURN: f32 = 512.0;

fn wld_axes() -> Mat3 {
    WORLD_AXES * WLD_TO_WORLD
}

/// Converts a world position or direction to the renderer's frame.
#[must_use]
pub fn from_world(vector: Vec3) -> Vec3 {
    WORLD_AXES * vector
}

/// Converts a renderer position or direction to world coordinates.
#[must_use]
pub fn to_world(vector: Vec3) -> Vec3 {
    WORLD_AXES.transpose() * vector
}

/// Converts a WLD position or direction, normals included, to the renderer's
/// frame.
#[must_use]
pub fn from_wld(vector: Vec3) -> Vec3 {
    wld_axes() * vector
}

/// Converts a WLD position, such as a destination written into a zone file, to
/// world coordinates.
#[must_use]
pub fn wld_to_world(vector: Vec3) -> Vec3 {
    WLD_TO_WORLD * vector
}

/// Converts a WLD transform, such as a bone's, to one acting on renderer
/// coordinates: applied to `from_wld(p)`, it gives `from_wld(transform * p)`.
#[must_use]
pub fn wld_transform(transform: Mat4) -> Mat4 {
    let axes = Mat4::from_mat3(wld_axes());
    axes * transform * axes.transpose()
}

/// Converts a WLD rotation, such as a placed object's, to the renderer's frame.
#[must_use]
pub fn wld_rotation(rotation: Quat) -> Quat {
    let axes = wld_axes();
    Quat::from_mat3(&(axes * Mat3::from_quat(rotation) * axes.transpose()))
}

/// Orders a WLD triangle's corners for the renderer, which winds front faces
/// counter-clockwise around their normals. WLD files wind them clockwise. A
/// conversion that reflects reverses the winding by itself; one that does not
/// needs two corners swapped.
#[must_use]
pub fn wld_triangle<T>([a, b, c]: [T; 3]) -> [T; 3] {
    if wld_axes().determinant() > 0.0 {
        [a, c, b]
    } else {
        [a, b, c]
    }
}

/// The world direction an EQ heading faces.
#[must_use]
pub fn heading_direction(heading: f32) -> Vec3 {
    let radians = heading / FULL_TURN * TAU;
    Vec3::new(radians.sin(), radians.cos(), 0.0)
}

/// Renderer yaw about Y that turns +Z, the way the renderer's character roots
/// face, toward an EQ heading.
#[must_use]
pub fn render_heading(heading: f32) -> f32 {
    let facing = from_world(heading_direction(heading));
    facing.x.atan2(facing.z)
}

/// The EQ heading, from 0 up to 512, that a renderer yaw about Y turns +Z
/// toward.
#[must_use]
pub fn world_heading(yaw: f32) -> f32 {
    let facing = to_world(Vec3::new(yaw.sin(), 0.0, yaw.cos()));
    let heading = (facing.x.atan2(facing.y) / TAU * FULL_TURN).rem_euclid(FULL_TURN);
    // Rounding can carry a tiny negative angle to a whole turn.
    if heading < FULL_TURN { heading } else { 0.0 }
}

/// Renderer yaw that turns WLD geometry, which faces heading 0 as stored, to
/// face +Z: the turn between a character's model and its root.
#[must_use]
pub fn model_yaw() -> f32 {
    -render_heading(0.0)
}

/// Renderer yaw for WLD geometry placed at an EQ heading, such as a door or an
/// item on the ground: a root's turn and a model's together.
#[must_use]
pub fn static_yaw(heading: f32) -> f32 {
    render_heading(heading) + model_yaw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn the_world_is_the_mirror_image_of_the_renderer_with_up_kept_up() {
        assert!((WORLD_AXES.determinant() + 1.0).abs() < 1e-6);
        assert!(close(from_world(Vec3::Z), Vec3::Y));
        assert!(close(from_wld(Vec3::Z), Vec3::Y));
    }

    #[test]
    fn west_lies_counter_clockwise_of_north_seen_from_above() {
        // EQ's +Y is north and +X is west, so a north-up map has west on the
        // left: a counter-clockwise quarter turn about the renderer's up.
        let (north, west) = (from_world(Vec3::Y), from_world(Vec3::X));
        assert!(north.cross(west).dot(Vec3::Y) > 0.99);
    }

    #[test]
    fn conversions_round_trip_and_agree_between_frames() {
        let v = Vec3::new(10.0, 20.0, 30.0);
        assert!(close(from_world(v), Vec3::new(20.0, 30.0, -10.0)));
        assert!(close(to_world(from_world(v)), v));
        assert!(close(from_wld(v), from_world(wld_to_world(v))));
        assert!(close(wld_to_world(wld_to_world(v)), v));
    }

    #[test]
    fn wld_transforms_and_rotations_follow_the_points() {
        let rotation = Quat::from_euler(glam::EulerRot::XYZ, 0.3, -1.1, 2.0);
        let transform = Mat4::from_rotation_translation(rotation, Vec3::new(1.0, 2.0, 3.0));
        let point = Vec3::new(-4.0, 5.0, 6.0);
        assert!(close(
            wld_transform(transform).transform_point3(from_wld(point)),
            from_wld(transform.transform_point3(point)),
        ));
        assert!(close(
            wld_rotation(rotation) * from_wld(point),
            from_wld(rotation * point),
        ));
    }

    #[test]
    fn wld_triangles_wound_clockwise_face_their_normals_in_the_renderer() {
        // Clockwise around WLD +Z, seen from above.
        let [a, b, c] = wld_triangle([Vec3::ZERO, Vec3::Y, Vec3::X].map(from_wld));
        assert!((b - a).cross(c - a).dot(from_wld(Vec3::Z)) > 0.0);
    }

    #[test]
    fn headings_face_where_a_step_along_them_goes() {
        for heading in [0.0, 37.0, 128.0, 256.0, 300.0, 384.0, 451.0, 511.9] {
            let step = from_world(heading_direction(heading));
            let yaw = render_heading(heading);
            assert!(
                close(Quat::from_rotation_y(yaw) * Vec3::Z, step),
                "{heading}"
            );
            assert!((world_heading(yaw) - heading).abs() < 1e-3, "{heading}");
        }
        // Heading 128 is west.
        assert!(close(heading_direction(128.0), Vec3::X));
        let whole = world_heading(render_heading(FULL_TURN));
        assert!((0.0..FULL_TURN).contains(&whole));
        assert!(whole.min(FULL_TURN - whole) < 1e-3);
    }

    #[test]
    fn stored_geometry_turns_from_heading_zero() {
        for heading in [0.0, 100.0, 256.0, 500.0] {
            let facing = from_world(heading_direction(heading));
            let turned = Quat::from_rotation_y(static_yaw(heading)) * from_wld(Vec3::X);
            assert!(close(turned, facing), "{heading}");
        }
        let model = Quat::from_rotation_y(model_yaw()) * from_wld(Vec3::X);
        assert!(close(model, Vec3::Z));
    }
}

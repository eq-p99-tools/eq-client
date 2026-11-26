//! Titanium's walking object filter is distinct from floor and headroom.
use super::{CAPSULE_RADIUS, CollisionInstance, CollisionWorld, LIFT, SKIN, Surface};
use eq_network_game::world::SpawnKind;
use glam::Vec3;
use parry3d::{
    bounding_volume::Aabb,
    math::{Pose, Vector},
    query::{PointQueryWithLocation, ShapeCastOptions, cast_shapes},
    shape::{Ball, TrianglePointLocation},
};

#[derive(Clone, Copy)]
pub(super) enum SweepPurpose {
    Body,
    Walking,
}

impl CollisionWorld {
    pub(super) fn carrier_blocks(
        &self,
        purpose: SweepPurpose,
        instance: &CollisionInstance,
    ) -> bool {
        // [R] eqgame.exe 0x48acef uses 0x42cf6e for walking, also aboard
        // a ship: +0x158 is the boat link, not the +0x160 mount/rider link.
        // Preserve player/unknown/unclassified carriers (separate gap 6.665).
        !(self.titanium_walking_origin.is_some()
            && matches!(purpose, SweepPurpose::Walking)
            && instance.spawn_kind == Some(SpawnKind::Npc))
    }

    pub(super) fn carrier_headroom(&self, feet: Vec3, height: f32) -> bool {
        let Some(feet_offset) = self.titanium_walking_origin else {
            return true;
        };
        // [R] The NPC remains in the floor query (0x42d05f) and in the
        // distinct ceiling query (0x43a0a0: 0x42d123 / 0x42cf5c).
        // [R] The upward ceiling query starts at the actor origin, not at
        // the foot-level wall sphere (0x43a0a0). A low rope already below
        // that origin must not be turned back into a wall by this query.
        // [I] Retain this client's capsule radius and top as its shape estimate.
        let height = self.collision_height.unwrap_or(height);
        self.spawn_obstacles
            .iter()
            .filter(|(_, instance)| instance.spawn_kind == Some(SpawnKind::Npc))
            .all(|(_, instance)| headroom(instance.surface(), feet, height, feet_offset))
    }
}

fn headroom(surface: Surface<'_>, feet: Vec3, height: f32, feet_offset: f32) -> bool {
    let reach = height + LIFT - CAPSULE_RADIUS - feet_offset;
    if reach <= 0.0 {
        return true;
    }
    let start = Vector::from_array((feet + Vec3::Y * feet_offset).to_array());
    let begin = surface.pose.inverse_transform_point(start);
    let rise = surface.pose.rotation.inverse() * Vector::Y * reach;
    let end = begin + rise;
    let margin = Vector::splat(CAPSULE_RADIUS + SKIN);
    let bounds = Aabb::new(begin.min(end) - margin, begin.max(end) + margin);
    let pose = Pose::translation(begin.x, begin.y, begin.z);
    let ball = Ball::new(CAPSULE_RADIUS);
    let options = ShapeCastOptions {
        max_time_of_impact: 1.0,
        target_distance: SKIN,
        stop_at_penetration: false,
        compute_impact_geometry_on_penetration: true,
    };
    surface.mesh.bvh().intersect_aabb(&bounds).all(|index| {
        let triangle = surface.mesh.triangle(index);
        // Use authored facing for this upward query, just as a floor query
        // keeps the upward-facing side. Drawing both sides of a material
        // does not make a floor's back face a ceiling (0x101875c0).
        if triangle.scaled_normal().dot(rise) > 0.0 {
            return true;
        }
        if let Some(mut normal) = triangle.normal() {
            let mut distance = (begin - triangle.a).dot(normal);
            if distance < 0.0 {
                normal = -normal;
                distance = -distance;
            }
            // As in flight_sweep, a separating plane stays separating for
            // this whole linear move when it is nonclosing. This avoids a
            // numerically unstable zero-distance contact beside a mast.
            // Include the world-to-model subtraction in the error bound:
            // a scaled/translated actor otherwise turns exact tangency
            // into a tiny overlap and reports a false upward contact.
            let magnitude =
                start.abs().max_element() + surface.pose.translation.abs().max_element() + height;
            let roundoff = magnitude.max(1.0) * f32::EPSILON * 8.0;
            if distance - CAPSULE_RADIUS >= -roundoff && rise.dot(normal) >= 0.0 {
                return true;
            }
            // A centre already projected inside the convex face is at its
            // minimum possible distance from that plane. Moving parallel
            // or away cannot deepen its initial overlap, even if the centre
            // is exactly on a wide partition. Keep the cast for edge/vertex
            // approaches: their distance can still decrease along the plane.
            if rise.dot(normal) >= 0.0
                && matches!(
                    triangle
                        .project_local_point_and_get_location(begin, false)
                        .1,
                    TrianglePointLocation::OnFace(..)
                )
            {
                return true;
            }
        }
        matches!(
            cast_shapes(
                &pose,
                rise,
                &ball,
                &Pose::IDENTITY,
                Vector::ZERO,
                &triangle,
                options
            ),
            Ok(None)
        )
    })
}

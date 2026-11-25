//! Titanium's walking object filter is distinct from floor and headroom.
use super::{CAPSULE_RADIUS, CollisionInstance, CollisionWorld, LIFT, SKIN};
use eq_network_game::world::SpawnKind;
use glam::Vec3;
use parry3d::{
    math::{Pose, Vector},
    query::{ShapeCastOptions, cast_shapes},
    shape::Ball,
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
        !(self.titanium_walking
            && matches!(purpose, SweepPurpose::Walking)
            && instance.spawn_kind == Some(SpawnKind::Npc))
    }

    pub(super) fn carrier_headroom(&self, feet: Vec3, height: f32) -> bool {
        if !self.titanium_walking {
            return true;
        }
        // [R] The NPC remains in the floor/headroom query (0x42d05f).
        // [I] Sweep the capsule's radius upward through its existing height:
        // the same body volume, approached vertically instead of sideways.
        // A mast is not made a wall again by a stationary overlap test.
        let height = self.collision_height.unwrap_or(height);
        let pose = Pose::translation(feet.x, feet.y + LIFT + CAPSULE_RADIUS, feet.z);
        let ball = Ball::new(CAPSULE_RADIUS);
        let rise = Vector::Y * (height - 2.0 * CAPSULE_RADIUS);
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            target_distance: SKIN,
            stop_at_penetration: false,
            compute_impact_geometry_on_penetration: true,
        };
        self.spawn_obstacles
            .iter()
            .filter(|(_, instance)| instance.spawn_kind == Some(SpawnKind::Npc))
            .all(|(_, instance)| {
                let mesh = instance.surface();
                matches!(
                    cast_shapes(
                        &pose,
                        rise,
                        &ball,
                        &mesh.pose,
                        Vector::ZERO,
                        mesh.mesh,
                        options
                    ),
                    Ok(None)
                )
            })
    }
}

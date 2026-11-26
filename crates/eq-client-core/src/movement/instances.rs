//! Rigid placements of shared solid geometry; moving a placement keeps its BVH.
use super::CollisionMesh;
use glam::{Quat, Vec3};
use parry3d::{
    math::{Pose, Rotation, Vector},
    query::{Ray, RayCast},
    shape::TriMesh,
};
use std::sync::Arc;

/// A rigid placement of immutable collision geometry. Scale is part of the
/// shared geometry; translation and rotation do not rebuild its triangles/BVH.
#[derive(Clone)]
pub struct CollisionInstance {
    mesh: Arc<CollisionMesh>,
    pose: Pose,
    pub(super) spawn_kind: Option<eq_network_game::world::SpawnKind>,
}

impl CollisionInstance {
    /// Places already scaled geometry in the Y-up world frame.
    ///
    /// # Errors
    /// Rejects non-finite translation and a non-finite or non-unit rotation.
    pub fn new(
        mesh: Arc<CollisionMesh>,
        translation: Vec3,
        rotation: Quat,
    ) -> Result<Self, String> {
        if !translation.is_finite() || !rotation.is_finite() || !rotation.is_normalized() {
            return Err("collision placement requires finite translation and unit rotation".into());
        }
        Ok(Self {
            mesh,
            spawn_kind: None,
            pose: Pose::from_parts(
                Vector::from_array(translation.to_array()),
                Rotation::from_array(rotation.to_array()),
            ),
        })
    }

    pub(super) fn identity(mesh: Arc<CollisionMesh>) -> Self {
        Self {
            mesh,
            pose: Pose::IDENTITY,
            spawn_kind: None,
        }
    }

    /// Retains the received kind of a spawned carrier independently of its
    /// model or race. Unclassified instances retain their existing solidity.
    #[must_use]
    pub fn with_spawn_kind(mut self, kind: eq_network_game::world::SpawnKind) -> Self {
        self.spawn_kind = Some(kind);
        self
    }

    pub(super) fn surface(&self) -> Surface<'_> {
        Surface {
            mesh: &self.mesh.mesh,
            pose: self.pose,
        }
    }
}

/// A query view; callers pass its pose to narrow-phase tests, and traverse
/// its shared BVH in authored coordinates.
#[derive(Clone, Copy)]
pub(super) struct Surface<'a> {
    pub(super) mesh: &'a TriMesh,
    pub(super) pose: Pose,
}

impl<'a> Surface<'a> {
    pub(super) fn identity(mesh: &'a CollisionMesh) -> Self {
        Self {
            mesh: &mesh.mesh,
            pose: Pose::IDENTITY,
        }
    }

    pub(super) fn cast_ray(self, ray: &Ray, reach: f32, solid: bool) -> Option<f32> {
        self.mesh.cast_ray(&self.pose, ray, reach, solid)
    }

    /// A support query skips downward-facing faces, without changing the
    /// geometry used by walls, ceilings or viewing rays (6.651, Titanium
    /// 0x10185790 / 0x10192400). The same rule on Mac is inferred.
    /// A two-sided render material still contributes only its authored front.
    pub(super) fn cast_floor(self, ray: &Ray, reach: f32) -> Option<f32> {
        let local = ray.inverse_transform_by(&self.pose);
        let up = self.pose.rotation.inverse() * Vector::Y;
        self.mesh
            .bvh()
            .cast_ray(&local, reach, |index, best| {
                let face = self.mesh.triangle(index);
                // Test the authored normal before ray casting: returned
                // hit normals can be flipped to face the incoming ray.
                (face.scaled_normal().dot(up) >= 0.0)
                    .then(|| face.cast_local_ray(&local, best, false))
                    .flatten()
            })
            .filter(|(_, time)| *time < reach)
            .map(|(_, time)| time)
    }

    pub(super) fn minimum_y(self) -> f32 {
        if self.pose == Pose::IDENTITY {
            return self.mesh.local_aabb().mins.y;
        }
        // Placement diagnosis uses this as an exact search extent, not a
        // broad-phase bound. A rotated AABB can include unoccupied corners.
        self.mesh
            .vertices()
            .iter()
            .map(|point| self.pose.transform_point(*point).y)
            .min_by(f32::total_cmp)
            .unwrap_or(self.pose.translation.y)
    }
}

#[cfg(test)]
mod tests;

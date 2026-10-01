//! Click-time intersections with the current CPU-skinned character geometry.
use bevy::{ecs::system::SystemParam, mesh::VertexAttributeValues, prelude::*};

#[derive(SystemParam)]
pub(crate) struct Picker<'w, 's> {
    #[allow(clippy::type_complexity)]
    nodes: Query<
        'w,
        's,
        (
            Option<&'static Transform>,
            Option<&'static Children>,
            Option<&'static Mesh3d>,
            Option<&'static Visibility>,
        ),
    >,
    meshes: Option<Res<'w, Assets<Mesh>>>,
}

impl Picker<'_, '_> {
    /// Uses local hierarchy transforms so interpolation does not wait for propagation.
    pub(super) fn distance(&self, root: Entity, ray: Ray3d) -> Option<f32> {
        let meshes = self.meshes.as_ref()?;
        let mut nearest: Option<f32> = None;
        let mut pending = vec![(root, Mat4::IDENTITY)];
        while let Some((entity, parent)) = pending.pop() {
            let Ok((transform, children, mesh, visibility)) = self.nodes.get(entity) else {
                continue;
            };
            if visibility == Some(&Visibility::Hidden) {
                continue;
            }
            let world = parent * transform.map_or(Mat4::IDENTITY, Transform::to_matrix);
            if let Some(mesh) = mesh.and_then(|handle| meshes.get(&handle.0))
                && let Some(hit) = mesh_distance(mesh, world, ray)
                && nearest.is_none_or(|previous| hit < previous)
            {
                nearest = Some(hit);
            }
            if let Some(children) = children {
                pending.extend(children.iter().map(|child| (child, world)));
            }
        }
        nearest
    }
}

/// Leaves the transformed direction unnormalized, preserving world-distance parameters.
fn mesh_distance(mesh: &Mesh, world: Mat4, ray: Ray3d) -> Option<f32> {
    if mesh.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
        return None;
    }
    let inverse = world.inverse();
    if !inverse.is_finite() {
        return None;
    }
    let origin = inverse.transform_point3(ray.origin);
    let direction = inverse.transform_vector3(*ray.direction);
    let VertexAttributeValues::Float32x3(vertices) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let mut nearest: Option<f32> = None;
    let mut visit = |indices: [usize; 3]| {
        let [Some(a), Some(b), Some(c)] =
            indices.map(|index| vertices.get(index).copied().map(Vec3::from_array))
        else {
            return;
        };
        if let Some(hit) = triangle_distance(origin, direction, [a, b, c])
            && nearest.is_none_or(|previous| hit < previous)
        {
            nearest = Some(hit);
        }
    };
    if let Some(indices) = mesh.indices() {
        let mut indices = indices.iter();
        while let (Some(a), Some(b), Some(c)) = (indices.next(), indices.next(), indices.next()) {
            visit([a, b, c]);
        }
    } else {
        for base in (0..vertices.len().saturating_sub(2)).step_by(3) {
            visit([base, base + 1, base + 2]);
        }
    }
    nearest
}

/// Double-sided intersection, excluding degenerate triangles and hits behind the camera.
fn triangle_distance(origin: Vec3, direction: Vec3, [vertex, b, c]: [Vec3; 3]) -> Option<f32> {
    let edge1 = b - vertex;
    let edge2 = c - vertex;
    let cross = direction.cross(edge2);
    let determinant = edge1.dot(cross);
    if !determinant.is_finite() || determinant.abs() < 1e-7 {
        return None;
    }
    let offset = origin - vertex;
    let u = offset.dot(cross) / determinant;
    let perpendicular = offset.cross(edge1);
    let v = direction.dot(perpendicular) / determinant;
    let distance = edge2.dot(perpendicular) / determinant;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance >= 0.0 && distance.is_finite())
        .then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        ecs::system::SystemState,
        mesh::{Indices, PrimitiveTopology},
    };

    fn triangle() -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        );
        mesh
    }

    #[test]
    fn mesh_hits_preserve_world_distance_with_scale_and_reject_empty_space() {
        let mut mesh = triangle();
        let ray = Ray3d::new(Vec3::new(0.0, 0.0, 10.0), Dir3::NEG_Z);
        let placement = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 3.0, 4.0),
            Quat::IDENTITY,
            Vec3::Z * 2.0,
        );
        assert!((mesh_distance(&mesh, placement, ray).unwrap() - 8.0).abs() < 0.001);
        // The bounding rectangle contains this ray but the actual triangle does not.
        let miss = Ray3d::new(Vec3::new(1.9, 2.9, 10.0), Dir3::NEG_Z);
        assert!(mesh_distance(&mesh, placement, miss).is_none());
        mesh.insert_indices(Indices::U32(vec![2, 1, 0]));
        assert!((mesh_distance(&mesh, placement, ray).unwrap() - 8.0).abs() < 0.001);
        assert!(mesh_distance(&mesh, Mat4::from_translation(Vec3::Z * 11.0), ray).is_none());
        assert!(mesh_distance(&mesh, Mat4::from_scale(Vec3::ZERO), ray).is_none());
        mesh.insert_indices(Indices::U32(vec![0, 1, 500]));
        assert!(mesh_distance(&mesh, placement, ray).is_none());
    }

    #[test]
    fn hierarchy_picking_tracks_pose_buffers_and_hidden_children() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        let handle = world.resource_mut::<Assets<Mesh>>().add(triangle());
        let root = world.spawn(Transform::from_xyz(3.0, 0.0, 0.0)).id();
        let child = world
            .spawn((Transform::from_xyz(0.0, 2.0, 0.0), Visibility::Inherited))
            .id();
        let leaf = world
            .spawn((Mesh3d(handle.clone()), Transform::from_xyz(0.0, 0.0, 2.0)))
            .id();
        world.entity_mut(root).add_child(child);
        world.entity_mut(child).add_child(leaf);
        let ray = Ray3d::new(Vec3::new(3.0, 2.0, 10.0), Dir3::NEG_Z);
        let mut system = SystemState::<Picker>::new(&mut world);
        assert!((system.get(&world).unwrap().distance(root, ray).unwrap() - 8.0).abs() < 0.001);
        world
            .resource_mut::<Assets<Mesh>>()
            .get_mut(&handle)
            .unwrap()
            .insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[-1.0, -1.0, 3.0], [1.0, -1.0, 3.0], [0.0, 1.0, 3.0]],
            );
        assert!((system.get(&world).unwrap().distance(root, ray).unwrap() - 5.0).abs() < 0.001);
        world.entity_mut(child).insert(Visibility::Hidden);
        assert!(system.get(&world).unwrap().distance(root, ray).is_none());
        world.entity_mut(child).insert(Visibility::Inherited);
        world
            .entity_mut(root)
            .insert(Transform::from_xyz(30.0, 0.0, 0.0));
        assert!(system.get(&world).unwrap().distance(root, ray).is_none());
    }
}

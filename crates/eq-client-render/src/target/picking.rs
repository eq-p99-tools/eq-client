//! Click-time intersections with the characters' geometry as drawn: a
//! skinned mesh's vertices are placed by their joints here, at the click, as
//! the GPU places them each frame.
use bevy::{
    ecs::system::SystemParam,
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
};

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
            Option<&'static SkinnedMesh>,
        ),
    >,
    meshes: Option<Res<'w, Assets<Mesh>>>,
    bindposes: Option<Res<'w, Assets<SkinnedMeshInverseBindposes>>>,
}

/// What a click's ray meets of one spawn as drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Pick {
    /// World distance along the ray to the nearest drawn triangle it hits.
    pub hit: Option<f32>,
    /// The box the drawn model spans in its current pose; None for a spawn
    /// with nothing drawn.
    pub posed: Option<PosedBox>,
}

/// The box a spawn's drawn vertices span, in its root's frame, so it turns
/// with the spawn's heading and follows its pose: a lying corpse's is flat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PosedBox {
    /// The spawn root's placement, from the box's frame to the world.
    pub frame: Mat4,
    pub min: Vec3,
    pub max: Vec3,
}

impl PosedBox {
    /// World distance along a ray to where it enters the box. None when it
    /// misses, or when the ray starts inside it: a camera inside a model,
    /// as in the player's own in first person, would otherwise pick that
    /// model with every click.
    pub fn entry(&self, ray: Ray3d) -> Option<f32> {
        let inverse = self.frame.inverse();
        if !inverse.is_finite() {
            return None;
        }
        let origin = inverse.transform_point3(ray.origin);
        if origin.cmpge(self.min).all() && origin.cmple(self.max).all() {
            return None;
        }
        // Unnormalized, so the distance stays in world units.
        let direction = inverse.transform_vector3(*ray.direction);
        eq_client_core::ground::ray_box(origin, direction, self.min, self.max)
    }

    /// The box's middle in the world.
    pub fn middle(&self) -> Vec3 {
        self.frame.transform_point3((self.min + self.max) / 2.0)
    }

    /// The box's eight corners in the world.
    pub fn corners(&self) -> [Vec3; 8] {
        let (min, max) = (self.min, self.max);
        std::array::from_fn(|index| {
            let corner = Vec3::new(
                if index & 1 == 0 { min.x } else { max.x },
                if index & 2 == 0 { min.y } else { max.y },
                if index & 4 == 0 { min.z } else { max.z },
            );
            self.frame.transform_point3(corner)
        })
    }
}

impl Picker<'_, '_> {
    /// The ray's nearest hit on a spawn's drawn triangles and the box they
    /// span. Uses local hierarchy transforms so interpolation does not wait
    /// for propagation.
    pub(super) fn pick(&self, root: Entity, ray: Ray3d) -> Pick {
        let Some(meshes) = self.meshes.as_ref() else {
            return Pick::default();
        };
        let frame = self
            .nodes
            .get(root)
            .ok()
            .and_then(|(transform, ..)| transform)
            .map_or(Mat4::IDENTITY, Transform::to_matrix);
        let into_frame = Some(frame.inverse()).filter(Mat4::is_finite);
        let mut nearest: Option<f32> = None;
        let mut bounds: Option<(Vec3, Vec3)> = None;
        let mut pending = vec![(root, Mat4::IDENTITY)];
        while let Some((entity, parent)) = pending.pop() {
            let Ok((transform, children, mesh, visibility, skinned)) = self.nodes.get(entity)
            else {
                continue;
            };
            if visibility == Some(&Visibility::Hidden) {
                continue;
            }
            let world = parent * transform.map_or(Mat4::IDENTITY, Transform::to_matrix);
            let drawn = mesh
                .and_then(|handle| meshes.get(&handle.0))
                .and_then(|mesh| {
                    let placed = match skinned {
                        Some(skinned) => self.skinned_placed(mesh, skinned, parent),
                        None => placed(mesh, world),
                    }?;
                    Some((mesh, placed))
                });
            if let Some((mesh, placed)) = drawn {
                if let Some(hit) = triangles_distance(mesh, &placed, ray.origin, *ray.direction)
                    && nearest.is_none_or(|previous| hit < previous)
                {
                    nearest = Some(hit);
                }
                if let Some(into_frame) = into_frame {
                    for vertex in &placed {
                        let local = into_frame.transform_point3(*vertex);
                        bounds = Some(bounds.map_or((local, local), |(low, high)| {
                            (low.min(local), high.max(local))
                        }));
                    }
                }
            }
            if let Some(children) = children {
                pending.extend(children.iter().map(|child| (child, world)));
            }
        }
        Pick {
            hit: nearest,
            posed: bounds
                .filter(|(low, high)| low.is_finite() && high.is_finite())
                .map(|(min, max)| PosedBox { frame, min, max }),
        }
    }

    /// A skinned mesh's vertices in the world, placed by their joints,
    /// which hang beside it from the same parent, as a character's do.
    fn skinned_placed(
        &self,
        mesh: &Mesh,
        skinned: &SkinnedMesh,
        parent: Mat4,
    ) -> Option<Vec<Vec3>> {
        let bindposes = self.bindposes.as_ref()?.get(&skinned.inverse_bindposes)?;
        let joints = skinned
            .joints
            .iter()
            .zip(bindposes.iter())
            .map(|(joint, bindpose)| {
                let (transform, ..) = self.nodes.get(*joint).ok()?;
                Some(parent * transform?.to_matrix() * *bindpose)
            })
            .collect::<Option<Vec<_>>>()?;
        let VertexAttributeValues::Uint16x4(bones) = mesh
            .try_attribute_option(Mesh::ATTRIBUTE_JOINT_INDEX)
            .ok()
            .flatten()?
        else {
            return None;
        };
        positions(mesh)?
            .iter()
            .zip(bones)
            .map(|(position, [bone, ..])| {
                Some(
                    joints
                        .get(usize::from(*bone))?
                        .transform_point3(Vec3::from_array(*position)),
                )
            })
            .collect()
    }
}

/// A mesh's positions; None for a mesh without them, or whose data went to
/// the render world.
fn positions(mesh: &Mesh) -> Option<&[[f32; 3]]> {
    match mesh
        .try_attribute_option(Mesh::ATTRIBUTE_POSITION)
        .ok()
        .flatten()?
    {
        VertexAttributeValues::Float32x3(vertices) => Some(vertices),
        _ => None,
    }
}

/// A static mesh's vertices in the world. A mesh whose data went to the
/// render world is skipped like one without positions.
fn placed(mesh: &Mesh, world: Mat4) -> Option<Vec<Vec3>> {
    Some(
        positions(mesh)?
            .iter()
            .map(|position| world.transform_point3(Vec3::from_array(*position)))
            .collect(),
    )
}

/// The nearest hit along a ray among a mesh's triangles, with its vertices
/// where they are placed, in the ray's frame.
fn triangles_distance(
    mesh: &Mesh,
    vertices: &[Vec3],
    origin: Vec3,
    direction: Vec3,
) -> Option<f32> {
    if mesh.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
        return None;
    }
    let mut nearest: Option<f32> = None;
    let mut visit = |indices: [usize; 3]| {
        let [Some(a), Some(b), Some(c)] = indices.map(|index| vertices.get(index).copied()) else {
            return;
        };
        if let Some(hit) = triangle_distance(origin, direction, [a, b, c])
            && nearest.is_none_or(|previous| hit < previous)
        {
            nearest = Some(hit);
        }
    };
    if let Some(indices) = mesh.try_indices_option().ok()? {
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

    /// The nearest hit of a world ray on a static mesh placed in the world.
    fn mesh_distance(mesh: &Mesh, world: Mat4, ray: Ray3d) -> Option<f32> {
        triangles_distance(mesh, &placed(mesh, world)?, ray.origin, *ray.direction)
    }

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
        assert!((system.get(&world).unwrap().pick(root, ray).hit.unwrap() - 8.0).abs() < 0.001);
        world
            .resource_mut::<Assets<Mesh>>()
            .get_mut(&handle)
            .unwrap()
            .insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[-1.0, -1.0, 3.0], [1.0, -1.0, 3.0], [0.0, 1.0, 3.0]],
            );
        assert!((system.get(&world).unwrap().pick(root, ray).hit.unwrap() - 5.0).abs() < 0.001);
        world.entity_mut(child).insert(Visibility::Hidden);
        assert!(system.get(&world).unwrap().pick(root, ray).hit.is_none());
        world.entity_mut(child).insert(Visibility::Inherited);
        world
            .entity_mut(root)
            .insert(Transform::from_xyz(30.0, 0.0, 0.0));
        assert!(system.get(&world).unwrap().pick(root, ray).hit.is_none());
    }

    #[test]
    fn meshes_whose_data_went_to_the_render_world_are_skipped() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        let ray = Ray3d::new(Vec3::new(0.0, 0.0, 10.0), Dir3::NEG_Z);
        // What the render world's extraction leaves of a mesh drawn only there.
        let mut extracted = triangle();
        extracted.take_gpu_data().unwrap();
        assert!(mesh_distance(&extracted, Mat4::IDENTITY, ray).is_none());
        let (extracted, body) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (meshes.add(extracted), meshes.add(triangle()))
        };
        let root = world.spawn(Transform::IDENTITY).id();
        let held = world.spawn((Mesh3d(extracted), Transform::IDENTITY)).id();
        world.entity_mut(root).add_child(held);
        let mut system = SystemState::<Picker>::new(&mut world);
        assert!(system.get(&world).unwrap().pick(root, ray).hit.is_none());
        // The spawn's other meshes are still picked.
        let figure = world
            .spawn((Mesh3d(body), Transform::from_xyz(0.0, 0.0, 1.0)))
            .id();
        world.entity_mut(root).add_child(figure);
        assert!((system.get(&world).unwrap().pick(root, ray).hit.unwrap() - 9.0).abs() < 0.001);
    }

    #[test]
    fn a_skinned_character_is_picked_where_its_joints_put_it() {
        use crate::character::{AnimatedCharacter, testing};
        let mut world = World::new();
        testing::stores(&mut world);
        let prepared = testing::prepared(&mut world);
        let root = testing::drawn(&mut world, &prepared);
        let character = world.get::<AnimatedCharacter>(root).unwrap();
        let (model, part) = (character.model, character.parts[0].entity);
        let placement = world.get::<Transform>(model).unwrap().to_matrix();
        // The triangle's bone is the second joint; its middle sits a third
        // of the way up its bone-local triangle.
        let bone = world.get::<SkinnedMesh>(part).unwrap().joints[1];
        let held = world.get::<Transform>(bone).unwrap().to_matrix();
        let middle = (placement * held).transform_point3(Vec3::new(0.0, 1.0 / 3.0, 0.0));
        let facing = (placement * held).transform_vector3(Vec3::Z).normalize();
        let ray = Ray3d::new(middle + facing * 10.0, Dir3::new(-facing).unwrap());
        let mut system = SystemState::<Picker>::new(&mut world);
        assert!((system.get(&world).unwrap().pick(root, ray).hit.unwrap() - 10.0).abs() < 0.001);
        // The joint moves the triangle aside, out of the ray's way, though
        // the mesh is unchanged.
        world.get_mut::<Transform>(bone).unwrap().translation.x += 5.0;
        assert!(system.get(&world).unwrap().pick(root, ray).hit.is_none());
    }

    #[test]
    fn held_items_built_for_a_spawn_are_picked_through_their_holder() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        let mut materials = Assets::<StandardMaterial>::default();
        let primitive = eq_client_assets::ZonePrimitive {
            positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: Vec::new(),
            texture_coordinates: Vec::new(),
            indices: vec![0, 1, 2],
            texture: None,
            material_mode: eq_client_assets::MaterialMode::Opaque,
        };
        let held = crate::create_render_primitives(
            vec![primitive],
            &[],
            &[],
            &mut world.resource_mut::<Assets<Mesh>>(),
            &mut materials,
            true,
            crate::character::ON_A_SPAWN,
        );
        let (mesh, _) = &held[0];
        let usage = world
            .resource::<Assets<Mesh>>()
            .get(mesh)
            .unwrap()
            .asset_usage;
        assert!(usage.contains(RenderAssetUsages::MAIN_WORLD));
        // Only the item is under the holder, at its hand.
        let root = world.spawn(Transform::from_xyz(3.0, 0.0, 0.0)).id();
        let hand = world
            .spawn((Transform::from_xyz(0.0, 2.0, 0.0), Visibility::Inherited))
            .id();
        let item = world
            .spawn((Mesh3d(mesh.clone()), Transform::IDENTITY))
            .id();
        world.entity_mut(root).add_child(hand);
        world.entity_mut(hand).add_child(item);
        let ray = Ray3d::new(Vec3::new(3.0, 2.0, 10.0), Dir3::NEG_Z);
        let mut system = SystemState::<Picker>::new(&mut world);
        assert!((system.get(&world).unwrap().pick(root, ray).hit.unwrap() - 10.0).abs() < 0.001);
    }

    fn legs() -> Mesh {
        // Two thin triangles with a gap between them, as a figure's legs.
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [-1.0, 0.0, 0.0],
                [-0.6, 0.0, 0.0],
                [-0.8, 3.0, 0.0],
                [0.6, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.8, 3.0, 0.0],
            ],
        );
        mesh
    }

    #[test]
    fn a_click_between_the_legs_misses_the_triangles_but_enters_the_posed_box() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        let handle = world.resource_mut::<Assets<Mesh>>().add(legs());
        let root = world.spawn(Transform::from_xyz(5.0, 0.0, 0.0)).id();
        let leaf = world.spawn((Mesh3d(handle), Transform::IDENTITY)).id();
        world.entity_mut(root).add_child(leaf);
        let mut system = SystemState::<Picker>::new(&mut world);
        let gap = Ray3d::new(Vec3::new(5.0, 1.0, 10.0), Dir3::NEG_Z);
        let pick = system.get(&world).unwrap().pick(root, gap);
        assert_eq!(pick.hit, None);
        let posed = pick.posed.unwrap();
        assert_eq!(posed.min, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(posed.max, Vec3::new(1.0, 3.0, 0.0));
        assert!((posed.entry(gap).unwrap() - 10.0).abs() < 0.001);
        assert!((posed.middle() - Vec3::new(5.0, 1.5, 0.0)).length() < 0.001);
        // On a leg, the triangle is hit too.
        let leg = Ray3d::new(Vec3::new(4.2, 1.0, 10.0), Dir3::NEG_Z);
        assert!((system.get(&world).unwrap().pick(root, leg).hit.unwrap() - 10.0).abs() < 0.001);
        // Beside the figure, neither.
        let beside = Ray3d::new(Vec3::new(6.5, 1.0, 10.0), Dir3::NEG_Z);
        let pick = system.get(&world).unwrap().pick(root, beside);
        assert_eq!(pick.hit, None);
        assert_eq!(pick.posed.unwrap().entry(beside), None);
    }

    #[test]
    fn the_posed_box_turns_with_the_spawn_and_leaves_out_hidden_parts() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        let handle = world.resource_mut::<Assets<Mesh>>().add(triangle());
        // Turned a quarter: the triangle's width now lies along the world's Z.
        let root = world
            .spawn(
                Transform::from_xyz(5.0, 0.0, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            )
            .id();
        let body = world
            .spawn((Mesh3d(handle.clone()), Transform::IDENTITY))
            .id();
        let hidden = world
            .spawn((
                Mesh3d(handle),
                Transform::from_xyz(0.0, 10.0, 0.0),
                Visibility::Hidden,
            ))
            .id();
        world.entity_mut(root).add_children(&[body, hidden]);
        let mut system = SystemState::<Picker>::new(&mut world);
        let along_x = Ray3d::new(Vec3::new(15.0, 0.0, 0.5), Dir3::NEG_X);
        let pick = system.get(&world).unwrap().pick(root, along_x);
        let posed = pick.posed.unwrap();
        // In the spawn's own frame the box is the triangle's, the hidden
        // part ten units up left out.
        assert!((posed.min - Vec3::new(-1.0, -1.0, 0.0)).length() < 0.001);
        assert!((posed.max - Vec3::new(1.0, 1.0, 0.0)).length() < 0.001);
        assert!((posed.entry(along_x).unwrap() - 10.0).abs() < 0.001);
        // A ray from inside the box picks nothing by it.
        let inside = Ray3d::new(Vec3::new(5.0, 0.0, 0.0), Dir3::NEG_X);
        assert_eq!(posed.entry(inside), None);
    }

    #[test]
    fn a_skinned_characters_posed_box_follows_its_joints() {
        use crate::character::{AnimatedCharacter, testing};
        let mut world = World::new();
        testing::stores(&mut world);
        let prepared = testing::prepared(&mut world);
        let root = testing::drawn(&mut world, &prepared);
        let character = world.get::<AnimatedCharacter>(root).unwrap();
        let part = character.parts[0].entity;
        let bone = world.get::<SkinnedMesh>(part).unwrap().joints[1];
        let ray = Ray3d::new(Vec3::splat(1000.0), Dir3::NEG_Y);
        let mut system = SystemState::<Picker>::new(&mut world);
        let before = system.get(&world).unwrap().pick(root, ray).posed.unwrap();
        world.get_mut::<Transform>(bone).unwrap().translation.x += 5.0;
        let after = system.get(&world).unwrap().pick(root, ray).posed.unwrap();
        assert!(after.middle().distance(before.middle()) > 0.1);
    }
}

//! A zone's placed objects merged into one mesh per world-grid cell, model and
//! primitive, so the trees and rocks of a zone cost the renderer one entity
//! per batch instead of one per placement and primitive.
//!
//! Each batch keeps its model primitive's material, so it draws with the same
//! texture, colour and alpha as the placements it holds. Blended and additive
//! primitives stay one entity per placement, as their draw order is sorted by
//! distance and a merged mesh would sort as one.
use crate::object_transform;
use bevy::prelude::*;
use eq_client_assets::{MaterialMode, ZoneObject, ZonePrimitive};
use std::collections::BTreeMap;

/// The side of a batch's square cell on the ground, in world units. A
/// placement joins the cell its origin stands in. Measured on Emerald Jungle,
/// whose 24,505 placements drew as 116,483 entities: cells of 128, 256 and 512
/// make 60,687, 28,369 and 10,035, and 512 is the smallest of those near the
/// zone's entity count before its posed objects were placed.
pub(super) const CELL: f32 = 512.0;

/// One merged mesh: model `model`'s primitive `primitive`, for every placement
/// of that model in one cell, in world coordinates.
pub(super) struct Batch {
    pub(super) model: usize,
    pub(super) primitive: usize,
    pub(super) geometry: ZonePrimitive,
}

/// What a zone's placements draw as.
#[derive(Default)]
pub(super) struct Batched {
    pub(super) batches: Vec<Batch>,
    /// `(object, primitive)`: a blended or additive primitive of a placement,
    /// drawn on its own at the placement's transform.
    pub(super) apart: Vec<(usize, usize)>,
}

/// Merges `objects` by cell, model and primitive. `models[i]` is model `i`'s
/// drawn primitives, in the order the caller made their materials.
pub(super) fn batch(models: &[Vec<ZonePrimitive>], objects: &[ZoneObject], cell: f32) -> Batched {
    let mut groups: BTreeMap<(i32, i32, usize, usize), ZonePrimitive> = BTreeMap::new();
    let mut apart = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        let Some(model) = models.get(object.model) else {
            continue;
        };
        let place = Placement::new(object);
        #[allow(clippy::cast_possible_truncation)]
        let at = (
            (object.translation[0] / cell).floor() as i32,
            (object.translation[2] / cell).floor() as i32,
        );
        for (primitive, source) in model.iter().enumerate() {
            if matches!(
                source.material_mode,
                MaterialMode::Blended(_) | MaterialMode::Additive
            ) {
                apart.push((index, primitive));
                continue;
            }
            let group = groups
                .entry((at.0, at.1, object.model, primitive))
                .or_insert_with(|| ZonePrimitive {
                    positions: Vec::new(),
                    normals: Vec::new(),
                    texture_coordinates: Vec::new(),
                    indices: Vec::new(),
                    texture: source.texture,
                    material_mode: source.material_mode,
                });
            place.append(source, group);
        }
    }
    Batched {
        batches: groups
            .into_iter()
            .map(|((_, _, model, primitive), geometry)| Batch {
                model,
                primitive,
                geometry,
            })
            .collect(),
        apart,
    }
}

/// A placement's transform, as baked into vertices.
struct Placement {
    matrix: Mat4,
    /// The inverse-transpose of the transform's linear part, scaled by its
    /// determinant's magnitude, which a normal is renormalised after: defined
    /// even for a placement scaled flat along one axis.
    normals: Mat3,
    mirrored: bool,
}

impl Placement {
    fn new(object: &ZoneObject) -> Self {
        let matrix = object_transform(object).to_matrix();
        let linear = Mat3::from_mat4(matrix);
        let determinant = linear.determinant();
        // The cofactor matrix: the determinant times the inverse-transpose.
        let cofactor = Mat3::from_cols(
            linear.y_axis.cross(linear.z_axis),
            linear.z_axis.cross(linear.x_axis),
            linear.x_axis.cross(linear.y_axis),
        );
        let normals = if determinant < 0.0 {
            -cofactor
        } else {
            cofactor
        };
        Self {
            matrix,
            normals,
            mirrored: determinant < 0.0,
        }
    }

    /// Appends `source` moved by this placement to `into`. A mirroring
    /// transform turns each triangle's winding, which is reversed back so it
    /// stays counter-clockwise around its normals.
    fn append(&self, source: &ZonePrimitive, into: &mut ZonePrimitive) {
        debug_assert_eq!(
            source.indices.len() % 3,
            0,
            "a triangle list's indices come in threes"
        );
        let base = u32::try_from(into.positions.len()).expect("a batch's vertices fit u32 indices");
        into.positions.extend(
            source
                .positions
                .iter()
                .map(|&p| self.matrix.transform_point3(Vec3::from_array(p)).to_array()),
        );
        into.normals.extend(source.normals.iter().map(|&n| {
            (self.normals * Vec3::from_array(n))
                .normalize_or_zero()
                .to_array()
        }));
        into.texture_coordinates
            .extend_from_slice(&source.texture_coordinates);
        into.indices.extend(
            source
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|&[a, b, c]| {
                    let [a, b, c] = [a + base, b + base, c + base];
                    if self.mirrored { [a, c, b] } else { [a, b, c] }
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(mode: MaterialMode) -> ZonePrimitive {
        ZonePrimitive {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            texture_coordinates: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
            texture: Some(3),
            material_mode: mode,
        }
    }

    fn placed(model: usize, translation: [f32; 3], rotation: Quat, scale: [f32; 3]) -> ZoneObject {
        ZoneObject {
            model,
            translation,
            rotation: rotation.to_array(),
            scale,
        }
    }

    fn only(batched: &Batched) -> &ZonePrimitive {
        assert_eq!(batched.batches.len(), 1);
        &batched.batches[0].geometry
    }

    #[test]
    fn a_rotated_unevenly_scaled_placement_bakes_its_points_and_normals() {
        let rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 1.1);
        let object = placed(0, [10.0, 2.0, -5.0], rotation, [2.0, 0.5, 3.0]);
        let batched = batch(
            &[vec![triangle(MaterialMode::Opaque)]],
            std::slice::from_ref(&object),
            CELL,
        );
        let baked = only(&batched);
        let matrix = object_transform(&object).to_matrix();
        let normals = Mat3::from_mat4(matrix).inverse().transpose();
        let source = triangle(MaterialMode::Opaque);
        for (at, point) in baked.positions.iter().zip(&source.positions) {
            let want = matrix.transform_point3(Vec3::from_array(*point));
            assert!(
                Vec3::from_array(*at).abs_diff_eq(want, 1e-4),
                "{at:?} {want}"
            );
        }
        let want = (normals * Vec3::Z).normalize();
        for normal in &baked.normals {
            assert!(
                Vec3::from_array(*normal).abs_diff_eq(want, 1e-4),
                "{normal:?} {want}"
            );
        }
        assert_eq!(baked.indices, [0, 1, 2]);
        assert_eq!(baked.texture_coordinates, source.texture_coordinates);
        assert_eq!(baked.texture, Some(3));
    }

    /// The face's own winding, by its corners, agrees with its baked normal.
    fn wound_with_normal(geometry: &ZonePrimitive) -> bool {
        let corner = |i: usize| Vec3::from_array(geometry.positions[geometry.indices[i] as usize]);
        let face = (corner(1) - corner(0)).cross(corner(2) - corner(0));
        face.dot(Vec3::from_array(geometry.normals[0])) > 0.0
    }

    #[test]
    fn a_mirrored_placement_keeps_its_faces_wound_round_their_normals() {
        let rotation = Quat::from_rotation_y(0.3);
        let mirrored = placed(0, [0.0; 3], rotation, [-1.5, 1.0, 1.0]);
        let batched = batch(&[vec![triangle(MaterialMode::Masked)]], &[mirrored], CELL);
        let baked = only(&batched);
        assert_eq!(baked.indices, [0, 2, 1]);
        assert!(wound_with_normal(baked));

        let upright = placed(0, [0.0; 3], rotation, [1.5, 1.0, 1.0]);
        let batched = batch(&[vec![triangle(MaterialMode::Masked)]], &[upright], CELL);
        assert!(wound_with_normal(only(&batched)));
    }

    #[test]
    fn placements_merge_by_cell_model_and_primitive() {
        let models = [
            vec![
                triangle(MaterialMode::Opaque),
                triangle(MaterialMode::Masked),
            ],
            vec![triangle(MaterialMode::Opaque)],
        ];
        let objects = [
            placed(0, [1.0, 0.0, 1.0], Quat::IDENTITY, [1.0; 3]),
            placed(0, [CELL - 1.0, 9.0, 3.0], Quat::IDENTITY, [1.0; 3]),
            placed(0, [CELL + 1.0, 0.0, 1.0], Quat::IDENTITY, [1.0; 3]),
            placed(1, [2.0, 0.0, 2.0], Quat::IDENTITY, [1.0; 3]),
            placed(7, [2.0, 0.0, 2.0], Quat::IDENTITY, [1.0; 3]),
        ];
        let batched = batch(&models, &objects, CELL);
        let mut keys: Vec<_> = batched
            .batches
            .iter()
            .map(|b| (b.model, b.primitive, b.geometry.positions.len()))
            .collect();
        keys.sort_unstable();
        // Model 0 in the first cell twice and the next once, per primitive;
        // model 1 once; the unknown model 7 draws nothing.
        assert_eq!(
            keys,
            [(0, 0, 3), (0, 0, 6), (0, 1, 3), (0, 1, 6), (1, 0, 3)]
        );
        let shared = batched
            .batches
            .iter()
            .find(|b| b.model == 0 && b.primitive == 0 && b.geometry.positions.len() == 6)
            .unwrap();
        assert_eq!(shared.geometry.indices, [0, 1, 2, 3, 4, 5]);
        assert_eq!(shared.geometry.positions[3], [CELL - 1.0, 9.0, 3.0]);
        assert_eq!(shared.geometry.material_mode, MaterialMode::Opaque);
        assert_eq!(batched.apart, []);
    }

    #[test]
    fn blended_and_additive_primitives_stay_with_their_placement() {
        let models = [vec![
            triangle(MaterialMode::Opaque),
            triangle(MaterialMode::Blended(eq_client_assets::BlendOpacity::Half)),
            triangle(MaterialMode::Additive),
        ]];
        let objects = [
            placed(0, [1.0, 0.0, 1.0], Quat::IDENTITY, [1.0; 3]),
            placed(0, [2.0, 0.0, 2.0], Quat::IDENTITY, [1.0; 3]),
        ];
        let batched = batch(&models, &objects, CELL);
        assert_eq!(batched.batches.len(), 1);
        assert_eq!(batched.batches[0].geometry.positions.len(), 6);
        assert_eq!(batched.apart, [(0, 1), (0, 2), (1, 1), (1, 2)]);
    }
}

//! The installed item models on the GPU, shared by everything that draws an
//! item: items on the ground and items held in hand.
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// A drawable item model.
pub(super) struct Shape {
    pub(super) primitives: super::doors::Primitives,
    /// Bounds in model space, when the model has geometry.
    pub(super) bounds: Option<(Vec3, Vec3)>,
    /// Whether the model is flat like a shield.
    pub(super) flat: bool,
}

/// The installed item models, opened when the first one is needed, and the
/// shapes already built, which outlive zones.
#[derive(Resource, Default)]
pub(super) struct ItemLibrary {
    models: Option<eq_client_assets::items::ItemModels>,
    opened: bool,
    shapes: HashMap<String, Option<Arc<Shape>>>,
}

/// The asset stores systems that build item shapes take as parameters.
pub(super) type GpuAssets<'a> = (
    ResMut<'a, Assets<Image>>,
    ResMut<'a, Assets<Mesh>>,
    ResMut<'a, Assets<StandardMaterial>>,
);

/// Where item shapes are built.
pub(super) type Gpu<'a, 'b, 'c> = (
    &'a mut Assets<Image>,
    &'b mut Assets<Mesh>,
    &'c mut Assets<StandardMaterial>,
);

impl ItemLibrary {
    /// A model's shape, such as `IT63`, built on first use; None when not installed.
    pub(super) fn shape(
        &mut self,
        key: &str,
        directory: Option<&Path>,
        gpu: Gpu,
    ) -> Option<Arc<Shape>> {
        if let Some(shape) = self.shapes.get(key) {
            return shape.clone();
        }
        if !self.opened {
            self.opened = true;
            self.models = directory.and_then(|directory| {
                eq_client_assets::items::ItemModels::open(directory)
                    .inspect_err(|error| warn!("Item models unavailable: {error}"))
                    .ok()
            });
        }
        let model = self.models.as_mut().and_then(|models| {
            models
                .model(key)
                .inspect_err(|error| warn!("Item model {key} unavailable: {error}"))
                .ok()
                .flatten()
        });
        let shape = model.map(|model| Arc::new(build(model, gpu)));
        self.shapes.insert(key.to_owned(), shape.clone());
        shape
    }

    /// Adds an already built shape, for tests.
    #[cfg(test)]
    pub(super) fn insert(&mut self, key: &str, shape: Shape) {
        self.opened = true;
        self.shapes.insert(key.to_owned(), Some(Arc::new(shape)));
    }
}

fn build(model: eq_client_assets::items::ItemModel, (images, meshes, materials): Gpu) -> Shape {
    let bounds = bounds(model.primitives.iter().flat_map(|part| &part.positions));
    let textures = super::create_texture_images(model.textures, images);
    Shape {
        // Held items hang under a spawn. A shape is built once and shared
        // with every holder and the ground, so it keeps its data from the
        // start: data moved to the render world doesn't come back.
        primitives: super::create_render_primitives(
            model.primitives,
            &textures,
            meshes,
            materials,
            true,
            super::character::ON_A_SPAWN,
        ),
        bounds,
        flat: model.flat,
    }
}

/// The smallest box around some points.
pub(super) fn bounds<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> Option<(Vec3, Vec3)> {
    points
        .map(|point| Vec3::from_array(*point))
        .fold(None, |bounds, point| {
            Some(bounds.map_or((point, point), |(min, max): (Vec3, Vec3)| {
                (min.min(point), max.max(point))
            }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use eq_client_assets::{MaterialMode, ZonePrimitive, items::ItemModel};

    #[test]
    fn shapes_keep_their_vertices_where_clicks_are_picked() {
        let mut images = Assets::<Image>::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let model = ItemModel {
            name: "IT1".into(),
            primitives: vec![ZonePrimitive {
                positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
                normals: Vec::new(),
                texture_coordinates: Vec::new(),
                indices: vec![0, 1, 2],
                texture: None,
                material_mode: MaterialMode::Opaque,
            }],
            textures: Vec::new(),
            flat: false,
        };
        let shape = build(model, (&mut images, &mut meshes, &mut materials));
        assert_eq!(shape.primitives.len(), 1);
        for (mesh, _) in &shape.primitives {
            let usage = meshes.get(mesh).unwrap().asset_usage;
            assert!(
                usage.contains(RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD)
            );
        }
    }
}

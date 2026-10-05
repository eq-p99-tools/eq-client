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
/// shapes built during the current admission. Archive access can outlive zones.
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
    /// Releases cached GPU shapes while keeping the installed archive open.
    pub(super) fn clear_shapes(&mut self) {
        self.shapes.clear();
    }

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
        primitives: super::create_render_primitives(
            model.primitives,
            &textures,
            meshes,
            materials,
            true,
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

    #[test]
    fn leaving_releases_cached_shapes_but_not_active_owners() {
        let mut library = ItemLibrary::default();
        library.insert(
            "IT1",
            Shape {
                primitives: Vec::new(),
                bounds: None,
                flat: false,
            },
        );
        let active = library.shapes["IT1"].as_ref().unwrap().clone();
        let lifetime = Arc::downgrade(&active);
        library.clear_shapes();
        assert!(library.shapes.is_empty());
        assert!(library.opened);
        assert!(lifetime.upgrade().is_some());
        drop(active);
        assert!(lifetime.upgrade().is_none());
    }
}

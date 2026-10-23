//! The installed item models on the GPU, shared by everything that draws an
//! item: items on the ground and items held in hand.
use crate::asset_loads::{Asset, Loads};
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// A drawable item model.
pub(super) struct Shape {
    pub(super) primitives: super::doors::Primitives,
    /// Bounds in model space, when the model has geometry.
    pub(super) bounds: Option<(Vec3, Vec3)>,
    /// Whether the model is flat like a shield.
    pub(super) flat: bool,
}

/// The installed item models, opened when the first one is needed, and the
/// shapes built during this admission. Archive access can outlive zones.
#[derive(Resource, Default)]
pub(super) struct ItemLibrary {
    source: Arc<Mutex<Source>>,
    directory: Option<PathBuf>,
    loads: Loads<String, eq_client_assets::items::ItemModel>,
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

/// Archive ownership is shared only by background jobs; the frame never locks it.
#[derive(Default)]
struct Source {
    models: Option<eq_client_assets::items::ItemModels>,
    attempted: Option<u64>,
}

impl Source {
    fn load(
        &mut self,
        directory: &Path,
        key: &str,
        generation: u64,
    ) -> Option<eq_client_assets::items::ItemModel> {
        if self.models.is_none() && self.attempted != Some(generation) {
            self.attempted = Some(generation);
            self.models = eq_client_assets::items::ItemModels::open(directory)
                .inspect_err(|error| warn!("Item models unavailable: {error}"))
                .ok();
        }
        self.models
            .as_mut()?
            .model(key)
            .inspect_err(|error| warn!("Item model {key} unavailable: {error}"))
            .ok()
            .flatten()
    }
}

impl ItemLibrary {
    /// Releases cached shapes and obsolete reads; active entities retain handles.
    pub(super) fn clear_shapes(&mut self) {
        self.shapes.clear();
        self.loads.clear();
    }

    /// Looks up a shape or requests its decode without doing archive work here.
    pub(super) fn shape(&mut self, key: &str, directory: Option<&Path>) -> Asset<Arc<Shape>> {
        if self.directory.as_deref() != directory {
            self.clear_shapes();
            self.source = Arc::default();
            self.directory = directory.map(Path::to_owned);
        }
        if let Some(shape) = self.shapes.get(key) {
            return shape.clone().map_or(Asset::Missing, Asset::Ready);
        }
        let Some(directory) = self.directory.clone() else {
            self.shapes.insert(key.to_owned(), None);
            return Asset::Missing;
        };
        let source = Arc::clone(&self.source);
        let name = key.to_owned();
        let generation = self.loads.generation();
        self.loads.request(name.clone(), move || {
            source
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .load(&directory, &name, generation)
        });
        Asset::Pending
    }

    /// Adds an already built shape, for tests.
    #[cfg(test)]
    pub(super) fn insert(&mut self, key: &str, shape: Shape) {
        self.shapes.insert(key.to_owned(), Some(Arc::new(shape)));
    }
}

/// Installs at most one decoded item each frame, before ground and held items.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn poll(
    mut library: ResMut<ItemLibrary>,
    (mut images, mut meshes, mut materials): GpuAssets,
) {
    if let Some((key, model)) = library.loads.poll() {
        let shape =
            model.map(|model| Arc::new(build(model, (&mut images, &mut meshes, &mut materials))));
        library.shapes.insert(key, shape);
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
        let source = Arc::clone(&library.source);
        library.clear_shapes();
        assert!(library.shapes.is_empty());
        assert!(Arc::ptr_eq(&source, &library.source));
        assert!(lifetime.upgrade().is_some());
        drop(active);
        assert!(lifetime.upgrade().is_none());
    }
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

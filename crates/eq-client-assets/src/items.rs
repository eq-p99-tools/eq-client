//! Item models from the installed `gequip*.s3d` archives: what EQ draws for an
//! item on the ground, such as `IT63`, the small bag servers show by default.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::Path;

use libeq::pfs::PfsReader;

use crate::{LoadError, StagedPrimitive, ZonePrimitive, ZoneTexture, model_key};

/// The classic item archives, searched in this order; each holds a WLD of the
/// same name. An installation may lack the later ones.
const ARCHIVES: [&str; 7] = [
    "gequip", "gequip2", "gequip3", "gequip4", "gequip5", "gequip6", "gequip8",
];

/// One item model with its own textures, ready to draw.
#[derive(Clone, Debug)]
pub struct ItemModel {
    /// Model name without `_ACTORDEF`, such as `IT63`.
    pub name: String,
    /// Geometry grouped by material; texture indexes refer to `textures`.
    pub primitives: Vec<ZonePrimitive>,
    /// The textures this model uses.
    pub textures: Vec<ZoneTexture>,
}

struct Entry {
    archive: usize,
    primitives: Vec<StagedPrimitive>,
}

/// The installed item models by name. Opening reads every model's geometry;
/// textures are decoded only when a model is built.
pub struct ItemModels {
    archives: Vec<PfsReader<File>>,
    models: HashMap<String, Entry>,
}

impl ItemModels {
    /// Opens the installed item archives and reads their static models. Models
    /// with a skeleton instead of a single mesh are left out.
    ///
    /// # Errors
    /// Returns [`LoadError`] when an installed archive or its WLD cannot be read.
    pub fn open(eq_directory: &Path) -> Result<Self, LoadError> {
        let mut archives = Vec::new();
        let mut models = HashMap::new();
        for stem in ARCHIVES {
            let path = eq_directory.join(format!("{stem}.s3d"));
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => return Err(LoadError::OpenArchive { path, source }),
            };
            let mut archive = PfsReader::open(file)?;
            let world_name = format!("{stem}.wld");
            let bytes = archive
                .get(&world_name)?
                .ok_or_else(|| LoadError::MissingWorld(world_name.clone()))?;
            let (_, world) = crate::parse_wld(&bytes, &world_name)?;
            for model in world.models() {
                let (Some(name), Some(mesh)) = (model.name(), model.mesh()) else {
                    continue;
                };
                let primitives = crate::stage_mesh(&mesh);
                if !primitives.is_empty() {
                    // Earlier archives win, as the list above orders them.
                    models.entry(model_key(name)).or_insert(Entry {
                        archive: archives.len(),
                        primitives,
                    });
                }
            }
            archives.push(archive);
        }
        Ok(Self { archives, models })
    }

    /// Number of models available.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether no item archive was installed or none held a static model.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Whether a model of this name, such as `IT63_ACTORDEF`, is available.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.models.contains_key(&model_key(name))
    }

    /// Builds a model with its textures; None when no archive has it.
    ///
    /// # Errors
    /// Returns [`LoadError`] when a texture cannot be read or decoded.
    pub fn model(&mut self, name: &str) -> Result<Option<ItemModel>, LoadError> {
        let key = model_key(name);
        let Some(entry) = self.models.get(&key) else {
            return Ok(None);
        };
        let archive = &mut self.archives[entry.archive];
        let mut textures = Vec::new();
        let mut indices = HashMap::new();
        let primitives = entry
            .primitives
            .iter()
            .cloned()
            .map(|primitive| primitive.realize(archive, &mut textures, &mut indices))
            .collect::<Result<_, _>>()?;
        Ok(Some(ItemModel {
            name: key,
            primitives,
            textures,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installation_without_item_archives_has_no_models() {
        let mut models = ItemModels::open(Path::new("no such installation")).unwrap();
        assert!(models.is_empty());
        assert!(!models.contains("IT63_ACTORDEF"));
        assert!(models.model("IT63_ACTORDEF").unwrap().is_none());
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn reads_the_installed_item_models() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let started = std::time::Instant::now();
        let mut models = ItemModels::open(Path::new(&install)).unwrap();
        let opened = started.elapsed();
        let bag = models.model("it63_ACTORDEF").unwrap().expect("IT63");
        let vertices: usize = bag.primitives.iter().map(|part| part.positions.len()).sum();
        let bounds = bag.primitives.iter().flat_map(|part| &part.positions).fold(
            ([f32::MAX; 3], [f32::MIN; 3]),
            |(low, high), p| {
                (
                    std::array::from_fn(|axis| low[axis].min(p[axis])),
                    std::array::from_fn(|axis| high[axis].max(p[axis])),
                )
            },
        );
        println!(
            "{} models opened in {opened:?}; IT63: {} primitives, {vertices} vertices, {} textures, bounds {bounds:?}",
            models.len(),
            bag.primitives.len(),
            bag.textures.len()
        );
        assert!(models.len() > 100);
    }
}

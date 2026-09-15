#![doc = "Owned, renderer-independent zone assets loaded from a local EQ installation."]

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use image::ImageError;
use libeq::pfs::PfsReader;
use libeq::wld::parser::{DrawStyle, MaterialType, RenderMethod};
use thiserror::Error;

/// A decoded image referenced by a zone material.
#[derive(Clone, Debug)]
pub struct ZoneTexture {
    /// Source filename inside the S3D archive.
    pub name: String,
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
    /// Pixels in row-major RGBA8 format.
    pub rgba8: Vec<u8>,
}

/// One draw call from a classic WLD zone.
#[derive(Clone, Debug)]
pub struct ZonePrimitive {
    /// Vertex positions in right-handed, Y-up render coordinates.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals in the same coordinate system as `positions`.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates.
    pub texture_coordinates: Vec<[f32; 2]>,
    /// Triangle-list indices.
    pub indices: Vec<u32>,
    /// Optional decoded diffuse texture.
    pub texture: Option<usize>,
}

/// Renderer-independent geometry and textures for a zone.
#[derive(Clone, Debug)]
pub struct ZoneAsset {
    /// Zone short name, such as `ecommons`.
    pub short_name: String,
    /// Geometry grouped by WLD material.
    pub primitives: Vec<ZonePrimitive>,
    /// Deduplicated diffuse textures used by `primitives`.
    pub textures: Vec<ZoneTexture>,
}

impl ZoneAsset {
    /// Returns the number of triangles in all primitives.
    pub fn triangle_count(&self) -> usize {
        self.primitives
            .iter()
            .map(|part| part.indices.len() / 3)
            .sum()
    }

    /// Returns the axis-aligned bounds of all loaded geometry.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut positions = self.primitives.iter().flat_map(|part| &part.positions);
        let first = *positions.next()?;
        Some(
            positions.fold((first, first), |(mut min, mut max), position| {
                for axis in 0..3 {
                    min[axis] = min[axis].min(position[axis]);
                    max[axis] = max[axis].max(position[axis]);
                }
                (min, max)
            }),
        )
    }
}

/// Failures while locating or decoding locally supplied game assets.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The requested zone name cannot safely name a local file.
    #[error("invalid zone short name: {0}")]
    InvalidZoneName(String),
    /// The zone archive could not be opened.
    #[error("could not open {path}: {source}")]
    OpenArchive {
        /// Archive path.
        path: PathBuf,
        /// Operating-system error.
        source: io::Error,
    },
    /// The S3D archive could not be parsed.
    #[error("could not read S3D archive: {0}")]
    Archive(#[from] libeq::pfs::Error),
    /// The expected WLD file is absent from the archive.
    #[error("archive does not contain {0}")]
    MissingWorld(String),
    /// The WLD payload could not be parsed.
    #[error("could not parse WLD data: {0}")]
    ParseWorld(String),
    /// A texture could not be decoded.
    #[error("could not decode texture {name}: {source}")]
    DecodeTexture {
        /// Source filename inside the archive.
        name: String,
        /// Decoder error.
        source: ImageError,
    },
}

/// Loads a classic S3D zone from an `EverQuest` installation.
///
/// # Errors
///
/// Returns [`LoadError`] when the name is unsafe, files are missing or
/// unreadable, or an archive, world, or texture cannot be decoded.
pub fn load_zone(eq_directory: &Path, short_name: &str) -> Result<ZoneAsset, LoadError> {
    if short_name.is_empty()
        || !short_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(LoadError::InvalidZoneName(short_name.to_owned()));
    }

    let archive_path = eq_directory.join(format!("{short_name}.s3d"));
    let file = File::open(&archive_path).map_err(|source| LoadError::OpenArchive {
        path: archive_path,
        source,
    })?;
    let mut archive = PfsReader::open(file)?;
    let world_name = format!("{short_name}.wld");
    let world_bytes = archive
        .get(&world_name)?
        .ok_or_else(|| LoadError::MissingWorld(world_name.clone()))?;
    let world =
        libeq::wld::load(&world_bytes).map_err(|error| LoadError::ParseWorld(error.to_string()))?;

    let mut textures = Vec::new();
    let mut texture_indices = HashMap::<String, usize>::new();
    let mut primitives = Vec::new();

    for mesh in world.meshes() {
        let center = mesh.center();
        for primitive in mesh.primitives() {
            let material = primitive.material();
            if !should_render(*material.render_method()) {
                continue;
            }
            let positions = primitive
                .positions()
                .into_iter()
                .map(|position| {
                    [
                        position[0] + center.0,
                        position[1] + center.1,
                        position[2] + center.2,
                    ]
                })
                .collect();
            let texture = material
                .base_color_texture()
                .and_then(|value| value.source())
                .and_then(|name| {
                    load_texture(&mut archive, &name, &mut textures, &mut texture_indices)
                        .transpose()
                })
                .transpose()?;

            primitives.push(ZonePrimitive {
                positions,
                normals: primitive.normals(),
                texture_coordinates: primitive.texture_coordinates(),
                indices: primitive.indices(),
                texture,
            });
        }
    }

    Ok(ZoneAsset {
        short_name: short_name.to_owned(),
        primitives,
        textures,
    })
}

fn should_render(method: RenderMethod) -> bool {
    !matches!(
        method,
        RenderMethod::Standard {
            draw_style: DrawStyle::Transparent,
            ..
        } | RenderMethod::UserDefined {
            material_type: MaterialType::Boundary
                | MaterialType::InvisibleUnknown
                | MaterialType::InvisibleUnknown2
                | MaterialType::InvisibleUnknown3,
        }
    )
}

fn load_texture(
    archive: &mut PfsReader<File>,
    name: &str,
    textures: &mut Vec<ZoneTexture>,
    indices: &mut HashMap<String, usize>,
) -> Result<Option<usize>, LoadError> {
    if let Some(index) = indices.get(name) {
        return Ok(Some(*index));
    }
    let Some(bytes) = archive.get(name)? else {
        return Ok(None);
    };
    let decoded = image::load_from_memory(&bytes).map_err(|source| LoadError::DecodeTexture {
        name: name.to_owned(),
        source,
    })?;
    let rgba = decoded.into_rgba8();
    let index = textures.len();
    textures.push(ZoneTexture {
        name: name.to_owned(),
        width: rgba.width(),
        height: rgba.height(),
        rgba8: rgba.into_raw(),
    });
    indices.insert(name.to_owned(), index);
    Ok(Some(index))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use libeq::wld::parser::{
        DrawStyle, Lighting, MaterialType, RenderMethod, Shading, TextureStyle,
    };

    use super::{LoadError, load_zone, should_render};

    #[test]
    fn rejects_names_that_could_escape_the_install_directory() {
        let result = load_zone(Path::new("unused"), "../secrets");
        assert!(matches!(result, Err(LoadError::InvalidZoneName(_))));
    }

    #[test]
    fn excludes_boundary_and_non_drawing_materials() {
        let transparent = RenderMethod::Standard {
            draw_style: DrawStyle::Transparent,
            lighting: Lighting::ZeroIntensity,
            shading: Shading::None1,
            texture_style: TextureStyle::None,
            unknown_bits: 0,
        };
        let boundary = RenderMethod::UserDefined {
            material_type: MaterialType::Boundary,
        };
        let diffuse = RenderMethod::UserDefined {
            material_type: MaterialType::Diffuse,
        };

        assert!(!should_render(transparent));
        assert!(!should_render(boundary));
        assert!(should_render(diffuse));
    }
}

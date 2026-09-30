#![doc = "Owned, renderer-independent zone assets loaded from a local EQ installation."]

pub mod characters;
pub mod regions;
pub mod spells;
pub mod ui;

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use image::ImageError;
use libeq::pfs::PfsReader;
use libeq::wld::parser::{DrawStyle, MaterialType, RenderMethod, TextureStyle};
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
    /// RGB palette entry treated as transparent by masked classic materials.
    pub color_key: Option<[u8; 3]>,
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
    /// How the material combines with the scene behind it.
    pub material_mode: MaterialMode,
}

/// Renderer-independent material behavior used by classic WLD meshes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaterialMode {
    /// Fully opaque diffuse rendering.
    Opaque,
    /// Binary transparency using the texture's black color key.
    Masked,
    /// Conventional alpha blending at the given fixed opacity.
    Blended(BlendOpacity),
    /// Additive transparent rendering.
    Additive,
}

/// Fixed opacity values encoded by classic WLD materials.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BlendOpacity {
    /// 25 percent opacity.
    Quarter,
    /// 50 percent opacity.
    Half,
    /// 75 percent opacity.
    ThreeQuarters,
}

/// A reusable static model loaded from `<zone>_obj.s3d`.
#[derive(Clone, Debug)]
pub struct ZoneModel {
    /// Actor definition name referenced by zone object placements.
    pub name: String,
    /// Model geometry grouped by material.
    pub primitives: Vec<ZonePrimitive>,
    /// Solid triangles, including invisible collision surfaces.
    pub collision: Vec<[[f32; 3]; 3]>,
}

/// One placed instance from `objects.wld`.
#[derive(Clone, Debug)]
pub struct ZoneObject {
    /// Index into [`ZoneAsset::models`].
    pub model: usize,
    /// World translation in right-handed, Y-up coordinates.
    pub translation: [f32; 3],
    /// Euler rotation in degrees around the X, Y, and Z axes.
    pub rotation_degrees: [f32; 3],
    /// Scale along the X, Y, and Z axes.
    pub scale: [f32; 3],
}

/// Renderer-independent geometry and textures for a zone.
#[derive(Clone, Debug)]
pub struct ZoneAsset {
    /// Local boundary volumes; destination routing comes from the server.
    pub regions: regions::ZoneRegions,
    /// Zone short name, such as `ecommons`.
    pub short_name: String,
    /// Geometry grouped by WLD material.
    pub primitives: Vec<ZonePrimitive>,
    /// Deduplicated diffuse textures used by `primitives`.
    pub textures: Vec<ZoneTexture>,
    /// Reusable static models referenced by placed objects.
    pub models: Vec<ZoneModel>,
    /// Static object placements in the zone.
    pub objects: Vec<ZoneObject>,
    /// Solid terrain triangles independent of material visibility.
    pub collision: Vec<[[f32; 3]; 3]>,
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
    let (doc, world) = parse_wld(&world_bytes, &world_name)?;
    let regions = regions::ZoneRegions::from_doc(&doc)?;

    let mut textures = Vec::new();
    let mut texture_indices = HashMap::<String, usize>::new();
    let mut primitives = Vec::new();

    for mesh in world.meshes() {
        let center = mesh.center();
        for primitive in mesh.primitives() {
            let material = primitive.material();
            let Some(material_mode) = material_mode(*material.render_method()) else {
                continue;
            };
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
                material_mode,
            });
        }
    }

    let collision = world
        .meshes()
        .flat_map(|mesh| mesh_collision(&mesh))
        .collect();
    let placements = load_object_placements(&mut archive)?;
    let (models, objects) = load_object_models(
        eq_directory,
        short_name,
        placements,
        &mut textures,
        &mut texture_indices,
    )?;

    Ok(ZoneAsset {
        regions,
        short_name: short_name.to_owned(),
        primitives,
        textures,
        models,
        objects,
        collision,
    })
}

fn material_mode(method: RenderMethod) -> Option<MaterialMode> {
    match method {
        RenderMethod::Standard {
            draw_style: DrawStyle::Transparent,
            ..
        }
        | RenderMethod::UserDefined {
            material_type:
                MaterialType::Boundary
                | MaterialType::InvisibleUnknown
                | MaterialType::InvisibleUnknown2
                | MaterialType::InvisibleUnknown3,
        } => None,
        RenderMethod::Standard {
            texture_style:
                TextureStyle::TransTexture1
                | TextureStyle::TransTexture2
                | TextureStyle::TransTexture4
                | TextureStyle::TransTexture5,
            ..
        }
        | RenderMethod::UserDefined {
            material_type: MaterialType::TransparentMasked | MaterialType::TransparentMaskedPassable,
        } => Some(MaterialMode::Masked),
        RenderMethod::UserDefined {
            material_type: MaterialType::Transparent25,
        } => Some(MaterialMode::Blended(BlendOpacity::Quarter)),
        RenderMethod::UserDefined {
            material_type: MaterialType::Transparent50,
        } => Some(MaterialMode::Blended(BlendOpacity::Half)),
        RenderMethod::UserDefined {
            material_type: MaterialType::Transparent75,
        } => Some(MaterialMode::Blended(BlendOpacity::ThreeQuarters)),
        RenderMethod::UserDefined {
            material_type:
                MaterialType::TransparentAdditive
                | MaterialType::TransparentAdditiveUnlit
                | MaterialType::TransparentAdditiveUnlitSkydome,
        } => Some(MaterialMode::Additive),
        _ => Some(MaterialMode::Opaque),
    }
}

struct ObjectPlacement {
    model_name: String,
    translation: [f32; 3],
    rotation_degrees: [f32; 3],
    scale: [f32; 3],
}

/// Parses a WLD file into libeq's document and its model view. Building the view
/// (`libeq::wld::load`) panics on data it cannot parse, which would end the client
/// while zoning, so it is built only from bytes that already parsed.
fn parse_wld(
    bytes: &[u8],
    name: &str,
) -> Result<(libeq::wld::parser::WldDoc, libeq::wld::Wld), LoadError> {
    let doc = libeq::wld::parser::WldDoc::parse(bytes)
        .map_err(|_| LoadError::ParseWorld(format!("{name} could not be parsed")))?;
    let world =
        libeq::wld::load(bytes).map_err(|error| LoadError::ParseWorld(error.to_string()))?;
    Ok((doc, world))
}

fn load_object_placements(
    archive: &mut PfsReader<File>,
) -> Result<Vec<ObjectPlacement>, LoadError> {
    let Some(bytes) = archive.get("objects.wld")? else {
        return Ok(Vec::new());
    };
    let (_, world) = parse_wld(&bytes, "objects.wld")?;

    Ok(world
        .objects()
        .filter_map(|object| {
            let model_name = object.model_name()?.to_owned();
            let (x, y, z) = object.center();
            let (rotation_x, rotation_y, rotation_z) = object.rotation();
            let (_, uniform_scale) = object.scale();
            let uniform_scale = nonzero_scale(uniform_scale);
            Some(ObjectPlacement {
                model_name,
                translation: [x, y, z],
                rotation_degrees: [rotation_x, rotation_y, rotation_z],
                scale: [uniform_scale; 3],
            })
        })
        .collect())
}

fn load_object_models(
    eq_directory: &Path,
    short_name: &str,
    placements: Vec<ObjectPlacement>,
    textures: &mut Vec<ZoneTexture>,
    texture_indices: &mut HashMap<String, usize>,
) -> Result<(Vec<ZoneModel>, Vec<ZoneObject>), LoadError> {
    let archive_path = eq_directory.join(format!("{short_name}_obj.s3d"));
    let file = match File::open(&archive_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(source) => {
            return Err(LoadError::OpenArchive {
                path: archive_path,
                source,
            });
        }
    };
    let mut archive = PfsReader::open(file)?;
    let world_name = format!("{short_name}_obj.wld");
    let world_bytes = archive
        .get(&world_name)?
        .ok_or_else(|| LoadError::MissingWorld(world_name.clone()))?;
    let (_, world) = parse_wld(&world_bytes, &world_name)?;

    let mut models = Vec::new();
    let mut model_indices = HashMap::new();
    for model in world.models() {
        let (Some(name), Some(mesh)) = (model.name(), model.mesh()) else {
            continue;
        };
        let primitives = load_mesh_primitives(&mesh, &mut archive, textures, texture_indices)?;
        if primitives.is_empty() {
            continue;
        }
        model_indices.insert(name.to_owned(), models.len());
        models.push(ZoneModel {
            name: name.to_owned(),
            primitives,
            collision: mesh_collision(&mesh),
        });
    }

    let objects = placements
        .into_iter()
        .filter_map(|placement| {
            let model = *model_indices.get(&placement.model_name)?;
            Some(ZoneObject {
                model,
                translation: placement.translation,
                rotation_degrees: placement.rotation_degrees,
                scale: placement.scale,
            })
        })
        .collect();

    Ok((models, objects))
}

fn load_mesh_primitives(
    mesh: &libeq::wld::Mesh<'_>,
    archive: &mut PfsReader<File>,
    textures: &mut Vec<ZoneTexture>,
    texture_indices: &mut HashMap<String, usize>,
) -> Result<Vec<ZonePrimitive>, LoadError> {
    let center = mesh.center();
    let mut primitives = Vec::new();
    for primitive in mesh.primitives() {
        let (material_mode, texture_name) = {
            let material = primitive.material();
            let Some(material_mode) = material_mode(*material.render_method()) else {
                continue;
            };
            let texture_name = material
                .base_color_texture()
                .and_then(|value| value.source());
            (material_mode, texture_name)
        };
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
        let texture = texture_name
            .map(|name| load_texture(archive, &name, textures, texture_indices))
            .transpose()?
            .flatten();
        primitives.push(ZonePrimitive {
            positions,
            normals: primitive.normals(),
            texture_coordinates: primitive.texture_coordinates(),
            indices: primitive.indices(),
            texture,
            material_mode,
        });
    }
    Ok(primitives)
}

/// Reads the WLD solid-face flags independently of visible material batches.
fn mesh_collision(mesh: &libeq::wld::Mesh<'_>) -> Vec<[[f32; 3]; 3]> {
    let positions = mesh.positions();
    let center = mesh.center();
    mesh.collision_indices()
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|indices| {
            let mut triangle = [[0.0; 3]; 3];
            for (out, index) in triangle.iter_mut().zip(indices) {
                let p = positions.get(*index as usize)?;
                *out = [p[0] + center.0, p[1] + center.1, p[2] + center.2];
            }
            Some(triangle)
        })
        .collect()
}

fn nonzero_scale(value: f32) -> f32 {
    if value.abs() <= f32::EPSILON {
        1.0
    } else {
        value
    }
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
        color_key: bmp_color_key(&bytes),
    });
    indices.insert(name.to_owned(), index);
    Ok(Some(index))
}

fn bmp_color_key(bytes: &[u8]) -> Option<[u8; 3]> {
    if bytes.get(..2)? != b"BM" {
        return None;
    }
    let dib_size = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?) as usize;
    let bits_per_pixel = u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?);
    let pixel_offset = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?) as usize;
    let palette_offset = 14_usize.checked_add(dib_size)?;
    if bits_per_pixel > 8 || palette_offset.checked_add(4)? > pixel_offset {
        return None;
    }
    let entry = bytes.get(palette_offset..palette_offset + 4)?;
    Some([entry[2], entry[1], entry[0]])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use libeq::wld::parser::{
        DrawStyle, Lighting, MaterialType, RenderMethod, Shading, TextureStyle,
    };

    use super::{LoadError, MaterialMode, bmp_color_key, load_zone, material_mode};

    #[test]
    fn unparseable_world_files_are_errors_not_crashes() {
        assert!(matches!(
            super::parse_wld(b"not a world file", "broken.wld"),
            Err(LoadError::ParseWorld(_))
        ));
    }

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

        assert_eq!(material_mode(transparent), None);
        assert_eq!(material_mode(boundary), None);
        assert_eq!(material_mode(diffuse), Some(MaterialMode::Opaque));
    }

    #[test]
    fn reads_the_first_indexed_bmp_palette_color_as_the_mask_key() {
        let mut bmp = vec![0_u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&58_u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp[28..30].copy_from_slice(&8_u16.to_le_bytes());
        bmp[54..58].copy_from_slice(&[3, 5, 8, 0]);

        assert_eq!(bmp_color_key(&bmp), Some([8, 5, 3]));
        assert_eq!(bmp_color_key(b"not a bitmap"), None);
    }
}

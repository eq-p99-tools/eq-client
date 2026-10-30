#![doc = "Owned, renderer-independent zone assets loaded from a local EQ installation."]

pub mod characters;
pub mod clock;
pub mod falls;
pub mod items;
pub mod maps;
pub mod regions;
pub mod sidl;
mod skeleton;
pub mod sounds;
pub mod spells;
pub mod strings;
#[cfg(test)]
mod test_world;
pub mod ui;

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
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
    /// Vertex positions in the renderer's frame (see `eq_client_axes`).
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals in the same coordinate system as `positions`.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates.
    pub texture_coordinates: Vec<[f32; 2]>,
    /// Triangle-list indices, wound counter-clockwise around the normals.
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
    /// Translation in the renderer's frame.
    pub translation: [f32; 3],
    /// Rotation in the renderer's frame, as an `(x, y, z, w)` quaternion.
    pub rotation: [f32; 4],
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
    /// Placements left out because the zone's object archive defines nothing
    /// drawn under their model's name.
    pub objects_unplaced: usize,
    /// Skins of placed objects left out because their vertices aren't each
    /// on one bone of the object's skeleton.
    pub object_skins_skipped: usize,
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
    /// The zone's files stopped libeq's reader part way, on data it cannot
    /// read; its message says where.
    #[error("could not read zone {zone}: the zone reader stopped: {message}")]
    Unreadable {
        /// The zone's short name.
        zone: String,
        /// What the reader said as it stopped.
        message: String,
    },
    /// A texture could not be decoded.
    #[error("could not decode texture {name}: {source}")]
    DecodeTexture {
        /// Source filename inside the archive.
        name: String,
        /// Decoder error.
        source: ImageError,
    },
}

/// A zone's short name, checked before it names a file: the server chooses
/// it, so it may not reach outside the installation.
///
/// # Errors
///
/// Returns [`LoadError::InvalidZoneName`] unless the name is lowercase ASCII
/// letters, digits and underscores.
pub(crate) fn zone_name(short_name: &str) -> Result<&str, LoadError> {
    if short_name.is_empty()
        || !short_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(LoadError::InvalidZoneName(short_name.to_owned()));
    }
    Ok(short_name)
}

/// Loads a classic S3D zone from an `EverQuest` installation.
///
/// # Errors
///
/// Returns [`LoadError`] when the name is unsafe, files are missing or
/// unreadable, or an archive, world, or texture cannot be decoded.
pub fn load_zone(eq_directory: &Path, short_name: &str) -> Result<ZoneAsset, LoadError> {
    let short_name = zone_name(short_name)?;
    guarded(short_name, || {
        read_zone(eq_directory, short_name, Textures::Realize)
    })
}

/// Loads a zone's solids alone: its collision, its regions and its placed
/// objects with their models' collision, the same as [`load_zone`] gives
/// them, but with no drawn geometry and no texture decoded. For building the
/// navigation mesh, where the textures would be most of the time.
///
/// # Errors
///
/// As [`load_zone`], but for textures, which aren't read.
pub fn load_zone_collision(eq_directory: &Path, short_name: &str) -> Result<ZoneAsset, LoadError> {
    let short_name = zone_name(short_name)?;
    guarded(short_name, || {
        read_zone(eq_directory, short_name, Textures::Skip)
    })
}

/// Whether a zone's read decodes its drawn geometry and textures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Textures {
    Realize,
    Skip,
}

/// Runs a zone's read through libeq, whose parser and model view panic on
/// some data they cannot read (a count or index past the end of a list), and
/// turns such a panic into [`LoadError::Unreadable`] carrying its message, so
/// a zone the reader stops on is a zone that can't be loaded, said as such,
/// rather than a panic in the loading task.
fn guarded<T>(
    zone: &str,
    read: impl FnOnce() -> Result<T, LoadError> + std::panic::UnwindSafe,
) -> Result<T, LoadError> {
    std::panic::catch_unwind(read).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_owned());
        Err(LoadError::Unreadable {
            zone: zone.to_owned(),
            message,
        })
    })
}

fn read_zone(
    eq_directory: &Path,
    short_name: &str,
    textures_read: Textures,
) -> Result<ZoneAsset, LoadError> {
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

    if textures_read == Textures::Realize {
        for mesh in world.meshes() {
            for staged in stage_mesh(&mesh) {
                primitives.push(staged.realize(
                    &mut archive,
                    &mut textures,
                    &mut texture_indices,
                )?);
            }
        }
    }

    let collision = world
        .meshes()
        .flat_map(|mesh| mesh_collision(&mesh))
        .collect();
    let placements = load_object_placements(&mut archive)?;
    let (models, objects, report) = load_object_models(
        eq_directory,
        short_name,
        placements,
        textures_read,
        (&mut textures, &mut texture_indices),
    )?;

    Ok(ZoneAsset {
        regions,
        short_name: short_name.to_owned(),
        primitives,
        textures,
        models,
        objects,
        objects_unplaced: report.unplaced,
        object_skins_skipped: report.skins_skipped,
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
    rotation: [f32; 4],
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
                translation: render([x, y, z]),
                rotation: placement_rotation([rotation_x, rotation_y, rotation_z]),
                scale: [uniform_scale; 3],
            })
        })
        .collect())
}

/// What a zone's object archive couldn't place.
#[derive(Default)]
struct ObjectReport {
    /// Placements naming no model the archive draws.
    unplaced: usize,
    /// Skins of objects on skeletons left out (see [`PosedHierarchy`]).
    skins_skipped: usize,
}

/// The zone's object models and the placements that use them, and what the
/// archive couldn't place.
fn load_object_models(
    eq_directory: &Path,
    short_name: &str,
    placements: Vec<ObjectPlacement>,
    textures_read: Textures,
    (textures, texture_indices): (&mut Vec<ZoneTexture>, &mut HashMap<String, usize>),
) -> Result<(Vec<ZoneModel>, Vec<ZoneObject>, ObjectReport), LoadError> {
    let archive_path = eq_directory.join(format!("{short_name}_obj.s3d"));
    let file = match File::open(&archive_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let report = ObjectReport {
                unplaced: placements.len(),
                skins_skipped: 0,
            };
            return Ok((Vec::new(), Vec::new(), report));
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
    let (doc, world) = parse_wld(&world_bytes, &world_name)?;

    let mut models = Vec::new();
    let mut model_indices = HashMap::new();
    let mut report = ObjectReport::default();
    for model in world.models() {
        let (Some(name), Some(mesh)) = (model.name(), model.mesh()) else {
            continue;
        };
        let staged = stage_mesh(&mesh);
        let Some(primitives) = realize_object(
            staged,
            textures_read,
            &mut archive,
            textures,
            texture_indices,
        )?
        else {
            continue;
        };
        model_indices.insert(name.to_owned(), models.len());
        models.push(ZoneModel {
            name: name.to_owned(),
            primitives,
            collision: mesh_collision(&mesh),
        });
    }
    // Objects built on a skeleton (trees, torches), stood in their rest
    // pose. The official client animates them; drawing them still is
    // inferred as a first step, item 6.283.
    let meshes = ObjectMeshes::new(&doc, &world);
    for actor in doc.fragment_iter::<libeq::wld::parser::ActorDef>() {
        let Some(name) = doc.get_string(actor.name_reference) else {
            continue;
        };
        if model_indices.contains_key(name) {
            continue;
        }
        let Some(posed) = posed_hierarchy(&doc, &meshes, actor) else {
            continue;
        };
        report.skins_skipped += posed.skins_skipped;
        let Some(primitives) = realize_object(
            posed.staged,
            textures_read,
            &mut archive,
            textures,
            texture_indices,
        )?
        else {
            continue;
        };
        model_indices.insert(name.to_owned(), models.len());
        models.push(ZoneModel {
            name: name.to_owned(),
            primitives,
            collision: posed.collision,
        });
    }

    let objects = placements
        .into_iter()
        .filter_map(|placement| {
            let Some(&model) = model_indices.get(&placement.model_name) else {
                report.unplaced += 1;
                return None;
            };
            Some(ZoneObject {
                model,
                translation: placement.translation,
                rotation: placement.rotation,
                scale: placement.scale,
            })
        })
        .collect();

    Ok((models, objects, report))
}

/// An object's primitives, or None when it draws nothing, so both reads
/// place the same objects.
fn realize_object(
    staged: Vec<StagedPrimitive>,
    textures_read: Textures,
    archive: &mut PfsReader<File>,
    textures: &mut Vec<ZoneTexture>,
    texture_indices: &mut HashMap<String, usize>,
) -> Result<Option<Vec<ZonePrimitive>>, LoadError> {
    if staged.is_empty() {
        return Ok(None);
    }
    Ok(Some(match textures_read {
        Textures::Realize => staged
            .into_iter()
            .map(|primitive| primitive.realize(archive, textures, texture_indices))
            .collect::<Result<_, _>>()?,
        Textures::Skip => Vec::new(),
    }))
}

/// A hierarchical object's drawn primitives, one per material, and its solid
/// triangles, in its rest pose.
struct PosedHierarchy {
    staged: Vec<StagedPrimitive>,
    collision: Vec<[[f32; 3]; 3]>,
    /// Skins left out because their vertices aren't each on one bone of
    /// the skeleton.
    skins_skipped: usize,
}

/// An object archive's meshes, found by the fragment a sprite names rather
/// than by name: two meshes in one archive can share a name.
struct ObjectMeshes<'a> {
    /// Each mesh fragment's place among the file's meshes.
    order: HashMap<*const libeq::wld::parser::DmSpriteDef2, usize>,
    meshes: Vec<libeq::wld::Mesh<'a>>,
}

impl<'a> ObjectMeshes<'a> {
    /// The meshes of `world`, a second parse of the same file as `doc`;
    /// both list their meshes in the file's order.
    fn new(doc: &libeq::wld::parser::WldDoc, world: &'a libeq::wld::Wld) -> Self {
        Self {
            order: doc
                .fragment_iter::<libeq::wld::parser::DmSpriteDef2>()
                .enumerate()
                .map(|(index, raw)| (std::ptr::from_ref(raw), index))
                .collect(),
            meshes: world.meshes().collect(),
        }
    }

    /// The mesh that is this fragment of `doc`.
    fn of(&self, raw: &libeq::wld::parser::DmSpriteDef2) -> Option<&libeq::wld::Mesh<'a>> {
        self.meshes.get(*self.order.get(&std::ptr::from_ref(raw))?)
    }
}

/// One mesh of a hierarchical object in its rest pose.
struct PosedMesh {
    staged: Vec<StagedPrimitive>,
    /// The mesh's vertices, posed, as libeq orders their axes.
    solids: Vec<[f32; 3]>,
    /// The vertices of its solid faces.
    indices: Vec<u32>,
}

/// Stands an actor built on a skeleton in its rest pose: each bone's mesh
/// moved by its bone, then each skin's vertices by the bone each belongs to.
/// None when the actor isn't one, its skeleton can't be read, or nothing is
/// drawn. A bone's sprite that isn't a mesh (a flame, a 2D sprite) draws
/// nothing here; the official client draws it, item 6.284.
fn posed_hierarchy(
    doc: &libeq::wld::parser::WldDoc,
    meshes: &ObjectMeshes<'_>,
    actor: &libeq::wld::parser::ActorDef,
) -> Option<PosedHierarchy> {
    use libeq::wld::parser::HierarchicalSprite;
    let sprite: &HierarchicalSprite = skeleton::fragment(doc, *actor.fragment_references.first()?)?;
    let definition = doc.get(&sprite.reference)?;
    let bones = skeleton::Skeleton::read(doc, definition).ok()?;
    // Bones act in WLD space; libeq's vectors have Y and Z swapped.
    let swap = Mat4::from_mat3(LIBEQ_AXES);
    let joints: Vec<Mat4> = bones
        .rest_bones()
        .into_iter()
        .map(|bone| swap * bone * swap)
        .collect();
    Some(pose_hierarchy(doc, meshes, definition, &joints))
}

/// Poses a skeleton's meshes and skins with these joints, which act on
/// libeq's axes.
fn pose_hierarchy(
    doc: &libeq::wld::parser::WldDoc,
    meshes: &ObjectMeshes<'_>,
    definition: &libeq::wld::parser::HierarchicalSpriteDef,
    joints: &[Mat4],
) -> PosedHierarchy {
    use libeq::wld::parser::DmSprite;
    let mesh_of = |reference: u32| {
        let sprite: &DmSprite = skeleton::fragment(doc, reference)?;
        let raw = doc.get(&sprite.reference)?;
        meshes.of(raw).map(|mesh| (mesh, raw))
    };
    // Each posed mesh's drawn primitives, its vertices and its solid faces.
    let mut posed: Vec<PosedMesh> = Vec::new();
    let mut pose = |mesh: &libeq::wld::Mesh<'_>, joint_of: &dyn Fn(usize) -> Mat4| {
        let mut staged = stage_mesh(mesh);
        for primitive in &mut staged {
            for (index, (position, normal)) in primitive
                .positions
                .iter_mut()
                .zip(primitive.normals.iter_mut())
                .enumerate()
            {
                let joint = joint_of(index);
                *position = joint
                    .transform_point3(Vec3::from_array(*position))
                    .to_array();
                *normal = joint
                    .transform_vector3(Vec3::from_array(*normal))
                    .normalize_or_zero()
                    .to_array();
            }
        }
        let center = mesh.center();
        let solids = mesh
            .positions()
            .into_iter()
            .enumerate()
            .map(|(index, p)| {
                let p = Vec3::new(p[0] + center.0, p[1] + center.1, p[2] + center.2);
                joint_of(index).transform_point3(p).to_array()
            })
            .collect();
        posed.push(PosedMesh {
            staged,
            solids,
            indices: mesh.collision_indices(),
        });
    };
    for (index, dag) in definition.dags.iter().enumerate() {
        if dag.mesh_or_sprite_reference == 0 {
            continue;
        }
        if let Some((mesh, _)) = mesh_of(dag.mesh_or_sprite_reference) {
            let joint = joints[index];
            pose(mesh, &|_| joint);
        }
    }
    let mut skipped = 0;
    for &reference in definition.dm_sprites.as_deref().unwrap_or_default() {
        let Some((mesh, raw)) = mesh_of(reference) else {
            continue;
        };
        let owners: Vec<usize> = raw
            .skin_assignment_groups
            .iter()
            .flat_map(|(count, bone)| std::iter::repeat_n(usize::from(*bone), usize::from(*count)))
            .collect();
        // A skin whose vertices aren't all on bones of this skeleton is
        // left out, as the characters refuse it.
        if owners.len() != raw.positions.len()
            || owners.len() != mesh.positions().len()
            || owners.iter().any(|bone| *bone >= joints.len())
        {
            skipped += 1;
            continue;
        }
        pose(mesh, &|vertex| joints[owners[vertex]]);
    }
    let mut collision = Vec::new();
    let mut by_material: Vec<StagedPrimitive> = Vec::new();
    let skins_skipped = skipped;
    for PosedMesh {
        staged,
        solids,
        indices,
    } in posed
    {
        collision.extend(solid_triangles(&solids, &indices));
        for primitive in staged {
            match by_material
                .iter_mut()
                .find(|merged| merged.material == primitive.material)
            {
                Some(merged) => merged.append(primitive),
                None => by_material.push(primitive),
            }
        }
    }
    PosedHierarchy {
        staged: by_material,
        collision,
        skins_skipped,
    }
}
/// A drawable mesh primitive whose texture is named but not yet decoded, so
/// geometry can be read up front and textures only when a model is used.
#[derive(Clone, Debug)]
struct StagedPrimitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    texture_coordinates: Vec<[f32; 2]>,
    indices: Vec<u32>,
    material_mode: MaterialMode,
    texture: Option<String>,
    /// The material's name in upper case, such as `HUMCH0001_MDF`.
    material: Option<String>,
}

impl StagedPrimitive {
    /// Adds another primitive of the same material after this one's.
    fn append(&mut self, other: Self) {
        let offset = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.texture_coordinates.extend(other.texture_coordinates);
        self.indices
            .extend(other.indices.into_iter().map(|index| index + offset));
    }

    /// Decodes the texture (once per archive and name) and returns the
    /// primitive in the renderer's frame.
    fn realize(
        self,
        archive: &mut PfsReader<File>,
        textures: &mut Vec<ZoneTexture>,
        texture_indices: &mut HashMap<String, usize>,
    ) -> Result<ZonePrimitive, LoadError> {
        let texture = self
            .texture
            .map(|name| load_texture(archive, &name, textures, texture_indices))
            .transpose()?
            .flatten();
        Ok(ZonePrimitive {
            positions: self.positions.into_iter().map(render).collect(),
            normals: self.normals.into_iter().map(render).collect(),
            texture_coordinates: self.texture_coordinates,
            indices: self
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|&triangle| eq_client_axes::wld_triangle(triangle))
                .collect(),
            texture,
            material_mode: self.material_mode,
        })
    }
}

/// libeq hands out WLD vectors with Y and Z swapped, for Y-up viewers; this
/// matrix swaps them back, and is its own inverse.
const LIBEQ_AXES: Mat3 = Mat3::from_cols(Vec3::X, Vec3::Z, Vec3::Y);

/// Converts a libeq vector to WLD coordinates.
fn wld(vector: [f32; 3]) -> Vec3 {
    LIBEQ_AXES * Vec3::from_array(vector)
}

/// Converts a libeq position or normal to the renderer's frame.
fn render(vector: [f32; 3]) -> [f32; 3] {
    eq_client_axes::from_wld(wld(vector)).to_array()
}

/// Converts libeq's placement rotation, Euler angles in degrees about its
/// swapped axes, to an `(x, y, z, w)` quaternion in the renderer's frame.
fn placement_rotation(degrees: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = degrees.map(f32::to_radians);
    let swapped = Mat3::from_quat(Quat::from_euler(EulerRot::XYZ, x, y, z));
    let rotation = Quat::from_mat3(&(LIBEQ_AXES * swapped * LIBEQ_AXES));
    eq_client_axes::wld_rotation(rotation).to_array()
}

/// Reads a mesh's drawable primitives, positioned at the mesh's center.
fn stage_mesh(mesh: &libeq::wld::Mesh<'_>) -> Vec<StagedPrimitive> {
    let center = mesh.center();
    let mut primitives = Vec::new();
    for primitive in mesh.primitives() {
        let (material_mode, texture, name) = {
            let material = primitive.material();
            let Some(material_mode) = material_mode(*material.render_method()) else {
                continue;
            };
            let texture = material
                .base_color_texture()
                .and_then(|value| value.source());
            (
                material_mode,
                texture,
                material.name().map(str::to_ascii_uppercase),
            )
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
        primitives.push(StagedPrimitive {
            positions,
            normals: primitive.normals(),
            texture_coordinates: primitive.texture_coordinates(),
            indices: primitive.indices(),
            material_mode,
            texture,
            material: name,
        });
    }
    primitives
}

/// The name EQ uses to match an actor model, such as `IT63` for `it63_ACTORDEF`:
/// upper case, without the `_ACTORDEF` suffix. Server strings stay names; they
/// never become paths.
#[must_use]
pub fn model_key(name: &str) -> String {
    let name = name.trim().to_ascii_uppercase();
    name.strip_suffix("_ACTORDEF").unwrap_or(&name).to_owned()
}

/// Reads the WLD solid-face flags independently of visible material batches,
/// in the renderer's frame with the same winding as the drawn triangles.
fn mesh_collision(mesh: &libeq::wld::Mesh<'_>) -> Vec<[[f32; 3]; 3]> {
    let center = mesh.center();
    let positions: Vec<[f32; 3]> = mesh
        .positions()
        .into_iter()
        .map(|p| [p[0] + center.0, p[1] + center.1, p[2] + center.2])
        .collect();
    solid_triangles(&positions, &mesh.collision_indices())
}

/// Solid triangles over libeq positions, in the renderer's frame with the
/// same winding as the drawn triangles.
fn solid_triangles(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[[f32; 3]; 3]> {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|&triangle| {
            let mut corners = [[0.0; 3]; 3];
            for (out, index) in corners
                .iter_mut()
                .zip(eq_client_axes::wld_triangle(triangle))
            {
                *out = render(*positions.get(index as usize)?);
            }
            Some(corners)
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

    use super::{
        LoadError, MaterialMode, ZoneAsset, bmp_color_key, load_zone, load_zone_collision,
        material_mode,
    };

    #[test]
    fn unparseable_world_files_are_errors_not_crashes() {
        assert!(matches!(
            super::parse_wld(b"not a world file", "broken.wld"),
            Err(LoadError::ParseWorld(_))
        ));
    }

    /// A world file whose one mesh claims more faces for its material than it
    /// has, which libeq's model view stops on with a panic.
    fn world_the_reader_stops_on() -> Vec<u8> {
        use crate::test_world::{Coordinates, OLD_FORMAT, WorldFile, material, palette, triangle};
        let mut world = WorldFile::new(OLD_FORMAT);
        let (m, p, t) = (
            world.name("M_MDF"),
            world.name("P_MP"),
            world.name("T_DMSPRITEDEF"),
        );
        let material = world.fragment(0x30, material(m, 0x8000_0001));
        let palette = world.fragment(0x31, palette(p, material));
        let coordinates = Coordinates::Old([(0, 0), (256, 0), (0, 256)]);
        world.fragment(0x36, triangle(t, palette, &coordinates, 2));
        world.bytes()
    }

    #[test]
    fn a_zone_the_reader_stops_on_is_an_error_not_a_panic() {
        use libeq::pfs::PfsWriter;
        use std::io::Cursor;
        let install =
            std::env::temp_dir().join(format!("eq-zone-unreadable-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        let mut writer = PfsWriter::create(Cursor::new(Vec::new())).unwrap();
        writer
            .insert("broken.wld", world_the_reader_stops_on().as_slice())
            .unwrap();
        std::fs::write(
            install.join("broken.s3d"),
            writer.finish().unwrap().into_inner(),
        )
        .unwrap();
        let result = load_zone(&install, "broken");
        std::fs::remove_dir_all(&install).unwrap();
        let Err(LoadError::Unreadable { zone, message }) = result else {
            panic!("expected the zone to be unreadable");
        };
        assert_eq!(zone, "broken");
        assert_ne!(message, "");
    }

    /// A zone in the new WLD format, with an object file holding an
    /// unlisted user-defined material and vertex animations of both
    /// alignments.
    fn write_new_format_zone(install: &Path) {
        use crate::test_world::{
            Coordinates, NEW_FORMAT, OLD_FORMAT, WorldFile, material, palette, triangle,
            vertex_animation, write_archive,
        };
        let words = |words: &[u32]| -> Vec<u8> {
            words.iter().flat_map(|word| word.to_le_bytes()).collect()
        };

        let mut zone = WorldFile::new(NEW_FORMAT);
        let (m, p, t) = (
            zone.name("M_MDF"),
            zone.name("P_MP"),
            zone.name("R1_DMSPRITEDEF"),
        );
        let terrain = zone.fragment(0x30, material(m, 0x8000_0001));
        let terrain_palette = zone.fragment(0x31, palette(p, terrain));
        let coordinates = Coordinates::New([(0.0, 0.0), (1.0, 0.0), (0.5, -2.25)]);
        zone.fragment(0x36, triangle(t, terrain_palette, &coordinates, 1));

        // One placement of the object: name, model, flags (a location),
        // sphere, the location, and no user data.
        let mut placements = WorldFile::new(OLD_FORMAT);
        let model = placements.name("OBJ_ACTORDEF");
        let mut placement = words(&[0, model.cast_unsigned(), 0x02, 0]);
        for value in [10.0_f32, 20.0, 30.0, 0.0, 0.0, 0.0] {
            placement.extend(value.to_le_bytes());
        }
        placement.extend(words(&[0, 0]));
        placements.fragment(0x15, placement);

        let mut objects = WorldFile::new(OLD_FORMAT);
        let (w, p, t) = (
            objects.name("W_MDF"),
            objects.name("P_MP"),
            objects.name("OBJ_DMSPRITEDEF"),
        );
        let (model, a1, a2) = (
            objects.name("OBJ_ACTORDEF"),
            objects.name("A1_DMTRACKDEF"),
            objects.name("A2_DMTRACKDEF"),
        );
        let water = objects.fragment(0x30, material(w, 0x8000_0018));
        let object_palette = objects.fragment(0x31, palette(p, water));
        let coordinates = Coordinates::Old([(0, 0), (256, 0), (0, 256)]);
        let mesh = objects.fragment(0x36, triangle(t, object_palette, &coordinates, 1));
        // A sprite naming the mesh, then the model: name, flags, callback,
        // no actions, one sprite reference, no bounds, the sprite, unknown.
        let sprite = objects.fragment(0x2d, words(&[0, mesh.cast_unsigned(), 0]));
        objects.fragment(
            0x14,
            words(&[
                model.cast_unsigned(),
                0,
                0,
                0,
                1,
                0,
                sprite.cast_unsigned(),
                0,
            ]),
        );
        // 18 + 6 bytes ends on a word; 18 + 12 is padded with two more.
        objects.fragment(0x37, vertex_animation(a1, 1));
        objects.fragment(0x37, vertex_animation(a2, 2));

        write_archive(
            install,
            "newzone.s3d",
            &[
                ("newzone.wld", zone.bytes()),
                ("objects.wld", placements.bytes()),
            ],
        );
        write_archive(
            install,
            "newzone_obj.s3d",
            &[("newzone_obj.wld", objects.bytes())],
        );
    }

    #[test]
    fn a_new_format_zone_with_its_objects_loads_whole() {
        let install =
            std::env::temp_dir().join(format!("eq-zone-new-format-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        write_new_format_zone(&install);
        let result = load_zone(&install, "newzone");
        std::fs::remove_dir_all(&install).unwrap();
        let zone = result.unwrap();

        assert_eq!(zone.primitives.len(), 1);
        assert_eq!(
            zone.primitives[0].texture_coordinates,
            vec![[0.0, 0.0], [1.0, 0.0], [0.5, -2.25]]
        );
        assert_eq!(zone.models.len(), 1);
        assert_eq!(zone.models[0].name, "OBJ_ACTORDEF");
        assert_eq!(zone.models[0].primitives.len(), 1);
        // A user-defined type with no transparency mapping draws opaque.
        assert_eq!(
            zone.models[0].primitives[0].material_mode,
            MaterialMode::Opaque
        );
        assert_eq!(zone.objects.len(), 1);
        assert_eq!(zone.objects[0].model, 0);
    }

    /// A zone whose object archive holds a plain object, a tree built on a
    /// two-bone skeleton (a rigid mesh on the second bone and a skin over
    /// both), and a flame whose one bone carries no mesh; placements name
    /// each, and one names nothing.
    #[allow(clippy::too_many_lines)]
    fn write_hierarchical_zone(install: &Path) {
        use crate::test_world::{
            Coordinates, OLD_FORMAT, WorldFile, material, mesh, palette, palette_of,
            skinned_triangle, triangle, write_archive,
        };
        let words = |words: &[u32]| -> Vec<u8> {
            words.iter().flat_map(|word| word.to_le_bytes()).collect()
        };
        let coordinates = Coordinates::Old([(0, 0), (256, 0), (0, 256)]);
        let mut zone = WorldFile::new(OLD_FORMAT);
        let (m, p, t) = (
            zone.name("M_MDF"),
            zone.name("P_MP"),
            zone.name("Z_DMSPRITEDEF"),
        );
        let terrain = zone.fragment(0x30, material(m, 0x8000_0001));
        let terrain_palette = zone.fragment(0x31, palette(p, terrain));
        zone.fragment(0x36, triangle(t, terrain_palette, &coordinates, 1));
        let mut placements = WorldFile::new(OLD_FORMAT);
        for (name, x) in [
            ("OBJ_ACTORDEF", 0.0_f32),
            ("TREE_ACTORDEF", 50.0),
            ("FLAME_ACTORDEF", 70.0),
            ("GONE_ACTORDEF", 90.0),
        ] {
            let model = placements.name(name);
            let mut placement = words(&[0, model.cast_unsigned(), 0x02, 0]);
            for value in [x, 0.0, 0.0, 0.0, 0.0, 0.0] {
                placement.extend(value.to_le_bytes());
            }
            placement.extend(words(&[0, 0]));
            placements.fragment(0x15, placement);
        }

        let mut objects = WorldFile::new(OLD_FORMAT);
        let (w, p) = (objects.name("W_MDF"), objects.name("P_MP"));
        let solid = objects.fragment(0x30, material(w, 0x8000_0001));
        let object_palette = objects.fragment(0x31, palette(p, solid));
        let tri = objects.name("TRI_DMSPRITEDEF");
        let tri = objects.fragment(0x36, triangle(tri, object_palette, &coordinates, 1));
        // The skin's first two vertices are on the root, the third on the arm.
        let skin = objects.name("SKIN_DMSPRITEDEF");
        let skin = objects.fragment(
            0x36,
            skinned_triangle(skin, object_palette, &coordinates, 1, &[(2, 0), (1, 1)]),
        );
        let tri_sprite = objects.fragment(0x2d, words(&[0, tri.cast_unsigned(), 0]));
        let skin_sprite = objects.fragment(0x2d, words(&[0, skin.cast_unsigned(), 0]));
        // A model: name, flags, callback, no actions, one sprite reference,
        // no bounds, the sprite, unknown.
        let actor = |objects: &mut WorldFile, name: &str, sprite: i32| {
            let name = objects.name(name);
            objects.fragment(
                0x14,
                words(&[
                    name.cast_unsigned(),
                    0,
                    0,
                    0,
                    1,
                    0,
                    sprite.cast_unsigned(),
                    0,
                ]),
            );
        };
        actor(&mut objects, "OBJ_ACTORDEF", tri_sprite);
        // Base tracks of one frame: the root at rest, the arm 10 along the
        // file's x and 5 up its z (2560 / 256, 1280 / 256).
        let mut tracks = Vec::new();
        for (name, shift_x, shift_z) in [
            ("ROOT_TRACKDEF", 0_i16, 0_i16),
            ("ARM_TRACKDEF", 2560, 1280),
        ] {
            let name = objects.name(name);
            let mut body = words(&[name.cast_unsigned(), 0x08, 1]);
            for value in [16384_i16, 0, 0, 0, shift_x, 0, shift_z, 256] {
                body.extend(value.to_le_bytes());
            }
            let definition = objects.fragment(0x12, body);
            let track = objects.name("BONE_TRACK");
            tracks.push(objects.fragment(
                0x13,
                words(&[track.cast_unsigned(), definition.cast_unsigned(), 0]),
            ));
        }
        // The tree's skeleton: flags (with skins), two bones, no collision
        // volume; root (its track, no mesh, the arm its child), arm (its
        // track, the rigid mesh); one skin, updated with the root.
        let bone = objects.name("BONE_DAG");
        let tree = objects.name("TREE_HS_DEF");
        let mut body = words(&[tree.cast_unsigned(), 0x200, 2, 0]);
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[0].cast_unsigned(),
            0,
            1,
            1,
        ]));
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[1].cast_unsigned(),
            tri_sprite.cast_unsigned(),
            0,
        ]));
        body.extend(words(&[1, skin_sprite.cast_unsigned(), 0]));
        let tree = objects.fragment(0x10, body);
        let tree_sprite = objects.name("TREE_HS");
        let tree_sprite = objects.fragment(
            0x11,
            words(&[tree_sprite.cast_unsigned(), tree.cast_unsigned(), 0]),
        );
        actor(&mut objects, "TREE_ACTORDEF", tree_sprite);
        // A flame: one bone whose sprite is not a mesh (here a material).
        let flame = objects.name("FLAME_HS_DEF");
        let mut body = words(&[flame.cast_unsigned(), 0, 1, 0]);
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[0].cast_unsigned(),
            solid.cast_unsigned(),
            0,
        ]));
        let flame = objects.fragment(0x10, body);
        let flame_sprite = objects.name("FLAME_HS");
        let flame_sprite = objects.fragment(
            0x11,
            words(&[flame_sprite.cast_unsigned(), flame.cast_unsigned(), 0]),
        );
        actor(&mut objects, "FLAME_ACTORDEF", flame_sprite);
        // A bush: a skin of two faces in two materials, two vertices on each
        // bone, followed by another mesh of the same name with fewer
        // vertices, which the skin doesn't name.
        let (w2, p2) = (objects.name("W2_MDF"), objects.name("P2_MP"));
        let second = objects.fragment(0x30, material(w2, 0x8000_0001));
        let two = objects.fragment(0x31, palette_of(p2, &[solid, second]));
        let leaf = objects.name("LEAF_DMSPRITEDEF");
        let leaf_mesh = objects.fragment(
            0x36,
            mesh(
                leaf,
                two,
                &[[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0]],
                &[[0, 1, 2], [1, 3, 2]],
                &[(1, 0), (1, 1)],
                &[(2, 0), (2, 1)],
            ),
        );
        objects.fragment(0x36, triangle(leaf, object_palette, &coordinates, 1));
        let leaf_sprite = objects.fragment(0x2d, words(&[0, leaf_mesh.cast_unsigned(), 0]));
        let bush = objects.name("BUSH_HS_DEF");
        let mut body = words(&[bush.cast_unsigned(), 0x200, 2, 0]);
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[0].cast_unsigned(),
            0,
            1,
            1,
        ]));
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[1].cast_unsigned(),
            0,
            0,
        ]));
        body.extend(words(&[1, leaf_sprite.cast_unsigned(), 0]));
        let bush = objects.fragment(0x10, body);
        let bush_sprite = objects.name("BUSH_HS");
        let bush_sprite = objects.fragment(
            0x11,
            words(&[bush_sprite.cast_unsigned(), bush.cast_unsigned(), 0]),
        );
        actor(&mut objects, "BUSH_ACTORDEF", bush_sprite);
        // A skin whose groups cover one of its three vertices, unplaced.
        let short = objects.name("SHORT_DMSPRITEDEF");
        let short = objects.fragment(
            0x36,
            skinned_triangle(short, object_palette, &coordinates, 1, &[(1, 0)]),
        );
        let short_sprite = objects.fragment(0x2d, words(&[0, short.cast_unsigned(), 0]));
        let stump = objects.name("STUMP_HS_DEF");
        let mut body = words(&[stump.cast_unsigned(), 0x200, 1, 0]);
        body.extend(words(&[
            bone.cast_unsigned(),
            0,
            tracks[0].cast_unsigned(),
            0,
            0,
        ]));
        body.extend(words(&[1, short_sprite.cast_unsigned(), 0]));
        let stump = objects.fragment(0x10, body);
        let stump_sprite = objects.name("STUMP_HS");
        let stump_sprite = objects.fragment(
            0x11,
            words(&[stump_sprite.cast_unsigned(), stump.cast_unsigned(), 0]),
        );
        actor(&mut objects, "STUMP_ACTORDEF", stump_sprite);

        write_archive(
            install,
            "hzone.s3d",
            &[
                ("hzone.wld", zone.bytes()),
                ("objects.wld", placements.bytes()),
            ],
        );
        write_archive(
            install,
            "hzone_obj.s3d",
            &[("hzone_obj.wld", objects.bytes())],
        );
    }

    fn load_hierarchical_zone(label: &str) -> ZoneAsset {
        let install = std::env::temp_dir().join(format!("eq-zone-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        write_hierarchical_zone(&install);
        let result = load_zone(&install, "hzone");
        std::fs::remove_dir_all(&install).unwrap();
        result.unwrap()
    }

    /// The renderer's frame for libeq positions, wound as drawn.
    fn rendered(triangle: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        eq_client_axes::wld_triangle(triangle).map(super::render)
    }

    #[test]
    fn a_plain_object_loads_and_collides_as_it_always_has() {
        let zone = load_hierarchical_zone("plain-object");
        let model = zone
            .models
            .iter()
            .find(|model| model.name == "OBJ_ACTORDEF")
            .unwrap();
        // libeq gives the third vertex (0, 1, 0) as (0, 0, 1), Y and Z swapped.
        let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(model.primitives.len(), 1);
        assert_eq!(model.primitives[0].positions, corners.map(super::render));
        assert_eq!(
            model.primitives[0].indices,
            eq_client_axes::wld_triangle([0, 1, 2])
        );
        assert_eq!(model.collision, vec![rendered(corners)]);
        let placed = zone
            .objects
            .iter()
            .find(|object| zone.models[object.model].name == "OBJ_ACTORDEF");
        assert!(placed.is_some());
    }

    #[test]
    fn an_object_on_a_skeleton_is_placed_in_its_rest_pose_and_collides() {
        let zone = load_hierarchical_zone("hierarchical-object");
        let index = zone
            .models
            .iter()
            .position(|model| model.name == "TREE_ACTORDEF")
            .expect("the tree is a model");
        let tree = &zone.models[index];
        // The rigid mesh moved by the arm; then the skin, its
        // first two vertices where the root holds them and the third moved
        // by the arm. One material, so one primitive.
        // The arm's shift up the file's z lands on libeq's second axis,
        // which libeq swaps with the third.
        let rigid = [[10.0, 5.0, 0.0], [11.0, 5.0, 0.0], [10.0, 5.0, 1.0]];
        let skinned = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [10.0, 5.0, 1.0]];
        assert_eq!(tree.primitives.len(), 1);
        let expected: Vec<[f32; 3]> = rigid
            .iter()
            .chain(&skinned)
            .copied()
            .map(super::render)
            .collect();
        assert_eq!(tree.primitives[0].positions, expected);
        let first = eq_client_axes::wld_triangle([0, 1, 2]);
        let second = first.map(|index| index + 3);
        assert_eq!(tree.primitives[0].indices, [first, second].concat());
        assert_eq!(tree.collision, vec![rendered(rigid), rendered(skinned)]);
        assert_eq!(
            zone.objects
                .iter()
                .filter(|object| object.model == index)
                .count(),
            1
        );
    }

    #[test]
    fn a_skin_in_two_materials_poses_the_mesh_it_names_not_another_of_its_name() {
        let zone = load_hierarchical_zone("two-material-skin");
        let bush = zone
            .models
            .iter()
            .find(|model| model.name == "BUSH_ACTORDEF")
            .expect("the bush is a model");
        // Each material's primitive holds the whole mesh's four vertices,
        // the last two moved by the arm.
        let posed = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [10.0, 5.0, 1.0],
            [11.0, 5.0, 1.0],
        ]
        .map(super::render);
        assert_eq!(bush.primitives.len(), 2);
        for primitive in &bush.primitives {
            assert_eq!(primitive.positions, posed);
        }
        assert_eq!(bush.collision.len(), 2);
    }

    #[test]
    fn a_bone_sprite_that_is_not_a_mesh_draws_nothing_and_unplaced_are_counted() {
        let zone = load_hierarchical_zone("unplaced-objects");
        assert!(
            zone.models
                .iter()
                .all(|model| model.name != "FLAME_ACTORDEF")
        );
        assert_eq!(zone.objects.len(), 2);
        // The flame, which draws nothing, and the placement naming no model.
        assert_eq!(zone.objects_unplaced, 2);
        // The stump's skin covers one vertex of three: left out and counted,
        // and the zone still loads.
        assert_eq!(zone.object_skins_skipped, 1);
        assert!(
            zone.models
                .iter()
                .all(|model| model.name != "STUMP_ACTORDEF")
        );
    }

    #[test]
    fn a_zones_solids_alone_are_the_same_as_in_the_whole_zone() {
        let install =
            std::env::temp_dir().join(format!("eq-zone-collision-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        write_new_format_zone(&install);
        let (whole, solids) = (
            load_zone(&install, "newzone"),
            load_zone_collision(&install, "newzone"),
        );
        std::fs::remove_dir_all(&install).unwrap();
        let (whole, solids) = (whole.unwrap(), solids.unwrap());
        assert!(solids.primitives.is_empty() && solids.textures.is_empty());
        assert_eq!(solids.collision, whole.collision);
        assert_eq!(
            solids.regions.liquid_region_count(),
            whole.regions.liquid_region_count()
        );
        let models = |zone: &ZoneAsset| -> Vec<(String, Vec<[[f32; 3]; 3]>)> {
            zone.models
                .iter()
                .map(|model| (model.name.clone(), model.collision.clone()))
                .collect()
        };
        assert_eq!(models(&solids), models(&whole));
        assert!(
            solids
                .models
                .iter()
                .all(|model| model.primitives.is_empty())
        );
        // Each placement's model, translation, rotation and scale.
        let placed = |zone: &ZoneAsset| -> Vec<String> {
            zone.objects
                .iter()
                .map(|object| {
                    format!(
                        "{} {:?} {:?} {:?}",
                        object.model, object.translation, object.rotation, object.scale
                    )
                })
                .collect()
        };
        assert_eq!(placed(&solids), placed(&whole));
        assert_eq!(solids.objects.len(), 1);
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

    /// The share of triangles wound counter-clockwise around their vertex
    /// normals, as the renderer expects.
    fn counter_clockwise_share(primitives: &[super::ZonePrimitive]) -> f64 {
        use glam::Vec3;
        let (mut agree, mut total) = (0_u32, 0_u32);
        for primitive in primitives {
            for triangle in primitive.indices.as_chunks::<3>().0 {
                let point = |index: u32| Vec3::from_array(primitive.positions[index as usize]);
                let normal: Vec3 = triangle
                    .iter()
                    .map(|&index| Vec3::from_array(primitive.normals[index as usize]))
                    .sum();
                let [a, b, c] = triangle.map(point);
                let winding = (b - a).cross(c - a);
                if winding.length_squared() > 1e-8 && normal.length_squared() > 1e-8 {
                    total += 1;
                    agree += u32::from(winding.dot(normal) > 0.0);
                }
            }
        }
        f64::from(agree) / f64::from(total)
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn installed_triangles_wind_counter_clockwise_around_their_normals() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let zone = load_zone(Path::new(&install), "qeytoqrg").unwrap();
        let human = crate::characters::load_installed_character(
            Path::new(&install),
            "qeynos2",
            "HUM",
            &crate::characters::DonorArchives::default(),
        )
        .unwrap();
        let (terrain, character) = (
            counter_clockwise_share(&zone.primitives),
            counter_clockwise_share(&human.primitives),
        );
        println!("counter-clockwise share: terrain {terrain:.3}, human {character:.3}");
        assert!(terrain > 0.9 && character > 0.9);
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

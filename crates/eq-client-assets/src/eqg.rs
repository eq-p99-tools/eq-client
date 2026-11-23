//! Zones in the EQG format newer clients install, read through David Henry's
//! MIT `libeq_eqg` reader (github.com/djhenry/libeq, commit 686b74a).
//!
//! [`load_eqg_zone`] reads a zone's descriptor (`<zone>.zon`, `EQGZ`), its
//! terrain and model meshes (`EQGT`, `EQGM`) and the placements of its models
//! into the same [`ZoneAsset`] the classic S3D loader gives. Nothing calls it
//! yet: [`crate::load_zone`] reads S3D archives alone, and no installed
//! client generation that ships EQG zones can be chosen.
//!
//! What is read here comes from the files' layout alone, with no read of the
//! program that draws them, so each choice the format leaves open is inferred
//! (6.498): the coordinate frame (`render`, `placement_rotation`), which
//! file a model name names, what a triangle's material and flags mean, and
//! which material property names the diffuse texture. Regions and lights are
//! not read: the zone has none.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use glam::{EulerRot, Quat, Vec3};
use libeq::pfs::PfsReader;
use libeq_eqg::mesh::{self, Mesh, MeshKind};
use libeq_eqg::zone;

use crate::{
    LoadError, MaterialMode, ObjectMaterial, TerrainMaterial, ZoneAsset, ZoneModel, ZoneObject,
    ZonePrimitive, ZoneTexture, guarded, load_texture, zone_name,
};

/// A triangle the zone's solids leave out: walked and seen through
/// (inferred, 6.498).
const PASSABLE: u32 = 0x01;

/// The material property naming a material's diffuse texture (inferred,
/// 6.498).
const DIFFUSE: &[u8] = b"e_TextureDiffuse0";

/// A material property whose value is an offset into the string table.
const STRING_PROPERTY: u32 = 2;

/// Loads an EQG zone from an `EverQuest` installation: `<zone>.eqg`.
///
/// # Errors
///
/// Returns [`LoadError`] when the name is unsafe, the archive is missing or
/// unreadable, it holds no zone descriptor, a descriptor or mesh is
/// malformed, or a texture cannot be decoded. A reader panic on data it
/// can't read is [`LoadError::Unreadable`].
pub fn load_eqg_zone(eq_directory: &Path, short_name: &str) -> Result<ZoneAsset, LoadError> {
    let short_name = zone_name(short_name)?;
    guarded(short_name, || read(eq_directory, short_name))
}

fn read(eq_directory: &Path, short_name: &str) -> Result<ZoneAsset, LoadError> {
    let archive_path = eq_directory.join(format!("{short_name}.eqg"));
    let file = File::open(&archive_path).map_err(|source| LoadError::OpenArchive {
        path: archive_path,
        source,
    })?;
    let mut archive = PfsReader::open(file)?;
    let descriptor_name = format!("{short_name}.zon");
    let descriptor = archive
        .get(&descriptor_name)?
        .ok_or_else(|| LoadError::MissingWorld(descriptor_name.clone()))?;
    let descriptor = zone::parse(&descriptor).map_err(|error| unreadable(short_name, &error))?;
    let mut textures = Vec::new();
    let mut texture_indices = HashMap::new();
    let mut asset = ZoneAsset {
        regions: crate::regions::ZoneRegions::default(),
        short_name: short_name.to_owned(),
        primitives: Vec::new(),
        terrain_materials: Vec::new(),
        point_lights: Vec::new(),
        day_ambient: crate::lighting::day_ambient(eq_directory),
        textures: Vec::new(),
        models: Vec::new(),
        objects: Vec::new(),
        objects_unplaced: 0,
        object_skins_skipped: 0,
        object_lights_skipped: 0,
        collision: Vec::new(),
    };
    // Each of the descriptor's models, as a model of the asset's when it is
    // one, the terrain's meshes drawn in place.
    let mut models: Vec<Option<usize>> = Vec::with_capacity(descriptor.models.len());
    for name in &descriptor.models {
        let Some(offset) = name else {
            models.push(None);
            continue;
        };
        let name = descriptor
            .string(*offset)
            .map_err(|error| unreadable(short_name, &error))?;
        let Some((file_name, bytes)) = model_file(&mut archive, name)? else {
            models.push(None);
            continue;
        };
        let mesh = mesh::parse(&bytes).map_err(|error| unreadable(short_name, &error))?;
        let staged = Staged::from(&mesh, &mut archive, &mut textures, &mut texture_indices)
            .map_err(|error| match error {
                Failure::Load(error) => error,
                Failure::Mesh(error) => unreadable(short_name, &error),
            })?;
        match mesh.kind {
            MeshKind::Terrain => {
                asset
                    .terrain_materials
                    .extend(staged.primitives.iter().map(|_| TerrainMaterial {
                        double_sided: false,
                        animation: None,
                    }));
                asset.primitives.extend(staged.primitives);
                asset.collision.extend(staged.collision);
                models.push(None);
            }
            MeshKind::Model => {
                models.push(Some(asset.models.len()));
                asset.models.push(ZoneModel {
                    name: file_name,
                    materials: staged
                        .primitives
                        .iter()
                        .map(|_| ObjectMaterial {
                            double_sided: false,
                        })
                        .collect(),
                    primitives: staged.primitives,
                    collision: staged.collision,
                    vertex_count: None,
                });
            }
        }
    }
    for placement in &descriptor.placements {
        let model = placement
            .model_index
            .and_then(|index| models.get(usize::try_from(index).ok()?).copied().flatten());
        let Some(model) = model else {
            asset.objects_unplaced += 1;
            continue;
        };
        asset.objects.push(ZoneObject {
            model,
            translation: render(placement.position),
            rotation: placement_rotation(placement.rotation),
            scale: [placement.scale; 3],
            light: None,
        });
    }
    asset.textures = textures;
    Ok(asset)
}

/// A model name's file in the archive: the name as it is, else with the
/// model or terrain extension (inferred, 6.498).
fn model_file(
    archive: &mut PfsReader<File>,
    name: &[u8],
) -> Result<Option<(String, Vec<u8>)>, LoadError> {
    let name = String::from_utf8_lossy(name).to_ascii_lowercase();
    for candidate in [name.clone(), format!("{name}.mod"), format!("{name}.ter")] {
        if let Some(bytes) = archive.get(&candidate)? {
            return Ok(Some((name, bytes)));
        }
    }
    Ok(None)
}

fn unreadable(zone: &str, error: &dyn std::fmt::Display) -> LoadError {
    LoadError::Unreadable {
        zone: zone.to_owned(),
        message: error.to_string(),
    }
}

enum Failure {
    Load(LoadError),
    Mesh(mesh::ParseError),
}

impl From<LoadError> for Failure {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

impl From<mesh::ParseError> for Failure {
    fn from(error: mesh::ParseError) -> Self {
        Self::Mesh(error)
    }
}

/// One mesh's drawn primitives, one per material its triangles use, and its
/// solids, in the renderer's frame.
struct Staged {
    primitives: Vec<ZonePrimitive>,
    collision: Vec<[[f32; 3]; 3]>,
}

impl Staged {
    fn from(
        mesh: &Mesh<'_>,
        archive: &mut PfsReader<File>,
        textures: &mut Vec<ZoneTexture>,
        texture_indices: &mut HashMap<String, usize>,
    ) -> Result<Self, Failure> {
        let mut by_material: Vec<Option<ZonePrimitive>> = std::iter::repeat_with(|| None)
            .take(mesh.materials.len())
            .collect();
        let mut collision = Vec::new();
        for triangle in &mesh.triangles {
            // The reader keeps every index within the mesh's vertices.
            let corners = triangle.vertex_indices.map(|index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| mesh.vertices.get(index))
            });
            let [Some(a), Some(b), Some(c)] = corners else {
                continue;
            };
            let corners = eq_client_axes::wld_triangle([a, b, c]);
            if triangle.flags & PASSABLE == 0 {
                collision.push(corners.map(|vertex| render(vertex.position)));
            }
            // A triangle with no material of the mesh's is a solid alone.
            let Some(slot) = usize::try_from(triangle.material_index)
                .ok()
                .and_then(|index| by_material.get_mut(index))
            else {
                continue;
            };
            let primitive = slot.get_or_insert_with(|| ZonePrimitive {
                positions: Vec::new(),
                normals: Vec::new(),
                texture_coordinates: Vec::new(),
                indices: Vec::new(),
                texture: None,
                material_mode: MaterialMode::Opaque,
                light: Vec::new(),
            });
            for vertex in corners {
                let index = u32::try_from(primitive.positions.len()).unwrap_or(u32::MAX);
                primitive.positions.push(render(vertex.position));
                primitive.normals.push(render(vertex.normal));
                primitive.texture_coordinates.push(vertex.uv0);
                primitive.indices.push(index);
            }
        }
        let mut primitives = Vec::new();
        for (material, primitive) in mesh.materials.iter().zip(by_material) {
            let Some(mut primitive) = primitive else {
                continue;
            };
            if let Some(name) = diffuse(mesh, material)? {
                primitive.texture = load_texture(archive, &name, textures, texture_indices)?;
            }
            primitives.push(primitive);
        }
        Ok(Self {
            primitives,
            collision,
        })
    }
}

/// The file a material's diffuse texture property names, if it has one.
fn diffuse(mesh: &Mesh<'_>, material: &mesh::Material) -> Result<Option<String>, mesh::ParseError> {
    for property in &material.properties {
        if property.kind == STRING_PROPERTY && mesh.string(property.name_offset)? == DIFFUSE {
            let name = mesh.string(property.value)?;
            return Ok(Some(String::from_utf8_lossy(name).to_ascii_lowercase()));
        }
    }
    Ok(None)
}

/// Converts an EQG position or normal to the renderer's frame, taking EQG
/// files to store geometry as WLD files do (inferred, 6.498).
fn render(vector: [f32; 3]) -> [f32; 3] {
    eq_client_axes::from_wld(Vec3::from_array(vector)).to_array()
}

/// Converts a placement's rotation to an `(x, y, z, w)` quaternion in the
/// renderer's frame, taking it as radians about the WLD axes (inferred,
/// 6.498).
fn placement_rotation([x, y, z]: [f32; 3]) -> [f32; 4] {
    eq_client_axes::wld_rotation(Quat::from_euler(EulerRot::XYZ, x, y, z)).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libeq::pfs::PfsWriter;
    use std::io::Cursor;

    /// A string table and the offset of each string in it.
    fn strings(names: &[&str]) -> (Vec<u8>, Vec<u32>) {
        let mut table = Vec::new();
        let mut offsets = Vec::new();
        for name in names {
            offsets.push(u32::try_from(table.len()).unwrap());
            table.extend(name.as_bytes());
            table.push(0);
        }
        (table, offsets)
    }

    fn words(bytes: &mut Vec<u8>, words: &[u32]) {
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
    }

    fn floats(bytes: &mut Vec<u8>, floats: &[f32]) {
        for float in floats {
            bytes.extend(float.to_le_bytes());
        }
    }

    /// A version 1 mesh: two materials (the first with a diffuse texture),
    /// four vertices and three triangles: one per material, and one passable
    /// on the first material.
    fn mesh(magic: [u8; 4]) -> Vec<u8> {
        let (table, at) = strings(&[
            "Wall",
            "Opaque_MaxCB1.fx",
            "e_TextureDiffuse0",
            "wall.dds",
            "Floor",
        ]);
        let mut bytes = magic.to_vec();
        words(
            &mut bytes,
            &[1, u32::try_from(table.len()).unwrap(), 2, 4, 3],
        );
        if &magic == b"EQGM" {
            words(&mut bytes, &[0]);
        }
        bytes.extend(&table);
        words(
            &mut bytes,
            &[0, at[0], at[1], 1, at[2], STRING_PROPERTY, at[3]],
        );
        words(&mut bytes, &[1, at[4], u32::MAX, 0]);
        for position in [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 20.0, 0.0],
            [0.0, 0.0, 30.0],
        ] {
            floats(&mut bytes, &position);
            floats(&mut bytes, &[0.0, 0.0, 1.0, 0.5, 0.25]);
        }
        words(&mut bytes, &[0, 1, 2, 0, 0]);
        words(&mut bytes, &[0, 1, 3, 1, 0]);
        words(&mut bytes, &[0, 2, 3, 0, PASSABLE]);
        bytes
    }

    /// A version 1 descriptor naming `models` and placing model 1 once.
    fn descriptor(models: &[&str]) -> Vec<u8> {
        let mut names: Vec<&str> = models.to_vec();
        names.push("Tree01");
        let (table, at) = strings(&names);
        let mut bytes = b"EQGZ".to_vec();
        words(&mut bytes, &[1, u32::try_from(table.len()).unwrap()]);
        words(&mut bytes, &[u32::try_from(models.len()).unwrap(), 2, 0, 0]);
        bytes.extend(&table);
        words(&mut bytes, &at[..models.len()]);
        words(&mut bytes, &[1, at[models.len()]]);
        floats(&mut bytes, &[1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 2.0]);
        // A placement of no model.
        words(&mut bytes, &[u32::MAX, at[models.len()]]);
        floats(&mut bytes, &[0.0; 7]);
        bytes
    }

    fn install(files: &[(&str, &[u8])]) -> tempfile_dir::Dir {
        let dir = tempfile_dir::Dir::new();
        let mut writer = PfsWriter::create(Cursor::new(Vec::new())).unwrap();
        for (name, bytes) in files {
            writer.insert(*name, *bytes).unwrap();
        }
        std::fs::write(
            dir.path().join("example.eqg"),
            writer.finish().unwrap().into_inner(),
        )
        .unwrap();
        dir
    }

    /// A directory removed when the test ends.
    mod tempfile_dir {
        use std::path::{Path, PathBuf};

        pub(super) struct Dir(PathBuf);

        impl Dir {
            pub(super) fn new() -> Self {
                static NEXT: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let path = std::env::temp_dir().join(format!(
                    "eqg-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            pub(super) fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn texture() -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn an_eqg_zone_reads_its_terrain_models_placements_and_textures() {
        let terrain = mesh(*b"EQGT");
        let model = mesh(*b"EQGM");
        let png = texture();
        let dir = install(&[
            ("example.zon", &descriptor(&["ter_example", "tree01.mod"])),
            ("ter_example.ter", &terrain),
            ("tree01.mod", &model),
            ("wall.dds", &png),
        ]);
        let zone = load_eqg_zone(dir.path(), "example").unwrap();
        // The terrain: both materials drawn, the passable triangle drawn but
        // not solid.
        assert_eq!(zone.primitives.len(), 2);
        assert_eq!(zone.terrain_materials.len(), 2);
        assert_eq!(zone.triangle_count(), 3);
        assert_eq!(zone.collision.len(), 2);
        assert_eq!(zone.primitives[0].texture, Some(0));
        assert_eq!(zone.primitives[1].texture, None);
        assert_eq!(zone.textures.len(), 1);
        assert_eq!(zone.textures[0].name, "wall.dds");
        // Every position went through the one conversion.
        assert_eq!(zone.collision[0][0], render([0.0, 0.0, 0.0]),);
        assert!(
            zone.primitives[0]
                .positions
                .contains(&render([0.0, 20.0, 0.0]))
        );
        // The model, placed once; the placement of no model is counted.
        assert_eq!(zone.models.len(), 1);
        assert_eq!(zone.models[0].name, "tree01.mod");
        assert_eq!(zone.models[0].primitives.len(), 2);
        assert_eq!(zone.models[0].collision.len(), 2);
        assert_eq!(zone.objects.len(), 1);
        assert_eq!(zone.objects[0].model, 0);
        assert_eq!(zone.objects[0].translation, render([1.0, 2.0, 3.0]));
        assert_eq!(zone.objects[0].scale, [2.0; 3]);
        assert_eq!(zone.objects_unplaced, 1);
        // No lights.
        assert_eq!(zone.point_lights.len(), 0);
    }

    #[test]
    fn a_bad_eqg_archive_is_an_error_not_a_crash() {
        let terrain = mesh(*b"EQGT");
        let zon = descriptor(&["ter_example", "tree01"]);
        // A truncated mesh.
        let dir = install(&[
            ("example.zon", &zon),
            ("ter_example.ter", &terrain[..terrain.len() - 1]),
        ]);
        let Err(LoadError::Unreadable { message, .. }) = load_eqg_zone(dir.path(), "example")
        else {
            panic!("a truncated mesh is unreadable");
        };
        assert!(message.contains("truncated"), "{message}");
        // The whole archive reads once the mesh is whole.
        let whole = install(&[("example.zon", &zon), ("ter_example.ter", &terrain)]);
        assert!(load_eqg_zone(whole.path(), "example").is_ok());
        // A truncated descriptor.
        let dir = install(&[("example.zon", &zon[..zon.len() - 1])]);
        assert!(matches!(
            load_eqg_zone(dir.path(), "example"),
            Err(LoadError::Unreadable { .. })
        ));
        // No descriptor, no archive, an archive that isn't one, an unsafe name.
        let dir = install(&[("ter_example.ter", &terrain)]);
        assert!(matches!(
            load_eqg_zone(dir.path(), "example"),
            Err(LoadError::MissingWorld(_))
        ));
        assert!(matches!(
            load_eqg_zone(dir.path(), "absent"),
            Err(LoadError::OpenArchive { .. })
        ));
        std::fs::write(dir.path().join("broken.eqg"), b"not an archive").unwrap();
        assert!(load_eqg_zone(dir.path(), "broken").is_err());
        assert!(matches!(
            load_eqg_zone(dir.path(), "../example"),
            Err(LoadError::InvalidZoneName(_))
        ));
    }

    #[test]
    fn classic_zone_loading_never_reads_an_eqg_archive() {
        // An installation with only the EQG archive has no classic zone.
        let terrain = mesh(*b"EQGT");
        let dir = install(&[
            ("example.zon", &descriptor(&["ter_example", "tree01"])),
            ("ter_example.ter", &terrain),
        ]);
        assert!(matches!(
            crate::load_zone(dir.path(), "example"),
            Err(LoadError::OpenArchive { .. })
        ));
        assert!(load_eqg_zone(dir.path(), "example").is_ok());
    }
}

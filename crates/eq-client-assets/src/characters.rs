//! Classic character archive discovery, independent of the renderer and session.

mod model;
pub use model::{Attachment, CharacterAsset, CharacterPose, Piece, load_character};

use std::{fs::File, path::Path};

use libeq::{
    pfs::PfsReader,
    wld::parser::{DmSprite, FragmentRef, FragmentType, HierarchicalSpriteDef, Track, WldDoc},
};

use crate::LoadError;

/// Finds a classic model in the zone archive or the installation's global archives.
///
/// # Errors
/// Returns the last asset error when no candidate supplies a usable model.
pub fn load_installed_character(
    directory: &Path,
    zone: &str,
    model: &str,
) -> Result<CharacterAsset, LoadError> {
    let mut archives = vec![format!("{zone}_chr.s3d"), "global_chr.s3d".into()];
    archives.extend((2..=7).map(|i| format!("global{i}_chr.s3d")));
    let mut last_error = None;
    for archive in archives {
        match load_character(&directory.join(archive), model) {
            Ok(asset) => return Ok(asset),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| invalid("no character archive candidates")))
}

/// A skeletal model available in a locally supplied character archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterDefinition {
    /// WLD skeleton identifier, retained for subsequent mesh and animation lookup.
    pub name: String,
    /// Number of bones in original WLD order (the skin weights use these indices).
    pub bone_count: usize,
    /// Mesh identifiers explicitly attached to this skeleton.
    pub meshes: Vec<String>,
}

/// Lists skeletons and validates their base tracks and attached mesh references.
///
/// This first model-loading stage deliberately does not infer race, equipment,
/// or appearance from filename prefixes. Those belong to the character selection
/// and session layers.
///
/// # Errors
/// Returns an error for unreadable archives, absent WLD data, or broken skeleton
/// references. Game asset bytes are never written to the project directory.
pub fn inspect_archive(path: &Path) -> Result<Vec<CharacterDefinition>, LoadError> {
    let file = File::open(path).map_err(|source| LoadError::OpenArchive {
        path: path.to_owned(),
        source,
    })?;
    let mut archive = PfsReader::open(file)?;
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LoadError::ParseWorld("character archive needs a UTF-8 filename".into()))?;
    let world_name = format!("{stem}.wld");
    let bytes = archive
        .get(&world_name)?
        .ok_or(LoadError::MissingWorld(world_name))?;
    let doc = WldDoc::parse(&bytes).map_err(|errors| {
        LoadError::ParseWorld(format!("{} character WLD parse errors", errors.len()))
    })?;
    let mut characters = Vec::new();
    for skeleton in doc.fragment_iter::<HierarchicalSpriteDef>() {
        let name = doc
            .get_string(skeleton.name_reference)
            .ok_or_else(|| invalid("skeleton name"))?
            .to_owned();
        let mut parents = vec![None; skeleton.dags.len()];
        for (index, bone) in skeleton.dags.iter().enumerate() {
            let track: &Track = resolve(&doc, bone.track_reference)?;
            let definition = doc
                .get(&track.reference)
                .ok_or_else(|| invalid("base track"))?;
            if definition.frame_count == 0 {
                return Err(invalid("empty base track"));
            }
            for &child in &bone.sub_dags {
                let parent = parents
                    .get_mut(child as usize)
                    .ok_or_else(|| invalid("child bone"))?;
                if parent.replace(index).is_some() || child as usize == index {
                    return Err(invalid("bone parent"));
                }
            }
        }
        validate_parents(&parents)?;
        let mut meshes = Vec::new();
        for &reference in skeleton.dm_sprites.as_deref().unwrap_or_default() {
            let sprite: &DmSprite = resolve(&doc, reference)?;
            // libeq types this reference as a modern mesh, but global_chr also
            // contains legacy 0x2c skins (including the invisible-man model).
            let FragmentRef::Index(index, _) = sprite.reference else {
                return Err(invalid("named skin reference"));
            };
            let fragment = index
                .checked_sub(1)
                .and_then(|index| doc.at(index as usize));
            let name_reference = match fragment {
                Some(FragmentType::DmSpriteDef2(mesh)) => mesh.name_reference,
                Some(FragmentType::DmSpriteDef(mesh)) => mesh.name_reference,
                _ => return Err(invalid("skin mesh")),
            };
            let mesh_name = doc
                .get_string(name_reference)
                .ok_or_else(|| invalid("mesh name"))?;
            meshes.push(mesh_name.to_owned());
        }
        characters.push(CharacterDefinition {
            name,
            bone_count: skeleton.dags.len(),
            meshes,
        });
    }
    Ok(characters)
}

/// Rejects cycles before future pose evaluation can walk the hierarchy.
fn validate_parents(parents: &[Option<usize>]) -> Result<(), LoadError> {
    for start in 0..parents.len() {
        let mut cursor = Some(start);
        for _ in 0..parents.len() {
            cursor = match cursor {
                Some(index) => *parents.get(index).ok_or_else(|| invalid("parent bone"))?,
                None => break,
            };
        }
        if cursor.is_some() {
            return Err(invalid("cyclic skeleton"));
        }
    }
    Ok(())
}

fn resolve<T: libeq::wld::parser::Fragment + 'static>(
    doc: &WldDoc,
    index: u32,
) -> Result<&T, LoadError> {
    let index = i32::try_from(index)
        .ok()
        .filter(|index| *index > 0)
        .ok_or_else(|| invalid("fragment index"))?;
    doc.get(&FragmentRef::new(index))
        .ok_or_else(|| invalid("fragment type"))
}

fn invalid(detail: &str) -> LoadError {
    LoadError::ParseWorld(format!("invalid character {detail}"))
}

#[cfg(test)]
mod tests {
    use super::validate_parents;

    #[test]
    fn hierarchy_accepts_parent_after_child_and_multiple_roots() {
        assert!(validate_parents(&[Some(2), None, Some(1), None]).is_ok());
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn installed_models_name_their_materials_and_find_other_looks() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let human =
            super::load_installed_character(std::path::Path::new(&install), "qeynos2", "HUM")
                .unwrap();
        let materials: Vec<_> = human.materials.iter().flatten().collect();
        println!("HUM materials: {materials:?}");
        assert!(
            materials
                .iter()
                .any(|name| name.as_str() == "HUMCH0001_MDF")
        );
        assert!(human.has_material("humch0201_mdf"));
        let (chain, _) = human.material_texture("HUMCH0201_MDF").unwrap().unwrap();
        assert!(chain.width > 0 && chain.name.contains("humch0201"));
        assert!(human.material_texture("HUMCH9901_MDF").unwrap().is_none());
        // Both hands and the shield point are on the skeleton.
        let (_, attachments) = human.pose_with_attachments("P01", 0.0, true, &[]);
        println!("HUM attachments: {attachments:?}");
        assert!(attachments.iter().all(Option::is_some));
        // The right hand is on the right. A model faces heading 0 as stored,
        // with up along Y, so its right is facing × up; a mirrored frame would
        // put the right hand on the left.
        let right = eq_client_axes::from_wld(glam::Vec3::X).cross(glam::Vec3::Y);
        let side = |point: super::Attachment| {
            attachments[point.index()]
                .unwrap()
                .w_axis
                .truncate()
                .dot(right)
        };
        assert!(side(super::Attachment::RightHand) > 0.0);
        assert!(side(super::Attachment::LeftHand) < 0.0);
        // The robe body and the three helmed heads come with the model, and
        // the archive has every robe's materials.
        for piece in [
            super::Piece::Body(1),
            super::Piece::Head(1),
            super::Piece::Head(2),
            super::Piece::Head(3),
        ] {
            assert!(human.pieces.contains(&piece), "{piece:?}");
        }
        assert!(human.has_material("CLK0701_MDF") && human.has_material("CLK1006_MDF"));
    }

    #[test]
    fn hierarchy_rejects_cycles_and_invalid_indices() {
        assert!(validate_parents(&[Some(1), Some(0)]).is_err());
        assert!(validate_parents(&[Some(0)]).is_err());
        assert!(validate_parents(&[Some(4)]).is_err());
    }
}

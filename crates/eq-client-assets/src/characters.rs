//! Classic character archive discovery, independent of the renderer and session.

mod model;
pub use model::{
    Attachment, CharacterAsset, CharacterPose, Piece, STANDING_CLIP, SkinBind, load_character,
};

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use libeq::{
    pfs::PfsReader,
    wld::parser::{DmSprite, FragmentRef, FragmentType, HierarchicalSpriteDef, Track, WldDoc},
};

use crate::LoadError;

/// The character archives parsed for the movements models borrow, each read
/// once however many models borrow from it (Coldain, male, female and king,
/// all from `global_chr.s3d`). Clones share the archives; a fresh handle starts
/// empty, so its holder decides how long they stay in memory (the render keeps
/// one per zone).
#[derive(Clone, Debug, Default)]
pub struct DonorArchives(Arc<Mutex<HashMap<PathBuf, Option<WldDoc>>>>);

impl DonorArchives {
    /// Runs `read` on the archive's parsed WLD, parsing it on first use, or
    /// returns None for an archive that cannot be read (remembered too).
    fn with<R>(&self, path: &Path, read: impl FnOnce(&WldDoc) -> R) -> Option<R> {
        let mut archives = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        archives
            .entry(path.to_owned())
            .or_insert_with(|| model::read_world(path).ok())
            .as_ref()
            .map(read)
    }

    /// How many archives were tried, readable or not.
    #[cfg(test)]
    fn tried(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

/// Finds a classic model in the zone archive or the installation's global
/// archives, with the movements it borrows from another model in any of them,
/// parsed through `donors`.
///
/// # Errors
/// Returns [`LoadError::InvalidZoneName`] for a zone name that could leave the
/// installation, and otherwise the last asset error when no candidate supplies
/// a usable model.
pub fn load_installed_character(
    directory: &Path,
    zone: &str,
    model: &str,
    donors: &DonorArchives,
) -> Result<CharacterAsset, LoadError> {
    let zone = crate::zone_name(zone)?;
    let mut archives = vec![format!("{zone}_chr.s3d")];
    archives.extend(global_archives());
    load_from(directory, archives, model, donors)
}

/// Finds a classic model in the installation's global archives alone, as
/// [`load_installed_character`] does past a zone's own: the model a race
/// draws with wherever a zone doesn't replace it.
///
/// # Errors
/// The last asset error when no global archive supplies a usable model.
pub fn load_global_character(
    directory: &Path,
    model: &str,
    donors: &DonorArchives,
) -> Result<CharacterAsset, LoadError> {
    load_from(directory, global_archives().collect(), model, donors)
}

fn global_archives() -> impl Iterator<Item = String> {
    std::iter::once("global_chr.s3d".to_owned())
        .chain((2..=7).map(|i| format!("global{i}_chr.s3d")))
}

/// The model from the first of `archives` under `directory` that supplies a
/// usable one, borrowing movements from the others.
fn load_from(
    directory: &Path,
    archives: Vec<String>,
    model: &str,
    donors: &DonorArchives,
) -> Result<CharacterAsset, LoadError> {
    let archives: Vec<_> = archives
        .into_iter()
        .map(|archive| directory.join(archive))
        .collect();
    let mut last_error = None;
    for (index, archive) in archives.iter().enumerate() {
        // Another candidate may hold the movements this model borrows.
        let others: Vec<_> = archives
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, path)| path.clone())
            .collect();
        match model::load_with_donors(archive, model, &others, donors) {
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
    #[test]
    fn server_zone_names_never_leave_the_installation() {
        assert!(matches!(
            super::load_installed_character(
                std::path::Path::new("unused"),
                "../secrets",
                "HUM",
                &super::DonorArchives::default(),
            ),
            Err(crate::LoadError::InvalidZoneName(_))
        ));
    }

    use super::validate_parents;

    #[test]
    fn hierarchy_accepts_parent_after_child_and_multiple_roots() {
        assert!(validate_parents(&[Some(2), None, Some(1), None]).is_ok());
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn installed_models_name_their_materials_and_find_other_looks() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let human = super::load_installed_character(
            std::path::Path::new(&install),
            "qeynos2",
            "HUM",
            &super::DonorArchives::default(),
        )
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

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn a_zone_model_borrows_movements_kept_in_a_global_archive() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let install = std::path::Path::new(&install);
        let donors = super::DonorArchives::default();
        for model in ["COM", "COF"] {
            let coldain =
                super::load_installed_character(install, "thurgadina", model, &donors).unwrap();
            let codes = coldain.animation_codes();
            println!("{model} clips: {codes:?}");
            assert!(codes.contains(&"P01") && codes.contains(&"L01"));
            // The one-archive loader still sees only the zone archive's tracks.
            let alone = super::load_character(&install.join("thurgadina_chr.s3d"), model).unwrap();
            assert_eq!(alone.animation_codes(), [] as [&str; 0]);
        }
        // Both Coldain borrowed from global_chr, read once.
        assert_eq!(donors.tried(), 1);
        let donors = super::DonorArchives::default();
        let human = super::load_installed_character(install, "qeynos2", "HUM", &donors).unwrap();
        let alone = super::load_character(&install.join("global_chr.s3d"), "HUM").unwrap();
        assert_eq!(human.animation_codes(), alone.animation_codes());
        // The human's own archive holds its donor, so nothing else is read.
        assert_eq!(donors.tried(), 0);
    }

    /// Prints, for each model a zone shows, what the client would draw: the
    /// archive it came from, its pieces, its base-look height and, per clip,
    /// how far the base-look pieces reach and whether any point is lost
    /// (not finite). A model that loads yet draws nothing visible shows here
    /// as no base pieces, a collapsed height or lost points.
    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn inspect_zone_models() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let install = std::path::Path::new(&install);
        let zone = std::env::var("EQ_PROBE_ZONE").unwrap_or_else(|_| "akanon".into());
        let models = std::env::var("EQ_PROBE_MODELS")
            .unwrap_or_else(|_| "CLM,CLF,GNM,GNF,RAT,SPI,BET,SNA,BIX,DWM,DWF".into());
        let donors = super::DonorArchives::default();
        for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
            let asset = match super::load_installed_character(install, &zone, model, &donors) {
                Ok(asset) => asset,
                Err(error) => {
                    println!("{model}: not loaded ({error})");
                    continue;
                }
            };
            let archive = asset
                .source()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut pieces: Vec<String> = asset.pieces.iter().map(|p| format!("{p:?}")).collect();
            pieces.dedup();
            let base: Vec<bool> = asset.pieces.iter().map(|piece| piece.base()).collect();
            let shown = base.iter().filter(|shown| **shown).count();
            println!(
                "{model}: from {archive}; {} primitives, {shown} in the base look; pieces {pieces:?}; base span {:?}; clips {:?}",
                asset.primitives.len(),
                asset.span(),
                asset.animation_codes(),
            );
            for clip in asset.animation_codes() {
                let length = asset.clip_seconds(clip).unwrap_or(0.0);
                for at in [0.0, length / 2.0] {
                    let (poses, _) = asset.pose_with_attachments(clip, at, true, &base);
                    let points: Vec<[f32; 3]> = poses
                        .iter()
                        .flatten()
                        .flat_map(|pose| pose.positions.iter().copied())
                        .collect();
                    let lost = points
                        .iter()
                        .filter(|p| !p.iter().all(|v| v.is_finite()))
                        .count();
                    let reach = |axis: usize| {
                        points
                            .iter()
                            .map(|p| p[axis])
                            .filter(|v| v.is_finite())
                            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                                (lo.min(v), hi.max(v))
                            })
                    };
                    println!(
                        "  {clip} at {at:.2}s: x {:?} y {:?} z {:?}; {} points, {lost} lost",
                        reach(0),
                        reach(1),
                        reach(2),
                        points.len(),
                    );
                }
            }
        }
    }

    /// Prints how many bones the skeletons of every character archive in the
    /// installation have, the most in each archive and every model over 256,
    /// the most joints a skin may have where it is drawn; fails if any is.
    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn inspect_bone_counts() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let mut archives: Vec<_> = std::fs::read_dir(&install)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with("_chr.s3d"))
            })
            .collect();
        archives.sort();
        let (mut models, mut most, mut over) = (0, 0, Vec::new());
        for archive in &archives {
            let Ok(characters) = super::inspect_archive(archive) else {
                continue;
            };
            let mut names: Vec<String> = characters
                .into_iter()
                .filter_map(|character| character.name.split('_').next().map(str::to_owned))
                .collect();
            names.sort();
            names.dedup();
            let mut archive_most = 0;
            for name in names {
                let Ok(asset) = super::load_character(archive, &name) else {
                    continue;
                };
                models += 1;
                archive_most = archive_most.max(asset.bone_count());
                if asset.bone_count() > 256 {
                    over.push(format!("{name} ({})", asset.bone_count()));
                }
            }
            most = usize::max(most, archive_most);
            let file = archive.file_name().unwrap_or_default().to_string_lossy();
            println!("{file}: at most {archive_most} bones");
        }
        println!(
            "{} archives, {models} models, at most {most} bones; over 256: {over:?}",
            archives.len()
        );
        assert_eq!(over, Vec::<String>::new());
    }

    /// Prints, for every model in a zone's character archive, which movement
    /// clips move the body below the root (checklist 6.232). A model whose
    /// walk moves only the root is drawn walking in its run, and the other
    /// way round. `EQ_PROBE_ZONE` defaults to the gnolls' zone.
    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn inspect_movement_clips() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let install = std::path::Path::new(&install);
        let zone = std::env::var("EQ_PROBE_ZONE").unwrap_or_else(|_| "qeytoqrg".into());
        let archive = install.join(format!("{zone}_chr.s3d"));
        let mut models: Vec<String> = super::inspect_archive(&archive)
            .unwrap()
            .into_iter()
            .filter_map(|character| character.name.split('_').next().map(str::to_owned))
            .collect();
        models.sort();
        models.dedup();
        let donors = super::DonorArchives::default();
        for model in models {
            match super::load_installed_character(install, &zone, &model, &donors) {
                Ok(asset) => {
                    let codes = asset.animation_codes();
                    let clips: Vec<String> = ["L01", "L02", "L06", "L08"]
                        .into_iter()
                        .filter(|clip| codes.contains(clip))
                        .map(|clip| {
                            format!(
                                "{clip} {}",
                                if asset.clip_moves_body(clip) {
                                    "body"
                                } else {
                                    "root only"
                                }
                            )
                        })
                        .collect();
                    println!("{model}: {}", clips.join(", "));
                }
                Err(error) => println!("{model}: not loaded ({error})"),
            }
        }
    }

    #[test]
    fn an_unreadable_donor_archive_is_tried_once() {
        let donors = super::DonorArchives::default();
        let missing = std::path::Path::new("no-such-dir/global_chr.s3d");
        for _ in 0..2 {
            assert_eq!(donors.clone().with(missing, |_| unreachable!()), None::<()>);
        }
        assert_eq!(donors.tried(), 1);
    }
}

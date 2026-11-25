//! Model actors in character archives, including ships and launches.
use super::{
    LoadError, ObjectMeshes, Textures, ZoneModel, ZoneTexture, actor_mesh, guarded, mesh_collision,
    parse_wld, posed_hierarchy, realize_object, stage_mesh, zone_name,
};
use libeq::pfs::PfsReader;
use std::{collections::HashMap, fs::File, path::Path};

/// An authored actor in its rest pose, with owned geometry and decoded textures.
#[derive(Clone, Debug)]
pub struct ActorAsset {
    /// The actor's draw geometry, authored solidity and materials.
    pub model: ZoneModel,
    /// Textures indexed by the model's primitives and material animations.
    pub textures: Vec<ZoneTexture>,
}

/// Reads a named actor from the zone's character archive, then the
/// global character archives. These actors keep
/// the authored model origin and solid-face flags.
///
/// # Errors
/// Returns the last archive error or rejects an absent/unreadable actor.
pub fn load_installed_actor(
    directory: &Path,
    zone: &str,
    tag: &str,
) -> Result<ActorAsset, LoadError> {
    let zone = zone_name(zone)?;
    let manifest = directory.join(format!("{zone}_chr.txt"));
    let imported = std::fs::metadata(&manifest)
        .ok()
        .filter(|metadata| metadata.len() <= 65_536)
        .and_then(|_| std::fs::read_to_string(manifest).ok())
        .map_or_else(Vec::new, |text| imports(&text, tag));
    let archives = std::iter::once(crate::archives::zone_characters(zone))
        .chain(imported)
        .chain(crate::archives::global_characters());
    let mut last = None;
    for stem in archives {
        match guarded(&stem, || {
            read_actor(&crate::archives::path(directory, &stem), &stem, tag)
        }) {
            Ok(actor) => return Ok(actor),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| LoadError::ParseWorld("no actor archives".to_owned())))
}

fn read_actor(path: &Path, stem: &str, tag: &str) -> Result<ActorAsset, LoadError> {
    let file = File::open(path).map_err(|source| LoadError::OpenArchive {
        path: path.to_owned(),
        source,
    })?;
    let mut archive = PfsReader::open(file)?;
    let name = format!("{stem}.wld");
    let bytes = archive
        .get(&name)?
        .ok_or_else(|| LoadError::MissingWorld(name.clone()))?;
    let (doc, world) = parse_wld(&bytes, &name)?;
    let actor_name = format!("{tag}_ACTORDEF");
    let actors = doc.fragment_iter::<libeq::wld::parser::ActorDef>();
    let geometry = actors.zip(world.models()).find_map(|(actor, model)| {
        let name = model.name()?;
        if !name.eq_ignore_ascii_case(&actor_name) {
            return None;
        }
        let mesh = model.mesh()?;
        let raw = actor_mesh(&doc, actor);
        Some((
            stage_mesh(&mesh, &[], raw.map(|raw| (&doc, raw))),
            mesh_collision(&mesh),
        ))
    });
    let (staged, collision) = if let Some(geometry) = geometry {
        geometry
    } else {
        let meshes = ObjectMeshes::new(&doc, &world);
        let actor = doc
            .fragment_iter::<libeq::wld::parser::ActorDef>()
            .find(|actor| {
                doc.get_string(actor.name_reference)
                    .is_some_and(|name| name.eq_ignore_ascii_case(&actor_name))
            })
            .ok_or_else(|| LoadError::ParseWorld(format!("actor {tag} is unavailable")))?;
        let mut posed = posed_hierarchy(&doc, &meshes, actor)
            .filter(|posed| posed.skins_skipped == 0)
            .ok_or_else(|| {
                LoadError::ParseWorld(format!("actor {tag} has no complete readable pose"))
            })?;
        // The no-hit test applies to zone placements. The spawned actor's
        // collision mode is unread; retain its authored solid faces as before
        // the placement split (inferred, checklist 6.650).
        posed.collision.append(&mut posed.no_hit_collision);
        (posed.staged, posed.collision)
    };
    let mut textures = Vec::new();
    let realized = realize_object(
        &doc,
        staged,
        Textures::Realize,
        &mut archive,
        &mut textures,
        &mut HashMap::new(),
    )?
    .ok_or_else(|| LoadError::ParseWorld(format!("actor {tag} draws no geometry")))?;
    Ok(ActorAsset {
        model: ZoneModel {
            name: actor_name,
            primitives: realized.primitives,
            materials: realized.materials,
            collision,
            no_hit_collision: Vec::new(),
            vertex_count: None,
            // A carrier is never a door, whose riders the radius measures.
            radius: None,
        },
        textures,
    })
}

// Installed zone import manifests name the supplying archive explicitly.
// Never guess an archive from the actor tag, or let a manifest leave the
// installation root. Search ordering is a loader inference under 6.467.
fn imports(text: &str, tag: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in text.lines().take(4096) {
        let Some((model, archive)) = line.trim().split_once(',') else {
            continue;
        };
        if !model.trim().eq_ignore_ascii_case(tag) {
            continue;
        }
        let Ok(archive) = zone_name(archive.trim()) else {
            continue;
        };
        let archive = archive.to_ascii_lowercase();
        if !imports.contains(&archive) {
            imports.push(archive);
        }
    }
    imports
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_matching_safe_manifest_imports_select_an_archive() {
        let text =
            "5\nSHP,SHP_CHR\nOTHER,OTHER_CHR\nSHP,../outside\nSHP,shp_chr\nSHP,C:\\outside\n";
        assert_eq!(imports(text, "SHP"), ["shp_chr"]);
        assert_eq!(imports(text, "MISSING"), Vec::<String>::new());
    }
}

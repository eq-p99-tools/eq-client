//! Lists a zone's object models with their drawn and solid triangle counts,
//! without connecting to a game server.
//!
//! One zone:
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> <zone> [name filter]`
//!
//! Every door model named in a list of `zone<TAB>model` lines (for example the
//! server's doors table) that has faces flagged passable, in every installed
//! zone the list names, with its mesh's 0x36 flags and +0x14 reference, and
//! whether the loader makes it solid throughout (read F4b's named case):
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --doors <list>`
//!
//! Every object mesh in every installed `<zone>_obj.s3d` whose walking faces
//! the loader's two rules change, placed objects and doors alike: `named`
//! (read F4b: passable faces made solid) or `no-hit` (read R49-49c: every
//! face dropped), and how many named meshes are in the no-hit case, which
//! should be none:
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --changed`
//!
//! Where a zone places the objects built on those meshes, in server axes,
//! with the `/loc` the game prints (y, x, z):
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --places <zone>`
//!
//! For each object model a zone places: how many placements carry the WLD
//! 0x15 instance flag 0x80 (libeq's `sprite_volume_only`), beside its meshes'
//! faces, passable faces and the meshes read F4b's unreferenced case covers
//! (read F4f: the placer copies that bit into the actor at +0xe4):
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --instance-flags <zone>`
//!
//! Each placed model's meshes, one line each: the mesh's 0x36 flags, its
//! +0x14 reference, whether read R49-49c's no-hit case covers it (flags lack
//! 0x10000 and no reference: the walking sweep skips the whole mesh), and
//! its faces and passable faces. An optional filter keeps the models whose
//! names contain it:
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --actor-meshes <zone> [filter]`
use libeq::pfs::PfsReader;
use libeq::wld::parser::{DmSpriteDef2, FragmentRef, StringReference, WldDoc};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(args.next().ok_or("supply an EQ install directory")?);
    let zone = args.next().ok_or("supply a zone short name or --doors")?;
    if zone == "--places" {
        let zone = args.next().ok_or("supply a zone short name")?;
        places(&directory, &zone)?;
        return Ok(());
    }
    if zone == "--instance-flags" {
        let zone = args.next().ok_or("supply a zone short name")?;
        instance_flags(&directory, &zone)?;
        return Ok(());
    }
    if zone == "--actor-meshes" {
        let zone = args.next().ok_or("supply a zone short name")?;
        let filter = args.next().map(|name| name.to_ascii_uppercase());
        actor_mesh_lines(&directory, &zone, filter.as_deref())?;
        return Ok(());
    }
    if zone == "--changed" {
        changed(&directory)?;
        return Ok(());
    }
    if zone == "--doors" {
        let list = std::fs::read_to_string(args.next().ok_or("supply the doors list")?)?;
        doors(&directory, &list);
        return Ok(());
    }
    let filter = args.next().map(|name| name.to_ascii_lowercase());
    let asset = eq_client_assets::load_zone(&directory, &zone)?;
    println!("model\tdrawn\tsolid\tsolid_low\tsolid_high");
    for model in &asset.models {
        if filter
            .as_ref()
            .is_some_and(|filter| !model.name.to_ascii_lowercase().contains(filter))
        {
            continue;
        }
        let drawn: usize = model
            .primitives
            .iter()
            .map(|primitive| primitive.indices.len() / 3)
            .sum();
        let bounds = model.collision.iter().flatten().fold(
            None,
            |bounds: Option<([f32; 3], [f32; 3])>, point| {
                Some(bounds.map_or((*point, *point), |(low, high)| {
                    (
                        std::array::from_fn(|axis| low[axis].min(point[axis])),
                        std::array::from_fn(|axis| high[axis].max(point[axis])),
                    )
                }))
            },
        );
        let (low, high) = bounds.map_or_else(
            || ("-".to_owned(), "-".to_owned()),
            |(low, high)| (format!("{low:.1?}"), format!("{high:.1?}")),
        );
        println!(
            "{}\t{drawn}\t{}\t{low}\t{high}",
            model.name,
            model.collision.len()
        );
    }
    Ok(())
}

/// One mesh's facts the solid-throughout rule reads.
struct MeshFacts {
    /// Whether the loader keeps every face as solid (a named mesh).
    solid_throughout: bool,
    /// Read R49-49c's no-hit case: no 0x10000 flag and no +0x14 reference.
    /// The loader gives such a mesh no walking faces.
    unreferenced: bool,
    flags: u32,
    referenced: bool,
    faces: usize,
    passable: usize,
}

impl MeshFacts {
    /// Which of the loader's rules changes this mesh's walking faces: the
    /// no-hit case drops every face it met before, and the named case makes
    /// the passable ones solid.
    fn rule(&self) -> Option<&'static str> {
        if self.unreferenced {
            (self.faces > self.passable).then_some("no-hit")
        } else {
            (self.solid_throughout && self.passable > 0).then_some("named")
        }
    }
}

/// The meshes of a zone's object archive, by name.
fn mesh_facts(directory: &Path, zone: &str) -> Option<BTreeMap<String, MeshFacts>> {
    let file = std::fs::File::open(directory.join(format!("{zone}_obj.s3d"))).ok()?;
    let mut archive = PfsReader::open(file).ok()?;
    let bytes = archive.get(&format!("{zone}_obj.wld")).ok()??;
    let doc = WldDoc::parse(&bytes).ok()?;
    let mut facts = BTreeMap::new();
    for raw in doc.fragment_iter::<DmSpriteDef2>() {
        let Some(name) = doc.get_string(raw.name_reference) else {
            continue;
        };
        let referenced = !matches!(raw.fragment4, FragmentRef::Name(StringReference(0), _));
        facts.entry(name.to_owned()).or_insert_with(|| MeshFacts {
            solid_throughout: named(name),
            unreferenced: raw.flags & 0x1_0000 == 0 && !referenced,
            flags: raw.flags,
            referenced,
            faces: raw.faces.len(),
            passable: raw
                .faces
                .iter()
                .filter(|face| face.flags & 0x10 != 0)
                .count(),
        });
    }
    Some(facts)
}

/// For each zone the list names, each named door model whose mesh has
/// passable faces: its mesh's flags and reference, its faces, and the solid
/// triangles the loader now gives it. `solid` is every face when the engine
/// makes the mesh solid throughout, and its unflagged faces otherwise.
fn doors(directory: &Path, list: &str) {
    let mut by_zone: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in list.lines() {
        let mut fields = line.split('\t').map(str::trim);
        if let (Some(zone), Some(name)) = (fields.next(), fields.next())
            && !zone.is_empty()
            && !name.is_empty()
        {
            by_zone
                .entry(zone.to_ascii_lowercase())
                .or_default()
                .insert(name.to_ascii_uppercase());
        }
    }
    let (mut without, mut solid_throughout, mut kept, mut missing, mut unloaded) = (0, 0, 0, 0, 0);
    println!("zone\tmesh\tflags\treferenced\tfaces\tpassable\tsolid\trule");
    for (zone, names) in &by_zone {
        let (Some(facts), Ok(asset)) = (
            mesh_facts(directory, zone),
            eq_client_assets::load_zone_collision(directory, zone),
        ) else {
            unloaded += 1;
            continue;
        };
        for name in names {
            let mesh = format!("{name}_DMSPRITEDEF");
            let (Some(fact), Some(model)) = (
                facts.get(&mesh),
                asset
                    .models
                    .iter()
                    .find(|model| model.name.eq_ignore_ascii_case(&format!("{name}_ACTORDEF"))),
            ) else {
                missing += 1;
                continue;
            };
            if fact.passable == 0 {
                without += 1;
                continue;
            }
            let rule = if model.collision.len() == fact.faces {
                solid_throughout += 1;
                "solid throughout"
            } else {
                kept += 1;
                "passable kept"
            };
            println!(
                "{zone}\t{mesh}\t{:#x}\t{}\t{}\t{}\t{}\t{rule}",
                fact.flags,
                fact.referenced,
                fact.faces,
                fact.passable,
                model.collision.len()
            );
        }
    }
    println!(
        "# zones {}, unloaded {unloaded}; door meshes without passable faces {without}, solid throughout {solid_throughout}, passable kept {kept}, not found {missing}",
        by_zone.len()
    );
}

/// The five names the loader's table holds (read F4b), repeated here since
/// the probe reads the archive itself.
fn named(name: &str) -> bool {
    let starts = |prefix: &str| {
        name.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    starts("LADDER")
        || starts("IT66")
        || name.starts_with("SBDOOR103_")
        || name == "HHCELL_DMSPRITEDEF"
        || name == "PORT1414_DMSPRITEDEF"
}

/// Every object mesh whose walking faces a rule of the loader changes.
fn changed(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut zones: Vec<String> = std::fs::read_dir(directory)?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let zone = name
                .to_ascii_lowercase()
                .strip_suffix("_obj.s3d")?
                .to_owned();
            Some(zone)
        })
        .collect();
    zones.sort();
    let (mut read, mut count, mut named_no_hit) = (0, 0, 0);
    println!("zone\tmesh\tflags\treferenced\tfaces\tpassable\trule");
    for zone in &zones {
        let Some(facts) = mesh_facts(directory, zone) else {
            continue;
        };
        read += 1;
        for (mesh, fact) in &facts {
            let rule = fact.rule();
            named_no_hit += usize::from(fact.unreferenced && fact.solid_throughout);
            if let Some(rule) = rule {
                count += 1;
                println!(
                    "{zone}\t{mesh}\t{:#x}\t{}\t{}\t{}\t{rule}",
                    fact.flags, fact.referenced, fact.faces, fact.passable
                );
            }
        }
    }
    println!(
        "# object archives {}, read {read}; meshes a rule changes {count}; named meshes in the no-hit case {named_no_hit}",
        zones.len()
    );
    Ok(())
}

/// Each actor of a zone's object archive with the meshes it draws: its
/// own mesh, or each bone's mesh and skin of its skeleton.
fn actor_meshes(directory: &Path, zone: &str) -> Option<BTreeMap<String, BTreeSet<String>>> {
    use libeq::wld::parser::{ActorDef, DmSprite, HierarchicalSprite, HierarchicalSpriteDef};
    let file = std::fs::File::open(directory.join(format!("{zone}_obj.s3d"))).ok()?;
    let mut archive = PfsReader::open(file).ok()?;
    let bytes = archive.get(&format!("{zone}_obj.wld")).ok()??;
    let doc = WldDoc::parse(&bytes).ok()?;
    let at = |reference: u32| i32::try_from(reference).ok().filter(|index| *index > 0);
    let mesh_name = |reference: u32| -> Option<String> {
        let sprite: &DmSprite = doc.get(&FragmentRef::new(at(reference)?))?;
        let mesh: &DmSpriteDef2 = doc.get(&sprite.reference)?;
        doc.get_string(mesh.name_reference).map(str::to_owned)
    };
    let mut actors = BTreeMap::new();
    for actor in doc.fragment_iter::<ActorDef>() {
        let Some(name) = doc.get_string(actor.name_reference) else {
            continue;
        };
        let Some(&first) = actor.fragment_references.first() else {
            continue;
        };
        let mut meshes = BTreeSet::new();
        if let Some(mesh) = mesh_name(first) {
            meshes.insert(mesh);
        } else if let Some(index) = at(first)
            && let Some(sprite) = doc.get::<HierarchicalSprite>(&FragmentRef::new(index))
            && let Some(definition) = doc.get::<HierarchicalSpriteDef>(&sprite.reference)
        {
            for dag in &definition.dags {
                meshes.extend(mesh_name(dag.mesh_or_sprite_reference));
            }
            for &reference in definition.dm_sprites.as_deref().unwrap_or_default() {
                meshes.extend(mesh_name(reference));
            }
        }
        actors.insert(name.to_owned(), meshes);
    }
    Some(actors)
}

/// Each placed object whose model draws a mesh the rule makes solid, up to
/// five places each, in server axes.
fn places(directory: &Path, zone: &str) -> Result<(), Box<dyn std::error::Error>> {
    let facts = mesh_facts(directory, zone).ok_or("no object archive")?;
    let actors = actor_meshes(directory, zone).ok_or("no object archive")?;
    let asset = eq_client_assets::load_zone_collision(directory, zone)?;
    println!("## {zone}");
    println!("model\tmeshes\tsolid\tx\ty\tz\tloc (y, x, z)");
    for (index, model) in asset.models.iter().enumerate() {
        let meshes: Vec<&str> = actors
            .get(&model.name)
            .into_iter()
            .flatten()
            .filter(|mesh| facts.get(*mesh).is_some_and(|fact| fact.rule().is_some()))
            .map(|mesh| mesh.trim_end_matches("_DMSPRITEDEF"))
            .collect();
        if meshes.is_empty() {
            continue;
        }
        let placed = asset.objects.iter().filter(|object| object.model == index);
        println!("# {} placed {}", model.name, placed.clone().count());
        for object in placed.take(5) {
            let at = eq_client_axes::to_world(glam::Vec3::from_array(object.translation));
            println!(
                "{}\t{}\t{}\t{:.0}\t{:.0}\t{:.0}\t{:.0}, {:.0}, {:.0}",
                model.name,
                meshes.join(","),
                model.collision.len(),
                at.x,
                at.y,
                at.z,
                at.y,
                at.x,
                at.z
            );
        }
    }
    let drawing: Vec<&String> = actors
        .iter()
        .filter(|(_, meshes)| {
            meshes
                .iter()
                .any(|mesh| facts.get(mesh).is_some_and(|fact| fact.rule().is_some()))
        })
        .map(|(actor, _)| actor)
        .collect();
    println!(
        "# actors drawing a changed mesh {}: {}",
        drawing.len(),
        drawing
            .iter()
            .map(|actor| actor.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    Ok(())
}

/// Per placed object model: placements with and without the 0x15 instance
/// flag 0x80, and its meshes' faces.
/// Each model's meshes with what read R49-49c's no-hit case decides.
fn actor_mesh_lines(
    directory: &Path,
    zone: &str,
    filter: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let facts = mesh_facts(directory, zone).ok_or("no object archive")?;
    let actors = actor_meshes(directory, zone).ok_or("no object archive")?;
    println!("## {zone}");
    println!("model\tmesh\tflags\treferenced\tno_hit\tfaces\tpassable");
    for (model, meshes) in &actors {
        if filter.is_some_and(|filter| !model.to_ascii_uppercase().contains(filter)) {
            continue;
        }
        for mesh in meshes {
            match facts.get(mesh) {
                Some(fact) => println!(
                    "{model}\t{mesh}\t{:#x}\t{}\t{}\t{}\t{}",
                    fact.flags, fact.referenced, fact.unreferenced, fact.faces, fact.passable
                ),
                None => println!("{model}\t{mesh}\t-\t-\t-\t-\t-"),
            }
        }
    }
    Ok(())
}

fn instance_flags(directory: &Path, zone: &str) -> Result<(), Box<dyn std::error::Error>> {
    use libeq::wld::parser::Actor;
    let facts = mesh_facts(directory, zone).ok_or("no object archive")?;
    let actors = actor_meshes(directory, zone).ok_or("no object archive")?;
    let file = std::fs::File::open(directory.join(format!("{zone}.s3d")))?;
    let mut archive = PfsReader::open(file)?;
    let bytes = archive
        .get("objects.wld")?
        .ok_or("the zone has no objects.wld")?;
    let doc = WldDoc::parse(&bytes).map_err(|_| "objects.wld could not be parsed")?;
    // Model name -> (placements, with 0x80, the flag words seen).
    let mut placed: BTreeMap<String, (usize, usize, BTreeSet<u32>)> = BTreeMap::new();
    for actor in doc.fragment_iter::<Actor>() {
        let Some(model) = doc.get_string(actor.actor_def_reference) else {
            continue;
        };
        let flags = u32::from(actor.flags);
        let entry = placed.entry(model.to_ascii_uppercase()).or_default();
        entry.0 += 1;
        entry.1 += usize::from(flags & 0x80 != 0);
        entry.2.insert(flags);
    }
    println!("## {zone}");
    println!("model\tplaced\tflag_0x80\tflag_words\tmeshes\tfaces\tpassable\tcase_a_meshes");
    let (mut with, mut without) = (0, 0);
    for (model, (count, flagged, words)) in &placed {
        let meshes = actors.get(model).cloned().unwrap_or_default();
        let (mut face_count, mut passable, mut case_a) = (0, 0, 0);
        for mesh in &meshes {
            if let Some(fact) = facts.get(mesh) {
                face_count += fact.faces;
                passable += fact.passable;
                case_a += usize::from(fact.unreferenced && fact.passable > 0);
            }
        }
        with += flagged;
        without += count - flagged;
        let words: Vec<String> = words.iter().map(|word| format!("{word:#x}")).collect();
        println!(
            "{model}\t{count}\t{flagged}\t{}\t{}\t{face_count}\t{passable}\t{case_a}",
            words.join(","),
            meshes.len()
        );
    }
    println!("# placements with 0x80 {with}, without {without}");
    Ok(())
}

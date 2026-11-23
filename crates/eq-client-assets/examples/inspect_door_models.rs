//! Lists a zone's object models with their drawn, solid and passable triangle
//! counts, without connecting to a game server. A door collides with its
//! passable faces too, where a placed object walks through them.
//!
//! One zone:
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> <zone> [name filter]`
//!
//! Every door model named in a list of `zone<TAB>model` lines (for example the
//! server's doors table), in every installed zone it names, with what the
//! door's collider gains:
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> --doors <list>`
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(args.next().ok_or("supply an EQ install directory")?);
    let zone = args.next().ok_or("supply a zone short name or --doors")?;
    if zone == "--doors" {
        let list = std::fs::read_to_string(args.next().ok_or("supply the doors list")?)?;
        doors(&directory, &list);
        return Ok(());
    }
    let filter = args.next().map(|name| name.to_ascii_lowercase());
    let asset = eq_client_assets::load_zone(&directory, &zone)?;
    println!("model\tdrawn\tsolid\tpassable\tsolid_low\tsolid_high");
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
            "{}\t{drawn}\t{}\t{}\t{low}\t{high}",
            model.name,
            model.collision.len(),
            model.passable.len()
        );
    }
    Ok(())
}

/// For each zone the list names, the named door models that have passable
/// faces: `added` where the collider had solid faces already, `new` where it
/// had none. Doors whose models have no passable faces are counted only.
fn doors(directory: &std::path::Path, list: &str) {
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
                .insert(format!("{}_ACTORDEF", name.to_ascii_uppercase()));
        }
    }
    let (mut unchanged, mut added, mut new, mut missing, mut unloaded) = (0, 0, 0, 0, 0);
    println!("zone\tmodel\tkind\tsolid\tpassable");
    for (zone, names) in &by_zone {
        let Ok(asset) = eq_client_assets::load_zone_collision(directory, zone) else {
            unloaded += 1;
            continue;
        };
        for name in names {
            let Some(model) = asset
                .models
                .iter()
                .find(|model| model.name.eq_ignore_ascii_case(name))
            else {
                missing += 1;
                continue;
            };
            let kind = match (model.collision.is_empty(), model.passable.is_empty()) {
                (_, true) => {
                    unchanged += 1;
                    continue;
                }
                (false, false) => {
                    added += 1;
                    "added"
                }
                (true, false) => {
                    new += 1;
                    "new"
                }
            };
            println!(
                "{zone}\t{}\t{kind}\t{}\t{}",
                model.name,
                model.collision.len(),
                model.passable.len()
            );
        }
    }
    println!(
        "# zones {}, unloaded {unloaded}; models unchanged {unchanged}, added {added}, new {new}, not in the zone's objects {missing}",
        by_zone.len()
    );
}

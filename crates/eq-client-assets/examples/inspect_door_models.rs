//! Lists a zone's object models with their drawn and solid triangle counts,
//! without connecting to a game server. A door the client draws but walks
//! through shows here as a model with drawn triangles and no solid ones.
//!
//! `cargo run -p eq-client-assets --example inspect_door_models -- <EQ dir> <zone> [name filter]`
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(args.next().ok_or("supply an EQ install directory")?);
    let zone = args.next().ok_or("supply a zone short name")?;
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

//! Inspect local zone tags without connecting to a game server.
use libeq::{
    pfs::PfsReader,
    wld::parser::{WldDoc, WorldTree, Zone},
};
use std::{fs::File, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(args.next().ok_or("supply an EQ install directory")?);
    let zone = args.next().ok_or("supply a zone short name")?;
    if !zone.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("invalid zone short name".into());
    }
    let mut archive = PfsReader::open(File::open(directory.join(format!("{zone}.s3d")))?)?;
    let bytes = archive
        .get(&format!("{zone}.wld"))?
        .ok_or("missing zone world")?;
    let doc = WldDoc::parse(&bytes).map_err(|_| "invalid zone world")?;
    let regions = eq_client_assets::regions::ZoneRegions::from_doc(&doc)?;
    println!("Boundary regions: {}", regions.boundary_region_count());
    let query = args
        .map(|value| value.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()?;
    if !query.is_empty() {
        let position: [f32; 3] = query
            .try_into()
            .map_err(|_| "supply exactly three WLD coordinates")?;
        println!("Boundary query: {:?}", regions.zone_line_at(position));
    }
    for tree in doc.fragment_iter::<WorldTree>() {
        println!("BSP nodes: {}", tree.world_nodes.len());
    }
    for region in doc.fragment_iter::<Zone>() {
        println!(
            "{}: {} regions, tag {:?}",
            doc.get_string(region.name_reference).unwrap_or("unnamed"),
            region.regions.len(),
            region.user_data
        );
    }
    Ok(())
}

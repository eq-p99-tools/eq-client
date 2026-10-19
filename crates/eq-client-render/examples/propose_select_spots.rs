//! Proposes a character select spot for each installed zone from its shape
//! (`eq_client_core::select_spots::propose`), printing lines for the spots
//! file, every one unreviewed. It reads the installation only and opens no
//! window.
//!
//! `cargo run --release -p eq-client-render --example propose_select_spots -- <EverQuest folder> [zone ...]`
//!
//! Without zones named, it tries every zone in the zone table whose files
//! the installation has. Zones it can't load or place are said on stderr.
use bevy::math::Vec3;
use eq_client_core::select_spots::{self, Spots};
use std::path::PathBuf;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: propose_select_spots <EverQuest folder> [zone ...]");
        std::process::exit(2);
    };
    let named: Vec<String> = arguments.collect();
    let zones = if named.is_empty() {
        installed(&directory)
    } else {
        named
    };
    let listed = Spots::built_in();
    for zone in zones {
        if listed.get(&zone).is_some_and(|spot| spot.reviewed) {
            eprintln!("{zone}: reviewed already, kept");
            continue;
        }
        let asset = match eq_client_assets::load_zone(&directory, &zone) {
            Ok(asset) => asset,
            Err(error) => {
                eprintln!("{zone}: not loaded: {error}");
                continue;
            }
        };
        let Some(world) = eq_client_render::zone_collision(&asset) else {
            eprintln!("{zone}: no collision");
            continue;
        };
        let Some((min, max)) = asset.bounds() else {
            eprintln!("{zone}: no geometry");
            continue;
        };
        let regions = &asset.regions;
        let wet = |point: Vec3| regions.liquid_at(point.to_array()).is_some();
        match select_spots::propose(&world, (Vec3::from_array(min), Vec3::from_array(max)), wet) {
            Some(spot) => println!("{}", spot.line(&zone)),
            None => eprintln!("{zone}: no floor qualifies"),
        }
    }
}

/// Every zone in the zone table whose main archive the installation has.
fn installed(directory: &std::path::Path) -> Vec<String> {
    let mut zones: Vec<String> = (0..=u32::from(u16::MAX))
        .filter_map(eq_client_core::zones::short_name)
        .filter(|zone| directory.join(format!("{zone}.s3d")).is_file())
        .map(str::to_owned)
        .collect();
    zones.sort();
    zones.dedup();
    zones
}

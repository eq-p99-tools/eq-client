#![doc = "Command-line entry point for the offline EQ zone viewer."]

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use eq_client_assets::ZoneAsset;
use eq_client_render::{ProjectionStyle, ViewerConfig};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CameraStyle {
    Perspective,
    Orthographic,
}

#[derive(Debug, Parser)]
#[command(version, about)]
struct Arguments {
    /// Path to a locally installed `EverQuest` client.
    #[arg(long, env = "EQ_CLIENT_DIR")]
    eq_dir: PathBuf,

    /// Zone short name to load from `<zone>.s3d`.
    #[arg(long, default_value = "ecommons")]
    zone: String,

    /// Camera projection used by the offline viewer.
    #[arg(long, value_enum, default_value_t = CameraStyle::Perspective)]
    camera: CameraStyle,

    /// Validate and summarize the zone without opening a window.
    #[arg(long)]
    inspect_only: bool,
}

fn main() {
    let arguments = Arguments::parse();
    let zone = match eq_client_assets::load_zone(&arguments.eq_dir, &arguments.zone) {
        Ok(zone) => zone,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    };

    print_summary(&zone);
    if arguments.inspect_only {
        return;
    }

    println!("Controls: drag the right mouse button to orbit; use the wheel to zoom.");
    eq_client_render::run(
        zone,
        ViewerConfig {
            projection: match arguments.camera {
                CameraStyle::Perspective => ProjectionStyle::Perspective,
                CameraStyle::Orthographic => ProjectionStyle::Orthographic,
            },
        },
    );
}

fn print_summary(zone: &ZoneAsset) {
    println!("Zone: {}", zone.short_name);
    println!("Primitives: {}", zone.primitives.len());
    println!("Triangles: {}", zone.triangle_count());
    println!("Textures: {}", zone.textures.len());
    if let Some((min, max)) = zone.bounds() {
        println!("Bounds: {min:?} to {max:?}");
    }
}

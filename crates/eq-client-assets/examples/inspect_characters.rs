//! Inspect locally installed character skeletons without a renderer or network session.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archive = std::env::args().nth(1).ok_or("pass a character S3D path")?;
    if let Some(model) = std::env::args().nth(2) {
        let asset = eq_client_assets::characters::load_character(Path::new(&archive), &model)?;
        println!(
            "{}: {} primitives, {} textures, clips {:?}",
            asset.name,
            asset.primitives.len(),
            asset.textures.len(),
            asset.animation_codes()
        );
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in asset.primitives.iter().flat_map(|p| &p.positions) {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        println!("Bounds: {min:?} to {max:?}");
        return Ok(());
    }
    for character in eq_client_assets::characters::inspect_archive(Path::new(&archive))? {
        println!(
            "{}: {} bones; meshes: {}",
            character.name,
            character.bone_count,
            character.meshes.join(", ")
        );
    }
    Ok(())
}

//! Verify creation models against a user-owned installation, without a server.
use eq_client_assets::characters::{CreationAppearance, load_creation_character};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("pass the installation directory")?;
    let players = [
        (1, "HU"),
        (2, "BA"),
        (3, "ER"),
        (4, "EL"),
        (5, "HI"),
        (6, "DA"),
        (7, "HA"),
        (8, "DW"),
        (9, "TR"),
        (10, "OG"),
        (11, "HO"),
        (12, "GN"),
        (128, "IK"),
        (130, "KE"),
        (330, "FR"),
    ];
    let mut errors = Vec::new();
    for (race, prefix) in players {
        for gender in 0..=1 {
            let model = format!("{prefix}{}", if gender == 0 { 'M' } else { 'F' });
            let look = CreationAppearance {
                race,
                gender,
                eyes: if race == 330 { [0, 9] } else { [1, 10] },
                hair_color: 12,
                beard_color: 12,
                ..Default::default()
            };
            match load_creation_character(Path::new(&directory), &model, look) {
                Ok(asset) => {
                    let standing = asset.clip_seconds("P01");
                    if asset
                        .head_points(&asset.joints("P01", 0.0, true))
                        .is_empty()
                    {
                        errors.push(format!("{model}: no head vertices for the close-up"));
                    }
                    let height = asset
                        .extent(&asset.joints("P01", 0.0, true), glam::Vec3::Y)
                        .zip(asset.standing_bottom())
                        .map(|((_, top), soles)| top - soles);
                    if height.is_none_or(|h| !(0.4..1.4).contains(&(h / asset.height()))) {
                        errors.push(format!("{model}: invalid standing extent {height:?}"));
                    }
                    println!(
                        "{model}: {} parts, {} textures, standing={standing:?}, height={:.2}",
                        asset.primitives.len(),
                        asset.textures.len(),
                        asset.height()
                    );
                    if standing.is_none() {
                        errors.push(format!("{model}: no standing animation"));
                    }
                }
                Err(error) => errors.push(format!("{model}: {error}")),
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }
    Ok(())
}

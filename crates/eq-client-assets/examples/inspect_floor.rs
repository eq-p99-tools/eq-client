//! Lists a zone's terrain faces near a point, in server axes, with each face's
//! WLD flags and material, and the solid floor our loader gives under the
//! point. For finding why a player falls through a floor. Reads the zone files
//! only, without connecting to a game server.
//!
//! `cargo run -p eq-client-assets --example inspect_floor -- <EQ dir> <zone> <x> <y> [radius]`
//!
//! `x` and `y` are server axes (`#goto x y z`; `/loc` prints y first).
use eq_client_axes::{to_world, wld_to_world};
use glam::Vec3;
use libeq::pfs::PfsReader;
use libeq::wld::parser::{DmSpriteDef2, MaterialDef, MaterialPalette, WldDoc};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(args.next().ok_or("supply an EQ install directory")?);
    let zone = args.next().ok_or("supply a zone short name")?;
    let at_x: f32 = args.next().ok_or("supply x")?.parse()?;
    let at_y: f32 = args.next().ok_or("supply y")?.parse()?;
    let radius: f32 = args.next().map_or(Ok(12.0), |r| r.parse())?;

    let file = std::fs::File::open(directory.join(format!("{zone}.s3d")))?;
    let mut archive = PfsReader::open(file)?;
    let bytes = archive
        .get(&format!("{zone}.wld"))?
        .ok_or("the archive has no zone wld")?;
    let doc = WldDoc::parse(&bytes).map_err(|_| "the zone wld could not be parsed")?;

    println!("## {zone} faces within {radius} of server x {at_x} y {at_y}");
    println!("mesh\tface\tflags\tpassable\tmaterial\tz_low\tz_high\tcentre_x\tcentre_y");
    let (mut near, mut passable) = (0, 0);
    for raw in doc.fragment_iter::<DmSpriteDef2>() {
        let mesh = doc.get_string(raw.name_reference).unwrap_or("?");
        let materials = material_names(&doc, raw);
        let scale = 1.0 / 2f32.powi(i32::from(raw.scale));
        let (cx, cy, cz) = raw.center;
        let corner = |index: u16| -> Option<Vec3> {
            let p = raw.positions.get(usize::from(index))?;
            let wld = Vec3::new(
                f32::from(p.0) * scale + cx,
                f32::from(p.1) * scale + cy,
                f32::from(p.2) * scale + cz,
            );
            Some(wld_to_world(wld))
        };
        let face_material = face_materials(raw);
        for (index, face) in raw.faces.iter().enumerate() {
            let (first, second, third) = face.vertex_indexes;
            let (Some(first), Some(second), Some(third)) =
                (corner(first), corner(second), corner(third))
            else {
                continue;
            };
            let centre = (first + second + third) / 3.0;
            if (centre.x - at_x).hypot(centre.y - at_y) > radius {
                continue;
            }
            near += 1;
            let flagged = face.flags & 0x10 != 0;
            passable += usize::from(flagged);
            let material = face_material
                .get(index)
                .and_then(|m| materials.get(usize::from(*m)))
                .map_or("?", String::as_str);
            println!(
                "{mesh}\t{index}\t{:#x}\t{flagged}\t{material}\t{:.2}\t{:.2}\t{:.1}\t{:.1}",
                face.flags,
                first.z.min(second.z).min(third.z),
                first.z.max(second.z).max(third.z),
                centre.x,
                centre.y
            );
        }
    }
    println!("# faces near {near}, flagged passable {passable}");

    // The floors (and ceilings) our loader gives: every solid triangle the
    // vertical crosses at each point of a short line along x through the
    // given point, as the walk ran, highest first.
    let asset = eq_client_assets::load_zone_collision(&directory, &zone)?;
    let world: Vec<[Vec3; 3]> = asset
        .collision
        .iter()
        .map(|t| t.map(|p| to_world(Vec3::from_array(p))))
        .collect();
    println!(
        "## loader floor under the line y {at_y}, x {} to {}",
        at_x - radius,
        at_x + radius
    );
    println!("x\ty\tsolid_z_levels_high_to_low\tsolid_hits");
    let mut step = at_x - radius;
    while step <= at_x + radius {
        let hits: Vec<f32> = world
            .iter()
            .filter_map(|t| height_at(t, step, at_y))
            .collect();
        let mut levels = hits.clone();
        levels.sort_by(|a, b| b.total_cmp(a));
        let shown = if levels.is_empty() {
            "none".to_owned()
        } else {
            levels
                .iter()
                .map(|z| format!("{z:.2}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        println!("{step:.1}\t{at_y:.1}\t{shown}\t{}", hits.len());
        step += 1.0;
    }
    Ok(())
}

/// The z of a triangle at (`at_x`, `at_y`), when the vertical through it
/// crosses the triangle, by barycentric weights.
fn height_at(triangle: &[Vec3; 3], at_x: f32, at_y: f32) -> Option<f32> {
    let [first, second, third] = triangle;
    let area =
        (second.y - third.y) * (first.x - third.x) + (third.x - second.x) * (first.y - third.y);
    if area.abs() < 1e-6 {
        return None;
    }
    let weight_first =
        ((second.y - third.y) * (at_x - third.x) + (third.x - second.x) * (at_y - third.y)) / area;
    let weight_second =
        ((third.y - first.y) * (at_x - third.x) + (first.x - third.x) * (at_y - third.y)) / area;
    let weight_third = 1.0 - weight_first - weight_second;
    (weight_first >= -1e-4 && weight_second >= -1e-4 && weight_third >= -1e-4)
        .then_some(weight_first * first.z + weight_second * second.z + weight_third * third.z)
}

/// The material index of each face, from the mesh's face groups.
fn face_materials(raw: &DmSpriteDef2) -> Vec<u16> {
    raw.face_material_groups
        .iter()
        .flat_map(|&(count, material)| std::iter::repeat_n(material, usize::from(count)))
        .collect()
}

/// The names of the mesh's materials, in palette order.
fn material_names(doc: &WldDoc, raw: &DmSpriteDef2) -> Vec<String> {
    let Some(palette) = doc.get::<MaterialPalette>(&raw.material_list_ref) else {
        return Vec::new();
    };
    palette
        .fragments
        .iter()
        .map(|reference| {
            doc.get::<MaterialDef>(reference)
                .and_then(|material| doc.get_string(material.name_reference))
                .unwrap_or("?")
                .to_owned()
        })
        .collect()
}

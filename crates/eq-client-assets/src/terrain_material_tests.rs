//! Synthetic archives exercise the same terrain material path as installed zones.
use crate::test_world::{Coordinates, OLD_FORMAT, WorldFile, palette, triangle, write_archive};
use crate::{BlendOpacity, MaterialMode, ZoneAsset, load_zone, load_zone_collision};
use std::io::Cursor;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn bitmap(world: &mut WorldFile, names: &[&str]) -> i32 {
    const KEY: [u8; 8] = [0x95, 0x3a, 0xc5, 0x2a, 0x95, 0x7a, 0x95, 0x6a];
    let mut bytes = words(&[0, u32::try_from(names.len() - 1).unwrap()]);
    for name in names {
        let encoded: Vec<_> = name
            .bytes()
            .chain(std::iter::once(0))
            .zip(KEY.into_iter().cycle())
            .map(|(byte, key)| byte ^ key)
            .collect();
        bytes.extend(u16::try_from(encoded.len()).unwrap().to_le_bytes());
        bytes.extend(encoded);
    }
    world.fragment(3, bytes)
}

fn png(color: [u8; 4]) -> Vec<u8> {
    let mut image = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba(color))
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    image.into_inner()
}

fn world(flags: u32, delay: Option<u32>, layered: bool, skip_frames: bool) -> Vec<u8> {
    let mut world = WorldFile::new(OLD_FORMAT);
    let a = bitmap(&mut world, &["a.png"]);
    let b = bitmap(
        &mut world,
        if layered {
            &["b.png", "a.png"]
        } else {
            &["b.png"]
        },
    );
    // Two materials sharing a first bitmap retain their own sequences and delay.
    for (index, frames, interval, mode) in [
        (0, vec![a, b, a], delay, 0x8000_0005),
        (1, vec![a, a, b], Some(70), 0x8000_0007),
    ] {
        // Bit1 is deliberately set: it must not be mistaken for skip bit6.
        let texture_flags = if interval.is_some() { 0x1a } else { 0x10 };
        let mut sprite = words(&[0, texture_flags | if skip_frames { 0x40 } else { 0 }, 3]);
        if let Some(interval) = interval {
            sprite.extend(interval.to_le_bytes());
        }
        sprite.extend(frames.into_iter().flat_map(i32::to_le_bytes));
        let texture = world.fragment(4, sprite);
        let reference = world.fragment(5, words(&[0, texture.cast_unsigned(), 0]));
        let name = world.name(&format!("M{index}_MDF"));
        let mut material = words(&[name.cast_unsigned(), flags, mode, 0]);
        material.extend(1.0_f32.to_le_bytes());
        material.extend(1.0_f32.to_le_bytes());
        material.extend(reference.to_le_bytes());
        let material = world.fragment(0x30, material);
        let palette_name = world.name(&format!("P{index}_MP"));
        let palette = world.fragment(0x31, palette(palette_name, material));
        let name = world.name(&format!("R{index}_DMSPRITEDEF"));
        world.fragment(
            0x36,
            triangle(name, palette, &Coordinates::Old([(0, 0); 3]), 1),
        );
    }
    world.bytes()
}

fn load(
    label: &str,
    flags: u32,
    delay: Option<u32>,
    missing: bool,
    layered: bool,
    skip_frames: bool,
) -> ZoneAsset {
    let directory = std::env::temp_dir().join(format!("eq-terrain-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut files = vec![
        ("test.wld", world(flags, delay, layered, skip_frames)),
        ("a.png", png([30, 60, 90, 255])),
    ];
    if !missing {
        files.push(("b.png", png([90, 60, 30, 255])));
    }
    write_archive(&directory, "test.s3d", &files);
    let result = load_zone(&directory, "test");
    let collision = load_zone_collision(&directory, "test").unwrap();
    assert_eq!(collision.textures.len(), 0);
    assert_eq!(collision.terrain_materials.len(), 0);
    std::fs::remove_dir_all(directory).unwrap();
    result.unwrap()
}

#[test]
fn material_flags_frames_and_intervals_survive_the_zone_load() {
    let zone = load("animation", 0, Some(120), false, false, false);
    assert_eq!(zone.terrain_materials.len(), zone.primitives.len());
    assert!(
        zone.terrain_materials
            .iter()
            .all(|value| !value.double_sided)
    );
    assert_eq!(zone.textures.len(), 2);
    let a = zone.primitives[0].texture.unwrap();
    let b = (0..2).find(|index| *index != a).unwrap();
    let first = zone.terrain_materials[0].animation.as_ref().unwrap();
    let second = zone.terrain_materials[1].animation.as_ref().unwrap();
    assert_eq!(first.frames, [a, b, a]);
    assert_eq!(first.frame_millis.get(), 120);
    assert!(!first.skip_frames);
    assert_eq!(second.frames, [a, a, b]);
    assert_eq!(second.frame_millis.get(), 70);
    assert_eq!(
        zone.primitives[0].material_mode,
        MaterialMode::Blended(BlendOpacity::Half)
    );
    assert_eq!(zone.primitives[1].material_mode, MaterialMode::Masked);
}

#[test]
fn explicitly_two_sided_materials_stay_two_sided() {
    let zone = load("sided", 1, Some(120), false, false, false);
    assert!(
        zone.terrain_materials
            .iter()
            .all(|value| value.double_sided)
    );
}

#[test]
fn unknown_timing_missing_frames_and_layers_keep_the_still_texture() {
    for (label, delay, missing, layered) in [
        ("zero", Some(0), false, false),
        ("no-delay", None, false, false),
        ("missing", Some(120), true, false),
        ("layered", Some(120), false, true),
    ] {
        let zone = load(label, 0, delay, missing, layered, false);
        assert!(zone.terrain_materials[0].animation.is_none());
        assert!(zone.primitives[0].texture.is_some());
    }
}

#[test]
fn only_the_authored_skip_bit_permits_frame_skipping() {
    let zone = load("skip", 0, Some(120), false, false, true);
    assert!(
        zone.terrain_materials[0]
            .animation
            .as_ref()
            .unwrap()
            .skip_frames
    );
}

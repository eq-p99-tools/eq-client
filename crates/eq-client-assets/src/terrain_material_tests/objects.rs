//! Complete synthetic archives exercise object palettes, shared names and posing.
use super::{bitmap, png, words};
use crate::{
    MaterialMode, ZoneAsset,
    actors::{ActorAsset, load_installed_actor},
    load_zone, load_zone_collision,
    test_world::{OLD_FORMAT, WorldFile, material, mesh, palette_of, write_archive},
};

#[derive(Clone, Copy)]
enum Incomplete {
    MissingImage,
    Layered,
    NoInterval,
    ZeroInterval,
    CurrentFrame,
}

fn actor(world: &mut WorldFile, name: &str, sprite: i32) {
    let name = world.name(name);
    world.fragment(
        0x14,
        words(&[
            name.cast_unsigned(),
            0,
            0,
            0,
            1,
            0,
            sprite.cast_unsigned(),
            0,
        ]),
    );
}

fn object_world(incomplete: Option<Incomplete>) -> Vec<u8> {
    let mut world = WorldFile::new(OLD_FORMAT);
    let a = bitmap(&mut world, &["a.png"]);
    let b = bitmap(&mut world, &["b.png"]);
    let c = bitmap(
        &mut world,
        if matches!(incomplete, Some(Incomplete::Layered)) {
            &["c.png", "a.png"]
        } else {
            &["c.png"]
        },
    );
    let name = world.name("STATIC_MDF");
    let solid = world.fragment(0x30, material(name, 0x8000_0001));
    let name = world.name("HIDDEN_MDF");
    let hidden = world.fragment(0x30, material(name, 0x8000_0000));
    let mut sprites = Vec::new();
    for (index, frames) in [[a, b, a, c], [a, c, b, a]].into_iter().enumerate() {
        let flags = 0x1a
            | if index == 1 { 0x40 } else { 0 }
            | if index == 0 && matches!(incomplete, Some(Incomplete::CurrentFrame)) {
                0x20
            } else {
                0
            };
        let timed = index != 0 || !matches!(incomplete, Some(Incomplete::NoInterval));
        let flags = if timed { flags } else { flags & !0x10 };
        let mut body = words(&[0, flags, 4]);
        if flags & 0x20 != 0 {
            body.extend(1_u32.to_le_bytes());
        }
        if timed {
            let interval = if index == 0 && matches!(incomplete, Some(Incomplete::ZeroInterval)) {
                0
            } else if index == 0 {
                100_u32
            } else {
                200
            };
            body.extend(interval.to_le_bytes());
        }
        body.extend(frames.into_iter().flat_map(i32::to_le_bytes));
        let texture = world.fragment(4, body);
        let reference = world.fragment(5, words(&[0, texture.cast_unsigned(), 0]));
        // Identical names must not substitute another palette's interval or frames.
        let name = world.name("SAME_MDF");
        let mut body = material(name, 0x8000_000b);
        body[24..28].copy_from_slice(&reference.to_le_bytes());
        let animated = world.fragment(0x30, body);
        let name = world.name("SAME_MP");
        let palette = world.fragment(0x31, palette_of(name, &[hidden, solid, animated]));
        let name = world.name("SAME_DMSPRITEDEF");
        let mesh = world.fragment(
            0x36,
            mesh(
                name,
                palette,
                &[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
                &[[0, 1, 2]; 3],
                &[(1, 0), (1, 1), (1, 2)],
                &[],
            ),
        );
        let sprite = world.fragment(0x2d, words(&[0, mesh.cast_unsigned(), 0]));
        actor(
            &mut world,
            if index == 0 {
                "ONE_ACTORDEF"
            } else {
                "TWO_ACTORDEF"
            },
            sprite,
        );
        sprites.push(sprite);
    }
    // Two rigid bone meshes, each referencing its own same-named material.
    let name = world.name("POSED_HS_DEF");
    let mut body = words(&[name.cast_unsigned(), 0, 2, 0]);
    for sprite in sprites {
        body.extend(words(&[0, 0, 0, sprite.cast_unsigned(), 0]));
    }
    let skeleton = world.fragment(0x10, body);
    let sprite = world.fragment(0x11, words(&[0, skeleton.cast_unsigned(), 0]));
    actor(&mut world, "POSED_ACTORDEF", sprite);
    world.bytes()
}

fn load(label: &str, incomplete: Option<Incomplete>) -> (ZoneAsset, ActorAsset) {
    let directory = std::env::temp_dir().join(format!(
        "eq-object-animation-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut placements = WorldFile::new(OLD_FORMAT);
    for name in ["ONE_ACTORDEF", "TWO_ACTORDEF", "POSED_ACTORDEF"] {
        let name = placements.name(name);
        let mut body = words(&[0, name.cast_unsigned(), 2, 0]);
        body.extend([0_u8; 28]);
        body.extend(0_u32.to_le_bytes());
        placements.fragment(0x15, body);
    }
    write_archive(
        &directory,
        "test.s3d",
        &[
            ("test.wld", WorldFile::new(OLD_FORMAT).bytes()),
            ("objects.wld", placements.bytes()),
        ],
    );
    let objects = object_world(incomplete);
    let mut textures = vec![
        ("a.png", png([30, 60, 90, 255])),
        ("b.png", png([90, 60, 30, 255])),
    ];
    if !matches!(incomplete, Some(Incomplete::MissingImage)) {
        textures.push(("c.png", png([40, 80, 120, 255])));
    }
    let mut files = textures.clone();
    files.push(("test_obj.wld", objects.clone()));
    write_archive(&directory, "test_obj.s3d", &files);
    textures.push(("test_chr.wld", objects));
    write_archive(&directory, "test_chr.s3d", &textures);
    let zone = load_zone(&directory, "test").unwrap();
    let collision = load_zone_collision(&directory, "test").unwrap();
    assert!(collision.textures.is_empty() && collision.terrain_materials.is_empty());
    assert_eq!(collision.objects.len(), zone.objects.len());
    for (solid, whole) in collision.models.iter().zip(&zone.models) {
        assert_eq!(solid.collision, whole.collision);
        assert!(solid.materials.is_empty() && solid.primitives.is_empty());
    }
    let actor = load_installed_actor(&directory, "test", "ONE").unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    (zone, actor)
}

#[test]
fn object_palettes_keep_their_own_frames_and_intervals_through_static_and_posed_loading() {
    let (zone, actor) = load("identity", None);
    assert_eq!(zone.objects.len(), 3);
    for (name, interval, order, skip) in [
        ("ONE_ACTORDEF", 100, [0, 1, 0, 2], false),
        ("TWO_ACTORDEF", 200, [0, 2, 1, 0], true),
    ] {
        let model = zone.models.iter().find(|model| model.name == name).unwrap();
        assert_eq!(model.materials.len(), 2);
        assert!(model.materials[0].animation.is_none());
        assert_eq!(model.primitives[1].material_mode, MaterialMode::Additive);
        let animation = model.materials[1].animation.as_ref().unwrap();
        assert_eq!(animation.frame_millis.get(), interval);
        assert_eq!(animation.skip_frames, skip);
        let colors = [[30, 60, 90, 255], [90, 60, 30, 255], [40, 80, 120, 255]];
        let actual: Vec<_> = animation
            .frames
            .iter()
            .map(|&index| zone.textures[index].rgba8[..4].to_vec())
            .collect();
        assert_eq!(actual, order.map(|index| colors[index].to_vec()));
    }
    let posed = zone
        .models
        .iter()
        .find(|model| model.name == "POSED_ACTORDEF")
        .unwrap();
    let intervals: Vec<_> = posed
        .materials
        .iter()
        .filter_map(|material| material.animation.as_ref().map(|a| a.frame_millis.get()))
        .collect();
    assert_eq!(intervals, [100, 200]);
    // Named carrier/boat actors retain metadata in the shared loader too.
    assert_eq!(
        actor.model.materials[1]
            .animation
            .as_ref()
            .unwrap()
            .frame_millis
            .get(),
        100
    );
}

#[test]
fn incomplete_object_sequences_retain_the_initial_texture_without_guessing_timing() {
    for (label, incomplete) in [
        ("missing", Incomplete::MissingImage),
        ("layered", Incomplete::Layered),
        ("untimed", Incomplete::NoInterval),
        ("zero", Incomplete::ZeroInterval),
        ("current", Incomplete::CurrentFrame),
    ] {
        let (zone, _) = load(label, Some(incomplete));
        let model = zone
            .models
            .iter()
            .find(|m| m.name == "ONE_ACTORDEF")
            .unwrap();
        assert!(model.materials[1].animation.is_none());
        let texture = model.primitives[1].texture.unwrap();
        assert_eq!(&zone.textures[texture].rgba8[..4], [30, 60, 90, 255]);
    }
}

//! Small installed zones written byte by byte, for other crates' tests that
//! need the real loader's output.

use crate::test_world::{
    OLD_FORMAT, WorldFile, material, material_with_flags, mesh, mesh_with_flags, palette,
    write_archive,
};
use std::path::Path;

/// Writes two solid WLD faces: the clockwise face at x0 faces up, and
/// the counter-clockwise face at x10 faces down. Coordinates are WLD's.
pub fn write_floor_winding(install: &Path) {
    let mut zone = WorldFile::new(OLD_FORMAT);
    let material_name = zone.name("FLOOR_MDF");
    let material = zone.fragment(0x30, material(material_name, 0x8000_0001));
    let palette_name = zone.name("FLOOR_MP");
    let palette = zone.fragment(0x31, palette(palette_name, material));
    let mesh_name = zone.name("FLOOR_DMSPRITEDEF");
    zone.fragment(
        0x36,
        mesh(
            mesh_name,
            palette,
            &[
                [0, 0, 0],
                [0, 4, 0],
                [4, 0, 0],
                [10, 0, 0],
                [14, 0, 0],
                [10, 4, 0],
            ],
            &[[0, 1, 2], [3, 4, 5]],
            &[(2, 0)],
            &[],
        ),
    );
    write_archive(
        install,
        "floor_winding.s3d",
        &[("floor_winding.wld", zone.bytes())],
    );
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// Writes the zone `grove` into `install`: a floor at height 0, 100 across
/// and 150 long, and two trees of the same shape placed 50 apart along the
/// file's x.
///
/// Each tree is a branch, a level face 3 above the floor running 10 out
/// from the tree's origin and 4 wide, ending at its trunk, a two-sided wall
/// 20 high. `NOHIT_ACTORDEF`'s mesh is in read R49-49c's no-hit case, as
/// every installed tree is (0x36 flags 0x4003, no +0x14 reference).
/// `SOLID_ACTORDEF`'s is a referenced mesh as buildings are (0x14003).
pub fn write_grove(install: &Path) {
    let mut zone = WorldFile::new(OLD_FORMAT);
    let (m, p, t) = (
        zone.name("M_MDF"),
        zone.name("P_MP"),
        zone.name("FLOOR_DMSPRITEDEF"),
    );
    let floor_material = zone.fragment(0x30, material(m, 0x8000_0001));
    let floor_palette = zone.fragment(0x31, palette(p, floor_material));
    zone.fragment(
        0x36,
        mesh(
            t,
            floor_palette,
            &[[-50, -50, 0], [100, -50, 0], [100, 50, 0], [-50, 50, 0]],
            &[[0, 2, 1], [0, 3, 2]],
            &[(2, 0)],
            &[],
        ),
    );

    let mut placements = WorldFile::new(OLD_FORMAT);
    for (name, x) in [("NOHIT_ACTORDEF", 0.0_f32), ("SOLID_ACTORDEF", 50.0)] {
        let model = placements.name(name);
        let mut placement = words(&[0, model.cast_unsigned(), 0x02, 0]);
        for value in [x, 0.0, 0.0, 0.0, 0.0, 0.0] {
            placement.extend(value.to_le_bytes());
        }
        placement.extend(words(&[0, 0]));
        placements.fragment(0x15, placement);
    }

    let mut objects = WorldFile::new(OLD_FORMAT);
    let (w, p) = (objects.name("W_MDF"), objects.name("P_MP"));
    let solid = objects.fragment(0x30, material_with_flags(w, 0x8000_0001, 0x2));
    let object_palette = objects.fragment(0x31, palette(p, solid));
    // The branch (vertices 0 to 3, facing up) and the trunk's wall at its
    // end (4 to 7, both ways).
    let positions = [
        [0, -2, 3],
        [10, -2, 3],
        [10, 2, 3],
        [0, 2, 3],
        [10, -2, 0],
        [10, 2, 0],
        [10, 2, 20],
        [10, -2, 20],
    ];
    let faces = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [4, 6, 5],
        [4, 7, 6],
    ];
    for (actor_name, mesh_name, flags) in [
        ("NOHIT_ACTORDEF", "NOHIT_DMSPRITEDEF", 0x4003),
        ("SOLID_ACTORDEF", "SOLID_DMSPRITEDEF", 0x0001_4003),
    ] {
        let name = objects.name(mesh_name);
        let tree = objects.fragment(
            0x36,
            mesh_with_flags(
                name,
                object_palette,
                &positions,
                &faces,
                &[(6, 0)],
                &[],
                flags,
            ),
        );
        let sprite = objects.fragment(0x2d, words(&[0, tree.cast_unsigned(), 0]));
        // A model: name, flags, callback, no actions, one sprite reference,
        // no bounds, the sprite, unknown.
        let actor = objects.name(actor_name);
        objects.fragment(
            0x14,
            words(&[
                actor.cast_unsigned(),
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

    write_archive(
        install,
        "grove.s3d",
        &[
            ("grove.wld", zone.bytes()),
            ("objects.wld", placements.bytes()),
        ],
    );
    write_archive(
        install,
        "grove_obj.s3d",
        &[("grove_obj.wld", objects.bytes())],
    );
}

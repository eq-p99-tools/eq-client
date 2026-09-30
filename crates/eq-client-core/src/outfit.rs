//! Which materials dress a classic character model in its worn gear. The
//! server says what each texture slot shows ([`Appearance`]); classic models
//! name their materials by body part, material and piece, such as
//! `HUMCH0001_MDF` (human chest, material 0, piece 1), and ship a set of
//! looks for each part. This module only chooses names: the renderer loads
//! them and keeps the base look when a model lacks one.
pub use eq_network_game::appearance::{Appearance, TextureSlot, WearChange};

/// Armor materials classic models ship textures for: leather, chain, plate
/// and, on some models, a fourth look. Robes (10 and up) and later
/// race-specific materials are not drawn yet and keep the base look.
const DRAWN_MATERIALS: std::ops::RangeInclusive<u32> = 1..=4;

/// A classic material name split into its parts.
struct MaterialName<'a> {
    model: &'a str,
    part: &'a str,
    piece: &'a str,
}

/// Splits names such as `HUMCH0001_MDF`; None for any other shape.
fn parse(name: &str) -> Option<MaterialName<'_>> {
    let stem = name.strip_suffix("_MDF").unwrap_or(name);
    if stem.len() != 9 || !stem.is_ascii() || !stem[5..].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(MaterialName {
        model: &stem[..3],
        part: &stem[3..5],
        piece: &stem[7..],
    })
}

/// The texture slot that dresses a body part, by its two-letter code.
fn slot(part: &str) -> Option<TextureSlot> {
    Some(match part {
        "HE" => TextureSlot::Head,
        "CH" => TextureSlot::Chest,
        "UA" => TextureSlot::Arms,
        "FA" => TextureSlot::Wrist,
        "HN" => TextureSlot::Hands,
        "LG" => TextureSlot::Legs,
        "FT" => TextureSlot::Feet,
        _ => return None,
    })
}

/// The material a primitive draws with for this appearance, from the one it
/// uses in the base look. Armor swaps the two material digits (`HUMCH0201_MDF`
/// for a chain tunic); a head swaps its face digit (`HUMHE0031_MDF` for face 3);
/// anything else keeps its base material.
#[must_use]
pub fn dressed(base: &str, appearance: &Appearance) -> String {
    let base = base.to_ascii_uppercase();
    let Some(name) = parse(&base) else {
        return base;
    };
    match slot(name.part) {
        Some(TextureSlot::Head) => {
            // Face pieces: the first digit is the face, the second the piece.
            let piece = &name.piece[1..];
            format!("{}HE00{}{piece}_MDF", name.model, appearance.face.min(9))
        }
        Some(slot) if DRAWN_MATERIALS.contains(&appearance.material(slot)) => format!(
            "{}{}{:02}{}_MDF",
            name.model,
            name.part,
            appearance.material(slot),
            name.piece
        ),
        _ => base,
    }
}

/// The tint on the slot that dresses this base material, if any. Faces are
/// never tinted.
#[must_use]
pub fn tint(base: &str, appearance: &Appearance) -> Option<[u8; 3]> {
    let base = base.to_ascii_uppercase();
    let name = parse(&base)?;
    slot(name.part)
        .filter(|slot| *slot != TextureSlot::Head)
        .and_then(|slot| appearance.tint(slot))
}

/// The item model held in a hand, such as `IT10`, when one is held.
#[must_use]
pub fn held_model(appearance: &Appearance, slot: TextureSlot) -> Option<String> {
    let number = appearance.material(slot);
    (slot.held() && number > 0).then(|| format!("IT{number}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wearing(slot: TextureSlot, material: u32) -> Appearance {
        let mut look = Appearance::default();
        look.materials[slot.index()] = material;
        look
    }

    #[test]
    fn armor_swaps_the_material_digits_of_its_body_part() {
        let chain = wearing(TextureSlot::Chest, 2);
        assert_eq!(dressed("HUMCH0001_MDF", &chain), "HUMCH0201_MDF");
        assert_eq!(dressed("humch0002_mdf", &chain), "HUMCH0202_MDF");
        // Other parts keep their base look.
        assert_eq!(dressed("HUMLG0003_MDF", &chain), "HUMLG0003_MDF");
        let plate_legs = wearing(TextureSlot::Legs, 3);
        assert_eq!(dressed("ELFLG0003_MDF", &plate_legs), "ELFLG0303_MDF");
        for (part, slot) in [
            ("UA", TextureSlot::Arms),
            ("FA", TextureSlot::Wrist),
            ("HN", TextureSlot::Hands),
            ("FT", TextureSlot::Feet),
        ] {
            let look = wearing(slot, 1);
            assert_eq!(
                dressed(&format!("DWM{part}0001_MDF"), &look),
                format!("DWM{part}0101_MDF")
            );
        }
    }

    #[test]
    fn robes_later_materials_and_other_names_keep_the_base_look() {
        for material in [0, 10, 16, 17, 99] {
            let look = wearing(TextureSlot::Chest, material);
            assert_eq!(dressed("HUMCH0001_MDF", &look), "HUMCH0001_MDF");
        }
        let chain = wearing(TextureSlot::Chest, 2);
        for name in ["CLK0401_MDF", "HUMCH01_MDF", "IT63", "HUMXX0001_MDF"] {
            assert_eq!(dressed(name, &chain), name.to_ascii_uppercase());
        }
    }

    #[test]
    fn heads_show_the_face_and_keep_their_piece() {
        let mut look = Appearance::default();
        assert_eq!(dressed("HUMHE0001_MDF", &look), "HUMHE0001_MDF");
        look.face = 3;
        assert_eq!(dressed("HUMHE0001_MDF", &look), "HUMHE0031_MDF");
        assert_eq!(dressed("HUMHE0002_MDF", &look), "HUMHE0032_MDF");
        // A helm does not change the face texture.
        look.materials[TextureSlot::Head.index()] = 2;
        assert_eq!(dressed("HUMHE0002_MDF", &look), "HUMHE0032_MDF");
    }

    #[test]
    fn tints_follow_the_slot_but_never_the_face() {
        let mut look = wearing(TextureSlot::Chest, 2);
        look.tints[TextureSlot::Chest.index()] = Some([200, 10, 10]);
        look.tints[TextureSlot::Head.index()] = Some([1, 2, 3]);
        assert_eq!(tint("HUMCH0001_MDF", &look), Some([200, 10, 10]));
        assert_eq!(tint("HUMLG0001_MDF", &look), None);
        assert_eq!(tint("HUMHE0001_MDF", &look), None);
        assert_eq!(tint("CLK0401_MDF", &look), None);
    }

    #[test]
    fn hands_hold_item_models_by_number() {
        let look = wearing(TextureSlot::Primary, 10);
        assert_eq!(
            held_model(&look, TextureSlot::Primary).as_deref(),
            Some("IT10")
        );
        assert_eq!(held_model(&look, TextureSlot::Secondary), None);
        assert_eq!(
            held_model(&wearing(TextureSlot::Chest, 2), TextureSlot::Chest),
            None
        );
    }
}

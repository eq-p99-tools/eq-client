//! Which meshes and materials dress a classic character model in its worn
//! gear. The server says what each texture slot shows ([`Appearance`]);
//! classic models name their materials by body part, material and piece, such
//! as `HUMCH0001_MDF` (human chest, material 0, piece 1), and ship a set of
//! looks for each part, plus whole bodies and heads to swap in for robes and
//! helms ([`shape`]). This module only chooses: the renderer loads what it
//! names and keeps the base look when a model lacks one.
pub use eq_network_game::appearance::{Appearance, TextureSlot, WearChange};

/// Armor materials classic models ship textures for: leather, chain, plate
/// and, on some models, a fourth look. Robes (10 and up) swap the whole body
/// instead ([`shape`]); later race-specific materials keep the base look.
const DRAWN_MATERIALS: std::ops::RangeInclusive<u32> = 1..=4;

/// Races whose models have a robe body: human, erudite, high elf, dark elf,
/// gnome and iksar.
const ROBED_RACES: [u32; 6] = [1, 3, 5, 6, 12, 128];

/// Chest materials that put those races in their robe body.
const ROBES: std::ops::RangeInclusive<u32> = 10..=23;

/// The body and head a model draws with, numbered as its meshes are (`HUM01`
/// is body 1, `HUMHE03` head 3); 0 is the base body and the bare head.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Shape {
    /// The body.
    pub body: u8,
    /// The head.
    pub head: u8,
}

/// The body and head an appearance draws with, given which ones the model
/// has. A robe puts a robe-wearing race in body 1. A head material picks the
/// head of that number (1 to 3 are the leather, chain and plate helms) when
/// `helm` says the helm shows: the official client shows every other
/// character's helm, whatever the server's show-helm flag says, and leaves
/// the player's own to the player's show-helm option.
#[must_use]
pub fn shape(
    race: u32,
    appearance: &Appearance,
    helm: bool,
    has_body: impl Fn(u8) -> bool,
    has_head: impl Fn(u8) -> bool,
) -> Shape {
    let robed = ROBED_RACES.contains(&race)
        && ROBES.contains(&appearance.material(TextureSlot::Chest))
        && has_body(1);
    let head = u8::try_from(appearance.material(TextureSlot::Head))
        .ok()
        .filter(|head| helm && *head > 0 && has_head(*head))
        .unwrap_or(0);
    Shape {
        body: u8::from(robed),
        head,
    }
}

/// The robe texture set a chest material draws: the classic robes 10 to 16
/// draw sets 4 to 10 (`CLK0401_MDF` to `CLK1006_MDF`).
fn robe_set(appearance: &Appearance) -> Option<u32> {
    let material = appearance.material(TextureSlot::Chest);
    (10..=16).contains(&material).then(|| material - 6)
}

/// A robe material in another set: the robe body's `CLK04` pieces, or the
/// hood an erudite's bare head wears (`CLKERM06_MDF`). None for any other name.
fn robe(base: &str, set: u32) -> Option<String> {
    let stem = base.strip_suffix("_MDF")?.strip_prefix("CLK")?;
    let piece = match stem.strip_prefix("04") {
        Some(piece) if piece.len() == 2 => piece,
        _ if stem.len() == 5 && stem.ends_with("06") => "06",
        _ => return None,
    };
    Some(format!("CLK{set:02}{piece}_MDF"))
}

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
/// a robe swaps its set (`CLK0701_MDF` for robe 13); anything else keeps its
/// base material.
#[must_use]
pub fn dressed(base: &str, appearance: &Appearance) -> String {
    let base = base.to_ascii_uppercase();
    if let Some(robed) = robe_set(appearance).and_then(|set| robe(&base, set)) {
        return robed;
    }
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

/// The tint on the slot that dresses this base material, if any. Robes take
/// the chest's tint and a helm the head's (`helm` says the material is on a
/// helmed head); faces are never tinted.
#[must_use]
pub fn tint(base: &str, helm: bool, appearance: &Appearance) -> Option<[u8; 3]> {
    let base = base.to_ascii_uppercase();
    if base.starts_with("CLK") {
        return appearance.tint(TextureSlot::Chest);
    }
    match parse(&base).and_then(|name| slot(name.part)) {
        Some(TextureSlot::Head) => None,
        Some(slot) => appearance.tint(slot),
        None if helm => appearance.tint(TextureSlot::Head),
        None => None,
    }
}

/// The item model held in a hand, such as `IT10`, when one is held.
#[must_use]
pub fn held_model(appearance: &Appearance, slot: TextureSlot) -> Option<String> {
    let number = appearance.material(slot);
    (slot.held() && number > 0).then(|| format!("IT{number}"))
}

/// Whether an off-hand item goes on the shield point instead of in the left
/// hand. Titanium servers say only which model a hand holds, not what kind of
/// item it is, so this is decided from the model: the classic shields are
/// IT200 through IT299, and later models (IT10000 and up) are shields when
/// they are flat like one. Flat classic models outside that range, such as
/// broad blades, stay in the hand.
#[must_use]
pub fn on_shield_point(number: u32, flat: bool) -> bool {
    (200..300).contains(&number) || (number >= 10_000 && flat)
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
        assert_eq!(tint("HUMCH0001_MDF", false, &look), Some([200, 10, 10]));
        assert_eq!(tint("HUMLG0001_MDF", false, &look), None);
        // Faces never take a tint, even under a helm; the helm itself does.
        assert_eq!(tint("HUMHE0001_MDF", true, &look), None);
        assert_eq!(tint("HELM14_MDF", true, &look), Some([1, 2, 3]));
        assert_eq!(tint("HELM14_MDF", false, &look), None);
        // Robes take the chest's tint.
        assert_eq!(tint("CLK0401_MDF", false, &look), Some([200, 10, 10]));
    }

    #[test]
    fn robes_put_robe_wearing_races_in_their_robe_body_and_set() {
        let robe = wearing(TextureSlot::Chest, 13);
        let all = |_| true;
        assert_eq!(shape(1, &robe, true, all, all), Shape { body: 1, head: 0 });
        // Races without robes, and models without the body, keep their own.
        assert_eq!(shape(2, &robe, true, all, all).body, 0);
        assert_eq!(shape(1, &robe, true, |_| false, all).body, 0);
        let chest = wearing(TextureSlot::Chest, 3);
        assert_eq!(shape(1, &chest, true, all, all).body, 0);
        // Robe 13 draws set 7, on the robe body and on an erudite's hood.
        assert_eq!(dressed("CLK0401_MDF", &robe), "CLK0701_MDF");
        assert_eq!(dressed("clk0406_mdf", &robe), "CLK0706_MDF");
        assert_eq!(dressed("CLKERM06_MDF", &robe), "CLK0706_MDF");
        let last = wearing(TextureSlot::Chest, 16);
        assert_eq!(dressed("CLK0401_MDF", &last), "CLK1001_MDF");
        // Later robes keep the default set.
        let later = wearing(TextureSlot::Chest, 17);
        assert_eq!(dressed("CLK0401_MDF", &later), "CLK0401_MDF");
    }

    #[test]
    fn shown_helms_pick_the_head_of_their_material() {
        let plate = wearing(TextureSlot::Head, 3);
        let all = |_| true;
        // The server's own flag does not hide a helm.
        assert!(!plate.show_helm);
        assert_eq!(shape(1, &plate, true, all, all).head, 3);
        // A hidden helm keeps the bare head.
        assert_eq!(shape(1, &plate, false, all, all).head, 0);
        // A model without that head keeps the bare one.
        assert_eq!(shape(1, &plate, true, all, |head| head < 3).head, 0);
        let bare = wearing(TextureSlot::Head, 0);
        assert_eq!(shape(1, &bare, true, all, all), Shape::default());
    }

    #[test]
    fn classic_shields_and_flat_later_models_go_on_the_shield_point() {
        assert!(on_shield_point(200, true));
        assert!(on_shield_point(228, false));
        assert!(on_shield_point(10_530, true));
        assert!(!on_shield_point(10_653, false));
        // A flat classic blade stays in the hand.
        assert!(!on_shield_point(40, true));
        assert!(!on_shield_point(48, false));
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

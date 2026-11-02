//! Creation looks decoded from the user's installed models and color palette.
use super::{CharacterAsset, Piece, invalid};
use crate::{LoadError, ZoneTexture, characters::DonorArchives, skeleton::Skin};
use std::{collections::HashMap, fs, path::Path};

/// A creation look, independent of a network server or rendering engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct CreationAppearance {
    /// Player race ID.
    pub race: u32,
    /// Male (0) or female (1).
    pub gender: u32,
    /// Base face, without packed detail.
    pub face: u8,
    /// Hair model or erudite head detail; 255 removes attached hair.
    pub hair: u8,
    /// Installed hair palette index.
    pub hair_color: u8,
    /// Beard model or face detail; 255 removes an attached beard.
    pub beard: u8,
    /// Installed beard palette index.
    pub beard_color: u8,
    /// Left and right eye palette indexes.
    pub eyes: [u8; 2],
    /// Barbarian tattoo index.
    pub tattoo: u8,
    /// Classic face textures instead of Luclin models.
    pub classic: bool,
}

impl CreationAppearance {
    fn detail(self) -> u8 {
        match (self.race, self.gender) {
            (2, _) => self.tattoo,
            (3, _) => self.hair,
            (5..=7, 0) | (8, 1) => self.beard,
            _ => 0,
        }
    }

    fn material(self, model: &str, name: &str) -> String {
        let Some(rest) = name.strip_prefix(model) else {
            return name.into();
        };
        let bytes = rest.as_bytes();
        if bytes.len() != 10
            || !rest.ends_with("_MDF")
            || !bytes[2..6].iter().all(u8::is_ascii_digit)
        {
            return name.into();
        }
        let head = rest.starts_with("HE");
        let whole_body = !self.classic && matches!(self.race, 130 | 330);
        if !head && !whole_body {
            return name.into();
        }
        let detail = if head && !self.classic {
            self.detail()
        } else {
            0
        };
        format!(
            "{model}{}{:02}{}{}_MDF",
            &rest[..2],
            detail,
            self.face,
            char::from(bytes[5])
        )
    }

    fn eye(self, side: usize) -> u32 {
        let offset = match self.race {
            6 => 20,
            128 => 40,
            130 => 60,
            10 => 80,
            9 => 100,
            330 => 120,
            _ => 0,
        };
        // These faces have a fixed scarred right eye (Titanium 0x47b2a9).
        let color = match (side, self.race, self.gender, self.face) {
            (1, 1, 0, 3) | (1, 2, 0, 6) => 209,
            (1, 8, 0, 5) | (1, 2, 0, 4) => 207,
            _ => u32::from(self.eyes[side]),
        };
        offset + color
    }
}

/// Loads a complete creation preview from an installed client. No game assets
/// or palettes are built into this crate.
///
/// # Errors
/// Reports missing or malformed installed models, textures or palettes.
pub fn load_creation_character(
    directory: &Path,
    model: &str,
    look: CreationAppearance,
) -> Result<CharacterAsset, LoadError> {
    let mut asset = if look.classic {
        crate::characters::load_global_character(directory, model, &DonorArchives::default())?
    } else {
        let stem = match look.race {
            330 => "globalpcfroglok_chr".into(),
            _ => format!("global{}_chr", model.to_ascii_lowercase()),
        };
        let donors = [
            "globalelm_chr.s3d",
            "globalelf_chr.s3d",
            "globalhum_chr.s3d",
            "globalhuf_chr.s3d",
        ]
        .map(|name| directory.join(name));
        super::load_with_source(
            &directory.join(format!("{stem}.s3d")),
            model,
            &donors,
            &DonorArchives::default(),
            match model {
                "ERM" => Some("HUM"),
                "ERF" => Some("HUF"),
                "HIM" | "DAM" | "HAM" => Some("ELM"),
                "HIF" | "DAF" | "HAF" => Some("ELF"),
                "FRF" => Some("FRM"),
                _ => None,
            },
        )?
    };
    asset.dress_materials(|name| look.material(model, name))?;
    if !look.classic {
        apply_layers(&mut asset, directory, look)?;
        apply_eyes(&mut asset, directory, look)?;
        apply_attachments(&mut asset, directory, look)?;
        // Luclin WLD UVs address DDS rows bottom-up. Decode/composite first,
        // then orient the finished textures for the renderer's top-down images.
        for texture in &mut asset.textures {
            if texture.name.to_ascii_lowercase().ends_with(".dds") {
                let mut image = image::RgbaImage::from_raw(
                    texture.width,
                    texture.height,
                    std::mem::take(&mut texture.rgba8),
                )
                .ok_or_else(|| invalid("appearance texture dimensions"))?;
                image::imageops::flip_vertical_in_place(&mut image);
                texture.rgba8 = image.into_raw();
            }
        }
    }
    Ok(asset)
}

// Luclin face details are layered textures, not whole-face tints. The WLD
// names an opaque skin followed by alpha overlays with a synthetic `_layer`
// suffix. Only the overlay receives the beard color; skin stays unchanged.
fn apply_layers(
    asset: &mut CharacterAsset,
    directory: &Path,
    look: CreationAppearance,
) -> Result<(), LoadError> {
    let (mut archive, bytes) = super::open_world(asset.source())?;
    let (_, world) = crate::parse_wld(&bytes, &asset.name)?;
    let layers: HashMap<_, Vec<_>> = world
        .materials()
        .filter_map(|material| {
            let layers: Vec<_> = material
                .base_color_texture()?
                .iter_sources()
                .filter_map(|name| name.strip_suffix("_layer").map(str::to_owned))
                .collect();
            let name = material.name()?.to_ascii_uppercase();
            (!layers.is_empty()).then_some((name, layers))
        })
        .collect();
    let beard_color = if matches!((look.race, look.gender), (5..=7, 0) | (8, 1)) && look.beard != 0
    {
        *read_palette(directory)?
            .get(usize::from(look.beard_color))
            .ok_or_else(|| invalid("beard color index"))?
    } else {
        [255; 3]
    };
    let mut replacements = HashMap::new();
    let mut textures = Vec::new();
    let mut indices = HashMap::new();
    for (primitive, material) in asset.primitives.iter_mut().zip(&asset.materials) {
        let Some(name) = material else {
            continue;
        };
        let Some(layers) = layers.get(name) else {
            continue;
        };
        if let Some(index) = replacements.get(name) {
            primitive.texture = Some(*index);
            continue;
        }
        let Some(index) = primitive.texture else {
            continue;
        };
        let mut texture = asset.textures[index].clone();
        let color = if name.starts_with(&format!("{}HE", asset.name))
            && ["1_MDF", "3_MDF"].iter().any(|end| name.ends_with(end))
        {
            beard_color
        } else {
            [255; 3]
        };
        for layer in layers {
            let index = crate::load_texture(&mut archive, layer, &mut textures, &mut indices)?
                .ok_or_else(|| invalid(&format!("installed appearance layer {layer}")))?;
            composite(&mut texture, &textures[index], color)?;
        }
        let index = asset.textures.len();
        asset.textures.push(texture);
        primitive.texture = Some(index);
        replacements.insert(name.clone(), index);
    }
    Ok(())
}

fn composite(base: &mut ZoneTexture, layer: &ZoneTexture, color: [u8; 3]) -> Result<(), LoadError> {
    let image = image::RgbaImage::from_raw(layer.width, layer.height, layer.rgba8.clone())
        .ok_or_else(|| invalid("appearance layer dimensions"))?;
    let layer = image::imageops::resize(
        &image,
        base.width,
        base.height,
        image::imageops::FilterType::Triangle,
    );
    for (base, layer) in base
        .rgba8
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(layer.pixels())
    {
        let alpha = u32::from(layer[3]);
        for i in 0..3 {
            let tinted = u32::from(layer[i]) * u32::from(color[i]) / 255;
            base[i] = u8::try_from((u32::from(base[i]) * (255 - alpha) + tinted * alpha) / 255)
                .unwrap_or(255);
        }
    }
    Ok(())
}

fn apply_eyes(
    asset: &mut CharacterAsset,
    directory: &Path,
    look: CreationAppearance,
) -> Result<(), LoadError> {
    split_shared_eyes(asset);
    let stem = if look.race == 330 {
        "frogequip"
    } else {
        "lgequip"
    };
    let path = directory.join(format!("{stem}.s3d"));
    let (mut archive, bytes) = super::open_world(&path)?;
    let (_, world) = crate::parse_wld(&bytes, stem)?;
    let replacements: Vec<_> = world
        .materials()
        .filter_map(|m| {
            Some((
                m.name()?.to_ascii_uppercase(),
                m.base_color_texture()?.source()?,
            ))
        })
        .collect();
    for side in 0..2 {
        let eye_name = format!("CHR_EYE{:03}_MDF", look.eye(side));
        let Some((_, texture)) = replacements.iter().find(|(name, _)| *name == eye_name) else {
            if look.eyes[side] == 0 {
                continue;
            }
            return Err(invalid(&format!("installed eye material {eye_name}")));
        };
        let texture = crate::load_texture(
            &mut archive,
            texture,
            &mut asset.textures,
            &mut HashMap::new(),
        )?;
        let target = format!("{}{}_EYE_MDF", asset.name, ["L", "R"][side]);
        for (primitive, material) in asset.primitives.iter_mut().zip(&asset.materials) {
            if material.as_deref() == Some(&target) {
                primitive.texture = texture;
            }
        }
    }
    Ok(())
}

// Froglok eyes share one material. Separate its two disconnected sides so
// independent eye choices retain their own textures without changing the skin.
fn split_shared_eyes(asset: &mut CharacterAsset) {
    let Some(index) = asset
        .materials
        .iter()
        .position(|name| name.as_deref() == Some("CHR_EYE120_MDF"))
    else {
        return;
    };
    let mut right = asset.primitives[index].clone();
    let (left_indices, right_indices): (Vec<[u32; 3]>, Vec<[u32; 3]>) = right
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .copied()
        .partition(|triangle| {
            // In the unrotated WLD model's render frame, forward is +X and left -Z.
            triangle
                .iter()
                .map(|index| right.positions[*index as usize][2])
                .sum::<f32>()
                < 0.0
        });
    asset.primitives[index].indices = left_indices.into_iter().flatten().collect();
    right.indices = right_indices.into_iter().flatten().collect();
    asset.materials[index] = Some(format!("{}L_EYE_MDF", asset.name));
    asset
        .materials
        .push(Some(format!("{}R_EYE_MDF", asset.name)));
    asset.primitives.push(right);
    asset.skins.push(asset.skins[index].clone());
    asset.pieces.push(asset.pieces[index]);
}

// The item IDs and attachment names used by both client families. Model data
// itself is read from lgequip/lgequip2 (Titanium 0x47b1f1 and 0x47b511).
const ITEM_BASE: [(u32, u32); 14] = [
    (1, 360),
    (2, 540),
    (3, 480),
    (4, 240),
    (5, 300),
    (6, 180),
    (7, 60),
    (8, 120),
    (9, 660),
    (10, 720),
    (11, 780),
    (12, 0),
    (128, 420),
    (130, 600),
];

fn apply_attachments(
    asset: &mut CharacterAsset,
    directory: &Path,
    look: CreationAppearance,
) -> Result<(), LoadError> {
    let Some((_, base)) = ITEM_BASE.iter().find(|(race, _)| *race == look.race) else {
        return Ok(());
    };
    let mut palette = None;
    for (slot, (style, color)) in [(look.hair, look.hair_color), (look.beard, look.beard_color)]
        .into_iter()
        .enumerate()
    {
        let Some(bone) = asset.cosmetics[slot] else {
            continue;
        };
        let available = match slot {
            0 => {
                matches!(look.race, 1 | 2 | 4..=8 | 11 | 12)
                    || matches!((look.race, look.gender), (9 | 10, 1))
                    || look.race == 130 && look.face == 7
            }
            _ => look.gender == 0 && matches!(look.race, 1 | 2 | 3 | 8 | 11 | 12),
        };
        if style == 255 || !available {
            continue;
        }
        let id = base + look.gender * 30 + [1000, 2000][slot] + u32::from(style);
        let item_name = format!("IT{id}");
        let mut item = super::load_character(&directory.join("lgequip.s3d"), &item_name)
            .or_else(|_| super::load_character(&directory.join("lgequip2.s3d"), &item_name))?;
        if look.race != 130 {
            let colors = if let Some(colors) = palette {
                colors
            } else {
                let colors = read_palette(directory)?;
                palette = Some(colors);
                colors
            };
            let color = colors
                .get(usize::from(color))
                .ok_or_else(|| invalid("appearance color index"))?;
            for texture in &mut item.textures {
                tint(texture, *color);
            }
        }
        let offset = asset.textures.len();
        asset.textures.extend(item.textures);
        for mut primitive in item.primitives {
            let skin = Skin {
                positions: primitive.positions.clone(),
                normals: primitive.normals.clone(),
                bones: vec![bone; primitive.positions.len()],
            };
            primitive.texture = primitive.texture.map(|index| index + offset);
            asset.primitives.push(primitive);
            asset.skins.push(skin);
            asset.materials.push(None);
            asset.pieces.push(Piece::Fixed);
        }
    }
    let posed = asset.pose("", 0.0);
    for (primitive, pose) in asset.primitives.iter_mut().zip(posed) {
        primitive.positions = pose.positions;
        primitive.normals = pose.normals;
    }
    Ok(())
}

fn tint(texture: &mut ZoneTexture, color: [u8; 3]) {
    let channel = |value: u8, color: u8| {
        u8::try_from(u16::from(value) * u16::from(color) / 255).unwrap_or(255)
    };
    for pixel in texture.rgba8.as_chunks_mut::<4>().0 {
        for i in 0..3 {
            pixel[i] = channel(pixel[i], color[i]);
        }
    }
    texture.color_key = texture
        .color_key
        .map(|key| std::array::from_fn(|i| channel(key[i], color[i])));
}

fn read_palette(directory: &Path) -> Result<[[u8; 3]; 24], LoadError> {
    let path = directory.join("eqgame.exe");
    let bytes = fs::read(&path).map_err(|source| LoadError::OpenArchive { path, source })?;
    palette_from_pe(&bytes).ok_or_else(|| {
        invalid(
            "installed client appearance palette (supported Titanium or EQMac executable required)",
        )
    })
}

fn palette_from_pe(bytes: &[u8]) -> Option<[[u8; 3]; 24]> {
    let word = |at: usize| {
        Some(u16::from_le_bytes(
            bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let dword = |at: usize| {
        Some(u32::from_le_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = usize::try_from(dword(0x3c)?).ok()?;
    if bytes.get(pe..pe.checked_add(4)?)? != b"PE\0\0" || word(pe + 24)? != 0x10b {
        return None;
    }
    let base = dword(pe + 52)?;
    let sections = pe.checked_add(24 + usize::from(word(pe + 20)?))?;
    for address in [0x0069_89a0_u32, 0x0060_2850] {
        let Some(rva) = address.checked_sub(base) else {
            continue;
        };
        for i in 0..usize::from(word(pe + 6)?) {
            let section = sections.checked_add(i.checked_mul(40)?)?;
            let Some(offset) = rva.checked_sub(dword(section + 12)?) else {
                continue;
            };
            if offset.checked_add(96)? > dword(section + 16)? {
                continue;
            }
            let at = usize::try_from(dword(section + 20)?.checked_add(offset)?).ok()?;
            let data = bytes.get(at..at.checked_add(96)?)?;
            if data.as_chunks::<4>().0.iter().any(|color| color[3] != 0)
                || data.get(48..51)? != [255, 255, 255]
            {
                continue;
            }
            return Some(std::array::from_fn(|i| {
                [data[i * 4 + 2], data[i * 4 + 1], data[i * 4]]
            }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_details_change_only_the_corresponding_installed_materials() {
        let human = CreationAppearance {
            race: 1,
            face: 7,
            ..Default::default()
        };
        assert_eq!(human.material("HUM", "HUMHE0001_MDF"), "HUMHE0071_MDF");
        assert_eq!(human.material("HUM", "HUMCH0001_MDF"), "HUMCH0001_MDF");
        let tattoo = CreationAppearance {
            race: 2,
            tattoo: 8,
            ..human
        };
        assert_eq!(tattoo.material("BAM", "BAMHE0001_MDF"), "BAMHE0871_MDF");
        let frog = CreationAppearance {
            race: 330,
            face: 9,
            ..human
        };
        assert_eq!(frog.material("FRF", "FRFCH0001_MDF"), "FRFCH0091_MDF");
        assert_eq!(frog.material("FRF", "CHR_EYE120_MDF"), "CHR_EYE120_MDF");
        assert_eq!(CreationAppearance { face: 3, ..human }.eye(1), 209);
    }

    #[test]
    fn beard_overlay_colors_hair_without_tinting_uncovered_skin() {
        let mut skin = ZoneTexture {
            name: "skin".into(),
            width: 3,
            height: 1,
            rgba8: vec![200, 150, 100, 255, 200, 150, 100, 255, 200, 150, 100, 255],
            color_key: None,
        };
        let beard = ZoneTexture {
            name: "beard".into(),
            width: 3,
            height: 1,
            rgba8: vec![255, 255, 255, 0, 255, 255, 255, 255, 255, 255, 255, 128],
            color_key: None,
        };
        composite(&mut skin, &beard, [40, 30, 20]).unwrap();
        assert_eq!(&skin.rgba8[..4], &[200, 150, 100, 255]);
        assert_eq!(&skin.rgba8[4..8], &[40, 30, 20, 255]);
        assert_eq!(&skin.rgba8[8..], &[119, 89, 59, 255]);
    }

    #[test]
    fn installed_palette_requires_a_bounded_pe_section_and_valid_color_words() {
        let mut bytes = vec![0; 608];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[134..136].copy_from_slice(&1u16.to_le_bytes());
        bytes[148..150].copy_from_slice(&224u16.to_le_bytes());
        bytes[152..154].copy_from_slice(&0x10bu16.to_le_bytes());
        bytes[180..184].copy_from_slice(&0x0060_0000u32.to_le_bytes());
        bytes[388..392].copy_from_slice(&0x0009_89a0u32.to_le_bytes());
        bytes[392..396].copy_from_slice(&96u32.to_le_bytes());
        bytes[396..400].copy_from_slice(&512u32.to_le_bytes());
        for (index, color) in bytes[512..].as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let n = u8::try_from(index).unwrap();
            color.copy_from_slice(&[n, n + 1, n + 2, 0]);
        }
        bytes[560..563].fill(255);
        assert_eq!(palette_from_pe(&bytes).unwrap()[0], [2, 1, 0]);
        for length in 0..bytes.len() {
            assert!(palette_from_pe(&bytes[..length]).is_none());
        }
        bytes[563] = 1;
        assert!(palette_from_pe(&bytes).is_none());
    }
}

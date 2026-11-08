//! The installed classic zone's placed point lights.
use glam::Vec3;
use libeq::wld::parser::{PointLight, WldDoc};

/// A resolved WLD 0x28 -> 0x1c -> 0x1b chain, in renderer coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedLight {
    /// Position in the zone.
    pub position: [f32; 3],
    /// Authored radius in zone units.
    pub radius: f32,
    /// First authored RGB frame (or white for an explicitly colorless light).
    pub rgb: [f32; 3],
    /// First authored brightness frame.
    pub level: f32,
}

pub(crate) fn placed(doc: &WldDoc) -> Vec<PlacedLight> {
    doc.fragment_iter::<PointLight>()
        .filter_map(|point| {
            let reference = doc.get(&point.reference)?;
            let light = doc.get(&reference.reference)?;
            let level = *light.light_levels.as_ref()?.first()?;
            let rgb = match &light.colors {
                Some(colors) => {
                    let &(red, green, blue) = colors.first()?;
                    [red, green, blue]
                }
                None => [1.0; 3],
            };
            let position = eq_client_axes::from_wld(Vec3::new(point.x, point.y, point.z));
            if !position.is_finite()
                || !point.radius.is_finite()
                || point.radius <= 0.0
                || !level.is_finite()
                || level <= 0.0
                || !rgb
                    .iter()
                    .all(|channel| channel.is_finite() && *channel >= 0.0)
            {
                return None;
            }
            Some(PlacedLight {
                position: position.to_array(),
                radius: point.radius,
                rgb,
                level,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_world::{OLD_FORMAT, WorldFile};

    #[test]
    fn placed_lights_resolve_the_wld_chain_and_skip_unresolved_or_invalid_ones() {
        let mut file = WorldFile::new(OLD_FORMAT);
        let words = |words: &[u32]| words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let rgb = [1.0_f32, 0.5, 0.25];
        let source = file.fragment(
            0x1b,
            words(&[
                0,
                0x14,
                1,
                0.75_f32.to_bits(),
                rgb[0].to_bits(),
                rgb[1].to_bits(),
                rgb[2].to_bits(),
            ]),
        );
        let reference = file.fragment(0x1c, words(&[0, source.cast_unsigned(), 0]));
        for (link, radius) in [(reference, 30.0_f32), (reference, 0.0), (999, 30.0)] {
            file.fragment(
                0x28,
                words(&[
                    0,
                    link.cast_unsigned(),
                    0x100,
                    2.0_f32.to_bits(),
                    3.0_f32.to_bits(),
                    4.0_f32.to_bits(),
                    radius.to_bits(),
                ]),
            );
        }
        let doc = WldDoc::parse(&file.bytes()).unwrap();
        assert_eq!(
            placed(&doc),
            [PlacedLight {
                position: eq_client_axes::from_wld(Vec3::new(2.0, 3.0, 4.0)).to_array(),
                radius: 30.0,
                rgb,
                level: 0.75,
            }]
        );
    }
}

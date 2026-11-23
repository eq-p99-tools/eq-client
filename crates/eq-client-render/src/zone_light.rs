//! The graphics engine's single-pass lighting for zone geometry, worked out
//! per vertex as the engine's vertex shaders do it (EQGraphicsDX9.dll; see
//! `docs/zone-lighting.md`).
//!
//! By default (`MultiPassLighting` off, eqclient.ini `[Defaults]`, read at
//! 0x1007fb50) the engine draws region and placed-object geometry with
//! single-pass effects: each vertex sums the ambient by its weight, its own
//! baked light, the directional light and up to three point lights; the
//! sum saturates to 0-1 and multiplies the texture (stage 0 MODULATE).

/// What the frame routine 0x10076f20 hands the effects, each already
/// halved there: the zone ambient and the special ambient (the floor plus
/// the scene's third colour, clamped to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BaseInputs {
    pub ambient: [f32; 3],
    pub special: [f32; 3],
}

impl BaseInputs {
    /// The inputs for eqgame.exe's ambient and floor (0x441295), as the
    /// engine's setters at scene vtable +0xa4 and +0xa8 store them. The
    /// third colour (scene +0xd0) has no setter read yet and counts as none
    /// (inferred).
    pub(super) fn new(ambient: [f32; 3], floor: [f32; 3]) -> Self {
        Self {
            ambient: ambient.map(|channel| channel.clamp(0.0, 1.0) * 0.5),
            special: floor.map(|channel| channel.clamp(0.0, 1.0) * 0.5),
        }
    }
}

/// The light a vertex with baked light `baked` (RGB, and the alpha that
/// weights the ambient) sums before point lights: the ambient by its
/// weight plus the special ambient, past the 0.12 offset and scaled by
/// 1.111, plus the baked light at full weight (the `RegionC1` vertex
/// shader). The scene's directional light is left out: in a zone without a
/// sky its value isn't read and the scene has no sun (inferred).
pub(super) fn base_light(baked: [u8; 4], inputs: BaseInputs) -> [f32; 3] {
    let weight = f32::from(baked[3]) / 255.0;
    std::array::from_fn(|channel| {
        let sum = inputs.ambient[channel] * weight + inputs.special[channel];
        ((sum - 0.12) * 1.111).max(0.0) + f32::from(baked[channel]) / 255.0
    })
}

/// One point light in a draw's list: its position, colour and radius R
/// (the light's +0x5c; taken as the placed light's radius, or a carried
/// light's range: inferred).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PassLight {
    pub position: [f32; 3],
    pub radius: f32,
    pub rgb: [f32; 3],
}

impl PassLight {
    /// The squared distance from the light to a box's nearest point.
    pub(super) fn distance_squared(&self, (low, high): ([f32; 3], [f32; 3])) -> f32 {
        (0..3)
            .map(|axis| {
                let at = self.position[axis].clamp(low[axis], high[axis]);
                (self.position[axis] - at).powi(2)
            })
            .sum()
    }
}

/// How many point lights a draw takes: the first three of its list
/// (0x100756e0).
pub(super) const SLOTS: usize = 3;

/// What the light in `slot` adds at a vertex with a unit normal (the
/// `RegionC1` vertex shader, as 0x100756e0 feeds it): slots 0 and 1 the
/// colour times min(0.05 R² / d², 2) times N·L, with no cutoff; slot 2
/// the colour times (N·L)², with no distance term. The 0.05 R² strength
/// is the one for ordinary meshes; which mesh types take R itself isn't
/// matched to zone geometry, so every mesh takes 0.05 R² (inferred).
pub(super) fn point_light(
    slot: usize,
    light: &PassLight,
    at: [f32; 3],
    normal: [f32; 3],
) -> [f32; 3] {
    let toward: [f32; 3] = std::array::from_fn(|axis| light.position[axis] - at[axis]);
    let squared: f32 = toward.iter().map(|axis| axis * axis).sum();
    let facing = if squared > 0.0 {
        (toward.iter().zip(normal).map(|(a, b)| a * b).sum::<f32>() / squared.sqrt()).max(0.0)
    } else {
        1.0
    };
    let factor = if slot >= 2 {
        facing * facing
    } else {
        let strength = 0.05 * light.radius * light.radius;
        let falloff = if squared > 0.0 {
            (strength / squared).min(2.0)
        } else {
            2.0
        };
        falloff * facing
    };
    light.rgb.map(|channel| channel * factor)
}

/// The colour the texture is multiplied by: the vertex's sum, saturated to
/// 0-1 by the colour interpolator, as a linear vertex colour (alpha 1).
///
/// The engine multiplies in gamma space and interpolates the saturated
/// colour across a triangle in gamma space; this renderer multiplies and
/// interpolates linear values. Converting the vertex colour by the sRGB
/// curve keeps a vertex's own product within 0.06 (on the 0-1 display
/// scale) of the engine's; between vertices the blend differs slightly. A WGSL material
/// that multiplies in gamma space would close both.
pub(super) fn vertex_colour(light: [f32; 3]) -> [f32; 4] {
    let [red, green, blue] = light.map(|channel| linear(channel.clamp(0.0, 1.0)));
    [red, green, blue, 1.0]
}

/// The largest difference, on the 0-1 display scale, between the engine's
/// gamma-space texture × colour and this renderer's linear product of the
/// two, over every pair of 8-bit values (checked by a test).
#[cfg(test)]
const GAMMA_ERROR: f32 = 0.06;

/// The sRGB curve from gamma to linear.
fn linear(channel: f32) -> f32 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn a_palace_vertex_is_lit_by_its_own_light_alone() {
        // A 0.5 ambient at no weight and the 0.08 floor: 0.04 stays under
        // the 0.12 offset, so the vertex draws its texture times its colour.
        let inputs = BaseInputs::new([0.5; 3], [0.08; 3]);
        assert!(close(
            base_light([200, 100, 0, 0], inputs),
            [200.0 / 255.0, 100.0 / 255.0, 0.0]
        ));
    }

    #[test]
    fn the_ambient_reaches_a_vertex_by_its_alpha_past_the_offset() {
        let inputs = BaseInputs::new([0.5; 3], [0.08; 3]);
        // Full weight: (0.25 + 0.04 - 0.12) × 1.111.
        assert!(close(base_light([0, 0, 0, 255], inputs), [0.17 * 1.111; 3]));
        // A 0.1 zone's ambient and floor stay under the offset: black.
        let dim = BaseInputs::new([0.1; 3], [0.08; 3]);
        assert!(close(base_light([0, 0, 0, 255], dim), [0.0; 3]));
        // Ultravision's floor, 80/80/120 halved, clears it at no weight,
        // blue most.
        let ultra = BaseInputs::new([0.1; 3], [80.0 / 255.0, 80.0 / 255.0, 120.0 / 255.0]);
        let light = base_light([0, 0, 0, 0], ultra);
        let past = |floor: f32| (floor / 255.0 * 0.5 - 0.12) * 1.111;
        assert!(close(light, [past(80.0), past(80.0), past(120.0)]));
    }

    #[test]
    fn a_point_light_falls_off_by_the_inverse_square_capped_at_two() {
        // R = 20: strength 0.05 × 400 = 20.
        let light = PassLight {
            position: [0.0, 10.0, 0.0],
            radius: 20.0,
            rgb: [1.0, 0.5, 0.0],
        };
        let up = [0.0, 1.0, 0.0];
        // 10 below: 20 / 100.
        assert!(close(
            point_light(0, &light, [0.0, 0.0, 0.0], up),
            [0.2, 0.1, 0.0]
        ));
        // Close up the factor stops at 2.
        assert!(close(
            point_light(1, &light, [0.0, 9.0, 0.0], up),
            [2.0, 1.0, 0.0]
        ));
        // Past the radius it still adds a little: 20 / 900.
        let far = point_light(0, &light, [0.0, -20.0, 0.0], up);
        assert!((far[0] - 20.0 / 900.0).abs() < 1e-6);
        // Slot 2 has no distance term, only (N·L)².
        assert!(close(
            point_light(2, &light, [0.0, -500.0, 0.0], up),
            [1.0, 0.5, 0.0]
        ));
        // A face turned away gets nothing.
        assert_eq!(
            point_light(0, &light, [0.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            [0.0; 3]
        );
        assert_eq!(
            point_light(2, &light, [0.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            [0.0; 3]
        );
        // A box's nearest point.
        assert!((light.distance_squared(([-1.0, 0.0, -1.0], [1.0, 5.0, 1.0])) - 25.0).abs() < 1e-6);
    }

    #[test]
    fn the_sum_saturates_before_it_multiplies_the_texture() {
        let inputs = BaseInputs::new([1.0; 3], [1.0; 3]);
        let light = base_light([255, 255, 255, 255], inputs);
        assert!(light[0] > 1.0);
        assert_eq!(vertex_colour(light), [1.0; 4]);
        assert_eq!(vertex_colour([0.0; 3]), [0.0, 0.0, 0.0, 1.0]);
        assert!((vertex_colour([0.5; 3])[0] - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn the_linear_product_stays_near_the_gamma_product() {
        let mut worst = 0.0_f32;
        for texture in 0..=255_u8 {
            for colour in 0..=255_u8 {
                let (texture, colour) = (f32::from(texture) / 255.0, f32::from(colour) / 255.0);
                let ours = linear(texture) * linear(colour);
                let engine = linear(texture * colour);
                // Compared on the display scale, where the engine works.
                let display = |value: f32| {
                    if value <= 0.003_130_8 {
                        value * 12.92
                    } else {
                        1.055 * value.powf(1.0 / 2.4) - 0.055
                    }
                };
                worst = worst.max((display(ours) - display(engine)).abs());
            }
        }
        assert!(worst <= GAMMA_ERROR, "worst {worst}");
    }
}

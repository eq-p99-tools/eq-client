//! The run controls' icons, drawn in code so they need no asset files and no
//! glyphs the bundled font lacks: ▶ run, ‖ pause, |▶ resume, ▶| step, ■ stop.

use bevy::prelude::*;

/// Square icon size in texels; drawn smaller, so edges stay smooth.
const SIZE: u32 = 32;
/// Subsamples per texel side, for anti-aliased edges.
const SAMPLES: u32 = 4;

/// One filled piece of an icon, in unit coordinates with y down.
#[derive(Clone, Copy)]
enum Piece {
    Bar {
        left: f32,
        right: f32,
    },
    /// A right-pointing triangle from `left` to `right`, full height.
    Play {
        left: f32,
        right: f32,
    },
    Square,
}

impl Piece {
    fn contains(self, x: f32, y: f32) -> bool {
        const TOP: f32 = 0.16;
        const BOTTOM: f32 = 0.84;
        if !(TOP..=BOTTOM).contains(&y) {
            return false;
        }
        match self {
            Self::Bar { left, right } => (left..=right).contains(&x),
            Self::Play { left, right } => {
                // The point's distance from the vertical middle narrows the
                // triangle towards its tip.
                let half = (BOTTOM - TOP) / 2.0;
                let from_middle = (y - (TOP + half)).abs() / half;
                x >= left && x <= right - (right - left) * from_middle
            }
            Self::Square => (0.2..=0.8).contains(&x) && (0.2..=0.8).contains(&y),
        }
    }
}

/// The controls with an icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Run,
    Pause,
    Resume,
    Step,
    Stop,
}

impl Icon {
    fn pieces(self) -> &'static [Piece] {
        match self {
            Self::Run => &[Piece::Play {
                left: 0.24,
                right: 0.84,
            }],
            Self::Pause => &[
                Piece::Bar {
                    left: 0.24,
                    right: 0.42,
                },
                Piece::Bar {
                    left: 0.58,
                    right: 0.76,
                },
            ],
            Self::Resume => &[
                Piece::Bar {
                    left: 0.14,
                    right: 0.28,
                },
                Piece::Play {
                    left: 0.38,
                    right: 0.9,
                },
            ],
            Self::Step => &[
                Piece::Play {
                    left: 0.1,
                    right: 0.62,
                },
                Piece::Bar {
                    left: 0.72,
                    right: 0.86,
                },
            ],
            Self::Stop => &[Piece::Square],
        }
    }

    /// The icon's colour, so run and stop read at a glance.
    pub(super) const fn tint(self) -> Color {
        match self {
            Self::Run | Self::Resume => Color::srgb(0.55, 0.87, 0.49),
            Self::Stop => Color::srgb(1.0, 0.45, 0.42),
            Self::Pause | Self::Step => Color::srgb(0.92, 0.93, 0.95),
        }
    }

    /// The tint while the control can't be pressed: faint, so it reads as
    /// off on its greyed button.
    pub(super) fn dimmed(self) -> Color {
        self.tint().with_alpha(0.3)
    }

    /// White coverage in alpha; the node's colour tints it.
    pub(super) fn image(self) -> Image {
        let pieces = self.pieces();
        let mut pixels = image::RgbaImage::new(SIZE, SIZE);
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "small texel counts and a coverage in 0..=255"
        )]
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let mut covered = 0;
            for sub_y in 0..SAMPLES {
                for sub_x in 0..SAMPLES {
                    let at = |texel: u32, sub: u32| {
                        ((texel * SAMPLES + sub) as f32 + 0.5) / (SIZE * SAMPLES) as f32
                    };
                    let (u, v) = (at(x, sub_x), at(y, sub_y));
                    if pieces.iter().any(|piece| piece.contains(u, v)) {
                        covered += 1;
                    }
                }
            }
            let alpha = (covered * 255 / (SAMPLES * SAMPLES)) as u8;
            *pixel = image::Rgba([255, 255, 255, alpha]);
        }
        let mut texture = crate::sheets::image(pixels);
        texture.sampler = bevy::image::ImageSampler::linear();
        texture
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(icon: Icon, x: u32, y: u32) -> u8 {
        let image = icon.image();
        let index = ((y * SIZE + x) * 4 + 3) as usize;
        image.data.as_ref().expect("pixels")[index]
    }

    #[test]
    fn icons_cover_their_shapes_and_leave_their_margins_clear() {
        let middle = SIZE / 2;
        for icon in [Icon::Run, Icon::Pause, Icon::Resume, Icon::Step, Icon::Stop] {
            assert_eq!(alpha(icon, 0, 0), 0, "{icon:?}");
            assert_eq!(alpha(icon, SIZE - 1, SIZE - 1), 0, "{icon:?}");
        }
        // A play triangle is solid along its base and at its middle, and
        // clear past its sloping edge near the top.
        assert_eq!(alpha(Icon::Run, 8, 7), 255);
        assert_eq!(alpha(Icon::Run, 24, middle), 255);
        assert_eq!(alpha(Icon::Run, 25, 7), 0);
        // Pause is two bars with a gap between them.
        assert_eq!(alpha(Icon::Pause, 10, middle), 255);
        assert_eq!(alpha(Icon::Pause, middle, middle), 0);
        assert_eq!(alpha(Icon::Pause, 21, middle), 255);
        // Resume has its bar first, step has it last.
        assert_eq!(alpha(Icon::Resume, 6, 7), 255);
        assert_eq!(alpha(Icon::Resume, 27, 7), 0);
        assert_eq!(alpha(Icon::Step, 4, middle), 255);
        assert_eq!(alpha(Icon::Step, 25, 7), 255);
        assert_eq!(alpha(Icon::Stop, middle, middle), 255);
    }
}

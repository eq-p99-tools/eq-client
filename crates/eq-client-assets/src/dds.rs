//! `DirectDraw` Surface (DDS) textures, read here rather than by the `image`
//! crate, whose reader takes only block-compressed (DXT) surfaces whose sides
//! are multiples of four. Community texture packs for the classic client also
//! ship uncompressed 16, 24 and 32-bit surfaces, and DXT surfaces with odd
//! sides (258 or 1026 pixels, say), which the original client draws. Only the
//! top mip level is read; the renderer builds its own chain.

use image::error::{DecodingError, ImageFormatHint, UnsupportedError, UnsupportedErrorKind};
use image::{ImageError, ImageFormat, RgbaImage};

const MAGIC: &[u8; 4] = b"DDS ";
/// The magic, then the 124-byte header, then the surface data.
const DATA_OFFSET: usize = 128;
const PF_ALPHA_PIXELS: u32 = 0x1;
const PF_ALPHA: u32 = 0x2;
const PF_FOUR_CC: u32 = 0x4;
const PF_RGB: u32 = 0x40;
const PF_LUMINANCE: u32 = 0x2_0000;
/// Larger than any texture the client ships; stops a corrupt header from
/// asking for gigabytes.
const MAX_SIDE: u32 = 16_384;

/// Whether `bytes` start like a DDS file.
pub(crate) fn is_dds(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Decodes a texture file to RGBA: DDS here, every other format by `image`.
///
/// # Errors
/// As [`decode`] for DDS files, and as `image` for the rest.
pub(crate) fn decode_image(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    if is_dds(bytes) {
        decode(bytes)
    } else {
        image::load_from_memory(bytes).map(image::DynamicImage::into_rgba8)
    }
}

/// Decodes the top level of a DDS file to RGBA.
///
/// # Errors
/// Rejects truncated files, impossible sizes and pixel formats other than
/// DXT1/DXT3/DXT5 and uncompressed RGB(A), luminance or alpha masks.
pub(crate) fn decode(bytes: &[u8]) -> Result<RgbaImage, ImageError> {
    if !is_dds(bytes) {
        return Err(decoding("missing DDS magic"));
    }
    let height = word(bytes, 12)?;
    let width = word(bytes, 16)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(decoding(format!("impossible DDS size {width}x{height}")));
    }
    let flags = word(bytes, 80)?;
    let data = bytes.get(DATA_OFFSET..).unwrap_or_default();
    if flags & PF_FOUR_CC != 0 {
        let four_cc = bytes.get(84..88).ok_or_else(truncated)?;
        let block = match four_cc {
            b"DXT1" => Block::Dxt1,
            b"DXT2" | b"DXT3" => Block::Dxt3,
            b"DXT4" | b"DXT5" => Block::Dxt5,
            other => {
                return Err(unsupported(format!(
                    "DDS four-character code {:?}",
                    String::from_utf8_lossy(other)
                )));
            }
        };
        return decode_blocks(data, width, height, block);
    }
    let masks = Masks {
        bits: word(bytes, 88)?,
        red: word(bytes, 92)?,
        green: word(bytes, 96)?,
        blue: word(bytes, 100)?,
        alpha: if flags & (PF_ALPHA_PIXELS | PF_ALPHA) == 0 {
            0
        } else {
            word(bytes, 104)?
        },
        luminance: flags & PF_LUMINANCE != 0,
    };
    if flags & (PF_RGB | PF_LUMINANCE | PF_ALPHA) == 0 {
        return Err(unsupported(format!("DDS pixel format flags {flags:#x}")));
    }
    decode_masked(data, width, height, &masks)
}

#[derive(Clone, Copy)]
enum Block {
    Dxt1,
    Dxt3,
    Dxt5,
}

impl Block {
    fn size(self) -> usize {
        match self {
            Self::Dxt1 => 8,
            Self::Dxt3 | Self::Dxt5 => 16,
        }
    }
}

fn decode_blocks(
    data: &[u8],
    width: u32,
    height: u32,
    block: Block,
) -> Result<RgbaImage, ImageError> {
    let (across, down) = (width.div_ceil(4), height.div_ceil(4));
    let needed = across as usize * down as usize * block.size();
    let data = data.get(..needed).ok_or_else(truncated)?;
    let mut image = RgbaImage::new(width, height);
    for (index, bytes) in (0_u32..).zip(data.chunks_exact(block.size())) {
        let texels = match block {
            Block::Dxt1 => color_block(bytes, true),
            Block::Dxt3 => {
                let mut texels = color_block(&bytes[8..], false);
                explicit_alpha(&bytes[..8], &mut texels);
                texels
            }
            Block::Dxt5 => {
                let mut texels = color_block(&bytes[8..], false);
                interpolated_alpha(&bytes[..8], &mut texels);
                texels
            }
        };
        let (bx, by) = ((index % across) * 4, (index / across) * 4);
        for (texel, rgba) in (0_u32..).zip(texels.iter()) {
            let (x, y) = (bx + texel % 4, by + texel / 4);
            // Blocks past an odd edge hold padding the surface doesn't show.
            if x < width && y < height {
                image.put_pixel(x, y, image::Rgba(*rgba));
            }
        }
    }
    Ok(image)
}

/// The 16 texels of a colour block, row by row. DXT1 blocks whose first
/// colour is not the greater one have three colours and transparent black;
/// DXT3 and DXT5 blocks always have four.
fn color_block(bytes: &[u8], dxt1: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([bytes[0], bytes[1]]);
    let c1 = u16::from_le_bytes([bytes[2], bytes[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |wa: u16, wb: u16, total: u16| {
        let channel = |i: usize| byte((u16::from(a[i]) * wa + u16::from(b[i]) * wb) / total);
        [channel(0), channel(1), channel(2), 255]
    };
    let palette = if c0 > c1 || !dxt1 {
        [a, b, mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [a, b, mix(1, 1, 2), [0, 0, 0, 0]]
    };
    let indices = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    std::array::from_fn(|texel| palette[((indices >> (texel * 2)) & 0b11) as usize])
}

fn rgb565(color: u16) -> [u8; 4] {
    let red = ((color >> 11) & 0x1f) as u8;
    let green = ((color >> 5) & 0x3f) as u8;
    let blue = (color & 0x1f) as u8;
    [
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
        255,
    ]
}

fn explicit_alpha(bytes: &[u8], texels: &mut [[u8; 4]; 16]) {
    let bits = u64::from_le_bytes(bytes.try_into().expect("an 8-byte alpha block"));
    for (texel, rgba) in texels.iter_mut().enumerate() {
        rgba[3] = ((bits >> (texel * 4)) & 0xf) as u8 * 17;
    }
}

fn interpolated_alpha(bytes: &[u8], texels: &mut [[u8; 4]; 16]) {
    let (a0, a1) = (u16::from(bytes[0]), u16::from(bytes[1]));
    let mut palette = [0_u8; 8];
    palette[0] = bytes[0];
    palette[1] = bytes[1];
    if a0 > a1 {
        for i in 1_u16..7 {
            palette[usize::from(i) + 1] = byte(((7 - i) * a0 + i * a1) / 7);
        }
    } else {
        for i in 1_u16..5 {
            palette[usize::from(i) + 1] = byte(((5 - i) * a0 + i * a1) / 5);
        }
        palette[6] = 0;
        palette[7] = 255;
    }
    let mut bits = [0_u8; 8];
    bits[..6].copy_from_slice(&bytes[2..8]);
    let bits = u64::from_le_bytes(bits);
    for (texel, rgba) in texels.iter_mut().enumerate() {
        rgba[3] = palette[((bits >> (texel * 3)) & 0b111) as usize];
    }
}

struct Masks {
    bits: u32,
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
    luminance: bool,
}

fn decode_masked(
    data: &[u8],
    width: u32,
    height: u32,
    masks: &Masks,
) -> Result<RgbaImage, ImageError> {
    let bytes_per_pixel = match masks.bits {
        8 | 16 | 24 | 32 => (masks.bits / 8) as usize,
        other => return Err(unsupported(format!("{other}-bit uncompressed DDS"))),
    };
    let pitch = width as usize * bytes_per_pixel;
    let data = data.get(..pitch * height as usize).ok_or_else(truncated)?;
    let mut image = RgbaImage::new(width, height);
    for (pixel, raw) in image.pixels_mut().zip(data.chunks_exact(bytes_per_pixel)) {
        let mut word = [0_u8; 4];
        word[..bytes_per_pixel].copy_from_slice(raw);
        let value = u32::from_le_bytes(word);
        let red = channel(value, masks.red);
        let (green, blue) = if masks.luminance {
            (red, red)
        } else {
            (channel(value, masks.green), channel(value, masks.blue))
        };
        let alpha = if masks.alpha == 0 {
            255
        } else {
            channel(value, masks.alpha)
        };
        pixel.0 = [red, green, blue, alpha];
    }
    Ok(image)
}

/// The channel under `mask`, scaled to 8 bits.
fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let max = u64::from(mask >> mask.trailing_zeros());
    let raw = u64::from((value & mask) >> mask.trailing_zeros());
    byte((raw * 255 + max / 2) / max)
}

/// A value already known to fit in a byte (a weighted mean of bytes, say).
fn byte<T: TryInto<u8>>(value: T) -> u8 {
    value.try_into().unwrap_or(u8::MAX)
}

fn word(bytes: &[u8], at: usize) -> Result<u32, ImageError> {
    let field = bytes.get(at..at + 4).ok_or_else(truncated)?;
    Ok(u32::from_le_bytes(
        field.try_into().expect("a 4-byte field"),
    ))
}

fn truncated() -> ImageError {
    decoding("truncated DDS file")
}

fn decoding(message: impl Into<String>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Dds),
        message.into(),
    ))
}

fn unsupported(message: String) -> ImageError {
    ImageError::Unsupported(UnsupportedError::from_format_and_kind(
        ImageFormatHint::Exact(ImageFormat::Dds),
        UnsupportedErrorKind::GenericFeature(message),
    ))
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// A DDS header for `width` x `height` with the given pixel format.
    fn header(width: u32, height: u32, flags: u32, four_cc: [u8; 4], masks: [u32; 5]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&height.to_le_bytes());
        bytes[16..20].copy_from_slice(&width.to_le_bytes());
        bytes[76..80].copy_from_slice(&32_u32.to_le_bytes());
        bytes[80..84].copy_from_slice(&flags.to_le_bytes());
        bytes[84..88].copy_from_slice(&four_cc);
        for (i, mask) in masks.iter().enumerate() {
            bytes[88 + i * 4..92 + i * 4].copy_from_slice(&mask.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_uncompressed_argb1555() {
        let mut bytes = header(
            2,
            1,
            0x41,
            *b"\0\0\0\0",
            [16, 0x7c00, 0x03e0, 0x001f, 0x8000],
        );
        // Opaque pure red, then transparent pure blue.
        bytes.extend_from_slice(&0xfc00_u16.to_le_bytes());
        bytes.extend_from_slice(&0x001f_u16.to_le_bytes());
        let image = decode(&bytes).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 0, 255, 0]);
    }

    #[test]
    fn reads_uncompressed_rgb565_and_24_and_32_bit() {
        let mut rgb565 = header(1, 1, 0x40, *b"\0\0\0\0", [16, 0xf800, 0x07e0, 0x001f, 0]);
        rgb565.extend_from_slice(&0x07e0_u16.to_le_bytes());
        assert_eq!(decode(&rgb565).unwrap().get_pixel(0, 0).0, [0, 255, 0, 255]);

        let mut rgb24 = header(1, 1, 0x40, *b"\0\0\0\0", [24, 0xff_0000, 0xff00, 0xff, 0]);
        rgb24.extend_from_slice(&[0x30, 0x20, 0x10]);
        assert_eq!(
            decode(&rgb24).unwrap().get_pixel(0, 0).0,
            [0x10, 0x20, 0x30, 255]
        );

        let mut argb32 = header(
            1,
            1,
            0x41,
            *b"\0\0\0\0",
            [32, 0xff_0000, 0xff00, 0xff, 0xff00_0000],
        );
        argb32.extend_from_slice(&[0x30, 0x20, 0x10, 0x80]);
        assert_eq!(
            decode(&argb32).unwrap().get_pixel(0, 0).0,
            [0x10, 0x20, 0x30, 0x80]
        );
    }

    #[test]
    fn reads_dxt1_with_odd_sides_and_its_transparent_colour() {
        // 5x3: two blocks across, one down; the second block is mostly padding.
        let mut bytes = header(5, 3, 0x4, *b"DXT1", [0; 5]);
        // Block 1: three-colour mode (c0 <= c1), every texel index 3.
        bytes.extend_from_slice(&[0x00, 0x00, 0x1f, 0x00, 0xff, 0xff, 0xff, 0xff]);
        // Block 2: four-colour mode, white (c0) then black (c1), index 0.
        bytes.extend_from_slice(&[0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let image = decode(&bytes).unwrap();
        assert_eq!(image.dimensions(), (5, 3));
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(image.get_pixel(4, 2).0, [255, 255, 255, 255]);
    }

    #[test]
    fn reads_dxt3_and_dxt5_alpha() {
        let mut dxt3 = header(4, 4, 0x4, *b"DXT3", [0; 5]);
        // Alpha nibbles 0x0 then 0xf, repeating; white colour, index 0.
        dxt3.extend_from_slice(&[0xf0; 8]);
        dxt3.extend_from_slice(&[0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let image = decode(&dxt3).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255, 0]);
        assert_eq!(image.get_pixel(1, 0).0, [255, 255, 255, 255]);

        let mut dxt5 = header(4, 4, 0x4, *b"DXT5", [0; 5]);
        // a0 = 200, a1 = 100; texel 0 index 1 (a1), texel 1 index 2.
        dxt5.extend_from_slice(&[200, 100, 0b0001_0001, 0, 0, 0, 0, 0]);
        dxt5.extend_from_slice(&[0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let image = decode(&dxt5).unwrap();
        assert_eq!(image.get_pixel(0, 0).0[3], 100);
        assert_eq!(image.get_pixel(1, 0).0[3], 185);
    }

    #[test]
    fn rejects_truncated_and_impossible_files() {
        let mut short = header(4, 4, 0x4, *b"DXT1", [0; 5]);
        short.extend_from_slice(&[0; 4]);
        assert!(decode(&short).is_err());
        assert!(decode(&header(0, 4, 0x4, *b"DXT1", [0; 5])).is_err());
        assert!(decode(&header(4, 4, 0x4, *b"ATI2", [0; 5])).is_err());
        assert!(decode(b"BM").is_err());
    }
}

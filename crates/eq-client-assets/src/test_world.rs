//! Builds S3D archives and the WLD files inside them byte by byte for tests,
//! laid out as the formats lay them out, so no test needs game files.

use std::io::Cursor;
use std::path::Path;

use libeq::pfs::PfsWriter;

/// The header version of old-format WLD files.
pub(crate) const OLD_FORMAT: u32 = 0x0001_5500;
/// The header version of new-format WLD files, whose mesh texture coordinates
/// are 32-bit floats.
pub(crate) const NEW_FORMAT: u32 = 0x1000_C800;

/// The key that WLD string hashes are encoded with, by XOR.
const STRING_KEY: [u8; 8] = [0x95, 0x3a, 0xc5, 0x2a, 0x95, 0x7a, 0x95, 0x6a];

pub(crate) struct WorldFile {
    version: u32,
    strings: Vec<u8>,
    /// How many strings the hash holds, the empty first one included.
    string_count: usize,
    fragments: Vec<(u32, Vec<u8>)>,
}

impl WorldFile {
    pub(crate) fn new(version: u32) -> Self {
        Self {
            version,
            strings: vec![0],
            string_count: 1,
            fragments: Vec::new(),
        }
    }

    /// Adds a name to the string hash and returns its (negative) reference.
    pub(crate) fn name(&mut self, name: &str) -> i32 {
        let reference = -i32::try_from(self.strings.len()).unwrap();
        self.strings.extend(name.as_bytes());
        self.strings.push(0);
        self.string_count += 1;
        reference
    }

    /// Adds a fragment, padded to 4 bytes, and returns its 1-based index.
    pub(crate) fn fragment(&mut self, fragment_type: u32, mut body: Vec<u8>) -> i32 {
        body.resize(body.len().div_ceil(4) * 4, 0);
        self.fragments.push((fragment_type, body));
        i32::try_from(self.fragments.len()).unwrap()
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut strings: Vec<u8> = self
            .strings
            .iter()
            .zip(STRING_KEY.iter().cycle())
            .map(|(byte, key)| byte ^ key)
            .collect();
        strings.resize(strings.len().div_ceil(4) * 4, 0);
        let count = |n: usize| u32::try_from(n).unwrap();
        let largest = self.fragments.iter().map(|f| f.1.len()).max().unwrap_or(0);
        let header = [
            0x5450_3D02,
            self.version,
            count(self.fragments.len()),
            0,
            count(largest),
            count(strings.len()),
            count(self.string_count),
        ];
        let mut out: Vec<u8> = header.iter().flat_map(|word| word.to_le_bytes()).collect();
        out.extend(strings);
        for (fragment_type, body) in &self.fragments {
            out.extend(count(body.len()).to_le_bytes());
            out.extend(fragment_type.to_le_bytes());
            out.extend(body);
        }
        out.extend([0xff; 4]);
        out
    }
}

fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// A 0x30 material with no texture and the given render method.
pub(crate) fn material(name: i32, render_method: u32) -> Vec<u8> {
    let mut body = words(&[name.cast_unsigned(), 0, render_method, 0]);
    body.extend(1.0_f32.to_le_bytes());
    body.extend(1.0_f32.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body
}

/// A 0x31 palette of one material.
pub(crate) fn palette(name: i32, material: i32) -> Vec<u8> {
    words(&[name.cast_unsigned(), 0, 1, material.cast_unsigned()])
}

/// Texture coordinates as one of the two WLD formats stores them.
pub(crate) enum Coordinates {
    Old([(i16, i16); 3]),
    New([(f32, f32); 3]),
}

/// A 0x36 mesh of one triangle whose one face group claims `grouped_faces`
/// faces of material 0 in `palette`.
pub(crate) fn triangle(
    name: i32,
    palette: i32,
    coordinates: &Coordinates,
    grouped_faces: u16,
) -> Vec<u8> {
    let mut body = words(&[name.cast_unsigned(), 0x0001_8003, palette.cast_unsigned()]);
    // Animation, two unknown references, center, three unknowns, radius,
    // minimum and maximum.
    body.extend([0_u8; 12 + 12 + 12 + 4 + 24]);
    // Positions, coordinates, normals, colors, faces, skin groups, face
    // groups, vertex groups, mesh operations and scale.
    for count in [3_u16, 3, 3, 0, 1, 0, 1, 1, 0, 0] {
        body.extend(count.to_le_bytes());
    }
    for axis in [0_i16, 0, 0, 1, 0, 0, 0, 1, 0] {
        body.extend(axis.to_le_bytes());
    }
    match coordinates {
        Coordinates::Old(pairs) => {
            for (u, v) in pairs {
                body.extend(u.to_le_bytes());
                body.extend(v.to_le_bytes());
            }
        }
        Coordinates::New(pairs) => {
            for (u, v) in pairs {
                body.extend(u.to_le_bytes());
                body.extend(v.to_le_bytes());
            }
        }
    }
    body.extend([0_u8, 0, 127].repeat(3));
    // The face (flags, then its vertices), its face group and vertex group.
    for word in [0_u16, 0, 1, 2, grouped_faces, 0, 3, 0] {
        body.extend(word.to_le_bytes());
    }
    body
}

/// A 0x37 vertex animation of one vertex over `frames` frames, with the
/// alignment padding the fragment's length calls for.
pub(crate) fn vertex_animation(name: i32, frames: u16) -> Vec<u8> {
    let mut body = words(&[name.cast_unsigned(), 0]);
    for word in [1_u16, frames, 100, 0, 9] {
        body.extend(word.to_le_bytes());
    }
    for frame in 0..frames.cast_signed() {
        for axis in [frame, 0, 0] {
            body.extend(axis.to_le_bytes());
        }
    }
    body
}

/// Writes an S3D archive holding `files` into `directory`.
pub(crate) fn write_archive(directory: &Path, name: &str, files: &[(&str, Vec<u8>)]) {
    let mut writer = PfsWriter::create(Cursor::new(Vec::new())).unwrap();
    for (file, bytes) in files {
        writer.insert(*file, bytes.as_slice()).unwrap();
    }
    std::fs::write(directory.join(name), writer.finish().unwrap().into_inner()).unwrap();
}

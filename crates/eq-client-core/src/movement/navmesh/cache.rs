//! Built meshes kept on disk, keyed by what they were built from, so a zone
//! entered again reads its mesh instead of building it.
//!
//! A file is `navmesh/<zone>-<key>.bin` under the settings directory. The
//! key hashes the input triangles, static collision and liquid data, build settings, the limits
//! the ledges' drops are found with, the rerecast version and this format's
//! version, so a changed install or setting gives a new key. The header repeats the key, the settings and the triangle
//! count, and a read checks them, so a hash collision or a damaged file is
//! rebuilt rather than trusted.

use super::drops::{DROP_SPACING, THINNING};
use super::{BuildReport, DropLink, Link, NavMesh, Polygon, Settings};
use crate::movement::CollisionWorld;
use crate::movement::path::{CELL, MAX_DROP};
use glam::Vec3;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The rerecast version the mesh is built with, as pinned in Cargo.toml.
const RERECAST: &str = "0.4.0";
/// This format's version; raised whenever the bytes or the build change.
const FORMAT: u32 = 8;
const MAGIC: &[u8; 4] = b"EQNM";

/// Where a mesh came from.
#[derive(Debug)]
pub enum Source {
    /// Read from the cache.
    Cache,
    /// Built, as the report says.
    Built(BuildReport),
}

/// A zone's mesh, read or built.
#[derive(Debug)]
pub struct Loaded {
    /// The mesh.
    pub mesh: NavMesh,
    /// Where it came from.
    pub source: Source,
    /// The cache file's bytes, or why the mesh isn't kept.
    pub cached: io::Result<usize>,
}

/// The mesh for `zone` over `triangles`: read from the cache under
/// `directory` when it holds one, else built on `threads` threads, its
/// ledges' drops tried in `world` (the zone's own, when there is one), and
/// kept there. None when `cancel` is set before a build finishes; a cancelled
/// build keeps nothing.
#[must_use]
pub fn load_or_build(
    directory: Option<&Path>,
    zone: &str,
    triangles: &[[[f32; 3]; 3]],
    world: Option<&CollisionWorld>,
    settings: &Settings,
    threads: usize,
    cancel: &AtomicBool,
) -> Option<Loaded> {
    load_or_build_until(
        directory,
        zone,
        triangles,
        world,
        settings,
        threads,
        &|| cancel.load(Ordering::Relaxed),
    )
}

pub(super) fn load_or_build_until(
    directory: Option<&Path>,
    zone: &str,
    triangles: &[[[f32; 3]; 3]],
    world: Option<&CollisionWorld>,
    settings: &Settings,
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Loaded> {
    if let (Some(directory), Some(key)) = (directory, key(triangles, world, settings))
        && let Some(path) = path(directory, zone, key)
        && let Ok(bytes) = fs::read(path)
        && let Some(mesh) = decode(&bytes, &header(key, triangles.len(), settings))
    {
        return Some(Loaded {
            mesh,
            source: Source::Cache,
            cached: Ok(bytes.len()),
        });
    }
    let (mesh, report) = NavMesh::build_until(triangles, world, settings, threads, cancelled)?;
    let cached = directory.map_or_else(
        || Err(io::Error::other("no settings directory")),
        |directory| store(directory, zone, triangles, world, settings, &mesh),
    );
    Some(Loaded {
        mesh,
        source: Source::Built(report),
        cached,
    })
}

/// Complete build identity: geometry, static collision, liquids and settings.
/// Dynamic obstacles are excluded, as drop construction removes them. Returns
/// None when a liquid provider cannot fingerprint its data; such builds are
/// usable in memory but must not read or write the persistent cache.
#[must_use]
pub fn key(
    triangles: &[[[f32; 3]; 3]],
    world: Option<&CollisionWorld>,
    settings: &Settings,
) -> Option<u64> {
    let mut hash = Fnv::default();
    hash.write(RERECAST.as_bytes());
    hash.write(&FORMAT.to_le_bytes());
    for value in settings_values(settings).into_iter().chain(drop_values()) {
        hash.write(&value.to_le_bytes());
    }
    hash.write(&(triangles.len() as u64).to_le_bytes());
    for value in triangles.iter().flatten().flatten() {
        hash.write(&value.to_le_bytes());
    }
    hash.write(&[u8::from(world.is_some())]);
    if let Some(world) = world {
        // Drops use this geometry, which need not equal the rasterized triangles.
        let mesh = &world.terrain.mesh;
        hash.write(&(mesh.vertices().len() as u64).to_le_bytes());
        for point in mesh.vertices() {
            for value in [point.x, point.y, point.z] {
                hash.write(&value.to_le_bytes());
            }
        }
        hash.write(&(mesh.indices().len() as u64).to_le_bytes());
        for index in mesh.indices().iter().flatten() {
            hash.write(&index.to_le_bytes());
        }
        hash.write(&[u8::from(world.liquids.is_some())]);
        if let Some(liquids) = &world.liquids {
            hash.write(&liquids.cache_fingerprint()?.to_le_bytes());
        }
    }
    Some(hash.0)
}

/// Where the mesh for `zone` with `key` is kept under `directory`; None for
/// a zone name that isn't a plain file name.
#[must_use]
pub fn path(directory: &Path, zone: &str, key: u64) -> Option<PathBuf> {
    let plain = !zone.is_empty() && zone.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    plain.then(|| {
        directory
            .join("navmesh")
            .join(format!("{zone}-{key:016x}.bin"))
    })
}

/// The cached mesh for `zone` over `triangles`, if one is kept and its
/// header matches.
#[must_use]
pub fn load(
    directory: &Path,
    zone: &str,
    triangles: &[[[f32; 3]; 3]],
    world: Option<&CollisionWorld>,
    settings: &Settings,
) -> Option<NavMesh> {
    let key = key(triangles, world, settings)?;
    let bytes = fs::read(path(directory, zone, key)?).ok()?;
    decode(&bytes, &header(key, triangles.len(), settings))
}

/// Whether a mesh for `zone` built with `settings` is kept under `directory`,
/// judged by the header alone: its format and settings, not its key, so the
/// count of built zones never loads a zone. A file whose triangles have since
/// changed counts until a load rebuilds it.
#[must_use]
pub fn present(directory: &Path, zone: &str, settings: &Settings) -> bool {
    // A zone that isn't a plain file name has no file.
    if path(directory, zone, 0).is_none() {
        return false;
    }
    let Ok(entries) = fs::read_dir(directory.join("navmesh")) else {
        return false;
    };
    let expected = header(0, 0, settings);
    // The key and the triangle count sit between the format and the
    // settings.
    let (format, settings_at) = (MAGIC.len() + 4, MAGIC.len() + 4 + 8 + 8);
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let ours = name
            .to_str()
            .and_then(|name| name.strip_suffix(".bin"))
            .and_then(|stem| stem.rsplit_once('-'))
            .is_some_and(|(owner, _)| owner == zone);
        ours && read_prefix(&entry.path(), expected.len()).is_some_and(|bytes| {
            bytes[..format] == expected[..format] && bytes[settings_at..] == expected[settings_at..]
        })
    })
}

/// The first `length` bytes of the file at `path`, when it has that many.
fn read_prefix(path: &Path, length: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = vec![0; length];
    fs::File::open(path).ok()?.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// Keeps `mesh` as the cached mesh for `zone` over `triangles`, removing the
/// zone's files under other keys; the bytes written.
///
/// # Errors
///
/// When the directory or the file can't be written, or `zone` isn't a plain
/// file name.
pub fn store(
    directory: &Path,
    zone: &str,
    triangles: &[[[f32; 3]; 3]],
    world: Option<&CollisionWorld>,
    settings: &Settings,
    mesh: &NavMesh,
) -> io::Result<usize> {
    let key = key(triangles, world, settings).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "liquid provider has no cache fingerprint",
        )
    })?;
    let path = path(directory, zone, key)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "zone name"))?;
    let folder = path.parent().unwrap_or(directory);
    fs::create_dir_all(folder)?;
    let mut bytes = header(key, triangles.len(), settings);
    encode(mesh, &mut bytes);
    // Written whole beside the file, then moved into place, so a crash never
    // leaves half a file under the key.
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, &bytes)?;
    // A file under the key is one a read just rejected; Windows renames
    // onto no existing file, so it goes first.
    match fs::remove_file(&path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => (),
    }
    fs::rename(&temporary, &path)?;
    let name = path.file_name().and_then(|name| name.to_str());
    for entry in fs::read_dir(folder)?.flatten() {
        let other = entry.file_name();
        let Some(other) = other.to_str() else {
            continue;
        };
        // The zone's files under other keys, and any temp file a crash left.
        let stale = Some(other) != name
            && other
                .strip_suffix(".bin")
                .or_else(|| other.strip_suffix(".tmp"))
                .and_then(|stem| stem.rsplit_once('-'))
                .is_some_and(|(owner, _)| owner == zone);
        if stale {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(bytes.len())
}

/// The fields of `settings` that change a build.
fn settings_values(settings: &Settings) -> [f32; 10] {
    [
        settings.cell_size,
        settings.cell_height,
        settings.tile,
        settings.radius,
        settings.height,
        settings.sizes[0],
        settings.sizes[1],
        settings.sizes[2],
        settings.climb,
        settings.floor_normal_y,
    ]
}

/// The limits the drops are found with.
const fn drop_values() -> [f32; 4] {
    [MAX_DROP, DROP_SPACING, THINNING, CELL]
}

fn header(key: u64, triangles: usize, settings: &Settings) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&(triangles as u64).to_le_bytes());
    for value in settings_values(settings) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn encode(mesh: &NavMesh, out: &mut Vec<u8>) {
    let count = |out: &mut Vec<u8>, n: usize| {
        #[allow(clippy::cast_possible_truncation)] // Polygons and corners are counted in u32.
        out.extend_from_slice(&(n as u32).to_le_bytes());
    };
    let point = |out: &mut Vec<u8>, v: Vec3| {
        for f in v.to_array() {
            out.extend_from_slice(&f.to_le_bytes());
        }
    };
    count(out, mesh.polygons.len());
    for polygon in &mesh.polygons {
        count(out, polygon.vertices.len());
        for &v in &polygon.vertices {
            point(out, v);
        }
        out.extend_from_slice(&u32::from(polygon.size).to_le_bytes());
        count(out, polygon.links.len());
        for link in &polygon.links {
            out.extend_from_slice(&link.to.to_le_bytes());
            point(out, link.a);
            point(out, link.b);
        }
    }
    count(out, mesh.drops.len());
    for link in &mesh.drops {
        out.extend_from_slice(&link.from.to_le_bytes());
        out.extend_from_slice(&link.to.to_le_bytes());
        point(out, link.top);
        point(out, link.landing);
    }
}

/// The mesh in `bytes` after `header`; None for a different header, a short
/// file, trailing bytes or a link to no polygon.
fn decode(bytes: &[u8], header: &[u8]) -> Option<NavMesh> {
    let mut reader = Reader(bytes.strip_prefix(header)?);
    let count = reader.u32()?;
    let mut polygons = Vec::new();
    for _ in 0..count {
        let vertices = (0..reader.u32()?)
            .map(|_| reader.point())
            .collect::<Option<_>>()?;
        let size = u8::try_from(reader.u32()?).ok().filter(|&size| size < 3)?;
        let links = (0..reader.u32()?)
            .map(|_| {
                let to = reader.u32()?;
                (to < count).then_some(())?;
                Some(Link {
                    to,
                    a: reader.point()?,
                    b: reader.point()?,
                })
            })
            .collect::<Option<_>>()?;
        polygons.push(Polygon {
            vertices,
            links,
            size,
        });
    }
    let drops = (0..reader.u32()?)
        .map(|_| {
            let (from, to) = (reader.u32()?, reader.u32()?);
            (from < count && to < count).then_some(())?;
            Some(DropLink {
                from,
                to,
                top: reader.point()?,
                landing: reader.point()?,
            })
        })
        .collect::<Option<_>>()?;
    reader
        .0
        .is_empty()
        .then(|| NavMesh::from_parts(polygons, drops))
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let (word, rest) = self.0.split_first_chunk::<4>()?;
        self.0 = rest;
        Some(u32::from_le_bytes(*word))
    }

    fn point(&mut self) -> Option<Vec3> {
        Some(Vec3::new(
            f32::from_bits(self.u32()?),
            f32::from_bits(self.u32()?),
            f32::from_bits(self.u32()?),
        ))
    }
}

/// 64-bit FNV-1a.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

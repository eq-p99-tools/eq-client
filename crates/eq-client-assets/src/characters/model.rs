//! Pose and skinning for the classic rigid-per-vertex character format.

use std::{collections::HashMap, fs::File, path::Path};

use glam::{Mat4, Quat, Vec3};
use libeq::{
    pfs::PfsReader,
    wld::parser::{DmSprite, DmSpriteDef2, FrameTransform, HierarchicalSpriteDef, Track, WldDoc},
};

use super::{invalid, resolve, validate_parents};
use crate::{LoadError, ZonePrimitive, ZoneTexture, load_mesh_primitives};

/// A renderer-independent character with original bone-local mesh data.
#[derive(Clone, Debug)]
pub struct CharacterAsset {
    /// Model identifier such as HUM.
    pub name: String,
    /// Material-grouped geometry, initially in its base pose.
    pub primitives: Vec<ZonePrimitive>,
    /// Locally decoded diffuse textures.
    pub textures: Vec<ZoneTexture>,
    parents: Vec<Option<usize>>,
    base_pose: Vec<LocalTransform>,
    clips: HashMap<String, Vec<Option<AnimationTrack>>>,
    skins: Vec<Skin>,
}

/// Interpolated vertex data for a single draw primitive.
#[derive(Clone, Debug)]
pub struct CharacterPose {
    /// Posed Y-up positions.
    pub positions: Vec<[f32; 3]>,
    /// Posed Y-up unit normals.
    pub normals: Vec<[f32; 3]>,
}

#[derive(Clone, Debug)]
struct Skin {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    bones: Vec<usize>,
}

#[derive(Clone, Debug)]
struct AnimationTrack {
    frames: Vec<LocalTransform>,
    interval: f32,
}

#[derive(Clone, Copy, Debug)]
struct LocalTransform {
    translation: Vec3,
    rotation: Quat,
    scale: f32,
}

impl LocalTransform {
    fn decode(frame: &FrameTransform) -> Self {
        let rotation = Quat::from_xyzw(
            f32::from(frame.rotate_x_numerator),
            f32::from(frame.rotate_y_numerator),
            f32::from(frame.rotate_z_numerator),
            f32::from(frame.rotate_denominator),
        );
        Self {
            translation: Vec3::new(
                f32::from(frame.shift_x_numerator),
                f32::from(frame.shift_y_numerator),
                f32::from(frame.shift_z_numerator),
            ) / 256.0,
            rotation: if rotation.length_squared() > f32::EPSILON {
                rotation.normalize()
            } else {
                Quat::IDENTITY
            },
            scale: if frame.shift_denominator == 0 {
                1.0
            } else {
                f32::from(frame.shift_denominator) / 256.0
            },
        }
    }

    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation,
            self.translation,
        )
    }
}

impl CharacterAsset {
    /// Height of the base-pose mesh in native EQ world units.
    pub fn height(&self) -> f32 {
        let (min, max) = self
            .primitives
            .iter()
            .flat_map(|p| &p.positions)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), p| {
                (min.min(p[1]), max.max(p[1]))
            });
        (max - min).max(1.0)
    }

    /// Available animation codes as stored in this archive, in deterministic order.
    pub fn animation_codes(&self) -> Vec<&str> {
        let mut codes: Vec<_> = self.clips.keys().map(String::as_str).collect();
        codes.sort_unstable();
        codes
    }

    /// Samples a clip, or the base pose when the clip is unavailable.
    /// No root motion is applied to the entity; movement remains owned by simulation.
    pub fn pose(&self, animation: &str, seconds: f32) -> Vec<CharacterPose> {
        self.sample_pose(animation, seconds, true)
    }

    /// Plays a transition once and holds its final frame instead of looping it.
    pub fn pose_held(&self, animation: &str, seconds: f32) -> Vec<CharacterPose> {
        self.sample_pose(animation, seconds, false)
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn sample_pose(&self, animation: &str, seconds: f32, looping: bool) -> Vec<CharacterPose> {
        let clip = self.clips.get(animation);
        let local: Vec<_> = self
            .base_pose
            .iter()
            .enumerate()
            .map(|(index, base)| {
                let Some(track) = clip.and_then(|clip| clip[index].as_ref()) else {
                    return base.matrix();
                };
                let time = if seconds.is_finite() {
                    seconds.max(0.0)
                } else {
                    0.0
                };
                let frame = if looping {
                    (time / track.interval) % track.frames.len() as f32
                } else {
                    (time / track.interval).min((track.frames.len() - 1) as f32)
                };
                let a = track.frames[frame as usize];
                let next = if looping {
                    (frame as usize + 1) % track.frames.len()
                } else {
                    (frame as usize + 1).min(track.frames.len() - 1)
                };
                let b = track.frames[next];
                let fraction = frame.fract();
                LocalTransform {
                    translation: if self.parents[index].is_none() {
                        base.translation
                    } else {
                        a.translation.lerp(b.translation, fraction)
                    },
                    rotation: a.rotation.slerp(b.rotation, fraction),
                    scale: a.scale + (b.scale - a.scale) * fraction,
                }
                .matrix()
            })
            .collect();
        let world = compose_hierarchy(&local, &self.parents);
        self.skins
            .iter()
            .map(|skin| {
                let mut positions = Vec::with_capacity(skin.positions.len());
                let mut normals = Vec::with_capacity(skin.normals.len());
                for ((position, normal), bone) in
                    skin.positions.iter().zip(&skin.normals).zip(&skin.bones)
                {
                    // Skinning occurs in native Z-up WLD coordinates before swapping axes.
                    let [x, y, z] = *position;
                    let point = world[*bone].transform_point3(Vec3::new(x, z, y));
                    let [x, y, z] = *normal;
                    let normal = world[*bone]
                        .transform_vector3(Vec3::new(x, z, y))
                        .normalize_or_zero();
                    positions.push([point.x, point.z, point.y]);
                    normals.push([normal.x, normal.z, normal.y]);
                }
                CharacterPose { positions, normals }
            })
            .collect()
    }
}

/// Composes a validated hierarchy without assuming parents precede children.
fn compose_hierarchy(local: &[Mat4], parents: &[Option<usize>]) -> Vec<Mat4> {
    local
        .iter()
        .enumerate()
        .map(|(index, transform)| {
            let mut result = *transform;
            let mut parent = parents[index];
            while let Some(index) = parent {
                result = local[index] * result;
                parent = parents[index];
            }
            result
        })
        .collect()
}

/// Loads one classic model from an explicitly selected local character archive.
///
/// # Errors
/// Rejects missing models, unsupported legacy skins/tracks, invalid hierarchies,
/// or skin groups that do not cover the mesh. It never fabricates missing bones.
#[allow(clippy::too_many_lines)]
pub fn load_character(path: &Path, model: &str) -> Result<CharacterAsset, LoadError> {
    let file = File::open(path).map_err(|source| LoadError::OpenArchive {
        path: path.to_owned(),
        source,
    })?;
    let mut archive = PfsReader::open(file)?;
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("archive name"))?;
    let world_name = format!("{stem}.wld");
    let bytes = archive
        .get(&world_name)?
        .ok_or(LoadError::MissingWorld(world_name))?;
    let doc = WldDoc::parse(&bytes)
        .map_err(|errors| invalid(&format!("WLD ({} parse errors)", errors.len())))?;
    let name = model.to_ascii_uppercase();
    let identifier = format!("{name}_HS_DEF");
    let skeleton = doc
        .fragment_iter::<HierarchicalSpriteDef>()
        .find(|s| doc.get_string(s.name_reference) == Some(identifier.as_str()))
        .ok_or_else(|| invalid(&format!("unknown model {name}")))?;
    let mut parents = vec![None; skeleton.dags.len()];
    let mut base_pose = Vec::new();
    let mut bone_names = Vec::new();
    for (index, bone) in skeleton.dags.iter().enumerate() {
        let track: &Track = resolve(&doc, bone.track_reference)?;
        let definition = doc
            .get(&track.reference)
            .ok_or_else(|| invalid("base track"))?;
        let frame = definition
            .frame_transforms
            .as_ref()
            .and_then(|frames| frames.first())
            .ok_or_else(|| invalid("unsupported base transform"))?;
        base_pose.push(LocalTransform::decode(frame));
        bone_names.push(
            doc.get_string(track.name_reference)
                .ok_or_else(|| invalid("track name"))?,
        );
        for child in &bone.sub_dags {
            let parent = parents
                .get_mut(*child as usize)
                .ok_or_else(|| invalid("child bone"))?;
            if parent.replace(index).is_some() {
                return Err(invalid("duplicate bone parent"));
            }
        }
    }
    validate_parents(&parents)?;
    let mut clips: HashMap<String, Vec<Option<AnimationTrack>>> = HashMap::new();
    let source = animation_source(&name);
    for track in doc.fragment_iter::<Track>() {
        let Some(track_name) = doc.get_string(track.name_reference) else {
            continue;
        };
        let Some((code, suffix)) = track_name.split_at_checked(3) else {
            continue;
        };
        let inherited = source.is_some_and(|source| suffix.starts_with(source));
        let Some(index) = bone_names.iter().position(|bone| {
            *bone == suffix
                || source.is_some_and(|source| {
                    bone.strip_prefix(name.as_str())
                        .zip(suffix.strip_prefix(source))
                        .is_some_and(|(own, donor)| own == donor)
                })
        }) else {
            continue;
        };
        let Some(definition) = doc.get(&track.reference) else {
            continue;
        };
        let Some(frames) = &definition.frame_transforms else {
            continue;
        };
        if frames.is_empty() {
            continue;
        }
        let interval = u16::try_from(track.sleep.unwrap_or(100))
            .unwrap_or(100)
            .max(1);
        let tracks = clips
            .entry(code.to_owned())
            .or_insert_with(|| vec![None; parents.len()]);
        // Model-specific tracks override inherited ones regardless of fragment order.
        if inherited && tracks[index].is_some() {
            continue;
        }
        tracks[index] = Some(AnimationTrack {
            frames: frames.iter().map(LocalTransform::decode).collect(),
            interval: f32::from(interval) / 1000.0,
        });
    }
    let world = libeq::wld::load(&bytes).map_err(|error| invalid(&error.to_string()))?;
    let mut textures = Vec::new();
    let mut texture_indices = HashMap::new();
    let mut primitives = Vec::new();
    let mut skins = Vec::new();
    for reference in skeleton.dm_sprites.as_deref().unwrap_or_default() {
        let sprite: &DmSprite = resolve(&doc, *reference)?;
        let raw: &DmSpriteDef2 = doc
            .get(&sprite.reference)
            .ok_or_else(|| invalid("legacy skin is not yet renderable"))?;
        let mesh_name = doc
            .get_string(raw.name_reference)
            .ok_or_else(|| invalid("mesh name"))?;
        let mesh = world
            .meshes()
            .find(|mesh| mesh.name() == Some(mesh_name))
            .ok_or_else(|| invalid("mesh"))?;
        let bones: Vec<_> = raw
            .skin_assignment_groups
            .iter()
            .flat_map(|(count, bone)| std::iter::repeat_n(usize::from(*bone), usize::from(*count)))
            .collect();
        if bones.len() != raw.positions.len() || bones.iter().any(|bone| *bone >= parents.len()) {
            return Err(invalid("skin bone assignments"));
        }
        for primitive in
            load_mesh_primitives(&mesh, &mut archive, &mut textures, &mut texture_indices)?
        {
            skins.push(Skin {
                positions: primitive.positions.clone(),
                normals: primitive.normals.clone(),
                bones: bones.clone(),
            });
            primitives.push(primitive);
        }
    }
    if primitives.is_empty() {
        return Err(invalid("model has no renderable skins"));
    }
    let mut asset = CharacterAsset {
        name,
        primitives,
        textures,
        parents,
        base_pose,
        clips,
        skins,
    };
    let pose = asset.pose("", 0.0);
    for (primitive, pose) in asset.primitives.iter_mut().zip(pose) {
        primitive.positions = pose.positions;
        primitive.normals = pose.normals;
    }
    Ok(asset)
}

/// Classic playable donors: <https://github.com/LanternEQ/LanternExtractor/blob/main/LanternExtractor/ClientData/animationsources.txt>
fn animation_source(model: &str) -> Option<&'static str> {
    match model {
        "HUM" | "BAM" | "ERM" | "HIM" | "DAM" | "HAM" => Some("ELM"),
        "HUF" | "BAF" | "ERF" | "HIF" | "DAF" | "HAF" => Some("ELF"),
        "TRM" | "TRF" | "OGM" => Some("OGF"),
        "HOM" | "GNM" => Some("DWM"),
        "HOF" | "GNF" => Some("DWF"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_pose_keeps_final_frame_while_locomotion_still_loops() {
        let base = LocalTransform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
        };
        let final_frame = LocalTransform {
            rotation: Quat::from_rotation_z(std::f32::consts::PI),
            ..base
        };
        let asset = CharacterAsset {
            name: "synthetic".into(),
            primitives: Vec::new(),
            textures: Vec::new(),
            parents: vec![None],
            base_pose: vec![base],
            clips: HashMap::from([(
                "P02".into(),
                vec![Some(AnimationTrack {
                    frames: vec![base, final_frame],
                    interval: 1.0,
                })],
            )]),
            skins: vec![Skin {
                positions: vec![[1.0, 0.0, 0.0]],
                normals: vec![[1.0, 0.0, 0.0]],
                bones: vec![0],
            }],
        };
        for time in [1.0, 1.5, 2.0, 10.0] {
            assert!((asset.pose_held("P02", time)[0].positions[0][0] + 1.0).abs() < 0.0001);
        }
        assert!((asset.pose("P02", 2.0)[0].positions[0][0] - 1.0).abs() < 0.0001);
        for time in [-1.0, f32::NAN] {
            assert!((asset.pose_held("P02", time)[0].positions[0][0] - 1.0).abs() < 0.0001);
        }
        assert!((asset.pose_held("missing", 10.0)[0].positions[0][0] - 1.0).abs() < 0.0001);
    }

    #[test]
    fn fixed_point_translation_and_quaternion_have_independent_scales() {
        let frame = FrameTransform {
            rotate_denominator: 100,
            rotate_x_numerator: 0,
            rotate_y_numerator: 0,
            rotate_z_numerator: 100,
            shift_x_numerator: 512,
            shift_y_numerator: 0,
            shift_z_numerator: 0,
            shift_denominator: 128,
        };
        let point = LocalTransform::decode(&frame)
            .matrix()
            .transform_point3(Vec3::X);
        assert!((point - Vec3::new(2.0, 0.5, 0.0)).length() < 0.0001);
    }

    #[test]
    fn child_inherits_parent_rotation_and_translation() {
        let local = [
            Mat4::from_translation(Vec3::X),
            Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2),
        ];
        let world = compose_hierarchy(&local, &[Some(1), None]);
        assert!((world[0].transform_point3(Vec3::ZERO) - Vec3::Y).length() < 0.0001);
    }
}

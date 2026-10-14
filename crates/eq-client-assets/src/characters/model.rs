//! Pose and skinning for the classic rigid-per-vertex character format.

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};

use glam::{Mat4, Quat, Vec3};
use libeq::{
    pfs::PfsReader,
    wld::parser::{DmSprite, DmSpriteDef2, FrameTransform, HierarchicalSpriteDef, Track, WldDoc},
};

use super::{invalid, resolve, validate_parents};
use crate::{LoadError, MaterialMode, ZonePrimitive, ZoneTexture, load_texture, stage_mesh};

/// A renderer-independent character with original bone-local mesh data.
#[derive(Clone, Debug)]
pub struct CharacterAsset {
    /// Model identifier such as HUM.
    pub name: String,
    /// Material-grouped geometry, initially in its base pose.
    pub primitives: Vec<ZonePrimitive>,
    /// Locally decoded diffuse textures.
    pub textures: Vec<ZoneTexture>,
    /// Per primitive, the material it draws with in its base look, in upper
    /// case, such as `HUMCH0001_MDF`.
    pub materials: Vec<Option<String>>,
    /// Per primitive, the piece of the model it belongs to.
    pub pieces: Vec<Piece>,
    /// The archive the model came from, for the textures of other looks.
    source: PathBuf,
    /// Every material in that archive by name: its texture file and blending.
    catalog: HashMap<String, (Option<String>, MaterialMode)>,
    parents: Vec<Option<usize>>,
    base_pose: Vec<LocalTransform>,
    clips: HashMap<String, Vec<Option<AnimationTrack>>>,
    skins: Vec<Skin>,
    /// Bones held items attach to, in [`Attachment`] order.
    attachments: [Option<usize>; 3],
}

/// Which interchangeable piece of a classic model a mesh is. Models swap
/// whole bodies (`HUM01`, the robe body, beside `HUM`) and heads (`HUMHE01` to
/// `HUMHE03`, the helmed heads, beside the bare `HUMHE00`); the number in the
/// mesh's name says which.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Piece {
    /// A body; 0 is the base body.
    Body(u8),
    /// A head; 0 is the bare head.
    Head(u8),
    /// Any other mesh, always drawn.
    Fixed,
}

impl Piece {
    /// Reads a mesh name such as `HUMHE02_DMSPRITEDEF` for the model `HUM`.
    fn of(model: &str, mesh: &str) -> Self {
        let number = |digits: &str| {
            (digits.len() == 2 && digits.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| digits.parse().ok())
                .flatten()
        };
        let Some(rest) = mesh
            .strip_suffix("_DMSPRITEDEF")
            .and_then(|stem| stem.strip_prefix(model))
        else {
            return Self::Fixed;
        };
        if rest.is_empty() {
            Self::Body(0)
        } else if let Some(head) = rest.strip_prefix("HE").and_then(number) {
            Self::Head(head)
        } else {
            number(rest).map_or(Self::Fixed, Self::Body)
        }
    }

    /// Whether the piece is drawn in the model's base look.
    #[must_use]
    pub const fn base(self) -> bool {
        matches!(self, Self::Body(0) | Self::Head(0) | Self::Fixed)
    }
}

/// Where a classic skeleton holds items.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attachment {
    /// The right hand (`R_POINT`), for the primary item.
    RightHand,
    /// The left hand (`L_POINT`), for an off-hand item that is not a shield.
    LeftHand,
    /// The forearm's shield point (`SHIELD_POINT`).
    Shield,
}

impl Attachment {
    /// Index into [`CharacterAsset::pose_with_attachments`]'s attachments.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The bone name this point uses, after the model prefix.
    const fn bone(self) -> &'static str {
        match self {
            Self::RightHand => "R_POINT",
            Self::LeftHand => "L_POINT",
            Self::Shield => "SHIELD_POINT",
        }
    }
}

/// The bone for an attachment point: a track or DAG name such as
/// `HUMR_POINT_TRACK` for the model `HUM`.
fn attachment_bone(bones: &[&str], model: &str, point: Attachment) -> Option<usize> {
    bones.iter().position(|bone| {
        let bone = bone.to_ascii_uppercase();
        let stem = bone
            .strip_suffix("_TRACK")
            .or_else(|| bone.strip_suffix("_DAG"))
            .unwrap_or(&bone);
        stem.strip_prefix(model) == Some(point.bone())
    })
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

impl Skin {
    /// Poses this primitive with these bone transforms.
    fn pose(&self, world: &[Mat4]) -> CharacterPose {
        let mut positions = Vec::with_capacity(self.positions.len());
        let mut normals = Vec::with_capacity(self.normals.len());
        for ((position, normal), bone) in self.positions.iter().zip(&self.normals).zip(&self.bones)
        {
            // Skinning occurs in native Z-up WLD coordinates.
            let point = world[*bone].transform_point3(crate::wld(*position));
            let normal = world[*bone]
                .transform_vector3(crate::wld(*normal))
                .normalize_or_zero();
            positions.push(eq_client_axes::from_wld(point).to_array());
            normals.push(eq_client_axes::from_wld(normal).to_array());
        }
        CharacterPose { positions, normals }
    }
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
        self.span().map_or(1.0, |(min, max)| (max - min).max(1.0))
    }

    /// The lowest and highest points of the base-pose mesh, such as the
    /// soles and the top of the head, in native EQ world units; None for a
    /// model without base pieces.
    #[must_use]
    pub fn span(&self) -> Option<(f32, f32)> {
        let (min, max) = self
            .primitives
            .iter()
            .zip(&self.pieces)
            .filter(|(_, piece)| piece.base())
            .flat_map(|(p, _)| &p.positions)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), p| {
                (min.min(p[1]), max.max(p[1]))
            });
        (min <= max).then_some((min, max))
    }

    /// The archive the model came from.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Whether the model's archive has this material, such as `HUMCH0201_MDF`.
    #[must_use]
    pub fn has_material(&self, material: &str) -> bool {
        self.catalog.contains_key(&material.to_ascii_uppercase())
    }

    /// The texture and blending of another material in the model's archive,
    /// such as `HUMCH0201_MDF` for a chain tunic; None when the archive has no
    /// such material or it draws without a texture.
    ///
    /// # Errors
    /// Returns [`LoadError`] when the archive or the texture cannot be read.
    pub fn material_texture(
        &self,
        material: &str,
    ) -> Result<Option<(ZoneTexture, MaterialMode)>, LoadError> {
        let Some((Some(texture), mode)) = self.catalog.get(&material.to_ascii_uppercase()) else {
            return Ok(None);
        };
        let file = File::open(&self.source).map_err(|source| LoadError::OpenArchive {
            path: self.source.clone(),
            source,
        })?;
        let mut archive = PfsReader::open(file)?;
        let mut textures = Vec::new();
        let loaded = load_texture(&mut archive, texture, &mut textures, &mut HashMap::new())?;
        Ok(loaded
            .and_then(|_| textures.pop())
            .map(|texture| (texture, *mode)))
    }

    /// Available animation codes as stored in this archive, in deterministic order.
    pub fn animation_codes(&self) -> Vec<&str> {
        let mut codes: Vec<_> = self.clips.keys().map(String::as_str).collect();
        codes.sort_unstable();
        codes
    }

    /// How long a clip takes to play once, such as `C05`: its longest
    /// track's frames times their interval. None where the model lacks it.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a track has far fewer frames than an f32 counts exactly"
    )]
    pub fn clip_seconds(&self, animation: &str) -> Option<f32> {
        self.clips
            .get(animation)?
            .iter()
            .flatten()
            .map(|track| track.frames.len() as f32 * track.interval)
            .reduce(f32::max)
    }

    /// Samples a clip, or the base pose when the clip is unavailable.
    /// No root motion is applied to the entity; movement remains owned by simulation.
    pub fn pose(&self, animation: &str, seconds: f32) -> Vec<CharacterPose> {
        self.skin(&self.bones(animation, seconds, true))
    }

    /// Plays a transition once and holds its final frame instead of looping it.
    pub fn pose_held(&self, animation: &str, seconds: f32) -> Vec<CharacterPose> {
        self.skin(&self.bones(animation, seconds, false))
    }

    /// Samples a clip, looping or held as [`Self::pose`] and [`Self::pose_held`]
    /// do, for the primitives `shown` marks (the others get None), and also
    /// returns where each held item attaches, in the renderer's frame like
    /// the pose, for the attachment points this skeleton has.
    pub fn pose_with_attachments(
        &self,
        animation: &str,
        seconds: f32,
        looping: bool,
        shown: &[bool],
    ) -> (Vec<Option<CharacterPose>>, [Option<Mat4>; 3]) {
        let bones = self.bones(animation, seconds, looping);
        let attachments = self.attachments.map(|bone| {
            bone.and_then(|bone| bones.get(bone))
                .map(|matrix| eq_client_axes::wld_transform(*matrix))
        });
        let poses = self
            .skins
            .iter()
            .enumerate()
            .map(|(index, skin)| {
                shown
                    .get(index)
                    .copied()
                    .unwrap_or(false)
                    .then(|| skin.pose(&bones))
            })
            .collect();
        (poses, attachments)
    }

    /// Each bone's transform in native Z-up WLD space for a clip.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn bones(&self, animation: &str, seconds: f32, looping: bool) -> Vec<Mat4> {
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
        compose_hierarchy(&local, &self.parents)
    }

    /// Skins every primitive with these bone transforms.
    fn skin(&self, world: &[Mat4]) -> Vec<CharacterPose> {
        self.skins.iter().map(|skin| skin.pose(world)).collect()
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
    let catalog = world
        .materials()
        .filter_map(|material| {
            let name = material.name()?.to_ascii_uppercase();
            let mode = crate::material_mode(*material.render_method())?;
            let texture = material
                .base_color_texture()
                .and_then(|texture| texture.source());
            Some((name, (texture, mode)))
        })
        .collect();
    let mut textures = Vec::new();
    let mut texture_indices = HashMap::new();
    let mut primitives = Vec::new();
    let mut materials = Vec::new();
    let mut pieces = Vec::new();
    let mut skins = Vec::new();
    // The skeleton's own meshes, then the bodies and heads that can replace
    // them, which share its bones.
    let mut sprites = Vec::new();
    for reference in skeleton.dm_sprites.as_deref().unwrap_or_default() {
        let sprite: &DmSprite = resolve(&doc, *reference)?;
        let raw: &DmSpriteDef2 = doc
            .get(&sprite.reference)
            .ok_or_else(|| invalid("legacy skin is not yet renderable"))?;
        sprites.push(raw);
    }
    let attached: Vec<_> = sprites
        .iter()
        .filter_map(|raw| doc.get_string(raw.name_reference))
        .collect();
    let others: Vec<&DmSpriteDef2> = doc
        .fragment_iter::<DmSpriteDef2>()
        .filter(|raw| {
            doc.get_string(raw.name_reference)
                .is_some_and(|mesh| !attached.contains(&mesh) && !Piece::of(&name, mesh).base())
        })
        .collect();
    sprites.extend(others);
    for raw in sprites {
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
        let piece = Piece::of(&name, mesh_name);
        for staged in stage_mesh(&mesh) {
            let material = staged.material.clone();
            // Skinning starts from libeq's axes; poses convert to the renderer's.
            skins.push(Skin {
                positions: staged.positions.clone(),
                normals: staged.normals.clone(),
                bones: bones.clone(),
            });
            primitives.push(staged.realize(&mut archive, &mut textures, &mut texture_indices)?);
            materials.push(material);
            pieces.push(piece);
        }
    }
    if primitives.is_empty() {
        return Err(invalid("model has no renderable skins"));
    }
    let attachments = [
        Attachment::RightHand,
        Attachment::LeftHand,
        Attachment::Shield,
    ]
    .map(|point| attachment_bone(&bone_names, &name, point));
    let mut asset = CharacterAsset {
        name,
        primitives,
        textures,
        materials,
        pieces,
        source: path.to_owned(),
        catalog,
        parents,
        base_pose,
        clips,
        skins,
        attachments,
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
            materials: Vec::new(),
            pieces: Vec::new(),
            source: PathBuf::new(),
            catalog: HashMap::new(),
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
            attachments: [Some(0), None, None],
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
    fn a_clip_lasts_as_long_as_its_longest_track() {
        let still = LocalTransform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
        };
        let track = |frames: usize, interval: f32| {
            Some(AnimationTrack {
                frames: vec![still; frames],
                interval,
            })
        };
        let asset = CharacterAsset {
            name: "synthetic".into(),
            primitives: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            pieces: Vec::new(),
            source: PathBuf::new(),
            catalog: HashMap::new(),
            parents: vec![None, Some(0), Some(0)],
            base_pose: vec![still; 3],
            clips: HashMap::from([
                ("C05".into(), vec![track(4, 0.1), None, track(10, 0.08)]),
                ("P01".into(), vec![None, None, None]),
            ]),
            skins: Vec::new(),
            attachments: [None; 3],
        };
        assert!((asset.clip_seconds("C05").unwrap() - 0.8).abs() < 0.0001);
        // A clip with no tracks, or none at all, has no length.
        assert_eq!(asset.clip_seconds("P01"), None);
        assert_eq!(asset.clip_seconds("C08"), None);
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
    fn attachment_points_are_found_by_bone_name_for_this_model() {
        let bones = [
            "HUMPE_TRACK",
            "HUMR_POINT_TRACK",
            "HUML_POINT_DAG",
            "HUMSHIELD_POINT_TRACK",
            "ELMR_POINT_TRACK",
        ];
        assert_eq!(
            attachment_bone(&bones, "HUM", Attachment::RightHand),
            Some(1)
        );
        assert_eq!(
            attachment_bone(&bones, "HUM", Attachment::LeftHand),
            Some(2)
        );
        assert_eq!(attachment_bone(&bones, "HUM", Attachment::Shield), Some(3));
        assert_eq!(
            attachment_bone(&bones, "ELM", Attachment::RightHand),
            Some(4)
        );
        assert_eq!(attachment_bone(&bones, "ELM", Attachment::Shield), None);
    }

    #[test]
    fn attachments_come_in_the_poses_frame() {
        // A bone raised along WLD up (Z) holds its item up the renderer's Y.
        let base = LocalTransform {
            translation: Vec3::new(0.0, 0.0, 5.0),
            rotation: Quat::IDENTITY,
            scale: 1.0,
        };
        let asset = CharacterAsset {
            name: "synthetic".into(),
            primitives: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            pieces: Vec::new(),
            source: PathBuf::new(),
            catalog: HashMap::new(),
            parents: vec![None],
            base_pose: vec![base],
            clips: HashMap::new(),
            skins: Vec::new(),
            attachments: [Some(0), None, None],
        };
        let (_, attachments) = asset.pose_with_attachments("", 0.0, true, &[]);
        let hand = attachments[Attachment::RightHand.index()].unwrap();
        assert!((hand.transform_point3(Vec3::ZERO) - Vec3::new(0.0, 5.0, 0.0)).length() < 0.0001);
        assert!(attachments[Attachment::Shield.index()].is_none());
    }

    #[test]
    fn mesh_names_say_which_body_or_head_they_are() {
        assert_eq!(Piece::of("HUM", "HUM_DMSPRITEDEF"), Piece::Body(0));
        assert_eq!(Piece::of("HUM", "HUM01_DMSPRITEDEF"), Piece::Body(1));
        assert_eq!(Piece::of("HUM", "HUMHE00_DMSPRITEDEF"), Piece::Head(0));
        assert_eq!(Piece::of("HUM", "HUMHE03_DMSPRITEDEF"), Piece::Head(3));
        for other in [
            "ELF_DMSPRITEDEF",
            "HUMHE3_DMSPRITEDEF",
            "HUMXX_DMSPRITEDEF",
            "HUM1A_DMSPRITEDEF",
            "HUM01",
        ] {
            assert_eq!(Piece::of("HUM", other), Piece::Fixed, "{other}");
        }
        assert!(Piece::Fixed.base() && Piece::Head(0).base() && !Piece::Body(1).base());
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

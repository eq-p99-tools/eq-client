//! The bones of a hierarchical sprite and its rest pose, read one way for
//! characters and for the placed zone objects built on skeletons (trees,
//! torches and the like).

use glam::{Mat4, Quat, Vec3};
use libeq::wld::parser::{FrameTransform, HierarchicalSpriteDef, Track, WldDoc};

use crate::LoadError;
use crate::characters::CharacterPose;

/// A bone's transform relative to its parent, as one track frame holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

impl LocalTransform {
    pub fn decode(frame: &FrameTransform) -> Self {
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

    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation,
            self.translation,
        )
    }
}

/// What a bone's base track gives for its rest pose.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Rest {
    /// The base track's first frame.
    Frame(LocalTransform),
    /// The bone names no track.
    Untracked,
    /// The track names no track definition.
    NoDefinition,
    /// The definition holds no frame this reader decodes: `count` frames,
    /// none of them, or only legacy ones.
    NoFrames { count: u32 },
}

/// A hierarchical sprite's bones: each one's parent, rest pose and track
/// name.
#[derive(Debug)]
pub(crate) struct Skeleton<'a> {
    pub parents: Vec<Option<usize>>,
    pub rest: Vec<Rest>,
    /// Each bone's base track name, where it has one.
    pub names: Vec<Option<&'a str>>,
}

impl<'a> Skeleton<'a> {
    /// Reads the bones of `definition`.
    ///
    /// # Errors
    /// Refuses a child bone out of range, a bone that is its own parent or
    /// has two, and a cycle.
    pub fn read(doc: &'a WldDoc, definition: &HierarchicalSpriteDef) -> Result<Self, LoadError> {
        let mut parents = vec![None; definition.dags.len()];
        let mut rest = Vec::with_capacity(definition.dags.len());
        let mut names = Vec::with_capacity(definition.dags.len());
        for (index, bone) in definition.dags.iter().enumerate() {
            let track: Option<&Track> = fragment(doc, bone.track_reference);
            names.push(track.and_then(|track| doc.get_string(track.name_reference)));
            rest.push(match track.map(|track| doc.get(&track.reference)) {
                None => Rest::Untracked,
                Some(None) => Rest::NoDefinition,
                Some(Some(base)) => base
                    .frame_transforms
                    .as_ref()
                    .and_then(|frames| frames.first())
                    .map_or(
                        Rest::NoFrames {
                            count: base.frame_count,
                        },
                        |frame| Rest::Frame(LocalTransform::decode(frame)),
                    ),
            });
            for &child in &bone.sub_dags {
                let parent = parents
                    .get_mut(child as usize)
                    .ok_or_else(|| invalid("child bone"))?;
                if child as usize == index {
                    return Err(invalid("bone parent"));
                }
                if parent.replace(index).is_some() {
                    return Err(invalid("duplicate bone parent"));
                }
            }
        }
        validate_parents(&parents)?;
        Ok(Self {
            parents,
            rest,
            names,
        })
    }

    /// Each bone's transform at rest, in WLD space: its base track's first
    /// frame, or where it has none its default (no move), composed down its
    /// parents. Placed zone objects stand in this pose; characters refuse a
    /// bone without a frame before posing.
    pub fn rest_bones(&self) -> Vec<Mat4> {
        let local: Vec<Mat4> = self
            .rest
            .iter()
            .map(|rest| match rest {
                Rest::Frame(frame) => frame.matrix(),
                _ => Mat4::IDENTITY,
            })
            .collect();
        compose_hierarchy(&local, &self.parents)
    }
}

/// Composes a validated hierarchy without assuming parents precede children.
pub(crate) fn compose_hierarchy(local: &[Mat4], parents: &[Option<usize>]) -> Vec<Mat4> {
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

/// Rejects cycles before future pose evaluation can walk the hierarchy.
pub(crate) fn validate_parents(parents: &[Option<usize>]) -> Result<(), LoadError> {
    for start in 0..parents.len() {
        let mut cursor = Some(start);
        for _ in 0..parents.len() {
            cursor = match cursor {
                Some(index) => *parents.get(index).ok_or_else(|| invalid("parent bone"))?,
                None => break,
            };
        }
        if cursor.is_some() {
            return Err(invalid("cyclic skeleton"));
        }
    }
    Ok(())
}

/// A primitive's bone-local vertices in the renderer's frame, so that a
/// joint from a character's clip places them as they are.
#[derive(Clone, Debug)]
pub(crate) struct Skin {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub bones: Vec<usize>,
}

impl Skin {
    /// Poses this primitive with these joints.
    pub fn pose(&self, joints: &[Mat4]) -> CharacterPose {
        let mut positions = Vec::with_capacity(self.positions.len());
        let mut normals = Vec::with_capacity(self.normals.len());
        for ((position, normal), bone) in self.positions.iter().zip(&self.normals).zip(&self.bones)
        {
            let joint = joints[*bone];
            positions.push(
                joint
                    .transform_point3(Vec3::from_array(*position))
                    .to_array(),
            );
            normals.push(
                joint
                    .transform_vector3(Vec3::from_array(*normal))
                    .normalize_or_zero()
                    .to_array(),
            );
        }
        CharacterPose { positions, normals }
    }
}

/// The fragment a 1-based reference names, if it is of this type.
pub(crate) fn fragment<T: libeq::wld::parser::Fragment + 'static>(
    doc: &WldDoc,
    reference: u32,
) -> Option<&T> {
    let index = i32::try_from(reference).ok().filter(|index| *index > 0)?;
    doc.get(&libeq::wld::parser::FragmentRef::new(index))
}

fn invalid(detail: &str) -> LoadError {
    LoadError::ParseWorld(format!("invalid skeleton {detail}"))
}

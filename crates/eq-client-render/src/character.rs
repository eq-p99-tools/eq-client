//! CPU skinning for the small classic models; asset and pose math stay renderer independent.

use bevy::prelude::*;
use eq_client_assets::characters::CharacterAsset;
use std::sync::Arc;

use super::{create_render_primitives, create_texture_images};

#[derive(Component)]
pub(super) struct AnimatedCharacter {
    pub(super) asset: Arc<CharacterAsset>,
    /// Per drawn primitive, its asset index and this instance's posed mesh.
    meshes: Vec<(usize, Handle<Mesh>)>,
    /// The entities drawing the primitives.
    pub(super) parts: Vec<Part>,
    /// The gear last drawn and whether its helm showed, if dressed yet.
    pub(super) dressed: Option<(eq_client_core::outfit::Appearance, bool)>,
    /// The entity the parts hang from; held items hang from it too.
    pub(super) model: Entity,
    /// The render layers the model draws on.
    pub(super) layers: bevy::camera::visibility::RenderLayers,
    /// Items in the primary and secondary hands.
    pub(super) held: [Option<Held>; 2],
    /// Where held items attach in the latest pose, in `Attachment` order.
    pub(super) attachments: [Option<Mat4>; 3],
    /// Per asset primitive, whether its piece is drawn; hidden ones are not
    /// posed.
    pub(super) shown: Vec<bool>,
    elapsed: f32,
    since_pose: f32,
    moving_for: f32,
    previous_position: Vec3,
    clip: (&'static str, bool),
}

/// A corpse: drawn with its race's model, lying as it fell.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(super) struct Corpse;

/// The death clip, whose last frame is the body lying on the ground.
const DEATH: (&str, bool) = ("D05", true);

/// How high over a corpse's position its name shows.
pub(super) const CORPSE_NAME_HEIGHT: f32 = 1.5;

/// One drawn primitive of a character instance.
pub(super) struct Part {
    /// Index into the asset's primitives and base material names.
    pub(super) index: usize,
    /// The entity drawing it.
    pub(super) entity: Entity,
    /// Its material in the base look.
    pub(super) base: Handle<StandardMaterial>,
}

/// An item drawn in a hand.
pub(super) struct Held {
    /// The item's model number, such as 10 for `IT10`.
    pub(super) number: u32,
    /// The entity drawing it.
    pub(super) entity: Entity,
    /// Where it attaches.
    pub(super) point: eq_client_assets::characters::Attachment,
}

/// Marks a held item's entity, which follows its attachment point.
#[derive(Component)]
pub(super) struct HeldItem;

/// Held items, placed and shown by their character's animation.
type HeldItems<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility),
    (With<HeldItem>, Without<AnimatedCharacter>),
>;

/// The model a movement root wears, for views that draw it again elsewhere.
#[derive(Component)]
pub(super) struct Model(pub PreparedCharacter);

/// Attaches the model to the movement root while keeping feet at terrain height.
pub(super) fn spawn(
    commands: &mut Commands,
    parent: Entity,
    asset: CharacterAsset,
    feet_offset: f32,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let prepared = prepare(asset, images, meshes, materials);
    spawn_prepared(commands, parent, &prepared, feet_offset, meshes);
    commands.entity(parent).insert(Model(prepared));
}

/// Shared immutable model, texture, and material data for nearby instances.
#[derive(Clone)]
pub(super) struct PreparedCharacter {
    asset: Arc<CharacterAsset>,
    /// Drawable primitives, each with the index of its asset primitive, which
    /// also indexes its pose and base material name.
    primitives: Vec<(usize, Handle<Mesh>, Handle<StandardMaterial>)>,
}

impl PreparedCharacter {
    pub fn height(&self) -> f32 {
        self.asset.height()
    }
}

/// Uploads model textures once; posed vertex buffers remain per-instance.
pub(super) fn prepare(
    asset: CharacterAsset,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> PreparedCharacter {
    let textures = create_texture_images(asset.textures.clone(), images);
    // Empty primitives draw nothing; the rest keep their asset index.
    let drawn: Vec<usize> = (0..asset.primitives.len())
        .filter(|index| {
            let primitive = &asset.primitives[*index];
            !primitive.indices.is_empty() && !primitive.positions.is_empty()
        })
        .collect();
    let rendered = create_render_primitives(
        drawn
            .iter()
            .map(|index| asset.primitives[*index].clone())
            .collect(),
        &textures,
        meshes,
        materials,
        true,
    );
    for (handle, _) in &rendered {
        if let Some(mut mesh) = meshes.get_mut(handle) {
            mesh.asset_usage = bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD;
        }
    }
    PreparedCharacter {
        asset: Arc::new(asset),
        primitives: drawn
            .into_iter()
            .zip(rendered)
            .map(|(index, (mesh, material))| (index, mesh, material))
            .collect(),
    }
}

/// Creates an independently animated instance sharing its model's textures/materials.
pub(super) fn spawn_prepared(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
    meshes: &mut Assets<Mesh>,
) {
    spawn_on_layers(
        commands,
        parent,
        prepared,
        feet_offset,
        meshes,
        &bevy::camera::visibility::RenderLayers::default(),
    );
}

/// As [`spawn_prepared`], drawn only by cameras that see one of `layers`.
pub(super) fn spawn_on_layers(
    commands: &mut Commands,
    parent: Entity,
    prepared: &PreparedCharacter,
    feet_offset: f32,
    meshes: &mut Assets<Mesh>,
    layers: &bevy::camera::visibility::RenderLayers,
) {
    let asset = prepared.asset.clone();
    let primitives: Vec<_> = prepared
        .primitives
        .iter()
        .filter_map(|(index, handle, material)| {
            let mesh = meshes.get(handle)?.clone();
            Some((*index, meshes.add(mesh), material.clone()))
        })
        .collect();
    let handles = primitives
        .iter()
        .map(|(index, mesh, _)| (*index, mesh.clone()))
        .collect();
    let (bottom, top) = asset.span().unwrap_or_default();
    // Until gear says otherwise, only the base body and bare head show.
    let shown: Vec<bool> = asset.pieces.iter().map(|piece| piece.base()).collect();
    let mut parts = Vec::with_capacity(primitives.len());
    let child = commands
        .spawn((
            // Classic character meshes face heading 0 as stored, like all WLD
            // geometry (installed HUM/ERM/ELM/DWM toes extend along it), not
            // the movement root's +Z. Keep protocol heading on the root and
            // turn the model once here.
            Transform::from_xyz(0.0, -feet_offset - bottom, 0.0)
                .with_rotation(Quat::from_rotation_y(eq_client_core::model_yaw())),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            for (index, mesh, material) in primitives {
                let visibility = if shown.get(index).copied().unwrap_or(true) {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                let part = parent
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        visibility,
                        layers.clone(),
                    ))
                    .id();
                parts.push(Part {
                    index,
                    entity: part,
                    base: material,
                });
            }
        })
        .id();
    commands
        .entity(parent)
        .add_child(child)
        // The model hangs with its soles at the feet, so its top is this far
        // above the root.
        .insert(super::names::Overhead(top - bottom - feet_offset))
        .insert(AnimatedCharacter {
            asset,
            meshes: handles,
            parts,
            dressed: None,
            model: child,
            layers: layers.clone(),
            held: [None, None],
            attachments: [None; 3],
            shown,
            elapsed: 0.0,
            since_pose: 0.0,
            moving_for: 0.0,
            previous_position: Vec3::ZERO,
            clip: ("P01", false),
        });
}

/// The characters the animation poses, with what decides their clip: the
/// spawn they draw, whether they are the player, and whether a corpse.
type Animated<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static mut AnimatedCharacter,
        Option<&'static super::entities::RemoteEntity>,
        Has<super::Player>,
        Has<Corpse>,
    ),
>;

/// Updates local animation only; it cannot send movement or gameplay commands.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate(
    time: Res<Time>,
    online: Res<super::online::OnlineState>,
    mut characters: Animated,
    mut held_items: HeldItems,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (transform, mut character, remote, own, corpse) in &mut characters {
        let id = remote.map(|entity| entity.id).or_else(|| {
            own.then(|| online.world().player().map(|player| player.spawn_id))
                .flatten()
        });
        let posture = id.and_then(|id| online.world().posture(id));
        let moving = transform
            .translation
            .distance_squared(character.previous_position)
            > 0.0001;
        character.previous_position = transform.translation;
        if posture == Some(eq_client_core::PostureState::Frozen) {
            continue;
        }
        let dt = time.delta_secs().min(0.1);
        character.elapsed += dt;
        character.since_pose += dt;
        character.moving_for = if moving {
            0.15
        } else {
            (character.moving_for - dt).max(0.0)
        };
        if character.since_pose < 1.0 / 30.0 {
            continue;
        }
        character.since_pose = 0.0;
        let (selected, start) = chosen_clip(corpse, posture, character.moving_for > 0.0);
        if selected != character.clip {
            character.clip = selected;
            character.elapsed = start;
        }
        let (clip, held) = selected;
        let (mut poses, attachments) =
            character
                .asset
                .pose_with_attachments(clip, character.elapsed, !held, &character.shown);
        // Held items follow their hand or shield point, and stay hidden on a
        // skeleton without one rather than sitting at the model's origin.
        character.attachments = attachments;
        for item in character.held.iter().flatten() {
            let Ok((mut placed, mut shown)) = held_items.get_mut(item.entity) else {
                continue;
            };
            let point = attachments[item.point.index()];
            if let Some(point) = point {
                *placed = Transform::from_matrix(point);
            }
            let visibility = if point.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            shown.set_if_neq(visibility);
        }
        for (index, handle) in &character.meshes {
            let Some(pose) = poses.get_mut(*index).and_then(Option::take) else {
                continue;
            };
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pose.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, pose.normals);
            }
        }
    }
}

/// The clip a character shows, and where in it a newly chosen clip starts:
/// a corpse lies as it fell, in the death clip's last frame, rather than
/// falling each time it is drawn (a held clip stays on its last frame).
fn chosen_clip(
    corpse: bool,
    posture: Option<eq_client_core::PostureState>,
    moving: bool,
) -> ((&'static str, bool), f32) {
    if corpse {
        (DEATH, 1.0e6)
    } else {
        (posture_clip(posture, moving), 0.0)
    }
}

/// WLD clip names documented by `EQEmu`; presentation never changes network position.
fn posture_clip(
    posture: Option<eq_client_core::PostureState>,
    moving: bool,
) -> (&'static str, bool) {
    use eq_client_core::PostureState;
    match posture {
        Some(PostureState::Sitting) => ("P02", true),
        Some(PostureState::Ducking) if moving => ("L06", false),
        Some(PostureState::Ducking) => ("L08", true),
        Some(PostureState::Looting) => ("P05", true),
        Some(PostureState::Lying) => ("D05", true),
        _ if moving => ("L01", false),
        _ => ("P01", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corpse_lies_in_the_death_clips_last_frame_whatever_its_posture() {
        use eq_client_core::PostureState;
        let (clip, start) = chosen_clip(true, Some(PostureState::Standing), true);
        assert_eq!(clip, ("D05", true));
        assert!(start > 1000.0);
        assert_eq!(
            chosen_clip(false, Some(PostureState::Sitting), false),
            (("P02", true), 0.0)
        );
    }

    #[test]
    fn posture_overrides_walk_and_transition_clips_hold_their_final_pose() {
        use eq_client_core::PostureState;
        assert_eq!(
            posture_clip(Some(PostureState::Sitting), true),
            ("P02", true)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Ducking), false),
            ("L08", true)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Ducking), true),
            ("L06", false)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Standing), true),
            ("L01", false)
        );
        assert_eq!(
            posture_clip(Some(PostureState::Unknown(999)), false),
            ("P01", false)
        );
    }
}

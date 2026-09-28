//! CPU skinning for the small classic models; asset and pose math stay renderer independent.

use bevy::prelude::*;
use eq_client_assets::characters::CharacterAsset;
use std::sync::Arc;

use super::{create_render_primitives, create_texture_images};

#[derive(Component)]
pub(super) struct AnimatedCharacter {
    asset: Arc<CharacterAsset>,
    meshes: Vec<Handle<Mesh>>,
    elapsed: f32,
    since_pose: f32,
    moving_for: f32,
    previous_position: Vec3,
    clip: (&'static str, bool),
}

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
}

/// Shared immutable model, texture, and material data for nearby instances.
#[derive(Clone)]
pub(super) struct PreparedCharacter {
    asset: Arc<CharacterAsset>,
    primitives: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
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
    let primitives =
        create_render_primitives(asset.primitives.clone(), &textures, meshes, materials, true);
    for (handle, _) in &primitives {
        if let Some(mut mesh) = meshes.get_mut(handle) {
            mesh.asset_usage = bevy::asset::RenderAssetUsages::MAIN_WORLD
                | bevy::asset::RenderAssetUsages::RENDER_WORLD;
        }
    }
    PreparedCharacter {
        asset: Arc::new(asset),
        primitives,
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
    let asset = prepared.asset.clone();
    let primitives: Vec<_> = prepared
        .primitives
        .iter()
        .filter_map(|(handle, material)| {
            let mesh = meshes.get(handle)?.clone();
            Some((meshes.add(mesh), material.clone()))
        })
        .collect();
    let handles = primitives.iter().map(|(mesh, _)| mesh.clone()).collect();
    let bottom = asset
        .primitives
        .iter()
        .flat_map(|p| &p.positions)
        .map(|p| p[1])
        .fold(f32::INFINITY, f32::min);
    let child = commands
        .spawn((
            // Classic character meshes face opposite the movement root's +Z
            // forward convention. Keep protocol heading on the root and correct
            // the model-forward convention once here.
            Transform::from_xyz(0.0, -feet_offset - bottom, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            Visibility::Inherited,
        ))
        .with_children(|parent| {
            for (mesh, material) in primitives {
                parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
            }
        })
        .id();
    commands
        .entity(parent)
        .add_child(child)
        .insert(AnimatedCharacter {
            asset,
            meshes: handles,
            elapsed: 0.0,
            since_pose: 0.0,
            moving_for: 0.0,
            previous_position: Vec3::ZERO,
            clip: ("P01", false),
        });
}

/// Updates local animation only; it cannot send movement or gameplay commands.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate(
    time: Res<Time>,
    online: Res<super::online::OnlineState>,
    mut characters: Query<(
        &Transform,
        &mut AnimatedCharacter,
        Option<&super::entities::RemoteEntity>,
        Has<super::Player>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (transform, mut character, remote, own) in &mut characters {
        let id = remote.map(|entity| entity.id).or_else(|| {
            own.then(|| online.player.as_ref().map(|player| player.spawn_id))
                .flatten()
        });
        let posture = id.and_then(|id| online.postures.get(&id)).copied();
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
        let selected = posture_clip(posture, character.moving_for > 0.0);
        if selected != character.clip {
            character.clip = selected;
            character.elapsed = 0.0;
        }
        let (clip, held) = selected;
        let poses = if held {
            character.asset.pose_held(clip, character.elapsed)
        } else {
            character.asset.pose(clip, character.elapsed)
        };
        for (handle, pose) in character.meshes.iter().zip(poses) {
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pose.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, pose.normals);
            }
        }
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

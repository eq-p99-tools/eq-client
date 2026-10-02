//! Nearby spawn presentation. Distant entities retain state but own no render objects.

use super::{ViewerSettings, character, online::OnlineState};
use bevy::prelude::*;
use eq_client_assets::characters::load_installed_character;
use eq_client_core::{SpawnKind, entities::nearby, races, render_position};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
pub(super) struct NearbyEntities {
    pub(super) rendered: BTreeMap<u16, Entity>,
    revisions: BTreeMap<u16, u64>,
    models: BTreeMap<&'static str, Option<character::PreparedCharacter>>,
    elapsed: f32,
}

impl NearbyEntities {
    /// Forgets the spawns drawn for an admission the world forgot, and the
    /// models loaded for its zone.
    pub(super) fn forget(&mut self, commands: &mut Commands) {
        for (_, entity) in std::mem::take(&mut self.rendered) {
            commands.entity(entity).despawn();
        }
        self.models.clear();
        self.revisions.clear();
    }
}

#[derive(Component)]
pub(super) struct RemoteEntity {
    pub(super) id: u16,
    /// The server report this entity moves on from, and when it was first seen.
    report: Option<(eq_client_core::WorldPosition, [f32; 3], std::time::Instant)>,
}

/// Maintains a bounded nearby set. At most one new model is instantiated per frame.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
pub(super) fn reconcile(
    mut commands: Commands,
    state: Res<OnlineState>,
    (settings, options): (Res<ViewerSettings>, Res<crate::options::OptionsState>),
    time: Res<Time>,
    mut nearby_state: ResMut<NearbyEntities>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !state.world().connected() {
        return;
    }
    let Some(player) = state.world().player() else {
        return;
    };
    let Some(directory) = &settings.0.eq_directory else {
        return;
    };
    nearby_state.elapsed += time.delta_secs();
    if nearby_state.elapsed < 0.1 {
        return;
    }
    nearby_state.elapsed = 0.0;
    let replaced: Vec<_> = nearby_state
        .rendered
        .keys()
        .copied()
        .filter(|id| {
            nearby_state.revisions.get(id).copied()
                != state.world().spawn(*id).map(|spawn| spawn.revision)
        })
        .collect();
    for id in replaced {
        if let Some(entity) = nearby_state.rendered.remove(&id) {
            commands.entity(entity).despawn();
        }
        nearby_state.revisions.remove(&id);
    }
    let present: BTreeSet<_> = nearby_state.rendered.keys().copied().collect();
    let selected = nearby(
        state.world().spawns().values().map(|spawn| &spawn.state),
        player.spawn_id,
        player.position,
        &present,
        // Drawing stops where the scene's does, the Far Clip Plane's share of
        // the zone's far clip; with no far clip, at its share of the client's
        // own distance.
        {
            let distance = settings.0.entity_distance.unwrap_or(200.0);
            match options.options.clip_distance(state.world().far_clip()) {
                Some(clip) => distance.min(clip),
                None => distance * options.options.clip_share(),
            }
        },
        200,
    );
    let desired: BTreeSet<_> = selected.iter().copied().collect();
    for id in present.difference(&desired) {
        if let Some(entity) = nearby_state.rendered.remove(id) {
            commands.entity(entity).despawn();
        }
    }
    let Some(id) = selected
        .into_iter()
        .find(|id| !nearby_state.rendered.contains_key(id))
    else {
        return;
    };
    let spawn = &state.world().spawns()[&id].state;
    let model = races::model(spawn.race, spawn.gender);
    let asset = model.and_then(|code| {
        nearby_state
            .models
            .entry(code)
            .or_insert_with(|| {
                // Cache failures too, avoiding repeated disk reads for unsupported models.
                load_installed_character(directory, state.world().zone(), code)
                    .ok()
                    .map(|asset| {
                        character::prepare(asset, &mut images, &mut meshes, &mut materials)
                    })
            })
            .clone()
    });
    let corpse = matches!(spawn.kind, SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse);
    let position = Vec3::from_array(render_position(spawn.position));
    let entity = commands
        .spawn((
            RemoteEntity { id, report: None },
            Name::new(spawn.name.clone()),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
                eq_client_core::render_heading(spawn.position.heading),
            )),
            Visibility::Inherited,
        ))
        .id();
    if !races::drawn(spawn.race) {
        // An unseen marker, such as a spawn point: nothing to draw.
    } else if let Some(asset) = asset.filter(|_| !corpse) {
        let height = asset.height().max(0.1);
        let scale = if spawn.size > 0.0 {
            (spawn.size / height).clamp(0.05, 20.0)
        } else {
            1.0
        };
        // The server reports the position EQ's size rule puts above the feet; the
        // offset is in model units because the model is scaled below its root.
        let feet = eq_client_core::z_offset(spawn.race, spawn.size) / scale;
        character::spawn_prepared(&mut commands, entity, &asset, feet, &mut meshes);
        commands.entity(entity).insert(
            Transform::from_translation(position)
                .with_rotation(Quat::from_rotation_y(eq_client_core::render_heading(
                    spawn.position.heading,
                )))
                .with_scale(Vec3::splat(scale)),
        );
    } else {
        // An explicit marker keeps unknown model races visible without inventing an appearance.
        let height = if corpse {
            0.5
        } else {
            spawn.size.clamp(1.0, 6.0)
        };
        let color = match spawn.kind {
            SpawnKind::Player => Color::srgb(0.4, 0.65, 0.9),
            SpawnKind::Npc => Color::srgb(0.8, 0.65, 0.3),
            _ => Color::srgb(0.4, 0.4, 0.4),
        };
        commands
            .entity(entity)
            .insert(super::names::Overhead(height / 2.0));
        commands.entity(entity).with_children(|children| {
            children.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.6, height, 0.6))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    unlit: true,
                    ..default()
                })),
            ));
        });
    }
    nearby_state.rendered.insert(id, entity);
    nearby_state
        .revisions
        .insert(id, state.world().spawns()[&id].revision);
}

/// Interpolates between received locations without extrapolating beyond the server.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn interpolate(
    state: Res<OnlineState>,
    time: Res<Time>,
    mut entities: Query<(&mut RemoteEntity, &mut Transform)>,
) {
    let weight = 1.0 - (-time.delta_secs().min(0.1) / 0.1).exp();
    let now = std::time::Instant::now();
    for (mut entity, mut transform) in &mut entities {
        let Some(spawn) = state.world().spawn(entity.id).map(|spawn| &spawn.state) else {
            continue;
        };
        // A spawn moves on from its latest report at its reported velocity.
        let since = match entity.report {
            Some((position, velocity, seen))
                if position == spawn.position
                    && velocity.map(f32::to_bits) == spawn.velocity.map(f32::to_bits) =>
            {
                seen
            }
            _ => {
                entity.report = Some((spawn.position, spawn.velocity, now));
                now
            }
        };
        let position = eq_client_core::entities::extrapolate(spawn, now.duration_since(since));
        let target = Vec3::from_array(render_position(position));
        if transform.translation.distance_squared(target) > 100.0 * 100.0 {
            transform.translation = target;
        } else {
            transform.translation = transform.translation.lerp(target, weight);
        }
        transform.rotation = transform.rotation.slerp(
            Quat::from_rotation_y(eq_client_core::render_heading(spawn.position.heading)),
            weight,
        );
    }
}

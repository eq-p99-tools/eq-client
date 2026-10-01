//! Nearby spawn presentation. Distant entities retain state but own no render objects.

use super::{ViewerSettings, character, online::OnlineState};
use bevy::prelude::*;
use eq_client_assets::characters::load_installed_character;
use eq_client_core::{
    SpawnKind, WorldEvent, WorldUpdate, entities::nearby, races, render_position,
};
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
    settings: Res<ViewerSettings>,
    time: Res<Time>,
    mut nearby_state: ResMut<NearbyEntities>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !state.world.connected() {
        return;
    }
    let Some(player) = state.world.player() else {
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
                != state.world.spawn(*id).map(|spawn| spawn.revision)
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
        state.world.spawns().values().map(|spawn| &spawn.state),
        player.spawn_id,
        player.position,
        &present,
        // As the official client's clip plane, drawing stops at the zone's far clip.
        settings
            .0
            .entity_distance
            .unwrap_or(200.0)
            .min(state.world.far_clip().unwrap_or(f32::INFINITY)),
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
    let spawn = &state.world.spawns()[&id].state;
    let model = races::model(spawn.race, spawn.gender);
    let asset = model.and_then(|code| {
        nearby_state
            .models
            .entry(code)
            .or_insert_with(|| {
                // Cache failures too, avoiding repeated disk reads for unsupported models.
                load_installed_character(directory, state.world.zone(), code)
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
        .insert(id, state.world.spawns()[&id].revision);
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
        let Some(spawn) = state.world.spawn(entity.id).map(|spawn| &spawn.state) else {
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

/// Exercises the same nearby renderer using three synthetic, moving entities offline.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn demo(
    settings: Res<ViewerSettings>,
    time: Res<Time>,
    mut state: ResMut<OnlineState>,
    scene: Res<super::SceneInfo>,
    players: Query<&Transform, With<super::Player>>,
    surface: Res<super::TerrainSurface>,
    mut chat: ResMut<super::chat::ChatState>,
) {
    if !settings.0.demo_entities || state.enabled {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let origin = super::world_position(player.translation.to_array(), 0.0);
    let now = std::time::Instant::now();
    let news = |state: &mut OnlineState, event| {
        state.world.apply(
            &WorldUpdate::Game(event),
            now,
            &eq_client_core::world::NoSpells,
        );
    };
    if state.world.session_id().is_none() {
        super::online::admit_preview(&mut state, origin, &scene.zone_name);
    }
    if chat.history.revision() == 0 {
        super::chat::seed_demo(&mut chat.history);
    }
    // The preview player stands wherever the viewer moved them.
    news(
        &mut state,
        WorldEvent::Position {
            spawn_id: 1,
            position: origin,
            velocity: [0.0; 3],
        },
    );
    for (id, race, offset, size) in [(2u16, 1, 0.0, 0.0), (3, 42, 2.1, 2.5), (4, 54, 4.2, 6.0)] {
        let phase = time.elapsed_secs() % 18.0;
        let angle = if id == 2 && phase < 12.0 {
            0.0
        } else {
            time.elapsed_secs() * 0.25 + offset
        };
        let mut p = origin;
        let radius = if id == 2 { 6.0 } else { 14.0 };
        p.x += angle.cos() * radius;
        p.y += angle.sin() * radius;
        let height = if size > 0.0 { size } else { 6.0 };
        let [x, _, z] = render_position(p);
        p.z = surface
            .height_below(x, z, origin.z + 10.0)
            .unwrap_or(origin.z - height * 0.5)
            + height * 0.5;
        p.heading = (-angle).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 512.0;
        // A spawn appears once, then only moves, so it is not drawn again.
        if state.world.spawn(id).is_none() {
            news(
                &mut state,
                WorldEvent::Spawns(vec![synthetic(id, race, size, p)]),
            );
        } else {
            news(
                &mut state,
                WorldEvent::Position {
                    spawn_id: id,
                    position: p,
                    velocity: [0.0; 3],
                },
            );
        }
        if id == 2 {
            let posture = if phase < 6.0 {
                eq_client_core::PostureState::Sitting
            } else if phase < 12.0 {
                eq_client_core::PostureState::Ducking
            } else {
                eq_client_core::PostureState::Standing
            };
            news(
                &mut state,
                WorldEvent::Posture {
                    spawn_id: id,
                    posture,
                },
            );
        }
    }
}

/// One of the demo's synthetic spawns: spawn 2 a player, the rest creatures.
fn synthetic(
    id: u16,
    race: u32,
    size: f32,
    position: eq_client_core::WorldPosition,
) -> eq_client_core::SpawnState {
    eq_client_core::SpawnState {
        class: None,
        spawn_id: id,
        name: format!("Synthetic {id}"),
        kind: if id == 2 {
            SpawnKind::Player
        } else {
            SpawnKind::Npc
        },
        race,
        gender: 0,
        position,
        velocity: [0.0; 3],
        size,
        invisible: false,
        appearance: eq_client_core::outfit::Appearance::default(),
    }
}

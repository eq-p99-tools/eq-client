//! Nearby spawn presentation. Distant entities retain state but own no render objects.

use super::{ViewerSettings, character, online::OnlineState};
use bevy::prelude::*;
use eq_client_assets::characters::load_installed_character;
use eq_client_core::{SpawnKind, classic_model, entities::nearby, render_position};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
pub(super) struct NearbyEntities {
    session: Option<u64>,
    pub(super) rendered: BTreeMap<u16, Entity>,
    revisions: BTreeMap<u16, u64>,
    models: BTreeMap<&'static str, Option<character::PreparedCharacter>>,
    elapsed: f32,
}

#[derive(Component)]
pub(super) struct RemoteEntity {
    pub(super) id: u16,
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
    if nearby_state.session != state.session_id || state.finished {
        for (_, entity) in std::mem::take(&mut nearby_state.rendered) {
            commands.entity(entity).despawn();
        }
        nearby_state.models.clear();
        nearby_state.revisions.clear();
        nearby_state.session = state.session_id;
    }
    if !state.connected {
        return;
    }
    let Some(player) = &state.player else {
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
        .filter(|id| nearby_state.revisions.get(id) != state.revisions.get(id))
        .collect();
    for id in replaced {
        if let Some(entity) = nearby_state.rendered.remove(&id) {
            commands.entity(entity).despawn();
        }
        nearby_state.revisions.remove(&id);
    }
    let present: BTreeSet<_> = nearby_state.rendered.keys().copied().collect();
    let selected = nearby(
        &state.spawns,
        player.spawn_id,
        player.position,
        &present,
        settings.0.entity_distance.unwrap_or(200.0),
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
    let spawn = &state.spawns[&id];
    let model = model_code(spawn.race, spawn.gender);
    let asset = model.and_then(|code| {
        nearby_state
            .models
            .entry(code)
            .or_insert_with(|| {
                // Cache failures too, avoiding repeated disk reads for unsupported models.
                load_installed_character(directory, &state.zone, code)
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
            RemoteEntity { id },
            Name::new(spawn.name.clone()),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
                eq_client_core::render_heading(spawn.position.heading),
            )),
            Visibility::Inherited,
        ))
        .id();
    if let Some(asset) = asset.filter(|_| !corpse) {
        let height = asset.height().max(0.1);
        let scale = if spawn.size > 0.0 {
            (spawn.size / height).clamp(0.05, 20.0)
        } else {
            1.0
        };
        character::spawn_prepared(&mut commands, entity, &asset, height * 0.5, &mut meshes);
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
    nearby_state.revisions.insert(id, state.revisions[&id]);
}

/// Interpolates between received locations without extrapolating beyond the server.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn interpolate(
    state: Res<OnlineState>,
    time: Res<Time>,
    mut entities: Query<(&RemoteEntity, &mut Transform)>,
) {
    let weight = 1.0 - (-time.delta_secs().min(0.1) / 0.1).exp();
    for (entity, mut transform) in &mut entities {
        let Some(spawn) = state.spawns.get(&entity.id) else {
            continue;
        };
        let target = Vec3::from_array(render_position(spawn.position));
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

fn model_code(race: u32, gender: u32) -> Option<&'static str> {
    classic_model(race, gender).or(match race {
        22 => Some("BET"),
        37 => Some("SNA"),
        42 => Some("WOL"),
        43 => Some("BEA"),
        44 => Some("GNN"),
        50 => Some("LIM"),
        54 => Some("ORC"),
        60 => Some("SKE"),
        70 => Some("ZOM"),
        _ => None,
    })
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
    mut hud: ResMut<super::hud::HudState>,
    mut chat: ResMut<super::chat::ChatState>,
) {
    if !settings.0.demo_entities || state.enabled {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let origin = super::world_position(player.translation.to_array(), 0.0);
    state.connected = true;
    state.session_id = Some(1);
    state.zone.clone_from(&scene.zone_name);
    hud.status = "Offline entity demo".into();
    if chat.history.revision() == 0 {
        super::chat::seed_demo(&mut chat.history);
    }
    state.player = Some(eq_client_core::PlayerState {
        base_attributes: None,
        deity: None,
        class: Some(1),
        spawn_id: 1,
        race: 1,
        gender: 0,
        level: 1,
        position: origin,
        mana: 0,
        endurance: Some(0),
        skills: None,
        spell_refresh_ms: None,
        memorized_spells: [None; 8],
        size: 0.0,
        walk_speed: 0.0,
        run_speed: 0.0,
        hp_percent: None,
    });
    for (id, race, offset, size) in [(2u16, 1, 0.0, 0.0), (3, 42, 2.1, 2.5), (4, 54, 4.2, 6.0)] {
        let phase = time.elapsed_secs() % 18.0;
        if id == 2 {
            state.postures.insert(
                id,
                if phase < 6.0 {
                    eq_client_core::PostureState::Sitting
                } else if phase < 12.0 {
                    eq_client_core::PostureState::Ducking
                } else {
                    eq_client_core::PostureState::Standing
                },
            );
        }
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
        p.z = surface
            .height_below(p.y, p.x, origin.z + 10.0)
            .unwrap_or(origin.z - height * 0.5)
            + height * 0.5;
        p.heading = (-angle).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 512.0;
        state.spawns.insert(
            id,
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
                position: p,
                size,
                invisible: false,
            },
        );
        state.revisions.insert(id, 1);
    }
}

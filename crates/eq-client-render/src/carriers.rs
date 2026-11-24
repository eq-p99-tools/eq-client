//! Authored carrier children on ordinary spawn roots, with shared posed collision.
use crate::{
    ViewerSettings,
    background::Background,
    entities::{Drawn, NearbyEntities},
    online::OnlineState,
};
use bevy::prelude::*;
use eq_client_assets::actors::{ActorAsset, load_installed_actor};
use eq_client_core::{
    movement::{CollisionInstance, CollisionMesh},
    races,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

#[derive(Clone)]
struct Model {
    primitives: crate::doors::Primitives,
    collision: Arc<Vec<[[f32; 3]; 3]>>,
    top: f32,
}

/// The ordinary spawn renderer owns this root and its visible fallback.
/// Installing a carrier actor hides only this child, retaining target/name identity.
#[derive(Component)]
pub(super) struct Placeholder {
    pub(super) spawn_id: u16,
    pub(super) root: Entity,
    pub(super) overhead: f32,
}

#[derive(Component)]
struct CarrierModel;

struct Visual {
    root: Entity,
    model: Entity,
    tag: &'static str,
    incarnation: u64,
    scale: f32,
    top: f32,
}

/// Admission-scoped actor loads, children and scaled geometry. Failed loads
/// retain the ordinary spawn's marker and never fabricate collision.
/// These actor assets and BVHs are memory-only; the navigation disk cache
/// contains static zone geometry, not spawned carrier instances.
#[derive(Resource, Default)]
pub(super) struct Scene {
    admission: Option<u64>,
    models: BTreeMap<&'static str, Option<Model>>,
    loading: Option<(&'static str, Background<Option<ActorAsset>>)>,
    rendered: BTreeMap<u16, Visual>,
    colliders: BTreeMap<u16, CachedCollider>,
}

struct CachedCollider {
    incarnation: u64,
    tag: &'static str,
    scale: Vec3,
    mesh: Option<Arc<CollisionMesh>>,
}

/// An incarnation, tag or scale change rebuilds the geometry once, including
/// a failed build. Each moving frame reuses that result and only places the
/// same BVH at the shared carrier pose; no world-space triangle baking.
fn cached_collider(
    cache: &mut BTreeMap<u16, CachedCollider>,
    sample: eq_client_core::movement::CarrierSample,
    tag: &'static str,
    model: &Model,
    transform: &Transform,
) -> Option<CollisionInstance> {
    let previous = cache.get(&sample.spawn_id).filter(|cached| {
        cached.incarnation == sample.incarnation
            && cached.tag == tag
            && cached.scale == transform.scale
    });
    let mesh = if let Some(previous) = previous {
        previous.mesh.clone()
    } else {
        let mesh = CollisionMesh::new(model.collision.iter().map(|triangle| {
            triangle.map(|point| (Vec3::from_array(point) * transform.scale).to_array())
        }))
        .inspect_err(|error| {
            warn!(spawn_id = sample.spawn_id, tag, triangles = model.collision.len(), %error,
                "Could not build carrier collision");
            eq_client_core::gaps::unshown("Carrier collision mesh could not be built");
        })
        .ok()
        .map(Arc::new);
        cache.insert(
            sample.spawn_id,
            CachedCollider {
                incarnation: sample.incarnation,
                tag,
                scale: transform.scale,
                mesh: mesh.clone(),
            },
        );
        mesh
    }?;
    CollisionInstance::new(mesh, transform.translation, transform.rotation).ok()
}

fn prepare(
    asset: ActorAsset,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Model {
    let top = asset
        .model
        .primitives
        .iter()
        .flat_map(|primitive| &primitive.positions)
        .map(|point| point[1])
        .filter(|height| height.is_finite())
        .max_by(f32::total_cmp)
        .unwrap_or(0.0);
    let textures = crate::create_texture_images(asset.textures, images);
    Model {
        primitives: crate::create_render_primitives(
            asset.model.primitives,
            &asset.model.materials,
            &textures,
            meshes,
            materials,
            false,
            crate::character::ON_A_SPAWN,
        ),
        collision: Arc::new(asset.model.collision),
        top,
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn reconcile(
    mut commands: Commands,
    state: Res<OnlineState>,
    settings: Res<ViewerSettings>,
    nearby: Res<NearbyEntities>,
    mut scene: ResMut<Scene>,
    mut collision: ResMut<crate::Collision>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut placeholders: Query<(&Placeholder, &mut Visibility)>,
    mut overheads: Query<&mut crate::names::Overhead>,
    mut roots: Query<&mut Transform, With<crate::entities::RemoteEntity>>,
) {
    let admission = state
        .world()
        .session_id()
        .filter(|_| state.world().connected());
    if scene.admission != admission {
        for (_, visual) in std::mem::take(&mut scene.rendered) {
            commands.entity(visual.model).try_despawn();
        }
        *scene = Scene {
            admission,
            ..Scene::default()
        };
    }
    if let Some((tag, job)) = scene.loading.as_mut()
        && let Some(asset) = job.poll(admission)
    {
        let tag = *tag;
        let model = asset.map(|asset| prepare(asset, &mut images, &mut meshes, &mut materials));
        scene.models.insert(tag, model);
        scene.loading = None;
    }
    // A Spawns/Carriers pair may cross receive frames. Only a root prepared
    // for this carrier may receive an authored child, avoiding a second visual
    // on an ordinary model while its renderer catches up.
    let carrier_roots: BTreeSet<_> = placeholders
        .iter()
        .map(|(placeholder, _)| placeholder.root)
        .collect();
    let now = Instant::now();
    let mut remaining = BTreeSet::new();
    let mut colliders = Vec::new();
    let mut collision_ids = BTreeSet::new();
    if let (Some(player), Some(directory), Some(admission)) = (
        state.world().player(),
        settings.0.eq_directory.as_ref(),
        admission,
    ) {
        for report in state.world().carriers().reports() {
            let id = report.sample().spawn_id;
            let Some(spawn) = state.world().spawn(id).map(|spawn| &spawn.state) else {
                continue;
            };
            let pose = report.pose(now);
            // The ordinary spawn renderer owns draw range and visibility.
            // Collision loading retains its separate bounded reach.
            let root = nearby
                .rendered
                .get(&id)
                .copied()
                .filter(|root| carrier_roots.contains(root));
            if let Some(root) = root
                && let Ok(mut transform) = roots.get_mut(root)
            {
                transform.translation = Vec3::from_array(eq_client_core::render_position(pose));
                transform.rotation =
                    Quat::from_rotation_y(eq_client_core::render_heading(pose.heading));
            }
            let distance = (pose.x - player.position.x)
                .hypot(pose.y - player.position.y)
                .hypot(pose.z - player.position.z);
            // The boat an arrival is held for needs its collider wherever
            // the profile's boat-frame position seems to put the player.
            let arriving = matches!(
                state.world().arrival(),
                Some(eq_client_core::boats::riders::Arrival::Aboard { sample, .. })
                    if sample.spawn_id == id
            );
            let collision_near =
                arriving || distance <= eq_client_core::movement::CARRIER_REACH * 2.0;
            if root.is_none() && !collision_near {
                continue;
            }
            let Some(tag) = races::carrier_actor(spawn.race, spawn.gender) else {
                continue;
            };
            if !scene.models.contains_key(tag) {
                if scene.loading.is_none() {
                    let directory = directory.clone();
                    let zone = state.world().zone().to_owned();
                    let mut job = Background::default();
                    job.start(admission, move || {
                        load_installed_actor(&directory, &zone, tag)
                            .inspect_err(|error| warn!(tag, %error, "Carrier actor unavailable"))
                            .ok()
                    });
                    scene.loading = Some((tag, job));
                }
                continue;
            }
            let Some(model) = scene.models.get(tag).and_then(Option::as_ref).cloned() else {
                continue;
            };
            let drawn = Drawn::of(
                settings.0.installed_client,
                state.world(),
                (spawn.race, spawn.gender, spawn.size),
            );
            let transform = Transform::from_translation(Vec3::from_array(
                eq_client_core::render_position(pose),
            ))
            .with_rotation(Quat::from_rotation_y(eq_client_core::static_yaw(
                pose.heading,
            )))
            .with_scale(Vec3::splat(drawn.scale));
            if collision_near {
                collision_ids.insert(id);
                if let Some(instance) = cached_collider(
                    &mut scene.colliders,
                    report.sample(),
                    tag,
                    &model,
                    &transform,
                ) {
                    colliders.push((id, instance));
                }
            }
            let Some(root) = root.filter(|_| !model.primitives.is_empty()) else {
                continue;
            };
            remaining.insert(id);
            if scene.rendered.get(&id).is_some_and(|visual| {
                visual.root == root
                    && visual.tag == tag
                    && visual.incarnation == report.sample().incarnation
                    && visual.scale.to_bits() == drawn.scale.to_bits()
            }) {
                continue;
            }
            if let Some(previous) = scene.rendered.remove(&id) {
                commands.entity(previous.model).try_despawn();
            }
            let primitives = model.primitives.clone();
            // Spawn roots use the common protocol heading. Only the authored
            // child receives the model-frame yaw and size, as characters do.
            let model_entity = commands
                .spawn((
                    CarrierModel,
                    ChildOf(root),
                    Visibility::Inherited,
                    Transform::from_rotation(Quat::from_rotation_y(eq_client_core::model_yaw()))
                        .with_scale(Vec3::splat(drawn.scale)),
                ))
                .with_children(|parent| {
                    for (mesh, material) in primitives {
                        parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                    }
                })
                .id();
            scene.rendered.insert(
                id,
                Visual {
                    root,
                    model: model_entity,
                    tag,
                    incarnation: report.sample().incarnation,
                    scale: drawn.scale,
                    top: model.top * drawn.scale,
                },
            );
        }
    }
    scene.colliders.retain(|id, _| collision_ids.contains(id));
    let removed: Vec<_> = scene
        .rendered
        .keys()
        .copied()
        .filter(|id| !remaining.contains(id))
        .collect();
    for id in removed {
        if let Some(visual) = scene.rendered.remove(&id) {
            commands.entity(visual.model).try_despawn();
        }
    }
    for (placeholder, mut visibility) in &mut placeholders {
        let visual = scene
            .rendered
            .get(&placeholder.spawn_id)
            .filter(|visual| visual.root == placeholder.root);
        *visibility = if visual.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Ok(mut overhead) = overheads.get_mut(placeholder.root) {
            overhead.0 = visual.map_or(placeholder.overhead, |visual| visual.top);
        }
    }
    if let Some(world) = collision.0.as_mut() {
        world.set_spawn_instances(colliders);
    }
}

#[cfg(test)]
mod tests;

//! Received carrier actors: one pose for drawing, collision and rider motion.
use crate::{ViewerSettings, background::Background, entities::Drawn, online::OnlineState};
use bevy::prelude::*;
use eq_client_assets::actors::{ActorAsset, load_installed_actor};
use eq_client_core::{
    movement::{CollisionMesh, CollisionWorld},
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
}

/// Only this admission's static actor assets and entities. Failed loads are
/// remembered until the admission changes; old jobs cannot install into it.
#[derive(Resource, Default)]
pub(super) struct Scene {
    admission: Option<u64>,
    models: BTreeMap<&'static str, Option<Model>>,
    loading: Option<(&'static str, Background<Option<ActorAsset>>)>,
    rendered: BTreeMap<u16, (Entity, &'static str)>,
    colliders: BTreeMap<u16, CachedCollider>,
}

struct CachedCollider {
    incarnation: u64,
    tag: &'static str,
    transform: Transform,
    mesh: Option<Arc<CollisionMesh>>,
}

fn cached_collider(
    cache: &mut BTreeMap<u16, CachedCollider>,
    sample: eq_client_core::movement::CarrierSample,
    tag: &'static str,
    model: &Model,
    transform: &Transform,
) -> Option<Arc<CollisionMesh>> {
    let previous = cache.get(&sample.spawn_id).filter(|cached| {
        cached.incarnation == sample.incarnation
            && cached.tag == tag
            && cached.transform == *transform
    });
    if let Some(previous) = previous {
        return previous.mesh.clone();
    }
    let mesh = collider(model, transform);
    cache.insert(
        sample.spawn_id,
        CachedCollider {
            incarnation: sample.incarnation,
            tag,
            transform: *transform,
            mesh: mesh.clone(),
        },
    );
    mesh
}

/// Carries a real model's solid triangles with the exact drawn transform.
/// The source of the original client's carrier mesh is unread; choosing its
/// own authored model collision is inferred (recording checklist 6.467).
fn collider(model: &Model, transform: &Transform) -> Option<Arc<CollisionMesh>> {
    CollisionMesh::new(model.collision.iter().map(|triangle| {
        triangle.map(|point| {
            transform
                .transform_point(Vec3::from_array(point))
                .to_array()
        })
    }))
    .ok()
    .map(Arc::new)
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
    options: Res<crate::options::OptionsState>,
    mut scene: ResMut<Scene>,
    mut collision: ResMut<crate::Collision>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform, Without<crate::OrbitCamera>>,
    cameras: Query<(&Transform, &Projection), With<crate::OrbitCamera>>,
) {
    let admission = state
        .world()
        .session_id()
        .filter(|_| state.world().connected());
    if scene.admission != admission {
        for (_, (entity, _)) in std::mem::take(&mut scene.rendered) {
            commands.entity(entity).despawn();
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
        let model = asset.map(|asset| {
            let textures = crate::create_texture_images(asset.textures, &mut images);
            Model {
                primitives: crate::create_render_primitives(
                    asset.model.primitives,
                    &asset.model.materials,
                    &textures,
                    &mut meshes,
                    &mut materials,
                    false,
                    bevy::asset::RenderAssetUsages::RENDER_WORLD,
                ),
                collision: Arc::new(asset.model.collision),
            }
        });
        scene.models.insert(tag, model);
        scene.loading = None;
    }
    let now = Instant::now();
    let mut remaining = BTreeSet::new();
    let mut colliders = Vec::new();
    let mut collision_ids = BTreeSet::new();
    if let (Some(player), Some(directory), Some(admission)) = (
        state.world().player(),
        settings.0.eq_directory.as_ref(),
        admission,
    ) {
        let clip = crate::camera::clip_distance(
            settings.0.installed_client,
            &options.options,
            state.world(),
        )
        .unwrap_or(eq_client_assets::camera::FarPlane::UNCLIPPED);
        let reach = crate::entities::loading_reach(
            clip,
            cameras.single().ok(),
            Vec3::from_array(eq_client_core::render_position(player.position)),
        );
        let draw_reach = settings
            .0
            .entity_distance
            .map_or(reach, |distance| distance.min(reach));
        for report in state.world().carriers().reports() {
            let id = report.sample().spawn_id;
            let Some(spawn) = state.world().spawn(id).map(|spawn| &spawn.state) else {
                continue;
            };
            let pose = report.pose(now);
            let distance = (pose.x - player.position.x)
                .hypot(pose.y - player.position.y)
                .hypot(pose.z - player.position.z);
            let reveal = options
                .options
                .qol
                .on(eq_client_core::qol::Fix::ShowInvisible);
            let draw_near = distance <= draw_reach && (reveal || state.world().can_see(spawn));
            let collision_near = distance <= eq_client_core::movement::CARRIER_REACH * 2.0;
            if !draw_near && !collision_near {
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
                if let Some(mesh) = cached_collider(
                    &mut scene.colliders,
                    report.sample(),
                    tag,
                    &model,
                    &transform,
                ) {
                    colliders.push((id, mesh));
                }
            }
            if !draw_near {
                continue;
            }
            remaining.insert(id);
            let current = scene.rendered.get(&id).copied();
            match current {
                Some((entity, previous)) if previous == tag => {
                    if let Ok(mut current) = transforms.get_mut(entity) {
                        *current = transform;
                    }
                }
                previous => {
                    if let Some((entity, _)) = previous {
                        commands.entity(entity).despawn();
                    }
                    let primitives = model.primitives.clone();
                    let entity = commands
                        .spawn((transform, Visibility::Inherited))
                        .with_children(|parent| {
                            for (mesh, material) in primitives {
                                parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
                            }
                        })
                        .id();
                    scene.rendered.insert(id, (entity, tag));
                }
            }
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
        if let Some((entity, _)) = scene.rendered.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    if let Some(world) = collision.0.as_mut() {
        replace_colliders(world, colliders);
    }
}

fn replace_colliders(world: &mut CollisionWorld, colliders: Vec<(u16, Arc<CollisionMesh>)>) {
    world.set_spawn_obstacles(colliders);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deck_collision_uses_the_drawn_translation_rotation_and_scale() {
        let model = Model {
            primitives: Vec::new(),
            collision: Arc::new(vec![[[-4.0, 0.0, -3.0], [4.0, 0.0, -3.0], [0.0, 0.0, 3.0]]]),
        };
        let transform = Transform::from_xyz(20.0, 5.0, 30.0)
            .with_rotation(Quat::from_rotation_y(1.0))
            .with_scale(Vec3::splat(2.0));
        let mut world = CollisionWorld::new([[
            [-100.0, -10.0, -100.0],
            [100.0, -10.0, -100.0],
            [0.0, -10.0, 100.0],
        ]])
        .unwrap();
        replace_colliders(&mut world, vec![(8, collider(&model, &transform).unwrap())]);
        assert_eq!(
            world.ground_spawn(Vec3::new(20.0, 5.0, 30.0), 0.1, 0.1),
            Some(8)
        );
        replace_colliders(&mut world, Vec::new());
        assert!(
            world
                .ground_spawn(Vec3::new(20.0, 5.0, 30.0), 0.1, 0.1)
                .is_none()
        );
    }
    #[test]
    fn an_unchanged_carrier_pose_reuses_its_collision_acceleration_structure() {
        let now = Instant::now();
        let mut spawn = crate::online::testing::pet(8, 7);
        spawn.race = 72;
        spawn.pet_owner = None;
        let mut reports = eq_client_core::movement::Carriers::default();
        reports.observe(
            &eq_client_core::WorldEvent::Spawns(vec![spawn]),
            eq_client_core::GameDialect::Titanium,
            now,
        );
        let sample = reports.latest(8).unwrap().sample();
        let model = Model {
            primitives: Vec::new(),
            collision: Arc::new(vec![[[-4.0, 0.0, -3.0], [4.0, 0.0, -3.0], [0.0, 0.0, 3.0]]]),
        };
        let mut cache = BTreeMap::new();
        let first =
            cached_collider(&mut cache, sample, "SHIP", &model, &Transform::IDENTITY).unwrap();
        let second =
            cached_collider(&mut cache, sample, "SHIP", &model, &Transform::IDENTITY).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let moved = cached_collider(
            &mut cache,
            sample,
            "SHIP",
            &model,
            &Transform::from_xyz(1.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&first, &moved));
    }
}

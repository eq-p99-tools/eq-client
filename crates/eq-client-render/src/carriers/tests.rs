use super::*;
use crate::{entities, names, online::testing};
use eq_client_assets::{MaterialMode, ObjectMaterial, ZoneModel, ZonePrimitive};
use eq_client_core::{
    ClientCommand, GameDialect, WorldEvent, WorldPosition,
    movement::{Carriers, CollisionWorld},
};
use std::{path::PathBuf, sync::mpsc, time::Duration};

fn asset() -> ActorAsset {
    let points = vec![[-4.0, 0.0, -3.0], [4.0, 0.0, -3.0], [0.0, 0.0, 3.0]];
    ActorAsset {
        model: ZoneModel {
            name: "BOAT_ACTORDEF".into(),
            primitives: vec![ZonePrimitive {
                positions: points.clone(),
                normals: vec![[0.0, 1.0, 0.0]; 3],
                texture_coordinates: vec![[0.0; 2]; 3],
                indices: vec![0, 1, 2],
                texture: None,
                material_mode: MaterialMode::Opaque,
                light: Vec::new(),
            }],
            materials: vec![ObjectMaterial { double_sided: true }],
            collision: vec![[points[0], points[1], points[2]]],
            no_hit_collision: Vec::new(),
            vertex_count: None,
        },
        textures: Vec::new(),
    }
}

fn fixture(gender: u32, invisible: bool) -> (App, Model) {
    let mut images = Assets::<Image>::default();
    let mut meshes = Assets::<Mesh>::default();
    let mut materials = Assets::<StandardMaterial>::default();
    let model = prepare(asset(), &mut images, &mut meshes, &mut materials);
    let mut state = OnlineState::new(true);
    testing::admit(&mut state, 11, testing::player(7));
    let mut spawn = testing::pet(8, 7);
    spawn.name = "a_carrier_fixture00".into();
    spawn.race = 141;
    spawn.gender = gender;
    spawn.size = 1.0;
    spawn.pet_owner = None;
    spawn.invisible = invisible;
    spawn.position = WorldPosition {
        x: 10.0,
        y: 0.0,
        z: 5.0,
        heading: 128.0,
    };
    let spawned = WorldEvent::Spawns(vec![spawn]);
    let mut carriers = Carriers::default();
    let reports = carriers.observe(&spawned, GameDialect::Titanium, Instant::now());
    testing::news(&mut state, [spawned, WorldEvent::Carriers(reports)]);
    let scene = Scene {
        admission: Some(11),
        models: BTreeMap::from([("BOAT", None)]),
        ..Scene::default()
    };
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(200));
    let collision = CollisionWorld::new([[
        [-1000.0, -100.0, -1000.0],
        [1000.0, -100.0, -1000.0],
        [0.0, -100.0, 1000.0],
    ]])
    .unwrap();
    let mut app = App::new();
    app.insert_resource(state)
        .insert_resource(scene)
        .insert_resource(time)
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
            eq_directory: Some(PathBuf::from("synthetic-installation")),
            ..crate::ViewerConfig::default()
        }))
        .insert_resource(crate::Collision(Some(collision)))
        .insert_resource(images)
        .insert_resource(meshes)
        .insert_resource(materials)
        .init_resource::<entities::NearbyEntities>()
        .init_resource::<crate::options::OptionsState>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<names::NameTags>()
        .add_systems(
            Update,
            (
                entities::reconcile,
                entities::interpolate,
                reconcile,
                names::update,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .npc_names = true;
    (app, model)
}

fn root(app: &App) -> Entity {
    app.world().resource::<NearbyEntities>().rendered[&8]
}

fn placeholder_visibility(app: &mut App) -> Visibility {
    *app.world_mut()
        .query_filtered::<&Visibility, With<Placeholder>>()
        .single(app.world())
        .unwrap()
}

#[test]
fn failed_loading_and_unknown_actors_keep_ordinary_name_and_target_roots_without_floor() {
    for state in 0..3 {
        let (mut app, _) = fixture(if state == 2 { 9 } else { 2 }, false);
        if state == 1 {
            let mut scene = app.world_mut().resource_mut::<Scene>();
            scene.models.remove("BOAT");
            let mut job = Background::default();
            job.keep_pending(11);
            scene.loading = Some(("BOAT", job));
        }
        app.update();
        let root = root(&app);
        assert!(app.world().get::<entities::RemoteEntity>(root).is_some());
        assert!(app.world().get::<names::Overhead>(root).is_some());
        assert_eq!(placeholder_visibility(&mut app), Visibility::Inherited);
        let labels: Vec<_> = app
            .world_mut()
            .query_filtered::<&Text, With<names::NameTag>>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        assert_eq!(labels.len(), 1);
        assert!(labels[0].contains("carrier fixture"));
        assert!(crate::target::unavailable(app.world().resource::<OnlineState>(), 8).is_none());
        let (sent, received) = mpsc::sync_channel(2);
        crate::target::select(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            &crate::outbox::Outbox::new(Some(sent)),
            Some(8),
        );
        assert!(matches!(
            received.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                spawn_id: Some(8),
                ..
            }
        ));
        let collision = app
            .world()
            .resource::<crate::Collision>()
            .0
            .as_ref()
            .unwrap();
        assert_eq!(collision.obstacle_count(), 0);
        assert!(
            collision
                .ground_spawn(Vec3::new(0.0, 5.0, 10.0), 0.01, 0.05)
                .is_none()
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Checks one ordered visual/collision lifecycle.
fn loading_and_retiring_an_actor_preserves_the_spawn_root_without_duplicate_visuals() {
    let (mut app, model) = fixture(2, false);
    app.update();
    let original = root(&app);
    app.world_mut()
        .resource_mut::<Scene>()
        .models
        .insert("BOAT", Some(model));
    app.update();
    assert_eq!(root(&app), original);
    assert_eq!(placeholder_visibility(&mut app), Visibility::Hidden);
    let scene = app.world().resource::<Scene>();
    let visual = &scene.rendered[&8];
    assert_eq!(visual.root, original);
    let model_entity = visual.model;
    assert_eq!(
        app.world().get::<ChildOf>(model_entity).unwrap().parent(),
        original
    );
    let models = app
        .world_mut()
        .query_filtered::<Entity, With<CarrierModel>>()
        .iter(app.world())
        .count();
    assert_eq!(models, 1);
    let handle = app.world().resource::<Scene>().models["BOAT"]
        .as_ref()
        .unwrap()
        .primitives[0]
        .0
        .clone();
    assert!(
        app.world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .is_some()
    );
    assert!(
        app.world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .unwrap()
            .asset_usage
            .contains(bevy::asset::RenderAssetUsages::MAIN_WORLD)
    );
    let world = app.world();
    let root_transform = world.get::<Transform>(original).unwrap();
    let child_transform = world.get::<Transform>(model_entity).unwrap();
    let pose = world
        .resource::<OnlineState>()
        .world()
        .spawn(8)
        .unwrap()
        .state
        .position;
    let expected =
        Transform::from_translation(Vec3::from_array(eq_client_core::render_position(pose)))
            .with_rotation(Quat::from_rotation_y(eq_client_core::static_yaw(
                pose.heading,
            )));
    for point in asset().model.collision[0] {
        let point = Vec3::from_array(point);
        assert!(
            ((root_transform.to_matrix() * child_transform.to_matrix()).transform_point3(point)
                - expected.transform_point(point))
            .length()
                < 0.001
        );
    }
    assert!(crate::target::unavailable(app.world().resource::<OnlineState>(), 8).is_none());
    let floor = Vec3::from_array(eq_client_core::render_position(
        app.world()
            .resource::<OnlineState>()
            .world()
            .spawn(8)
            .unwrap()
            .state
            .position,
    ));
    assert_eq!(
        app.world()
            .resource::<crate::Collision>()
            .0
            .as_ref()
            .unwrap()
            .ground_spawn(floor, 0.01, 0.05),
        Some(8)
    );
    app.world_mut()
        .resource_mut::<Scene>()
        .models
        .insert("BOAT", None);
    app.update();
    assert_eq!(root(&app), original);
    assert_eq!(placeholder_visibility(&mut app), Visibility::Inherited);
    assert!(app.world().get_entity(model_entity).is_err());
    assert_eq!(
        app.world()
            .resource::<crate::Collision>()
            .0
            .as_ref()
            .unwrap()
            .obstacle_count(),
        0
    );
}

#[test]
fn moving_carriers_reuse_the_bvh_and_a_new_admission_clears_roots_and_instances() {
    let (mut app, model) = fixture(2, false);
    app.world_mut()
        .resource_mut::<Scene>()
        .models
        .insert("BOAT", Some(model));
    app.update();
    let original = root(&app);
    let first = Arc::clone(
        app.world().resource::<Scene>().colliders[&8]
            .mesh
            .as_ref()
            .unwrap(),
    );
    let mut carriers = app
        .world()
        .resource::<OnlineState>()
        .world()
        .carriers()
        .clone();
    let changed = WorldEvent::Position {
        spawn_id: 8,
        position: WorldPosition {
            x: 30.0,
            y: 15.0,
            z: 8.0,
            heading: 220.0,
        },
        velocity: [0.0; 3],
        forward: 0.0,
    };
    let reports = carriers.observe(&changed, GameDialect::Titanium, Instant::now());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [changed, WorldEvent::Carriers(reports)],
    );
    app.update();
    let second = Arc::clone(
        app.world().resource::<Scene>().colliders[&8]
            .mesh
            .as_ref()
            .unwrap(),
    );
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(root(&app), original);
    let spawn = app
        .world()
        .resource::<OnlineState>()
        .world()
        .spawn(8)
        .unwrap()
        .state
        .clone();
    let replacement = WorldEvent::Spawns(vec![spawn]);
    let updates = carriers.observe(&replacement, GameDialect::Titanium, Instant::now());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [replacement, WorldEvent::Carriers(updates)],
    );
    app.update();
    let replacement_root = root(&app);
    assert_ne!(replacement_root, original);
    assert!(app.world().get_entity(original).is_err());
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<CarrierModel>>()
            .iter(app.world())
            .count(),
        1
    );
    testing::admit(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        12,
        testing::player(7),
    );
    app.update();
    assert!(app.world().get_entity(replacement_root).is_err());
    assert!(
        !app.world()
            .resource::<NearbyEntities>()
            .rendered
            .contains_key(&8)
    );
    assert_eq!(
        app.world()
            .resource::<crate::Collision>()
            .0
            .as_ref()
            .unwrap()
            .obstacle_count(),
        0
    );
}

#[test]
fn invisible_carrier_fallback_uses_the_ordinary_visibility_and_target_policy() {
    let (mut app, _) = fixture(2, true);
    app.update();
    assert!(
        !app.world()
            .resource::<NearbyEntities>()
            .rendered
            .contains_key(&8)
    );
    app.world_mut()
        .resource_mut::<crate::options::OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::ShowInvisible, true);
    app.update();
    assert!(
        app.world()
            .resource::<NearbyEntities>()
            .rendered
            .contains_key(&8)
    );
    assert!(crate::target::unavailable(app.world().resource::<OnlineState>(), 8).is_some());
}

#[test]
fn split_spawn_and_carrier_events_do_not_install_an_actor_on_an_old_ordinary_root() {
    let (mut app, model) = fixture(2, false);
    app.world_mut()
        .resource_mut::<Scene>()
        .models
        .insert("BOAT", Some(model));
    let mut carriers = app
        .world()
        .resource::<OnlineState>()
        .world()
        .carriers()
        .clone();
    let spawn = app
        .world()
        .resource::<OnlineState>()
        .world()
        .spawn(8)
        .unwrap()
        .state
        .clone();
    let replacement = WorldEvent::Spawns(vec![spawn]);
    let updates = carriers.observe(&replacement, GameDialect::Titanium, Instant::now());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [replacement],
    );
    app.update();
    let ordinary = root(&app);
    assert!(app.world().resource::<Scene>().rendered.is_empty());
    assert!(app.world().get::<names::Overhead>(ordinary).is_some());
    testing::news(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        [WorldEvent::Carriers(updates)],
    );
    app.update();
    let carrier = root(&app);
    assert_ne!(carrier, ordinary);
    assert!(app.world().get_entity(ordinary).is_err());
    assert_eq!(app.world().resource::<Scene>().rendered[&8].root, carrier);
    assert_eq!(placeholder_visibility(&mut app), Visibility::Hidden);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<CarrierModel>>()
            .iter(app.world())
            .count(),
        1
    );
}

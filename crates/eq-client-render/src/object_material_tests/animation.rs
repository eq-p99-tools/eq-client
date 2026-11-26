use super::*;
use bevy::ecs::system::RunSystemOnce;
use eq_client_assets::{TextureAnimation, ZoneTexture};
use std::{num::NonZeroU32, time::Duration};

const COLORS: [[u8; 4]; 4] = [
    [30, 60, 90, 255],
    [90, 60, 30, 255],
    [40, 80, 120, 255],
    [120, 80, 40, 255],
];

fn zone() -> ZoneAsset {
    let mut empty = triangle(MaterialMode::Additive);
    empty.indices.clear();
    let mut flame = triangle(MaterialMode::Additive);
    flame.texture = Some(0);
    let mut masked = triangle(MaterialMode::Masked);
    masked.texture = Some(0);
    let material = |animation| ObjectMaterial {
        double_sided: false,
        animation,
    };
    let mut zone = ZoneAsset {
        regions: default(),
        short_name: "synthetic".into(),
        primitives: Vec::new(),
        terrain_materials: Vec::new(),
        point_lights: Vec::new(),
        day_ambient: None,
        textures: COLORS
            .into_iter()
            .enumerate()
            .map(|(i, color)| ZoneTexture {
                name: format!("frame{i}.png"),
                width: 1,
                height: 1,
                rgba8: color.to_vec(),
                color_key: None,
                compressed: None,
            })
            .collect(),
        models: vec![ZoneModel {
            name: "TEST_ACTORDEF".into(),
            primitives: vec![empty, triangle(MaterialMode::Opaque), flame, masked],
            materials: vec![
                material(None),
                material(None),
                material(Some(TextureAnimation {
                    frames: vec![0, 1, 2, 3],
                    frame_millis: NonZeroU32::new(100).unwrap(),
                    skip_frames: false,
                })),
                material(Some(TextureAnimation {
                    frames: vec![0, 2, 3, 1],
                    frame_millis: NonZeroU32::new(200).unwrap(),
                    skip_frames: false,
                })),
            ],
            collision: Vec::new(),
            no_hit_collision: Vec::new(),
            vertex_count: Some(3),
            radius: None,
        }],
        objects: Vec::new(),
        objects_unplaced: 0,
        object_skins_skipped: 0,
        object_lights_skipped: 0,
        collision: Vec::new(),
    };
    for (index, x) in [0.0, 20.0, 600.0, 640.0].into_iter().enumerate() {
        zone.objects.push(ZoneObject {
            model: 0,
            translation: [x, 0.0, 0.0],
            rotation: Quat::IDENTITY.to_array(),
            scale: [if index == 3 { -1.0 } else { 1.0 }, 1.0, 1.0],
            light: (index == 1).then(|| vec![[80, 90, 100, 255]; 3]),
        });
    }
    zone
}

fn spawn(app: &mut App, zone: ZoneAsset) {
    let mut images = app
        .world_mut()
        .remove_resource::<Assets<Image>>()
        .unwrap_or_default();
    let mut meshes = app
        .world_mut()
        .remove_resource::<Assets<Mesh>>()
        .unwrap_or_default();
    let mut materials = app
        .world_mut()
        .remove_resource::<Assets<StandardMaterial>>()
        .unwrap_or_default();
    let mut queue = CommandQueue::default();
    spawn_static_zone(
        &mut Commands::new(&mut queue, app.world()),
        zone,
        false,
        &mut images,
        &mut meshes,
        &mut materials,
    );
    queue.apply(app.world_mut());
    app.insert_resource(images)
        .insert_resource(meshes)
        .insert_resource(materials);
}

fn fixture() -> App {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_systems(Update, gpu_assets::animate_objects);
    spawn(&mut app, zone());
    app
}

fn tick(app: &mut App, millis: u64) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(millis));
    app.update();
}

fn frame(app: &App, handle: &Handle<StandardMaterial>) -> Vec<u8> {
    let material = app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(handle)
        .unwrap();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(material.base_color_texture.as_ref().unwrap())
        .unwrap();
    image.data.as_ref().unwrap()[..4].to_vec()
}

fn controllers(app: &mut App) -> usize {
    let world = app.world_mut();
    world
        .query::<&gpu_assets::ObjectAnimation>()
        .iter(world)
        .count()
}

#[test]
fn shared_object_timers_keep_authored_intervals_reflected_frames_and_material_state() {
    let mut app = fixture();
    let model = &app.world().resource::<doors::Models>().0["TEST"];
    let still = model.primitives[0].1.clone();
    let flame = model.primitives[1].1.clone();
    let masked = model.primitives[2].1.clone();
    // Doors take the same material handles from this zone model registry.
    let door = app.world_mut().spawn(MeshMaterial3d(flame.clone())).id();
    let mut draws = app.world_mut().query::<&MeshMaterial3d<StandardMaterial>>();
    let handles: Vec<_> = draws.iter(app.world()).map(|draw| draw.0.clone()).collect();
    let material_assets = app.world().resource::<Assets<StandardMaterial>>();
    let reflected = handles
        .iter()
        .find(|handle| {
            let material = material_assets.get(*handle).unwrap();
            material.alpha_mode == AlphaMode::Add && material.cull_mode == Some(Face::Front)
        })
        .unwrap()
        .clone();
    let original = material_assets.get(&flame).unwrap().clone();
    let reflected_original = material_assets.get(&reflected).unwrap().clone();
    let still_original = material_assets
        .get(&still)
        .unwrap()
        .base_color_texture
        .clone();
    let mesh_count = app.world().resource::<Assets<Mesh>>().len();
    assert_eq!(controllers(&mut app), 2);
    assert_eq!(handles.iter().filter(|handle| **handle == flame).count(), 4);
    assert_eq!(
        handles
            .iter()
            .filter(|handle| **handle == reflected)
            .count(),
        1
    );
    assert_eq!(
        handles.iter().filter(|handle| **handle == masked).count(),
        2
    );
    for (millis, flame_frame, masked_frame) in
        [(100, 0, 0), (1, 1, 0), (100, 1, 2), (1, 2, 2), (900, 3, 3)]
    {
        tick(&mut app, millis);
        assert_eq!(frame(&app, &flame), COLORS[flame_frame]);
        assert_eq!(frame(&app, &reflected), COLORS[flame_frame]);
        assert_eq!(frame(&app, &masked), COLORS[masked_frame]);
        assert_eq!(
            &app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(door)
                .unwrap()
                .0,
            &flame
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        for (handle, before) in [(&flame, &original), (&reflected, &reflected_original)] {
            let material = materials.get(handle).unwrap();
            assert_eq!(material.alpha_mode, before.alpha_mode);
            assert_eq!(material.cull_mode, before.cull_mode);
            assert_eq!(material.base_color, before.base_color);
            assert_eq!(material.unlit, before.unlit);
            assert_eq!(material.double_sided, before.double_sided);
        }
        assert_eq!(
            materials.get(&still).unwrap().base_color_texture,
            still_original
        );
    }
}

#[test]
fn the_production_zone_leave_drops_controllers_and_reentry_starts_at_frame_zero() {
    let mut app = fixture();
    let previous = app.world().resource::<doors::Models>().0["TEST"].primitives[1]
        .1
        .clone();
    tick(&mut app, 101);
    assert_eq!(frame(&app, &previous), COLORS[1]);
    app.world_mut()
        .run_system_once(|mut commands: Commands, mut scene: crate::zone::Scene| {
            scene.leave(
                &mut commands,
                &mut eq_client_assets::regions::ZoneRegions::default(),
            );
        })
        .unwrap();
    assert_eq!(controllers(&mut app), 0);
    assert!(app.world().resource::<doors::Models>().0.is_empty());
    tick(&mut app, 1000);
    assert_eq!(frame(&app, &previous), COLORS[1]);
    spawn(&mut app, zone());
    assert_eq!(controllers(&mut app), 2);
    let next = &app.world().resource::<doors::Models>().0["TEST"].primitives[1].1;
    assert_ne!(next, &previous);
    assert_eq!(frame(&app, next), COLORS[0]);
}

#[test]
fn still_actor_callers_keep_their_texture_and_do_not_install_object_timers() {
    let source = zone();
    let model = source.models.into_iter().next().unwrap();
    let mut app = App::new();
    let mut images = Assets::default();
    let textures = create_texture_images(source.textures, &mut images);
    let mut materials = Assets::default();
    let draws = create_render_primitives(
        model.primitives,
        &model.materials,
        &textures,
        &mut Assets::default(),
        &mut materials,
        false,
        character::ON_A_SPAWN,
    );
    app.insert_resource(images).insert_resource(materials);
    assert_eq!(controllers(&mut app), 0);
    assert_eq!(frame(&app, &draws[1].1), COLORS[0]);
}

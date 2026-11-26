use super::*;
use crate::online::testing as session;
use eq_client_assets::lighting::PlacedLight;
use eq_client_core::{
    WorldEvent,
    clock::{Fog, ZoneSky},
};

#[test]
fn the_base_lifts_every_vertex_by_a_little_of_the_zone_ambient() {
    let inputs = lighting::interior_inputs(4, None, lighting::Vision::Ordinary, None);
    // A palace vertex: its own light, no ambient weight.
    let engine = zone_light::base_light([200, 100, 0, 0], inputs);
    let tuning = Tuning::default();
    let fill = tuning.fill;
    let rich = base([200, 100, 0, 0], inputs, None, &[], &tuning);
    for channel in 0..3 {
        assert!((rich[channel] - engine[channel] - fill * 0.5).abs() < 1e-5);
    }
}

#[test]
fn a_lamp_lights_a_rough_surface_as_the_engine_term_does() {
    // Bevy: colour × I / (4π) × 1/π × exposure / d²; the engine:
    // colour × 0.05 R² / d².
    let pi = std::f32::consts::PI;
    let radius = 30.0;
    let bevy = intensity(radius, 1.0) / (4.0 * pi) / pi * Exposure::default().exposure();
    assert!((bevy - 0.05 * radius * radius).abs() < 1e-2);
}

#[test]
fn flames_flicker_within_their_bounds_and_magic_does_not() {
    assert!(warm([0.996, 0.9412, 0.6274]));
    assert!(!warm([0.4, 0.996, 0.996]));
    for step in 0..1000 {
        #[allow(clippy::cast_precision_loss)]
        let wander = flicker(step as f32 * 0.037, 1.3);
        assert!((-1.0..=1.0).contains(&wander));
    }
}

fn lamp() -> PlacedLight {
    PlacedLight {
        position: [0.0, 10.0, 0.0],
        radius: 20.0,
        rgb: [0.9, 0.5, 0.2],
        level: 1.0,
    }
}

#[test]
fn the_wash_tints_by_the_lights_nearby_and_fades_smoothly_to_nothing() {
    let tuning = Tuning::default();
    let inputs = lighting::interior_inputs(4, None, lighting::Vision::Ordinary, None);
    let near = near(&[lamp()], ([0.0; 3], [200.0, 0.0, 0.0]), &tuning);
    assert_eq!(near.len(), 1);
    let alone = base([0, 0, 0, 0], inputs, None, &[], &tuning);
    let wash = |x: f32| {
        let lit = base([0, 0, 0, 0], inputs, Some([x, 10.0, 0.0]), &near, &tuning);
        lit[0] - alone[0]
    };
    // Full under the lamp, the lamp's own hue, fading with distance, and
    // nothing from the span's edge on.
    assert!((wash(0.0) - tuning.wash * 0.9).abs() < 1e-5);
    assert!(wash(10.0) < wash(0.0) && wash(10.0) > wash(30.0));
    assert!(wash(39.0) > 0.0 && wash(39.0) < 5e-3);
    assert_eq!(wash(40.0), 0.0);
    assert_eq!(wash(100.0), 0.0);
    // A box out of the span takes no light.
    assert_eq!(
        super::near(&[lamp()], ([100.0; 3], [200.0; 3]), &tuning),
        []
    );
}

fn palace() -> (App, Handle<Mesh>, Handle<StandardMaterial>) {
    zone(0)
}

fn zone(sky: u8) -> (App, Handle<Mesh>, Handle<StandardMaterial>) {
    let mut online = OnlineState::new(true);
    let mut own = session::player(7);
    own.appearance.light = 2;
    session::admit(&mut online, 5, own);
    session::news(
        &mut online,
        [WorldEvent::Sky(ZoneSky {
            sky,
            time_type: 4,
            fog: [Fog {
                color: [0; 3],
                near: 0.0,
                far: 0.0,
            }; 4],
        })],
    );
    let placed = lamp();
    let mut scene = SceneLights::new(online.world().zone().to_owned(), vec![placed], None);
    let mut meshes = Assets::<Mesh>::default();
    let mesh = meshes.add(Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    ));
    let positions = vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 100.0]];
    scene.meshes.push(lighting::LitMesh {
        offset: 0,
        mesh: mesh.clone(),
        light: vec![[100, 50, 0, 0]; 3],
        placed_at: Some((positions, vec![[0.0, 1.0, 0.0]; 3])),
        additive: false,
    });
    let mut standard = Assets::<StandardMaterial>::default();
    let material = standard.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    let mut app = App::new();
    app.insert_resource(online)
        .insert_resource(scene)
        .insert_resource(meshes)
        .insert_resource(standard)
        .insert_resource(Assets::<RichMaterial>::default())
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
        .init_resource::<OptionsState>()
        .init_resource::<NearbyEntities>()
        .init_resource::<RichState>()
        .init_resource::<Tuning>()
        .init_resource::<Time>()
        .add_systems(
            Update,
            (lighting::update, gather, swap, follow_frames, lamps, grade).chain(),
        );
    app.world_mut().spawn((
        Player,
        Transform::from_xyz(0.0, 0.0, 0.0),
        PlayerBody {
            feet_offset: 0.0,
            height: 6.0,
            size: 6.0,
        },
    ));
    app.world_mut().spawn((
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 120.0,
            yaw: 0.0,
            pitch: -0.5,
            pulled: None,
            view: default(),
            look: 0.0,
        },
        Transform::default(),
        Tonemapping::None,
    ));
    app.world_mut()
        .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
    (app, mesh, material)
}

fn set(app: &mut App, on: bool) {
    app.world_mut()
        .resource_mut::<OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::RichLighting, on);
}

#[test]
#[allow(clippy::too_many_lines)] // The whole round trip, on and off.
fn rich_lighting_swaps_the_zone_in_and_out_whole() {
    let (mut app, mesh, material) = palace();
    // Off, the zone draws as the engine does: nothing changes hands.
    app.update();
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&MeshMaterial3d<StandardMaterial>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(world.query::<&PointLight>().iter(world).count(), 0);
    let engine = match app
        .world()
        .resource::<Assets<Mesh>>()
        .get(&mesh)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_COLOR)
    {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) => colours.clone(),
        other => panic!("no vertex colours: {other:?}"),
    };

    set(&mut app, true);
    // One frame swaps and places the lamps, the next places the torch.
    app.update();
    app.update();
    let world = app.world_mut();
    let (drawn, parked) = world
        .query::<(&MeshMaterial3d<RichMaterial>, &Parked)>()
        .single(world)
        .unwrap();
    assert_eq!(parked.0, material);
    let rich = world
        .resource::<Assets<RichMaterial>>()
        .get(&drawn.0)
        .unwrap();
    assert!(!rich.base.unlit);
    assert_eq!(
        world
            .query::<&MeshMaterial3d<StandardMaterial>>()
            .iter(world)
            .count(),
        0
    );
    // The lamp and the player's torch: whole and unshadowed, as torch
    // shadows are the player's to ask for.
    assert_eq!(world.query::<&Lamp>().iter(world).count(), 1);
    let whole: Vec<_> = world
        .query::<(&Carried, &PointLight)>()
        .iter(world)
        .map(|(carried, light)| (carried.0, light.shadow_maps_enabled, light.intensity))
        .collect();
    assert_eq!(whole.len(), 1);
    assert_eq!((whole[0].0, whole[0].1), (0, false));
    // Asked for, the torch is a shadowed part and an unshadowed fill that
    // sum to the whole light.
    world
        .resource_mut::<OptionsState>()
        .options
        .qol
        .set(eq_client_core::qol::Fix::TorchShadows, true);
    app.update();
    let world = app.world_mut();
    let mut torch: Vec<_> = world
        .query::<(&Carried, &PointLight)>()
        .iter(world)
        .map(|(carried, light)| (carried.0, light.shadow_maps_enabled, light.intensity))
        .collect();
    torch.sort_by_key(|part| part.0);
    assert_eq!(
        torch
            .iter()
            .map(|part| (part.0, part.1))
            .collect::<Vec<_>>(),
        [(0, true), (1, false)]
    );
    // The player's own body casts no shadow from the light inside it.
    let own_near = world
        .query::<(&Carried, &PointLight)>()
        .iter(world)
        .find(|(carried, _)| carried.0 == 0)
        .map(|(_, light)| light.shadow_map_near_z)
        .unwrap();
    let height = world
        .query_filtered::<&PlayerBody, With<Player>>()
        .single(world)
        .unwrap()
        .height;
    assert!((own_near - height * Tuning::default().own_shadow_near).abs() < 1e-4 && own_near > 1.0);
    let strength = Tuning::default().shadow_strength;
    assert!((torch[0].2 * (1.0 - strength) - torch[1].2 * strength).abs() < 1e-2 * torch[0].2);
    assert!((torch[0].2 + torch[1].2 - whole[0].2).abs() < 1e-2 * whole[0].2);
    let (tone, glow) = world
        .query::<(&Tonemapping, Has<Bloom>)>()
        .single(world)
        .unwrap();
    assert_eq!(*tone, Tuning::default().tone);
    assert!(glow);
    // The vertex colours hold the base alone, brighter by the fill than
    // the engine's base, with no point light in them.
    let colours = match world
        .resource::<Assets<Mesh>>()
        .get(&mesh)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_COLOR)
    {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) => colours.clone(),
        other => panic!("no vertex colours: {other:?}"),
    };
    let inputs = lighting::interior_inputs(4, None, lighting::Vision::Ordinary, None);
    assert_eq!(
        colours[1],
        zone_light::vertex_colour(base(
            [100, 50, 0, 0],
            inputs,
            Some([100.0, 0.0, 0.0]),
            &near(
                &[lamp()],
                ([0.0; 3], [100.0, 0.0, 100.0]),
                &Tuning::default()
            ),
            &Tuning::default(),
        ))
    );
    assert_ne!(colours[1], engine[1]);

    set(&mut app, false);
    app.update();
    app.update();
    let world = app.world_mut();
    let plain = world
        .query::<&MeshMaterial3d<StandardMaterial>>()
        .single(world)
        .unwrap();
    assert_eq!(plain.0, material);
    assert_eq!(world.query::<&Parked>().iter(world).count(), 0);
    assert_eq!(world.query::<&PointLight>().iter(world).count(), 0);
    let (tone, glow) = world
        .query::<(&Tonemapping, Has<Bloom>)>()
        .single(world)
        .unwrap();
    assert_eq!(*tone, Tonemapping::None);
    assert!(!glow);
    let colours = match world
        .resource::<Assets<Mesh>>()
        .get(&mesh)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_COLOR)
    {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) => colours.clone(),
        other => panic!("no vertex colours: {other:?}"),
    };
    assert_eq!(colours, engine);
}

#[test]
fn outdoors_lights_wait_for_the_night_and_keep_their_strength_through_it() {
    assert!(outdoor_scale(0.0).abs() < f32::EPSILON);
    assert!(outdoor_scale(0.5).abs() < f32::EPSILON);
    // One stop down: drawn at twice, graded back to half the strength.
    assert!((outdoor_scale(-1.0) - 1.0).abs() < 1e-6);
    assert!((outdoor_scale(-2.0) - 3.0).abs() < 1e-6);
}

#[test]
fn outdoors_rich_lighting_keeps_the_plain_day_and_adds_the_lamps() {
    let (mut app, mesh, _) = zone(1);
    set(&mut app, true);
    app.update();
    app.update();
    let world = app.world_mut();
    // The surfaces take the lamps per pixel...
    assert_eq!(
        world
            .query::<&MeshMaterial3d<RichMaterial>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(world.query::<&Lamp>().iter(world).count(), 1);
    // ...while their colours stay the plain texture's and no tone curve
    // darkens the day; the glow stays.
    let (tone, glow) = world
        .query::<(&Tonemapping, Has<Bloom>)>()
        .single(world)
        .unwrap();
    assert_eq!(*tone, Tonemapping::None);
    assert!(glow);
    match world
        .resource::<Assets<Mesh>>()
        .get(&mesh)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_COLOR)
    {
        Some(bevy::mesh::VertexAttributeValues::Float32x4(colours)) => {
            assert_eq!(*colours, vec![[1.0; 4]; 3]);
        }
        other => panic!("no vertex colours: {other:?}"),
    }
    // By day the lamp is out and not drawn, and no carried light is
    // placed: nothing is clustered or casts a shadow.
    let lamp = world
        .query::<(&PointLight, &Visibility)>()
        .iter(world)
        .filter(|_| true)
        .count();
    assert_eq!(lamp, 1);
    let (light, shown) = world
        .query_filtered::<(&PointLight, &Visibility), With<Lamp>>()
        .single(world)
        .unwrap();
    assert!(light.intensity.abs() < f32::EPSILON);
    assert_eq!(*shown, Visibility::Hidden);
    assert_eq!(world.query::<&Carried>().iter(world).count(), 0);
}

#[test]
fn night_falling_outdoors_leaves_the_materials_alone() {
    let (mut app, _, _) = zone(1);
    set(&mut app, true);
    app.update();
    app.update();
    let limits = |app: &mut App| {
        let world = app.world_mut();
        let handle = world
            .query::<&MeshMaterial3d<RichMaterial>>()
            .single(world)
            .unwrap()
            .0
            .clone();
        world
            .resource::<Assets<RichMaterial>>()
            .get(&handle)
            .unwrap()
            .extension
            .limits
    };
    let day = limits(&mut app);
    assert!((day.x - Tuning::default().light_cap).abs() < f32::EPSILON);
    assert!((day.y - 1.0).abs() < f32::EPSILON, "outdoors: {day}");
    // Night: the grading darkens the frame. The shader raises the ceiling
    // from the view's exposure; no material changes (swap writes them only
    // when their limits change), so none is prepared again: all of them at
    // once stalled the frame.
    let mut grading = bevy::render::view::ColorGrading::default();
    grading.global.exposure = -2.0;
    let world = app.world_mut();
    let camera = world
        .query_filtered::<Entity, With<OrbitCamera>>()
        .single(world)
        .unwrap();
    world.entity_mut(camera).insert(grading);
    app.update();
    app.update();
    assert_eq!(limits(&mut app), day);
}

#[test]
fn an_animated_surface_takes_each_frames_material_without_changing_any() {
    let (mut app, _, plain) = palace();
    let mut images = Assets::<Image>::default();
    let first = images.add(Image::default());
    let second = images.add(Image::default());
    let texture = |app: &mut App, image: &Handle<Image>| {
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .get_mut(&plain)
            .unwrap()
            .base_color_texture = Some(image.clone());
    };
    texture(&mut app, &first);
    let world = app.world_mut();
    let surface = world
        .query_filtered::<Entity, With<Mesh3d>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(surface)
        .insert(crate::gpu_assets::TerrainAnimation::testing(vec![
            first.clone(),
            second.clone(),
        ]));
    set(&mut app, true);
    app.update();
    app.update();
    let drawn = |app: &mut App| {
        let handle = app
            .world()
            .get::<MeshMaterial3d<RichMaterial>>(surface)
            .unwrap()
            .0
            .clone();
        let image = app
            .world()
            .resource::<Assets<RichMaterial>>()
            .get(&handle)
            .unwrap()
            .base
            .base_color_texture
            .clone();
        (handle, image)
    };
    let (on_first, image) = drawn(&mut app);
    assert_eq!(image, Some(first.clone()));
    // The next frame: a material of its own; the first frame's keeps its
    // image, so no material changed.
    texture(&mut app, &second);
    app.update();
    let (on_second, image) = drawn(&mut app);
    assert_ne!(on_second, on_first);
    assert_eq!(image, Some(second.clone()));
    let first_image = app
        .world()
        .resource::<Assets<RichMaterial>>()
        .get(&on_first)
        .unwrap()
        .base
        .base_color_texture
        .clone();
    assert_eq!(first_image, Some(first.clone()));
    // Round again: the frames' materials are reused, none is made.
    let made = app.world().resource::<Assets<RichMaterial>>().len();
    texture(&mut app, &first);
    app.update();
    assert_eq!(drawn(&mut app).0, on_first);
    texture(&mut app, &second);
    app.update();
    assert_eq!(drawn(&mut app).0, on_second);
    assert_eq!(app.world().resource::<Assets<RichMaterial>>().len(), made);
}

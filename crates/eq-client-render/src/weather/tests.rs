use super::*;
use crate::{online::testing, preview};
use eq_client_core::{
    WorldEvent, WorldPosition, WorldUpdate, ZoneOffer, ZoneRejection,
    clock::{Fog, ZoneSky},
    world::Link,
};
use std::time::Duration;

#[test]
fn density_ramps_up_crossfades_and_finishes_clearing() {
    let mut field = Field::default();
    let camera = Transform::IDENTITY;
    field.step(1, [0.2, 0.0], FADE_SECONDS);
    let light = field.geometry(&camera).1.indices.len();
    assert!(light > 0);
    field.step(1, [0.9, 0.0], FADE_SECONDS);
    let heavy = field.geometry(&camera).1.indices.len();
    assert!(heavy > light * 3);
    let rain_travel = field.rain.travel;

    field.step(1, [0.0, 0.7], FADE_SECONDS * 0.5);
    assert!(field.rain.density > 0.0 && field.snow.density > 0.0);
    assert_ne!(field.rain.travel, rain_travel);
    field.step(1, [0.0, 0.7], FADE_SECONDS);
    assert!(field.rain.density <= 0.0 && field.snow.density > 0.0);
    field.step(1, [0.0, 0.0], FADE_SECONDS);
    assert_eq!(field.geometry(&camera).1.indices, Vec::<u32>::new());
}

#[test]
fn new_admission_drops_the_old_layer_even_if_weather_arrives_immediately() {
    let mut field = Field::default();
    field.step(1, [1.0, 0.0], FADE_SECONDS);
    field.step(2, [0.0, 1.0], FADE_SECONDS * 0.5);
    assert!(field.rain.density <= 0.0 && field.snow.density > 0.0);
}

#[test]
fn camera_relative_geometry_has_a_fixed_budget_and_valid_vertices() {
    let mut field = Field::default();
    field.step(1, [1.0, 1.0], 30.0);
    let camera = Transform::from_xyz(-13_000.0, 7.0, 8_000.0)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    let (_, quads) = field.geometry(&camera);
    let limit = usize::from(RAIN.limit + SNOW.limit);
    assert_eq!(quads.positions.len(), limit * 4);
    assert_eq!(quads.colors.len(), limit * 4);
    assert_eq!(quads.indices.len(), limit * 6);
    assert!(quads.indices.iter().all(|&i| (i as usize) < limit * 4));
    let max = VOLUME * 0.5 + Vec3::splat(2.0);
    assert!(
        quads
            .positions
            .iter()
            .all(|&position| Vec3::from_array(position).abs().cmple(max).all())
    );
    assert!(
        quads
            .colors
            .iter()
            .all(|color| (0.0..=1.0).contains(&color[3]))
    );
}

fn news(app: &mut App, event: WorldEvent) {
    testing::news(&mut app.world_mut().resource_mut::<OnlineState>(), [event]);
}

fn sky(app: &mut App, sky: u8) {
    news(
        app,
        WorldEvent::Sky(ZoneSky {
            sky,
            time_type: 1,
            fog: [Fog {
                color: [0; 3],
                near: 0.0,
                far: 0.0,
            }; 4],
        }),
    );
}

fn meshes(app: &mut App) -> Vec<Handle<Mesh>> {
    let world = app.world_mut();
    world
        .query::<&Mesh3d>()
        .iter(world)
        .map(|mesh| mesh.0.clone())
        .collect()
}

fn weather_app() -> App {
    let mut app = crate::testing::app();
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .insert_resource(SceneInfo {
            zone_name: "example".into(),
        });
    app.world_mut().spawn((
        Camera::default(),
        Transform::IDENTITY,
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 50.0,
            yaw: 0.0,
            pitch: 0.0,
            pulled: None,
            view: default(),
        },
    ));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(FADE_SECONDS));
    news(
        &mut app,
        WorldEvent::Entered {
            capabilities: Vec::new(),
            choices: Vec::new(),
            session_id: 1,
            zone: "example".into(),
            player: Box::new(preview::player(default(), [None; 8])),
            far_clip: None,
            spell_landing_flag: None,
            zone_id: 0,
        },
    );
    testing::apply(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        &WorldUpdate::Connection(Link::Connected),
    );
    app
}

#[test]
fn weather_mesh_follows_sky_scene_option_and_server_lifetime() {
    let mut app = weather_app();
    app.add_systems(Update, update);
    let storm = Weather::Rain {
        intensity: u32::MAX,
    };
    news(&mut app, WorldEvent::Weather(storm));
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());
    sky(&mut app, 0);
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());

    sky(&mut app, 1);
    app.world_mut()
        .resource_mut::<SceneInfo>()
        .zone_name
        .clear();
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());
    app.world_mut().resource_mut::<SceneInfo>().zone_name = "example".into();
    app.update();
    let active = meshes(&mut app);
    assert_eq!(active.len(), 1);
    let active_mesh = app
        .world()
        .resource::<Assets<Mesh>>()
        .get(&active[0])
        .unwrap();
    assert_eq!(active_mesh.count_vertices(), usize::from(RAIN.limit) * 4);
    app.update();
    assert_eq!(meshes(&mut app), active);

    let clearly = |app: &mut App, enabled| {
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .qol
            .set(Fix::ClearView, enabled);
    };
    clearly(&mut app, true);
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());
    assert_eq!(
        app.world().resource::<OnlineState>().world().weather(),
        Some(storm)
    );
    clearly(&mut app, false);
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    news(
        &mut app,
        WorldEvent::Weather(Weather::Unknown {
            kind: 5,
            intensity: 9,
        }),
    );
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());

    news(&mut app, WorldEvent::Weather(storm));
    app.update();
    assert_eq!(meshes(&mut app).len(), 1);
    testing::apply(
        &mut app.world_mut().resource_mut::<OnlineState>(),
        &WorldUpdate::Connection(Link::Ended),
    );
    app.update();
    assert_eq!(meshes(&mut app), Vec::<Handle<Mesh>>::new());
}

#[test]
fn rejected_transfer_in_one_receive_batch_discards_the_old_weather_layer() {
    use crate::online::{Updates, receive};
    use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
    use std::sync::{Mutex, mpsc};

    let mut app = weather_app();
    let (sender, receiver) = mpsc::channel();
    app.insert_resource(Updates(Mutex::new(Some(receiver))))
        .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
        .add_systems(Update, (receive, update).chain());
    sky(&mut app, 1);
    news(
        &mut app,
        WorldEvent::Weather(Weather::Rain { intensity: 10 }),
    );
    app.update();
    assert!(app.world().resource::<Field>().rain.density > 0.0);

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(FADE_SECONDS * 0.5));
    let snow = Weather::Snow { intensity: 10 };
    for event in [
        WorldEvent::ZoneTransfer(ZoneOffer {
            zone_id: 2,
            instance_id: 0,
            position: WorldPosition::default(),
            reason: 0,
            to_bind: false,
            solicited: true,
        }),
        WorldEvent::ZoneTransferRejected {
            session_id: 1,
            reason: ZoneRejection::Cancelled,
        },
        WorldEvent::Weather(snow),
    ] {
        sender.send(WorldUpdate::Game(event)).unwrap();
    }
    app.update();

    let world = app.world().resource::<OnlineState>().world();
    assert_eq!(world.session_id(), Some(1));
    assert_eq!(world.weather(), Some(snow));
    let field = app.world().resource::<Field>();
    assert!(field.rain.density <= 0.0);
    assert!(field.snow.density > 0.0);
}

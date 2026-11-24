//! Opt-in capture of a zone without a sky, drawn as the engine draws it or
//! with Rich Lighting, from a fixed eye: for looking at the two side by
//! side. Needs an installation and a graphics adapter; no session.
use super::*;
use crate::online::testing as session;
use bevy::{
    app::AppExit,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use eq_client_core::{
    WorldEvent, WorldPosition,
    clock::{Fog, ZoneSky},
    render_position,
};

fn point(name: &str) -> Option<Vec3> {
    let values: Vec<f32> = std::env::var(name)
        .ok()?
        .split(',')
        .map(|number| number.trim().parse().unwrap())
        .collect();
    Some(Vec3::from_array(render_position(WorldPosition {
        x: values[0],
        y: values[1],
        z: values[2],
        heading: 0.0,
    })))
}

/// The look, from `EQ_PROBE_TUNE`: `name=value` pairs, comma separated.
fn tuning() -> Tuning {
    let mut tuning = Tuning::default();
    for pair in std::env::var("EQ_PROBE_TUNE")
        .unwrap_or_default()
        .split(',')
    {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let number = || value.parse::<f32>().unwrap();
        match name.trim() {
            "fill" => tuning.fill = number(),
            "wash" => tuning.wash = number(),
            "span" => tuning.wash_span = number(),
            "lamp" => tuning.lamp = number(),
            "carried" => tuning.carried = number(),
            "outdoor" => tuning.outdoor = number(),
            "whiten" => tuning.whiten = number(),
            "reach" => tuning.reach = number(),
            "drawn" => tuning.lamps_drawn = value.parse().unwrap(),
            "shadowed" => tuning.shadowed_lamps = value.parse().unwrap(),
            "near" => tuning.lamp_shadow_near = number(),
            "roughness" => tuning.roughness = number(),
            "reflectance" => tuning.reflectance = number(),
            "glow" => tuning.glow = number(),
            "cap" => tuning.light_cap = number(),
            "shadow" => tuning.shadow_strength = number(),
            "tone" => {
                tuning.tone = match value {
                    "none" => Tonemapping::None,
                    "reinhard" => Tonemapping::ReinhardLuminance,
                    "boring" => Tonemapping::SomewhatBoringDisplayTransform,
                    "blender" => Tonemapping::BlenderFilmic,
                    _ => Tonemapping::AcesFitted,
                }
            }
            other => panic!("unknown tuning {other}"),
        }
    }
    tuning
}

#[test]
#[ignore = "requires EQ_PROBE_INSTALL, EQ_PROBE_ZONE, EQ_PROBE_EYE, EQ_PROBE_LOOK, EQ_PROBE_PNG and a graphics adapter"]
#[allow(clippy::too_many_lines)] // One capture, set up whole.
fn capture() {
    let install = std::path::PathBuf::from(std::env::var("EQ_PROBE_INSTALL").unwrap());
    let name = std::env::var("EQ_PROBE_ZONE").unwrap();
    let output = std::path::PathBuf::from(std::env::var("EQ_PROBE_PNG").unwrap());
    let eye = point("EQ_PROBE_EYE").unwrap();
    let look = point("EQ_PROBE_LOOK").unwrap();
    let feet = point("EQ_PROBE_FEET").unwrap_or(look);
    let rich = std::env::var("EQ_PROBE_RICH").is_ok_and(|value| value == "1");
    let light: u8 = std::env::var("EQ_PROBE_LIGHT").map_or(255, |light| light.parse().unwrap());
    let frames: u32 = std::env::var("EQ_PROBE_FRAMES").map_or(40, |frames| frames.parse().unwrap());
    let seconds: f32 =
        std::env::var("EQ_PROBE_SECONDS").map_or(8.0, |seconds| seconds.parse().unwrap());
    // A sky and an hour for an outdoor zone; without, a no-sky zone.
    let sky: u8 = std::env::var("EQ_PROBE_SKY").map_or(0, |sky| sky.parse().unwrap());
    let hour: Option<u8> = std::env::var("EQ_PROBE_HOUR")
        .ok()
        .map(|hour| hour.parse().unwrap());
    let size: (u32, u32) = std::env::var("EQ_PROBE_SIZE").map_or((1280, 720), |size| {
        let (width, height) = size.split_once('x').unwrap();
        (width.parse().unwrap(), height.parse().unwrap())
    });

    let mut online = OnlineState::new(true);
    let mut own = session::player(7);
    own.appearance.light = light;
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
    if let Some(hour) = hour {
        session::news(
            &mut online,
            [WorldEvent::TimeOfDay(eq_client_core::clock::GameTime {
                hour,
                minute: 0,
                day: 3,
                month: 2,
                year: 3200,
            })],
        );
    }
    let world_zone = online.world().zone().to_owned();

    let mut app = crate::testing::hidden_graphics_probe("Rich lighting probe", size);
    register(&mut app);
    app.insert_resource(tuning());
    let mut options = OptionsState::default();
    options
        .options
        .qol
        .set(eq_client_core::qol::Fix::RichLighting, rich);
    app.insert_resource(online)
        .insert_resource(options)
        .insert_resource(crate::ViewerSettings(crate::ViewerConfig::default()))
        .init_resource::<NearbyEntities>()
        .insert_resource(ClearColor(Color::BLACK));
    let zone = eq_client_assets::load_zone(&install, &name).unwrap();
    if std::env::var("EQ_PROBE_DUMP").is_ok() {
        for light in &zone.point_lights {
            let at = eq_client_core::world_position(light.position, 0.0);
            println!(
                "light x={:.0} y={:.0} z={:.0} r={:.0} rgb={:?} level={}",
                at.x, at.y, at.z, light.radius, light.rgb, light.level
            );
        }
    }
    let zone = std::sync::Mutex::new(Some(zone));
    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut images: ResMut<Assets<Image>>,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>| {
            let zone = zone.lock().unwrap().take().unwrap();
            crate::spawn_static_zone(
                &mut commands,
                zone,
                false,
                &mut images,
                &mut meshes,
                &mut materials,
            );
            commands.spawn((
                Player,
                Transform::from_translation(feet),
                PlayerBody {
                    feet_offset: 0.0,
                    height: 6.0,
                    size: 6.0,
                },
            ));
            commands.spawn((
                Camera3d::default(),
                Tonemapping::None,
                Projection::Perspective(PerspectiveProjection {
                    near: 1.0,
                    ..default()
                }),
                OrbitCamera {
                    focus: look,
                    radius: eye.distance(look),
                    yaw: 0.0,
                    pitch: 0.0,
                    pulled: None,
                    view: default(),
                    look: 0.0,
                },
                Transform::from_translation(eye).looking_at(look, Vec3::Y),
            ));
        },
    );
    app.add_systems(
        Update,
        (
            move |mut scene: Option<ResMut<SceneLights>>| {
                if let Some(scene) = scene.as_mut()
                    && scene.zone != world_zone
                {
                    scene.zone.clone_from(&world_zone);
                }
            },
            crate::daylight::update,
            lighting::update,
            gather,
            swap,
            follow_frames,
            lamps,
            grade,
        )
            .chain(),
    );
    app.add_systems(
        Update,
        move |mut frame: Local<u32>,
              mut since: Local<Option<std::time::Instant>>,
              mut taken: Local<bool>,
              mut commands: Commands| {
            // At least so many frames and seconds, so the textures are up.
            *frame += 1;
            let since = since.get_or_insert_with(std::time::Instant::now);
            if !*taken && *frame >= frames && since.elapsed().as_secs_f32() >= seconds {
                *taken = true;
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(output.clone()))
                    .observe(
                        |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                            exit.write(AppExit::Success);
                        },
                    );
            }
        },
    );
    assert_eq!(app.run(), AppExit::Success);
}

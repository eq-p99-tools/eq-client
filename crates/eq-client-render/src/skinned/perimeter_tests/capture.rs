//! Opt-in GPU capture through the production Bevy UI/Screenshot path.
//! Generated SIDL and atlas only; no session, installed art or desktop input.
//! Uses the hidden-window route exercised by `map::preview::capture`.
use super::{SHEET, bounds, fixture};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::WindowResolution,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Component)]
struct Case(&'static str);

fn frame(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &eq_client_assets::sidl::Border,
    name: &'static str,
    [x, y, width, height]: [f32; 4],
) {
    parent
        .spawn((Case(name), super::super::at(x, y, width, height)))
        .with_children(|root| {
            super::super::border(root, art, look, (width, height));
        });
}

fn install_cases(app: &mut App) {
    let (look, pixels) = fixture();
    crate::sheets::testing::sheet(app, SHEET, pixels);
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut art: crate::sheets::Art| {
            commands.spawn(Camera2d);
            commands
                .spawn(Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                })
                .with_children(|root| {
                    frame(root, &mut art, &look, "normal", [18.0, 20.0, 40.0, 32.0]);
                    frame(root, &mut art, &look, "minimum", [96.0, 20.0, 17.0, 18.0]);
                    frame(root, &mut art, &look, "tiny", [156.0, 20.0, 9.0, 8.0]);
                    frame(root, &mut art, &look, "empty", [200.0, 20.0, 0.0, 32.0]);
                    let mut zero = look.clone();
                    let cap = zero.left_top.as_mut().unwrap();
                    cap.width = 0;
                    cap.height = 0;
                    frame(root, &mut art, &zero, "zero-cap", [18.0, 88.0, 40.0, 32.0]);
                    let mut absent = look.clone();
                    absent.left_top = None;
                    frame(
                        root,
                        &mut art,
                        &absent,
                        "absent-cap",
                        [96.0, 88.0, 40.0, 32.0],
                    );
                    root.spawn(Node {
                        overflow: Overflow::clip(),
                        ..super::super::at(156.0, 88.0, 52.0, 36.0)
                    })
                    .with_children(|clipped| {
                        frame(
                            clipped,
                            &mut art,
                            &look,
                            "nested-clipped",
                            [30.0, 8.0, 40.0, 32.0],
                        );
                    });
                });
        },
    );
}

fn rect(rect: Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
}

fn metadata(world: &mut World, requested_scale: f32) -> serde_json::Value {
    let cases: BTreeMap<Entity, &'static str> = world
        .query::<(Entity, &Case)>()
        .iter(world)
        .map(|(entity, case)| (entity, case.0))
        .collect();
    let roots: Vec<_> = cases
        .iter()
        .map(|(entity, name)| {
            let computed = world.get::<ComputedNode>(*entity).unwrap();
            assert!((computed.inverse_scale_factor().recip() - requested_scale).abs() < 0.0001);
            serde_json::json!({
                "case": name, "bounds": rect(bounds(world, *entity)),
                "effective_scale": computed.inverse_scale_factor().recip(),
            })
        })
        .collect();
    let pictures: Vec<_> = world
        .query::<(Entity, &ImageNode, Option<&CalculatedClip>)>()
        .iter(world)
        .map(|(entity, image, clip)| {
            let mut ancestor = entity;
            let name = loop {
                if let Some(name) = cases.get(&ancestor) {
                    break *name;
                }
                ancestor = world.get::<ChildOf>(ancestor).unwrap().parent();
            };
            let clipped = clip
                .expect("all perimeter pictures must retain their clip")
                .clip;
            serde_json::json!({
                "case": name, "bounds": rect(bounds(world, entity)),
                "clip": rect(clipped), "source_rect": rect(image.rect.unwrap()),
            })
        })
        .collect();
    assert_eq!(roots.len(), 7);
    assert!(pictures.iter().any(|p| p["case"] == "normal"));
    assert!(!pictures.iter().any(|p| p["case"] == "empty"));
    serde_json::json!({
        "evidence": "production Bevy layout/image GPU capture; synthetic inputs; no native comparison",
        "requested_scale": requested_scale, "ui_scale": world.resource::<UiScale>().0,
        "physical_viewport": [640,480],
        "bounds_coordinates": "top-left physical UI target coordinates, identical to PNG coordinates; no camera-center conversion",
        "background_srgb": [32,64,96], "roots": roots, "pictures": pictures,
    })
}

#[test]
#[ignore = "requires EQ_SKIN_PREVIEW_PNG, EQ_SKIN_PREVIEW_SCALE and a graphics adapter; hidden offline window"]
fn perimeter_capture() {
    let output = PathBuf::from(std::env::var("EQ_SKIN_PREVIEW_PNG").unwrap());
    let scale: f32 = std::env::var("EQ_SKIN_PREVIEW_SCALE")
        .unwrap()
        .parse()
        .unwrap();
    assert!([1.0, 1.25, 1.5, 2.25].contains(&scale));
    assert!(output.is_absolute() && !output.exists());
    assert!(!output.with_extension("layout.json").exists());
    assert!(!output.with_extension("checks.json").exists());
    assert!(output.parent().unwrap().is_dir());
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Offline synthetic skin probe".into(),
                    resolution: WindowResolution::new(640, 480).with_scale_factor_override(1.0),
                    visible: false,
                    focused: false,
                    ..default()
                }),
                ..default()
            })
            .set(bevy::winit::WinitPlugin {
                run_on_any_thread: true,
            }),
    )
    .insert_resource(bevy::winit::WinitSettings::continuous())
    .insert_resource(ClearColor(Color::srgb_u8(32, 64, 96)))
    .insert_resource(UiScale(scale))
    .insert_resource(crate::ViewerSettings(crate::ViewerConfig {
        eq_directory: Some("synthetic-no-files".into()),
        ..default()
    }))
    .init_resource::<crate::sheets::Sheets>()
    .init_resource::<crate::skin::UiSkin>();
    super::super::register_border_pixels(&mut app);
    install_cases(&mut app);
    let path = output.clone();
    let mut frames = 0_u32;
    app.add_systems(Update, move |world: &mut World| {
        frames += 1;
        if frames == 60 {
            let state = metadata(world, scale);
            std::fs::write(
                path.with_extension("layout.json"),
                serde_json::to_vec_pretty(&state).unwrap(),
            )
            .unwrap();
            world
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()))
                .observe(
                    |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                        exit.write(AppExit::Success);
                    },
                );
        }
    });
    assert_eq!(app.run(), AppExit::Success);
    assert!(output.is_file() && output.with_extension("layout.json").is_file());
    let checks = super::capture_pixels::verify(&output, scale);
    std::fs::write(
        output.with_extension("checks.json"),
        serde_json::to_vec_pretty(&checks).unwrap(),
    )
    .unwrap();
}

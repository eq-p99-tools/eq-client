//! Opt-in offline rendering probe: synthetic entities only, no assets or connection.
use super::*;
use crate::{
    online::testing as session,
    windows::{Shown, WindowId},
};
use bevy::{
    app::AppExit,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use eq_client_assets::maps::{MapLabel, MapLine};
use eq_client_core::SpawnKind;

#[test]
#[ignore = "requires EQ_MAP_PREVIEW_PNG and a graphics adapter; opens only a hidden offline window"]
fn capture() {
    let path = std::path::PathBuf::from(std::env::var("EQ_MAP_PREVIEW_PNG").unwrap());
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Offline map probe".into(),
                    resolution: (900, 700).into(),
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
    .insert_resource(ClearColor(crate::theme::COVER))
    .init_resource::<crate::ViewerSettings>()
    .init_resource::<crate::keys::Typing>()
    .init_resource::<WatchState>()
    .init_resource::<MapMotion>()
    .init_resource::<Presentation>()
    .init_resource::<Shown>();
    app.world_mut().resource_mut::<Shown>().open(WindowId::Map);
    install_scene(&mut app);
    let geometry = std::env::var("EQ_MAP_PREVIEW_MODE").unwrap_or_default() != "no-lines";
    let mut view = MapView {
        admission: Some(1),
        zone: Some(("synthetic".into(), geometry.then(geometry_map))),
        bounds: map_bounds(&geometry_map()),
        ..default()
    };
    if std::env::var("EQ_MAP_PREVIEW_MODE").unwrap_or_default() == "search" {
        view.query = "guard".into();
    }
    let mut watches = WatchState::default();
    watches.add("Watchful traveler");
    watches.add("Rare guardian");
    app.insert_resource(watches).insert_resource(view);
    app.world_mut().spawn(Camera2d);
    app.world_mut().spawn((
        crate::theme::text(
            "OFFLINE MAP PREVIEW  /  synthetic entities",
            crate::theme::Size::Heading,
            crate::theme::INK,
        ),
        Node {
            position_type: PositionType::Absolute,
            left: px(100),
            top: px(30),
            ..default()
        },
    ));
    app.add_systems(
        Update,
        (
            sample_movement,
            ensure_window,
            draw,
            entities::update,
            update_items,
            draw_movement,
            controls::update,
            show_target,
            present,
        )
            .chain(),
    );
    let output = path.clone();
    app.add_systems(
        Update,
        move |mut frames: Local<u32>,
              mut commands: Commands,
              mut view: ResMut<MapView>,
              online: Res<OnlineState>,
              mut players: Query<&mut Transform, With<crate::Player>>| {
            *frames += 1;
            // Synthetic local strafe, retaining the initial facing.
            if let Ok(mut transform) = players.single_mut() {
                transform.translation.z -= 0.3;
            }
            if *frames == 10 {
                // Pin a synthetic NPC through the same overlap-aware selection.
                entities::select_preview(&mut view, online.world(), 5);
            }
            if *frames == 60 {
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
    assert!(path.is_file(), "the screenshot must actually be saved");
}

fn install_scene(app: &mut App) {
    let online = snapshot();
    app.insert_resource(crate::SceneInfo {
        zone_name: online.world().zone().into(),
    });
    app.world_mut().spawn((
        crate::Player,
        crate::zone::placement(online.world().player().unwrap().position),
    ));
    app.insert_resource(online);
}

fn snapshot() -> OnlineState {
    let mut online = OnlineState::new(false);
    let mut player = session::player(1);
    player.level = 20;
    player.position.y = -100.0;
    session::admit(&mut online, 1, player);
    let spawns = [
        (2, 5, "A forest guard001", -300.0, 0.0),
        (3, 18, "A gate guard002", -160.0, 0.0),
        (4, 20, "A watch guard003", 0.0, 0.0),
        (5, 22, "A veteran guard004", 160.0, 0.0),
        (6, 24, "A captain guard005", 300.0, 0.0),
        (7, 30, "Watchful traveler", -140.0, -160.0),
        (8, 12, "Another traveler", 140.0, -160.0),
        (9, 35, "Rare_guardian001", 270.0, 140.0),
        (10, 8, "A_boar001", -250.0, 130.0),
    ]
    .into_iter()
    .map(|(id, level, name, x, y)| {
        let mut spawn = session::pet(id, 1);
        spawn.name = name.into();
        spawn.level = level;
        spawn.hp_percent = Some(82);
        spawn.pet_owner = None;
        spawn.position.x = x;
        spawn.position.y = y;
        if id == 7 || id == 8 {
            spawn.kind = SpawnKind::Player;
            spawn.position.heading = if id == 7 { 128.0 } else { 256.0 };
        }
        if id == 3 || id == 7 {
            spawn.velocity = [4.0, 0.0, 0.0];
        }
        if id == 9 {
            spawn.forward = 0.5;
        }
        spawn
    })
    .collect();
    session::spawns(&mut online, spawns);
    session::news(
        &mut online,
        [eq_client_core::WorldEvent::Objects(
            eq_client_core::ground::ObjectUpdate::Spawn(eq_client_core::ground::GroundObject {
                drop_id: 70_000,
                model: "IT63_ACTORDEF".into(),
                position: eq_client_core::WorldPosition {
                    x: 100.0,
                    y: 140.0,
                    ..default()
                },
                object_type: 0,
            }),
        )],
    );
    online
}

fn geometry_map() -> ZoneMap {
    ZoneMap {
        lines: [
            ([-400.0, -300.0, 0.0], [400.0, -300.0, 0.0]),
            ([400.0, -300.0, 0.0], [400.0, 300.0, 0.0]),
            ([400.0, 300.0, 0.0], [-400.0, 300.0, 0.0]),
            ([-400.0, 300.0, 0.0], [-400.0, -300.0, 0.0]),
            ([-400.0, 70.0, 0.0], [400.0, 70.0, 0.0]),
        ]
        .into_iter()
        .map(|(from, to)| MapLine {
            from,
            to,
            color: [60, 65, 70],
            layer: 0,
        })
        .collect(),
        labels: vec![MapLabel {
            at: [0.0, -220.0, 0.0],
            text: "Synthetic courtyard".into(),
            color: [50, 50, 50],
            layer: 0,
            size: 1,
        }],
    }
}

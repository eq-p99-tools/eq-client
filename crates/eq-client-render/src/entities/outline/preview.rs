//! Synthetic graphics probe for the same mask layer and compositor as gameplay.
use super::*;
use crate::online::testing as session;
use bevy::{
    app::AppExit,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use eq_client_core::maps::Watches;

#[test]
#[ignore = "requires EQ_OUTLINE_PREVIEW_PNG and a graphics adapter; hidden offline window"]
fn capture() {
    let path = std::path::PathBuf::from(std::env::var("EQ_OUTLINE_PREVIEW_PNG").unwrap());
    assert!(!path.exists(), "use a fresh screenshot path");
    let mut app =
        crate::testing::hidden_graphics_probe("Offline tracked outline probe", (800, 500));
    app.insert_resource(ClearColor(Color::srgb(0.055, 0.07, 0.085)))
        .add_systems(Startup, scene)
        .add_systems(Update, (sync_layers, composite).chain());
    install_shader(&mut app);
    let output = path.clone();
    app.add_systems(
        Update,
        move |mut frames: Local<u32>, mut commands: Commands| {
            *frames += 1;
            if *frames == 90 {
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

fn scene(
    mut commands: Commands,
    mut mask: ResMut<Mask>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 10.0,
            yaw: 0.0,
            pitch: 0.0,
            pulled: None,
            view: default(),
        },
    ));
    let wall = materials.add(StandardMaterial {
        base_color: Color::srgb(0.21, 0.24, 0.27),
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.7, 3.9, 0.4))),
        MeshMaterial3d(wall),
        Transform::from_xyz(1.8, 0.0, 1.3),
    ));
    let body = meshes.add(Capsule3d::new(0.35, 1.3));
    let arm = meshes.add(Sphere::new(0.22));
    let figure = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.57, 0.57),
        unlit: true,
        ..default()
    });
    let mut nearby = NearbyEntities::default();
    let mut figures = BTreeMap::new();
    let mut online = OnlineState::new(false);
    session::admit(&mut online, 1, session::player(1));
    for (id, name, x) in [
        (2, "A_sentinel001", -2.7),
        (3, "A_sentinel002", 0.9),
        (4, "A_bystander001", 2.7),
    ] {
        let mut spawn = session::pet(id, 1);
        spawn.name = name.into();
        session::spawns(&mut online, vec![spawn]);
        let root = commands.spawn_empty().id();
        let parts = [
            commands
                .spawn((
                    Mesh3d(body.clone()),
                    MeshMaterial3d(figure.clone()),
                    Transform::from_xyz(x, 0.0, 0.0),
                ))
                .id(),
            commands
                .spawn((
                    Mesh3d(arm.clone()),
                    MeshMaterial3d(figure.clone()),
                    Transform::from_xyz(x + 0.38, 0.3, 0.0),
                ))
                .id(),
        ];
        figures.insert(root, parts);
        nearby.rendered.insert(id, root);
        nearby
            .revisions
            .insert(id, online.world().spawn(id).unwrap().revision);
    }
    let mut watches = Watches::default();
    if std::env::var_os("EQ_OUTLINE_PREVIEW_DISABLED").is_none() {
        watches.add("a sentinel");
    }
    mask.selected.extend(
        remote_roots(online.world(), &nearby, |name| watches.contains(name))
            .flat_map(|root| figures[&root]),
    );
}

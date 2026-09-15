#![doc = "Bevy scene and camera support for renderer-independent EQ zone assets."]

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use eq_client_assets::ZoneAsset;

/// The projection used by the top-down camera.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectionStyle {
    /// A RuneScape-like perspective view.
    #[default]
    Perspective,
    /// A fixed-scale Diablo-like view.
    Orthographic,
}

/// Settings for an offline viewer window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ViewerConfig {
    /// Initial camera projection.
    pub projection: ProjectionStyle,
}

#[derive(Resource)]
struct PendingZone(Option<ZoneAsset>);

#[derive(Resource)]
struct ViewerSettings(ViewerConfig);

#[derive(Component)]
struct OrbitCamera {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

/// Opens a window and renders a loaded zone until the user closes it.
pub fn run(zone: ZoneAsset, config: ViewerConfig) {
    App::new()
        .insert_resource(PendingZone(Some(zone)))
        .insert_resource(ViewerSettings(config))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "eq-client offline viewer".to_owned(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_scene)
        .add_systems(Update, orbit_camera)
        .run();
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn setup_scene(
    mut commands: Commands,
    mut pending: ResMut<PendingZone>,
    settings: Res<ViewerSettings>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let zone = pending
        .0
        .take()
        .expect("startup only consumes the zone once");
    let (bounds_min, bounds_max) = zone.bounds().unwrap_or(([-100.0; 3], [100.0; 3]));
    let min = Vec3::from_array(bounds_min);
    let max = Vec3::from_array(bounds_max);
    let focus = (min + max) * 0.5;
    let radius = (max - min).length().max(100.0) * 0.35;

    let fallback_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.52, 0.34),
        perceptual_roughness: 0.95,
        cull_mode: None,
        ..default()
    });
    let zone_materials = create_materials(zone.textures, &mut images, &mut materials);

    for primitive in zone.primitives {
        if primitive.indices.is_empty() || primitive.positions.is_empty() {
            continue;
        }
        let material = primitive
            .texture
            .and_then(|index| zone_materials.get(index).cloned())
            .unwrap_or_else(|| fallback_material.clone());
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, primitive.positions);
        if !primitive.normals.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, primitive.normals);
        }
        if !primitive.texture_coordinates.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, primitive.texture_coordinates);
        }
        mesh.insert_indices(Indices::U32(primitive.indices));
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.8, 0.0)),
    ));
    commands.spawn(AmbientLight {
        color: Color::srgb(0.62, 0.68, 0.8),
        brightness: 150.0,
        affects_lightmapped_meshes: true,
    });

    let orbit = OrbitCamera {
        focus,
        radius,
        yaw: 0.75,
        pitch: -0.8,
    };
    let projection = match settings.0.projection {
        ProjectionStyle::Perspective => Projection::Perspective(PerspectiveProjection::default()),
        ProjectionStyle::Orthographic => {
            let mut projection = OrthographicProjection::default_3d();
            projection.scale = radius / 400.0;
            Projection::Orthographic(projection)
        }
    };
    commands.spawn((
        Camera3d::default(),
        projection,
        orbit_transform(&orbit),
        orbit,
    ));
}

fn create_materials(
    textures: Vec<eq_client_assets::ZoneTexture>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<Handle<StandardMaterial>> {
    textures
        .into_iter()
        .map(|texture| {
            let image = images.add(Image::new(
                Extent3d {
                    width: texture.width,
                    height: texture.height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                texture.rgba8,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
            materials.add(StandardMaterial {
                base_color_texture: Some(image),
                perceptual_roughness: 0.95,
                cull_mode: None,
                ..default()
            })
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn orbit_camera(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut cameras: Query<(&mut OrbitCamera, &mut Transform)>,
) {
    let drag = if mouse_buttons.pressed(MouseButton::Right) {
        motion.read().map(|event| event.delta).sum::<Vec2>()
    } else {
        motion.clear();
        Vec2::ZERO
    };
    let scroll = wheel.read().map(|event| event.y).sum::<f32>();

    for (mut camera, mut transform) in &mut cameras {
        camera.yaw -= drag.x * 0.005;
        camera.pitch = (camera.pitch - drag.y * 0.005).clamp(-1.45, -0.15);
        camera.radius = (camera.radius * (-scroll * 0.12).exp()).clamp(20.0, 20_000.0);
        *transform = orbit_transform(&camera);
    }
}

fn orbit_transform(camera: &OrbitCamera) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    let position = camera.focus + rotation * Vec3::new(0.0, 0.0, camera.radius);
    Transform::from_translation(position).looking_at(camera.focus, Vec3::Y)
}

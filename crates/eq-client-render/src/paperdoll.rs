//! The character in the inventory's paperdoll: an idle copy of the player's model
//! on its own render layer, filmed into an image the inventory window shows. The
//! camera only renders while that window is open.

use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
};

use super::{Player, character};

/// The render layer only the paperdoll camera draws.
const LAYER: usize = 1;
/// Rendered size in pixels: about twice the default skin's character area, so
/// the image stays sharp when the window scales it down.
const SIZE: UVec2 = UVec2::new(176, 348);

/// The image the paperdoll camera renders into, and its size in pixels.
#[derive(Resource)]
pub(super) struct PaperdollImage {
    pub handle: Handle<Image>,
    pub size: Vec2,
}

/// Adds the paperdoll's camera, and keeps its figure and activity current.
pub(super) fn register(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(Update, (sync, toggle));
}

#[derive(Component)]
struct PaperdollCamera;

/// The root of the model copy the camera films.
#[derive(Component)]
struct Figure;

/// Creates the image, a camera that films only the paperdoll layer, and its light.
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None);
    let handle = images.add(image);
    commands.spawn((
        PaperdollCamera,
        Camera3d::default(),
        Camera {
            order: -1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        RenderLayers::layer(LAYER),
        frame(6.0),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            ..default()
        },
        RenderLayers::layer(LAYER),
        Transform::from_xyz(3.0, 8.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(PaperdollImage {
        handle,
        size: SIZE.as_vec2(),
    });
}

/// Looks at a figure of this height, standing on the origin and facing +Z, so it
/// fills the image's height with a little margin.
fn frame(height: f32) -> Transform {
    let half_fov = std::f32::consts::FRAC_PI_8;
    let distance = height * 0.55 / half_fov.tan();
    let center = Vec3::Y * height * 0.5;
    Transform::from_translation(center + Vec3::Z * distance).looking_at(center, Vec3::Y)
}

/// Replaces the figure whenever the player's model changes, as at each zone-in.
#[allow(clippy::needless_pass_by_value)]
fn sync(
    mut commands: Commands,
    players: Query<&character::Model, (With<Player>, Changed<character::Model>)>,
    figures: Query<Entity, With<Figure>>,
    mut cameras: Query<&mut Transform, With<PaperdollCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(model) = players.single() else {
        return;
    };
    for figure in &figures {
        commands.entity(figure).despawn();
    }
    let root = commands
        .spawn((Figure, Transform::IDENTITY, Visibility::Visible))
        .id();
    character::spawn_on_layers(
        &mut commands,
        root,
        &model.0,
        0.0,
        &mut meshes,
        &RenderLayers::layer(LAYER),
    );
    for mut camera in &mut cameras {
        *camera = frame(model.0.height());
    }
}

/// Renders the figure only while someone can see it.
#[allow(clippy::needless_pass_by_value)]
fn toggle(
    shown: Res<super::windows::Shown>,
    mut cameras: Query<&mut Camera, With<PaperdollCamera>>,
) {
    let open = shown.is_open(super::windows::WindowId::Inventory);
    for mut camera in &mut cameras {
        if camera.is_active != open {
            camera.is_active = open;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_camera_frames_the_whole_figure() {
        let camera = super::frame(6.0);
        let (center, top) = (bevy::math::Vec3::Y * 3.0, bevy::math::Vec3::Y * 6.0);
        let to_center = (center - camera.translation).normalize();
        let to_top = (top - camera.translation).normalize();
        // The head sits inside the 22.5-degree half field of view, near its edge.
        let angle = to_center.angle_between(to_top);
        assert!(angle < std::f32::consts::FRAC_PI_8, "{angle}");
        assert!(angle > std::f32::consts::FRAC_PI_8 * 0.8, "{angle}");
    }
}

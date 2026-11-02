//! Render the widget into its native workbench slot, retaining normal clipping,
//! stacking, resizing and minimization of every other Bevy window.
use super::{CodePass, EditorCamera, State, Surface};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat},
};
use bevy_egui::{EguiContext, EguiContextSettings, EguiSchedule};

pub(super) fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    surfaces: Query<Entity, With<Surface>>,
    mut state: ResMut<State>,
) {
    let Ok(surface) = surfaces.single() else {
        return;
    };
    let image = images.add(Image::new_target_texture(
        800,
        280,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    state.image = image.clone();
    commands
        .entity(surface)
        .insert(ImageNode::new(image.clone()));
    commands.spawn((
        EditorCamera,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: -10,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(image.into()),
        EguiContext::default(),
        EguiSchedule::new(CodePass),
        EguiContextSettings {
            // The host maps UI coordinates and preserves modifier order.
            input_system_settings: bevy_egui::EguiInputSystemSettings {
                run_write_keyboard_input_messages_system: false,
                run_write_pointer_button_messages: false,
                run_write_non_window_pointer_moved_messages_system: false,
                run_write_mouse_wheel_messages_system: false,
                ..default()
            },
            enable_cursor_icon_updates: false,
            ..default()
        },
    ));
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(super) fn resize(
    surfaces: Query<&ComputedNode, With<Surface>>,
    state: Res<State>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Camera, &mut EguiContext), With<EditorCamera>>,
) {
    let (Ok(node), Ok((mut camera, mut context))) = (surfaces.single(), cameras.single_mut())
    else {
        return;
    };
    camera.is_active = state.visible;
    context
        .get_mut()
        .set_zoom_factor(node.inverse_scale_factor().recip());
    let size = node
        .content_box()
        .size()
        .ceil()
        .max(Vec2::ONE)
        .min(Vec2::splat(4096.0));
    let extent = Extent3d {
        width: size.x as u32,
        height: size.y as u32,
        depth_or_array_layers: 1,
    };
    if state.visible
        && let Some(mut image) = images.get_mut(&state.image)
        && image.texture_descriptor.size != extent
    {
        image.resize(extent);
    }
}

/// Place the operating system's IME candidate window beside the embedded caret.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn platform(
    surfaces: Query<(&ComputedNode, &UiGlobalTransform), With<Surface>>,
    output: Query<&bevy_egui::EguiOutput, With<EditorCamera>>,
    mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut state: ResMut<State>,
) {
    let (Ok((node, transform)), Ok(output), Ok(mut window)) =
        (surfaces.single(), output.single(), window.single_mut())
    else {
        return;
    };
    if state.focused
        && let Some(ime) = output.platform_output.ime
    {
        let point = node.content_box().min
            + Vec2::new(ime.cursor_rect.left(), ime.cursor_rect.bottom())
                / node.inverse_scale_factor();
        window.ime_position = transform.transform_point2(point) / window.scale_factor();
        window.ime_enabled = true;
        state.ime_owned = true;
    } else if state.ime_owned {
        window.ime_enabled = false;
        state.ime_owned = false;
    }
}

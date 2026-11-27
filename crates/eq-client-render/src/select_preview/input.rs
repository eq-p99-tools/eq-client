//! Preview-only camera capture: never turns or selects the character.
use super::{Listed, Preview};
use crate::{OrbitCamera, camera, options, windows};
use bevy::{
    input::mouse::{MouseButtonInput, MouseMotion},
    prelude::*,
    window::{PrimaryWindow, WindowFocused},
};

#[derive(Default)]
pub(super) struct Drag {
    listed: Option<Listed>,
    held: bool,
}

/// A fresh right press on the preview captures until release or cancellation.
/// Moving across UI during capture does not transfer the gesture to a control.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn orbit_input(
    mut preview: ResMut<Preview>,
    mut drag: Local<Drag>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut motion: MessageReader<MouseMotion>,
    mut focus: MessageReader<WindowFocused>,
    keyboard: Res<ButtonInput<KeyCode>>,
    held: Res<ButtonInput<MouseButton>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    ui: windows::pointer::PointerUi,
    options: Res<options::OptionsState>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let window = windows.single().ok();
    let primary = window.map(|(entity, _)| entity);
    let lost_focus = focus
        .read()
        .any(|event| Some(event.window) == primary && !event.focused);
    let listed = preview.showing().then_some(preview.shown).flatten();
    let available = listed.is_some()
        && window.is_some_and(|(_, window)| window.focused)
        && !lost_focus
        && !keyboard.just_pressed(KeyCode::Escape);
    if !available || drag.listed != listed || !held.pressed(MouseButton::Right) {
        drag.held = false;
    }
    drag.listed = listed;
    for event in buttons.read().filter(|event| Some(event.window) == primary) {
        if event.button == MouseButton::Right {
            drag.held = event.state.is_pressed()
                && available
                && window
                    .and_then(|(_, window)| window.physical_cursor_position())
                    .is_some_and(|cursor| !ui.contains(cursor));
            if drag.held {
                preview.turning = false;
            }
        }
    }
    if !drag.held {
        motion.clear();
        return;
    }
    let delta: Vec2 = motion.read().map(|event| event.delta).sum();
    let turn = options
        .options
        .turn_per_pixel(camera::screen_width(window.map(|(_, window)| window)));
    let rise = if options.options.invert_y { -1.0 } else { 1.0 };
    if let Ok(mut camera) = cameras.single_mut() {
        camera.yaw -= delta.x * turn;
        camera.pitch = (camera.pitch - delta.y * turn * rise).clamp(-1.45, 1.45);
    }
}

#[cfg(test)]
mod tests;

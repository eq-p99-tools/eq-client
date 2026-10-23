//! The window's size from the command line (`--window-size`), in physical
//! pixels, as a recording or screenshot of the window is measured. Bevy opens
//! a window at its size in the desktop's scaled pixels, so on a scaled
//! desktop (125%, 150%) the window is set to the asked size again once it
//! exists and knows its scale. The player may resize it after that.
use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowCreated, WindowResolution},
};

/// The window's drawing area the command line asked for, in physical pixels.
#[derive(Resource)]
struct Asked(UVec2);

/// What the window opens at for an asked size.
pub(crate) fn resolution((width, height): (u32, u32)) -> WindowResolution {
    WindowResolution::new(width, height)
}

/// Keeps the asked size once the window knows the desktop's scale.
pub(crate) fn install(app: &mut App, (width, height): (u32, u32)) {
    app.insert_resource(Asked(UVec2::new(width, height)))
        .add_systems(Update, settle);
}

/// Sets the drawing area to the asked size as the window is created, where
/// the desktop's scale made it another.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn settle(
    mut created: MessageReader<WindowCreated>,
    asked: Res<Asked>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    for created in created.read() {
        if let Ok(mut window) = windows.get_mut(created.window)
            && window.resolution.physical_size() != asked.0
        {
            window
                .resolution
                .set_physical_resolution(asked.0.x, asked.0.y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window as Bevy creates it on a desktop at `scale`: the asked size
    /// taken as scaled pixels.
    fn created(app: &mut App, scale: f32) -> Entity {
        let mut resolution = resolution((1920, 1080));
        resolution.set_scale_factor_and_apply_to_physical_size(scale);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut().write_message(WindowCreated { window });
        app.update();
        window
    }

    fn physical(app: &App, window: Entity) -> UVec2 {
        app.world()
            .get::<Window>(window)
            .unwrap()
            .resolution
            .physical_size()
    }

    #[test]
    fn the_window_draws_the_asked_pixels_on_a_scaled_desktop_too() {
        let mut app = App::new();
        app.add_message::<WindowCreated>();
        install(&mut app, (1920, 1080));
        let unscaled = created(&mut app, 1.0);
        assert_eq!(physical(&app, unscaled), UVec2::new(1920, 1080));
        // At 150%, Bevy made it 2880x1620, so it's set back to the asked size.
        let scaled = created(&mut app, 1.5);
        assert_eq!(physical(&app, scaled), UVec2::new(1920, 1080));
        // Once created, the player's own size stays.
        app.world_mut()
            .get_mut::<Window>(scaled)
            .unwrap()
            .resolution
            .set_physical_resolution(1280, 720);
        app.update();
        assert_eq!(physical(&app, scaled), UVec2::new(1280, 720));
    }
}

//! The installed client's full-window screens in the screen's own pixels.
//! The official client draws its login screen, its list of worlds and its
//! loading screen at their own size in the screen's pixels, whatever the
//! desktop's display scaling: a 640 by 480 screen covers 640 by 480 pixels,
//! centred on black (a screenshot of it at 2560 by 1600, 10-19). This
//! client's windows otherwise follow the desktop's scaling, so while one of
//! those screens is drawn the interface is laid out at one unit to a pixel:
//! every size, place, border and font size in it, text rasterised at the
//! skin's size. The rest of the interface keeps the desktop's scaling.
//! Places and scroll positions are kept in logical pixels (a unit at the
//! interface's scale), so a window keeps its place and a list its scroll
//! across the change.
use crate::{loading::Loading, login::FrontEnd, online::OnlineState};
use bevy::{prelude::*, window::PrimaryWindow};

/// Lays the interface out at one unit to a pixel while an official
/// full-window screen is drawn, and at the desktop's scaling otherwise.
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        fit.in_set(crate::Stage::Present)
            .after(crate::loading::Shown),
    );
}

/// The interface's scale for a window at `window_factor` (its scale factor):
/// its inverse while an official screen is drawn, so a unit is a pixel;
/// else none of its own.
fn scale(official: bool, window_factor: f32) -> f32 {
    if official && window_factor > 0.0 {
        window_factor.recip()
    } else {
        1.0
    }
}

/// Whether an official full-window screen is drawn: the skin's login screen
/// or list of worlds, or the installation's loading picture, at Enter World
/// or over a move between zones, while it is drawn rather than waiting.
fn official(
    front: &FrontEnd,
    online: &OnlineState,
    look: &crate::login::LoginLook,
    loading: &Loading,
    screen: &crate::loading::Screen,
) -> bool {
    let entering = front.screen(online) == crate::login::Screen::Characters
        && online
            .selection
            .as_ref()
            .is_some_and(crate::character_select::Selection::entering);
    let loading = (loading.covered() && !loading.waiting()) || entering;
    look.drawn(front, online) || (screen.pictured() && loading)
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn fit(
    windows: Query<&Window, With<PrimaryWindow>>,
    (front, online, look): (
        Res<FrontEnd>,
        Res<OnlineState>,
        Res<crate::login::LoginLook>,
    ),
    (loading, screen): (Res<Loading>, Res<crate::loading::Screen>),
    mut ui: ResMut<UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let wanted = scale(
        official(&front, &online, &look, &loading, &screen),
        window.scale_factor(),
    );
    // Exactly, so an unchanged scale never marks the layout for a rebuild.
    if ui.0.to_bits() != wanted.to_bits() {
        ui.0 = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_official_screen_takes_one_unit_to_a_pixel_and_the_rest_keeps_the_desktops_scale() {
        assert_eq!((640.0 * scale(true, 1.5) * 1.5_f32).round(), 640.0);
        assert_eq!(scale(true, 1.0), 1.0);
        assert_eq!(scale(true, 2.0), 0.5);
        assert_eq!(scale(false, 1.5), 1.0);
        assert_eq!(scale(true, 0.0), 1.0);
    }

    #[test]
    fn the_loading_picture_takes_one_unit_to_a_pixel_while_it_is_drawn() {
        let mut app = App::new();
        app.insert_resource(OnlineState::new(true))
            .init_resource::<FrontEnd>()
            .init_resource::<crate::login::LoginLook>()
            .init_resource::<Loading>()
            .insert_resource(crate::loading::Screen::pictured_for_tests())
            .init_resource::<UiScale>()
            .init_resource::<Writes>()
            .add_systems(Update, (fit, count).chain());
        app.world_mut().spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(1920, 1080)
                    .with_scale_factor_override(1.5),
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
        let writes = |app: &App| app.world().resource::<Writes>().0;
        let before = writes(&app);
        app.update();
        // A frame that changes nothing leaves the scale, and the layout, be.
        assert_eq!(writes(&app), before);
        app.insert_resource(Loading::covering_for_tests());
        app.update();
        // 640 units of the picture then cover 640 of the screen's pixels,
        // and a font's size of 10 units is 10 of its pixels.
        let ui = app.world().resource::<UiScale>().0;
        assert_eq!((640.0 * ui * 1.5).round(), 640.0);
        assert_eq!((10.0 * ui * 1.5).round(), 10.0);
        let covered = writes(&app);
        app.update();
        assert_eq!(writes(&app), covered);
        app.insert_resource(Loading::default());
        app.update();
        // Exactly the desktop's scale again.
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
    }

    /// How many frames found the interface's scale changed.
    #[derive(Resource, Default)]
    struct Writes(usize);

    #[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
    fn count(ui: Res<UiScale>, mut writes: ResMut<Writes>) {
        if ui.is_changed() {
            writes.0 += 1;
        }
    }
}

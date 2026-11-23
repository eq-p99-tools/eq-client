//! The installed client's full-window screens in the screen's own pixels.
//! The official client draws its login screen, its list of worlds and its
//! loading screen at their own size in the screen's pixels, whatever the
//! desktop's display scaling: a 640 by 480 screen covers 640 by 480 pixels,
//! centred on black (a screenshot of it at 2560 by 1600, 10-19). This
//! client's windows otherwise follow the desktop's scaling, so while one of
//! those screens is drawn the interface is laid out at one unit to a pixel:
//! every size, place, border and font size in it, text rasterised at the
//! skin's size. The rest of the interface keeps the desktop's scaling,
//! enlarged or shrunk by the player's interface scale on the quality-of-life page
//! ([`Level::UiScale`]), which the official screens never take.
//! Places and scroll positions are kept in logical pixels (a unit at the
//! interface's scale), so a window keeps its place and a list its scroll
//! across the change. `/uiscale` sets the player's scale by typing, for when
//! the slider is hard to reach, and `/uiscale` alone returns it to 100 %.
use crate::{loading::Loading, login::FrontEnd, online::OnlineState};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::options::Level;

/// Lays the interface out at one unit to a pixel while an official
/// full-window screen is drawn, and at the desktop's scaling times the
/// player's interface scale otherwise.
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (request, fit)
            .chain()
            .in_set(crate::Stage::Present)
            .after(crate::loading::Shown),
    );
}

/// How `/uiscale` is typed, said when it is typed otherwise.
const USAGE: &str =
    "Use /uiscale with a size from 50 to 200 in steps of 5, or /uiscale alone for 100";

/// The interface scale a typed `/uiscale` asks for: the percent after it,
/// one the slider stops at, or 100 with none. None for any other line.
pub(crate) fn typed(input: &str) -> Option<Result<u16, String>> {
    let mut words = input.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("/uiscale") {
        return None;
    }
    let level = Level::UiScale;
    Some(match (words.next(), words.next()) {
        (None, _) => Ok(eq_client_core::options::Options::default().level(level)),
        (Some(word), None) => word
            .strip_suffix('%')
            .unwrap_or(word)
            .parse::<u16>()
            .ok()
            .filter(|percent| level.at(level.fraction(*percent)) == *percent)
            .ok_or_else(|| USAGE.to_owned()),
        _ => Err(USAGE.to_owned()),
    })
}

/// Sets the interface scale a typed `/uiscale` asked for, and says so.
fn request(
    chat: Option<ResMut<crate::chat::ChatState>>,
    options: Option<ResMut<crate::options::OptionsState>>,
) {
    let (Some(mut chat), Some(mut options)) = (chat, options) else {
        return;
    };
    let Some(percent) = chat.ui_scale.take() else {
        return;
    };
    if options.options.level(Level::UiScale) != percent {
        options.options.set_level(Level::UiScale, percent);
    }
    let words = format!("Interface scale set to {}.", Level::UiScale.words(percent));
    chat.history
        .push(crate::chat::system_line(crate::chat::Said::own(words)));
}

/// The interface's scale for a window at `window_factor` (its scale factor):
/// its inverse while an official screen is drawn, so a unit is a pixel;
/// else the player's interface scale, `percent` of the desktop's, exactly
/// 1.0 at 100.
fn scale(official: bool, window_factor: f32, percent: u16) -> f32 {
    if official {
        if window_factor > 0.0 {
            window_factor.recip()
        } else {
            1.0
        }
    } else {
        f32::from(percent) / 100.0
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
    let loading = loading.drawn() || entering;
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
    options: Option<Res<crate::options::OptionsState>>,
    mut ui: ResMut<UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let percent = options.map_or(100, |options| options.options.level(Level::UiScale));
    let wanted = scale(
        official(&front, &online, &look, &loading, &screen),
        window.scale_factor(),
        percent,
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
        assert_eq!((640.0 * scale(true, 1.5, 100) * 1.5_f32).round(), 640.0);
        assert_eq!(scale(true, 1.0, 100), 1.0);
        assert_eq!(scale(true, 2.0, 100), 0.5);
        assert_eq!(scale(false, 1.5, 100), 1.0);
        assert_eq!(scale(true, 0.0, 100), 1.0);
        // The player's scale enlarges or shrinks the windows, never an
        // official screen.
        assert_eq!(scale(false, 1.5, 150), 1.5);
        assert_eq!(scale(false, 1.0, 50), 0.5);
        assert_eq!((640.0 * scale(true, 1.5, 200) * 1.5_f32).round(), 640.0);
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

    #[test]
    fn the_players_interface_scale_sizes_the_windows_but_not_the_official_screens() {
        let mut app = App::new();
        app.insert_resource(OnlineState::new(true))
            .init_resource::<FrontEnd>()
            .init_resource::<crate::login::LoginLook>()
            .init_resource::<Loading>()
            .insert_resource(crate::loading::Screen::pictured_for_tests())
            .init_resource::<crate::options::OptionsState>()
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
        let set = |app: &mut App, percent: u16| {
            app.world_mut()
                .resource_mut::<crate::options::OptionsState>()
                .options
                .set_level(Level::UiScale, percent);
            app.update();
        };
        set(&mut app, 150);
        assert_eq!(app.world().resource::<UiScale>().0, 1.5);
        let writes = |app: &App| app.world().resource::<Writes>().0;
        let scaled = writes(&app);
        app.update();
        assert_eq!(writes(&app), scaled);
        // The loading picture still takes one unit to a pixel.
        app.insert_resource(Loading::covering_for_tests());
        app.update();
        let ui = app.world().resource::<UiScale>().0;
        assert_eq!((640.0 * ui * 1.5).round(), 640.0);
        app.insert_resource(Loading::default());
        app.update();
        assert_eq!(app.world().resource::<UiScale>().0, 1.5);
        set(&mut app, 100);
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
    }

    #[test]
    fn uiscale_is_typed_with_a_slider_stop_or_alone_for_100() {
        assert_eq!(typed("/uiscale"), Some(Ok(100)));
        assert_eq!(typed("/UIScale 150"), Some(Ok(150)));
        assert_eq!(typed("/uiscale 75%"), Some(Ok(75)));
        assert_eq!(typed("/uiscale 50"), Some(Ok(50)));
        assert_eq!(typed("/uiscale 200"), Some(Ok(200)));
        for bad in [
            "/uiscale 49",
            "/uiscale 201",
            "/uiscale 101",
            "/uiscale big",
            "/uiscale 100 100",
        ] {
            assert_eq!(typed(bad), Some(Err(USAGE.to_owned())), "{bad}");
        }
        assert_eq!(typed("/uiscales"), None);
        assert_eq!(typed("hello"), None);
    }

    #[test]
    fn a_typed_uiscale_sets_the_interface_scale_and_says_so() {
        let mut app = App::new();
        app.init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::options::OptionsState>()
            .add_systems(Update, request);
        app.world_mut()
            .resource_mut::<crate::options::OptionsState>()
            .options
            .set_level(Level::UiScale, 50);
        let ask = |app: &mut App, line: &str| {
            let mut chat = app.world_mut().resource_mut::<crate::chat::ChatState>();
            let asked = crate::chat::client_request(line, &mut chat);
            app.update();
            asked
        };
        let scale = |app: &App| {
            app.world()
                .resource::<crate::options::OptionsState>()
                .options
                .level(Level::UiScale)
        };
        assert_eq!(ask(&mut app, "/uiscale"), Some(Ok(())));
        assert_eq!(scale(&app), 100);
        assert_eq!(ask(&mut app, "/uiscale 125"), Some(Ok(())));
        assert_eq!(scale(&app), 125);
        assert_eq!(ask(&mut app, "/uiscale 7"), Some(Err(USAGE.to_owned())));
        assert_eq!(scale(&app), 125);
        let said: Vec<String> = app
            .world()
            .resource::<crate::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .into_iter()
            .map(|(_, line)| line.message.text.clone())
            .collect();
        assert_eq!(
            said,
            [
                "Interface scale set to 100 %.",
                "Interface scale set to 125 %."
            ]
        );
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

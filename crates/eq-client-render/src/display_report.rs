//! Keeps the display snapshot the sessions' machine report reads
//! ([`eq_client_core::machine`]): the desktop mode of the monitor the
//! window is on, the game screen's size, that monitor's place and name, and
//! the renderer's adapter name. It runs on the window's side, so the
//! session never asks the windowing or graphics libraries anything itself.
use bevy::{
    prelude::*,
    render::renderer::RenderAdapterInfo,
    window::{Monitor, PrimaryMonitor, PrimaryWindow, WindowMode, WindowPosition},
};
use eq_client_core::machine::Display;

/// One monitor as the report needs it.
#[derive(Clone, Debug, PartialEq)]
struct Screen {
    position: IVec2,
    size: UVec2,
    refresh_millihertz: Option<u32>,
    name: Option<String>,
    primary: bool,
}

/// The display as the window sees it: the monitor holding the window's
/// top-left corner, else the primary one, else the first; the game screen
/// is the window's own size only while it holds the monitor exclusively.
fn display(
    mut monitors: Vec<Screen>,
    window: Option<(WindowPosition, &WindowMode, UVec2)>,
    adapter: Option<&str>,
) -> Display {
    // Left to right, then top to bottom, as the monitors are numbered.
    monitors.sort_by_key(|screen| (screen.position.x, screen.position.y));
    let corner = window.and_then(|(position, _, _)| match position {
        WindowPosition::At(corner) => Some(corner),
        _ => None,
    });
    let holds = |screen: &Screen| {
        corner.is_some_and(|corner| {
            let end = screen.position + screen.size.as_ivec2();
            corner.cmpge(screen.position).all() && corner.cmplt(end).all()
        })
    };
    let monitor = monitors
        .iter()
        .position(holds)
        .or_else(|| monitors.iter().position(|screen| screen.primary))
        .unwrap_or(0);
    let chosen = monitors.get(monitor);
    let desktop = chosen.map_or((0, 0, 0), |screen| {
        let hertz = screen
            .refresh_millihertz
            .map_or(0, |millihertz| millihertz.saturating_add(500) / 1000);
        (screen.size.x, screen.size.y, hertz)
    });
    let screen = match window {
        Some((_, WindowMode::Fullscreen(..), size)) => (size.x, size.y),
        _ => (desktop.0, desktop.1),
    };
    Display {
        desktop,
        screen,
        monitor,
        monitor_name: chosen.and_then(|screen| screen.name.clone()),
        adapter: adapter.unwrap_or_default().to_owned(),
    }
}

/// Refreshes the snapshot from the monitors, the primary window and the
/// renderer's adapter.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn update(
    settings: Res<crate::ViewerSettings>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    window: Query<&Window, With<PrimaryWindow>>,
    adapter: Option<Res<RenderAdapterInfo>>,
) {
    let screens = monitors
        .iter()
        .map(|(monitor, primary)| Screen {
            position: monitor.physical_position,
            size: monitor.physical_size(),
            refresh_millihertz: monitor.refresh_rate_millihertz,
            name: monitor.name.clone(),
            primary,
        })
        .collect();
    let window = window
        .single()
        .ok()
        .map(|window| (window.position, &window.mode, window.physical_size()));
    let adapter = adapter.as_ref().map(|info| info.name.as_str());
    settings.0.display.set(display(screens, window, adapter));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(x: i32, width: u32, primary: bool) -> Screen {
        Screen {
            position: IVec2::new(x, 0),
            size: UVec2::new(width, 1080),
            refresh_millihertz: Some(59_940),
            name: Some(format!("monitor at {x}")),
            primary,
        }
    }

    #[test]
    fn the_desktop_is_the_monitor_the_window_is_on() {
        let monitors = vec![screen(1920, 2560, false), screen(0, 1920, true)];
        let windowed = WindowMode::Windowed;
        let on_second = display(
            monitors.clone(),
            Some((
                WindowPosition::At(IVec2::new(2000, 50)),
                &windowed,
                UVec2::new(800, 600),
            )),
            Some("Test Adapter"),
        );
        assert_eq!(on_second.desktop, (2560, 1080, 60));
        assert_eq!(on_second.screen, (2560, 1080));
        assert_eq!(on_second.monitor, 1);
        assert_eq!(on_second.monitor_name.as_deref(), Some("monitor at 1920"));
        assert_eq!(on_second.adapter, "Test Adapter");
        // A window whose place is not known yet is on the primary monitor.
        let unplaced = display(
            monitors,
            Some((WindowPosition::Automatic, &windowed, UVec2::new(800, 600))),
            None,
        );
        assert_eq!((unplaced.monitor, unplaced.desktop.0), (0, 1920));
        assert_eq!(unplaced.adapter, "");
    }

    #[test]
    fn the_game_screen_is_the_windows_own_only_when_it_holds_the_monitor() {
        let monitors = vec![screen(0, 1920, true)];
        let exclusive = WindowMode::Fullscreen(
            bevy::window::MonitorSelection::Current,
            bevy::window::VideoModeSelection::Current,
        );
        let at = WindowPosition::At(IVec2::ZERO);
        let size = UVec2::new(1280, 720);
        assert_eq!(
            display(monitors.clone(), Some((at, &exclusive, size)), None).screen,
            (1280, 720)
        );
        let borderless = WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current);
        assert_eq!(
            display(monitors, Some((at, &borderless, size)), None).screen,
            (1920, 1080)
        );
        assert_eq!(display(Vec::new(), None, None), Display::default());
    }
}

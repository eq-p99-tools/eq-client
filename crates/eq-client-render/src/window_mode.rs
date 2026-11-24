//! A desktop fullscreen gesture remembers the latest windowed bounds.
use crate::keys::{Act, Keys};
use bevy::{
    input::keyboard::KeyboardInput,
    prelude::*,
    window::{MonitorSelection, PrimaryWindow, WindowMode, WindowPosition},
};

#[derive(Clone, Copy)]
struct Placement {
    position: WindowPosition,
    size: UVec2,
}

/// The windowed placement before this fullscreen visit, independent of UI profiles.
#[derive(Default, Resource)]
pub(crate) struct Windowed(Option<Placement>);

impl Windowed {
    fn toggle(&mut self, window: &mut Window) {
        if window.mode == WindowMode::Windowed {
            self.0 = Some(Placement {
                position: window.position,
                size: window.resolution.physical_size(),
            });
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        } else {
            window.mode = WindowMode::Windowed;
            if let Some(placement) = self.0.take() {
                window.position = placement.position;
                window
                    .resolution
                    .set_physical_resolution(placement.size.x, placement.size.y);
            }
        }
    }
}

type DesktopInput<'w, 's> = ParamSet<
    'w,
    's,
    (
        Keys<'static, 'static>,
        Query<'static, 'static, &'static mut Window, With<PrimaryWindow>>,
        ResMut<'static, ButtonInput<KeyCode>>,
    ),
>;

/// A desktop chord also works in a text field, but a key being rebound belongs
/// to the binding editor. Native repeat messages never request another toggle,
/// even when a text field has consumed and reset that physical key's state.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn input(
    mut keyboard: MessageReader<KeyboardInput>,
    mut input: DesktopInput<'_, '_>,
    mut windowed: ResMut<Windowed>,
) {
    let events: Vec<_> = keyboard.read().collect();
    let (claimed, toggle) = {
        let keys = input.p0();
        keys.desktop_input(Act::Fullscreen, &events)
    };
    // Enter must not also connect from login or enter a selected world. Keep
    // its held state so repeated keyboard messages do not become new presses.
    for key in claimed {
        input.p2().clear_just_pressed(key);
    }
    if toggle && let Ok(mut window) = input.p1().single_mut() {
        windowed.toggle(&mut window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Typing;
    use bevy::{
        input::{ButtonState, keyboard::Key},
        window::WindowResolution,
    };

    fn app() -> (App, Entity) {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<Key>>()
            .init_resource::<Windowed>()
            .add_message::<KeyboardInput>()
            .add_systems(PreUpdate, crate::keys::presses::capture)
            .add_systems(Update, input);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    position: WindowPosition::At(IVec2::new(-1600, 90)),
                    resolution: WindowResolution::new(1200, 800),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        (app, window)
    }

    fn event(key_code: KeyCode, logical_key: Key, repeat: bool) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat,
            window: Entity::PLACEHOLDER,
        }
    }

    fn press(app: &mut App, repeat: bool) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.reset(KeyCode::Enter);
        keys.press(KeyCode::AltLeft);
        keys.press(KeyCode::Enter);
        app.world_mut()
            .resource_mut::<ButtonInput<Key>>()
            .press(Key::Alt);
        app.world_mut()
            .write_message(event(KeyCode::Enter, Key::Enter, repeat));
        app.update();
    }

    #[test]
    fn fullscreen_returns_to_the_latest_windowed_physical_bounds() {
        let mut window = Window {
            position: WindowPosition::At(IVec2::new(-1600, 90)),
            resolution: WindowResolution::new(1200, 800).with_scale_factor_override(1.5),
            ..default()
        };
        let mut saved = Windowed::default();
        for (position, size) in [
            (IVec2::new(-1600, 90), UVec2::new(1200, 800)),
            (IVec2::new(180, 130), UVec2::new(960, 640)),
        ] {
            window.position = WindowPosition::At(position);
            window.resolution.set_physical_resolution(size.x, size.y);
            saved.toggle(&mut window);
            assert_eq!(
                window.mode,
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            );
            // The desktop reports the fullscreen monitor's position and size.
            window.position = WindowPosition::At(IVec2::new(-1920, 0));
            window.resolution.set_physical_resolution(1920, 1080);
            saved.toggle(&mut window);
            assert_eq!(window.mode, WindowMode::Windowed);
            assert_eq!(window.position, WindowPosition::At(position));
            assert_eq!(window.resolution.physical_size(), size);
        }
    }

    #[test]
    fn an_alt_enter_hold_and_native_repeats_toggle_fullscreen_only_once() {
        let (mut app, window) = app();
        press(&mut app, false);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Enter)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        for _ in 0..3 {
            // Chat capture can reset a composing Enter, so a repeat can look
            // like a fresh physical edge. Its native repeat flag still wins.
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset(KeyCode::Enter);
            press(&mut app, true);
            assert_eq!(
                app.world().get::<Window>(window).unwrap().mode,
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            );
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::Enter)
            );
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::Enter);
        press(&mut app, false);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
    }

    #[test]
    fn fullscreen_respects_focus_and_binding_capture_and_works_while_composing() {
        let (mut app, window) = app();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        press(&mut app, false);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().resource_mut::<Typing>().rebinding = true;
        press(&mut app, false);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Windowed
        );
        app.world_mut().resource_mut::<Typing>().rebinding = false;
        app.world_mut().resource_mut::<Typing>().composing = true;
        press(&mut app, false);
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        assert!(app.world().resource::<Typing>().composing);
    }

    #[test]
    fn a_rebound_fullscreen_text_key_belongs_to_an_active_text_field() {
        let (mut app, window) = app();
        crate::keys::testing::bind(
            &mut app,
            Act::Fullscreen,
            crate::keys::Chord::key(KeyCode::KeyB),
        );
        for (composing, expected) in [
            (true, WindowMode::Windowed),
            (
                false,
                WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            ),
        ] {
            app.world_mut().resource_mut::<Typing>().composing = composing;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset(KeyCode::KeyB);
            keys.press(KeyCode::KeyB);
            let mut typed = event(KeyCode::KeyB, Key::Character("b".into()), false);
            typed.text = Some("b".into());
            app.world_mut().write_message(typed);
            app.update();
            assert_eq!(app.world().get::<Window>(window).unwrap().mode, expected);
        }
    }
}

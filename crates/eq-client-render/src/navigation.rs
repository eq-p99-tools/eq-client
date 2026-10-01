//! Logical arrow keys, including keypad navigation with Num Lock disabled.
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::PrimaryWindow,
};
use std::collections::HashMap;

/// Starts the arrow keys' state; their system runs with the chat's typing,
/// before movement and character selection read them.
pub(super) fn install(app: &mut App) {
    app.init_resource::<NavigationKeys>();
}

#[derive(Resource, Default)]
pub(super) struct NavigationKeys {
    sources: HashMap<KeyCode, KeyCode>,
    keys: ButtonInput<KeyCode>,
}

impl NavigationKeys {
    fn apply(&mut self, physical: KeyCode, logical: &Key, state: ButtonState) {
        if state == ButtonState::Released {
            if let Some(key) = self.sources.remove(&physical)
                && !self.sources.values().any(|held| *held == key)
            {
                self.keys.release(key);
            }
            return;
        }
        let key = match logical {
            Key::ArrowUp => KeyCode::ArrowUp,
            Key::ArrowDown => KeyCode::ArrowDown,
            Key::ArrowLeft => KeyCode::ArrowLeft,
            Key::ArrowRight => KeyCode::ArrowRight,
            _ => return,
        };
        self.sources.insert(physical, key);
        self.keys.press(key);
    }

    /// Merges held arrows and complete between-frame taps into physical bindings.
    pub fn sample(&self, physical: &ButtonInput<KeyCode>) -> ButtonInput<KeyCode> {
        let mut sampled = physical.clone();
        for key in self.keys.get_pressed().chain(self.keys.get_just_pressed()) {
            sampled.press(*key);
            if !self.keys.just_pressed(*key) && !physical.just_pressed(*key) {
                sampled.clear_just_pressed(*key);
            }
        }
        sampled
    }
}

/// Tracks releases by physical source even if Num Lock changes the release's meaning.
pub(super) fn update(
    mut events: MessageReader<KeyboardInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut navigation: ResMut<NavigationKeys>,
) {
    navigation.keys.clear();
    let focused = windows.single().is_ok_and(|window| window.focused);
    if !focused {
        *navigation = NavigationKeys::default();
    }
    for event in events.read() {
        if focused {
            navigation.apply(event.key_code, &event.logical_key, event.state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn holding_a_logical_arrow_does_not_create_new_press_edges() {
        let mut navigation = NavigationKeys::default();
        navigation.apply(KeyCode::Numpad2, &Key::ArrowDown, ButtonState::Pressed);
        assert!(
            navigation
                .sample(&ButtonInput::default())
                .just_pressed(KeyCode::ArrowDown)
        );
        navigation.keys.clear();
        let held = navigation.sample(&ButtonInput::default());
        assert!(held.pressed(KeyCode::ArrowDown));
        assert!(!held.just_pressed(KeyCode::ArrowDown));
    }
    #[test]
    fn keypad_arrows_preserve_taps_and_release_even_after_num_lock_changes() {
        let mut navigation = NavigationKeys::default();
        navigation.apply(KeyCode::Numpad2, &Key::ArrowDown, ButtonState::Pressed);
        navigation.apply(
            KeyCode::Numpad2,
            &Key::Character("2".into()),
            ButtonState::Released,
        );
        assert!(
            navigation
                .sample(&ButtonInput::default())
                .pressed(KeyCode::ArrowDown)
        );
        navigation.keys.clear();
        assert!(
            !navigation
                .sample(&ButtonInput::default())
                .pressed(KeyCode::ArrowDown)
        );
        navigation.apply(
            KeyCode::Numpad2,
            &Key::Character("2".into()),
            ButtonState::Pressed,
        );
        assert!(
            !navigation
                .sample(&ButtonInput::default())
                .pressed(KeyCode::ArrowDown)
        );
    }

    #[test]
    fn releasing_one_of_two_sources_does_not_release_the_other() {
        let mut navigation = NavigationKeys::default();
        navigation.apply(KeyCode::Numpad2, &Key::ArrowDown, ButtonState::Pressed);
        navigation.apply(KeyCode::ArrowDown, &Key::ArrowDown, ButtonState::Pressed);
        navigation.apply(KeyCode::Numpad2, &Key::ArrowDown, ButtonState::Released);
        navigation.keys.clear();
        assert!(
            navigation
                .sample(&ButtonInput::default())
                .pressed(KeyCode::ArrowDown)
        );
    }
}

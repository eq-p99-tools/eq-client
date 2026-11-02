//! Native chords keep their press-time modifiers until gameplay consumes them.
use super::Chord;
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

#[derive(Resource, Default)]
pub(crate) struct Presses(Vec<Chord>);

impl Presses {
    /// Physical press edges still govern consumption; scripts without native
    /// events use their synthetic keyboard's current modifiers.
    pub(super) fn matches(&self, chord: Chord, keys: &ButtonInput<KeyCode>) -> bool {
        keys.just_pressed(chord.key)
            && if self.0.iter().any(|pressed| pressed.key == chord.key) {
                self.0.contains(&chord)
            } else {
                chord.modifiers_held(keys)
            }
    }

    /// The first native chord for an assignment, or the synthetic key state.
    pub(super) fn chord(&self, key: KeyCode, keys: &ButtonInput<KeyCode>) -> Chord {
        self.0
            .iter()
            .find(|chord| chord.key == key)
            .copied()
            .unwrap_or(Chord {
                key,
                ctrl: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
                shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
                alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
            })
    }
}

/// Snapshot before text fields or binding capture consume keyboard events.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn capture(
    mut events: MessageReader<KeyboardInput>,
    logical: Res<ButtonInput<Key>>,
    mut presses: ResMut<Presses>,
) {
    let events: Vec<_> = events.read().cloned().collect();
    presses.0.clear();
    for (event, held) in events
        .iter()
        .zip(super::modifiers::snapshots(&events, &logical))
    {
        if event.state.is_pressed() && !event.repeat {
            presses.0.push(Chord {
                key: event.key_code,
                ctrl: held[0],
                shift: held[1],
                alt: held[2],
            });
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::keys::{Act, KeyMap};
    use bevy::input::ButtonState;

    /// A complete native chord can arrive before the next frame.
    pub(crate) fn chord_events(key_code: KeyCode, character: &str) -> [KeyboardInput; 4] {
        let event = |key_code, logical_key, state| KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        [
            event(KeyCode::ControlLeft, Key::Control, ButtonState::Pressed),
            event(
                key_code,
                Key::Character(character.into()),
                ButtonState::Pressed,
            ),
            event(
                key_code,
                Key::Character(character.into()),
                ButtonState::Released,
            ),
            event(KeyCode::ControlLeft, Key::Control, ButtonState::Released),
        ]
    }

    #[test]
    fn a_complete_chord_uses_its_press_modifiers_and_respects_consumption() {
        let mut app = App::new();
        app.init_resource::<Presses>()
            .init_resource::<ButtonInput<Key>>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, capture);
        app.world_mut()
            .write_message_batch(chord_events(KeyCode::Digit1, "1"));
        let mut physical = ButtonInput::default();
        physical.press(KeyCode::Digit1);
        physical.release(KeyCode::Digit1);
        app.update();
        let presses = app.world().resource::<Presses>();
        let map = KeyMap::default();
        assert!(map.just_pressed(&physical, Act::BindSlot(0), presses));
        assert!(!map.just_pressed(&physical, Act::Slot(0), presses));
        physical.reset_all();
        assert!(!map.just_pressed(&physical, Act::BindSlot(0), presses));

        // A later synthetic key must not inherit a previous native chord.
        app.update();
        physical.press(KeyCode::Digit1);
        let presses = app.world().resource::<Presses>();
        assert!(map.just_pressed(&physical, Act::Slot(0), presses));
        assert!(!map.just_pressed(&physical, Act::BindSlot(0), presses));
    }
}

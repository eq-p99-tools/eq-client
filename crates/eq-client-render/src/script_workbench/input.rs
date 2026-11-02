//! Keep keyboard modifiers paired with each event delivered to Bevy's editor.
//!
//! Bevy 0.19's widget reads the end-of-frame `ButtonInput<Key>` while handling
//! each event. A complete Ctrl+A chord in one frame otherwise loses Control.
//! Replay only the modifier snapshot around each focused event, then restore
//! the real input state. Editing, clipboard, selection and IME remain Bevy's.

use bevy::{
    ecs::message::MessageCursor,
    input::keyboard::{Key, KeyboardInput},
    input_focus::{InputFocus, dispatch_focused_input},
    prelude::*,
};

use crate::keys::modifiers::{MODIFIERS, snapshots};

/// Consume this editor's raw messages before the standard focused dispatcher.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn forward(
    mut commands: Commands,
    mut cursor: Local<MessageCursor<KeyboardInput>>,
    mut messages: ResMut<Messages<KeyboardInput>>,
    focus: Res<InputFocus>,
    fields: Query<(), (With<super::view::Field>, With<bevy::text::EditableText>)>,
    keys: Res<ButtonInput<Key>>,
) {
    if !focus.get().is_some_and(|entity| fields.contains(entity)) {
        cursor.clear(&messages);
        return;
    }
    let events: Vec<_> = cursor.read(&messages).cloned().collect();
    if events.is_empty() {
        return;
    }
    let modifiers = snapshots(&events, &keys);
    messages.clear();
    commands.queue(move |world: &mut World| {
        let real = world.resource::<ButtonInput<Key>>().clone();
        for (input, held) in events.into_iter().zip(modifiers) {
            {
                let mut keys = world.resource_mut::<ButtonInput<Key>>();
                for (key, pressed) in MODIFIERS.into_iter().zip(held) {
                    if pressed {
                        keys.press(key);
                    } else {
                        keys.release(key);
                    }
                }
            }
            // Use Bevy's dispatcher itself, including its focus and bubbling
            // rules. Clear the consumed copy before the scheduled dispatcher
            // and other readers can deliver it a second time.
            world.write_message(input);
            let result = world.run_system_cached(dispatch_focused_input::<KeyboardInput>);
            world.resource_mut::<Messages<KeyboardInput>>().clear();
            if let Err(error) = result {
                warn!(%error, "Could not dispatch script editor input");
                break;
            }
        }
        *world.resource_mut::<ButtonInput<Key>>() = real;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;

    #[derive(Resource, Default)]
    struct Seen(Vec<(Key, bool)>);

    #[test]
    fn focused_delivery_is_ordered_once_and_restores_real_key_state() {
        use bevy::input_focus::{FocusCause, FocusedInput, InputDispatchPlugin, InputFocusSystems};
        let mut app = App::new();
        app.add_plugins((bevy::input::InputPlugin, InputDispatchPlugin))
            .init_resource::<InputFocus>()
            .init_resource::<Seen>()
            .add_observer(
                |mut event: On<FocusedInput<KeyboardInput>>,
                 keys: Res<ButtonInput<Key>>,
                 mut seen: ResMut<Seen>| {
                    // This observer records one delivery before bubbling.
                    event.propagate(false);
                    seen.0
                        .push((event.input.logical_key.clone(), keys.pressed(Key::Control)));
                },
            )
            .add_systems(
                PreUpdate,
                forward
                    .after(bevy::input::InputSystems)
                    .before(InputFocusSystems::Dispatch),
            );
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        let field = app
            .world_mut()
            .spawn((
                super::super::view::Field::Path,
                bevy::text::EditableText::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        for mut event in [
            event(Key::Control, ButtonState::Pressed),
            event(Key::Character("a".into()), ButtonState::Pressed),
            event(Key::Control, ButtonState::Released),
        ] {
            event.window = window;
            app.world_mut().write_message(event);
        }
        app.update();
        assert_eq!(app.world().resource::<Seen>().0.len(), 3);
        assert!(app.world().resource::<Seen>().0[1].1);
        assert!(
            !app.world()
                .resource::<ButtonInput<Key>>()
                .pressed(Key::Control)
        );
        app.update();
        assert_eq!(app.world().resource::<Seen>().0.len(), 3);
    }

    fn event(key: Key, state: ButtonState) -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn rapid_chords_and_following_plain_text_keep_their_own_modifiers() {
        let events = [
            event(Key::Control, ButtonState::Pressed),
            event(Key::Character("a".into()), ButtonState::Pressed),
            event(Key::Character("a".into()), ButtonState::Released),
            event(Key::Control, ButtonState::Released),
            event(Key::Character("b".into()), ButtonState::Pressed),
        ];
        let states = snapshots(&events, &ButtonInput::default());
        assert!(states[1][0]);
        assert!(!states[4][0]);
    }

    #[test]
    fn modifiers_held_since_an_earlier_frame_are_preserved() {
        let mut keys = ButtonInput::default();
        keys.press(Key::Control);
        let events = [event(Key::Character("v".into()), ButtonState::Pressed)];
        assert!(snapshots(&events, &keys)[0][0]);
    }
}

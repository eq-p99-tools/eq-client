//! Name editing uses event-time modifiers, including chords within one frame.
use super::{Clipboard, ClipboardRead, Form, Key, KeyboardInput};
use bevy::prelude::ButtonInput;

impl Form {
    /// Keep asynchronous paste attached to this draft, never a later character.
    pub(super) fn edit_name(
        &mut self,
        events: &[KeyboardInput],
        keys: &ButtonInput<Key>,
        mut clipboard: Option<&mut Clipboard>,
    ) {
        if !self.typing {
            self.paste = None;
            return;
        }
        if let Some(result) = self.paste.as_mut().and_then(ClipboardRead::poll_result) {
            self.paste = None;
            match result {
                Ok(text) => self.append_name(&text),
                Err(_) => self.message = "Could not read clipboard text.".into(),
            }
        }
        let modifiers = crate::keys::modifiers::snapshots(events, keys);
        for (event, held) in events.iter().zip(modifiers) {
            if !event.state.is_pressed() {
                continue;
            }
            if held[0] || held[2] || held[3] {
                if held[0] && !held[2] && !held[3] {
                    match &event.logical_key {
                        Key::Character(key) if key.eq_ignore_ascii_case("a") => {
                            self.select_all = true;
                        }
                        Key::Character(key) if key.eq_ignore_ascii_case("v") => {
                            self.paste = clipboard.as_mut().map(|clipboard| clipboard.fetch_text());
                        }
                        _ => (),
                    }
                }
                continue;
            }
            if matches!(event.logical_key, Key::Backspace | Key::Delete) {
                self.paste = None;
                self.generating = None;
                if self.select_all {
                    self.draft.name.clear();
                    self.select_all = false;
                } else {
                    self.draft.name.pop();
                }
                self.message.clear();
            } else if let Some(text) = &event.text {
                self.append_name(text);
            }
        }
    }

    fn append_name(&mut self, text: &str) {
        for character in text.chars().filter(|c| !c.is_control()) {
            if self.select_all {
                self.draft.name.clear();
                self.select_all = false;
            }
            if self.draft.name.chars().count() < 30 {
                self.generating = None;
                self.draft.name.push(character);
            }
        }
        self.message.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{input::ButtonState, prelude::*};

    #[test]
    fn fast_select_all_and_paste_never_insert_shortcut_letters() {
        let (mut editor, ..) = super::super::tests::setup();
        let form = editor.form.as_mut().unwrap();
        let initial = form.draft.name.clone();
        let event = |logical_key: Key, state| KeyboardInput {
            key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            text: if let Key::Character(text) = &logical_key {
                Some(text.clone())
            } else {
                None
            },
            logical_key,
            state,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        let events = [
            event(Key::Control, ButtonState::Pressed),
            event(Key::Character("a".into()), ButtonState::Pressed),
            event(Key::Character("a".into()), ButtonState::Released),
            event(Key::Control, ButtonState::Released),
            event(Key::Control, ButtonState::Pressed),
            event(Key::Character("v".into()), ButtonState::Pressed),
            event(Key::Character("v".into()), ButtonState::Released),
            event(Key::Control, ButtonState::Released),
        ];
        form.edit_name(&events, &ButtonInput::default(), None);
        assert!(form.select_all);
        assert_eq!(form.draft.name, initial);
        form.paste = Some(ClipboardRead::Ready(Ok("Fresh\nhero".into())));
        form.edit_name(&[], &ButtonInput::default(), None);
        assert_eq!(form.draft.name, "Freshhero");
        assert!(!form.select_all);
        form.paste = Some(ClipboardRead::Ready(Ok("late".into())));
        form.typing = false;
        form.edit_name(&[], &ButtonInput::default(), None);
        assert!(form.paste.is_none());
        assert_eq!(form.draft.name, "Freshhero");
    }
}

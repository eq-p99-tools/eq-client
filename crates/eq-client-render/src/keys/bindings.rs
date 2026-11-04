//! Editing the Keyboard page and keeping assignments per character.
use super::{Act, Chord, KeyMap, Typing, key_name, scan_key};
use crate::profile_files::{CharacterProfile, Store};
use bevy::{input::keyboard::KeyboardInput, prelude::*, window::PrimaryWindow};
use std::fmt::Write as _;
use std::time::Instant;

/// A primary or alternate key cell in the Keyboard page.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Assignment {
    pub(crate) act: Act,
    pub(crate) slot: usize,
}

/// The cell waiting for a key, and the store for the assignments.
#[derive(Resource, Default)]
pub(crate) struct Bindings {
    pub(crate) active: Option<Assignment>,
    store: Store,
}

// These, together with the scan table, cover the ordinary keyboard. Modifier
// keys alone do not bind; they are read as part of the next key's chord.
const EXTRA: [KeyCode; 15] = [
    KeyCode::F13,
    KeyCode::F14,
    KeyCode::F15,
    KeyCode::F16,
    KeyCode::F17,
    KeyCode::F18,
    KeyCode::F19,
    KeyCode::F20,
    KeyCode::F21,
    KeyCode::F22,
    KeyCode::F23,
    KeyCode::F24,
    KeyCode::Pause,
    KeyCode::PrintScreen,
    KeyCode::ScrollLock,
];

fn bindable() -> impl Iterator<Item = KeyCode> {
    (0..=255).filter_map(scan_key).chain(EXTRA)
}

impl Chord {
    fn read(text: &str) -> Option<Self> {
        let mut chord = Self::key(KeyCode::Space);
        let mut parts = text.split('+').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                chord.key = bindable().find(|key| key_name(*key) == part)?;
                return Some(chord);
            }
            match part {
                "Ctrl" => chord.ctrl = true,
                "Shift" => chord.shift = true,
                "Alt" => chord.alt = true,
                _ => return None,
            }
        }
        None
    }
}

impl KeyMap {
    fn assign(&mut self, cell: Assignment, chord: Option<Chord>) {
        let chords = self.0.entry(cell.act).or_default();
        if cell.slot < chords.len() {
            chords.remove(cell.slot);
        }
        if let Some(chord) = chord {
            // A chord has one owner, including when it changes hands.
            for chords in self.0.values_mut() {
                chords.retain(|bound| *bound != chord);
            }
            let chords = self.0.entry(cell.act).or_default();
            chords.insert(cell.slot.min(chords.len()), chord);
        }
    }

    fn text(&self) -> String {
        let mut text = String::from("# eq-client keys v1\n");
        for (act, chords) in self.assignments() {
            let labels = chords.iter().map(|chord| chord.label()).collect::<Vec<_>>();
            writeln!(text, "{act:?} = {}", labels.join(", ")).expect("write to String");
        }
        text
    }

    fn read(text: &str, mut defaults: Self) -> Self {
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let act = defaults
                .0
                .keys()
                .find(|act| format!("{act:?}") == name.trim())
                .copied();
            let Some(act) = act else { continue };
            let value = value.trim();
            let chords = if value.is_empty() {
                Some(Vec::new())
            } else {
                value
                    .split(',')
                    .map(|part| Chord::read(part.trim()))
                    .collect::<Option<Vec<_>>>()
            };
            if let Some(chords) = chords.filter(|chords| chords.len() <= 2) {
                defaults.0.insert(act, Vec::new());
                for (slot, chord) in chords.into_iter().enumerate() {
                    defaults.assign(Assignment { act, slot }, Some(chord));
                }
            }
        }
        defaults
    }
}

fn file_name(profile: Option<&CharacterProfile>) -> String {
    crate::profile_files::name("keys", "keys.txt", profile)
}

/// Loads the current character's keys, and saves edits through the shared
/// atomic settings store, including its retry behavior.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut bindings: ResMut<Bindings>,
    mut map: ResMut<KeyMap>,
) {
    let changed = map.is_changed();
    let state = bindings.bypass_change_detection();
    let now = Instant::now();
    state.store.retry(now);
    if state.store.loaded && state.store.profile == profile.key() {
        if changed {
            state
                .store
                .save(file_name(state.store.profile.as_ref()), map.text(), now);
        }
        return;
    }
    if state.store.loaded {
        let name = file_name(state.store.profile.as_ref());
        state.store.save(name, map.text(), now);
    } else {
        state.store.open(settings.0.settings_directory.as_ref());
    }
    state.store.profile = profile.key();
    state.active = None;
    let text = state
        .store
        .read(&file_name(state.store.profile.as_ref()))
        .or_else(|| state.store.read("keys.txt"))
        .unwrap_or_default();
    *map = KeyMap::read(&text, KeyMap::for_client(settings.0.installed_client));
    state.store.loaded_text(map.text());
}

/// Captures before chat and gameplay, so the assigned key cannot also act.
/// Escape cancels, Delete clears, and losing focus or closing Options cancels.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn capture(
    clicks: crate::clicks::Clicks<Assignment>,
    mut bindings: ResMut<Bindings>,
    mut map: ResMut<KeyMap>,
    mut typing: ResMut<Typing>,
    mut input: ResMut<ButtonInput<KeyCode>>,
    presses: Res<super::presses::Presses>,
    mut events: Option<ResMut<Messages<KeyboardInput>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    shown: Res<crate::windows::Shown>,
    mut navigation: Option<ResMut<crate::navigation::NavigationKeys>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let clicked = clicks.iter().next().copied();
    if let Some(cell) = clicked {
        bindings.active = Some(cell);
        typing.composing = false;
        typing.counting = false;
    }
    typing.rebinding = bindings.active.is_some();
    let Some(cell) = bindings.active else { return };
    if let Some(navigation) = &mut navigation {
        **navigation = default();
    }
    if let Some(events) = &mut events {
        events.clear();
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    let open = shown.displayed(crate::windows::WindowId::Options);
    if !focused
        || !open
        || input.just_pressed(KeyCode::Escape)
        || (clicked.is_none() && mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]))
    {
        bindings.active = None;
        input.reset_all();
    } else if input.just_pressed(KeyCode::Delete) {
        map.assign(cell, None);
        bindings.active = None;
        input.reset_all();
    } else if let Some(key) = bindable().find(|key| input.just_pressed(*key)) {
        let chord = presses.chord(key, &input);
        map.assign(cell, Some(chord));
        bindings.active = None;
        input.reset_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> Assignment {
        Assignment {
            act: Act::FreeCamera,
            slot: 0,
        }
    }

    #[test]
    fn edited_keys_round_trip_and_move_conflicting_chords() {
        let mut map = KeyMap::default();
        map.assign(cell(), Some(Chord::key(KeyCode::KeyK)));
        assert_eq!(map.chords(Act::Consider), []);
        map.assign(
            Assignment { slot: 1, ..cell() },
            Some(Chord::ctrl_shift(KeyCode::F11)),
        );
        let text = map.text();
        let loaded = KeyMap::read(&text, KeyMap::default());
        assert_eq!(loaded.text(), text);
        assert_eq!(
            loaded.chords(Act::FreeCamera),
            [Chord::key(KeyCode::KeyK), Chord::ctrl_shift(KeyCode::F11)]
        );
        assert_eq!(loaded.chords(Act::Consider), []);
        let loaded = KeyMap::read(
            "FreeCamera = UnknownKey\nCameraUp = Ctrl+PageUp\nFutureAction = F2",
            loaded,
        );
        assert_eq!(loaded.label(Act::FreeCamera), "K");
        assert_eq!(loaded.label(Act::CameraUp), "Ctrl+PageUp");
    }

    #[test]
    fn saved_mac_bindings_override_new_starting_keys_including_cleared_actions() {
        let saved = "Attack = G, Alt+Q\nConsider = K\nDuck = C\nTurnLeft = Q\nStand = \n";
        let defaults = || KeyMap::for_client(eq_client_assets::ui::InstalledClient::EqMac);
        let loaded = KeyMap::read(saved, defaults());
        assert_eq!(
            loaded.chords(Act::Attack),
            [Chord::key(KeyCode::KeyG), Chord::alt(KeyCode::KeyQ)]
        );
        assert_eq!(loaded.chords(Act::Consider), [Chord::key(KeyCode::KeyK)]);
        assert_eq!(loaded.chords(Act::Duck), [Chord::key(KeyCode::KeyC)]);
        assert_eq!(loaded.chords(Act::TurnLeft), [Chord::key(KeyCode::KeyQ)]);
        assert_eq!(loaded.chords(Act::Stand), []);
        let text = loaded.text();
        assert_eq!(KeyMap::read(&text, defaults()).text(), text);
    }

    #[test]
    fn a_clicked_assignment_captures_a_chord_without_gameplay_then_can_cancel_or_clear() {
        let mut app = crate::testing::app();
        app.add_systems(Update, capture);
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Options);
        let button = app.world_mut().spawn((Interaction::None, cell())).id();
        crate::clicks::testing::click(&mut app, button);
        assert_eq!(app.world().resource::<Bindings>().active, Some(cell()));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
        }
        app.update();
        assert_eq!(app.world().resource::<Bindings>().active, Some(cell()));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyK);
        app.update();
        assert_eq!(
            app.world().resource::<KeyMap>().label(Act::FreeCamera),
            "Ctrl+K"
        );
        assert!(app.world().resource::<Typing>().rebinding);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyK)
        );
        app.update();
        assert!(!app.world().resource::<Typing>().rebinding);
        crate::clicks::testing::click(&mut app, button);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            app.world().resource::<KeyMap>().label(Act::FreeCamera),
            "Ctrl+K"
        );
        assert!(app.world().resource::<Bindings>().active.is_none());
        crate::clicks::testing::click(&mut app, button);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        assert_eq!(app.world().resource::<KeyMap>().chords(Act::FreeCamera), []);
    }

    #[test]
    fn a_complete_native_chord_is_assigned_with_its_original_modifiers() {
        let mut app = crate::testing::app();
        app.add_message::<KeyboardInput>()
            .add_systems(Update, (super::super::presses::capture, capture).chain());
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Options);
        app.world_mut().resource_mut::<Bindings>().active = Some(cell());
        app.world_mut()
            .write_message_batch(super::super::presses::tests::chord_events(
                KeyCode::KeyK,
                "k",
            ));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyK);
            keys.release(KeyCode::KeyK);
        }
        app.update();
        assert_eq!(
            app.world().resource::<KeyMap>().label(Act::FreeCamera),
            "Ctrl+K"
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::KeyK)
        );
        assert!(app.world().resource::<Typing>().rebinding);
    }

    #[test]
    fn bindings_are_saved_for_each_character_and_loaded_on_restart() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-camera-keys-{}", std::process::id()));
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.clone());
        app.add_systems(Update, (crate::profile_files::follow, persist).chain());
        let enter = |app: &mut App, name: &str| {
            let mut online = crate::online::OnlineState::new(true);
            crate::online::testing::news(
                &mut online,
                [eq_client_core::WorldEvent::WorldName {
                    short_name: "ExampleWorld".into(),
                }],
            );
            let mut player = crate::online::testing::player(7);
            player.name = name.into();
            crate::online::testing::admit(&mut online, 1, player);
            app.insert_resource(online);
            app.update();
        };
        enter(&mut app, "Example");
        app.world_mut()
            .resource_mut::<KeyMap>()
            .assign(cell(), Some(Chord::alt(KeyCode::F8)));
        app.update();
        assert!(
            std::fs::read_to_string(directory.join("keys-ExampleWorld-Example.txt"))
                .unwrap()
                .contains("FreeCamera = Alt+F8")
        );
        enter(&mut app, "Another");
        assert_eq!(
            app.world().resource::<KeyMap>().label(Act::FreeCamera),
            "F9"
        );
        app.insert_resource(Bindings::default());
        enter(&mut app, "Example");
        assert_eq!(
            app.world().resource::<KeyMap>().label(Act::FreeCamera),
            "Alt+F8"
        );
        std::fs::remove_file(directory.join("keys-ExampleWorld-Example.txt")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}

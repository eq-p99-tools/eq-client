//! Explicit background ownership of the existing in-client input paths.

use std::collections::HashSet;

use bevy::{
    ecs::message::MessageId,
    input::{
        keyboard::{KeyboardFocusLost, KeyboardInput},
        mouse::{MouseButtonInput, MouseWheel},
    },
    prelude::*,
    window::{PrimaryWindow, WindowFocused},
};

/// How a script shares the primary window with genuine user input.
///
/// This resource also records the launch policy between runs. Creating a new
/// [`super::Script`] still requires explicitly applying it with `input_mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Resource)]
pub enum InputMode {
    /// Require focus on public servers; retain offline/local test behavior.
    #[default]
    Attended,
    /// Yield to genuine keyboard, button or wheel input. Passive window focus
    /// and pointer presence do not interrupt a background run.
    Background,
}

/// Origins for this frame's synthesized keyboard and pointer input.
#[derive(Default, Resource)]
pub(crate) struct AuthoredInput {
    pub(super) typed: HashSet<MessageId<KeyboardInput>>,
    pub(crate) keys: HashSet<KeyCode>,
    pub(crate) left_button: bool,
    /// Every key the script sent a press or a release of this frame.
    pub(super) sent: HashSet<KeyCode>,
}

/// The script sets its keys' state itself and sends the typed keys after
/// Bevy has read the frame's keyboard messages, so Bevy reads them again a
/// frame later. Each key the script sent keeps the state it had before that
/// second reading, so a key a text field took back, such as a digit typed
/// in the chat line, is not pressed afresh for the game.
#[derive(Default, Resource)]
struct Replayed {
    /// The keys sent last frame, and whether each was down before Bevy read them.
    keys: Vec<(KeyCode, bool)>,
}

/// Notes the state of last frame's sent keys before Bevy reads them again.
#[allow(clippy::needless_pass_by_value)]
fn before_replay(
    authored: Res<AuthoredInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut replayed: ResMut<Replayed>,
) {
    replayed.keys = authored
        .sent
        .iter()
        .map(|key| (*key, keys.pressed(*key)))
        .collect();
}

/// Sets last frame's sent keys back as they were before Bevy read them again.
fn after_replay(mut replayed: ResMut<Replayed>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    for (key, down) in replayed.keys.drain(..) {
        keys.reset(key);
        if down {
            keys.press(key);
            keys.clear_just_pressed(key);
        }
    }
}

impl AuthoredInput {
    pub(crate) fn typed(&self, id: MessageId<KeyboardInput>) -> bool {
        self.typed.contains(&id)
    }
}

/// Native activity is observed separately from the script's typed messages.
/// The authored IDs live for one frame, not for the lifetime of a followed run.
#[derive(Default, Resource)]
pub(super) struct BackgroundInput {
    authored: HashSet<MessageId<KeyboardInput>>,
    pub(super) keys: HashSet<KeyCode>,
    pub(super) buttons: HashSet<MouseButton>,
    activity: bool,
}

impl BackgroundInput {
    pub(super) fn authored(&mut self, id: MessageId<KeyboardInput>) {
        self.authored.insert(id);
    }

    pub(super) fn busy(&self) -> bool {
        self.activity || !self.keys.is_empty() || !self.buttons.is_empty()
    }
}

/// Only an explicitly enabled process observes background input ownership.
pub(crate) fn install(app: &mut App, mode: InputMode) {
    app.insert_resource(mode)
        .init_resource::<AuthoredInput>()
        .init_resource::<Replayed>()
        .add_systems(
            PreUpdate,
            (
                before_replay.before(bevy::input::InputSystems),
                after_replay
                    .after(bevy::input::InputSystems)
                    .before(super::drive),
            ),
        );
    if mode == InputMode::Background {
        app.init_resource::<BackgroundInput>()
            .configure_sets(PreUpdate, bevy::ui::UiSystems::Focus.run_if(native_focus))
            .add_systems(
                PreUpdate,
                (
                    observe
                        .after(bevy::input::InputSystems)
                        .before(crate::camera::look)
                        .before(bevy::ui::UiSystems::Focus),
                    isolate_ui
                        .after(bevy::ui::UiSystems::Focus)
                        .before(crate::items::link_input)
                        .before(super::drive),
                ),
            );
    }
}

/// Physical UI hit testing resumes before the frame that takes native input.
#[allow(clippy::needless_pass_by_value)]
fn native_focus(script: Option<Res<super::Script>>) -> bool {
    !script.is_some_and(|script| script.background_owns_input())
}

/// The semantic script pointer owns UI interaction while native input is idle.
/// Keep its held/hovered controls stable, so [`crate::clicks::OnPress`] does not repeat each
/// frame. Clear cached physical hover without changing the Window or OS cursor.
#[allow(clippy::needless_pass_by_value)]
fn isolate_ui(
    script: Option<Res<super::Script>>,
    mut controls: Query<(
        Entity,
        &mut Interaction,
        Option<&mut bevy::ui::RelativeCursorPosition>,
    )>,
) {
    let Some(script) = script.filter(|script| script.background_owns_input()) else {
        return;
    };
    for (entity, mut interaction, relative) in &mut controls {
        let pressed = script.clicked.is_some() && script.pressed == Some(entity);
        let hovered = script.hovering == Some(entity) || script.pointed == Some(entity);
        if !pressed && !hovered {
            interaction.set_if_neq(Interaction::None);
        }
        if !pressed && let Some(mut relative) = relative {
            relative.set_if_neq(bevy::ui::RelativeCursorPosition::default());
        }
    }
}

/// Reads ordinary backend messages without consuming them for other readers.
/// No global device hooks, pointer movement or focus requests are involved.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn observe(
    mut state: ResMut<BackgroundInput>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut mouse: MessageReader<MouseButtonInput>,
    mut wheel: MessageReader<MouseWheel>,
    mut focus: MessageReader<WindowFocused>,
    mut lost: MessageReader<KeyboardFocusLost>,
    script: Option<ResMut<super::Script>>,
) {
    state.activity = false;
    for event in focus.read().filter(|event| windows.contains(event.window)) {
        if !event.focused {
            state.keys.clear();
            state.buttons.clear();
        }
    }
    if !lost.is_empty() {
        state.keys.clear();
        lost.clear();
    }
    for (event, id) in keyboard.read_with_id() {
        // Bevy's ButtonInput is shared by the app's windows. Genuine events
        // from another app window must yield ownership as well.
        if state.authored.remove(&id) {
            continue;
        }
        state.activity = true;
        if event.state.is_pressed() {
            state.keys.insert(event.key_code);
        } else {
            state.keys.remove(&event.key_code);
        }
    }
    state.authored.clear();
    for event in mouse.read() {
        state.activity = true;
        if event.state.is_pressed() {
            state.buttons.insert(event.button);
        } else {
            state.buttons.remove(&event.button);
        }
    }
    state.activity |= wheel.read().count() != 0;
    if let Some(mut script) = script {
        script.owns_input = script.input_mode == InputMode::Background
            && script.progress.phase.active()
            && !matches!(script.mode, super::RunMode::Paused(_))
            && !state.busy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{ButtonState, InputSystems, keyboard::Key};

    /// A key the script typed, and a text field then took back, stays let
    /// go when Bevy reads the script's message again a frame later.
    #[test]
    fn a_key_the_script_sent_is_not_pressed_again_when_bevy_reads_it() {
        #[derive(Resource)]
        struct Typing(bool);
        fn drive(
            mut typing: ResMut<Typing>,
            mut authored: ResMut<AuthoredInput>,
            mut keys: ResMut<ButtonInput<KeyCode>>,
            mut sent: MessageWriter<KeyboardInput>,
        ) {
            *authored = AuthoredInput::default();
            if std::mem::take(&mut typing.0) {
                keys.press(KeyCode::Digit9);
                let id = sent.write(KeyboardInput {
                    key_code: KeyCode::Digit9,
                    logical_key: Key::Character("9".into()),
                    state: ButtonState::Pressed,
                    text: Some("9".into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                authored.typed.insert(id);
                authored.sent.insert(KeyCode::Digit9);
            }
        }
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .add_message::<KeyboardFocusLost>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<Key>>()
            .insert_resource(Typing(true))
            .add_systems(
                PreUpdate,
                (
                    bevy::input::keyboard::keyboard_input_system.in_set(InputSystems),
                    drive.after(after_replay),
                ),
            )
            // The chat line takes the key back on the frame it is typed, as
            // `chat::capture` does, and only then.
            .add_systems(
                Update,
                |mut keys: ResMut<ButtonInput<KeyCode>>, mut taken: Local<bool>| {
                    if !std::mem::replace(&mut *taken, true) {
                        keys.reset(KeyCode::Digit9);
                    }
                },
            );
        install(&mut app, InputMode::Attended);
        app.update();
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.just_pressed(KeyCode::Digit9));
        assert!(!keys.pressed(KeyCode::Digit9));
    }
}

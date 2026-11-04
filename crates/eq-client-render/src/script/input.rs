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
    /// Run only while the primary window and pointer are free of user input.
    /// Returning to the window cancels any held synthetic input.
    Background,
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
    app.insert_resource(mode);
    if mode == InputMode::Background {
        app.init_resource::<BackgroundInput>().add_systems(
            PreUpdate,
            observe
                .after(bevy::input::InputSystems)
                .before(super::drive),
        );
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
}

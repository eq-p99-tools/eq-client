//! Shared character movement input; camera and UI actions do not take a walk.
use super::{Act, KeyMap};
use bevy::prelude::*;

/// Callers supply native keys only after text ownership, or explicit script acts.
/// Steering means an acquired character look, not a camera-only drag or UI click.
pub(crate) fn requested(
    map: &KeyMap,
    keys: &ButtonInput<KeyCode>,
    scripted: &[Act],
    steering: bool,
) -> bool {
    steering
        || [
            Act::Forward,
            Act::Back,
            Act::StrafeLeft,
            Act::StrafeRight,
            Act::CameraForward,
            Act::CameraBack,
            Act::CameraLeft,
            Act::CameraRight,
            Act::TurnLeft,
            Act::TurnRight,
            Act::Jump,
            Act::Duck,
            Act::Walk,
            Act::Autorun,
        ]
        .into_iter()
        .any(|act| map.holding(keys, scripted).held(act))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_input_6721_remapped_character_acts_and_steering_share_one_table() {
        for act in [
            Act::Forward,
            Act::Back,
            Act::StrafeLeft,
            Act::StrafeRight,
            Act::CameraForward,
            Act::CameraBack,
            Act::CameraLeft,
            Act::CameraRight,
            Act::TurnLeft,
            Act::TurnRight,
            Act::Jump,
            Act::Duck,
            Act::Walk,
            Act::Autorun,
        ] {
            let map = crate::keys::testing::map(&[(act, super::super::Chord::key(KeyCode::F12))]);
            let mut keys = ButtonInput::default();
            assert!(!requested(&map, &keys, &[], false));
            keys.press(KeyCode::F12);
            assert!(requested(&map, &keys, &[], false), "{act:?}");
            keys.reset_all();
            assert!(requested(&map, &keys, &[act], false), "script {act:?}");
        }
        assert!(requested(
            &KeyMap::default(),
            &ButtonInput::default(),
            &[],
            true
        ));
    }

    #[test]
    fn movement_input_6721_camera_ui_chat_and_combat_actions_do_not_move_the_character() {
        for act in [
            Act::FreeCamera,
            Act::CameraUp,
            Act::CameraDown,
            Act::LookUp,
            Act::LookDown,
            Act::CenterView,
            Act::Attack,
            Act::Hail,
            Act::Slot(0),
            Act::Toggle(crate::windows::WindowId::Inventory),
        ] {
            let map = crate::keys::testing::map(&[(act, super::super::Chord::key(KeyCode::F12))]);
            let mut keys = ButtonInput::default();
            keys.press(KeyCode::F12);
            assert!(!requested(&map, &keys, &[], false), "{act:?}");
        }
    }
}

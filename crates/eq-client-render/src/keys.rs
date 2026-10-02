//! The client's key bindings: one table of what each key does, read by every
//! system that takes keys, with the hints written from it. As in the official
//! client's key map, an action may have a primary and an alternate key.
//!
//! One modifier rule holds for every action: a chord matches only with
//! exactly its modifiers held, so Alt+1 casts the first gem while 1 uses the
//! first action slot, and Ctrl+1 binds that slot. Movement is the exception:
//! a movement key moves whatever is held with it.
use super::windows::WindowId;
use bevy::{prelude::*, window::PrimaryWindow};
use std::collections::BTreeMap;

/// Something a key press asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Act {
    /// Opens or closes a window.
    Toggle(WindowId),
    TargetSelf,
    TargetNext,
    TargetPrevious,
    Consider,
    Hail,
    Attack,
    Loot,
    Trade,
    /// Uses the door or picks up the item in reach.
    Use,
    Sit,
    Duck,
    Stand,
    /// Casts the spell in this gem, from 0.
    Gem(u8),
    /// Uses this action slot, from 0.
    Slot(u8),
    /// Binds the hovered gem or item to this action slot.
    BindSlot(u8),
    /// Empties this action slot.
    ClearSlot(u8),
    /// Character-relative moves.
    Forward,
    Back,
    StrafeLeft,
    StrafeRight,
    /// Camera-relative moves.
    CameraForward,
    CameraBack,
    CameraLeft,
    CameraRight,
    TurnLeft,
    TurnRight,
    Jump,
    /// Switches between walking and running.
    Walk,
}

/// The groups the Options window's key list filters by, as the official
/// client's filter names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyGroup {
    Movement,
    Commands,
    SpellCasting,
    Target,
    Camera,
    Chat,
    Ui,
    /// One of the hotbars, from 1.
    Hotbar(u8),
}

impl Act {
    /// What the Options window's key list calls it.
    pub(crate) fn command(self) -> String {
        let fixed = match self {
            Self::Toggle(WindowId::Inventory) => "Inventory Window",
            Self::Toggle(WindowId::Spellbook) => "Spellbook",
            Self::Toggle(WindowId::Options) => "Options Window",
            Self::Toggle(window) => return format!("{window:?} Window"),
            Self::TargetSelf => "Target Self",
            Self::TargetNext => "Target Next",
            Self::TargetPrevious => "Target Previous",
            Self::Consider => "Consider",
            Self::Hail => "Hail",
            Self::Attack => "Auto Attack",
            Self::Loot => "Loot",
            Self::Trade => "Trade",
            Self::Use => "Use Door or Item",
            Self::Sit => "Sit",
            Self::Duck => "Duck",
            Self::Stand => "Stand",
            Self::Gem(gem) => return format!("Cast Spell {}", gem + 1),
            Self::Slot(slot) => return format!("Hotbar Button {}", slot + 1),
            Self::BindSlot(slot) => return format!("Bind Hotbar Button {}", slot + 1),
            Self::ClearSlot(slot) => return format!("Clear Hotbar Button {}", slot + 1),
            Self::Forward => "Forward",
            Self::Back => "Back",
            Self::StrafeLeft => "Strafe Left",
            Self::StrafeRight => "Strafe Right",
            Self::CameraForward => "Forward, Facing the Camera",
            Self::CameraBack => "Back, Facing the Camera",
            Self::CameraLeft => "Left, Facing the Camera",
            Self::CameraRight => "Right, Facing the Camera",
            Self::TurnLeft => "Turn Left",
            Self::TurnRight => "Turn Right",
            Self::Jump => "Jump",
            Self::Walk => "Walk or Run",
        };
        fixed.to_owned()
    }

    /// The key list's group for it.
    pub(crate) const fn group(self) -> KeyGroup {
        match self {
            Self::Toggle(_) => KeyGroup::Ui,
            Self::TargetSelf | Self::TargetNext | Self::TargetPrevious => KeyGroup::Target,
            Self::Consider | Self::Hail | Self::Attack | Self::Loot | Self::Trade | Self::Use => {
                KeyGroup::Commands
            }
            Self::Gem(_) => KeyGroup::SpellCasting,
            Self::Slot(_) | Self::BindSlot(_) | Self::ClearSlot(_) => KeyGroup::Hotbar(1),
            Self::Sit
            | Self::Duck
            | Self::Stand
            | Self::Forward
            | Self::Back
            | Self::StrafeLeft
            | Self::StrafeRight
            | Self::CameraForward
            | Self::CameraBack
            | Self::CameraLeft
            | Self::CameraRight
            | Self::TurnLeft
            | Self::TurnRight
            | Self::Jump
            | Self::Walk => KeyGroup::Movement,
        }
    }
}

/// A key with exactly these modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chord {
    pub key: KeyCode,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub(crate) const fn key(key: KeyCode) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub(crate) const fn alt(key: KeyCode) -> Self {
        Self {
            alt: true,
            ..Self::key(key)
        }
    }

    pub(crate) const fn ctrl(key: KeyCode) -> Self {
        Self {
            ctrl: true,
            ..Self::key(key)
        }
    }

    pub(crate) const fn shift(key: KeyCode) -> Self {
        Self {
            shift: true,
            ..Self::key(key)
        }
    }

    pub(crate) const fn ctrl_shift(key: KeyCode) -> Self {
        Self {
            ctrl: true,
            shift: true,
            ..Self::key(key)
        }
    }

    /// The chord as a hint writes it, such as "Alt+1" or "K".
    pub(crate) fn label(self) -> String {
        let mut label = String::new();
        for (held, name) in [
            (self.ctrl, "Ctrl+"),
            (self.shift, "Shift+"),
            (self.alt, "Alt+"),
        ] {
            if held {
                label.push_str(name);
            }
        }
        label.push_str(&key_name(self.key));
        label
    }

    /// Whether exactly this chord's modifiers are held.
    fn modifiers_held(self, keys: &ButtonInput<KeyCode>) -> bool {
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        (ctrl, shift, alt) == (self.ctrl, self.shift, self.alt)
    }
}

/// A key as hints show it: `KeyB` as "B", `Digit1` as "1", `ArrowUp` as "Up".
pub(crate) fn key_name(key: KeyCode) -> String {
    let name = format!("{key:?}");
    ["Key", "Digit", "Arrow"]
        .into_iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(&name)
        .to_owned()
}

/// The number keys from 1 to 0, as the action slots and gems number them.
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

/// Every action's keys: a primary and, for some, an alternate.
#[derive(Resource, Clone, Debug)]
pub(crate) struct KeyMap(BTreeMap<Act, Vec<Chord>>);

impl Default for KeyMap {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        let mut bind = |act, chords: &[Chord]| {
            map.insert(act, chords.to_vec());
        };
        bind(
            Act::Toggle(WindowId::Inventory),
            &[Chord::key(KeyCode::KeyI)],
        );
        bind(
            Act::Toggle(WindowId::Spellbook),
            &[Chord::key(KeyCode::KeyB)],
        );
        bind(Act::Toggle(WindowId::Options), &[Chord::alt(KeyCode::KeyO)]);
        bind(Act::TargetSelf, &[Chord::key(KeyCode::F1)]);
        bind(Act::TargetNext, &[Chord::key(KeyCode::Tab)]);
        bind(Act::TargetPrevious, &[Chord::shift(KeyCode::Tab)]);
        bind(Act::Consider, &[Chord::key(KeyCode::KeyK)]);
        bind(Act::Hail, &[Chord::key(KeyCode::KeyH)]);
        bind(Act::Attack, &[Chord::key(KeyCode::KeyG)]);
        bind(Act::Loot, &[Chord::key(KeyCode::KeyL)]);
        bind(Act::Trade, &[Chord::key(KeyCode::KeyU)]);
        bind(Act::Use, &[Chord::key(KeyCode::KeyF)]);
        bind(Act::Sit, &[Chord::key(KeyCode::KeyX)]);
        bind(Act::Duck, &[Chord::key(KeyCode::KeyC)]);
        bind(Act::Stand, &[Chord::key(KeyCode::KeyV)]);
        for (index, key) in (0..).zip(DIGITS) {
            if index < 8 {
                bind(Act::Gem(index), &[Chord::alt(key)]);
            }
            bind(Act::Slot(index), &[Chord::key(key)]);
            bind(Act::BindSlot(index), &[Chord::ctrl(key)]);
            bind(Act::ClearSlot(index), &[Chord::ctrl_shift(key)]);
        }
        bind(Act::Forward, &[Chord::key(KeyCode::ArrowUp)]);
        bind(Act::Back, &[Chord::key(KeyCode::ArrowDown)]);
        bind(Act::StrafeLeft, &[Chord::key(KeyCode::ArrowLeft)]);
        bind(Act::StrafeRight, &[Chord::key(KeyCode::ArrowRight)]);
        bind(Act::CameraForward, &[Chord::key(KeyCode::KeyW)]);
        bind(Act::CameraBack, &[Chord::key(KeyCode::KeyS)]);
        bind(Act::CameraLeft, &[Chord::key(KeyCode::KeyA)]);
        bind(Act::CameraRight, &[Chord::key(KeyCode::KeyD)]);
        bind(Act::TurnLeft, &[Chord::key(KeyCode::KeyQ)]);
        bind(Act::TurnRight, &[Chord::key(KeyCode::KeyE)]);
        bind(Act::Jump, &[Chord::key(KeyCode::Space)]);
        bind(Act::Walk, &[Chord::key(KeyCode::Insert)]);
        Self(map)
    }
}

impl KeyMap {
    /// Every action with its keys, in the key map's order.
    pub(crate) fn assignments(&self) -> impl Iterator<Item = (Act, &[Chord])> {
        self.0.iter().map(|(act, chords)| (*act, chords.as_slice()))
    }

    /// The chords bound to an action.
    pub(crate) fn chords(&self, act: Act) -> &[Chord] {
        self.0.get(&act).map_or(&[], Vec::as_slice)
    }

    /// The keys bound to an action, whatever their modifiers.
    pub(crate) fn keys(&self, act: Act) -> impl Iterator<Item = KeyCode> + '_ {
        self.chords(act).iter().map(|chord| chord.key)
    }

    /// The action's primary chord as a hint writes it, or nothing if unbound.
    pub(crate) fn label(&self, act: Act) -> String {
        self.chords(act)
            .first()
            .map_or_else(String::new, |chord| chord.label())
    }

    /// The action named with its key, as "open the spellbook [B]", or just
    /// the name if it is unbound.
    pub(crate) fn named(&self, act: Act, what: &str) -> String {
        let label = self.label(act);
        if label.is_empty() {
            what.to_owned()
        } else {
            format!("{what} [{label}]")
        }
    }

    /// Key help for these actions, one way for every window: "K: consider |
    /// G: attack". Unbound actions are left out.
    pub(crate) fn help(&self, acts: &[(Act, &str)]) -> String {
        acts.iter()
            .filter_map(|(act, what)| {
                let label = self.label(*act);
                (!label.is_empty()).then(|| format!("{label}: {what}"))
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Whether a fresh press of one of the action's chords, with exactly its
    /// modifiers, is in these keys.
    pub(crate) fn just_pressed(&self, keys: &ButtonInput<KeyCode>, act: Act) -> bool {
        self.chords(act)
            .iter()
            .any(|chord| keys.just_pressed(chord.key) && chord.modifiers_held(keys))
    }

    /// Whether one of the action's keys is held, whatever is held with it.
    pub(crate) fn held(&self, keys: &ButtonInput<KeyCode>, act: Act) -> bool {
        self.keys(act).any(|key| keys.pressed(key))
    }

    /// -1, 0 or 1 as the negative or positive action is held, or both or neither.
    pub(crate) fn axis(&self, keys: &ButtonInput<KeyCode>, negative: Act, positive: Act) -> f32 {
        f32::from(self.held(keys, positive)) - f32::from(self.held(keys, negative))
    }

    /// The keys that move the player, whose short taps are kept until the
    /// next movement sample.
    pub(crate) fn movement_keys(&self) -> Vec<KeyCode> {
        [
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
        ]
        .into_iter()
        .flat_map(|act| self.keys(act).collect::<Vec<_>>())
        .collect()
    }
}

/// Whether the chat has the keyboard. Only the chat's input changes it;
/// everything else that takes keys asks it, through [`Keys`].
#[derive(Resource, Default, Debug)]
pub(crate) struct Typing {
    /// The player is typing a chat line.
    pub composing: bool,
    /// This frame's Escape closed the chat line, so nothing else takes it.
    pub escape_consumed: bool,
}

/// The keyboard as the game sees it: the bindings, and whether the game has
/// the keyboard at all (the window has focus and the chat is not being
/// typed in). Systems that take keys read them through this.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Keys<'w, 's> {
    /// The raw keyboard, for keys outside the map such as Escape and Shift-click.
    pub(crate) input: Res<'w, ButtonInput<KeyCode>>,
    pub(crate) map: Res<'w, KeyMap>,
    typing: Res<'w, Typing>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl Keys<'_, '_> {
    /// Whether the window has focus. Clicks count from then on, though the
    /// chat may still have the keyboard.
    pub(crate) fn window_focused(&self) -> bool {
        self.windows.single().is_ok_and(|window| window.focused)
    }

    /// Whether the game has the keyboard.
    pub(crate) fn focused(&self) -> bool {
        !self.typing.composing && self.window_focused()
    }

    /// Whether this frame's Escape belongs to the game, not to closing the chat.
    pub(crate) fn escape_free(&self) -> bool {
        self.focused() && !self.typing.escape_consumed
    }

    /// A fresh press of the action, while the game has the keyboard.
    pub(crate) fn pressed(&self, act: Act) -> bool {
        self.focused() && self.map.just_pressed(&self.input, act)
    }

    /// The first of these actions freshly pressed, while the game has the
    /// keyboard.
    pub(crate) fn first(&self, acts: impl IntoIterator<Item = Act>) -> Option<Act> {
        acts.into_iter().find(|act| self.pressed(*act))
    }
}

/// The keyboard for hand-built test apps.
#[cfg(test)]
pub(crate) mod testing {
    use bevy::prelude::*;

    /// Gives a hand-built test app the bindings and the chat's hold on the
    /// keyboard, which every system that takes keys reads.
    pub(crate) fn install(app: &mut App) {
        app.init_resource::<super::KeyMap>()
            .init_resource::<super::Typing>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(held: &[KeyCode], pressed: KeyCode) -> ButtonInput<KeyCode> {
        let mut keys = ButtonInput::default();
        for key in held {
            keys.press(*key);
        }
        keys.press(pressed);
        keys
    }

    #[test]
    fn a_chord_matches_only_with_exactly_its_modifiers() {
        let map = KeyMap::default();
        let one = keys(&[], KeyCode::Digit1);
        let alt_one = keys(&[KeyCode::AltLeft], KeyCode::Digit1);
        let ctrl_one = keys(&[KeyCode::ControlRight], KeyCode::Digit1);
        let ctrl_shift_one = keys(&[KeyCode::ControlLeft, KeyCode::ShiftLeft], KeyCode::Digit1);
        assert!(map.just_pressed(&one, Act::Slot(0)));
        assert!(!map.just_pressed(&one, Act::Gem(0)));
        assert!(map.just_pressed(&alt_one, Act::Gem(0)));
        assert!(!map.just_pressed(&alt_one, Act::Slot(0)));
        assert!(map.just_pressed(&ctrl_one, Act::BindSlot(0)));
        assert!(!map.just_pressed(&ctrl_shift_one, Act::BindSlot(0)));
        assert!(map.just_pressed(&ctrl_shift_one, Act::ClearSlot(0)));
        let tab = keys(&[KeyCode::ShiftRight], KeyCode::Tab);
        assert!(map.just_pressed(&tab, Act::TargetPrevious));
        assert!(!map.just_pressed(&tab, Act::TargetNext));
    }

    #[test]
    fn movement_keys_move_whatever_is_held_with_them() {
        let map = KeyMap::default();
        let walking = keys(&[KeyCode::ShiftLeft], KeyCode::KeyW);
        assert!(map.held(&walking, Act::CameraForward));
    }

    #[test]
    fn hints_name_each_actions_key() {
        let map = KeyMap::default();
        assert_eq!(map.label(Act::Gem(0)), "Alt+1");
        assert_eq!(map.label(Act::ClearSlot(9)), "Ctrl+Shift+0");
        assert_eq!(
            map.help(&[(Act::Consider, "consider"), (Act::Attack, "attack")]),
            "K: consider | G: attack"
        );
        assert_eq!(
            map.named(Act::Toggle(WindowId::Spellbook), "Spellbook"),
            "Spellbook [B]"
        );
        assert_eq!(map.label(Act::Forward), "Up");
        assert_eq!(key_name(KeyCode::F1), "F1");
    }

    #[test]
    fn every_action_the_client_takes_has_a_key() {
        let map = KeyMap::default();
        for act in [
            Act::Toggle(WindowId::Inventory),
            Act::Toggle(WindowId::Spellbook),
            Act::TargetSelf,
            Act::Consider,
            Act::Use,
            Act::Gem(7),
            Act::Slot(9),
            Act::Jump,
            Act::Walk,
        ] {
            assert!(!map.chords(act).is_empty(), "{act:?}");
        }
    }

    #[test]
    fn key_help_stays_within_the_default_fonts_ascii() {
        let map = KeyMap::default();
        for act in map.0.keys() {
            let help = map.help(&[(*act, "do it")]);
            assert!(help.is_ascii(), "{help}");
            assert!(map.named(*act, "it").is_ascii());
        }
    }
}

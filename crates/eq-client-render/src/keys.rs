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

pub(crate) mod bindings;
pub(crate) mod modifiers;
pub(crate) mod presses;

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
    /// Detaches the view, or returns it to the player.
    FreeCamera,
    CameraUp,
    CameraDown,
    TurnLeft,
    TurnRight,
    Jump,
    /// Switches between walking and running.
    Walk,
    /// Runs forward until a forward or back key, this key again or a
    /// posture change stops it.
    Autorun,
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
            Self::FreeCamera => "Toggle Free Camera",
            Self::CameraUp => "Free Camera Up",
            Self::CameraDown => "Free Camera Down",
            Self::TurnLeft => "Turn Left",
            Self::TurnRight => "Turn Right",
            Self::Jump => "Jump",
            Self::Walk => "Walk or Run",
            Self::Autorun => "Autorun",
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
            Self::FreeCamera | Self::CameraUp | Self::CameraDown => KeyGroup::Camera,
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
            | Self::Walk
            | Self::Autorun => KeyGroup::Movement,
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
        bind(Act::Toggle(WindowId::Map), &[Chord::ctrl(KeyCode::KeyM)]);
        // As the official client's notes on raids say (`raidsdoc.txt`).
        bind(Act::Toggle(WindowId::Raid), &[Chord::alt(KeyCode::KeyR)]);
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
        bind(Act::FreeCamera, &[Chord::key(KeyCode::F9)]);
        bind(Act::CameraUp, &[Chord::key(KeyCode::PageUp)]);
        bind(Act::CameraDown, &[Chord::key(KeyCode::PageDown)]);
        bind(Act::TurnLeft, &[Chord::key(KeyCode::KeyQ)]);
        bind(Act::TurnRight, &[Chord::key(KeyCode::KeyE)]);
        bind(Act::Jump, &[Chord::key(KeyCode::Space)]);
        bind(Act::Walk, &[Chord::key(KeyCode::Insert)]);
        // Num Lock, as the Titanium program binds it; the Mac-era client's
        // key is unread (recording checklist 6.351).
        bind(Act::Autorun, &[Chord::key(KeyCode::NumLock)]);
        Self(map)
    }
}

/// The acts a client's built-in table leaves unbound where it binds its
/// own: the Titanium program's two strafe commands have no default key
/// (which is left and which right is unread, recording checklist 6.361),
/// and it has no camera-relative moves, which stay this client's to bind.
const UNBOUND_BY_TABLE: [Act; 6] = [
    Act::StrafeLeft,
    Act::StrafeRight,
    Act::CameraForward,
    Act::CameraBack,
    Act::CameraLeft,
    Act::CameraRight,
];

/// The act a program key command asks for, where this client has it.
const fn act_for(command: eq_client_assets::keys::Command) -> Option<Act> {
    use eq_client_assets::keys::Command;
    Some(match command {
        Command::Autorun => Act::Autorun,
        Command::Jump => Act::Jump,
        Command::Forward => Act::Forward,
        Command::Back => Act::Back,
        Command::TurnRight => Act::TurnRight,
        Command::TurnLeft => Act::TurnLeft,
        Command::WalkRun => Act::Walk,
        Command::AutoAttack => Act::Attack,
        Command::Consider => Act::Consider,
        Command::Duck => Act::Duck,
        Command::Hail => Act::Hail,
        Command::Inventory => Act::Toggle(WindowId::Inventory),
        Command::Use => Act::Use,
        Command::TargetSelf => Act::TargetSelf,
        Command::TargetNext => Act::TargetNext,
        // This client keeps eight gems.
        Command::Gem(gem) if gem < 8 => Act::Gem(gem),
        Command::Gem(_) => return None,
        Command::HotButton(slot) => Act::Slot(slot),
    })
}

/// The key a `DirectInput` scan code names, where it names one.
const fn scan_key(scan: u8) -> Option<KeyCode> {
    Some(match scan {
        0x01 => KeyCode::Escape,
        0x02 => KeyCode::Digit1,
        0x03 => KeyCode::Digit2,
        0x04 => KeyCode::Digit3,
        0x05 => KeyCode::Digit4,
        0x06 => KeyCode::Digit5,
        0x07 => KeyCode::Digit6,
        0x08 => KeyCode::Digit7,
        0x09 => KeyCode::Digit8,
        0x0a => KeyCode::Digit9,
        0x0b => KeyCode::Digit0,
        0x0c => KeyCode::Minus,
        0x0d => KeyCode::Equal,
        0x0e => KeyCode::Backspace,
        0x0f => KeyCode::Tab,
        0x10 => KeyCode::KeyQ,
        0x11 => KeyCode::KeyW,
        0x12 => KeyCode::KeyE,
        0x13 => KeyCode::KeyR,
        0x14 => KeyCode::KeyT,
        0x15 => KeyCode::KeyY,
        0x16 => KeyCode::KeyU,
        0x17 => KeyCode::KeyI,
        0x18 => KeyCode::KeyO,
        0x19 => KeyCode::KeyP,
        0x1a => KeyCode::BracketLeft,
        0x1b => KeyCode::BracketRight,
        0x1c => KeyCode::Enter,
        0x1e => KeyCode::KeyA,
        0x1f => KeyCode::KeyS,
        0x20 => KeyCode::KeyD,
        0x21 => KeyCode::KeyF,
        0x22 => KeyCode::KeyG,
        0x23 => KeyCode::KeyH,
        0x24 => KeyCode::KeyJ,
        0x25 => KeyCode::KeyK,
        0x26 => KeyCode::KeyL,
        0x27 => KeyCode::Semicolon,
        0x28 => KeyCode::Quote,
        0x29 => KeyCode::Backquote,
        0x2b => KeyCode::Backslash,
        0x2c => KeyCode::KeyZ,
        0x2d => KeyCode::KeyX,
        0x2e => KeyCode::KeyC,
        0x2f => KeyCode::KeyV,
        0x30 => KeyCode::KeyB,
        0x31 => KeyCode::KeyN,
        0x32 => KeyCode::KeyM,
        0x33 => KeyCode::Comma,
        0x34 => KeyCode::Period,
        0x35 => KeyCode::Slash,
        0x37 => KeyCode::NumpadMultiply,
        0x39 => KeyCode::Space,
        0x3b => KeyCode::F1,
        0x3c => KeyCode::F2,
        0x3d => KeyCode::F3,
        0x3e => KeyCode::F4,
        0x3f => KeyCode::F5,
        0x40 => KeyCode::F6,
        0x41 => KeyCode::F7,
        0x42 => KeyCode::F8,
        0x43 => KeyCode::F9,
        0x44 => KeyCode::F10,
        0x45 => KeyCode::NumLock,
        0x47 => KeyCode::Numpad7,
        0x48 => KeyCode::Numpad8,
        0x49 => KeyCode::Numpad9,
        0x4a => KeyCode::NumpadSubtract,
        0x4b => KeyCode::Numpad4,
        0x4c => KeyCode::Numpad5,
        0x4d => KeyCode::Numpad6,
        0x4e => KeyCode::NumpadAdd,
        0x4f => KeyCode::Numpad1,
        0x50 => KeyCode::Numpad2,
        0x51 => KeyCode::Numpad3,
        0x52 => KeyCode::Numpad0,
        0x53 => KeyCode::NumpadDecimal,
        0x57 => KeyCode::F11,
        0x58 => KeyCode::F12,
        0x9c => KeyCode::NumpadEnter,
        0xb5 => KeyCode::NumpadDivide,
        0xc7 => KeyCode::Home,
        0xc8 => KeyCode::ArrowUp,
        0xc9 => KeyCode::PageUp,
        0xcb => KeyCode::ArrowLeft,
        0xcd => KeyCode::ArrowRight,
        0xcf => KeyCode::End,
        0xd0 => KeyCode::ArrowDown,
        0xd1 => KeyCode::PageDown,
        0xd2 => KeyCode::Insert,
        0xd3 => KeyCode::Delete,
        _ => return None,
    })
}

impl KeyMap {
    /// The keys a fresh installation of `client` starts with: its program's
    /// built-in table where read, in which each command this client has
    /// takes the table's keys and nothing else; this client's own keys for
    /// the rest, less any key the table gives a command and less
    /// [`UNBOUND_BY_TABLE`]. Without a table, this client's own.
    pub(crate) fn for_client(client: eq_client_assets::ui::InstalledClient) -> Self {
        let mut map = Self::default();
        let Some(table) = client.default_keys() else {
            return map;
        };
        let mut read: BTreeMap<Act, Vec<Chord>> = BTreeMap::new();
        // Primaries first, then alternates, as the key list shows them.
        for alternate in [false, true] {
            for entry in table.iter().filter(|entry| entry.alternate == alternate) {
                let (Some(act), Some(key)) = (act_for(entry.command), scan_key(entry.key.scan))
                else {
                    continue;
                };
                read.entry(act).or_default().push(Chord {
                    key,
                    ctrl: entry.key.ctrl,
                    shift: entry.key.shift,
                    alt: entry.key.alt,
                });
            }
        }
        let taken: Vec<Chord> = read.values().flatten().copied().collect();
        for (act, chords) in &mut map.0 {
            if let Some(keys) = read.remove(act) {
                *chords = keys;
            } else if UNBOUND_BY_TABLE.contains(act) {
                chords.clear();
            } else {
                chords.retain(|chord| !taken.contains(chord));
            }
        }
        map.0.extend(read);
        map
    }
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
    pub(crate) fn just_pressed(
        &self,
        keys: &ButtonInput<KeyCode>,
        act: Act,
        presses: &presses::Presses,
    ) -> bool {
        self.chords(act)
            .iter()
            .any(|chord| presses.matches(*chord, keys))
    }

    /// Whether one of the action's keys is held, whatever is held with it.
    pub(crate) fn held(&self, keys: &ButtonInput<KeyCode>, act: Act) -> bool {
        self.keys(act).any(|key| keys.pressed(key))
    }

    /// -1, 0 or 1 as the negative or positive action is held, or both or neither.
    pub(crate) fn axis(&self, keys: &ButtonInput<KeyCode>, negative: Act, positive: Act) -> f32 {
        f32::from(self.held(keys, positive)) - f32::from(self.held(keys, negative))
    }

    /// What movement reads as held: these keys through this table, or the
    /// acts a script holds itself, whatever keys the table gives them.
    pub(crate) const fn holding<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
        scripted: &'a [Act],
    ) -> Holding<'a> {
        Holding {
            keys,
            map: self,
            scripted,
        }
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

/// The movement commands held this frame: an act is held when one of its
/// keys is, whatever is held with it, or when a script holds the act itself.
/// A script's moving steps act here, on the commands, never on a key, so
/// they move the same under every key table.
#[derive(Clone, Copy)]
pub(crate) struct Holding<'a> {
    keys: &'a ButtonInput<KeyCode>,
    map: &'a KeyMap,
    scripted: &'a [Act],
}

impl Holding<'_> {
    /// Whether the act is held, by a key or by a script.
    pub(crate) fn held(&self, act: Act) -> bool {
        self.map.held(self.keys, act) || self.scripted.contains(&act)
    }

    /// -1, 0 or 1 as the negative or positive act is held, or both or neither.
    pub(crate) fn axis(&self, negative: Act, positive: Act) -> f32 {
        f32::from(self.held(positive)) - f32::from(self.held(negative))
    }
}

/// Whether a box the player types in has the keyboard: the chat's line,
/// the quantity window's number, or the login screens. Only those change
/// it; everything else that takes keys asks it, through [`Keys`].
#[derive(Resource, Default, Debug)]
#[allow(clippy::struct_excessive_bools)] // Each box holds the keyboard on its own.
pub(crate) struct Typing {
    /// The player is typing a chat line.
    pub composing: bool,
    /// The player is typing a number in the quantity window.
    pub counting: bool,
    /// A login screen is up: the login window or the login server's list,
    /// whose boxes and keys are their own.
    pub logging_in: bool,
    /// The map's local entity search owns text input.
    pub map_search: bool,
    /// A key assignment is being edited, including its finishing frame.
    pub rebinding: bool,
    /// A script document or path field owns the keyboard.
    pub editing_script: bool,
    /// The character creation form owns the keyboard.
    pub creating_character: bool,
    /// This frame's Escape closed the chat line or left the quantity
    /// window's number, so nothing else takes it.
    pub escape_consumed: bool,
}

/// The keyboard as the game sees it: the bindings, and whether the game has
/// the keyboard at all (the window has focus, the chat is not being typed
/// in and no login screen is up). Systems that take keys read them through
/// this.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Keys<'w, 's> {
    /// The raw keyboard, for keys outside the map such as Escape and Shift-click.
    pub(crate) input: Res<'w, ButtonInput<KeyCode>>,
    pub(crate) map: Res<'w, KeyMap>,
    presses: Res<'w, presses::Presses>,
    typing: Res<'w, Typing>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    script: Option<Res<'w, crate::script::Script>>,
}

impl Keys<'_, '_> {
    /// Whether native focus or the current script's input policy owns the
    /// window. Background scripts qualify only after their per-frame native
    /// input guard succeeds; local/offline default behavior is retained.
    pub(crate) fn window_focused(&self) -> bool {
        self.windows.single().is_ok_and(|window| window.focused)
            || self
                .script
                .as_ref()
                .is_some_and(|script| script.unattended())
    }

    /// Whether the game has the keyboard.
    pub(crate) fn focused(&self) -> bool {
        !self.typing.composing
            && !self.typing.creating_character
            && !self.typing.editing_script
            && !self.typing.counting
            && !self.typing.logging_in
            && !self.typing.map_search
            && !self.typing.rebinding
            && self.window_focused()
    }

    /// The movement acts a running script holds itself.
    pub(crate) fn scripted(&self) -> &[Act] {
        self.script.as_ref().map_or(&[], |script| script.acts())
    }

    /// The heading a running script's `face` step turns the player to.
    pub(crate) fn scripted_facing(&self) -> Option<f32> {
        self.script.as_ref().and_then(|script| script.facing())
    }

    /// Whether this frame's Escape belongs to the game, not to closing the chat.
    pub(crate) fn escape_free(&self) -> bool {
        self.focused() && !self.typing.escape_consumed
    }

    /// A separate text editor owns keys, including on the login screens.
    pub(crate) fn editing_script(&self) -> bool {
        self.typing.editing_script
    }

    /// A text field already used this frame's Escape press.
    pub(crate) fn escape_claimed(&self) -> bool {
        self.typing.escape_consumed || self.typing.editing_script
    }

    /// A fresh press of the action, while the game has the keyboard.
    pub(crate) fn pressed(&self, act: Act) -> bool {
        self.focused() && self.map.just_pressed(&self.input, act, &self.presses)
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
            .init_resource::<super::presses::Presses>()
            .init_resource::<super::Typing>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titanium_starts_with_its_programs_keys() {
        use eq_client_assets::ui::InstalledClient;
        let map = KeyMap::for_client(InstalledClient::Titanium);
        let keys = |act| map.chords(act).to_vec();
        assert_eq!(keys(Act::Autorun), [Chord::key(KeyCode::NumLock)]);
        assert_eq!(
            keys(Act::Forward),
            [Chord::key(KeyCode::Numpad8), Chord::key(KeyCode::ArrowUp)]
        );
        assert_eq!(
            keys(Act::TurnLeft),
            [Chord::key(KeyCode::Numpad4), Chord::key(KeyCode::ArrowLeft)]
        );
        assert_eq!(keys(Act::Attack), [Chord::key(KeyCode::KeyQ)]);
        assert_eq!(keys(Act::Consider), [Chord::key(KeyCode::KeyC)]);
        assert_eq!(keys(Act::Duck), [Chord::key(KeyCode::KeyD)]);
        assert_eq!(keys(Act::Use), [Chord::key(KeyCode::KeyU)]);
        assert_eq!(keys(Act::Walk), [Chord::key(KeyCode::F12)]);
        assert_eq!(keys(Act::Gem(7)), [Chord::alt(KeyCode::Digit8)]);
        assert_eq!(keys(Act::Slot(9)), [Chord::key(KeyCode::Digit0)]);
        // The program has no default for strafing nor any camera-relative
        // move; a key its table gives a command leaves this client's act.
        for act in UNBOUND_BY_TABLE {
            assert_eq!(keys(act), [], "{act:?}");
            assert!(map.assignments().any(|(listed, _)| listed == act));
        }
        assert_eq!(keys(Act::Trade), []);
        // This client's own keys stay where the table leaves them free.
        assert_eq!(keys(Act::BindSlot(0)), [Chord::ctrl(KeyCode::Digit1)]);
        assert_eq!(keys(Act::Loot), [Chord::key(KeyCode::KeyL)]);
        // No key does two things.
        let all: Vec<Chord> = map
            .assignments()
            .flat_map(|(_, chords)| chords.to_vec())
            .collect();
        for (index, chord) in all.iter().enumerate() {
            assert!(!all[index + 1..].contains(chord), "{chord:?}");
        }
        // The Mac-era client's table is unread: this client's own keys.
        let mac = KeyMap::for_client(InstalledClient::EqMac);
        for client in [&map, &mac] {
            assert_eq!(
                client.chords(Act::Toggle(WindowId::Map)),
                [Chord::ctrl(KeyCode::KeyM)]
            );
        }
        assert_eq!(mac.chords(Act::Attack), [Chord::key(KeyCode::KeyG)]);
        assert_eq!(mac.chords(Act::Autorun), [Chord::key(KeyCode::NumLock)]);
    }

    #[test]
    fn an_unattended_script_drives_a_window_without_the_focus() {
        #[derive(Resource, Default)]
        struct Seen(Option<bool>);
        let mut app = App::new();
        testing::install(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Seen>()
            .add_systems(Update, |keys: Keys, mut seen: ResMut<Seen>| {
                seen.0 = Some(keys.window_focused());
            });
        app.world_mut().spawn((
            Window {
                focused: false,
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, Some(false));
        // A script watched over, as on P99, still waits for the focus.
        app.insert_resource(crate::script::Script::new(Vec::new()));
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, Some(false));
        // One on a local server runs unattended, focus or not.
        app.insert_resource(crate::script::Script::new(Vec::new()).local_session(true));
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, Some(true));
    }

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
        let presses = presses::Presses::default();
        let one = keys(&[], KeyCode::Digit1);
        let alt_one = keys(&[KeyCode::AltLeft], KeyCode::Digit1);
        let ctrl_one = keys(&[KeyCode::ControlRight], KeyCode::Digit1);
        let ctrl_shift_one = keys(&[KeyCode::ControlLeft, KeyCode::ShiftLeft], KeyCode::Digit1);
        assert!(map.just_pressed(&one, Act::Slot(0), &presses));
        assert!(!map.just_pressed(&one, Act::Gem(0), &presses));
        assert!(map.just_pressed(&alt_one, Act::Gem(0), &presses));
        assert!(!map.just_pressed(&alt_one, Act::Slot(0), &presses));
        assert!(map.just_pressed(&ctrl_one, Act::BindSlot(0), &presses));
        assert!(!map.just_pressed(&ctrl_shift_one, Act::BindSlot(0), &presses));
        assert!(map.just_pressed(&ctrl_shift_one, Act::ClearSlot(0), &presses));
        let tab = keys(&[KeyCode::ShiftRight], KeyCode::Tab);
        assert!(map.just_pressed(&tab, Act::TargetPrevious, &presses));
        assert!(!map.just_pressed(&tab, Act::TargetNext, &presses));
    }

    #[test]
    fn typing_in_map_search_blocks_gameplay_bindings() {
        #[derive(Resource, Default)]
        struct Seen(bool);
        let mut app = crate::testing::app();
        app.init_resource::<Seen>()
            .add_systems(Update, |keys: Keys, mut seen: ResMut<Seen>| {
                seen.0 = keys.pressed(Act::Attack);
            });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyG);
        app.world_mut().resource_mut::<Typing>().map_search = true;
        app.update();
        assert!(!app.world().resource::<Seen>().0);
        app.world_mut().resource_mut::<Typing>().map_search = false;
        app.update();
        assert!(app.world().resource::<Seen>().0);
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

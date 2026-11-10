//! The client's key bindings: one table of what each key does, read by every
//! system that takes keys, with the hints written from it. As in the official
//! client's key map, an action may have a primary and an alternate key.
//!
//! One modifier rule holds for every action: a chord matches only with
//! exactly its modifiers held, so Alt+1 casts the first gem while 1 uses the
//! first action slot, and Ctrl+1 binds that slot. Movement is the exception:
//! a movement key moves whatever is held with it.
use super::windows::WindowId;
use bevy::{
    ecs::message::MessageId, input::keyboard::KeyboardInput, prelude::*, window::PrimaryWindow,
};
use std::collections::{BTreeMap, HashSet};

pub(crate) mod bindings;
mod installed;
pub(crate) mod modifiers;
pub(crate) mod presses;
pub(crate) mod search;

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
    SitStand,
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
    /// Selects this hotbar page, from 0.
    HotbarPage(u8),
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
    /// Tilts the first-person view up or down while held, and levels it.
    LookUp,
    LookDown,
    CenterView,
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
            Self::SitStand => "Sit or Stand",
            Self::Duck => "Duck",
            Self::Stand => "Stand",
            Self::Gem(gem) => return format!("Cast Spell {}", gem + 1),
            Self::Slot(slot) => return format!("Hotbar Button {}", slot + 1),
            Self::BindSlot(slot) => return format!("Bind Hotbar Button {}", slot + 1),
            Self::ClearSlot(slot) => return format!("Clear Hotbar Button {}", slot + 1),
            Self::HotbarPage(page) => return format!("Hotbar Page {}", page + 1),
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
            Self::LookUp => "Look Up",
            Self::LookDown => "Look Down",
            Self::CenterView => "Center View",
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
            Self::Slot(_) | Self::BindSlot(_) | Self::ClearSlot(_) | Self::HotbarPage(_) => {
                KeyGroup::Hotbar(1)
            }
            Self::FreeCamera
            | Self::CameraUp
            | Self::CameraDown
            | Self::LookUp
            | Self::LookDown
            | Self::CenterView => KeyGroup::Camera,
            Self::Sit
            | Self::SitStand
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

    #[cfg(test)]
    pub(crate) const fn alt(key: KeyCode) -> Self {
        Self {
            alt: true,
            ..Self::key(key)
        }
    }

    #[cfg(test)]
    pub(crate) const fn ctrl(key: KeyCode) -> Self {
        Self {
            ctrl: true,
            ..Self::key(key)
        }
    }

    #[cfg(test)]
    pub(crate) const fn shift(key: KeyCode) -> Self {
        Self {
            shift: true,
            ..Self::key(key)
        }
    }

    #[cfg(test)]
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

/// Every action's keys: a primary and, for some, an alternate.
#[derive(Resource, Clone, Debug)]
pub(crate) struct KeyMap(BTreeMap<Act, [Option<Chord>; 2]>);

impl Default for KeyMap {
    fn default() -> Self {
        // Offline/hand-built apps without an installed generation use the
        // Titanium table as an explicit fallback, through the same builder.
        Self::for_client(eq_client_assets::ui::InstalledClient::Titanium)
    }
}

/// Actions listed in Options but without a verified equivalent default.
/// Native strafe commands are unbound in Titanium; direct loot/trade and
/// separate sit/stand are extensions. Camera-relative native commands use
/// modifiers that this client's held movement currently ignores, so their
/// equivalence is not established and no substitute W/A/S/D keys are added.
const UNBOUND: [Act; 14] = [
    Act::TargetPrevious,
    Act::Loot,
    Act::Trade,
    Act::Sit,
    Act::Stand,
    Act::FreeCamera,
    Act::CameraUp,
    Act::CameraDown,
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
        Command::LookUp => Act::LookUp,
        Command::LookDown => Act::LookDown,
        Command::CenterView => Act::CenterView,
        Command::AutoAttack => Act::Attack,
        Command::Consider => Act::Consider,
        Command::Duck => Act::Duck,
        Command::Hail => Act::Hail,
        Command::Inventory => Act::Toggle(WindowId::Inventory),
        Command::Spellbook => Act::Toggle(WindowId::Spellbook),
        Command::Options => Act::Toggle(WindowId::Options),
        Command::Raid => Act::Toggle(WindowId::Raid),
        Command::Map => Act::Toggle(WindowId::Map),
        Command::SitStand => Act::SitStand,
        Command::Use => Act::Use,
        Command::TargetSelf => Act::TargetSelf,
        Command::TargetNext => Act::TargetNext,
        // This client keeps eight gems.
        Command::Gem(gem) if gem < 8 => Act::Gem(gem),
        Command::HotButton(slot) => Act::Slot(slot),
        Command::HotbarPage(page) if page < 10 => Act::HotbarPage(page),
        Command::Gem(_) | Command::HotbarPage(_) | Command::Reserved(_) => return None,
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

fn table_chord(key: eq_client_assets::keys::ScanKey) -> Option<Chord> {
    Some(Chord {
        key: scan_key(key.scan)?,
        ctrl: key.ctrl,
        shift: key.shift,
        alt: key.alt,
    })
}

impl KeyMap {
    /// The selected installation's keyboard seed, before this client's edits.
    pub(super) fn for_settings(settings: &super::ViewerConfig) -> Self {
        let mut map = Self::for_client(settings.installed_client);
        if settings.installed_client == eq_client_assets::ui::InstalledClient::Titanium
            && let Some(directory) = settings.eq_directory.as_deref()
        {
            map.import_titanium_movement(directory);
        }
        map
    }

    /// The installed generation's verified table, with only deliberate UI
    /// extensions. An unread table uses Titanium as a fallback, never a
    /// claimed native default for that generation. Saved keys load later.
    pub(crate) fn for_client(client: eq_client_assets::ui::InstalledClient) -> Self {
        let table = client
            .default_keys()
            .unwrap_or(eq_client_assets::keys::TITANIUM);
        let mut read = BTreeMap::<Act, [Option<Chord>; 2]>::new();
        for act in UNBOUND {
            read.insert(act, [None; 2]);
        }
        for index in 0..10 {
            read.insert(Act::BindSlot(index), [None; 2]);
            read.insert(Act::ClearSlot(index), [None; 2]);
        }
        for entry in table {
            let (Some(act), Some(chord)) = (act_for(entry.command), table_chord(entry.key)) else {
                continue;
            };
            read.entry(act).or_default()[usize::from(entry.alternate)] = Some(chord);
        }
        // EqMac has no native map action. Use Titanium's verified map keys
        // only if the entire native table, including unsupported commands,
        // leaves them free. Never borrow an unrelated native command's keys.
        if let std::collections::btree_map::Entry::Vacant(entry) =
            read.entry(Act::Toggle(WindowId::Map))
        {
            let reserved: Vec<_> = table
                .iter()
                .filter_map(|entry| table_chord(entry.key))
                .collect();
            let mut fallback = eq_client_assets::keys::TITANIUM
                .iter()
                .filter(|entry| entry.command == eq_client_assets::keys::Command::Map)
                .filter_map(|entry| table_chord(entry.key))
                .filter(|chord| !reserved.contains(chord));
            entry.insert([fallback.next(), fallback.next()]);
        }
        Self(read)
    }
    /// Every action with its keys, in the key map's order.
    pub(crate) fn assignments(&self) -> impl Iterator<Item = (Act, &[Option<Chord>; 2])> {
        self.0.iter().map(|(act, slots)| (*act, slots))
    }

    /// The chords bound to an action.
    fn bound(&self, act: Act) -> impl Iterator<Item = Chord> + '_ {
        self.0.get(&act).into_iter().flatten().flatten().copied()
    }

    #[cfg(test)]
    fn chords(&self, act: Act) -> Vec<Chord> {
        self.bound(act).collect()
    }

    /// The keys bound to an action, whatever their modifiers.
    pub(crate) fn keys(&self, act: Act) -> impl Iterator<Item = KeyCode> + '_ {
        self.bound(act).map(|chord| chord.key)
    }

    /// The action's first bound chord as a hint writes it, or nothing if unbound.
    pub(crate) fn label(&self, act: Act) -> String {
        self.bound(act)
            .next()
            .map_or_else(String::new, Chord::label)
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

    /// Key help for these actions, one way for every window: "C: consider |
    /// Q: attack". Unbound actions are left out.
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
        self.bound(act).any(|chord| presses.matches(chord, keys))
    }

    /// Whether one of the action's keys is held, whatever is held with it.
    pub(crate) fn held(&self, keys: &ButtonInput<KeyCode>, act: Act) -> bool {
        self.keys(act).any(|key| keys.pressed(key))
    }

    /// -1, 0 or 1 as the negative or positive action is held, or both or neither.
    /// Whether one of the action's chords is held with exactly its
    /// modifiers, for keys that, unlike movement, belong to their chord.
    pub(crate) fn held_exactly(&self, keys: &ButtonInput<KeyCode>, act: Act) -> bool {
        self.bound(act)
            .any(|chord| keys.pressed(chord.key) && chord.modifiers_held(keys))
    }

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
    /// The spellbook's local search owns text input.
    pub spell_search: bool,
    /// The saved spell set name owns text input.
    pub spell_set_name: bool,
    /// Whether a spellbook text field consumed this frame's keyboard events.
    pub spell_search_claimed: bool,
    /// Keys pressed in the editor stay there through their native repeats.
    pub spell_search_held: HashSet<KeyCode>,
    /// Ordered input claimed by the spell search during this frame.
    pub spell_search_events: HashSet<MessageId<KeyboardInput>>,
    /// Presses the chat line took this frame, so the game does not take
    /// them too when the line closes in the same frame.
    pub chat_events: HashSet<MessageId<KeyboardInput>>,
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

impl Typing {
    /// Whether another reader should skip keyboard input owned by the book.
    pub(crate) fn spell_search_captured(&self, id: MessageId<KeyboardInput>) -> bool {
        self.spell_search_claimed || self.spell_search_events.contains(&id)
    }
}

/// Whether the primary window is focused or the current script can use input
/// without native focus. Background runs must hold their per-frame guard.
pub(crate) fn window_owns_input(
    windows: &Query<&Window, With<PrimaryWindow>>,
    script: Option<&crate::script::Script>,
) -> bool {
    windows.single().is_ok_and(|window| window.focused)
        || script.is_some_and(crate::script::Script::unattended)
}

/// The keyboard as the game sees it: the bindings, and whether the game has
/// the keyboard at all (the window has focus, the chat is not being typed
/// in and no login screen is up). Systems that take keys read them through
/// this.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Keys<'w, 's> {
    /// The raw keyboard, for keys outside the map such as Escape and stack modifiers.
    pub(crate) input: Res<'w, ButtonInput<KeyCode>>,
    pub(crate) map: Res<'w, KeyMap>,
    presses: Res<'w, presses::Presses>,
    typing: Res<'w, Typing>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    script: Option<Res<'w, crate::script::Script>>,
}

impl Keys<'_, '_> {
    /// Whether native focus or the script's input policy owns the window.
    pub(crate) fn window_focused(&self) -> bool {
        window_owns_input(&self.windows, self.script.as_deref())
    }

    /// Whether the game has the keyboard.
    pub(crate) fn focused(&self) -> bool {
        !self.typing.composing
            && !self.typing.creating_character
            && !self.typing.editing_script
            && !self.typing.counting
            && !self.typing.logging_in
            && !self.typing.map_search
            && !self.typing.spell_search
            && !self.typing.spell_set_name
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
        let map = app
            .world()
            .get_resource::<crate::ViewerSettings>()
            .map_or_else(super::KeyMap::default, |settings| {
                super::KeyMap::for_settings(&settings.0)
            });
        if !app.world().contains_resource::<super::KeyMap>() {
            app.insert_resource(map);
        }
        app.init_resource::<super::presses::Presses>()
            .init_resource::<super::Typing>();
    }

    /// A test's explicit user assignment, with the same single-owner rule.
    pub(crate) fn bind(app: &mut App, act: super::Act, chord: super::Chord) {
        let mut map = app.world_mut().resource_mut::<super::KeyMap>();
        assign(&mut map, act, chord);
    }

    fn assign(map: &mut super::KeyMap, act: super::Act, chord: super::Chord) {
        map.unbind(chord);
        map.0.insert(act, [Some(chord), None]);
    }

    /// A movement test's explicit user map, rather than guessed defaults.
    pub(crate) fn map(bindings: &[(super::Act, super::Chord)]) -> super::KeyMap {
        let mut map = super::KeyMap::default();
        for (act, chord) in bindings {
            assign(&mut map, *act, *chord);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titanium_starts_with_its_programs_keys() {
        use eq_client_assets::ui::InstalledClient;
        let map = KeyMap::for_client(InstalledClient::Titanium);
        let keys = |act| map.chords(act);
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
        assert_eq!(keys(Act::Walk), [Chord::ctrl(KeyCode::KeyR)]);
        assert_eq!(keys(Act::SitStand), [Chord::ctrl(KeyCode::KeyS)]);
        assert_eq!(
            keys(Act::Toggle(WindowId::Spellbook)),
            [Chord::ctrl(KeyCode::KeyB)]
        );
        assert_eq!(keys(Act::Gem(7)), [Chord::alt(KeyCode::Digit8)]);
        assert_eq!(keys(Act::Slot(9)), [Chord::key(KeyCode::Digit0)]);
        assert_eq!(keys(Act::HotbarPage(0)), [Chord::shift(KeyCode::Digit1)]);
        assert_eq!(keys(Act::HotbarPage(9)), [Chord::shift(KeyCode::Digit0)]);
        // Commands 17-19 of the program's table (read R13).
        assert_eq!(
            keys(Act::LookUp),
            [Chord::key(KeyCode::Numpad9), Chord::key(KeyCode::PageUp)]
        );
        assert_eq!(
            keys(Act::LookDown),
            [Chord::key(KeyCode::Numpad3), Chord::key(KeyCode::PageDown)]
        );
        assert_eq!(
            keys(Act::CenterView),
            [Chord::key(KeyCode::Numpad5), Chord::key(KeyCode::Home)]
        );
        for act in UNBOUND {
            assert_eq!(keys(act), [], "{act:?}");
            assert!(map.assignments().any(|(listed, _)| listed == act));
        }
        assert_eq!(keys(Act::Trade), []);
        // This client's own keys stay where the table leaves them free.
        assert_eq!(keys(Act::BindSlot(0)), []);
        assert_eq!(keys(Act::Loot), []);
        // No key does two things.
        let all: Vec<Chord> = map
            .assignments()
            .flat_map(|(_, chords)| chords.iter().copied().flatten())
            .collect();
        for (index, chord) in all.iter().enumerate() {
            assert!(!all[index + 1..].contains(chord), "{chord:?}");
        }
        // EqMac has its own read table, not requested Q/Z starting guesses.
        let mac = KeyMap::for_client(InstalledClient::EqMac);
        for client in [&map, &mac] {
            assert_eq!(
                client.chords(Act::Toggle(WindowId::Map)),
                [Chord::key(KeyCode::Backspace), Chord::key(KeyCode::KeyM)]
            );
        }
        assert_eq!(mac.chords(Act::Attack), [Chord::key(KeyCode::KeyA)]);
        assert_eq!(mac.chords(Act::Autorun), [Chord::key(KeyCode::NumLock)]);
        // The Mac-era table holds the same keys under commands not yet read.
        for act in [Act::LookUp, Act::LookDown, Act::CenterView] {
            assert_eq!(mac.chords(act), [], "{act:?}");
        }
    }

    #[test]
    fn mac_native_command_keys_have_no_movement_or_command_conflicts() {
        let map = KeyMap::for_client(eq_client_assets::ui::InstalledClient::EqMac);
        for (act, key) in [
            (Act::Attack, KeyCode::KeyA),
            (Act::Consider, KeyCode::KeyC),
            (Act::Duck, KeyCode::KeyD),
            (Act::Use, KeyCode::KeyU),
        ] {
            let chord = Chord::key(key);
            assert_eq!(map.chords(act), [chord]);
            let owners: Vec<_> = map
                .assignments()
                .filter_map(|(owner, chords)| chords.contains(&Some(chord)).then_some(owner))
                .collect();
            assert_eq!(owners, [act]);
        }
        assert_eq!(
            map.chords(Act::TurnLeft),
            [Chord::key(KeyCode::Numpad4), Chord::key(KeyCode::ArrowLeft)]
        );
        for act in UNBOUND {
            assert_eq!(map.chords(act), [], "{act:?}");
        }
        assert_eq!(map.chords(Act::Walk), [Chord::ctrl(KeyCode::KeyR)]);
        assert_eq!(
            map.chords(Act::Toggle(WindowId::Spellbook)),
            [Chord::ctrl(KeyCode::KeyB)]
        );
        let chords: Vec<_> = map
            .assignments()
            .flat_map(|(_, chords)| chords.iter().flatten())
            .collect();
        for (index, chord) in chords.iter().enumerate() {
            assert!(!chords[index + 1..].contains(chord), "{chord:?}");
        }
    }

    #[test]
    fn unsupported_native_commands_reserve_keys_against_unrelated_defaults() {
        use eq_client_assets::ui::InstalledClient;
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let map = KeyMap::for_client(client);
            for chord in [
                Chord::ctrl(KeyCode::KeyM),
                Chord::key(KeyCode::F9),
                Chord::shift(KeyCode::PageUp),
                Chord::shift(KeyCode::PageDown),
            ] {
                assert!(client.default_keys().unwrap().iter().any(|entry| {
                    matches!(entry.command, eq_client_assets::keys::Command::Reserved(_))
                        && table_chord(entry.key) == Some(chord)
                }));
                assert!(
                    !map.assignments()
                        .any(|(_, chords)| chords.contains(&Some(chord))),
                    "{client:?}: {chord:?}"
                );
            }
            for index in 0..10 {
                assert_eq!(map.chords(Act::BindSlot(index)), []);
                assert_eq!(map.chords(Act::ClearSlot(index)), []);
            }
            assert_eq!(map.chords(Act::FreeCamera), []);
            assert_eq!(map.chords(Act::CameraUp), []);
            assert_eq!(map.chords(Act::CameraDown), []);
        }
        let titanium = InstalledClient::Titanium.default_keys().unwrap();
        assert!(titanium.iter().any(|entry| matches!(
            entry.command,
            eq_client_assets::keys::Command::Reserved(_)
        ) && table_chord(entry.key)
            == Some(Chord::ctrl(KeyCode::Digit1))));
    }

    #[test]
    fn offline_fallback_and_test_apps_use_the_same_generation_builder() {
        use eq_client_assets::ui::InstalledClient;
        assert_eq!(
            KeyMap::default().0,
            KeyMap::for_client(InstalledClient::Titanium).0
        );
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            let mut settings = crate::ViewerSettings::default();
            settings.0.installed_client = client;
            let mut app = App::new();
            app.insert_resource(settings);
            crate::init_presentation(&mut app);
            assert_eq!(
                app.world().resource::<KeyMap>().0,
                KeyMap::for_client(client).0
            );
            testing::install(&mut app);
            assert_eq!(
                app.world().resource::<KeyMap>().0,
                KeyMap::for_client(client).0
            );
            let mut app = App::new();
            let mut settings = crate::ViewerSettings::default();
            settings.0.installed_client = client;
            app.insert_resource(settings);
            testing::install(&mut app);
            assert_eq!(
                app.world().resource::<KeyMap>().0,
                KeyMap::for_client(client).0
            );
        }
        let mut app = App::new();
        testing::install(&mut app);
        assert_eq!(app.world().resource::<KeyMap>().0, KeyMap::default().0);
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
        // Explicit editor assignments can override native reservations.
        let map = testing::map(&[
            (Act::BindSlot(0), Chord::ctrl(KeyCode::Digit1)),
            (Act::ClearSlot(0), Chord::ctrl_shift(KeyCode::Digit1)),
        ]);
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
        assert!(!map.just_pressed(&tab, Act::TargetPrevious, &presses));
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
            .press(KeyCode::KeyQ);
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
        let walking = keys(&[KeyCode::ShiftLeft], KeyCode::ArrowUp);
        assert!(map.held(&walking, Act::Forward));
    }

    #[test]
    fn hints_name_each_actions_key() {
        let map = KeyMap::default();
        assert_eq!(map.label(Act::Gem(0)), "Alt+1");
        assert_eq!(map.label(Act::ClearSlot(9)), "");
        assert_eq!(
            map.help(&[(Act::Consider, "consider"), (Act::Attack, "attack")]),
            "C: consider | Q: attack"
        );
        assert_eq!(
            map.named(Act::Toggle(WindowId::Spellbook), "Spellbook"),
            "Spellbook [Ctrl+B]"
        );
        assert_eq!(map.label(Act::Forward), "Numpad8");
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

//! An installed client's built-in default keys, from the program: what a
//! fresh installation binds each of its key commands to before any settings
//! file changes them. A client generation's table is
//! [`crate::ui::InstalledClient::default_keys`].
//!
//! The Titanium program fills its key map from a built-in table (0x4b26f0),
//! then reads the `[KeyMaps]` overrides from its settings files, which a
//! fresh installation does not have (program read N3 of 11-01; reading the
//! overrides is recording checklist 6.352). Each command is the index the
//! program's key-command dispatcher (0x4619cd) switches on; only the
//! commands whose action is identified are listed here (the command map of
//! 11-01), and of those only the ones this client also has.

/// A program key command whose action is identified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Command {
    /// Runs forward until a forward or back key, the command again or a
    /// posture change stops it.
    Autorun,
    /// Jumps.
    Jump,
    /// Moves forward while held.
    Forward,
    /// Moves back while held.
    Back,
    /// Turns right while held.
    TurnRight,
    /// Turns left while held.
    TurnLeft,
    /// Switches between walking and running (identified from its routine
    /// and its key; recording checklist 6.361).
    WalkRun,
    /// Turns auto attack on and off.
    AutoAttack,
    /// Considers the target.
    Consider,
    /// Ducks, and stands from ducking.
    Duck,
    /// Hails the target.
    Hail,
    /// Opens or closes the inventory window.
    Inventory,
    /// Uses whatever is under the mouse: a spawn's right-click action, or a
    /// door.
    Use,
    /// Targets the player themselves.
    TargetSelf,
    /// Targets the next spawn (the order is unread, 6.361).
    TargetNext,
    /// Casts the spell in this gem, from 0.
    Gem(u8),
    /// Uses this hotbutton of the first bank, from 0.
    HotButton(u8),
    /// Selects this page of the first hotbar, from 0 (0x4619cd, commands 75..84).
    HotbarPage(u8),
}

/// A key as the program's table holds it: a `DirectInput` scan code with
/// the modifiers held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanKey {
    /// The `DirectInput` scan code.
    pub scan: u8,
    /// Held with Ctrl.
    pub ctrl: bool,
    /// Held with Shift.
    pub shift: bool,
    /// Held with Alt.
    pub alt: bool,
}

impl ScanKey {
    const fn plain(scan: u8) -> Self {
        Self {
            scan,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    const fn alt(scan: u8) -> Self {
        Self {
            alt: true,
            ..Self::plain(scan)
        }
    }

    const fn shift(scan: u8) -> Self {
        Self {
            shift: true,
            ..Self::plain(scan)
        }
    }
}

/// A command's default key: its primary, or with `alternate` its second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultKey {
    /// The command it asks for.
    pub command: Command,
    /// The command's second key rather than its first.
    pub alternate: bool,
    /// The key.
    pub key: ScanKey,
}

const fn primary(command: Command, scan: u8) -> DefaultKey {
    DefaultKey {
        command,
        alternate: false,
        key: ScanKey::plain(scan),
    }
}

const fn alternate(command: Command, scan: u8) -> DefaultKey {
    DefaultKey {
        command,
        alternate: true,
        key: ScanKey::plain(scan),
    }
}

const fn hotbar_page(page: u8, scan: u8) -> DefaultKey {
    DefaultKey {
        command: Command::HotbarPage(page),
        alternate: false,
        key: ScanKey::shift(scan),
    }
}

/// The Titanium program's defaults for the commands listed, by the
/// command's index in its table.
pub const TITANIUM: &[DefaultKey] = &[
    primary(Command::Autorun, 0x45),   // 0: Num Lock
    primary(Command::Jump, 0x39),      // 1: Space
    primary(Command::Forward, 0x48),   // 2: keypad 8
    alternate(Command::Forward, 0xc8), // 2: Up
    primary(Command::Back, 0x50),      // 3: keypad 2
    alternate(Command::Back, 0xd0),    // 3: Down
    primary(Command::TurnRight, 0x4d), // 4: keypad 6
    alternate(Command::TurnRight, 0xcd),
    primary(Command::TurnLeft, 0x4b), // 5: keypad 4
    alternate(Command::TurnLeft, 0xcb),
    primary(Command::WalkRun, 0x58),    // 8: F12
    primary(Command::AutoAttack, 0x10), // 10: Q
    primary(Command::Consider, 0x2e),   // 11: C
    primary(Command::Duck, 0x20),       // 12: D
    primary(Command::Hail, 0x23),       // 13: H
    primary(Command::Inventory, 0x17),  // 14: I
    primary(Command::Use, 0x16),        // 16: U
    primary(Command::TargetSelf, 0x3b), // 24: F1
    // 35-44: the first bank's hotbuttons on 1 to 0.
    primary(Command::HotButton(0), 0x02),
    primary(Command::HotButton(1), 0x03),
    primary(Command::HotButton(2), 0x04),
    primary(Command::HotButton(3), 0x05),
    primary(Command::HotButton(4), 0x06),
    primary(Command::HotButton(5), 0x07),
    primary(Command::HotButton(6), 0x08),
    primary(Command::HotButton(7), 0x09),
    primary(Command::HotButton(8), 0x0a),
    primary(Command::HotButton(9), 0x0b),
    // 75..84: Shift+1..0. Read from 0x4b26f0; modifier decoding/getters
    // 0x5ba2d0/0x5ba270 and formatting 0x5ba560 identify Shift.
    hotbar_page(0, 0x02),
    hotbar_page(1, 0x03),
    hotbar_page(2, 0x04),
    hotbar_page(3, 0x05),
    hotbar_page(4, 0x06),
    hotbar_page(5, 0x07),
    hotbar_page(6, 0x08),
    hotbar_page(7, 0x09),
    hotbar_page(8, 0x0a),
    hotbar_page(9, 0x0b),
    // 115-123: the spell gems on Alt+1 to Alt+9.
    DefaultKey {
        command: Command::Gem(0),
        alternate: false,
        key: ScanKey::alt(0x02),
    },
    DefaultKey {
        command: Command::Gem(1),
        alternate: false,
        key: ScanKey::alt(0x03),
    },
    DefaultKey {
        command: Command::Gem(2),
        alternate: false,
        key: ScanKey::alt(0x04),
    },
    DefaultKey {
        command: Command::Gem(3),
        alternate: false,
        key: ScanKey::alt(0x05),
    },
    DefaultKey {
        command: Command::Gem(4),
        alternate: false,
        key: ScanKey::alt(0x06),
    },
    DefaultKey {
        command: Command::Gem(5),
        alternate: false,
        key: ScanKey::alt(0x07),
    },
    DefaultKey {
        command: Command::Gem(6),
        alternate: false,
        key: ScanKey::alt(0x08),
    },
    DefaultKey {
        command: Command::Gem(7),
        alternate: false,
        key: ScanKey::alt(0x09),
    },
    DefaultKey {
        command: Command::Gem(8),
        alternate: false,
        key: ScanKey::alt(0x0a),
    },
    primary(Command::TargetNext, 0x0f), // 128: Tab
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_command_has_one_primary_at_most_and_no_key_twice() {
        for (index, entry) in TITANIUM.iter().enumerate() {
            for other in &TITANIUM[index + 1..] {
                assert_ne!(entry.key, other.key, "{entry:?} {other:?}");
                assert!(
                    entry.command != other.command || entry.alternate != other.alternate,
                    "{entry:?} {other:?}"
                );
            }
        }
        let autorun = TITANIUM
            .iter()
            .find(|entry| entry.command == Command::Autorun)
            .unwrap();
        assert_eq!(autorun.key, ScanKey::plain(0x45));
    }

    #[test]
    fn native_first_bar_pages_use_shift_one_through_zero() {
        let pages: Vec<_> = TITANIUM
            .iter()
            .filter(|entry| matches!(entry.command, Command::HotbarPage(_)))
            .collect();
        assert_eq!(pages.len(), 10);
        for (page, entry) in (0_u8..10).zip(pages) {
            assert_eq!(entry.command, Command::HotbarPage(page));
            assert_eq!(entry.key, ScanKey::shift(page + 2));
            assert!(!entry.alternate);
        }
    }
}

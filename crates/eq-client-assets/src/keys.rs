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
//! supported commands have typed actions; other read native assignments
//! reserve their chords by command index without implementing those actions.
//!
//! The Mac-era table is read at 0x55a83b, with command identities at
//! 0x611224 and dispatcher 0x54050c (whose command numbers are one-based).
//! Titanium identities at 0x78d868 distinguish mouse look (8, F12) from
//! walk/run (139, Ctrl+R). Modifier decoding differs between generations:
//! Titanium 0x5ba2d0/0x5ba560; Mac-era 0x52505a/0x525520.

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
    /// Switches between walking and running.
    WalkRun,
    /// Tilts the view up while held.
    LookUp,
    /// Tilts the view down while held.
    LookDown,
    /// Levels the view.
    CenterView,
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
    /// Opens or closes the spellbook.
    Spellbook,
    /// Opens or closes Options.
    Options,
    /// Opens or closes the raid window.
    Raid,
    /// Opens or closes the map window.
    Map,
    /// Switches between sitting and standing.
    SitStand,
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
    /// An unimplemented native command whose chords remain reserved.
    /// The index belongs to this generation's native table.
    Reserved(u16),
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

    const fn ctrl(scan: u8) -> Self {
        Self {
            ctrl: true,
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

const fn modified(command: Command, key: ScanKey) -> DefaultKey {
    DefaultKey {
        command,
        alternate: false,
        key,
    }
}

// Generation-independent modifier mask: Ctrl=1, Shift=2, Alt=4.
const fn reserved(command: u16, alternate: bool, scan: u8, modifiers: u8) -> DefaultKey {
    DefaultKey {
        command: Command::Reserved(command),
        alternate,
        key: ScanKey {
            scan,
            ctrl: modifiers & 1 != 0,
            shift: modifiers & 2 != 0,
            alt: modifiers & 4 != 0,
        },
    }
}

/// The Titanium program's defaults and native chord reservations, by command
/// index. Reservations do not claim implementation of the corresponding action.
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
    primary(Command::AutoAttack, 0x10), // 10: Q
    primary(Command::Consider, 0x2e),   // 11: C
    primary(Command::Duck, 0x20),       // 12: D
    primary(Command::Hail, 0x23),       // 13: H
    primary(Command::Inventory, 0x17),  // 14: I
    primary(Command::Use, 0x16),        // 16: U
    // 17-19: pitch up, pitch down and center view (0x4619cd, read R13).
    primary(Command::LookUp, 0x49),       // 17: keypad 9
    alternate(Command::LookUp, 0xc9),     // 17: Page Up
    primary(Command::LookDown, 0x51),     // 18: keypad 3
    alternate(Command::LookDown, 0xd1),   // 18: Page Down
    primary(Command::CenterView, 0x4c),   // 19: keypad 5
    alternate(Command::CenterView, 0xc7), // 19: Home
    primary(Command::TargetSelf, 0x3b),   // 24: F1
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
    primary(Command::TargetNext, 0x0f),                // 128: Tab
    modified(Command::Spellbook, ScanKey::ctrl(0x30)), // 129: Ctrl+B
    modified(Command::SitStand, ScanKey::ctrl(0x1f)),  // 138: Ctrl+S
    modified(Command::WalkRun, ScanKey::ctrl(0x13)),   // 139: Ctrl+R
    modified(Command::Options, ScanKey::alt(0x18)),    // 169: Alt+O
    modified(Command::Raid, ScanKey::alt(0x13)),       // 175: Alt+R
    primary(Command::Map, 0x0e),                       // 187: Backspace
    alternate(Command::Map, 0x32),                     // 187: M
    // Native chords remain reserved even when their action is unsupported.
    reserved(8, false, 0x58, 0),
    reserved(15, false, 0x14, 0),
    reserved(20, false, 0x47, 0),
    reserved(20, true, 0xd2, 0),
    reserved(21, false, 0x4f, 0),
    reserved(21, true, 0xd3, 0),
    reserved(22, false, 0x43, 0),
    reserved(23, false, 0x44, 0),
    reserved(25, false, 0x3c, 0),
    reserved(26, false, 0x3d, 0),
    reserved(27, false, 0x3e, 0),
    reserved(28, false, 0x3f, 0),
    reserved(29, false, 0x40, 0),
    reserved(30, false, 0x41, 0),
    reserved(32, false, 0x42, 0),
    reserved(32, true, 0x29, 0),
    reserved(34, false, 0x57, 0),
    reserved(124, false, 0x13, 0),
    reserved(125, false, 0x0f, 0),
    reserved(126, false, 0x0f, 2),
    reserved(130, false, 0x1e, 1),
    reserved(131, false, 0x2e, 1),
    reserved(132, false, 0x18, 1),
    reserved(133, false, 0x32, 1),
    reserved(134, false, 0x11, 1),
    reserved(135, false, 0x17, 1),
    reserved(136, false, 0x20, 1),
    reserved(137, false, 0x2d, 1),
    reserved(140, false, 0x0c, 2),
    reserved(141, false, 0x0d, 2),
    reserved(142, false, 0x4e, 0),
    reserved(143, false, 0x4a, 0),
    reserved(144, false, 0x48, 2),
    reserved(144, true, 0xc8, 2),
    reserved(145, false, 0x50, 2),
    reserved(145, true, 0xd0, 2),
    reserved(146, false, 0x49, 2),
    reserved(146, true, 0xc9, 2),
    reserved(147, false, 0x51, 2),
    reserved(147, true, 0xd1, 2),
    reserved(148, false, 0x28, 0),
    reserved(149, false, 0x27, 2),
    reserved(150, false, 0x26, 4),
    reserved(151, false, 0x15, 4),
    reserved(152, false, 0x19, 4),
    reserved(153, false, 0x14, 4),
    reserved(154, false, 0x1f, 4),
    reserved(155, false, 0x30, 4),
    reserved(156, false, 0x23, 4),
    reserved(161, false, 0x12, 1),
    reserved(162, false, 0x31, 1),
    reserved(163, false, 0x32, 4),
    reserved(164, false, 0x2f, 0),
    reserved(165, false, 0x17, 4),
    reserved(166, false, 0x14, 1),
    reserved(167, false, 0x21, 4),
    reserved(168, false, 0x3b, 4),
    reserved(171, false, 0x11, 4),
    reserved(173, false, 0x1e, 4),
    reserved(174, false, 0x20, 4),
    reserved(176, false, 0x25, 4),
    reserved(177, false, 0x48, 4),
    reserved(177, true, 0xc8, 4),
    reserved(178, false, 0x50, 4),
    reserved(178, true, 0xd0, 4),
    reserved(179, false, 0x4d, 4),
    reserved(179, true, 0xcd, 4),
    reserved(180, false, 0x4b, 4),
    reserved(180, true, 0xcb, 4),
    reserved(181, false, 0x49, 4),
    reserved(181, true, 0xc9, 4),
    reserved(182, false, 0x51, 4),
    reserved(182, true, 0xd1, 4),
    reserved(183, false, 0x24, 4),
    reserved(184, false, 0x12, 4),
    reserved(185, false, 0x13, 6),
    reserved(186, false, 0x31, 4),
    reserved(188, false, 0x22, 4),
    reserved(198, false, 0x2f, 4),
    reserved(199, false, 0x01, 0),
    reserved(200, false, 0x01, 0),
    reserved(201, false, 0x21, 1),
    reserved(202, false, 0x26, 0),
    reserved(203, false, 0x16, 4),
    reserved(204, false, 0x02, 1),
    reserved(205, false, 0x03, 1),
    reserved(206, false, 0x04, 1),
    reserved(207, false, 0x05, 1),
    reserved(208, false, 0x06, 1),
    reserved(209, false, 0x07, 1),
    reserved(210, false, 0x08, 1),
    reserved(211, false, 0x09, 1),
    reserved(212, false, 0x2e, 4),
    reserved(213, false, 0x2c, 4),
    reserved(214, false, 0x19, 1),
    reserved(215, false, 0x10, 4),
    reserved(216, false, 0x19, 0),
    reserved(217, false, 0x1f, 2),
    reserved(218, false, 0x2f, 1),
    reserved(219, false, 0x14, 2),
    reserved(224, false, 0x19, 2),
    reserved(225, false, 0x30, 0),
    reserved(232, false, 0x21, 0),
    reserved(233, false, 0x2c, 0),
    reserved(234, false, 0x22, 0),
    reserved(238, false, 0x2d, 4),
    reserved(246, false, 0x21, 5),
    reserved(249, false, 0x27, 0),
    reserved(250, false, 0x35, 0),
    reserved(256, false, 0x1c, 0),
    reserved(257, false, 0x1c, 4),
    reserved(261, false, 0xd0, 0),
    reserved(262, false, 0xc8, 0),
    reserved(263, false, 0x01, 0),
    reserved(264, false, 0x1c, 0),
    reserved(264, true, 0x9c, 0),
    reserved(266, false, 0x01, 0),
];

/// The Mac-era defaults before saved overrides, plus read native reservations
/// (including conditional initializer assignments). Indices below are zero-based
/// table indices, not dispatcher IDs.
pub const EQMAC: &[DefaultKey] = &[
    primary(Command::Autorun, 0x45), // 0: Num Lock
    primary(Command::Jump, 0x39),    // 1: Space
    primary(Command::Forward, 0x48), // 2: keypad 8
    alternate(Command::Forward, 0xc8),
    primary(Command::Back, 0x50), // 3: keypad 2
    alternate(Command::Back, 0xd0),
    primary(Command::TurnRight, 0x4d), // 4: keypad 6
    alternate(Command::TurnRight, 0xcd),
    primary(Command::TurnLeft, 0x4b), // 5: keypad 4
    alternate(Command::TurnLeft, 0xcb),
    primary(Command::AutoAttack, 0x1e), // 7: A
    primary(Command::Consider, 0x2e),   // 8: C
    primary(Command::Duck, 0x20),       // 9: D
    primary(Command::Hail, 0x23),       // 10: H
    primary(Command::Inventory, 0x17),  // 11: I
    primary(Command::Use, 0x16),        // 13: U
    primary(Command::TargetSelf, 0x3b), // 21: F1
    // 30..39: the first bank's hotbuttons on 1..0.
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
    // 40..49: first hotbar pages on Shift+1..0.
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
    // 50..57: eight spell gems on Alt+1..8.
    modified(Command::Gem(0), ScanKey::alt(0x02)),
    modified(Command::Gem(1), ScanKey::alt(0x03)),
    modified(Command::Gem(2), ScanKey::alt(0x04)),
    modified(Command::Gem(3), ScanKey::alt(0x05)),
    modified(Command::Gem(4), ScanKey::alt(0x06)),
    modified(Command::Gem(5), ScanKey::alt(0x07)),
    modified(Command::Gem(6), ScanKey::alt(0x08)),
    modified(Command::Gem(7), ScanKey::alt(0x09)),
    primary(Command::TargetNext, 0x0f),                // 62: Tab
    modified(Command::Spellbook, ScanKey::ctrl(0x30)), // 63: Ctrl+B
    modified(Command::SitStand, ScanKey::ctrl(0x1f)),  // 72: Ctrl+S
    modified(Command::WalkRun, ScanKey::ctrl(0x13)),   // 73: Ctrl+R
    modified(Command::Options, ScanKey::alt(0x18)),    // 102: Alt+O
    modified(Command::Raid, ScanKey::alt(0x13)),       // 108: Alt+R
    // Native chords remain reserved even when their action is unsupported.
    reserved(6, false, 0x58, 0),
    reserved(12, false, 0x14, 0),
    reserved(14, false, 0x49, 0),
    reserved(14, true, 0xc9, 0),
    reserved(15, false, 0x51, 0),
    reserved(15, true, 0xd1, 0),
    reserved(16, false, 0x4c, 0),
    reserved(16, true, 0xc7, 0),
    reserved(17, false, 0x47, 0),
    reserved(17, true, 0xd2, 0),
    reserved(18, false, 0x4f, 0),
    reserved(18, true, 0xd3, 0),
    reserved(19, false, 0x43, 0),
    reserved(20, false, 0x44, 0),
    reserved(22, false, 0x3c, 0),
    reserved(23, false, 0x3d, 0),
    reserved(24, false, 0x3e, 0),
    reserved(25, false, 0x3f, 0),
    reserved(26, false, 0x40, 0),
    reserved(27, false, 0x41, 0),
    reserved(28, false, 0x42, 0),
    reserved(29, false, 0x57, 0),
    reserved(58, false, 0x13, 0),
    reserved(59, false, 0x0f, 0),
    reserved(60, false, 0x0f, 2),
    reserved(61, false, 0x29, 0),
    reserved(64, false, 0x1e, 1),
    reserved(65, false, 0x2e, 1),
    reserved(66, false, 0x18, 1),
    reserved(67, false, 0x32, 1),
    reserved(68, false, 0x11, 1),
    reserved(69, false, 0x17, 1),
    reserved(70, false, 0x20, 1),
    reserved(71, false, 0x2d, 1),
    reserved(74, false, 0x0c, 2),
    reserved(75, false, 0x0d, 2),
    reserved(76, false, 0x4e, 0),
    reserved(77, false, 0x4a, 0),
    reserved(78, false, 0x48, 2),
    reserved(78, true, 0xc8, 2),
    reserved(79, false, 0x50, 2),
    reserved(79, true, 0xd0, 2),
    reserved(80, false, 0x4b, 2),
    reserved(80, true, 0xcb, 2),
    reserved(81, false, 0x4d, 2),
    reserved(81, true, 0xcd, 2),
    reserved(82, false, 0x47, 2),
    reserved(82, true, 0xc7, 2),
    reserved(83, false, 0x4f, 2),
    reserved(83, true, 0xcf, 2),
    reserved(84, false, 0x49, 2),
    reserved(84, true, 0xc9, 2),
    reserved(85, false, 0x51, 2),
    reserved(85, true, 0xd1, 2),
    reserved(86, false, 0x28, 0),
    reserved(87, false, 0x27, 2),
    reserved(88, false, 0x26, 4),
    reserved(89, false, 0x15, 4),
    reserved(90, false, 0x19, 4),
    reserved(91, false, 0x14, 4),
    reserved(92, false, 0x1f, 4),
    reserved(93, false, 0x30, 4),
    reserved(94, false, 0x23, 4),
    reserved(95, false, 0x2e, 4),
    reserved(96, false, 0x32, 4),
    reserved(97, false, 0x2f, 0),
    reserved(98, false, 0x17, 4),
    reserved(99, false, 0x14, 1),
    reserved(100, false, 0x21, 4),
    reserved(101, false, 0x3b, 4),
    reserved(104, false, 0x11, 4),
    reserved(106, false, 0x1e, 4),
    reserved(107, false, 0x20, 4),
    reserved(109, false, 0x25, 4),
    reserved(110, false, 0x48, 4),
    reserved(110, true, 0xc8, 4),
    reserved(111, false, 0x50, 4),
    reserved(111, true, 0xd0, 4),
    reserved(112, false, 0x4d, 4),
    reserved(112, true, 0xcd, 4),
    reserved(113, false, 0x4b, 4),
    reserved(113, true, 0xcb, 4),
    reserved(114, false, 0x49, 4),
    reserved(114, true, 0xc9, 4),
    reserved(115, false, 0x51, 4),
    reserved(115, true, 0xd1, 4),
    reserved(116, false, 0x21, 0),
    reserved(117, false, 0x2c, 0),
    reserved(118, false, 0x22, 0),
    reserved(122, false, 0x2d, 4),
    reserved(123, false, 0xb5, 0),
    reserved(124, false, 0x4e, 0),
    reserved(125, false, 0x52, 0),
    reserved(126, false, 0xd2, 2),
    reserved(127, false, 0xd3, 2),
    reserved(128, false, 0x27, 0),
    reserved(129, false, 0x35, 0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_command_has_one_primary_at_most_and_no_key_twice() {
        for table in [TITANIUM, EQMAC] {
            for (index, entry) in table.iter().enumerate() {
                for other in &table[index + 1..] {
                    if !matches!(entry.command, Command::Reserved(_))
                        && !matches!(other.command, Command::Reserved(_))
                    {
                        assert_ne!(entry.key, other.key, "{entry:?} {other:?}");
                    }
                    assert!(
                        entry.command != other.command || entry.alternate != other.alternate,
                        "{entry:?} {other:?}"
                    );
                }
            }
        }
        let autorun = TITANIUM
            .iter()
            .find(|entry| entry.command == Command::Autorun)
            .unwrap();
        assert_eq!(autorun.key, ScanKey::plain(0x45));
    }

    #[test]
    fn native_generations_distinguish_attack_and_share_command_modifiers() {
        let key = |table: &[DefaultKey], command| {
            table
                .iter()
                .find(|entry| entry.command == command)
                .unwrap()
                .key
        };
        assert_eq!(key(TITANIUM, Command::AutoAttack), ScanKey::plain(0x10));
        assert_eq!(key(EQMAC, Command::AutoAttack), ScanKey::plain(0x1e));
        for table in [TITANIUM, EQMAC] {
            assert_eq!(key(table, Command::Duck), ScanKey::plain(0x20));
            assert_eq!(key(table, Command::Use), ScanKey::plain(0x16));
            assert_eq!(key(table, Command::WalkRun), ScanKey::ctrl(0x13));
            assert_eq!(key(table, Command::SitStand), ScanKey::ctrl(0x1f));
            assert_eq!(key(table, Command::Spellbook), ScanKey::ctrl(0x30));
        }
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

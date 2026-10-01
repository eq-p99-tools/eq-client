//! The script language: one bounded step per line, `#` starts a comment.
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::KeyCode;

/// Most steps one script may queue, counting steps appended while following.
pub(super) const MAX_STEPS: usize = 500;
const MAX_HOLD: Duration = Duration::from_secs(10);
/// Longest wait, also the longest a scripted click waits for the pointer to leave.
pub(super) const MAX_WAIT: Duration = Duration::from_mins(2);
const MAX_TRACE: Duration = Duration::from_secs(10);
const MAX_WALK: Duration = Duration::from_mins(1);
/// `EQEmu` GM commands a script may send, without the leading `#`.
const GM_COMMANDS: [&str; 14] = [
    "summon",
    "summonitem",
    "castspell",
    "damage",
    "givemoney",
    "zone",
    "goto",
    "level",
    "heal",
    "kill",
    "repop",
    "freeze",
    "unfreeze",
    // Shows the target (or the GM) in another material, without changing gear.
    "wc",
];

/// One scripted action.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Waits until the character selection list is shown.
    WaitSelect,
    /// Highlights a listed character by name and presses Enter.
    Select(String),
    /// Creates a character from the selection screen (local test servers).
    Create(eq_client_core::creation::NewCharacter),
    /// Waits until the zone is admitted with a player present.
    WaitOnline,
    /// Waits until the named zone is admitted with a player present.
    WaitZone(String),
    /// Runs a game slash command such as `/camp`; chat text is refused.
    Slash(String),
    /// Sends an allowed `#` command, such as `summon`, to a local `EQEmu` server;
    /// the step stops the script on any other server.
    Gm(String),
    /// Presses keys together for one frame, modifiers first.
    Press(Vec<KeyCode>),
    /// Holds keys together for a bounded duration.
    Hold(Vec<KeyCode>, Duration),
    /// Pauses for a bounded duration.
    Wait(Duration),
    /// Orbits the camera so W walks along an EQ heading (0..512), at a pitch in degrees.
    Camera {
        /// EQ heading the camera looks along.
        heading: f32,
        /// Adds the player's current heading, for views relative to the model.
        relative: bool,
        /// Downward pitch in degrees, clamped like mouse orbiting.
        pitch: f32,
    },
    /// Logs the player's position and frame time every frame for a bounded duration.
    Trace(Duration),
    /// Turns the camera so W walks toward the current target.
    Face,
    /// Holds W toward the current target until it is within a distance, for a
    /// bounded duration.
    Approach(f32, Duration),
    /// Walks a searched path around walls to within a distance of the current
    /// target, for a bounded duration.
    Walk(f32, Duration),
    /// Left-clicks one UI control through Bevy's ordinary interaction state.
    Click(ClickTarget),
    /// Logs a numeric summary of player, resource, cast and buff state.
    Report(String),
    /// Saves the primary window to a PNG beside the script.
    Screenshot(PathBuf),
    /// Closes the client.
    Quit,
}

/// UI controls a script may click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickTarget {
    /// An inventory slot button by server slot number.
    Slot(i32),
    /// The spellbook's scribe-cursor-scroll button.
    Scribe,
    /// The inventory's auto-store-cursor button.
    Store,
    /// A spellbook row on the open page, from zero.
    BookRow(usize),
    /// The spellbook's memorize button for a gem, from zero.
    MemorizeGem(u8),
    /// A loot, merchant or window-closing button.
    Trade(TradeClick),
    /// A bag's tint swatch by storage slot, or one of its palette colors.
    Tint(i32, Option<usize>),
}

/// Loot and merchant window buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradeClick {
    /// Take one corpse slot.
    Take(u16),
    /// Take everything.
    TakeAll,
    /// Close the loot window.
    EndLoot,
    /// Buy one unit from a merchant slot.
    Buy(u32),
    /// Sell an inventory slot.
    Sell(i32),
    /// Close the merchant window.
    EndShop,
}

/// Parses a script: one step per line, `#` starts a comment.
///
/// # Errors
/// Returns the first line that is unknown, malformed or outside the bounds.
pub fn parse(text: &str, base: &Path) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let step =
            parse_step(line).map_err(|reason| format!("script line {}: {reason}", index + 1))?;
        steps.push(match step {
            Step::Screenshot(name) => Step::Screenshot(base.join(name)),
            step => step,
        });
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("script has more than {MAX_STEPS} steps"));
    }
    Ok(steps)
}

fn parse_step(line: &str) -> Result<Step, String> {
    let mut words = line.split_whitespace();
    let command = words.next().unwrap_or_default();
    let rest: Vec<&str> = words.collect();
    let millis = |value: &str, limit: Duration| {
        value
            .parse::<u64>()
            .ok()
            .map(Duration::from_millis)
            .filter(|duration| *duration <= limit)
            .ok_or_else(|| format!("expected milliseconds up to {}", limit.as_millis()))
    };
    let number = |value: &str, low: f32, high: f32| {
        value
            .parse::<f32>()
            .ok()
            .filter(|value| (low..=high).contains(value))
            .ok_or_else(|| format!("expected a number from {low} to {high}"))
    };
    let chord = |keys: &str| keys_from(keys).ok_or_else(|| String::from("unknown key"));
    Ok(match (command, rest.as_slice()) {
        ("wait_select", []) => Step::WaitSelect,
        ("select", [name]) => Step::Select((*name).to_owned()),
        ("create", arguments) => parse_create(arguments)?,
        ("wait_online", []) => Step::WaitOnline,
        ("wait_zone", [zone]) => Step::WaitZone(zone.to_ascii_lowercase()),
        ("slash", [command]) if matches!(*command, "camp" | "sit" | "stand") => {
            Step::Slash(format!("/{command}"))
        }
        ("slash", ["target", name @ ..]) if !name.is_empty() => {
            Step::Slash(format!("/target {}", name.join(" ")))
        }
        ("gm", words) => parse_gm(words)?,
        ("press", [keys]) => Step::Press(chord(keys)?),
        ("hold", [keys, duration]) => Step::Hold(chord(keys)?, millis(duration, MAX_HOLD)?),
        ("wait", [duration]) => Step::Wait(millis(duration, MAX_WAIT)?),
        ("camera", [heading, pitch]) => Step::Camera {
            heading: number(heading, 0.0, 512.0)?,
            relative: false,
            pitch: number(pitch, -83.0, -8.0)?,
        },
        ("camera", ["player", offset, pitch]) => Step::Camera {
            heading: number(offset, 0.0, 512.0)?,
            relative: true,
            pitch: number(pitch, -83.0, -8.0)?,
        },
        ("trace", [duration]) => Step::Trace(millis(duration, MAX_TRACE)?),
        ("face", []) => Step::Face,
        ("approach", [range, duration]) => {
            Step::Approach(number(range, 1.0, 200.0)?, millis(duration, MAX_HOLD)?)
        }
        ("walk", [range, duration]) => {
            Step::Walk(number(range, 1.0, 200.0)?, millis(duration, MAX_WALK)?)
        }
        ("click", target) => Step::Click(parse_click(target)?),
        ("report", label) => Step::Report(label.join(" ")),
        ("screenshot", [name])
            if Path::new(name)
                .file_name()
                .is_some_and(|file| file == std::ffi::OsStr::new(name))
                && Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png")) =>
        {
            Step::Screenshot(PathBuf::from(name))
        }
        ("quit", []) => Step::Quit,
        _ => return Err("unknown or malformed step".into()),
    })
}

/// `click <target>`: a slot, spellbook, trade or bag tint button.
fn parse_click(words: &[&str]) -> Result<ClickTarget, String> {
    fn value<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
        text.parse().map_err(|_| format!("expected {what}"))
    }
    Ok(match words {
        ["slot", slot] => ClickTarget::Slot(value(slot, "a slot number")?),
        ["scribe"] => ClickTarget::Scribe,
        ["store"] => ClickTarget::Store,
        ["book", row] => ClickTarget::BookRow(value(row, "a book row number")?),
        ["loot", slot] => ClickTarget::Trade(TradeClick::Take(value(slot, "a corpse slot")?)),
        ["loot_all"] => ClickTarget::Trade(TradeClick::TakeAll),
        ["loot_done"] => ClickTarget::Trade(TradeClick::EndLoot),
        ["buy", slot] => ClickTarget::Trade(TradeClick::Buy(value(slot, "a merchant slot")?)),
        ["sell", slot] => ClickTarget::Trade(TradeClick::Sell(value(slot, "an inventory slot")?)),
        ["shop_done"] => ClickTarget::Trade(TradeClick::EndShop),
        ["tint", slot] => ClickTarget::Tint(value(slot, "a bag slot")?, None),
        ["tint", slot, color] => ClickTarget::Tint(
            value(slot, "a bag slot")?,
            Some(value(color, "a palette color")?),
        ),
        ["memorize", gem] => ClickTarget::MemorizeGem(
            gem.parse::<u8>()
                .ok()
                .filter(|gem| (1..=8).contains(gem))
                .map(|gem| gem - 1)
                .ok_or_else(|| String::from("expected a gem from 1 to 8"))?,
        ),
        _ => return Err("unknown or malformed step".into()),
    })
}

/// `gm <command> [arguments]`: an allowed `EQEmu` command without its `#`, with up
/// to four plain arguments.
fn parse_gm(words: &[&str]) -> Result<Step, String> {
    let [command, arguments @ ..] = words else {
        return Err("gm takes a command".into());
    };
    if !GM_COMMANDS.contains(command) {
        return Err(format!("gm allows only {}", GM_COMMANDS.join(", ")));
    }
    let plain = |argument: &&str| {
        argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    };
    if arguments.len() > 4 || !arguments.iter().all(plain) {
        return Err("gm takes up to four plain arguments".into());
    }
    Ok(Step::Gm(words.join(" ")))
}

/// `create <Name> <race> <class> <gender> <deity> <start zone> <stat for free points>`.
fn parse_create(arguments: &[&str]) -> Result<Step, String> {
    let [name, race, class, gender, deity, zone, primary] = arguments else {
        return Err("create takes a name and six numbers".into());
    };
    let value = |text: &str| {
        text.parse::<u32>()
            .map_err(|_| String::from("expected a number"))
    };
    let character = eq_client_core::creation::NewCharacter::with_points_in(
        *name,
        (value(race)?, value(class)?, value(gender)?),
        (value(deity)?, value(zone)?),
        usize::try_from(value(primary)?).map_err(|_| String::from("bad stat"))?,
    )
    .map_err(|error| error.to_string())?;
    character.validate().map_err(|error| error.to_string())?;
    Ok(Step::Create(character))
}

fn keys_from(text: &str) -> Option<Vec<KeyCode>> {
    text.split('+').map(key).collect()
}

fn key(name: &str) -> Option<KeyCode> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    const FUNCTIONS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    match upper.as_str() {
        "SPACE" => Some(KeyCode::Space),
        "ESCAPE" => Some(KeyCode::Escape),
        "ENTER" => Some(KeyCode::Enter),
        "TAB" => Some(KeyCode::Tab),
        "UP" => Some(KeyCode::ArrowUp),
        "DOWN" => Some(KeyCode::ArrowDown),
        "LEFT" => Some(KeyCode::ArrowLeft),
        "RIGHT" => Some(KeyCode::ArrowRight),
        "SHIFT" => Some(KeyCode::ShiftLeft),
        "CTRL" => Some(KeyCode::ControlLeft),
        "ALT" => Some(KeyCode::AltLeft),
        _ if bytes.len() == 1 && bytes[0].is_ascii_uppercase() => {
            Some(LETTERS[usize::from(bytes[0] - b'A')])
        }
        _ if bytes.len() == 1 && bytes[0].is_ascii_digit() => {
            Some(DIGITS[usize::from(bytes[0] - b'0')])
        }
        _ => upper
            .strip_prefix('F')
            .and_then(|number| number.parse::<usize>().ok())
            .and_then(|number| FUNCTIONS.get(number.checked_sub(1)?).copied()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_steps_and_rejects_unsafe_input() {
        let base = Path::new("private");
        let steps = parse(
            "wait_select\nselect Someone\ncreate Testcleric 1 2 0 212 1 4\nwait_online\nwait_zone TOX\nslash camp\nslash target a cave rat\n\
             gm summon\ngm damage 10000\ngm givemoney 0 0 5 0\npress F1 # self\n\
             press alt+1\nhold W 1500\nwait 250\ncamera 128 -20\ncamera player 256 -15\ntrace 2000\nface\napproach 12 5000\nwalk 8 30000\nclick slot 23\n\
             click scribe\nclick store\nclick book 0\nclick memorize 2\nclick loot 22\nclick loot_all\nclick buy 3\nclick sell 23\nclick shop_done\nreport after cast\nscreenshot a.png\nquit\n",
            base,
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::WaitSelect,
                Step::Select("Someone".into()),
                Step::Create(
                    eq_client_core::creation::NewCharacter::with_points_in(
                        "Testcleric",
                        (1, 2, 0),
                        (212, 1),
                        4
                    )
                    .unwrap()
                ),
                Step::WaitOnline,
                Step::WaitZone("tox".into()),
                Step::Slash("/camp".into()),
                Step::Slash("/target a cave rat".into()),
                Step::Gm("summon".into()),
                Step::Gm("damage 10000".into()),
                Step::Gm("givemoney 0 0 5 0".into()),
                Step::Press(vec![KeyCode::F1]),
                Step::Press(vec![KeyCode::AltLeft, KeyCode::Digit1]),
                Step::Hold(vec![KeyCode::KeyW], Duration::from_millis(1500)),
                Step::Wait(Duration::from_millis(250)),
                Step::Camera {
                    heading: 128.0,
                    relative: false,
                    pitch: -20.0
                },
                Step::Camera {
                    heading: 256.0,
                    relative: true,
                    pitch: -15.0
                },
                Step::Trace(Duration::from_secs(2)),
                Step::Face,
                Step::Approach(12.0, Duration::from_secs(5)),
                Step::Walk(8.0, Duration::from_secs(30)),
                Step::Click(ClickTarget::Slot(23)),
                Step::Click(ClickTarget::Scribe),
                Step::Click(ClickTarget::Store),
                Step::Click(ClickTarget::BookRow(0)),
                Step::Click(ClickTarget::MemorizeGem(1)),
                Step::Click(ClickTarget::Trade(TradeClick::Take(22))),
                Step::Click(ClickTarget::Trade(TradeClick::TakeAll)),
                Step::Click(ClickTarget::Trade(TradeClick::Buy(3))),
                Step::Click(ClickTarget::Trade(TradeClick::Sell(23))),
                Step::Click(ClickTarget::Trade(TradeClick::EndShop)),
                Step::Report("after cast".into()),
                Step::Screenshot(base.join("a.png")),
                Step::Quit,
            ]
        );
        for bad in [
            "hold W 20000",
            "wait 999999",
            "press Nope",
            "screenshot ../a.png",
            "screenshot a.jpg",
            "teleport 1 2 3",
            "camera 600 -20",
            "camera 10 -90",
            "trace 60000",
            "click slot x",
            "click anything",
            "click memorize 9",
            "click memorize 0",
            "slash say hello",
            "create lowercase 1 2 0 212 1 4",
            "create Testbad 1 17 0 212 1 4",
            "slash ooc",
            "walk 8 90000",
            "walk 0 1000",
            "gm",
            "gm shutdown",
            "gm SUMMON",
            "gm zone a b c d e",
            "gm goto 1;2",
            "gm summon Name,Other",
        ] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
        // `#` starts a comment, so a script can never smuggle one into a step.
        assert_eq!(
            parse("gm zone qeynos #givemoney 999", base).unwrap(),
            [Step::Gm("zone qeynos".into())]
        );
    }

    #[test]
    fn parses_bag_tint_clicks() {
        let base = Path::new("private");
        assert_eq!(
            parse("click tint 23\nclick tint 23 4\n", base).unwrap(),
            [
                Step::Click(ClickTarget::Tint(23, None)),
                Step::Click(ClickTarget::Tint(23, Some(4))),
            ]
        );
        for bad in ["click tint x", "click tint 23 x", "click tint 23 4 5"] {
            assert!(parse(bad, base).is_err(), "{bad}");
        }
    }
}

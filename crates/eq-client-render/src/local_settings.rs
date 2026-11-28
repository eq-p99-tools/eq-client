//! The local-settings commands, as reads R49-22c and R49-16 read the
//! Titanium program: `/clearchat`, `/showspelleffects`, `/combatmusic`,
//! `/particledensity`, `/mousespeed`, `/chatfontsize` and `/usercolor`
//! (6.525), and `/toggleinspect` (6.574). They change only this client;
//! nothing goes to a server, on any server type.
//!
//! Where the program says a line from its string table, the installation's
//! words are said, with this client's own where it lacks them. Where the
//! program's lines are written into the program in English (those of
//! `/particledensity` and `/usercolor`), the lines are this client's own.
//! The program saves its choices to `eqclient.ini`; this client keeps them
//! per character in its own settings and never writes the installation's
//! files.
use bevy::prelude::*;
use eq_client_core::options::{Toggle, USER_COLOR_KINDS};

/// The lines of an on/off command, each a line id with our fallback:
/// its usage lines when no word was given, and its state lines, on then
/// off.
struct ToggleLines {
    usage: &'static [(u32, &'static str)],
    state: [(u32, &'static str); 2],
}

/// `/showspelleffects`.
const SPELL_EFFECTS: ToggleLines = ToggleLines {
    usage: &[
        (0x3370, "Usage: /showspelleffects <on|off>"),
        (0x3371, "Turns spell effects on or off."),
    ],
    state: [
        (0x3372, "Spell effects are on."),
        (0x3373, "Spell effects are off."),
    ],
};
/// `/combatmusic`.
const COMBAT_MUSIC: ToggleLines = ToggleLines {
    usage: &[(0x1d63, "Usage: /combatmusic <on|off>")],
    state: [
        (0x1d64, "Combat music is on."),
        (0x1d65, "Combat music is off."),
    ],
};
/// `/mousespeed` with no number: its two usage lines.
const MOUSE_SPEED_USAGE: [u32; 2] = [0x336d, 0x336e];
/// Its line naming the value.
const MOUSE_SPEED_NOW: u32 = 0x336f;
/// `/chatfontsize` without a size the window takes.
const CHAT_FONT_USAGE: u32 = 0x33d6;
/// The largest chat font size.
const LARGEST_CHAT_FONT: u8 = 6;
/// `/toggleinspect` (read R49-16).
const INSPECT: ToggleLines = ToggleLines {
    usage: &[
        (0x339a, "Usage: /toggleinspect <on|off>"),
        (
            0x339b,
            "Turns inspecting other players by right-clicking them on or off.",
        ),
    ],
    state: [
        (0x339c, "Right-clicking a player inspects them."),
        (0x339d, "Right-clicking a player does not inspect them."),
    ],
};
/// The chat colour of `/toggleinspect`'s lines (0x498436).
const INSPECT_COLOUR: u32 = 0x111;
/// The colour kinds the help pages list, 10 a page (1 to 69).
const LISTED_KINDS: u16 = 69;

/// A typed local-settings command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LocalSetting {
    /// `/clearchat`: the active chat window's lines go.
    ClearChat,
    /// `/showspelleffects`, with "off" (false), any other word (true), or no
    /// word.
    SpellEffects(Option<bool>),
    /// `/combatmusic`, read the same way.
    CombatMusic(Option<bool>),
    /// `/particledensity`: a level 0 to 3 and the words typed, or none for
    /// words it does not take.
    ParticleDensity(Option<(u8, String)>),
    /// `/mousespeed`: a finite number, or none for no number.
    MouseSpeed(Option<f64>),
    /// `/chatfontsize`: a size 0 to 6, or none for anything else.
    ChatFontSize(Option<u8>),
    /// `/usercolor`.
    UserColor(UserColor),
    /// `/toggleinspect`, with "on" (true), "off" (false), or none for any
    /// other word or no word (0x469f00, read R49-16).
    InspectOthers(Option<bool>),
}

/// What a `/usercolor` asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UserColor {
    /// Set a kind's colour; a channel that was not a number keeps its old
    /// value.
    Set { kind: u16, rgb: [Option<u8>; 3] },
    /// Put a kind's colour back.
    Reset(u16),
    /// A page of the list of kinds, 1 to 7.
    Help(u8),
    /// The words failed a range check, or named no kind.
    Usage,
}

/// The "off" word the program compares with (its string 0x3292).
fn on_word(word: Option<&str>) -> Option<bool> {
    word.map(|word| !word.eq_ignore_ascii_case("off"))
}

impl LocalSetting {
    /// The command a typed line is, if it is one of the seven, by its full
    /// name (the chat resolves the official abbreviations first).
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let mut words = input.strip_prefix('/')?.split_whitespace();
        let name = words.next()?.to_ascii_lowercase();
        let first = words.next();
        Some(match name.as_str() {
            "clearchat" => Self::ClearChat,
            "showspelleffects" => Self::SpellEffects(on_word(first)),
            "combatmusic" => Self::CombatMusic(on_word(first)),
            "particledensity" => Self::ParticleDensity(first.and_then(|word| {
                density(word).map(|level| {
                    (
                        level,
                        input
                            .split_once(' ')
                            .map_or("", |(_, rest)| rest)
                            .trim()
                            .to_owned(),
                    )
                })
            })),
            "mousespeed" => Self::MouseSpeed(
                first
                    .and_then(|word| word.parse::<f64>().ok())
                    .filter(|speed| speed.is_finite()),
            ),
            "chatfontsize" => Self::ChatFontSize(
                first
                    .and_then(|word| word.parse::<u8>().ok())
                    .filter(|size| *size <= LARGEST_CHAT_FONT),
            ),
            "toggleinspect" => Self::InspectOthers(first.and_then(|word| {
                if word.eq_ignore_ascii_case("on") {
                    Some(true)
                } else if word.eq_ignore_ascii_case("off") {
                    Some(false)
                } else {
                    None
                }
            })),
            "usercolor" => {
                let rest: Vec<&str> = std::iter::once(first).flatten().chain(words).collect();
                Self::UserColor(user_color(&rest))
            }
            _ => return None,
        })
    }
}

/// A `/particledensity` word's level: 0 to 3, or a word whose first three
/// letters are OFF, LOW, MED or HIG in any case (0x46bacb).
fn density(word: &str) -> Option<u8> {
    if let Ok(level) = word.parse::<u8>() {
        return (level <= 3).then_some(level);
    }
    let start = word.get(..3)?.to_ascii_uppercase();
    ["OFF", "LOW", "MED", "HIG"]
        .iter()
        .position(|name| *name == start)
        .and_then(|level| u8::try_from(level).ok())
}

/// Reads a `/usercolor`'s words (0x46b1b2).
fn user_color(words: &[&str]) -> UserColor {
    let page = |word: &str| -> Option<u8> {
        if word.eq_ignore_ascii_case("help") {
            return Some(1);
        }
        let start = word.get(..4)?;
        start.eq_ignore_ascii_case("page").then_some(())?;
        word[4..].parse().ok().filter(|page| (1..=7).contains(page))
    };
    let Some(first) = words.first() else {
        return UserColor::Usage;
    };
    if let Some(page) = page(first) {
        return UserColor::Help(page);
    }
    let Some(kind) = first
        .parse::<u16>()
        .ok()
        .filter(|kind| usize::from(*kind) < USER_COLOR_KINDS)
    else {
        return UserColor::Usage;
    };
    if words
        .get(1)
        .is_some_and(|word| word.eq_ignore_ascii_case("reset"))
    {
        return UserColor::Reset(kind);
    }
    let mut rgb = [None; 3];
    for (channel, word) in rgb.iter_mut().zip(words.iter().skip(1)) {
        // A word that is not a number keeps that channel.
        if let Ok(value) = word.parse::<i64>() {
            match u8::try_from(value) {
                Ok(value) => *channel = Some(value),
                Err(_) => return UserColor::Usage,
            }
        }
    }
    UserColor::Set { kind, rgb }
}

/// The particle density `/particledensity` set this session, 0 to 3; none
/// until then. It is not saved, as the program saves it nowhere. It is its
/// own flag: level 0 hides spell particles without changing what
/// `/showspelleffects` says. This client draws particles fully or not at
/// all, so levels 1 to 3 all draw them.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParticleDensity(pub(crate) Option<u8>);

impl ParticleDensity {
    /// Whether spell particles are drawn at this density.
    pub(crate) fn draws(self) -> bool {
        self.0 != Some(0)
    }
}

/// The colour kinds' names, 1 to 69, in this client's words: the
/// installation's `User_<n>` is chat kind n + 255, named as `EQEmu` names
/// its chat kinds.
pub(crate) const KIND_NAMES: [&str; LISTED_KINDS as usize] = [
    "Say",
    "Tell",
    "Group",
    "Guild",
    "Out of character",
    "Auction",
    "Shout",
    "Emote",
    "Spells",
    "You hit others",
    "Others hit you",
    "You miss others",
    "Others miss you",
    "Broadcasts",
    "Skills",
    "Disciplines",
    "Unused",
    "Default text",
    "Unused",
    "Merchant offers",
    "Merchant buys and sells",
    "Your death",
    "Others' deaths",
    "Others' hits",
    "Others' misses",
    "Who",
    "Yell for help",
    "Non-melee damage",
    "Spells wearing off",
    "Coin splits",
    "Loot",
    "Dice rolls",
    "Others' spells",
    "Spell failures",
    "Chat channels",
    "Chat channel 1",
    "Chat channel 2",
    "Chat channel 3",
    "Chat channel 4",
    "Chat channel 5",
    "Chat channel 6",
    "Chat channel 7",
    "Chat channel 8",
    "Chat channel 9",
    "Chat channel 10",
    "Melee critical hits",
    "Spell critical hits",
    "Too far away",
    "Rampages",
    "Flurries",
    "Enrages",
    "Your say",
    "Your tells",
    "Your group chat",
    "Your guild chat",
    "Your out-of-character chat",
    "Your auctions",
    "Your shouts",
    "Your emotes",
    "Your channel 1",
    "Your channel 2",
    "Your channel 3",
    "Your channel 4",
    "Your channel 5",
    "Your channel 6",
    "Your channel 7",
    "Your channel 8",
    "Your channel 9",
    "Your channel 10",
];

/// The usage lines a failed `/usercolor` says, in this client's words.
const USER_COLOR_USAGE: [&str; 5] = [
    "Usage: /usercolor <kind> <red> <green> <blue>",
    "The kind is a number from 1 to 69.",
    "Red, green and blue are each from 0 to 255.",
    "/usercolor <kind> reset puts a kind's colour back.",
    "/usercolor help, or page1 to page7, lists the kinds.",
];

/// Runs an on/off command: it sets `toggle` when a word was given, else
/// says the usage lines; either way it ends with where the toggle stands.
fn toggle_lines(
    options: &mut eq_client_core::options::Options,
    toggle: Toggle,
    word: Option<bool>,
    lines: &ToggleLines,
    said: &dyn Fn(u32, &str) -> crate::chat::Said,
) -> Vec<crate::chat::Said> {
    let mut said_lines = Vec::new();
    if let Some(on) = word {
        options.set(toggle, on);
    } else {
        said_lines.extend(lines.usage.iter().map(|&(id, fallback)| said(id, fallback)));
    }
    let [on, off] = lines.state;
    let (id, fallback) = if options.get(toggle) { on } else { off };
    said_lines.push(said(id, fallback));
    said_lines
}

/// Runs a typed local-settings command and says its lines.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn apply(
    mut chat: ResMut<crate::chat::ChatState>,
    messages: Option<Res<crate::hud::messages::Messages>>,
    mut options: ResMut<crate::options::OptionsState>,
    mut density: ResMut<ParticleDensity>,
    look: Option<Res<crate::chat::ChatLook>>,
) {
    let Some((setting, tab)) = chat.local_setting.take() else {
        return;
    };
    let said = |id: u32, arguments: &[String], fallback: &str| {
        messages.as_deref().map_or_else(
            || crate::chat::Said::own(fallback),
            |messages| messages.said_or(id, arguments, fallback),
        )
    };
    let own = |text: String| crate::chat::Said::own(text);
    let mut lines = Vec::new();
    match setting {
        LocalSetting::ClearChat => chat.clear_view(tab),
        LocalSetting::SpellEffects(word) => lines.extend(toggle_lines(
            &mut options.options,
            Toggle::SpellEffects,
            word,
            &SPELL_EFFECTS,
            &|id, fallback| said(id, &[], fallback),
        )),
        LocalSetting::CombatMusic(word) => lines.extend(toggle_lines(
            &mut options.options,
            Toggle::CombatMusic,
            word,
            &COMBAT_MUSIC,
            &|id, fallback| said(id, &[], fallback),
        )),
        LocalSetting::ParticleDensity(None) => lines.push(own(
            "Usage: /particledensity <0-3, or off, low, medium or high>".into(),
        )),
        LocalSetting::ParticleDensity(Some((level, typed))) => {
            density.0 = Some(level);
            lines.push(own(format!(
                "Particle density is now {typed}. This client draws spell particles fully, or not at all at 0."
            )));
        }
        LocalSetting::MouseSpeed(speed) => {
            if let Some(speed) = speed {
                options.options.mouse_pointer_speed = Some(speed);
            } else {
                lines.push(said(
                    MOUSE_SPEED_USAGE[0],
                    &[],
                    "Usage: /mousespeed <number>",
                ));
                lines.push(said(
                    MOUSE_SPEED_USAGE[1],
                    &[],
                    "Sets the mouse pointer's speed.",
                ));
            }
            let now = options
                .options
                .mouse_pointer_speed
                .unwrap_or(0.0)
                .to_string();
            lines.push(said(
                MOUSE_SPEED_NOW,
                std::slice::from_ref(&now),
                &format!("The mouse pointer speed is {now}. This client leaves the pointer to the system."),
            ));
        }
        LocalSetting::ChatFontSize(Some(size)) => options.options.chat_font_size = Some(size),
        LocalSetting::ChatFontSize(None) => {
            lines.push(said(CHAT_FONT_USAGE, &[], "Usage: /chatfontsize <0-6>"));
        }
        LocalSetting::UserColor(asked) => {
            user_color_lines(asked, &mut options, look.as_deref(), &mut lines);
        }
        LocalSetting::InspectOthers(word) => lines.extend(toggle_lines(
            &mut options.options,
            Toggle::InspectOthers,
            word,
            &INSPECT,
            &|id, fallback| said(id, &[], fallback).typed(INSPECT_COLOUR),
        )),
    }
    for line in lines {
        chat.history.push(crate::chat::system_line(line));
    }
}

/// Applies a `/usercolor` and says its lines, in this client's words.
fn user_color_lines(
    asked: UserColor,
    options: &mut crate::options::OptionsState,
    look: Option<&crate::chat::ChatLook>,
    lines: &mut Vec<crate::chat::Said>,
) {
    let own = |text: &str| crate::chat::Said::own(text.to_owned());
    match asked {
        UserColor::Usage => lines.extend(USER_COLOR_USAGE.iter().map(|line| own(line))),
        UserColor::Help(page) => {
            let first = (usize::from(page) - 1) * 10;
            for (index, name) in KIND_NAMES.iter().enumerate().skip(first).take(10) {
                lines.push(crate::chat::Said::own(format!("{} - {name}", index + 1)));
            }
        }
        // Kind 0 has no slot: it is taken, and nothing is written.
        UserColor::Reset(0) | UserColor::Set { kind: 0, .. } => {}
        UserColor::Reset(kind) => options.options.user_colors[usize::from(kind)] = None,
        UserColor::Set { kind, rgb } => {
            let slot = &mut options.options.user_colors[usize::from(kind)];
            // A channel left as it was keeps the colour the kind shows now.
            let mut color = slot
                .or_else(|| look.and_then(|look| look.message_color(u32::from(kind) + 255)))
                .unwrap_or([255, 255, 255]);
            for (channel, value) in color.iter_mut().zip(rgb) {
                if let Some(value) = value {
                    *channel = value;
                }
            }
            *slot = Some(color);
        }
    }
}

/// Registers the commands' system.
pub(super) fn register(app: &mut App) {
    app.init_resource::<ParticleDensity>()
        .add_systems(Update, apply.in_set(super::Stage::Present));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatState;
    use crate::options::OptionsState;
    use eq_client_core::chat::ChatTab;

    fn typed(line: &str) -> LocalSetting {
        LocalSetting::typed(line).expect(line)
    }

    #[test]
    fn each_command_reads_its_words_as_the_program_does() {
        assert_eq!(typed("/clearchat"), LocalSetting::ClearChat);
        assert_eq!(typed("/clearchat now"), LocalSetting::ClearChat);
        assert_eq!(typed("/showspelleffects"), LocalSetting::SpellEffects(None));
        assert_eq!(
            typed("/showspelleffects OFF"),
            LocalSetting::SpellEffects(Some(false))
        );
        // Any word but "off" turns it on: there is no toggle.
        assert_eq!(
            typed("/showspelleffects maybe"),
            LocalSetting::SpellEffects(Some(true))
        );
        assert_eq!(typed("/combatmusic"), LocalSetting::CombatMusic(None));
        assert_eq!(
            typed("/combatmusic off"),
            LocalSetting::CombatMusic(Some(false))
        );
        assert_eq!(
            typed("/combatmusic on"),
            LocalSetting::CombatMusic(Some(true))
        );
        assert_eq!(typed("/mousespeed"), LocalSetting::MouseSpeed(None));
        assert_eq!(typed("/mousespeed fast"), LocalSetting::MouseSpeed(None));
        assert_eq!(typed("/mousespeed inf"), LocalSetting::MouseSpeed(None));
        assert_eq!(typed("/mousespeed NaN"), LocalSetting::MouseSpeed(None));
        // No range check.
        assert_eq!(
            typed("/mousespeed -40.5"),
            LocalSetting::MouseSpeed(Some(-40.5))
        );
        assert_eq!(
            typed("/chatfontsize 6"),
            LocalSetting::ChatFontSize(Some(6))
        );
        assert_eq!(typed("/chatfontsize 7"), LocalSetting::ChatFontSize(None));
        assert_eq!(typed("/chatfontsize big"), LocalSetting::ChatFontSize(None));
        assert_eq!(typed("/chatfontsize"), LocalSetting::ChatFontSize(None));
        assert_eq!(LocalSetting::typed("/clear"), None);
        assert_eq!(LocalSetting::typed("clearchat"), None);
    }

    #[test]
    fn particle_density_takes_a_level_or_a_word_of_three_letters() {
        for (word, level) in [
            ("0", 0),
            ("3", 3),
            ("off", 0),
            ("LOW", 1),
            ("medium", 2),
            ("Higher", 3),
        ] {
            assert_eq!(
                typed(&format!("/particledensity {word}")),
                LocalSetting::ParticleDensity(Some((level, word.to_owned()))),
                "{word}",
            );
        }
        for word in ["", " 4", " of", " up"] {
            assert_eq!(
                typed(&format!("/particledensity{word}")),
                LocalSetting::ParticleDensity(None),
                "{word:?}",
            );
        }
    }

    #[test]
    fn user_color_reads_kinds_channels_resets_and_pages() {
        let color = |line: &str| match typed(line) {
            LocalSetting::UserColor(asked) => asked,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            color("/usercolor 4 10 20 30"),
            UserColor::Set {
                kind: 4,
                rgb: [Some(10), Some(20), Some(30)]
            }
        );
        // A word that is not a number keeps its channel.
        assert_eq!(
            color("/usercolor 4 x 20"),
            UserColor::Set {
                kind: 4,
                rgb: [None, Some(20), None]
            }
        );
        // Kinds 0 and 70 to 86 pass the range check.
        assert_eq!(
            color("/usercolor 0 1 2 3"),
            UserColor::Set {
                kind: 0,
                rgb: [Some(1), Some(2), Some(3)]
            }
        );
        assert_eq!(color("/usercolor 86 reset"), UserColor::Reset(86));
        assert_eq!(color("/usercolor 87 1 2 3"), UserColor::Usage);
        assert_eq!(color("/usercolor 4 256 0 0"), UserColor::Usage);
        assert_eq!(color("/usercolor 4 -1 0 0"), UserColor::Usage);
        assert_eq!(color("/usercolor"), UserColor::Usage);
        assert_eq!(color("/usercolor red"), UserColor::Usage);
        assert_eq!(color("/usercolor help"), UserColor::Help(1));
        assert_eq!(color("/usercolor PAGE7"), UserColor::Help(7));
        assert_eq!(color("/usercolor page8"), UserColor::Usage);
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .insert_resource(OptionsState::default())
            .init_resource::<ParticleDensity>()
            .add_systems(Update, apply);
        app
    }

    /// Runs a typed line and gives the lines it said.
    #[test]
    fn the_seven_are_reached_only_as_the_command_table_resolves_them() {
        // The chat spells a typed word as the installed client's table
        // resolves it (0x78a0cc) before it asks for a local setting.
        let reached = |line: &str| {
            crate::commands::spelled(line)
                .and_then(|spelled| LocalSetting::typed(&spelled))
                .is_some()
        };
        for name in [
            "clearchat",
            "showspelleffects",
            "combatmusic",
            "particledensity",
            "mousespeed",
            "chatfontsize",
            "usercolor",
        ] {
            assert_eq!(
                crate::commands::resolve(name),
                crate::commands::Resolved::Official(name),
                "{name}"
            );
            assert!(reached(&format!("/{name} 1")), "{name}");
        }
        // Shorter words the table gives them reach them too, as the
        // program's parser does.
        for line in [
            "/clear",
            "/shows",
            "/combat",
            "/particle",
            "/mouse",
            "/chatf",
            "/user",
        ] {
            assert!(reached(line), "{line}");
        }
        // A word the table gives an earlier name never does.
        for line in ["/c", "/co", "/cha", "/chat", "/show"] {
            assert!(!reached(line), "{line}");
        }
    }

    fn run(app: &mut App, line: &str) -> Vec<String> {
        let before = app
            .world()
            .resource::<ChatState>()
            .history
            .lines(ChatTab::All)
            .len();
        app.world_mut().resource_mut::<ChatState>().local_setting =
            Some((typed(line), ChatTab::All));
        app.update();
        let chat = app.world().resource::<ChatState>();
        assert!(chat.local_setting.is_none());
        chat.history
            .lines(ChatTab::All)
            .into_iter()
            .skip(before)
            .map(|(_, line)| line.message.text.clone())
            .collect()
    }

    const NO_LINES: [&str; 0] = [];

    fn options(app: &App) -> eq_client_core::options::Options {
        app.world().resource::<OptionsState>().options
    }

    #[test]
    fn spell_effects_and_combat_music_say_where_they_stand() {
        let mut app = app();
        assert_eq!(
            run(&mut app, "/showspelleffects off"),
            ["Spell effects are off."]
        );
        assert!(!options(&app).get(Toggle::SpellEffects));
        assert_eq!(
            run(&mut app, "/showspelleffects"),
            [
                "Usage: /showspelleffects <on|off>",
                "Turns spell effects on or off.",
                "Spell effects are off.",
            ]
        );
        assert_eq!(
            run(&mut app, "/showspelleffects yes"),
            ["Spell effects are on."]
        );
        assert!(options(&app).get(Toggle::SpellEffects));
        assert_eq!(run(&mut app, "/combatmusic off"), ["Combat music is off."]);
        assert!(!options(&app).get(Toggle::CombatMusic));
        assert_eq!(
            run(&mut app, "/combatmusic"),
            ["Usage: /combatmusic <on|off>", "Combat music is off."]
        );
    }

    #[test]
    fn toggleinspect_takes_on_and_off_and_says_where_it_stands() {
        assert_eq!(typed("/toggleinspect"), LocalSetting::InspectOthers(None));
        assert_eq!(
            typed("/ToggleInspect OFF"),
            LocalSetting::InspectOthers(Some(false))
        );
        assert_eq!(
            typed("/toggleinspect on now"),
            LocalSetting::InspectOthers(Some(true))
        );
        // Unlike `/showspelleffects`, any other word changes nothing.
        assert_eq!(
            typed("/toggleinspect yes"),
            LocalSetting::InspectOthers(None)
        );
        let mut app = app();
        // On until set otherwise, as the program's missing key is TRUE.
        assert!(options(&app).get(Toggle::InspectOthers));
        assert_eq!(
            run(&mut app, "/toggleinspect off"),
            ["Right-clicking a player does not inspect them."]
        );
        assert!(!options(&app).get(Toggle::InspectOthers));
        assert_eq!(
            run(&mut app, "/toggleinspect maybe"),
            [
                "Usage: /toggleinspect <on|off>",
                "Turns inspecting other players by right-clicking them on or off.",
                "Right-clicking a player does not inspect them.",
            ]
        );
        assert!(!options(&app).get(Toggle::InspectOthers));
        assert_eq!(
            run(&mut app, "/toggleinspect on"),
            ["Right-clicking a player inspects them."]
        );
        assert!(options(&app).get(Toggle::InspectOthers));
    }

    #[test]
    fn particle_density_is_its_own_flag_for_the_session() {
        let mut app = app();
        let said = run(&mut app, "/particledensity off");
        assert_eq!(said.len(), 1);
        assert!(
            said[0].starts_with("Particle density is now off."),
            "{said:?}"
        );
        assert_eq!(
            *app.world().resource::<ParticleDensity>(),
            ParticleDensity(Some(0))
        );
        assert!(!app.world().resource::<ParticleDensity>().draws());
        // /showspelleffects still says what it was.
        assert!(options(&app).get(Toggle::SpellEffects));
        assert_eq!(
            run(&mut app, "/showspelleffects on"),
            ["Spell effects are on."]
        );
        run(&mut app, "/particledensity 2");
        assert!(app.world().resource::<ParticleDensity>().draws());
        let said = run(&mut app, "/particledensity lots");
        assert!(said[0].starts_with("Usage: /particledensity"), "{said:?}");
        assert_eq!(
            *app.world().resource::<ParticleDensity>(),
            ParticleDensity(Some(2))
        );
        assert!(ParticleDensity::default().draws());
    }

    #[test]
    fn mouse_speed_is_kept_as_typed_and_reported() {
        let mut app = app();
        let said = run(&mut app, "/mousespeed 12.5");
        assert_eq!(said.len(), 1);
        assert!(
            said[0].starts_with("The mouse pointer speed is 12.5."),
            "{said:?}"
        );
        assert_eq!(options(&app).mouse_pointer_speed, Some(12.5));
        let said = run(&mut app, "/mousespeed fast");
        assert_eq!(said.len(), 3);
        assert_eq!(said[0], "Usage: /mousespeed <number>");
        assert!(said[2].starts_with("The mouse pointer speed is 12.5."));
        assert_eq!(options(&app).mouse_pointer_speed, Some(12.5));
    }

    #[test]
    fn chat_font_size_sets_quietly_or_says_its_usage() {
        let mut app = app();
        assert_eq!(run(&mut app, "/chatfontsize 4"), NO_LINES);
        assert_eq!(options(&app).chat_font_size, Some(4));
        assert_eq!(
            run(&mut app, "/chatfontsize 9"),
            ["Usage: /chatfontsize <0-6>"]
        );
        assert_eq!(options(&app).chat_font_size, Some(4));
    }

    #[test]
    fn user_color_sets_only_slots_it_has() {
        let mut app = app();
        assert_eq!(run(&mut app, "/usercolor 4 10 20 30"), NO_LINES);
        assert_eq!(options(&app).user_colors[4], Some([10, 20, 30]));
        assert_eq!(run(&mut app, "/usercolor 4 x 99"), NO_LINES);
        assert_eq!(options(&app).user_colors[4], Some([10, 99, 30]));
        assert_eq!(run(&mut app, "/usercolor 4 reset"), NO_LINES);
        assert_eq!(options(&app).user_colors[4], None);
        // Kind 0 is taken without a line and writes nothing.
        let before = options(&app).user_colors;
        assert_eq!(run(&mut app, "/usercolor 0 1 2 3"), NO_LINES);
        assert_eq!(options(&app).user_colors, before);
        assert_eq!(run(&mut app, "/usercolor 86 1 2 3"), NO_LINES);
        assert_eq!(run(&mut app, "/usercolor 87 1 2 3"), USER_COLOR_USAGE);
        assert_eq!(run(&mut app, "/usercolor 87 1 2 3").len(), 5);
        let page = run(&mut app, "/usercolor help");
        assert_eq!(page.len(), 10);
        assert_eq!(page[0], "1 - Say");
        let last = run(&mut app, "/usercolor page7");
        assert_eq!(
            last,
            [
                "61 - Your channel 2",
                "62 - Your channel 3",
                "63 - Your channel 4",
                "64 - Your channel 5",
                "65 - Your channel 6",
                "66 - Your channel 7",
                "67 - Your channel 8",
                "68 - Your channel 9",
                "69 - Your channel 10"
            ]
        );
    }

    #[test]
    fn clear_chat_hides_the_tab_but_keeps_the_history() {
        let mut app = app();
        {
            let mut chat = app.world_mut().resource_mut::<ChatState>();
            chat.history
                .push(crate::chat::system_line(crate::chat::Said::own("kept")));
        }
        assert_eq!(run(&mut app, "/clearchat"), NO_LINES);
        let chat = app.world().resource::<ChatState>();
        let lines = chat.history.lines(ChatTab::All);
        assert_eq!(lines.len(), 1, "the history keeps the line");
        assert_eq!(chat.cleared_before(ChatTab::All), lines[0].0);
    }

    #[test]
    fn a_script_or_a_typed_line_reaches_the_commands_locally() {
        let mut chat = ChatState::default();
        assert_eq!(
            crate::chat::client_request("/clearchat", &mut chat),
            Some(Ok(()))
        );
        assert_eq!(
            chat.local_setting,
            Some((LocalSetting::ClearChat, ChatTab::All))
        );
    }
}

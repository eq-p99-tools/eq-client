//! Named social definitions and the supported five-line pause syntax.
use std::{collections::BTreeMap, fmt::Write as _, time::Duration};

const HEADER: &str = "# eq-client socials v1";
// Titanium's saved socials occupy ten pages of twelve records.
const MAX_SOCIALS: usize = 120;
const MAX_LINES: usize = 5;
const MAX_LINE_BYTES: usize = 512;
/// The runner's step (char tick 0x422198, read R45b): it runs at most every
/// ~100 ms, and an ordinary line waits one step, so lines go about 100 ms
/// apart.
pub(super) const STEP: Duration = Duration::from_millis(100);
/// `/pause N` waits N steps, clamped to 1-600 (R45b).
const PAUSE_STEPS: std::ops::RangeInclusive<i64> = 1..=600;

#[derive(Default)]
pub(super) struct Catalog {
    serial: u32,
    pub(super) entries: BTreeMap<u32, Definition>,
}

pub(crate) struct Definition {
    pub(super) name: String,
    pub(super) lines: Vec<String>,
    pub(super) program: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Line {
    pub(super) command: Option<String>,
    pub(super) pause: Duration,
}

impl Definition {
    fn new(name: &str, lines: impl Iterator<Item = String>) -> Result<Self, String> {
        validate_name(name)?;
        let lines: Vec<_> = lines.take(MAX_LINES + 1).collect();
        if !(1..=MAX_LINES).contains(&lines.len()) {
            return Err("A social needs one to five lines".into());
        }
        let program = lines
            .iter()
            .map(|line| Line::parse(line))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            name: name.into(),
            lines,
            program,
        })
    }
}

impl Line {
    /// An ordinary line waits one step before the next. `/pause N` waits N
    /// steps, clamped to 1-600, and a command after its comma runs at once,
    /// before the wait (R45b).
    fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_LINE_BYTES || text.chars().any(char::is_control) {
            return Err("A social line contains invalid text".into());
        }
        let (name, rest) = command(text)?;
        if !name.eq_ignore_ascii_case("pause") {
            return Ok(Self {
                command: Some(action(text)?),
                pause: STEP,
            });
        }
        let (number, inline) = rest
            .split_once(',')
            .map_or((rest, None), |(number, inline)| {
                (number, Some(inline.trim()))
            });
        let steps = number
            .trim()
            .parse::<i64>()
            .map_err(|_| "Use /pause and a number of tenths of a second, 1 to 600")?
            .clamp(*PAUSE_STEPS.start(), *PAUSE_STEPS.end());
        let command = inline.map(action).transpose()?;
        Ok(Self {
            command,
            pause: STEP * u32::try_from(steps).expect("the clamp keeps it within 1-600"),
        })
    }
}

fn command(text: &str) -> Result<(&str, &str), String> {
    let command = text
        .strip_prefix('/')
        .ok_or("Each social line must start with /")?;
    let (name, rest) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    if name.is_empty() {
        return Err("A social line needs a command".into());
    }
    Ok((name, rest.trim()))
}

fn action(text: &str) -> Result<String, String> {
    use crate::commands::{Resolved, resolve};
    let (name, _) = command(text)?;
    // `/target`, `/assist`, `/camp` and `/quit` run as the original's
    // socials run them (Adam 11-19): a later line acts on whatever target
    // the earlier one leaves, and a departure stops the social. `/exit`
    // stays refused, as the choice named only those four.
    if matches!(
        resolve(name),
        Resolved::Official("exit") | Resolved::Ours("pause" | "social")
    ) {
        return Err(format!("/{name} cannot be run inside a social"));
    }
    Ok(text.into())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 24
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(
            "Use 1 to 24 letters, numbers, hyphens or underscores for a social name".into(),
        );
    }
    Ok(())
}

impl Catalog {
    pub(super) fn named(&self, name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|(_, social)| social.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
    }

    /// Edits preserve their binding ID. Deleting a social retires its ID.
    pub(super) fn set(&mut self, social: Definition) -> Result<(), String> {
        let id = if let Some(id) = self.named(&social.name) {
            id
        } else {
            if self.entries.len() >= MAX_SOCIALS {
                return Err("The social list is full".into());
            }
            self.serial = self
                .serial
                .checked_add(1)
                .ok_or("Social ID space is exhausted")?;
            self.serial
        };
        self.entries.insert(id, social);
        Ok(())
    }

    /// Writes `/hotbutton`'s social: into the one of the same name, letter
    /// case aside, its first line only, as the program writes only line 1
    /// there; or else into a new one. Gives the social's ID.
    pub(super) fn hot_button(&mut self, asked: &HotButton) -> Result<u32, HotButtonRefusal> {
        let invalid = HotButtonRefusal::Invalid;
        validate_name(&asked.name).map_err(invalid)?;
        let line = Line::parse(&asked.text).map_err(invalid)?;
        if let Some(id) = self.named(&asked.name) {
            let entry = self.entries.get_mut(&id).expect("named gives a held ID");
            entry.lines[0].clone_from(&asked.text);
            entry.program[0] = line;
            return Ok(id);
        }
        if self.entries.len() >= MAX_SOCIALS {
            return Err(HotButtonRefusal::Full);
        }
        let social =
            Definition::new(&asked.name, std::iter::once(asked.text.clone())).map_err(invalid)?;
        self.set(social).map_err(invalid)?;
        Ok(self.named(&asked.name).expect("the social was just set"))
    }

    /// A malformed file is rejected as a unit so a future save cannot silently
    /// drop an entry. Tab separators are reserved by the command validation.
    pub(super) fn read(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err("Unrecognized social settings format".into());
        }
        let serial: u32 = lines
            .next()
            .and_then(|line| line.strip_prefix("serial "))
            .and_then(|number| number.parse().ok())
            .ok_or("Invalid social settings serial")?;
        let mut catalog = Self {
            serial,
            ..Self::default()
        };
        for row in lines {
            let mut fields = row.split('\t');
            let id: u32 = fields
                .next()
                .and_then(|field| field.parse().ok())
                .filter(|id| (1..=serial).contains(id))
                .ok_or("Invalid saved social ID")?;
            let name = fields.next().ok_or("A saved social has no name")?;
            let entry = Definition::new(name, fields.map(str::to_owned))?;
            if catalog.entries.len() >= MAX_SOCIALS
                || catalog.named(name).is_some()
                || catalog.entries.contains_key(&id)
            {
                return Err("Duplicate or excessive social settings entries".into());
            }
            catalog.entries.insert(id, entry);
        }
        Ok(catalog)
    }

    pub(super) fn text(&self) -> String {
        let mut text = format!("{HEADER}\nserial {}\n", self.serial);
        for (id, entry) in &self.entries {
            let _ = writeln!(text, "{id}\t{}\t{}", entry.name, entry.lines.join("\t"));
        }
        text
    }
}

/// What `/hotbutton NAME [COLOUR] TEXT` writes (0x4735cc and 0x46bc70,
/// read R45 and R45b): the name, cut to the program's 15 characters, and
/// the first line. An all-digit word after the name is the colour, which
/// this client's socials don't keep or draw (recording checklist 6.487).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HotButton {
    pub(super) name: String,
    pub(super) text: String,
}

/// The program's longest social name (the entry's 16-byte name field).
const HOT_BUTTON_NAME: usize = 15;
/// The program's longest social line (its 256-byte field).
const HOT_BUTTON_TEXT: usize = 255;

impl HotButton {
    /// None when no name is given, for the help lines.
    fn parse(words: &str) -> Option<Self> {
        let words = words.trim();
        let (name, rest) = words.split_once(char::is_whitespace).unwrap_or((words, ""));
        if name.is_empty() {
            return None;
        }
        let rest = rest.trim_start();
        let (colour, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let text = if !colour.is_empty() && colour.bytes().all(|byte| byte.is_ascii_digit()) {
            after.trim_start()
        } else {
            rest
        };
        Some(Self {
            name: name.chars().take(HOT_BUTTON_NAME).collect(),
            text: cut(text, HOT_BUTTON_TEXT).to_owned(),
        })
    }
}

/// The longest start of a text within a number of bytes.
fn cut(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Why `/hotbutton` wrote nothing.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum HotButtonRefusal {
    /// All 120 socials are taken: the program's line.
    Full,
    /// This client's socials can't hold the name or the line.
    Invalid(String),
}

pub(crate) enum Request {
    /// `/hotbutton`, or with no name its help.
    HotButton(Option<HotButton>),
    List,
    Show(String),
    Set(Definition),
    Bind {
        slot: usize,
        name: String,
    },
    Play(String),
    Delete(String),
    Cancel,
}

impl Request {
    pub(crate) fn parse(text: &str) -> Option<Result<Self, String>> {
        let (name, words) = command(text).ok()?;
        if name.eq_ignore_ascii_case("hotbutton") {
            return Some(Ok(Self::HotButton(HotButton::parse(words))));
        }
        name.eq_ignore_ascii_case("social")
            .then(|| Self::operation(words))
    }

    fn operation(words: &str) -> Result<Self, String> {
        let (verb, args) = words.split_once(char::is_whitespace).unwrap_or((words, ""));
        let args = args.trim();
        let named = || {
            validate_name(args)?;
            Ok::<_, String>(args.to_owned())
        };
        match verb.to_ascii_lowercase().as_str() {
            "list" if args.is_empty() => Ok(Self::List),
            "show" => named().map(Self::Show),
            "play" => named().map(Self::Play),
            "delete" => named().map(Self::Delete),
            "cancel" if args.is_empty() => Ok(Self::Cancel),
            "bind" => {
                let (slot, name) = args
                    .split_once(char::is_whitespace)
                    .ok_or("Use /social bind 1-10 Name")?;
                let slot = slot
                    .parse::<usize>()
                    .ok()
                    .filter(|slot| (1..=eq_client_core::hotbar::SLOTS).contains(slot))
                    .ok_or("Use a hotbar slot from 1 to 10")?
                    - 1;
                let name = name.trim();
                validate_name(name)?;
                Ok(Self::Bind {
                    slot,
                    name: name.into(),
                })
            }
            "set" => {
                let (name, lines) = args
                    .split_once(" | ")
                    .ok_or("Use /social set Name | /command")?;
                Definition::new(
                    name.trim(),
                    lines.split(" | ").map(|line| line.trim().to_owned()),
                )
                .map(Self::Set)
            }
            _ => Err("Use /social list, show, set, bind, play, delete, or cancel".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(catalog: &mut Catalog, text: &str) {
        let Request::Set(entry) = Request::parse(text).unwrap().unwrap() else {
            panic!("expected a definition");
        };
        catalog.set(entry).unwrap();
    }

    #[test]
    fn hotbutton_takes_a_name_an_optional_colour_and_the_rest_as_its_line() {
        let parsed = |words: &str| HotButton::parse(words);
        assert_eq!(parsed(""), None);
        assert_eq!(parsed("   "), None);
        assert_eq!(
            parsed("Heal 3 /cast 1"),
            Some(HotButton {
                name: "Heal".into(),
                text: "/cast 1".into(),
            })
        );
        // A word that isn't all digits starts the line.
        assert_eq!(parsed("Heal 3x /cast 1").unwrap().text, "3x /cast 1");
        assert_eq!(parsed("Sit /sit").unwrap().text, "/sit");
        // The name is cut to 15 characters, the line to 255 bytes.
        let long = parsed(&format!("ABCDEFGHIJKLMNOPQRS /say {}", "x".repeat(300))).unwrap();
        assert_eq!(long.name, "ABCDEFGHIJKLMNO");
        assert_eq!(long.text.len(), 255);
        assert_eq!(parsed("Name").unwrap().text, "");
    }

    #[test]
    fn hotbutton_rewrites_line_one_of_the_same_name_or_takes_a_new_social() {
        let mut catalog = Catalog::default();
        set(&mut catalog, "/social set Heal | /cast 1 | /sit");
        let id = catalog.named("Heal").unwrap();
        let asked = |name: &str, text: &str| HotButton {
            name: name.into(),
            text: text.into(),
        };
        assert_eq!(catalog.hot_button(&asked("HEAL", "/cast 2")), Ok(id));
        let entry = &catalog.entries[&id];
        assert_eq!(entry.name, "Heal");
        assert_eq!(entry.lines, ["/cast 2", "/sit"]);
        assert_eq!(entry.program[0].command.as_deref(), Some("/cast 2"));
        let new = catalog.hot_button(&asked("Rest", "/sit")).unwrap();
        assert_ne!(new, id);
        assert_eq!(catalog.entries[&new].lines, ["/sit"]);
        assert!(matches!(
            catalog.hot_button(&asked("Bad", "say hi")),
            Err(HotButtonRefusal::Invalid(_))
        ));
        // All 120 taken: the program's full table; the same name still writes.
        for index in catalog.entries.len()..MAX_SOCIALS {
            set(&mut catalog, &format!("/social set S{index} | /sit"));
        }
        assert_eq!(
            catalog.hot_button(&asked("Another", "/sit")),
            Err(HotButtonRefusal::Full)
        );
        assert_eq!(catalog.hot_button(&asked("rest", "/stand")), Ok(new));
    }

    #[test]
    fn standalone_and_inline_pauses_are_measured_in_tenths() {
        let after = Line::parse("/pause 25, /say Hello, friend").unwrap();
        assert_eq!(after.command.as_deref(), Some("/say Hello, friend"));
        assert_eq!(after.pause, Duration::from_millis(2500));
        let alone = Line::parse("/pause 600").unwrap();
        assert!(alone.command.is_none());
        assert_eq!(alone.pause, Duration::from_secs(60));
        // The program clamps the count to 1-600 rather than refusing it.
        for (text, pause) in [
            ("/pause 0", STEP),
            ("/pause -3", STEP),
            ("/pause 601", Duration::from_secs(60)),
            ("/pause 99999999999", Duration::from_secs(60)),
        ] {
            assert_eq!(Line::parse(text).unwrap().pause, pause, "{text}");
        }
        // An ordinary line waits one step.
        assert_eq!(Line::parse("/sit").unwrap().pause, STEP);
        for text in [
            "/pause",
            "/pause soon",
            "/pause 1,",
            "/pause 1,/pause 2",
            "/social play Loop",
            "/exit",
            "hello",
        ] {
            assert!(Line::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn targets_and_departures_run_in_a_social_as_the_originals_do() {
        for text in [
            "/social set Pull | /target Missing | /ranged",
            "/social set Pull | /pause 1,/TARGET Missing | /harmtouch",
            "/social set Pull | /assist Friend | /layonhands",
            "/social set Out | /camp",
            "/social set Out | /quit",
        ] {
            assert!(Request::parse(text).unwrap().is_ok(), "{text}");
        }
        assert!(Request::parse("/social set Out | /exit").unwrap().is_err());
        assert!(
            Request::parse("/social set Safe | /ranged")
                .unwrap()
                .is_ok()
        );
    }

    #[test]
    fn round_trip_retains_edits_and_retired_binding_ids() {
        let mut catalog = Catalog::default();
        set(
            &mut catalog,
            "/social set Hello | /say héllo | /pause 1,/stand",
        );
        let first = catalog.named("hello").unwrap();
        set(&mut catalog, "/social set HELLO | /say Edited");
        assert_eq!(catalog.named("Hello"), Some(first));
        let mut read = Catalog::read(&catalog.text()).unwrap();
        assert_eq!(read.entries[&first].lines, ["/say Edited"]);
        read.entries.remove(&first);
        let mut read = Catalog::read(&read.text()).unwrap();
        set(&mut read, "/social set Hello | /sit");
        assert_ne!(read.named("hello"), Some(first));
        assert!(Catalog::read(&format!("{HEADER}\nserial 0\n2\tHello\t/sit")).is_err());
    }

    #[test]
    fn all_120_socials_survive_reload_and_edits_keep_their_hotbar_binding() {
        use eq_client_core::hotbar::{Binding, Hotbar};

        let mut catalog = Catalog::default();
        for index in 1..=120 {
            set(&mut catalog, &format!("/social set Macro{index} | /sit"));
            if matches!(index, 100 | 101 | 120) {
                assert_eq!(Catalog::read(&catalog.text()).unwrap().entries.len(), index);
            }
        }
        let id = catalog.named("Macro101").unwrap();
        let mut hotbar = Hotbar::default();
        hotbar.0[0] = Some(Binding::Social(id));
        set(&mut catalog, "/social set MACRO101 | /stand");
        let saved = catalog.text();
        let reloaded = Catalog::read(&saved).unwrap();
        let reloaded_hotbar = Hotbar::read(&hotbar.text(), Hotbar::default());
        assert_eq!(reloaded_hotbar.0[0], Some(Binding::Social(id)));
        assert_eq!(reloaded.named("Macro101"), Some(id));
        assert_eq!(reloaded.entries[&id].lines, ["/stand"]);

        let overflow = Definition::new("Macro121", ["/sit".into()].into_iter()).unwrap();
        assert!(catalog.set(overflow).is_err());
        assert_eq!(catalog.text(), saved);
        let oversized = saved.replacen("serial 120", "serial 121", 1) + "121\tMacro121\t/sit\n";
        assert!(Catalog::read(&oversized).is_err());
    }
}

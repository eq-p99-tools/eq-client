//! Named social definitions and the supported five-line pause syntax.
use std::{collections::BTreeMap, fmt::Write as _, time::Duration};

const HEADER: &str = "# eq-client socials v1";
// Titanium's saved socials occupy ten pages of twelve records.
const MAX_SOCIALS: usize = 120;
const MAX_LINES: usize = 5;
const MAX_LINE_BYTES: usize = 512;

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
    /// A leading pause plus comma delays after the accompanying command.
    fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_LINE_BYTES || text.chars().any(char::is_control) {
            return Err("A social line contains invalid text".into());
        }
        let (name, rest) = command(text)?;
        if !name.eq_ignore_ascii_case("pause") {
            return Ok(Self {
                command: Some(action(text)?),
                pause: Duration::ZERO,
            });
        }
        let (number, inline) = rest
            .split_once(',')
            .map_or((rest, None), |(number, inline)| {
                (number, Some(inline.trim()))
            });
        let tenths = number
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|number| (1..=600).contains(number))
            .ok_or("Use /pause 1 through 600 (tenths of a second)")?;
        let command = inline.map(action).transpose()?;
        Ok(Self {
            command,
            pause: Duration::from_millis(tenths * 100),
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
    let (name, _) = command(text)?;
    if matches!(
        name.to_ascii_lowercase().as_str(),
        // Both resolve asynchronously and may leave the old target selected.
        "target"
            | "assist"
            | "pause"
            | "social"
            | "camp"
            | "ex"
            | "exi"
            | "exit"
            | "q"
            | "qu"
            | "qui"
            | "quit"
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

pub(crate) enum Request {
    List,
    Show(String),
    Set(Definition),
    Bind { slot: usize, name: String },
    Play(String),
    Delete(String),
    Cancel,
}

impl Request {
    pub(crate) fn parse(text: &str) -> Option<Result<Self, String>> {
        let (name, words) = command(text).ok()?;
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
    fn standalone_and_inline_pauses_are_measured_in_tenths() {
        let after = Line::parse("/pause 25, /say Hello, friend").unwrap();
        assert_eq!(after.command.as_deref(), Some("/say Hello, friend"));
        assert_eq!(after.pause, Duration::from_millis(2500));
        let alone = Line::parse("/pause 600").unwrap();
        assert!(alone.command.is_none());
        assert_eq!(alone.pause, Duration::from_secs(60));
        for text in [
            "/pause 0",
            "/pause 601",
            "/pause 1,",
            "/pause 1,/pause 2",
            "/social play Loop",
            "/quit",
            "/camp",
            "hello",
        ] {
            assert!(Line::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_deferred_target_change_cannot_leave_later_lines_aimed_at_an_old_target() {
        for text in [
            "/social set Unsafe | /target Missing | /ranged",
            "/social set Unsafe | /pause 1,/TARGET Missing | /harmtouch",
            "/social set Unsafe | /assist Friend | /layonhands",
        ] {
            assert!(Request::parse(text).unwrap().is_err(), "{text}");
        }
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

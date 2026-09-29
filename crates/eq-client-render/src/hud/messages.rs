//! Localized, argument-free messages from the user's EQ string table.
use bevy::prelude::Resource;
use std::{collections::BTreeMap, path::Path};

#[derive(Resource, Default)]
pub(crate) struct Messages(BTreeMap<u32, String>);

impl Messages {
    /// Missing or incompatible local files leave numeric server reasons available.
    pub(crate) fn load(directory: Option<&Path>) -> Self {
        directory
            .and_then(|directory| std::fs::read(directory.join("eqstr_us.txt")).ok())
            .map_or_else(Self::default, |bytes| {
                Self::parse(&String::from_utf8_lossy(&bytes))
            })
    }

    /// Skips the format header and count line rather than treating them as messages.
    pub(crate) fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some("EQST0002") {
            return Self::default();
        }
        lines.next();
        Self(
            lines
                .filter_map(|line| {
                    let (id, message) = line.split_once(char::is_whitespace)?;
                    let message = message.trim();
                    if message.is_empty() {
                        return None;
                    }
                    Some((id.parse().ok()?, message.to_owned()))
                })
                .collect(),
        )
    }

    /// Interrupt packets supply a reason ID but no format arguments; never invent them.
    pub(super) fn interruption(&self, id: u32) -> String {
        self.argument_free(id)
            .unwrap_or_else(|| format!("Casting interrupted (server reason {id})"))
    }

    /// Substitutes server-supplied `%1`..`%9` arguments into a local string.
    /// Missing strings stay identifiable instead of being silently dropped.
    pub(crate) fn format(&self, id: u32, arguments: &[String]) -> String {
        self.format_nested(id, arguments, 0)
    }

    fn format_nested(&self, id: u32, arguments: &[String], depth: u8) -> String {
        let Some(template) = self.0.get(&id) else {
            return if arguments.is_empty() {
                format!("Server message {id}")
            } else {
                format!("Server message {id}: {}", arguments.join(", "))
            };
        };
        // EQ placeholders: %1, %T1 (an argument naming another string, formatted
        // with the same arguments) and %B1(...) (bold argument).
        let mut text = String::with_capacity(template.len());
        let mut rest = template.as_str();
        while let Some(position) = rest.find('%') {
            text.push_str(&rest[..position]);
            let after = &rest[position + 1..];
            let (marker, digits) = match after.as_bytes().first() {
                Some(b'T' | b'B') => (1, &after[1..]),
                _ => (0, after),
            };
            let Some(index) = digits
                .chars()
                .next()
                .and_then(|digit| digit.to_digit(10))
                .filter(|digit| *digit != 0)
            else {
                text.push('%');
                rest = after;
                continue;
            };
            let mut consumed = marker + 1;
            if marker == 1
                && after.as_bytes().first() == Some(&b'B')
                && digits[1..].starts_with('(')
                && let Some(close) = digits[1..].find(')')
            {
                consumed += close + 1;
            }
            let nested = |argument: &String| {
                argument
                    .parse::<u32>()
                    .ok()
                    .filter(|nested| depth < 2 && self.0.contains_key(nested))
                    .map(|nested| self.format_nested(nested, arguments, depth + 1))
            };
            match arguments.get(usize::try_from(index - 1).unwrap_or(usize::MAX)) {
                Some(argument) if after.starts_with('T') => {
                    text.push_str(&nested(argument).unwrap_or_else(|| argument.clone()));
                }
                Some(argument) => text.push_str(argument),
                None => text.push_str(&rest[position..position + 1 + consumed]),
            }
            rest = &after[consumed..];
        }
        text.push_str(rest);
        text
    }

    /// Returns a local argument-free message, or the fallback when unavailable.
    pub(super) fn text(&self, id: u32, fallback: &str) -> String {
        self.argument_free(id).unwrap_or_else(|| fallback.into())
    }

    fn argument_free(&self, id: u32) -> Option<String> {
        self.0
            .get(&id)
            .filter(|text| {
                !text
                    .as_bytes()
                    .windows(2)
                    .any(|pair| pair[0] == b'%' && pair[1].is_ascii_digit())
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_malformed_and_parameterized_messages_keep_numeric_reasons() {
        let table = Messages::parse(
            "EQST0002\r\n0 3\r\n73 Synthetic failure\r\n74 %1 failed\r\ninvalid line\r\n75 \r\n",
        );
        assert_eq!(table.interruption(73), "Synthetic failure");
        for id in [0, 74, 75, 99] {
            assert_eq!(
                table.interruption(id),
                format!("Casting interrupted (server reason {id})")
            );
        }
        assert!(Messages::parse("OTHER\n0 1\n73 Wrong format").0.is_empty());
        assert_eq!(
            Messages::load(None).interruption(73),
            "Casting interrupted (server reason 73)"
        );
    }

    #[test]
    fn server_arguments_fill_numbered_placeholders_in_order() {
        let table = Messages::parse(
            "EQST0002
0 2
80 %1 hits %2 for %3.
81 100% sure, %9 missing
",
        );
        let arguments = ["A rat".to_owned(), "YOU".to_owned(), "2 points".to_owned()];
        assert_eq!(table.format(80, &arguments), "A rat hits YOU for 2 points.");
        assert_eq!(table.format(81, &[]), "100% sure, %9 missing");
        let table = Messages::parse(
            "EQST0002
0 1
82 You gained \"%B1(1)\" at a cost of %2 ability %T3.
",
        );
        assert_eq!(
            table.format(82, &["Innate".into(), "0".into(), "points".into()]),
            "You gained \"Innate\" at a cost of 0 ability points."
        );
        assert_eq!(table.format(99, &[]), "Server message 99");
        assert_eq!(table.format(99, &["x".to_owned()]), "Server message 99: x");
    }

    #[test]
    fn text_placeholders_naming_strings_are_formatted_with_the_same_arguments() {
        let table = Messages::parse(
            "EQST0002
0 2
554 %1 says '%T2'
1146 Greetings, %3. You look like you could use a %4.
",
        );
        let arguments = ["Rowyl", "1146", "Clauderic", "Bread"].map(str::to_owned);
        assert_eq!(
            table.format(554, &arguments),
            "Rowyl says 'Greetings, Clauderic. You look like you could use a Bread.'"
        );
        // Unknown ids and plain text stay literal.
        assert_eq!(
            table.format(554, &["Nura".into(), "9999".into()]),
            "Nura says '9999'"
        );
        assert_eq!(
            table.format(554, &["Nura".into(), "Hello".into()]),
            "Nura says 'Hello'"
        );
    }
}

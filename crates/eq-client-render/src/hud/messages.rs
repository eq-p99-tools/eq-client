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
        let Some(template) = self.0.get(&id) else {
            return if arguments.is_empty() {
                format!("Server message {id}")
            } else {
                format!("Server message {id}: {}", arguments.join(", "))
            };
        };
        let mut text = String::with_capacity(template.len());
        let mut characters = template.chars().peekable();
        while let Some(character) = characters.next() {
            let index = characters
                .peek()
                .and_then(|next| next.to_digit(10))
                .filter(|digit| character == '%' && *digit != 0);
            if let Some(index) = index {
                characters.next();
                if let Some(argument) =
                    arguments.get(usize::try_from(index - 1).unwrap_or(usize::MAX))
                {
                    text.push_str(argument);
                } else {
                    text.push('%');
                    text.push(char::from_digit(index, 10).unwrap_or('?'));
                }
            } else {
                text.push(character);
            }
        }
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
        assert_eq!(table.format(99, &[]), "Server message 99");
        assert_eq!(table.format(99, &["x".to_owned()]), "Server message 99: x");
    }
}

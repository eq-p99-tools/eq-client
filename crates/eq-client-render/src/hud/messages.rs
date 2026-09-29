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
    pub(super) fn parse(text: &str) -> Self {
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
}

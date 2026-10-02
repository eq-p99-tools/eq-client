//! The installed client's words for what servers refer to by number, and
//! the client's own words where the installation has none.
use bevy::prelude::Resource;
use eq_client_assets::strings::StringTable;
use std::path::Path;

#[derive(Resource, Default)]
pub(crate) struct Messages(StringTable);

impl Messages {
    /// Missing or incompatible local files leave numeric server reasons available.
    pub(crate) fn load(directory: Option<&Path>) -> Self {
        Self(
            directory
                .and_then(|directory| StringTable::read(directory).ok())
                .unwrap_or_default(),
        )
    }

    /// A table from the text of a string file.
    #[cfg(test)]
    pub(crate) fn parse(text: &str) -> Self {
        Self(StringTable::parse(text))
    }

    /// Interrupt packets supply a reason ID but no format arguments; never invent them.
    pub(super) fn interruption(&self, id: u32) -> String {
        self.0.argument_free(id).map_or_else(
            || format!("Casting interrupted (server reason {id})"),
            str::to_owned,
        )
    }

    /// A string with the server's arguments in it. Missing strings stay
    /// identifiable instead of being silently dropped.
    pub(crate) fn format(&self, id: u32, arguments: &[String]) -> String {
        self.0.format(id, arguments).unwrap_or_else(|| {
            if arguments.is_empty() {
                format!("Server message {id}")
            } else {
                format!("Server message {id}: {}", arguments.join(", "))
            }
        })
    }

    /// The official words for a string with what it names, or the fallback
    /// when the installation lacks the string.
    pub(crate) fn official(&self, id: u32, arguments: &[String], fallback: &str) -> String {
        self.0
            .format(id, arguments)
            .unwrap_or_else(|| fallback.to_owned())
    }

    /// Returns a local argument-free message, or the fallback when unavailable.
    pub(crate) fn text(&self, id: u32, fallback: &str) -> String {
        self.0
            .argument_free(id)
            .map_or_else(|| fallback.into(), str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_strings_keep_numeric_reasons() {
        let table = Messages::parse("EQST0002\n0 2\n73 Synthetic failure\n74 %1 failed\n");
        assert_eq!(table.interruption(73), "Synthetic failure");
        for id in [74, 99] {
            assert_eq!(
                table.interruption(id),
                format!("Casting interrupted (server reason {id})")
            );
        }
        assert_eq!(
            Messages::load(None).interruption(73),
            "Casting interrupted (server reason 73)"
        );
        assert_eq!(table.format(99, &[]), "Server message 99");
        assert_eq!(table.format(99, &["x".to_owned()]), "Server message 99: x");
        assert_eq!(table.format(74, &["It".to_owned()]), "It failed");
    }
}

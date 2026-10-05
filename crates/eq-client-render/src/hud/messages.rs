//! The installed client's words for what servers refer to by number, and
//! the client's own words where the installation has none.
use crate::chat::Said;
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

    /// Why a cast was interrupted, as the chat says it. Interrupt packets
    /// supply a reason ID but no format arguments; never invent them.
    pub(crate) fn interruption(&self, id: u32) -> Said {
        self.0.argument_free(id).map_or_else(
            || Said::own(format!("Casting interrupted (server reason {id})")),
            Said::official,
        )
    }

    /// A string with the server's arguments in it. Missing strings stay
    /// identifiable instead of being silently dropped.
    pub(crate) fn format(&self, id: u32, arguments: &[String]) -> String {
        self.said(id, arguments).words.text
    }

    /// A string with what it names, as the chat says it: the installed
    /// client's words, or, where the installation lacks the string, this
    /// client's words naming it.
    pub(crate) fn said(&self, id: u32, arguments: &[String]) -> Said {
        self.0.format(id, arguments).map_or_else(
            || {
                Said::own(if arguments.is_empty() {
                    format!("Server message {id}")
                } else {
                    format!("Server message {id}: {}", arguments.join(", "))
                })
            },
            Said::official,
        )
    }

    /// Formats server arguments without discarding their original item links.
    pub(crate) fn linked(&self, id: u32, arguments: &[eq_client_core::chat::RichText]) -> Said {
        use eq_client_core::chat::{RichText, Source};
        let plain: Vec<_> = arguments
            .iter()
            .map(|argument| argument.text.clone())
            .collect();
        if let Some(formatted) = self.0.format_with_spans(id, &plain) {
            let mut words = RichText::from(formatted.text);
            for span in formatted.arguments {
                words.insert_links(&arguments[span.index], span.range.start);
            }
            Said {
                words,
                source: Source::Official,
            }
        } else {
            let mut words = RichText::from(format!("Server message {id}"));
            for (index, argument) in arguments.iter().enumerate() {
                words.text.push_str(if index == 0 { ": " } else { ", " });
                let offset = words.text.len();
                words.text.push_str(&argument.text);
                words.insert_links(argument, offset);
            }
            Said {
                words,
                source: Source::Client,
            }
        }
    }

    /// The installed client's words for a string with what it names, or
    /// the fallback, in this client's words, where the installation lacks it.
    pub(crate) fn said_or(&self, id: u32, arguments: &[String], fallback: &str) -> Said {
        self.0
            .format(id, arguments)
            .map_or_else(|| Said::own(fallback), Said::official)
    }

    /// An argument-free string as the chat says it, or the fallback in this
    /// client's words where the installation lacks it.
    pub(crate) fn said_text(&self, id: u32, fallback: &str) -> Said {
        self.0
            .argument_free(id)
            .map_or_else(|| Said::own(fallback), Said::official)
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

    fn linked_argument(label: &str, id: u32) -> eq_client_core::chat::RichText {
        eq_client_core::chat::Message {
            message: None,
            message_hex: None,
            text: format!("a {label}"),
            item_links: vec![eq_client_core::chat::ItemLink {
                body: format!("{id:040X}"),
                text: label.into(),
                start: 2,
                end: 44 + label.len(),
                text_start: 2,
                text_end: 2 + label.len(),
                item_id: id,
            }],
        }
        .into()
    }

    #[test]
    fn formatted_notices_retain_clickable_links_through_the_final_chat_line() {
        use eq_client_core::chat::Source;
        let table = Messages::parse("EQST0002\n0 2\n1 %2 / %1 / %2 / %T3\n2 %1\n");
        let arguments = vec![
            linked_argument("Épée", 7),
            linked_argument("盾", 9),
            "2".into(),
        ];
        let notice = eq_client_core::world::Notice::ServerString {
            id: 1,
            arguments,
            message_type: Some(10),
        };
        let mut lines = crate::notices::wording(&notice, Some(&table));
        assert_eq!(lines.len(), 1);
        let line = crate::chat::system_line(lines.remove(0).1);
        assert_eq!(line.source, Source::Official);
        assert_eq!(line.message.text, "a 盾 / a Épée / a 盾 / a Épée");
        assert_eq!(
            line.message
                .item_links
                .iter()
                .map(|link| link.item_id)
                .collect::<Vec<_>>(),
            [9, 7, 9, 7]
        );
        for link in &line.message.item_links {
            assert_eq!(
                &line.message.text[link.text_start..link.text_end],
                link.text
            );
            assert_eq!(link.body, format!("{:040X}", link.item_id));
        }
    }

    #[test]
    fn missing_table_and_invalid_spans_keep_readable_text_without_bad_clicks() {
        let valid = linked_argument("Épée", 7);
        let mut invalid = linked_argument("盾", 9);
        invalid.item_links[0].text_start = 3; // Inside a multibyte label.
        let notice = eq_client_core::world::Notice::ServerString {
            id: 99,
            arguments: vec![valid, invalid],
            message_type: None,
        };
        let mut lines = crate::notices::wording(&notice, None);
        let line = crate::chat::system_line(lines.remove(0).1);
        assert_eq!(line.message.text, "Server message 99: a Épée, a 盾");
        assert_eq!(line.message.item_links.len(), 1);
        let annotation = &line.message.item_links[0];
        assert_eq!(
            &line.message.text[annotation.text_start..annotation.text_end],
            "Épée"
        );
    }

    #[test]
    fn missing_strings_keep_numeric_reasons() {
        let table = Messages::parse("EQST0002\n0 2\n73 Synthetic failure\n74 %1 failed\n");
        assert_eq!(table.interruption(73), Said::official("Synthetic failure"));
        // A string that takes arguments is never filled in with made-up ones.
        for id in [74, 99] {
            assert_eq!(
                table.interruption(id),
                Said::own(format!("Casting interrupted (server reason {id})"))
            );
        }
        assert_eq!(
            Messages::load(None).interruption(73),
            Said::own("Casting interrupted (server reason 73)")
        );
        assert_eq!(table.format(99, &[]), "Server message 99");
        assert_eq!(table.format(99, &["x".to_owned()]), "Server message 99: x");
        assert_eq!(table.format(74, &["It".to_owned()]), "It failed");
        // The chat knows the installed words from this client's.
        assert_eq!(table.said(73, &[]), Said::official("Synthetic failure"));
        assert_eq!(table.said(99, &[]), Said::own("Server message 99"));
        assert_eq!(table.said_or(99, &[], "Mine"), Said::own("Mine"));
        assert_eq!(table.said_text(74, "Mine"), Said::own("Mine"));
        assert_eq!(
            table.said_text(73, "Mine"),
            Said::official("Synthetic failure")
        );
    }
}

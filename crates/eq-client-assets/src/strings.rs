//! The installed client's string table, `eqstr_us.txt`: the words servers
//! refer to by number, with their argument placeholders.
use std::{collections::BTreeMap, io, path::Path};

/// Strings by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringTable(BTreeMap<u32, String>);

/// A substitution's UTF-8 byte range in the formatted result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentSpan {
    /// Zero-based argument index, including arguments used in nested strings.
    pub index: usize,
    /// The complete inserted argument's range.
    pub range: std::ops::Range<usize>,
}

/// Formatted text with the locations needed to retain argument annotations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormattedString {
    /// Text after substituting the arguments.
    pub text: String,
    /// Substitutions in display order; repeated placeholders have separate spans.
    pub arguments: Vec<ArgumentSpan>,
}

impl StringTable {
    /// Reads `eqstr_us.txt` from the installation.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read.
    pub fn read(eq_directory: &Path) -> io::Result<Self> {
        Self::read_file(&eq_directory.join("eqstr_us.txt"))
    }

    /// Reads the login screen's own table, `eqlsstr_us.txt`, whose strings
    /// a Titanium login server names by id. That it is laid out as
    /// `eqstr_us.txt` is inferred; a table in another layout reads as empty.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read.
    pub fn read_login(eq_directory: &Path) -> io::Result<Self> {
        Self::read_file(&eq_directory.join("eqlsstr_us.txt"))
    }

    fn read_file(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Parses the table, skipping its format header and count line; a file
    /// in another format reads as empty.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some("EQST0002") {
            return Self::default();
        }
        lines.next();
        Self(
            lines
                .filter_map(|line| {
                    // One space parts the number from the words, whose own
                    // spaces stand: " AFK " goes between parts of a line.
                    let (id, message) = line.split_once(' ')?;
                    if message.trim().is_empty() {
                        return None;
                    }
                    Some((id.parse().ok()?, message.to_owned()))
                })
                .collect(),
        )
    }

    /// Whether the table has no strings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A string that takes no arguments, as it stands.
    #[must_use]
    pub fn argument_free(&self, id: u32) -> Option<&str> {
        self.0
            .get(&id)
            .filter(|text| {
                !text
                    .as_bytes()
                    .windows(2)
                    .any(|pair| pair[0] == b'%' && pair[1].is_ascii_digit())
            })
            .map(String::as_str)
    }

    /// A string with the server's arguments in its `%1` to `%9`
    /// placeholders, or nothing if the table lacks it. `%T1` names another
    /// string, formatted with the same arguments; `%B1(...)` is a bold
    /// argument. Placeholders without an argument stay as written.
    #[must_use]
    pub fn format(&self, id: u32, arguments: &[String]) -> Option<String> {
        self.format_with_spans(id, arguments)
            .map(|formatted| formatted.text)
    }

    /// Formats without discarding the placement of linked or otherwise annotated
    /// arguments. Literal template text does not receive an argument span.
    #[must_use]
    pub fn format_with_spans(&self, id: u32, arguments: &[String]) -> Option<FormattedString> {
        self.format_nested(id, arguments, 0)
    }

    fn format_nested(&self, id: u32, arguments: &[String], depth: u8) -> Option<FormattedString> {
        let template = self.0.get(&id)?;
        let mut text = String::with_capacity(template.len());
        let mut spans = Vec::new();
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
                    .filter(|_| depth < 2)
                    .and_then(|nested| self.format_nested(nested, arguments, depth + 1))
            };
            let index = usize::try_from(index - 1).unwrap_or(usize::MAX);
            match arguments.get(index) {
                Some(argument) => {
                    let start = text.len();
                    if let Some(formatted) =
                        after.starts_with('T').then(|| nested(argument)).flatten()
                    {
                        text.push_str(&formatted.text);
                        spans.extend(formatted.arguments.into_iter().map(|span| ArgumentSpan {
                            index: span.index,
                            range: start + span.range.start..start + span.range.end,
                        }));
                    } else {
                        text.push_str(argument);
                        spans.push(ArgumentSpan {
                            index,
                            range: start..text.len(),
                        });
                    }
                }
                None => text.push_str(&rest[position..position + 1 + consumed]),
            }
            rest = &after[consumed..];
        }
        text.push_str(rest);
        Some(FormattedString {
            text,
            arguments: spans,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_follow_unicode_repeated_and_nested_arguments() {
        let table = StringTable::parse("EQST0002\n0 2\n1 %2 / %B1(1) / %2 / %T3\n2 %1!\n");
        let args = ["Épée", "盾", "2"].map(str::to_owned);
        let formatted = table.format_with_spans(1, &args).unwrap();
        assert_eq!(formatted.text, "盾 / Épée / 盾 / Épée!");
        assert_eq!(
            formatted
                .arguments
                .iter()
                .map(|span| span.index)
                .collect::<Vec<_>>(),
            [1, 0, 1, 0]
        );
        for span in formatted.arguments {
            assert_eq!(
                formatted.text.get(span.range),
                Some(args[span.index].as_str())
            );
        }
    }

    #[test]
    fn missing_malformed_and_parameterized_strings_are_not_argument_free() {
        let table = StringTable::parse(
            "EQST0002\r\n0 3\r\n73 Synthetic failure\r\n74 %1 failed\r\ninvalid line\r\n75 \r\n",
        );
        assert_eq!(table.argument_free(73), Some("Synthetic failure"));
        for id in [0, 74, 75, 99] {
            assert_eq!(table.argument_free(id), None);
        }
        assert!(StringTable::parse("OTHER\n0 1\n73 Wrong format").is_empty());
        // Spaces around the words are part of them.
        let table = StringTable::parse("EQST0002\n0 1\n12311  AFK \n");
        assert_eq!(table.argument_free(12311), Some(" AFK "));
    }

    #[test]
    fn server_arguments_fill_numbered_placeholders_in_order() {
        let table = StringTable::parse(
            "EQST0002
0 2
80 %1 hits %2 for %3.
81 100% sure, %9 missing
",
        );
        let arguments = ["A rat".to_owned(), "YOU".to_owned(), "2 points".to_owned()];
        assert_eq!(
            table.format(80, &arguments).as_deref(),
            Some("A rat hits YOU for 2 points.")
        );
        assert_eq!(
            table.format(81, &[]).as_deref(),
            Some("100% sure, %9 missing")
        );
        let table = StringTable::parse(
            "EQST0002
0 1
82 You gained \"%B1(1)\" at a cost of %2 ability %T3.
",
        );
        assert_eq!(
            table
                .format(82, &["Innate".into(), "0".into(), "points".into()])
                .as_deref(),
            Some("You gained \"Innate\" at a cost of 0 ability points.")
        );
        assert_eq!(table.format(99, &[]), None);
    }

    #[test]
    fn text_placeholders_naming_strings_are_formatted_with_the_same_arguments() {
        let table = StringTable::parse(
            "EQST0002
0 2
554 %1 utters '%T2'
1146 Hello, %3. Care for a %4?
",
        );
        let arguments = ["Rowyl", "1146", "Examplecleric", "Bread"].map(str::to_owned);
        assert_eq!(
            table.format(554, &arguments).as_deref(),
            Some("Rowyl utters 'Hello, Examplecleric. Care for a Bread?'")
        );
        // Unknown ids and plain text stay literal.
        assert_eq!(
            table
                .format(554, &["Nura".into(), "9999".into()])
                .as_deref(),
            Some("Nura utters '9999'")
        );
        assert_eq!(
            table
                .format(554, &["Nura".into(), "Hello".into()])
                .as_deref(),
            Some("Nura utters 'Hello'")
        );
    }
}

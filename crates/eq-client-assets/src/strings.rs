//! The installed client's string table, `eqstr_us.txt`: the words servers
//! refer to by number, with their argument placeholders.
use std::{collections::BTreeMap, io, path::Path};

/// Strings by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringTable(BTreeMap<u32, String>);

impl StringTable {
    /// Reads `eqstr_us.txt` from the installation.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be read.
    pub fn read(eq_directory: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(eq_directory.join("eqstr_us.txt"))?;
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
        self.format_nested(id, arguments, 0)
    }

    fn format_nested(&self, id: u32, arguments: &[String], depth: u8) -> Option<String> {
        let template = self.0.get(&id)?;
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
                    .filter(|_| depth < 2)
                    .and_then(|nested| self.format_nested(nested, arguments, depth + 1))
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
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
554 %1 says '%T2'
1146 Greetings, %3. You look like you could use a %4.
",
        );
        let arguments = ["Rowyl", "1146", "Clauderic", "Bread"].map(str::to_owned);
        assert_eq!(
            table.format(554, &arguments).as_deref(),
            Some("Rowyl says 'Greetings, Clauderic. You look like you could use a Bread.'")
        );
        // Unknown ids and plain text stay literal.
        assert_eq!(
            table
                .format(554, &["Nura".into(), "9999".into()])
                .as_deref(),
            Some("Nura says '9999'")
        );
        assert_eq!(
            table
                .format(554, &["Nura".into(), "Hello".into()])
                .as_deref(),
            Some("Nura says 'Hello'")
        );
    }
}

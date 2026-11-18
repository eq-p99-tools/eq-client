//! Presentation-only token classes, shared by the editor and the read-only
//! source views. Validation belongs to `script::Program`; this module only
//! colours words the parser's own vocabulary names.

use crate::script::language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Statement words such as `if`, `for` and `while`, `and`/`or`/`not`
    /// and `limit`.
    Keyword,
    /// A known action command, called.
    Command,
    /// A leading word that is no statement or command, an old-syntax word,
    /// or a dotted name that is no field.
    Unknown,
    /// `int`, `bool` and `text`.
    Type,
    Number,
    String,
    /// `True` and `False`.
    Constant,
    /// A client field such as `zone.name`.
    Field,
    /// `known(...)`, `stale(...)` and `range(...)`.
    Builtin,
    /// A name this document declares.
    Variable,
    Operator,
    /// A key chord given to `press` or `hold`.
    Key,
    Argument,
    Comment,
}

impl Kind {
    /// One palette for every view, as 8-bit sRGB.
    pub(super) const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Keyword => [255, 167, 89],
            Self::Command => [115, 209, 255],
            Self::Unknown => [255, 105, 100],
            Self::Type | Self::Builtin => [205, 160, 255],
            Self::Number | Self::Constant => [230, 196, 128],
            Self::String => [140, 222, 125],
            Self::Field => [128, 226, 200],
            Self::Variable => [235, 236, 240],
            Self::Operator => [175, 190, 205],
            Self::Key => [255, 214, 102],
            Self::Argument => [184, 191, 196],
            Self::Comment => [128, 138, 148],
        }
    }

    pub(super) fn color(self) -> bevy::prelude::Color {
        let [red, green, blue] = self.rgb();
        bevy::prelude::Color::srgb_u8(red, green, blue)
    }
}

/// Names the document declares, so later uses colour as variables. User
/// functions and their parameters belong here too when the language has them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Symbols {
    variables: Vec<String>,
}

impl Symbols {
    /// Every declared name in the source, before or after a use: each
    /// `name: type = value` and each `for name in ...`.
    pub(super) fn scan(source: &str) -> Self {
        let mut variables = Vec::new();
        for line in source.lines() {
            let code = line[..language::comment(line)]
                .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
            let name = if let Some(rest) = code.strip_prefix("for")
                && rest.starts_with(char::is_whitespace)
            {
                rest.split_once(" in ").map(|(name, _)| name.trim())
            } else if let language::Shape::Assignment(name) = language::shape(code) {
                code[name.len()..]
                    .trim_start()
                    .starts_with(':')
                    .then_some(name)
            } else {
                None
            };
            if let Some(name) = name
                && !name.is_empty()
                && name != "_"
                && !variables.iter().any(|known| known == name)
            {
                variables.push(name.to_owned());
            }
        }
        Self { variables }
    }

    fn variable(&self, word: &str) -> bool {
        self.variables.iter().any(|name| name == word)
    }
}

/// Keep every byte, including whitespace, while styling a line of the DSL.
pub(super) fn tokens<'a>(line: &'a str, symbols: &Symbols) -> Vec<(&'a str, Kind)> {
    let (code, comment) = line.split_at(language::comment(line));
    let mut result = code_tokens(code, symbols);
    if !comment.is_empty() {
        result.push((comment, Kind::Comment));
    }
    result
}

fn code_tokens<'a>(mut code: &'a str, symbols: &Symbols) -> Vec<(&'a str, Kind)> {
    let leading = language::shape(code);
    let keys = matches!(leading, language::Shape::Action("press" | "hold", _));
    let mut first = true;
    let mut result = Vec::new();
    while let Some(ch) = code.chars().next() {
        let (end, kind) = match ch {
            ch if ch.is_whitespace() || ch == '\u{feff}' => (
                code.find(|ch: char| !ch.is_whitespace() && ch != '\u{feff}')
                    .unwrap_or(code.len()),
                Kind::Argument,
            ),
            '"' | '\'' => {
                let end = string_end(code, ch);
                let inner = code[1..end].trim_end_matches(ch);
                let key = keys
                    && inner.chars().any(char::is_alphabetic)
                    && inner
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '+');
                (end, if key { Kind::Key } else { Kind::String })
            }
            '0'..='9' | '.'
                if code
                    .as_bytes()
                    .get(usize::from(ch == '.'))
                    .is_some_and(u8::is_ascii_digit) =>
            {
                (
                    code.bytes()
                        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
                        .count(),
                    Kind::Number,
                )
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let end = code
                    .bytes()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
                    .count();
                let text = &code[..end];
                let kind = if first {
                    match leading {
                        language::Shape::Action(..) => Kind::Command,
                        language::Shape::Keyword(_) => Kind::Keyword,
                        language::Shape::Assignment(_) => Kind::Variable,
                        language::Shape::CommandName(_)
                        | language::Shape::Uncalled(_)
                        | language::Shape::Unknown(_) => Kind::Unknown,
                        language::Shape::Blank => Kind::Argument,
                    }
                } else {
                    word(text, symbols)
                };
                (end, kind)
            }
            '(' | ')' | ':' | '=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%' | '&' | '|'
            | ',' => (1, Kind::Operator),
            _ => (ch.len_utf8(), Kind::Argument),
        };
        if kind != Kind::Argument || !code[..end].trim().is_empty() {
            first = false;
        }
        let (token, rest) = code.split_at(end);
        result.push((token, kind));
        code = rest;
    }
    result
}

fn word(word: &str, symbols: &Symbols) -> Kind {
    match word {
        "and" | "or" | "not" | "limit" => Kind::Keyword,
        _ if language::keyword(word) => Kind::Keyword,
        "int" | "bool" | "text" => Kind::Type,
        "True" | "False" => Kind::Constant,
        "known" | "stale" | "range" => Kind::Builtin,
        _ if language::field(word) => Kind::Field,
        _ if word.contains('.') => Kind::Unknown,
        _ if symbols.variable(word) => Kind::Variable,
        _ => Kind::Argument,
    }
}

/// The end of the text literal that opens `code` with `quote`, or the line's end.
fn string_end(code: &str, quote: char) -> usize {
    let mut escaped = false;
    for (index, ch) in code.char_indices().skip(1) {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' => escaped = true,
            ch if ch == quote => return index + 1,
            _ => {}
        }
    }
    code.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds<'a>(source: &'a str, symbols: &Symbols) -> Vec<(&'a str, Kind)> {
        let tokens = tokens(source, symbols);
        assert_eq!(
            tokens.iter().map(|(text, _)| *text).collect::<String>(),
            source
        );
        tokens
    }

    #[test]
    fn flow_highlighting_preserves_source_and_quoted_hashes() {
        let tokens = kinds(
            r##"  if zone.name=="cat\"#1" and not False: # note"##,
            &Symbols::default(),
        );
        assert!(tokens.contains(&("if", Kind::Keyword)));
        assert!(tokens.contains(&("zone.name", Kind::Field)));
        assert!(tokens.contains(&("=", Kind::Operator)));
        assert!(tokens.contains(&(r##""cat\"#1""##, Kind::String)));
        assert!(tokens.contains(&("and", Kind::Keyword)));
        assert!(tokens.contains(&("not", Kind::Keyword)));
        assert!(tokens.contains(&("False", Kind::Constant)));
        assert!(tokens.contains(&("# note", Kind::Comment)));
        let single = kinds("report('it''s # not a comment')", &Symbols::default());
        assert!(single.contains(&("'it'", Kind::String)));
    }

    #[test]
    fn highlighting_preserves_source_and_recognizes_comments_and_numbers() {
        let tokens = kinds("  hold(\"CTRL+W\", 250) # café\r", &Symbols::default());
        assert!(tokens.contains(&("hold", Kind::Command)));
        assert!(tokens.contains(&("\"CTRL+W\"", Kind::Key)));
        assert!(tokens.contains(&("250", Kind::Number)));
        assert!(tokens.contains(&("# café\r", Kind::Comment)));
        let tokens = kinds("wheel(\"world\", -1.5)", &Symbols::default());
        assert!(tokens.contains(&("\"world\"", Kind::String)));
        assert!(tokens.contains(&("1.5", Kind::Number)));
    }

    #[test]
    fn every_parser_command_and_flow_word_has_its_own_class() {
        for command in crate::script::COMMANDS {
            let call = format!("{command}()");
            assert_eq!(
                kinds(&call, &Symbols::default())[0],
                (*command, Kind::Command)
            );
        }
        for bad in ["pres(\"I\")", "press I", "wait = 1"] {
            assert_eq!(kinds(bad, &Symbols::default())[0].1, Kind::Unknown, "{bad}");
        }
        let source = "tries: int = 0\nwhile not known(target.nam) and tries < 3, limit=5:\n    tries += 1\n    for i in range(2):\n        continue\nif True:\n    pass\nelif False:\n    break\nelse:\n    pass";
        let symbols = Symbols::scan(source);
        let all = source
            .lines()
            .flat_map(|line| tokens(line, &symbols))
            .filter(|(text, _)| !text.trim().is_empty())
            .collect::<Vec<_>>();
        for expected in [
            ("tries", Kind::Variable),
            ("int", Kind::Type),
            ("0", Kind::Number),
            ("while", Kind::Keyword),
            ("not", Kind::Keyword),
            ("known", Kind::Builtin),
            ("target.nam", Kind::Unknown),
            ("limit", Kind::Keyword),
            ("for", Kind::Keyword),
            ("i", Kind::Variable),
            ("in", Kind::Keyword),
            ("range", Kind::Builtin),
            ("continue", Kind::Keyword),
            ("True", Kind::Constant),
            ("elif", Kind::Keyword),
            ("break", Kind::Keyword),
            ("else", Kind::Keyword),
            ("pass", Kind::Keyword),
        ] {
            assert!(all.contains(&expected), "{expected:?}");
        }
    }

    #[test]
    fn a_variable_is_known_before_and_after_its_declaration() {
        let source = "if ready:\n    pass\nready: bool = client.ready # other: int = 1\nfor _ in range(2):\n    pass\n";
        let symbols = Symbols::scan(source);
        assert_eq!(symbols.variables, ["ready"]);
        assert!(kinds("if ready:", &symbols).contains(&("ready", Kind::Variable)));
        assert!(kinds("if other:", &symbols).contains(&("other", Kind::Argument)));
    }
}

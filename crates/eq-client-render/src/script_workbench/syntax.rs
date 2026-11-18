//! Presentation-only token classes, shared by the editor and the read-only
//! source views. Validation belongs to `script::Program`; this module only
//! colours words the parser's own vocabulary names.

use crate::script::{COMMANDS, language};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// `let`, `set`, `if`, `else`, `end`, `repeat`, `while` and `limit`.
    Keyword,
    /// A known action command.
    Command,
    /// A leading word that is no command, or a dotted name that is no field.
    Unknown,
    /// `int`, `bool` and `text`.
    Type,
    Number,
    String,
    /// `true` and `false`.
    Constant,
    /// A client field such as `zone.name`.
    Field,
    /// `known(...)` and `stale(...)`.
    Builtin,
    /// A name a `let` in this document declares.
    Variable,
    Operator,
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
    /// Every `let` name in the source, declared before or after a use.
    pub(super) fn scan(source: &str) -> Self {
        let mut variables = Vec::new();
        for line in source.lines() {
            let code = line[..language::comment(line)]
                .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
            if let Some(rest) = code.strip_prefix("let")
                && rest.starts_with(char::is_whitespace)
                && let Some((name, _)) = rest.split_once(':')
            {
                let name = name.trim();
                if !name.is_empty() && !variables.iter().any(|known| known == name) {
                    variables.push(name.to_owned());
                }
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
    let mut result = if code
        .split_whitespace()
        .next()
        .is_some_and(language::keyword)
    {
        flow_tokens(code, symbols)
    } else {
        action_tokens(code)
    };
    if !comment.is_empty() {
        result.push((comment, Kind::Comment));
    }
    result
}

fn action_tokens(code: &str) -> Vec<(&str, Kind)> {
    let mut first = true;
    let mut result = Vec::new();
    for token in code.split_inclusive(char::is_whitespace) {
        let word = token.trim();
        let kind = if word.is_empty() {
            Kind::Argument
        } else if first {
            first = false;
            if COMMANDS.contains(&word) {
                Kind::Command
            } else {
                Kind::Unknown
            }
        } else if word.parse::<f64>().is_ok() {
            Kind::Number
        } else if word.chars().any(char::is_alphabetic)
            && word
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '+')
        {
            Kind::Key
        } else {
            Kind::Argument
        };
        result.push((token, kind));
    }
    result
}

fn flow_tokens<'a>(mut code: &'a str, symbols: &Symbols) -> Vec<(&'a str, Kind)> {
    let mut result = Vec::new();
    while let Some(ch) = code.chars().next() {
        let (end, kind) = match ch {
            '"' => (string_end(code), Kind::String),
            '0'..='9' => (
                code.bytes().take_while(u8::is_ascii_digit).count(),
                Kind::Number,
            ),
            'a'..='z' | 'A'..='Z' | '_' => {
                let end = code
                    .bytes()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
                    .count();
                (end, word(&code[..end], symbols))
            }
            '(' | ')' | ':' | '=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%' | '&' | '|' => {
                (1, Kind::Operator)
            }
            _ => (ch.len_utf8(), Kind::Argument),
        };
        let (token, rest) = code.split_at(end);
        result.push((token, kind));
        code = rest;
    }
    result
}

fn word(word: &str, symbols: &Symbols) -> Kind {
    match word {
        _ if language::keyword(word) || word == "limit" => Kind::Keyword,
        "int" | "bool" | "text" => Kind::Type,
        "true" | "false" => Kind::Constant,
        "known" | "stale" => Kind::Builtin,
        _ if language::field(word) => Kind::Field,
        _ if word.contains('.') => Kind::Unknown,
        _ if symbols.variable(word) => Kind::Variable,
        _ => Kind::Argument,
    }
}

fn string_end(code: &str) -> usize {
    let mut escaped = false;
    for (index, ch) in code.char_indices().skip(1) {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return index + 1,
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
        let tokens = kinds(r##"  if zone.name=="cat\"#1" # note"##, &Symbols::default());
        assert!(tokens.contains(&("if", Kind::Keyword)));
        assert!(tokens.contains(&("zone.name", Kind::Field)));
        assert!(tokens.contains(&("=", Kind::Operator)));
        assert!(tokens.contains(&(r##""cat\"#1""##, Kind::String)));
        assert!(tokens.contains(&("# note", Kind::Comment)));
    }

    #[test]
    fn highlighting_preserves_source_and_recognizes_comments_and_numbers() {
        let tokens = kinds("  hold CTRL+W 250 # café\r", &Symbols::default());
        assert!(tokens.contains(&("hold ", Kind::Command)));
        assert!(tokens.contains(&("CTRL+W ", Kind::Key)));
        assert!(tokens.contains(&("250 ", Kind::Number)));
        assert!(tokens.contains(&("# café\r", Kind::Comment)));
    }

    #[test]
    fn every_parser_command_and_flow_word_has_its_own_class() {
        for command in COMMANDS {
            assert_eq!(
                kinds(command, &Symbols::default()),
                [(*command, Kind::Command)]
            );
        }
        assert_eq!(
            kinds("pres I", &Symbols::default())[0],
            ("pres ", Kind::Unknown)
        );
        let source = "let tries: int = 0\n\twhile !known(target.nam) && tries < 3 limit 5\nset tries = tries + 1\nif true\nelse\nend";
        let symbols = Symbols::scan(source);
        let all = source
            .lines()
            .flat_map(|line| tokens(line, &symbols))
            .filter(|(text, _)| !text.trim().is_empty())
            .collect::<Vec<_>>();
        for expected in [
            ("let", Kind::Keyword),
            ("tries", Kind::Variable),
            ("int", Kind::Type),
            ("0", Kind::Number),
            ("while", Kind::Keyword),
            ("known", Kind::Builtin),
            ("target.nam", Kind::Unknown),
            ("limit", Kind::Keyword),
            ("set", Kind::Keyword),
            ("true", Kind::Constant),
            ("else", Kind::Keyword),
            ("end", Kind::Keyword),
        ] {
            assert!(all.contains(&expected), "{expected:?}");
        }
    }

    #[test]
    fn a_variable_is_known_before_and_after_its_declaration() {
        let source = "if ready\nend\nlet ready: bool = client.ready # let other: int = 1\n";
        let symbols = Symbols::scan(source);
        assert_eq!(symbols.variables, ["ready"]);
        assert!(kinds("if ready", &symbols).contains(&("ready", Kind::Variable)));
        assert!(kinds("if other", &symbols).contains(&("other", Kind::Argument)));
    }
}

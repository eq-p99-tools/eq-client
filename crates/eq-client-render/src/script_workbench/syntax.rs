//! Presentation-only token classes. Validation belongs to `script::Program`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Command,
    Argument,
    Number,
    Key,
    Operator,
    String,
    State,
    Comment,
}

impl Kind {
    pub(super) fn color(self) -> bevy::prelude::Color {
        use crate::theme;
        match self {
            Self::Command | Self::Operator => theme::FOCUS,
            Self::Comment => theme::INK_DIM,
            Self::Number | Self::State => theme::INK_WARM,
            Self::Key | Self::String => theme::AGREED,
            Self::Argument => theme::INK_BRIGHT,
        }
    }
}

/// Keep every byte, including whitespace, while styling a line of the DSL.
pub(super) fn tokens(line: &str) -> Vec<(&str, Kind)> {
    use crate::script::language;
    let (code, comment) = line.split_at(language::comment(line));
    let mut result = if code
        .split_whitespace()
        .next()
        .is_some_and(language::keyword)
    {
        flow_tokens(code)
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
            Kind::Command
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

fn flow_tokens(mut code: &str) -> Vec<(&str, Kind)> {
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
                let word = &code[..end];
                let kind = if crate::script::language::keyword(word) || word == "limit" {
                    Kind::Command
                } else if word.contains('.')
                    || matches!(
                        word,
                        "int" | "bool" | "text" | "true" | "false" | "known" | "stale"
                    )
                {
                    Kind::State
                } else {
                    Kind::Argument
                };
                (end, kind)
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
    #[test]
    fn flow_highlighting_preserves_source_and_quoted_hashes() {
        let source = r##"  if zone.name=="cat\"#1" # note"##;
        let tokens = tokens(source);
        assert_eq!(
            tokens.iter().map(|(text, _)| *text).collect::<String>(),
            source
        );
        assert!(tokens.contains(&("zone.name", Kind::State)));
        assert!(tokens.contains(&("=", Kind::Operator)));
        assert!(tokens.contains(&(r##""cat\"#1""##, Kind::String)));
        assert!(tokens.contains(&("# note", Kind::Comment)));
    }

    #[test]
    fn highlighting_preserves_source_and_recognizes_comments_and_numbers() {
        let source = "  hold CTRL+W 250 # café\r";
        let tokens = tokens(source);
        assert_eq!(
            tokens.iter().map(|(text, _)| *text).collect::<String>(),
            source
        );
        assert!(tokens.contains(&("hold ", Kind::Command)));
        assert!(tokens.contains(&("CTRL+W ", Kind::Key)));
        assert!(tokens.contains(&("250 ", Kind::Number)));
        assert!(tokens.contains(&("# café\r", Kind::Comment)));
    }
}

//! Presentation-only token classes. Validation belongs to `script::Program`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Command,
    Argument,
    Number,
    Key,
    Comment,
}

/// Keep every byte, including whitespace, while styling a line of the DSL.
pub(super) fn tokens(line: &str) -> Vec<(&str, Kind)> {
    let (code, comment) = line
        .find('#')
        .map_or((line, ""), |index| line.split_at(index));
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
    if !comment.is_empty() {
        result.push((comment, Kind::Comment));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
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

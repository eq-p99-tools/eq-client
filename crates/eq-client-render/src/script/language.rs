//! Compile line-oriented control flow and execute it cooperatively.
mod compile;
mod expression;
mod observe;
mod run;

pub(super) use compile::{Builder, Code};
pub(super) use observe::Sight;
pub(super) use run::{Run, Tick};

use std::{fmt, ops::Range, sync::Arc};

/// A compile error and, when known, the bytes of the source it was found in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::script) struct Fault {
    pub(in crate::script) message: String,
    pub(in crate::script) span: Option<Range<usize>>,
}

impl Fault {
    fn at(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
        }
    }

    /// Move the span from a slice of a source into that whole source.
    fn within(mut self, outer: &str, inner: &str) -> Self {
        let offset = offset(outer, inner);
        self.span = self.span.map(|span| span.start + offset..span.end + offset);
        self
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Self {
        Self {
            message,
            span: None,
        }
    }
}

impl From<&str> for Fault {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

/// Where `inner`, a slice of `outer`, starts in it.
pub(in crate::script) fn offset(outer: &str, inner: &str) -> usize {
    let offset = inner.as_ptr().addr().wrapping_sub(outer.as_ptr().addr());
    debug_assert!(
        offset
            .checked_add(inner.len())
            .is_some_and(|end| end <= outer.len()),
        "a slice of the source"
    );
    offset.min(outer.len())
}

/// Statement keywords. `def` and `return` are reserved for functions.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "elif"
            | "else"
            | "for"
            | "in"
            | "while"
            | "break"
            | "continue"
            | "pass"
            | "def"
            | "return"
    )
}

/// A client field an expression can read, such as `zone.name`.
pub(crate) fn field(name: &str) -> bool {
    observe::Field::parse(name).is_some()
}

/// Words a variable cannot be named, besides keywords and command names.
pub(crate) const RESERVED: &[&str] = &[
    "limit", "int", "bool", "text", "True", "False", "known", "stale", "range", "and", "or", "not",
];

/// What a statement line is, from its first word. One classifier serves the
/// compiler, the highlighter and the line-at-a-time control protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape<'a> {
    Blank,
    /// A command name and the call after it, from its opening parenthesis.
    Action(&'a str, &'a str),
    Keyword(&'a str),
    /// A name followed by `:`, `=` or `+=`, `-=`, `*=`.
    Assignment(&'a str),
    /// A command used as a variable name.
    CommandName(&'a str),
    /// A command name without its call's parentheses.
    Uncalled(&'a str),
    /// A name with parentheses that is no command, or anything else.
    Unknown(&'a str),
}

/// Classify a statement's code, without its indent or comment.
pub(crate) fn shape(code: &str) -> Shape<'_> {
    let code = code.trim();
    if code.is_empty() {
        return Shape::Blank;
    }
    let length = code
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    if length == 0 {
        let first = code.chars().next().map_or(0, char::len_utf8);
        return Shape::Unknown(&code[..first]);
    }
    let word = &code[..length];
    let rest = code[length..].trim_start();
    let assigns = rest.starts_with(':')
        || (rest.starts_with('=') && !rest.starts_with("=="))
        || ["+=", "-=", "*="].iter().any(|op| rest.starts_with(op));
    if keyword(word) {
        Shape::Keyword(word)
    } else if super::COMMANDS.contains(&word) {
        if rest.starts_with('(') {
            Shape::Action(word, rest)
        } else if assigns {
            Shape::CommandName(word)
        } else {
            Shape::Uncalled(word)
        }
    } else if assigns {
        Shape::Assignment(word)
    } else {
        Shape::Unknown(word)
    }
}

/// Recognize a structured line even if a malformed BOM would prevent parsing it:
/// a keyword, an assignment or an indented line. An action call, a comment
/// and an unknown line are not.
fn starts_statement(line: &str) -> bool {
    let line = line.trim_start_matches('\u{feff}');
    let code = &line[..comment(line)];
    !code.trim().is_empty()
        && (code.starts_with(char::is_whitespace)
            || matches!(shape(code), Shape::Keyword(_) | Shape::Assignment(_)))
}

pub(super) fn first_structured_line(source: &str) -> Option<usize> {
    source
        .lines()
        .position(starts_statement)
        .map(|index| index + 1)
}

/// The comment boundary: the first `#` outside quoted text.
pub(crate) fn comment(line: &str) -> usize {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match (ch, quote) {
            ('\\', Some(_)) => escaped = true,
            ('"' | '\'', None) => quote = Some(ch),
            (ch, Some(open)) if ch == open => quote = None,
            ('#', None) => return index,
            _ => {}
        }
    }
    line.len()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Int,
    Bool,
    Text,
}

impl Kind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "int" => Some(Self::Int),
            "bool" => Some(Self::Bool),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Text => "text",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Int(i64),
    Bool(bool),
    Text(Arc<str>),
    Unknown,
    Stale,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(f, "{value}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Text(value) => write!(f, "{value:?}"),
            Self::Unknown => f.write_str("unknown"),
            Self::Stale => f.write_str("stale"),
        }
    }
}

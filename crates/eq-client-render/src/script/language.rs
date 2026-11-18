//! Compile line-oriented control flow and execute it cooperatively.
mod compile;
mod expression;
mod observe;
mod run;

pub(super) use compile::{Builder, Code};
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

/// Only these leading words opt in to expression syntax and quoted comments.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "let" | "set" | "if" | "else" | "repeat" | "while" | "end"
    )
}

/// A client field an expression can read, such as `zone.name`.
pub(crate) fn field(name: &str) -> bool {
    observe::Field::parse(name).is_some()
}

/// Words a variable cannot be named, besides the statement keywords.
pub(crate) const RESERVED: &[&str] = &[
    "limit", "int", "bool", "text", "true", "false", "known", "stale",
];

/// Recognize a structured line even if a malformed BOM would prevent parsing it.
fn starts_statement(line: &str) -> bool {
    line.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
        .split(|ch: char| ch.is_whitespace() || ch == '#')
        .next()
        .is_some_and(keyword)
}

pub(super) fn first_structured_line(source: &str) -> Option<usize> {
    source
        .lines()
        .position(starts_statement)
        .map(|index| index + 1)
}

/// The comment boundary, keeping legacy action arguments byte-for-byte compatible.
pub(crate) fn comment(line: &str) -> usize {
    if !starts_statement(line) {
        return line.find('#').unwrap_or(line.len());
    }
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '#' if !quoted => return index,
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

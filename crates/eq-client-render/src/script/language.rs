//! Compile line-oriented control flow and execute it cooperatively.
mod compile;
mod expression;
mod observe;
mod run;

pub(super) use compile::{Builder, Code};
pub(super) use run::{Run, Tick};

use std::{fmt, sync::Arc};

/// Only these leading words opt in to expression syntax and quoted comments.
pub(crate) fn keyword(word: &str) -> bool {
    matches!(
        word,
        "let" | "set" | "if" | "else" | "repeat" | "while" | "end"
    )
}

/// The comment boundary, keeping legacy action arguments byte-for-byte compatible.
pub(crate) fn comment(line: &str) -> usize {
    let first = line.split_whitespace().next().unwrap_or_default();
    if !keyword(first) {
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

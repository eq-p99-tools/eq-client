//! Validated script source and the source location of each executable step.

use std::{fmt, path::Path, sync::Arc};

use super::{
    Step,
    parse::{MAX_STEPS, parse_step},
};

/// Maximum UTF-8 source size accepted by the script loader and editor.
pub const MAX_SOURCE_BYTES: usize = 128 * 1024;

/// An executable step and its one-based line in the original source.
#[derive(Clone, Debug, PartialEq)]
pub struct Instruction {
    pub(super) line: usize,
    pub(super) step: Step,
}

impl Instruction {
    /// The one-based source line, including preceding comments and blank lines.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The validated action on this source line.
    #[must_use]
    pub const fn step(&self) -> &Step {
        &self.step
    }
}

/// An immutable execution snapshot, independent of subsequent editor changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    source: Arc<str>,
    instructions: Vec<Instruction>,
}

impl Program {
    /// Validates source using the same language as command-line scripts.
    /// Screenshot paths are relative to `base`, normally the source file's folder.
    ///
    /// # Errors
    /// Returns the first malformed line or a source/step limit violation.
    pub fn parse(text: &str, base: &Path) -> Result<Self, ParseError> {
        Self::compile(text, base).map_err(|errors| errors[0].clone())
    }

    /// Check every source line with the executor's parser, without running it.
    #[must_use]
    pub fn diagnostics(text: &str, base: &Path) -> Vec<ParseError> {
        Self::compile(text, base).err().unwrap_or_default()
    }

    fn compile(text: &str, base: &Path) -> Result<Self, Vec<ParseError>> {
        if text.len() > MAX_SOURCE_BYTES {
            return Err(vec![ParseError::document(format!(
                "script exceeds the {MAX_SOURCE_BYTES}-byte source limit"
            ))]);
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut instructions = Vec::new();
        let mut errors = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let step = match parse_step(line) {
                Ok(step) => step,
                Err(message) => {
                    errors.push(ParseError {
                        line: Some(index + 1),
                        message,
                    });
                    continue;
                }
            };
            let step = match step {
                Step::Screenshot(path) => Step::Screenshot(base.join(path)),
                step => step,
            };
            instructions.push(Instruction {
                line: index + 1,
                step,
            });
            if instructions.len() > MAX_STEPS {
                errors.push(ParseError::document(format!(
                    "script has more than {MAX_STEPS} steps"
                )));
                break;
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            source: Arc::from(text),
            instructions,
        })
    }

    /// The source that was validated, without an optional UTF-8 byte-order mark.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The steps in execution order, with their original line numbers.
    #[must_use]
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Converts a program to the legacy action list used by existing callers.
    #[must_use]
    pub fn into_steps(self) -> Vec<Step> {
        self.instructions
            .into_iter()
            .map(|instruction| instruction.step)
            .collect()
    }

    pub(super) fn into_parts(self) -> (Arc<str>, Vec<Instruction>) {
        (self.source, self.instructions)
    }
}

/// A script validation error with a source line where one can be identified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// One-based source line; `None` for a limit applying to the entire document.
    pub line: Option<usize>,
    /// The reason validation failed, without a redundant source-location prefix.
    pub message: String,
}

impl ParseError {
    fn document(message: String) -> Self {
        Self {
            line: None,
            message,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(formatter, "script line {line}: ")?;
        }
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_locations_keep_comments_and_blank_lines() {
        let source =
            "\u{feff}# a scenario\r\n\r\nreport first\r\n# another comment\r\nreport second\r\n";
        let program = Program::parse(source, Path::new("scripts")).unwrap();
        assert_eq!(
            program
                .instructions()
                .iter()
                .map(Instruction::line)
                .collect::<Vec<_>>(),
            [3, 5]
        );
        assert_eq!(program.source(), source.trim_start_matches('\u{feff}'));
        assert_eq!(
            program.clone().into_steps(),
            super::super::parse(source, Path::new("scripts")).unwrap()
        );
    }

    #[test]
    fn errors_point_to_the_source_instead_of_the_step_index() {
        let error = Program::parse("# scenario\n\nnot_a_command\n", Path::new(".")).unwrap_err();
        assert_eq!(error.line, Some(3));
        assert!(error.to_string().starts_with("script line 3: "));
    }

    #[test]
    fn diagnostics_find_every_bad_line_and_keep_execution_atomic() {
        let source = "# note\nwait invalid\nreport valid\npress UNKNOWN\n\nno_command\n";
        let errors = Program::diagnostics(source, Path::new("."));
        assert_eq!(
            errors.iter().map(|error| error.line).collect::<Vec<_>>(),
            [Some(2), Some(4), Some(6)]
        );
        assert_eq!(
            Program::parse(source, Path::new(".")).unwrap_err(),
            errors[0]
        );
        assert_eq!(Program::diagnostics("wait 1\n", Path::new(".")), []);
    }

    #[test]
    fn large_documents_are_rejected_even_if_they_only_contain_comments() {
        let error = Program::parse(
            &format!("#{}", "x".repeat(MAX_SOURCE_BYTES)),
            Path::new("."),
        )
        .unwrap_err();
        assert_eq!(error.line, None);
        assert!(error.message.contains("source limit"));
    }
}

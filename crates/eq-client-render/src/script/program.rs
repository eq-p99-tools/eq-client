//! Validated script source and the source location of each executable step.

use std::{fmt, ops::Range, path::Path, sync::Arc};

use super::{
    Step,
    language::{self, Builder, Code},
    parse::{COMMANDS, MAX_STEPS, parse_step},
    source::without_bom,
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
    code: Option<Code>,
    statements: usize,
}

#[derive(Debug)]
pub(super) enum AppendError {
    Structured(Option<usize>),
    Invalid(ParseError),
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

    /// The program, or every error the parser finds in the source.
    ///
    /// # Errors
    /// Every malformed line in source order, or one source/step limit violation.
    pub fn check(text: &str, base: &Path) -> Result<Self, Vec<ParseError>> {
        Self::compile(text, base)
    }

    /// Check every source line with the executor's parser, without running it.
    #[must_use]
    pub fn diagnostics(text: &str, base: &Path) -> Vec<ParseError> {
        Self::compile(text, base).err().unwrap_or_default()
    }

    pub(super) fn parse_append(text: &str, base: &Path) -> Result<Vec<Instruction>, AppendError> {
        // An incomplete or malformed header can be followed by action lines
        // in another poll. Identify it before ordinary parse errors can skip it.
        if let Some(line) = language::first_structured_line(text) {
            return Err(AppendError::Structured(Some(line)));
        }
        Self::parse(text, base)
            .map_err(AppendError::Invalid)?
            .into_linear_instructions()
            .map_err(|error| AppendError::Structured(error.line))
    }

    fn compile(text: &str, base: &Path) -> Result<Self, Vec<ParseError>> {
        #[cfg(test)]
        COMPILES.set(COMPILES.get() + 1);
        if text.len() > MAX_SOURCE_BYTES {
            return Err(vec![ParseError::document(format!(
                "script exceeds the {MAX_SOURCE_BYTES}-byte source limit"
            ))]);
        }
        let source = text;
        let text = without_bom(text);
        // Spans count bytes of the line as given, before a leading BOM is dropped.
        let bom = source.len() - text.len();
        let mut instructions = Vec::new();
        let mut errors = Vec::new();
        let mut flow = Builder::default();
        let mut statements = 0;
        for (index, raw) in text.lines().enumerate() {
            let line = raw[..language::comment(raw)].trim();
            if line.is_empty() {
                continue;
            }
            let start = language::offset(raw, line) + if index == 0 { bom } else { 0 };
            let at = |span: Range<usize>| Some(start + span.start..start + span.end);
            statements += 1;
            if statements > MAX_STEPS {
                return Err(vec![ParseError::document(format!(
                    "script has more than {MAX_STEPS} statements"
                ))]);
            }
            match flow.statement(line, index + 1) {
                Ok(false) => (),
                Ok(true) => continue,
                Err(fault) => {
                    errors.push(ParseError {
                        line: Some(index + 1),
                        span: at(fault.span.unwrap_or(0..line.len())),
                        message: fault.message,
                    });
                    continue;
                }
            }
            let step = match parse_step(line) {
                Ok(step) => step,
                Err(message) => {
                    // An unknown command marks its word; a malformed one, its arguments.
                    let word = line.find(char::is_whitespace).unwrap_or(line.len());
                    let arguments = line[word..].trim_start();
                    let span = if COMMANDS.contains(&&line[..word]) && !arguments.is_empty() {
                        line.len() - arguments.len()..line.len()
                    } else {
                        0..word
                    };
                    errors.push(ParseError {
                        line: Some(index + 1),
                        span: at(span),
                        message,
                    });
                    continue;
                }
            };
            let step = match step {
                Step::Screenshot(path) => Step::Screenshot(base.join(path)),
                step => step,
            };
            flow.action(instructions.len(), index + 1);
            instructions.push(Instruction {
                line: index + 1,
                step,
            });
        }
        errors.extend(flow.unclosed().map(|line| ParseError {
            line: Some(line),
            span: text.lines().nth(line - 1).map(|raw| {
                let code = raw.trim_start();
                let start = raw.len() - code.len() + if line == 1 { bom } else { 0 };
                start..start + code.find(char::is_whitespace).unwrap_or(code.len())
            }),
            message: "block needs a matching end".into(),
        }));
        if !errors.is_empty() {
            errors.sort_by_key(|error| error.line);
            return Err(errors);
        }
        Ok(Self {
            source: Arc::from(text),
            instructions,
            code: flow.finish(),
            statements,
        })
    }

    /// The source that was validated, without an optional UTF-8 byte-order mark.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Authored actions in source order, including actions inside blocks.
    #[must_use]
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Nonempty source statements, including block delimiters.
    #[must_use]
    pub const fn statement_count(&self) -> usize {
        self.statements
    }

    /// Validate authored actions against the session's local-only command gate.
    ///
    /// # Errors
    /// A restricted action anywhere in the source needs a local test session.
    pub fn check_local_commands(&self, local: bool) -> Result<(), ParseError> {
        if !local
            && let Some(instruction) =
                self.instructions
                    .iter()
                    .find(|instruction| match instruction.step() {
                        Step::Gm(_) | Step::Chat(_) | Step::Social(_) => true,
                        Step::Slash(command) => crate::chat::broadcasts_to_others(command),
                        _ => false,
                    })
        {
            return Err(ParseError {
                line: Some(instruction.line()),
                span: None,
                message: if matches!(instruction.step(), Step::Social(_)) {
                    "social steps require a network-enforced local test session.".into()
                } else if matches!(instruction.step(), Step::Slash(_)) {
                    "emote and random steps require a network-enforced local test session.".into()
                } else {
                    "gm and chat require a network-enforced local test session.".into()
                },
            });
        }
        Ok(())
    }

    /// Converts a linear program to the legacy action list.
    ///
    /// # Errors
    /// Structured programs need the executor to evaluate their control flow.
    pub fn into_steps(self) -> Result<Vec<Step>, ParseError> {
        Ok(self
            .into_linear_instructions()?
            .into_iter()
            .map(|instruction| instruction.step)
            .collect())
    }

    fn into_linear_instructions(self) -> Result<Vec<Instruction>, ParseError> {
        if self.code.is_some() {
            return Err(ParseError {
                line: language::first_structured_line(&self.source),
                span: None,
                message: "use Program to run scripts with variables or control flow".into(),
            });
        }
        Ok(self.instructions)
    }

    pub(super) fn into_parts(self) -> (Arc<str>, Vec<Instruction>, Option<Code>) {
        (self.source, self.instructions, self.code)
    }
}

#[cfg(test)]
thread_local! {
    /// Compilations on this thread, so tests can show work is not repeated.
    pub(crate) static COMPILES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A script validation error with a source line where one can be identified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// One-based source line; `None` for a limit applying to the entire document.
    pub line: Option<usize>,
    /// Bytes of that line the error is about, where they can be identified.
    /// Presentation only: the line and message carry the verdict.
    pub span: Option<Range<usize>>,
    /// The reason validation failed, without a redundant source-location prefix.
    pub message: String,
}

impl ParseError {
    fn document(message: String) -> Self {
        Self {
            line: None,
            span: None,
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
            program.clone().into_steps().unwrap(),
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

    /// The text each error's span covers, by line.
    fn spans(source: &str) -> Vec<(usize, &str)> {
        let lines = source.lines().collect::<Vec<_>>();
        Program::diagnostics(source, Path::new("."))
            .into_iter()
            .map(|error| {
                let line = error.line.unwrap();
                (line, &lines[line - 1][error.span.unwrap()])
            })
            .collect()
    }

    #[test]
    fn errors_point_at_the_words_they_are_about() {
        let source = concat!(
            "  pres I # typo\n",
            "\twait soon\n",
            "let n: int = 1 + \"é\"\n",
            "let m: int = missing * 2\n",
            "let t: text = \"open\n",
            "set nope = 1\n",
            "let 9x: int = 0\n",
            "let k: number = 0\n",
            "let flag: bool = 5\n",
            "if (true\n",
            "end\n",
            "else\n",
            "end now\n",
            "while true limit\n",
            "end\n",
            "if !3 # é\n",
            "end\n",
            "if n == 1 n\n",
            "end\n",
            "  repeat 2\n",
        );
        assert_eq!(
            spans(source),
            [
                (1, "pres"),
                (2, "soon"),
                (3, "+"),
                (4, "missing"),
                (5, "\"open"),
                (6, "nope"),
                (7, "9x"),
                (8, "number"),
                (9, "5"),
                (10, "("),
                // A header that fails opens no block, so its end has none.
                (11, "end"),
                (12, "else"),
                (13, "now"),
                (14, "limit"),
                (15, "end"),
                (16, "!"),
                (17, "end"),
                (18, "n"),
                (19, "end"),
                (20, "repeat"),
            ]
        );
    }

    #[test]
    fn spans_count_bytes_of_the_line_as_given_with_a_bom() {
        let errors = Program::diagnostics("\u{feff}  pres I\r\nwait x\r\n", Path::new("."));
        assert_eq!(errors[0].span, Some(5..9));
        assert_eq!(errors[1].span, Some(5..6));
        assert_eq!(
            errors[0].to_string(),
            "script line 1: unknown command 'pres'"
        );
    }

    #[test]
    fn declarations_conditions_and_blocks_have_source_diagnostics() {
        let errors = Program::diagnostics(
            "let n: int = 0\nset n = true\nif n == 0\n  let scoped: bool = false\n  report allowed\n",
            Path::new("."),
        );
        assert_eq!(
            errors.iter().map(|error| error.line).collect::<Vec<_>>(),
            [Some(2), Some(3), Some(4)]
        );
        assert!(errors[0].message.contains("expected int"));
        assert!(errors[1].message.contains("matching end"));
        assert!(errors[2].message.contains("top level"));
        assert_eq!(
            Program::parse("while true limit 0\nend", Path::new("."))
                .unwrap_err()
                .line,
            Some(1)
        );
    }

    #[test]
    fn restricted_actions_in_skipped_branches_keep_their_gate() {
        let source = "if false\n  gm heal\nelse\n  report safe\nend\n";
        let program = Program::parse(source, Path::new(".")).unwrap();
        assert_eq!(
            program.check_local_commands(false).unwrap_err().line,
            Some(2)
        );
        assert!(program.check_local_commands(true).is_ok());
        assert_eq!(program.into_steps().unwrap_err().line, Some(1));
    }

    #[test]
    fn a_gm_show_query_needs_a_local_session() {
        let program = Program::parse("report before\ngm show buffs\n", Path::new(".")).unwrap();
        assert_eq!(
            program.check_local_commands(false).unwrap_err().line,
            Some(2)
        );
        assert!(program.check_local_commands(true).is_ok());
    }

    #[test]
    fn emote_and_random_steps_need_a_local_session_but_listing_steps_do_not() {
        for (source, line) in [
            ("slash afk\nslash emote waves\n", Some(2)),
            ("slash assist\nslash random 1 6\n", Some(2)),
        ] {
            let program = Program::parse(source, Path::new(".")).unwrap();
            assert_eq!(program.check_local_commands(false).unwrap_err().line, line);
            assert!(program.check_local_commands(true).is_ok());
        }
        let program = Program::parse("slash afk\nslash assist\n", Path::new(".")).unwrap();
        assert!(program.check_local_commands(false).is_ok());
        for command in ["/emote waves", "/EM waves", " /random 1 6", "/Random"] {
            assert!(crate::chat::broadcasts_to_others(command), "{command}");
        }
        for command in ["/emoted", "/afk", "/assist", "emote waves", "/randomize"] {
            assert!(!crate::chat::broadcasts_to_others(command), "{command}");
        }
    }

    #[test]
    fn quoted_hashes_only_change_comment_handling_in_expressions() {
        let source = r#"let name: text = "example # one" # comment
report example # trailing"#;
        let program = Program::parse(source, Path::new(".")).unwrap();
        assert_eq!(program.statement_count(), 2);
        assert_eq!(
            program.instructions()[0].step(),
            &Step::Report("example".into())
        );
    }

    #[test]
    fn follow_append_keeps_linear_bom_support_and_rejects_structured_batches() {
        let base = Path::new(".");
        let instructions = Program::parse_append("\u{feff}report if # comment\n", base).unwrap();
        assert_eq!(instructions[0].step(), &Step::Report("if".into()));
        assert!(matches!(
            Program::parse_append("\u{feff}\u{feff}let n: int = 0\nquit\n", base),
            Err(AppendError::Structured(Some(1)))
        ));
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

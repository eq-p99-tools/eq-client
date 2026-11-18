//! Lower indented blocks to a bounded sequence of typed operations.

use std::ops::Range;

use super::{
    Fault, Kind, Shape,
    expression::{self, Expr},
    keyword, offset, shape,
};

const MAX_VARIABLES: usize = 32;
const MAX_NAME_BYTES: usize = 32;
const MAX_NESTING: usize = 16;
pub(super) const MAX_ITERATIONS: i64 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::script) struct Code {
    pub(super) ops: Vec<Located>,
    pub(super) variables: Vec<Variable>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Variable {
    pub(super) name: String,
    pub(super) kind: Kind,
    /// A `for` variable: assigned only by its loop.
    pub(super) loop_counter: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Located {
    pub(super) line: usize,
    pub(super) op: Op,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Action(usize),
    Store(usize, Expr),
    If {
        condition: Expr,
        otherwise: usize,
    },
    Repeat {
        count: Expr,
        exit: usize,
        /// The `for` variable that holds the zero-based iteration.
        variable: Option<usize>,
    },
    While {
        condition: Expr,
        limit: u16,
        exit: usize,
    },
    /// Leave the loop whose header is here.
    Break(usize),
    /// Go back to the loop header here.
    Continue(usize),
    Jump(usize),
}

enum Block {
    /// An `if` and its `elif`s: the header being filled, and the jumps that
    /// leave each finished branch.
    If {
        start: usize,
        jumps: Vec<usize>,
    },
    Else {
        jump: usize,
        jumps: Vec<usize>,
    },
    Loop {
        start: usize,
        variable: Option<usize>,
    },
}

/// An open block's place in the source, for indentation.
struct Open {
    block: Block,
    line: usize,
    /// The header's indent width.
    header: usize,
    /// The body's indent width, once its first line is seen.
    body: Option<usize>,
    /// The header keyword's bytes in its line.
    keyword: Range<usize>,
}

/// An action call's command and argument words, ready for the word grammar.
pub(in crate::script) struct Call<'a> {
    pub(in crate::script) command: &'a str,
    pub(in crate::script) words: Vec<String>,
    /// The bytes inside the parentheses.
    pub(in crate::script) arguments: Range<usize>,
}

/// An error on another line than the one being read.
pub(in crate::script) struct Deferred {
    pub(in crate::script) line: usize,
    pub(in crate::script) fault: Fault,
}

pub(in crate::script) struct Builder {
    ops: Vec<Located>,
    variables: Vec<Variable>,
    blocks: Vec<Open>,
    used: bool,
    indent: Option<char>,
    stray: Option<(usize, Range<usize>)>,
    /// The indent of a block header that failed, so its body is read for
    /// its own errors without one for each indent.
    failed: Option<usize>,
    deferred: Vec<Deferred>,
}

impl Builder {
    pub(in crate::script) fn new() -> Self {
        Self {
            ops: Vec::new(),
            variables: Vec::new(),
            blocks: Vec::new(),
            used: false,
            indent: None,
            stray: None,
            failed: None,
            deferred: Vec::new(),
        }
    }

    /// One nonblank line; a fault's span is in
    /// `raw`. An action comes back for the word grammar to check.
    pub(in crate::script) fn statement<'a>(
        &mut self,
        raw: &'a str,
        line: usize,
    ) -> Result<Option<Call<'a>>, Fault> {
        let code = raw[..super::comment(raw)].trim_end();
        let indent = &code[..code.len() - code.trim_start().len()];
        let code = &code[indent.len()..];
        self.indentation(indent, code, line)?;
        let start = indent.len();
        let at = |fault: Fault| fault.within(raw, code);
        match shape(code) {
            Shape::Blank => Ok(None),
            Shape::Action(command, call) => {
                let (words, arguments) =
                    arguments(command, call).map_err(|fault| fault.within(raw, call))?;
                let arguments =
                    offset(raw, call) + arguments.start..offset(raw, call) + arguments.end;
                Ok(Some(Call {
                    command,
                    words,
                    arguments,
                }))
            }
            Shape::Keyword(word) => {
                self.used = true;
                self.keyword(word, code, line, start).map_err(|fault| {
                    if matches!(word, "if" | "elif" | "else" | "for" | "while") {
                        self.failed = Some(start);
                    }
                    at(fault)
                })?;
                Ok(None)
            }
            Shape::Assignment(name) => {
                self.used = true;
                self.assignment(name, code, line).map_err(at)?;
                Ok(None)
            }
            Shape::CommandName(name) => Err(at(Fault::at(
                format!("'{name}' is a command and can't be a variable"),
                0..name.len(),
            ))),
            Shape::Uncalled(word) => Err(at(Fault::at(
                format!("call {word} with parentheses, such as {word}(...)"),
                0..word.len(),
            ))),
            Shape::Unknown(word) => Err(at(Fault::at(
                format!("unknown command '{word}'"),
                0..word.len(),
            ))),
        }
    }

    /// Close the blocks this line's indent ends and check the indent itself.
    fn indentation(&mut self, indent: &str, code: &str, line: usize) -> Result<(), Fault> {
        let width = indent.len();
        match self.failed {
            Some(header) if width > header => return Ok(()),
            _ => self.failed = None,
        }
        if let Some(first) = indent.chars().next() {
            let mixed = indent.chars().any(|ch| ch != first)
                || self.indent.is_some_and(|used| used != first);
            if mixed {
                return Err(Fault::at("indentation mixes tabs and spaces", 0..width));
            }
            self.indent = Some(first);
        }
        let alternative = matches!(shape(code), Shape::Keyword("else" | "elif"));
        if let Some(top) = self.blocks.last_mut()
            && top.body.is_none()
        {
            if width > top.header {
                top.body = Some(width);
            } else if alternative && width == top.header && matches!(top.block, Block::If { .. }) {
                // Report the empty branch but keep the if open for its else.
                let fault = empty(top.keyword.clone());
                top.body = Some(width + 1);
                let line = top.line;
                self.deferred.push(Deferred { line, fault });
            } else {
                let (header, keyword) = (top.line, top.keyword.clone());
                self.empty_block(header, keyword);
            }
        }
        let mut closed = false;
        while let Some(top) = self.blocks.last() {
            let body = top
                .body
                .expect("only the innermost block can await its body");
            let pairs = alternative && top.header == width && matches!(top.block, Block::If { .. });
            if body <= width || pairs {
                break;
            }
            let header = top.line;
            self.close(header).map_err(Fault::from)?;
            closed = true;
        }
        if alternative {
            return match self.blocks.last() {
                Some(top) if top.header != width => Err(Fault::at(
                    "unindent does not match any outer indentation level",
                    0..width,
                )),
                _ => Ok(()),
            };
        }
        let expected = self.blocks.last().and_then(|top| top.body).unwrap_or(0);
        if width > expected {
            if closed {
                return Err(Fault::at(
                    "unindent does not match any outer indentation level",
                    0..width,
                ));
            }
            if self.blocks.is_empty() {
                // Accepted only while the script has no statements at all.
                self.stray.get_or_insert((line, 0..width));
            } else {
                return Err(Fault::at("unexpected indent", 0..width));
            }
        }
        Ok(())
    }

    fn empty_block(&mut self, line: usize, keyword: Range<usize>) {
        self.deferred.push(Deferred {
            line,
            fault: empty(keyword),
        });
        self.close(line).expect("the block is open");
    }

    /// A keyword line; spans are in `code`, which starts at `indent` in its line.
    #[allow(
        clippy::too_many_lines,
        reason = "one match over every statement keyword keeps each header's checks together"
    )]
    fn keyword(&mut self, word: &str, code: &str, line: usize, indent: usize) -> Result<(), Fault> {
        let rest = code[word.len()..].trim();
        let keyword = 0..word.len();
        let header = |rest: &str| -> Result<(), Fault> {
            if rest.ends_with(':') {
                Ok(())
            } else {
                Err(Fault::at(
                    "expected ':' at the end of the line",
                    keyword.clone(),
                ))
            }
        };
        let open = |builder: &mut Self, block: Block| {
            builder.blocks.push(Open {
                block,
                line,
                header: indent,
                body: None,
                keyword: indent..indent + word.len(),
            });
        };
        match word {
            "if" | "elif" => {
                header(rest)?;
                let condition = inside_header(rest);
                let expr = self
                    .expression(condition, Kind::Bool)
                    .map_err(|fault| fault.within(code, condition))?;
                if word == "if" {
                    self.nesting()
                        .map_err(|message| Fault::at(message, keyword.clone()))?;
                    let start = self.ops.len();
                    self.push(
                        line,
                        Op::If {
                            condition: expr,
                            otherwise: 0,
                        },
                    );
                    open(
                        self,
                        Block::If {
                            start,
                            jumps: Vec::new(),
                        },
                    );
                } else {
                    self.elif(line, expr)
                        .map_err(|message| Fault::at(message, keyword.clone()))?;
                }
            }
            "else" => {
                if rest != ":" {
                    header(rest)?;
                    return Err(Fault::at("else takes no condition; use elif", keyword));
                }
                self.otherwise(line)
                    .map_err(|message| Fault::at(message, keyword.clone()))?;
            }
            "for" => {
                header(rest)?;
                let body = inside_header(rest);
                let (name, count) = body
                    .split_once(" in ")
                    .map(|(name, count)| (name.trim(), count.trim()))
                    .ok_or_else(|| {
                        Fault::at("expected 'for name in range(count):'", keyword.clone())
                    })?;
                let count_source = count
                    .strip_prefix("range")
                    .map(str::trim_start)
                    .and_then(|count| count.strip_prefix('('))
                    .and_then(|count| count.strip_suffix(')'))
                    .ok_or_else(|| {
                        let start = offset(code, count);
                        Fault::at("expected range(count)", start..start + count.len())
                    })?;
                let count = self
                    .expression(count_source, Kind::Int)
                    .map_err(|fault| fault.within(code, count_source))?;
                let variable = if name == "_" {
                    None
                } else {
                    let start = offset(code, name);
                    Some(
                        self.declare(name, Kind::Int, true)
                            .map_err(|message| Fault::at(message, start..start + name.len()))?,
                    )
                };
                self.nesting()
                    .map_err(|message| Fault::at(message, keyword.clone()))?;
                let start = self.ops.len();
                self.push(
                    line,
                    Op::Repeat {
                        count,
                        exit: 0,
                        variable,
                    },
                );
                open(self, Block::Loop { start, variable });
            }
            "while" => {
                header(rest)?;
                let body = inside_header(rest);
                let (condition, limit) = self
                    .limited(body)
                    .map_err(|fault| fault.within(code, body))?;
                self.nesting()
                    .map_err(|message| Fault::at(message, keyword.clone()))?;
                let start = self.ops.len();
                self.push(
                    line,
                    Op::While {
                        condition,
                        limit,
                        exit: 0,
                    },
                );
                open(
                    self,
                    Block::Loop {
                        start,
                        variable: None,
                    },
                );
            }
            "break" | "continue" | "pass" => {
                if !rest.is_empty() {
                    let start = offset(code, rest);
                    return Err(Fault::at(
                        format!("'{word}' takes nothing after it"),
                        start..start + rest.len(),
                    ));
                }
                if word != "pass" {
                    let header = self
                        .blocks
                        .iter()
                        .rev()
                        .find_map(|open| match open.block {
                            Block::Loop { start, .. } => Some(start),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            Fault::at(format!("'{word}' outside a loop"), keyword.clone())
                        })?;
                    let op = if word == "break" {
                        Op::Break(header)
                    } else {
                        Op::Continue(header)
                    };
                    self.push(line, op);
                }
            }
            "def" | "return" => {
                return Err(Fault::at(
                    "functions are not available yet; 'def' and 'return' are reserved",
                    keyword,
                ));
            }
            _ => return Err(Fault::at(format!("unknown command '{word}'"), keyword)),
        }
        Ok(())
    }

    /// `name: type = value`, `name = value` or `name op= value`; spans in `code`.
    fn assignment(&mut self, name: &str, code: &str, line: usize) -> Result<(), Fault> {
        let rest = code[name.len()..].trim_start();
        let name_at = |message: String| Fault::at(message, 0..name.len());
        if let Some(declaration) = rest.strip_prefix(':') {
            let (kind_text, value) = declaration
                .split_once('=')
                .ok_or_else(|| name_at("expected 'name: type = value'".into()))?;
            let kind_text = kind_text.trim();
            let kind = Kind::parse(kind_text).ok_or_else(|| {
                let start = offset(code, kind_text);
                Fault::at(
                    "expected type int, bool or text",
                    start..start + kind_text.len(),
                )
            })?;
            if !self.blocks.is_empty() {
                return Err(name_at(
                    "declare variables at the top level, then assign them inside blocks".into(),
                ));
            }
            // The value cannot read the variable it declares.
            let expr = self
                .expression(value, kind)
                .map_err(|fault| fault.within(code, value))?;
            let index = self.declare(name, kind, false).map_err(name_at)?;
            self.push(line, Op::Store(index, expr));
            return Ok(());
        }
        let (index, kind) = self.lookup(name).ok_or_else(|| {
            name_at(format!(
                "'{name}' must be declared with a type first, such as '{name}: int = 0'"
            ))
        })?;
        if self.variables[index].loop_counter {
            return Err(name_at(format!(
                "'{name}' is a loop variable and can't be assigned"
            )));
        }
        let expr = if let Some(value) = rest.strip_prefix('=') {
            self.expression(value, kind)
                .map_err(|fault| fault.within(code, value))?
        } else {
            let op = rest.chars().next().unwrap_or('=');
            let value = &rest[2..];
            let operand = expression::parse(value, |name| self.lookup(name))
                .map_err(|fault| fault.within(code, value))?;
            Expr::compound(index, kind, op, operand).map_err(|message| {
                let start = offset(code, rest);
                Fault::at(message, start..start + 2)
            })?
        };
        self.push(line, Op::Store(index, expr));
        Ok(())
    }

    pub(in crate::script) fn action(&mut self, index: usize, line: usize) {
        self.push(line, Op::Action(index));
    }

    /// Close every block at the end of the source and report what
    /// only the whole source shows.
    pub(in crate::script) fn end_of_source(&mut self) -> Vec<Deferred> {
        while let Some(top) = self.blocks.last() {
            let (line, keyword) = (top.line, top.keyword.clone());
            if top.body.is_none() {
                self.empty_block(line, keyword);
            } else {
                self.close(line).expect("the block is open");
            }
        }
        if self.used
            && let Some((line, span)) = self.stray.take()
        {
            self.deferred.push(Deferred {
                line,
                fault: Fault::at("unexpected indent", span),
            });
        }
        std::mem::take(&mut self.deferred)
    }

    pub(in crate::script) fn finish(self) -> Option<Code> {
        self.used.then_some(Code {
            ops: self.ops,
            variables: self.variables,
        })
    }

    fn lookup(&self, name: &str) -> Option<(usize, Kind)> {
        self.variables
            .iter()
            .enumerate()
            .rev()
            .find(|(index, variable)| variable.name == name && self.visible(*index))
            .map(|(index, variable)| (index, variable.kind))
    }

    /// A loop variable is visible only inside its own open loop.
    fn visible(&self, index: usize) -> bool {
        !self.variables[index].loop_counter
            || self.blocks.iter().any(|open| {
                matches!(open.block, Block::Loop { variable: Some(variable), .. } if variable == index)
            })
    }

    fn declare(&mut self, name: &str, kind: Kind, loop_counter: bool) -> Result<usize, String> {
        if !valid_name(name) {
            return Err("expected a variable name of up to 32 ASCII letters, digits or underscores, starting with a letter or underscore".into());
        }
        if keyword(name)
            || super::RESERVED.contains(&name)
            || crate::script::COMMANDS.contains(&name)
            || self.lookup(name).is_some()
        {
            return Err(format!("variable '{name}' is reserved or already declared"));
        }
        let index = self.variables.len();
        if index == MAX_VARIABLES {
            return Err(format!("at most {MAX_VARIABLES} variables can be declared"));
        }
        self.variables.push(Variable {
            name: name.into(),
            kind,
            loop_counter,
        });
        Ok(index)
    }

    /// A typed expression; a fault's span is in `source`.
    fn expression(&self, source: &str, kind: Kind) -> Result<Expr, Fault> {
        let expr = expression::parse(source, |name| self.lookup(name))?;
        expect_kind(&expr, kind).map_err(|message| {
            let trimmed = source.trim();
            let start = offset(source, trimmed);
            Fault::at(message, start..start + trimmed.len())
        })?;
        Ok(expr)
    }

    /// A while condition and its cap; a fault's span is in `source`.
    fn limited(&self, source: &str) -> Result<(Expr, u16), Fault> {
        let (condition, limit) = expression::limited_condition(source, |name| self.lookup(name))?;
        expect_kind(&condition, Kind::Bool)
            .map_err(|message| Fault::at(message, 0..source.len()))?;
        let limit = u16::try_from(limit)
            .ok()
            .filter(|limit| (1..=MAX_ITERATIONS).contains(&i64::from(*limit)))
            .ok_or_else(|| format!("while limit must be from 1 to {MAX_ITERATIONS}"))?;
        Ok((condition, limit))
    }

    fn push(&mut self, line: usize, op: Op) {
        self.ops.push(Located { line, op });
    }

    fn nesting(&self) -> Result<(), String> {
        if self.blocks.len() == MAX_NESTING {
            return Err(format!("blocks may be nested at most {MAX_NESTING} levels"));
        }
        Ok(())
    }

    fn elif(&mut self, line: usize, condition: Expr) -> Result<(), String> {
        let Some(Open {
            block: Block::If { start, jumps },
            body,
            ..
        }) = self.blocks.last_mut()
        else {
            return Err("elif requires an if without an else".into());
        };
        // The finished branch leaves the chain; a false test tries this one.
        jumps.push(self.ops.len());
        let next = self.ops.len() + 1;
        let Op::If { otherwise, .. } = &mut self.ops[*start].op else {
            unreachable!("if blocks point to their header");
        };
        *otherwise = next;
        *start = next;
        *body = None;
        self.push(line, Op::Jump(0));
        self.push(
            line,
            Op::If {
                condition,
                otherwise: 0,
            },
        );
        Ok(())
    }

    fn otherwise(&mut self, line: usize) -> Result<(), String> {
        let Some(open) = self.blocks.last_mut() else {
            return Err("else requires an open if without an else".into());
        };
        let Block::If { start, jumps } = &mut open.block else {
            return Err("else requires an open if without an else".into());
        };
        let jump = self.ops.len();
        let Op::If { otherwise, .. } = &mut self.ops[*start].op else {
            unreachable!("if blocks point to their header");
        };
        *otherwise = jump + 1;
        open.block = Block::Else {
            jump,
            jumps: std::mem::take(jumps),
        };
        open.body = None;
        self.push(line, Op::Jump(0));
        Ok(())
    }

    fn close(&mut self, line: usize) -> Result<(), String> {
        let Some(open) = self.blocks.pop() else {
            return Err("no block is open".into());
        };
        if let Block::Loop { start, .. } = open.block {
            self.push(line, Op::Jump(start));
        }
        let after = self.ops.len();
        let (header, jumps) = match open.block {
            Block::If { start, jumps } => (start, jumps),
            Block::Else { jump, jumps } => (jump, jumps),
            Block::Loop { start, .. } => (start, Vec::new()),
        };
        for jump in jumps {
            self.ops[jump].op = Op::Jump(after);
        }
        match &mut self.ops[header].op {
            Op::If { otherwise, .. } => *otherwise = after,
            Op::Repeat { exit, .. } | Op::While { exit, .. } => *exit = after,
            Op::Jump(end) => *end = after,
            _ => unreachable!("open blocks point to a header or an else"),
        }
        Ok(())
    }
}

/// The words of an action call, from its opening parenthesis; spans in `call`.
/// Each argument is one literal: a number as written, or quoted text.
fn arguments(command: &str, call: &str) -> Result<(Vec<String>, Range<usize>), Fault> {
    let free = crate::script::FREE_TEXT.contains(&command);
    let inner_end = call
        .rfind(')')
        .filter(|end| call[end + 1..].trim().is_empty())
        .ok_or_else(|| {
            Fault::at(
                "expected ')' to close the call on the same line",
                0..call.len(),
            )
        })?;
    let inside = 1..inner_end;
    let mut words = Vec::new();
    let mut rest = &call[1..inner_end];
    let mut expect_argument = false;
    loop {
        rest = rest.trim_start();
        let start = offset(call, rest);
        let Some(first) = rest.chars().next() else {
            if expect_argument {
                return Err(Fault::at(
                    "expected an argument after ','",
                    start.saturating_sub(1)..start,
                ));
            }
            break;
        };
        let (word, width) = if matches!(first, '"' | '\'') {
            let (text, width) = expression::text_literal(&rest[1..], first, usize::MAX)
                .map_err(|(message, end)| Fault::at(message, start..start + 1 + end))?;
            if text.chars().any(char::is_control) {
                return Err(Fault::at(
                    "control characters are unavailable in text",
                    start..start + 1 + width,
                ));
            }
            if !free && (text.is_empty() || text.chars().any(char::is_whitespace)) {
                return Err(Fault::at(
                    format!("each argument of {command} is one word; split it into arguments"),
                    start..start + 1 + width,
                ));
            }
            (text, 1 + width)
        } else {
            let width = rest
                .find(|ch: char| ch == ',' || ch.is_whitespace())
                .unwrap_or(rest.len());
            let token = &rest[..width];
            if !number(token) {
                return Err(Fault::at(
                    "action arguments are literals: quote text, such as \"I\"",
                    start..start + width,
                ));
            }
            (token.to_owned(), width)
        };
        if free {
            words.extend(word.split_whitespace().map(str::to_owned));
        } else {
            words.push(word);
        }
        rest = rest[width..].trim_start();
        expect_argument = false;
        if let Some(after) = rest.strip_prefix(',') {
            rest = after;
            expect_argument = true;
        } else if !rest.is_empty() {
            let start = offset(call, rest);
            return Err(Fault::at(
                "expected ',' between arguments",
                start..start + 1,
            ));
        }
    }
    Ok((words, inside))
}

/// A bare argument: a decimal number, signed, with an optional fraction or exponent.
fn number(token: &str) -> bool {
    let digits = token.strip_prefix(['+', '-']).unwrap_or(token);
    let (mantissa, exponent) = digits
        .split_once(['e', 'E'])
        .map_or((digits, None), |(mantissa, exponent)| {
            (mantissa, Some(exponent))
        });
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    !(whole.is_empty() && fraction.is_empty())
        && all_digits(whole)
        && all_digits(fraction)
        && exponent.is_none_or(|exponent| {
            let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            !exponent.is_empty() && all_digits(exponent)
        })
}

/// A header's text between its keyword and its closing colon.
fn inside_header(rest: &str) -> &str {
    rest.strip_suffix(':').unwrap_or(rest).trim()
}

fn empty(keyword: Range<usize>) -> Fault {
    Fault::at(
        "expected an indented block; use pass for an empty one",
        keyword,
    )
}

fn expect_kind(expr: &Expr, expected: Kind) -> Result<(), String> {
    if expr.kind == expected {
        Ok(())
    } else {
        Err(format!("expected {expected}, found {}", expr.kind))
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_NAME_BYTES
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn bare_arguments_are_decimal_numbers_only() {
        for ok in [
            "0", "500", "-30", "+90", "1.5", ".5", "5.", "5e2", "1E-3", "007", "-0",
        ] {
            assert!(number(ok), "{ok}");
        }
        for bad in [
            "", "-", ".", "e5", "5e", "inf", "-nan", "0x10", "1h", "I", "1_000", "5e+",
        ] {
            assert!(!number(bad), "{bad}");
        }
    }
}

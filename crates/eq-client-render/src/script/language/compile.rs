//! Lower indented blocks to a bounded sequence of typed operations.

use std::ops::Range;

use super::{
    Fault, Kind, Shape,
    expression::{self, Expr},
    keyword, offset, shape,
};

const MAX_VARIABLES: usize = 32;
const MAX_FUNCTIONS: usize = 32;
const MAX_PARAMETERS: usize = 8;
/// Parameters included.
const MAX_LOCALS: usize = 32;
const MAX_NAME_BYTES: usize = 32;
const MAX_NESTING: usize = 16;
pub(super) const MAX_ITERATIONS: i64 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::script) struct Code {
    pub(super) ops: Vec<Located>,
    pub(super) variables: Vec<Variable>,
    /// Each `if` or `elif` header whose false branch enters an `else`, and
    /// that `else`'s line: entering it is an edge, not an op of its own.
    pub(super) else_edges: Vec<(usize, usize)>,
    /// The ops of every block, header included, for stepping out of one.
    pub(super) blocks: Vec<Range<usize>>,
    pub(super) functions: Vec<Function>,
    /// The end of the top level: function bodies follow it, entered only
    /// by calls.
    pub(super) end: usize,
}

/// A compiled `def`: its body's first op and the slots each call owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Function {
    pub(super) name: String,
    /// The `def` line.
    pub(super) line: usize,
    pub(super) entry: usize,
    /// The parameters' slots, in order.
    pub(super) parameters: Vec<usize>,
    /// Every slot local to a call, parameters included.
    pub(super) locals: Vec<usize>,
}

impl Code {
    /// The lines a breakpoint can pause on: every statement that runs as an
    /// op, every `else` entered by an edge, and every `def`, entered by a
    /// call. Internal jumps and a body's implicit return carry a line but
    /// never run as a statement.
    pub(in crate::script) fn statement_lines(&self) -> impl Iterator<Item = usize> + '_ {
        self.ops
            .iter()
            .filter(|located| !matches!(located.op, Op::Jump(_) | Op::Return { explicit: false }))
            .map(|located| located.line)
            .chain(self.else_edges.iter().map(|(_, line)| *line))
            .chain(self.functions.iter().map(|function| function.line))
    }
}

impl Code {
    /// How many variable slots the script uses.
    #[cfg(test)]
    pub(in crate::script) fn variable_count(&self) -> usize {
        self.variables.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Variable {
    pub(super) name: String,
    pub(super) kind: Kind,
    /// A `for` variable: assigned only by its loop.
    pub(super) loop_counter: bool,
    /// The function whose calls own this slot; `None` for a global.
    pub(super) function: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Located {
    pub(super) line: usize,
    pub(super) op: Op,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Action(usize),
    /// A declaration: the first value of this slot.
    Store(usize, Expr),
    /// An assignment to a declared slot, whose declaration must have run.
    Assign(usize, Expr),
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
        /// The written cap; none runs until the condition is false, a
        /// `break`, or the run's own length.
        limit: Option<u16>,
        exit: usize,
    },
    /// Leave the loop whose header is here.
    Break(usize),
    /// Go back to the loop header here.
    Continue(usize),
    /// Call a function with these arguments, evaluated in the caller.
    Call {
        function: usize,
        arguments: Vec<Expr>,
    },
    /// Leave the function: a `return` line, or the end of its body.
    Return {
        explicit: bool,
    },
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
    /// A function body, below every other block. Its end returns.
    Function,
    /// A header that failed. It emits no ops, but its body is read by the
    /// ordinary rules, so errors inside it are its own: a loop's `break`
    /// still has a loop, its variable is still known, and an `if` still
    /// takes its `elif` and `else`.
    Failed {
        looping: bool,
        variable: Option<usize>,
        alternatives: bool,
    },
}

impl Block {
    /// An `if` chain that an `elif` or `else` at its indent continues.
    const fn takes_alternatives(&self) -> bool {
        matches!(
            self,
            Self::If { .. }
                | Self::Failed {
                    alternatives: true,
                    ..
                }
        )
    }
}

/// An open block's place in the source, for indentation.
struct Open {
    block: Block,
    line: usize,
    /// The block's first op: its header.
    first: usize,
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

/// A function as the pre-pass reads its header.
struct Signature {
    name: String,
    line: usize,
    parameters: Vec<Parameter>,
    entry: Option<usize>,
    slots: Vec<usize>,
}

struct Parameter {
    name: String,
    kind: Kind,
    /// The name's bytes in the `def` line.
    span: Range<usize>,
}

/// An error on another line than the one being read.
pub(in crate::script) struct Deferred {
    pub(in crate::script) line: usize,
    pub(in crate::script) fault: Fault,
}

pub(in crate::script) struct Builder {
    ops: Vec<Located>,
    variables: Vec<Variable>,
    else_edges: Vec<(usize, usize)>,
    ranges: Vec<Range<usize>>,
    blocks: Vec<Open>,
    used: bool,
    indent: Option<char>,
    stray: Option<(usize, Range<usize>)>,
    deferred: Vec<Deferred>,
    functions: Vec<Signature>,
    /// Names of `def` headers that failed after their name: a call to one
    /// is accepted unchecked, so the header's error is the only one.
    failed_functions: Vec<String>,
    /// The function whose body is being compiled.
    current: Option<usize>,
    /// Where the top level ended, once the first body starts.
    end: Option<usize>,
}

impl Builder {
    pub(in crate::script) fn new() -> Self {
        Self {
            ops: Vec::new(),
            variables: Vec::new(),
            else_edges: Vec::new(),
            ranges: Vec::new(),
            blocks: Vec::new(),
            used: false,
            indent: None,
            stray: None,
            deferred: Vec::new(),
            functions: Vec::new(),
            failed_functions: Vec::new(),
            current: None,
            end: None,
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
        let start = indent.len();
        // A `def` here is misplaced: its error is the only one, so its
        // indentation is taken as it stands.
        if let Shape::Keyword("def") = shape(code) {
            let stray = self.stray.clone();
            let _ = self.indentation(indent, code, line);
            self.stray = stray;
            self.used = true;
            return Err(Fault::at(
                "def is allowed only at the start of a line, outside any block",
                start..start + 3,
            ));
        }
        if let Err(fault) = self.indentation(indent, code, line) {
            // A misplaced header still holds its body, so the body's lines
            // are read without an indent error each.
            if let Shape::Keyword(word @ ("if" | "elif" | "else" | "for" | "while")) = shape(code) {
                self.used = true;
                self.fail(word, code, line, start, false);
            }
            return Err(fault);
        }
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
                if let Err(fault) = self.keyword(word, code, line, start) {
                    if matches!(word, "if" | "elif" | "else" | "for" | "while") {
                        self.fail(word, code, line, start, true);
                    }
                    return Err(at(fault));
                }
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
            Shape::Unknown(word) => {
                let rest = code[word.len()..].trim_start();
                if let Some(function) = self.function(word)
                    && rest.starts_with('(')
                {
                    self.used = true;
                    self.call(function, code, offset(code, rest), line)
                        .map_err(at)?;
                    return Ok(None);
                }
                let failed = self.failed_functions.iter().any(|name| name == word);
                if failed && rest.starts_with('(') {
                    self.used = true;
                    return Ok(None);
                }
                if failed || self.function(word).is_some() {
                    return Err(at(Fault::at(
                        format!("call {word} with parentheses, such as {word}(...)"),
                        0..word.len(),
                    )));
                }
                Err(at(Fault::at(
                    format!("unknown command '{word}'"),
                    0..word.len(),
                )))
            }
        }
    }

    /// Close the blocks this line's indent ends and check the indent itself.
    fn indentation(&mut self, indent: &str, code: &str, line: usize) -> Result<(), Fault> {
        let width = indent.len();
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
            } else if alternative && width == top.header && top.block.takes_alternatives() {
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
        let (mut closed, mut finished) = (false, false);
        while let Some(top) = self.blocks.last() {
            // Only the innermost block can still await its body.
            let body = top.body.unwrap_or(width + 1);
            let pairs = alternative && top.header == width && top.block.takes_alternatives();
            if body <= width || pairs {
                break;
            }
            let (header, same) = (top.line, top.header == width);
            self.close(header).map_err(Fault::from)?;
            closed = true;
            finished |= same;
        }
        if alternative {
            if finished {
                // The block at this indentation already ended, such as an if that has its else.
                let (word, message) = if code.starts_with("elif") {
                    ("elif", "elif requires an if without an else")
                } else {
                    ("else", "else requires an open if without an else")
                };
                return Err(Fault::at(message, width..width + word.len()));
            }
            return match self.blocks.last() {
                Some(top) if top.header != width && top.body == Some(width) => Err(Fault::at(
                    "this needs an 'if' at the same indentation",
                    0..width,
                )),
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
        // A failed header has its own error; an empty body adds nothing.
        if !matches!(
            self.blocks.last().map(|open| &open.block),
            Some(Block::Failed { .. })
        ) {
            self.deferred.push(Deferred {
                line,
                fault: empty(keyword),
            });
        }
        self.close(line).expect("the block is open");
    }

    /// Keep a failed header's place: an `elif` or `else` continues its chain
    /// as a failed one, any other header opens a failed block. `continues` is
    /// false when the line's indentation was the failure.
    fn fail(&mut self, word: &str, code: &str, line: usize, indent: usize, continues: bool) {
        let looping = matches!(word, "for" | "while");
        // A `for` whose name is sound keeps its variable for the body.
        let variable = (word == "for")
            .then(|| inside_header(code[word.len()..].trim()))
            .and_then(|body| body.split_once(" in "))
            .map(|(name, _)| name.trim())
            .filter(|name| *name != "_")
            .and_then(|name| {
                self.lookup(name)
                    .map(|(index, _)| index)
                    .or_else(|| self.declare(name, Kind::Int, true).ok())
            });
        let block = Block::Failed {
            looping,
            variable,
            alternatives: matches!(word, "if" | "elif"),
        };
        let keyword = indent..indent + word.len();
        if continues
            && matches!(word, "elif" | "else")
            && let Some(top) = self.blocks.last_mut()
            && top.header == indent
        {
            top.block = block;
            top.body = None;
            top.line = line;
            top.keyword = keyword;
            return;
        }
        self.blocks.push(Open {
            block,
            line,
            first: self.ops.len(),
            header: indent,
            body: None,
            keyword,
        });
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
                first: builder.ops.len() - 1,
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
                    // A loop outside the function this line is in never counts.
                    let header = self
                        .blocks
                        .iter()
                        .rev()
                        .take_while(|open| !matches!(open.block, Block::Function))
                        .find_map(|open| match open.block {
                            Block::Loop { start, .. } => Some(Some(start)),
                            Block::Failed { looping: true, .. } => Some(None),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            Fault::at(format!("'{word}' outside a loop"), keyword.clone())
                        })?;
                    // Inside a failed loop there is nothing to jump to; the
                    // script has an error already.
                    if let Some(header) = header {
                        let op = if word == "break" {
                            Op::Break(header)
                        } else {
                            Op::Continue(header)
                        };
                        self.push(line, op);
                    }
                }
            }
            "def" => unreachable!("a def line is read before its keyword"),
            "return" => {
                if !rest.is_empty() {
                    let start = offset(code, rest);
                    return Err(Fault::at(
                        "return values are not available yet; use return alone",
                        start..start + rest.len(),
                    ));
                }
                if self.current.is_none() {
                    return Err(Fault::at("'return' outside a function", keyword));
                }
                self.push(line, Op::Return { explicit: true });
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
            if self.depth() > 0 {
                return Err(name_at(
                    "declare variables at the top level of the script or of a function, then assign them inside blocks".into(),
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
        self.push(line, Op::Assign(index, expr));
        Ok(())
    }

    pub(in crate::script) fn action(&mut self, index: usize, line: usize) {
        self.push(line, Op::Action(index));
    }

    /// Close every open block, as the end of the source or of a function
    /// body does.
    pub(in crate::script) fn close_all(&mut self) {
        while let Some(top) = self.blocks.last() {
            let (line, keyword) = (top.line, top.keyword.clone());
            if top.body.is_none() {
                self.empty_block(line, keyword);
            } else {
                self.close(line).expect("the block is open");
            }
        }
    }

    /// Read a column-0 `def name(param: type, ...):` header, before any
    /// line compiles, so calls before and after it both resolve. Spans are
    /// in `code`, the line without its comment. Gives the function's index.
    pub(in crate::script) fn declare_function(
        &mut self,
        code: &str,
        line: usize,
    ) -> Result<usize, Fault> {
        self.used = true;
        let def_span = 0..3;
        let rest = code[3..].trim_start();
        let name_start = offset(code, rest);
        let name_length = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        let name = &rest[..name_length];
        let name_span = name_start..name_start + name.len().max(1);
        if name.is_empty() {
            return Err(Fault::at("expected 'def name(...):'", def_span));
        }
        if !valid_name(name) {
            return Err(Fault::at(
                "expected a function name of up to 32 ASCII letters, digits or underscores, starting with a letter or underscore",
                name_span,
            ));
        }
        if keyword(name)
            || super::RESERVED.contains(&name)
            || crate::script::COMMANDS.contains(&name)
        {
            return Err(Fault::at(
                format!("'{name}' is reserved and can't name a function"),
                name_span,
            ));
        }
        if self.function(name).is_some() {
            return Err(Fault::at(
                format!("function '{name}' is already defined"),
                name_span,
            ));
        }
        let parameters = Self::signature(code, &rest[name_length..], def_span)
            .and_then(|parameters| {
                if self.functions.len() == MAX_FUNCTIONS {
                    return Err(Fault::at(
                        format!("at most {MAX_FUNCTIONS} functions can be defined"),
                        name_span,
                    ));
                }
                Ok(parameters)
            })
            .inspect_err(|_| self.failed_functions.push(name.into()))?;
        self.functions.push(Signature {
            name: name.into(),
            line,
            parameters,
            entry: None,
            slots: Vec::new(),
        });
        Ok(self.functions.len() - 1)
    }

    /// The parameters of a `def` header from just after its name, up to and
    /// including the closing `:`.
    fn signature(code: &str, after: &str, def_span: Range<usize>) -> Result<Vec<Parameter>, Fault> {
        let after = after.trim_start();
        let open = offset(code, after);
        if !after.starts_with('(') {
            return Err(Fault::at(
                "expected '(' after the function name",
                open..open + after.len().max(1),
            ));
        }
        let Some(close) = after.find(')') else {
            return Err(Fault::at(
                "expected ')' to close the parameters",
                open..open + after.len(),
            ));
        };
        let tail = after[close + 1..].trim();
        if tail != ":" {
            if tail.starts_with("->") {
                let start = offset(code, tail);
                return Err(Fault::at(
                    "return values are not available yet",
                    start..start + tail.len(),
                ));
            }
            return Err(Fault::at("expected ':' at the end of the line", def_span));
        }
        parameters(code, &after[1..close])
    }

    /// A function's `def` line.
    pub(in crate::script) const fn function_count(&self) -> usize {
        self.functions.len()
    }

    pub(in crate::script) fn function_line(&self, function: usize) -> usize {
        self.functions[function].line
    }

    /// Start compiling a function's body, after every top-level line. Gives
    /// the errors on its `def` line, such as a parameter named like a global.
    pub(in crate::script) fn begin_function(&mut self, function: usize) -> Vec<Fault> {
        self.close_all();
        self.end.get_or_insert(self.ops.len());
        self.current = Some(function);
        let line = self.functions[function].line;
        self.functions[function].entry = Some(self.ops.len());
        self.blocks.push(Open {
            block: Block::Function,
            line,
            first: self.ops.len(),
            header: 0,
            body: None,
            keyword: 0..3,
        });
        let parameters = self.functions[function]
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    parameter.kind,
                    parameter.span.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut faults = Vec::new();
        for (name, kind, span) in parameters {
            if let Err(message) = self.declare(&name, kind, false) {
                faults.push(Fault::at(message, span));
            }
        }
        faults
    }

    /// End the body being compiled: its open blocks close and it returns.
    pub(in crate::script) fn end_function(&mut self) {
        self.close_all();
        self.current = None;
    }

    /// `name(arguments)` for a declared function; spans are in `code`, and
    /// the parenthesis is at `open`. Arguments are expressions.
    fn call(&mut self, function: usize, code: &str, open: usize, line: usize) -> Result<(), Fault> {
        let (pieces, inside) = call_arguments(code, open)?;
        let signature = &self.functions[function];
        let name = signature.name.clone();
        let kinds = signature
            .parameters
            .iter()
            .map(|parameter| parameter.kind)
            .collect::<Vec<_>>();
        if pieces.len() != kinds.len() {
            let span = if inside.is_empty() {
                inside.start - 1..inside.end + 1
            } else {
                inside
            };
            let given = pieces.len();
            let takes = kinds.len();
            let plural = if takes == 1 { "" } else { "s" };
            return Err(Fault::at(
                format!("{name} takes {takes} argument{plural}, given {given}"),
                span,
            ));
        }
        let mut arguments = Vec::with_capacity(pieces.len());
        for (range, kind) in pieces.into_iter().zip(kinds) {
            let source = &code[range.clone()];
            let expr = self
                .expression(source, kind)
                .map_err(|fault| fault.within(code, source))?;
            arguments.push(expr);
        }
        self.push(
            line,
            Op::Call {
                function,
                arguments,
            },
        );
        Ok(())
    }

    /// Report what only the whole source shows.
    pub(in crate::script) fn end_of_source(&mut self) -> Vec<Deferred> {
        self.close_all();
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
        let functions = self
            .functions
            .into_iter()
            .map(|signature| Function {
                name: signature.name,
                line: signature.line,
                entry: signature.entry.expect("every function body is compiled"),
                parameters: signature.slots[..signature.parameters.len()].to_vec(),
                locals: signature.slots,
            })
            .collect();
        let end = self.end.unwrap_or(self.ops.len());
        self.used.then_some(Code {
            ops: self.ops,
            variables: self.variables,
            else_edges: self.else_edges,
            blocks: self.ranges,
            functions,
            end,
        })
    }

    /// Open blocks in the current function body, or at the top level.
    fn depth(&self) -> usize {
        self.blocks
            .iter()
            .filter(|open| !matches!(open.block, Block::Function))
            .count()
    }

    fn function(&self, name: &str) -> Option<usize> {
        self.functions
            .iter()
            .position(|function| function.name == name)
    }

    fn lookup(&self, name: &str) -> Option<(usize, Kind)> {
        self.variables
            .iter()
            .enumerate()
            .rev()
            .find(|(index, variable)| variable.name == name && self.visible(*index))
            .map(|(index, variable)| (index, variable.kind))
    }

    /// A global is visible everywhere, a local only in its own function, and
    /// a loop variable only inside its own open loop.
    fn visible(&self, index: usize) -> bool {
        let variable = &self.variables[index];
        (variable.function.is_none() || variable.function == self.current)
            && (!variable.loop_counter
                || self.blocks.iter().any(|open| {
                    matches!(
                        open.block,
                        Block::Loop { variable: Some(variable), .. }
                            | Block::Failed { variable: Some(variable), .. }
                            if variable == index
                    )
                }))
    }

    fn declare(&mut self, name: &str, kind: Kind, loop_counter: bool) -> Result<usize, String> {
        if !valid_name(name) {
            return Err("expected a variable name of up to 32 ASCII letters, digits or underscores, starting with a letter or underscore".into());
        }
        if self.function(name).is_some() {
            return Err(format!("'{name}' is a function and can't be a variable"));
        }
        if let Some((index, _)) = self.lookup(name)
            && self.current.is_some()
            && self.variables[index].function.is_none()
        {
            return Err(format!(
                "'{name}' is a global variable; a function's variable can't reuse its name"
            ));
        }
        if keyword(name)
            || super::RESERVED.contains(&name)
            || crate::script::COMMANDS.contains(&name)
            || self.lookup(name).is_some()
        {
            return Err(format!("variable '{name}' is reserved or already declared"));
        }
        let owner = self.current;
        // A finished loop's variable of the same name takes the new loop, so
        // loops in sequence share one slot.
        if loop_counter
            && let Some(index) = self.variables.iter().position(|variable| {
                variable.name == name
                    && variable.loop_counter
                    && variable.kind == kind
                    && variable.function == owner
            })
        {
            return Ok(index);
        }
        let declared = self
            .variables
            .iter()
            .filter(|variable| variable.function == owner)
            .count();
        match owner {
            None if declared == MAX_VARIABLES => {
                return Err(format!("at most {MAX_VARIABLES} variables can be declared"));
            }
            Some(_) if declared == MAX_LOCALS => {
                return Err(format!(
                    "a function has at most {MAX_LOCALS} variables, parameters included"
                ));
            }
            _ => (),
        }
        let index = self.variables.len();
        self.variables.push(Variable {
            name: name.into(),
            kind,
            loop_counter,
            function: owner,
        });
        if let Some(function) = owner {
            self.functions[function].slots.push(index);
        }
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

    /// A while condition and its cap, if written; a fault's span is in `source`.
    fn limited(&self, source: &str) -> Result<(Expr, Option<u16>), Fault> {
        let (condition, limit) = expression::limited_condition(source, |name| self.lookup(name))?;
        expect_kind(&condition, Kind::Bool)
            .map_err(|message| Fault::at(message, 0..source.len()))?;
        let Some(limit) = limit else {
            return Ok((condition, None));
        };
        let limit = u16::try_from(limit)
            .ok()
            .filter(|limit| (1..=MAX_ITERATIONS).contains(&i64::from(*limit)))
            .ok_or_else(|| format!("while limit must be from 1 to {MAX_ITERATIONS}"))?;
        Ok((condition, Some(limit)))
    }

    fn push(&mut self, line: usize, op: Op) {
        self.ops.push(Located { line, op });
    }

    fn nesting(&self) -> Result<(), String> {
        if self.depth() == MAX_NESTING {
            return Err(format!("blocks may be nested at most {MAX_NESTING} levels"));
        }
        Ok(())
    }

    fn elif(&mut self, line: usize, condition: Expr) -> Result<(), String> {
        // A chain whose header failed takes its branches without ops.
        if let Some(top) = self.blocks.last_mut()
            && matches!(
                top.block,
                Block::Failed {
                    alternatives: true,
                    ..
                }
            )
        {
            top.body = None;
            top.line = line;
            return Ok(());
        }
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
        if let Block::Failed { alternatives, .. } = &mut open.block
            && *alternatives
        {
            *alternatives = false;
            open.body = None;
            open.line = line;
            return Ok(());
        }
        let Block::If { start, jumps } = &mut open.block else {
            return Err("else requires an open if without an else".into());
        };
        let jump = self.ops.len();
        self.else_edges.push((*start, line));
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
        if let Block::Function = open.block {
            self.push(line, Op::Return { explicit: false });
            return Ok(());
        }
        let after = self.ops.len();
        self.ranges.push(open.first..after);
        let (header, jumps) = match open.block {
            Block::If { start, jumps } => (start, jumps),
            Block::Else { jump, jumps } => (jump, jumps),
            Block::Loop { start, .. } => (start, Vec::new()),
            Block::Failed { .. } | Block::Function => return Ok(()),
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
    let inner_end = call.rfind(')').ok_or_else(|| {
        Fault::at(
            "expected ')' to close the call on the same line",
            0..call.len(),
        )
    })?;
    let after = call[inner_end + 1..].trim();
    if !after.is_empty() {
        let start = offset(call, after);
        return Err(Fault::at(
            "unexpected text after the call",
            start..start + after.len(),
        ));
    }
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

/// A function call's arguments, from its opening parenthesis at `open` in
/// `code`: each argument's bytes, split at commas outside quotes and
/// parentheses, and the bytes inside the call's parentheses.
fn call_arguments(code: &str, open: usize) -> Result<(Vec<Range<usize>>, Range<usize>), Fault> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut start = open + 1;
    let mut pieces = Vec::new();
    let mut close = None;
    for (index, ch) in code[open..].char_indices().map(|(at, ch)| (open + at, ch)) {
        if let Some(mark) = quote {
            if std::mem::take(&mut escaped) {
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == mark {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    pieces.push(start..index);
                    close = Some(index);
                    break;
                }
            }
            ',' if depth == 1 => {
                pieces.push(start..index);
                start = index + 1;
            }
            _ => (),
        }
    }
    let Some(close) = close else {
        return Err(Fault::at(
            "expected ')' to close the call on the same line",
            open..code.len(),
        ));
    };
    let after = code[close + 1..].trim();
    if !after.is_empty() {
        let at = offset(code, after);
        return Err(Fault::at(
            "unexpected text after the call",
            at..at + after.len(),
        ));
    }
    let inside = open + 1..close;
    if code[inside.clone()].trim().is_empty() {
        return Ok((Vec::new(), inside));
    }
    for piece in &pieces {
        let text = &code[piece.clone()];
        if text.trim().is_empty() {
            // The next character: the comma or parenthesis after the gap.
            let at = piece.start + text.len();
            return Err(Fault::at("expected an argument", at..at + 1));
        }
    }
    Ok((pieces, inside))
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

/// The parameters between a `def` header's parentheses, `inside`, with spans
/// in `code`.
fn parameters(code: &str, inside: &str) -> Result<Vec<Parameter>, Fault> {
    let mut parameters: Vec<Parameter> = Vec::new();
    if !inside.trim().is_empty() {
        for piece in inside.split(',') {
            let trimmed = piece.trim();
            let start = offset(code, piece) + (piece.len() - piece.trim_start().len());
            let span = start..start + trimmed.len().max(1);
            let Some((name, kind)) = trimmed.split_once(':') else {
                return Err(Fault::at("expected 'name: type' for each parameter", span));
            };
            let (name, kind_text) = (name.trim(), kind.trim());
            let name_span = start..start + name.len().max(1);
            let kind = Kind::parse(kind_text).ok_or_else(|| {
                let at = offset(code, kind_text);
                Fault::at(
                    "expected type int, bool or text",
                    at..at + kind_text.len().max(1),
                )
            })?;
            if !valid_name(name)
                || keyword(name)
                || super::RESERVED.contains(&name)
                || crate::script::COMMANDS.contains(&name)
            {
                return Err(Fault::at(
                    format!("'{name}' can't name a parameter"),
                    name_span,
                ));
            }
            if parameters.iter().any(|parameter| parameter.name == name) {
                return Err(Fault::at(
                    format!("parameter '{name}' is given twice"),
                    name_span,
                ));
            }
            if parameters.len() == MAX_PARAMETERS {
                return Err(Fault::at(
                    format!("a function takes at most {MAX_PARAMETERS} parameters"),
                    span,
                ));
            }
            parameters.push(Parameter {
                name: name.into(),
                kind,
                span: name_span,
            });
        }
    }
    Ok(parameters)
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

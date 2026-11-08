//! Lower line-oriented blocks to a bounded sequence of typed operations.

use super::{
    Kind,
    expression::{self, Expr},
    keyword,
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
    },
    While {
        condition: Expr,
        limit: u16,
        exit: usize,
    },
    Jump(usize),
}

enum Block {
    If(usize),
    Else(usize),
    Loop(usize),
}

impl Block {
    fn start(&self) -> usize {
        match *self {
            Self::If(start) | Self::Else(start) | Self::Loop(start) => start,
        }
    }
}

#[derive(Default)]
pub(in crate::script) struct Builder {
    ops: Vec<Located>,
    variables: Vec<Variable>,
    blocks: Vec<Block>,
    used: bool,
}

impl Builder {
    pub(in crate::script) fn statement(
        &mut self,
        source: &str,
        line: usize,
    ) -> Result<bool, String> {
        let (word, rest) = source
            .split_once(char::is_whitespace)
            .map_or((source, ""), |(word, rest)| (word, rest.trim()));
        if !keyword(word) {
            return Ok(false);
        }
        self.used = true;
        match word {
            "let" | "set" => self.store(word, rest, line)?,
            "if" => {
                let condition = self.expression(rest, Kind::Bool)?;
                self.open(
                    line,
                    Block::If,
                    Op::If {
                        condition,
                        otherwise: 0,
                    },
                )?;
            }
            "repeat" => {
                let count = self.expression(rest, Kind::Int)?;
                self.open(line, Block::Loop, Op::Repeat { count, exit: 0 })?;
            }
            "while" => {
                let (condition, limit) =
                    expression::limited_condition(rest, |name| self.lookup(name))?;
                expect_kind(&condition, Kind::Bool)?;
                let limit = u16::try_from(limit)
                    .ok()
                    .filter(|limit| (1..=MAX_ITERATIONS).contains(&i64::from(*limit)))
                    .ok_or_else(|| format!("while limit must be from 1 to {MAX_ITERATIONS}"))?;
                self.open(
                    line,
                    Block::Loop,
                    Op::While {
                        condition,
                        limit,
                        exit: 0,
                    },
                )?;
            }
            "else" if rest.is_empty() => self.otherwise(line)?,
            "end" if rest.is_empty() => self.close(line)?,
            _ => return Err(format!("'{word}' takes no arguments")),
        }
        Ok(true)
    }

    pub(in crate::script) fn action(&mut self, index: usize, line: usize) {
        self.push(line, Op::Action(index));
    }

    pub(in crate::script) fn unclosed(&self) -> impl Iterator<Item = usize> + '_ {
        self.blocks.iter().map(|block| self.ops[block.start()].line)
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
            .find(|(_, variable)| variable.name == name)
            .map(|(index, variable)| (index, variable.kind))
    }

    fn expression(&self, source: &str, kind: Kind) -> Result<Expr, String> {
        let expr = expression::parse(source, |name| self.lookup(name))?;
        expect_kind(&expr, kind)?;
        Ok(expr)
    }

    fn store(&mut self, word: &str, source: &str, line: usize) -> Result<(), String> {
        let (binding, value) = source
            .split_once('=')
            .ok_or("expected an assignment with '='")?;
        let binding = binding.trim();
        if word == "set" {
            let (index, kind) = self.lookup(binding).ok_or_else(|| {
                format!("'{binding}' must be declared with let before assignment")
            })?;
            let expr = self.expression(value, kind)?;
            self.push(line, Op::Store(index, expr));
            return Ok(());
        }
        if !self.blocks.is_empty() {
            return Err(
                "declare variables at the top level with let, then use set inside blocks".into(),
            );
        }
        let (name, kind) = binding
            .split_once(':')
            .ok_or("expected 'let name: type = expression'")?;
        let name = name.trim();
        if !valid_name(name) {
            return Err("expected a variable name of up to 32 ASCII letters, digits or underscores, starting with a letter or underscore".into());
        }
        if keyword(name)
            || matches!(
                name,
                "limit" | "int" | "bool" | "text" | "true" | "false" | "known" | "stale"
            )
            || self.lookup(name).is_some()
        {
            return Err(format!("variable '{name}' is reserved or already declared"));
        }
        let kind = Kind::parse(kind.trim()).ok_or("expected type int, bool or text")?;
        let index = self.variables.len();
        if index == MAX_VARIABLES {
            return Err(format!("at most {MAX_VARIABLES} variables can be declared"));
        }
        let expr = self.expression(value, kind)?;
        self.variables.push(Variable {
            name: name.into(),
            kind,
        });
        self.push(line, Op::Store(index, expr));
        Ok(())
    }

    fn push(&mut self, line: usize, op: Op) {
        self.ops.push(Located { line, op });
    }

    fn open(
        &mut self,
        line: usize,
        block: impl FnOnce(usize) -> Block,
        op: Op,
    ) -> Result<(), String> {
        if self.blocks.len() == MAX_NESTING {
            return Err(format!("blocks may be nested at most {MAX_NESTING} levels"));
        }
        self.blocks.push(block(self.ops.len()));
        self.push(line, op);
        Ok(())
    }

    fn otherwise(&mut self, line: usize) -> Result<(), String> {
        let Some(block @ Block::If(_)) = self.blocks.last_mut() else {
            return Err("else requires an open if without an else".into());
        };
        let jump = self.ops.len();
        let Op::If { otherwise, .. } = &mut self.ops[block.start()].op else {
            unreachable!("if blocks point to their header");
        };
        *otherwise = jump + 1;
        *block = Block::Else(jump);
        self.push(line, Op::Jump(0));
        Ok(())
    }

    fn close(&mut self, line: usize) -> Result<(), String> {
        let Some(block) = self.blocks.pop() else {
            return Err("end requires an open if, repeat or while".into());
        };
        let start = block.start();
        if matches!(block, Block::Loop(_)) {
            self.push(line, Op::Jump(start));
        }
        let after = self.ops.len();
        match &mut self.ops[start].op {
            Op::If { otherwise, .. } => *otherwise = after,
            Op::Repeat { exit, .. } | Op::While { exit, .. } => *exit = after,
            Op::Jump(end) => *end = after,
            _ => unreachable!("open blocks point to a header or an else"),
        }
        Ok(())
    }
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

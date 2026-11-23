//! Typed expression trees, with bounded parsing and checked evaluation.

use std::{ops::Range, sync::Arc};

use super::{Fault, Kind, Value, observe::Field};

const MAX_TOKENS: usize = 96;
const MAX_TEXT_BYTES: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Expr {
    pub(super) kind: Kind,
    node: Node,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Literal(Value),
    Variable(usize),
    Field(Field),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unary {
    Not,
    Negate,
    Known,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Binary {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl Binary {
    const fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual => 4,
            Self::Add | Self::Subtract => 5,
            Self::Multiply | Self::Divide | Self::Remainder => 6,
        }
    }

    fn result(self, left: Kind, right: Kind) -> Result<Kind, &'static str> {
        match self {
            Self::Or | Self::And if left == Kind::Bool && right == Kind::Bool => Ok(Kind::Bool),
            Self::Or | Self::And => Err("logical operators require two bool operands"),
            Self::Equal | Self::NotEqual if left == right => Ok(Kind::Bool),
            Self::Equal | Self::NotEqual => Err("equality requires matching operand types"),
            _ if left != Kind::Int || right != Kind::Int => {
                Err("arithmetic and ordering require two int operands")
            }
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual => Ok(Kind::Bool),
            _ => Ok(Kind::Int),
        }
    }

    fn evaluate(self, left: Value, right: Value) -> Result<Value, &'static str> {
        use Value::{Bool, Int};
        match (self, left, right) {
            (_, _, missing @ (Value::Unknown | Value::Stale)) => Ok(missing),
            (Self::Equal, left, right) => Ok(Bool(left == right)),
            (Self::NotEqual, left, right) => Ok(Bool(left != right)),
            (Self::Or | Self::And, Bool(_), Bool(right)) => Ok(Bool(right)),
            (Self::Less, Int(left), Int(right)) => Ok(Bool(left < right)),
            (Self::LessEqual, Int(left), Int(right)) => Ok(Bool(left <= right)),
            (Self::Greater, Int(left), Int(right)) => Ok(Bool(left > right)),
            (Self::GreaterEqual, Int(left), Int(right)) => Ok(Bool(left >= right)),
            (Self::Add, Int(left), Int(right)) => checked(left.checked_add(right)),
            (Self::Subtract, Int(left), Int(right)) => checked(left.checked_sub(right)),
            (Self::Multiply, Int(left), Int(right)) => checked(left.checked_mul(right)),
            (Self::Divide | Self::Remainder, Int(_), Int(0)) => Err("division by zero"),
            (Self::Divide, Int(left), Int(right)) => checked(left.checked_div(right)),
            (Self::Remainder, Int(left), Int(right)) => checked(left.checked_rem(right)),
            _ => unreachable!("expression types were checked during compilation"),
        }
    }
}

/// A variable read before its declaration ran. The runner replaces this
/// with a message naming the variable.
pub(super) const UNSET: &str = "a variable is read before it is set";

fn checked(value: Option<i64>) -> Result<Value, &'static str> {
    value.map(Value::Int).ok_or("integer overflow")
}

impl Expr {
    /// Evaluate with each variable's value, or `None` for one that has not
    /// been set yet: that is [`UNSET`], which the runner names.
    pub(super) fn evaluate(
        &self,
        variables: &[Option<Value>],
        read: &impl Fn(Field) -> Value,
    ) -> Result<Value, &'static str> {
        match &self.node {
            Node::Literal(value) => Ok(value.clone()),
            Node::Variable(index) => variables[*index].clone().ok_or(UNSET),
            Node::Field(field) => Ok(read(*field)),
            Node::Unary(op, child) => {
                let value = child.evaluate(variables, read)?;
                match (op, value) {
                    (Unary::Known, value) => {
                        Ok(Value::Bool(!matches!(value, Value::Unknown | Value::Stale)))
                    }
                    (Unary::Stale, value) => Ok(Value::Bool(value == Value::Stale)),
                    (_, missing @ (Value::Unknown | Value::Stale)) => Ok(missing),
                    (Unary::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
                    (Unary::Negate, Value::Int(value)) => checked(value.checked_neg()),
                    _ => unreachable!("unary types were checked during compilation"),
                }
            }
            Node::Binary(op, left, right) => {
                let left = left.evaluate(variables, read)?;
                match (&left, op) {
                    (Value::Unknown | Value::Stale, _)
                    | (Value::Bool(false), Binary::And)
                    | (Value::Bool(true), Binary::Or) => Ok(left),
                    _ => op.evaluate(left, right.evaluate(variables, read)?),
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Number(u64),
    Text(Arc<str>),
    Open,
    Close,
    /// `not`: binds looser than comparisons and tighter than `and`, as in Python.
    Not,
    Comma,
    Assign,
    Operator(Binary),
}

/// A token and its byte range in the expression source.
#[derive(Clone, Debug)]
struct Spanned<'a> {
    token: Token<'a>,
    span: Range<usize>,
}

fn lex(whole: &str) -> Result<Vec<Spanned<'_>>, Fault> {
    let mut tokens = Vec::new();
    let mut source = whole;
    loop {
        source = source.trim_start();
        let start = whole.len() - source.len();
        let Some(ch) = source.chars().next() else {
            return Ok(tokens);
        };
        if tokens.len() == MAX_TOKENS {
            return Err(Fault::at(
                format!("an expression may have at most {MAX_TOKENS} tokens"),
                start..whole.len(),
            ));
        }
        let (token, width) = match ch {
            '0'..='9' => {
                let len = source.bytes().take_while(u8::is_ascii_digit).count();
                let number = source[..len].parse().map_err(|_| {
                    Fault::at("integer literal is out of range", start..start + len)
                })?;
                (Token::Number(number), len)
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let len = source
                    .bytes()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
                    .count();
                let word = &source[..len];
                let token = match word {
                    "and" => Token::Operator(Binary::And),
                    "or" => Token::Operator(Binary::Or),
                    "not" => Token::Not,
                    _ => Token::Word(word),
                };
                (token, len)
            }
            '"' | '\'' => {
                let (text, width) = text_literal(&source[1..], ch, MAX_TEXT_BYTES)
                    .map_err(|(message, end)| Fault::at(message, start..start + 1 + end))?;
                (Token::Text(text.into()), 1 + width)
            }
            _ => operator(source)
                .map_err(|(message, width)| Fault::at(message, start..start + width))?,
        };
        source = &source[width..];
        tokens.push(Spanned {
            token,
            span: start..start + width,
        });
    }
}

/// The literal after its opening `quote` and the bytes it used, including the
/// closing quote, or a message and the byte where the literal stops being valid.
/// Expressions bound text to `MAX_TEXT_BYTES`; action arguments keep the
/// bounds their command has.
pub(super) fn text_literal(
    source: &str,
    quote: char,
    max: usize,
) -> Result<(String, usize), (String, usize)> {
    let mut text = String::new();
    let mut chars = source.char_indices();
    while let Some((index, ch)) = chars.next() {
        let end = index + ch.len_utf8();
        match ch {
            ch if ch == quote => return Ok((text, end)),
            '\\' => match chars.next() {
                Some((_, escaped)) if matches!(escaped, '\\' | '"' | '\'') => text.push(escaped),
                next => {
                    return Err((
                        "text escapes are limited to \\', \\\" and \\\\".into(),
                        next.map_or(end, |(index, ch)| index + ch.len_utf8()),
                    ));
                }
            },
            ch if ch.is_control() => {
                return Err(("control characters are unavailable in text".into(), end));
            }
            ch => text.push(ch),
        }
        if text.len() > max {
            return Err((format!("text literals are limited to {max} bytes"), end));
        }
    }
    Err(("unterminated text literal".into(), source.len()))
}

/// An operator token and its width, or a message and the width it covers.
fn operator(source: &str) -> Result<(Token<'_>, usize), (&'static str, usize)> {
    use Binary::{
        Add, Divide, Equal, Greater, GreaterEqual, Less, LessEqual, Multiply, NotEqual, Remainder,
        Subtract,
    };
    if source.starts_with(',') {
        return Ok((Token::Comma, 1));
    }
    if source.starts_with('=') && !source.starts_with("==") {
        return Ok((Token::Assign, 1));
    }
    for (symbol, op) in [
        ("==", Equal),
        ("!=", NotEqual),
        ("<=", LessEqual),
        (">=", GreaterEqual),
        ("<", Less),
        (">", Greater),
        ("+", Add),
        ("-", Subtract),
        ("*", Multiply),
        ("/", Divide),
        ("%", Remainder),
    ] {
        if source.starts_with(symbol) {
            return Ok((Token::Operator(op), symbol.len()));
        }
    }
    let token = match source.as_bytes()[0] {
        b'(' => Token::Open,
        b')' => Token::Close,
        _ => {
            let width = source.chars().next().map_or(1, char::len_utf8);
            return Err(("unexpected symbol in expression", width));
        }
    };
    Ok((token, 1))
}

/// The bytes from the first to the last of these tokens.
fn covering(tokens: &[Spanned<'_>]) -> Option<Range<usize>> {
    Some(tokens.first()?.span.start..tokens.last()?.span.end)
}

pub(super) fn parse(
    source: &str,
    lookup: impl Fn(&str) -> Option<(usize, Kind)>,
) -> Result<Expr, Fault> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        remaining: &tokens,
        lookup,
    };
    let value = parser.binary(1)?;
    if let Some(span) = covering(parser.remaining) {
        return Err(Fault::at("unexpected token after expression", span));
    }
    Ok(value)
}

/// A `while` header's condition and its required iteration cap: `c, limit=N`.
pub(super) fn limited_condition(
    source: &str,
    lookup: impl Fn(&str) -> Option<(usize, Kind)>,
) -> Result<(Expr, Option<u64>), Fault> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        remaining: &tokens,
        lookup,
    };
    let condition = parser.binary(1)?;
    let limit = match parser.remaining {
        [] => return Ok((condition, None)),
        [
            Spanned {
                token: Token::Comma,
                ..
            },
            Spanned {
                token: Token::Word("limit"),
                ..
            },
            Spanned {
                token: Token::Assign,
                ..
            },
            Spanned {
                token: Token::Number(limit),
                ..
            },
        ] => Some(*limit),
        _ => None,
    };
    limit.map(|limit| (condition, Some(limit))).ok_or_else(|| Fault {
        message: "expected 'while <bool expression>:' or 'while <bool expression>, limit=<integer literal>:'".into(),
        span: covering(parser.remaining).or(Some(0..source.len())),
    })
}

impl Expr {
    /// `variable op= value`, typed and checked as the long form would be.
    /// The variables this expression reads, in order.
    pub(super) fn reads(&self, into: &mut Vec<usize>) {
        match &self.node {
            Node::Variable(index) => into.push(*index),
            Node::Literal(_) | Node::Field(_) => (),
            Node::Unary(_, child) => child.reads(into),
            Node::Binary(_, left, right) => {
                left.reads(into);
                right.reads(into);
            }
        }
    }

    pub(super) fn compound(
        variable: usize,
        kind: Kind,
        op: char,
        value: Expr,
    ) -> Result<Self, &'static str> {
        let op = match op {
            '+' => Binary::Add,
            '-' => Binary::Subtract,
            '*' => Binary::Multiply,
            _ => return Err("expected +=, -= or *="),
        };
        if kind != Kind::Int || value.kind != Kind::Int {
            return Err("+=, -= and *= need an int variable and an int value");
        }
        Ok(Self {
            kind: Kind::Int,
            node: Node::Binary(
                op,
                Box::new(Self {
                    kind,
                    node: Node::Variable(variable),
                }),
                Box::new(value),
            ),
        })
    }
}

struct Parser<'a, 'source, Lookup> {
    remaining: &'a [Spanned<'source>],
    lookup: Lookup,
}

impl<'source, Lookup: Fn(&str) -> Option<(usize, Kind)>> Parser<'_, 'source, Lookup> {
    fn take(&mut self) -> Option<Spanned<'source>> {
        let (token, rest) = self.remaining.split_first()?;
        self.remaining = rest;
        Some(token.clone())
    }

    fn binary(&mut self, minimum: u8) -> Result<Expr, Fault> {
        let mut left = self.prefix()?;
        while let Some(Spanned {
            token: Token::Operator(op),
            span,
        }) = self.remaining.first().cloned()
        {
            let precedence = op.precedence();
            if precedence < minimum {
                break;
            }
            self.take();
            let right = self.binary(precedence + 1)?;
            left = Expr {
                kind: op
                    .result(left.kind, right.kind)
                    .map_err(|message| Fault::at(message, span))?,
                node: Node::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Expr, Fault> {
        let Spanned { token, span } = self.take().ok_or("expected an expression")?;
        let (kind, node) = match token {
            Token::Number(value) => (
                Kind::Int,
                Node::Literal(Value::Int(
                    i64::try_from(value)
                        .map_err(|_| Fault::at("integer literal is out of range", span))?,
                )),
            ),
            Token::Text(value) => (Kind::Text, Node::Literal(Value::Text(value))),
            Token::Word(word @ ("True" | "False")) => {
                (Kind::Bool, Node::Literal(Value::Bool(word == "True")))
            }
            Token::Not => {
                // Looser than comparisons, tighter than `and`: `not a == b` is
                // `not (a == b)`.
                let child = self.binary(Binary::Equal.precedence())?;
                if child.kind != Kind::Bool {
                    return Err(Fault::at("not requires a bool operand", span));
                }
                (Kind::Bool, Node::Unary(Unary::Not, Box::new(child)))
            }
            Token::Word(word @ ("known" | "stale")) => {
                let op = if word == "known" {
                    Unary::Known
                } else {
                    Unary::Stale
                };
                let Some(
                    open @ Spanned {
                        token: Token::Open, ..
                    },
                ) = self.take()
                else {
                    return Err(Fault::at(
                        "expected parentheses around the availability test",
                        span,
                    ));
                };
                return self.group(open.span).map(|child| Expr {
                    kind: Kind::Bool,
                    node: Node::Unary(op, Box::new(child)),
                });
            }
            Token::Word(word) => {
                if let Some((index, kind)) = (self.lookup)(word) {
                    (kind, Node::Variable(index))
                } else if let Some(field) = Field::parse(word) {
                    (field.kind(), Node::Field(field))
                } else {
                    return Err(Fault::at(
                        format!("unknown variable or client field '{word}'"),
                        span,
                    ));
                }
            }
            Token::Open => return self.group(span),
            Token::Operator(Binary::Subtract)
                if self.remaining.first().map(|next| &next.token)
                    == Some(&Token::Number(i64::MIN.unsigned_abs())) =>
            {
                self.take();
                (Kind::Int, Node::Literal(Value::Int(i64::MIN)))
            }
            Token::Operator(Binary::Subtract) => {
                let child = self.prefix()?;
                if child.kind != Kind::Int {
                    return Err(Fault::at("unary minus requires an int operand", span));
                }
                (Kind::Int, Node::Unary(Unary::Negate, Box::new(child)))
            }
            _ => {
                return Err(Fault::at(
                    "expected a literal, variable, client field or parenthesized expression",
                    span,
                ));
            }
        };
        Ok(Expr { kind, node })
    }

    /// The rest of a group whose opening parenthesis is at `open`.
    fn group(&mut self, open: Range<usize>) -> Result<Expr, Fault> {
        let value = self.binary(1)?;
        match self.take() {
            Some(Spanned {
                token: Token::Close,
                ..
            }) => Ok(value),
            _ => Err(Fault::at("expected closing parenthesis", open)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(source: &str, value: &Value) -> Result<Value, &'static str> {
        parse(source, |_| None)
            .unwrap()
            .evaluate(&[], &|_| value.clone())
    }

    #[test]
    fn precedence_and_integer_errors_are_checked() {
        assert_eq!(eval("2 + 3 * (4 - 1)", &Value::Unknown), Ok(Value::Int(11)));
        assert_eq!(eval("-8 / 3", &Value::Unknown), Ok(Value::Int(-2)));
        assert_eq!(
            eval("9223372036854775807 + 1", &Value::Unknown),
            Err("integer overflow")
        );
        assert_eq!(eval("9 % 0", &Value::Unknown), Err("division by zero"));
        assert_eq!(
            eval("-9223372036854775808", &Value::Unknown),
            Ok(Value::Int(i64::MIN))
        );
        assert_eq!(
            eval("-9223372036854775808 / -1", &Value::Unknown),
            Err("integer overflow")
        );
        assert!(parse("9223372036854775808", |_| None).is_err());
        assert!(parse("-9223372036854775809", |_| None).is_err());
        assert!(parse("True + 1", |_| None).is_err());
        assert!(parse("1 == '1'", |_| None).is_err());
    }

    #[test]
    fn unavailable_observations_keep_their_distinctions_and_can_be_guarded() {
        assert_eq!(
            eval("player.mana == 0", &Value::Unknown),
            Ok(Value::Unknown)
        );
        assert_eq!(eval("player.mana == 0", &Value::Stale), Ok(Value::Stale));
        assert_eq!(
            eval("player.mana == 0", &Value::Int(0)),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            eval("stale(player.mana)", &Value::Stale),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            eval("stale(player.mana)", &Value::Unknown),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            eval("known(player.mana) and player.mana > 0", &Value::Unknown),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            eval("True or 1 / 0 > 0", &Value::Unknown),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn python_words_and_precedence() {
        let python = |source: &str| parse(source, |_| None);
        let value = |source: &str| python(source).unwrap().evaluate(&[], &|_| Value::Unknown);
        // `not` is looser than comparisons and tighter than `and`.
        assert_eq!(value("not 1 == 2"), Ok(Value::Bool(true)));
        assert_eq!(value("not True and False"), Ok(Value::Bool(false)));
        assert_eq!(value("not False or False"), Ok(Value::Bool(true)));
        assert_eq!(value("'a\\'b' == \"a'b\""), Ok(Value::Bool(true)));
        for (bad, span) in [
            ("!True", 0..1),
            ("True && True", 5..6),
            ("False || True", 6..7),
        ] {
            let error = python(bad).unwrap_err();
            assert_eq!(error.message, "unexpected symbol in expression", "{bad}");
            assert_eq!(error.span, Some(span), "{bad}");
        }
        assert!(python("1 != 2").is_ok());
        assert!(python("'unterminated").is_err());
        assert_eq!(
            limited_condition("True, limit=5", |_| None).unwrap().1,
            Some(5)
        );
        assert!(limited_condition("True limit 5", |_| None).is_err());
    }
}

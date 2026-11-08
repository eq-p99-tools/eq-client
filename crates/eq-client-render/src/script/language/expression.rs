//! Typed expression trees, with bounded parsing and checked evaluation.

use std::sync::Arc;

use super::{Kind, Value, observe::Field};

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

fn checked(value: Option<i64>) -> Result<Value, &'static str> {
    value.map(Value::Int).ok_or("integer overflow")
}

impl Expr {
    pub(super) fn evaluate(
        &self,
        variables: &[Value],
        read: &impl Fn(Field) -> Value,
    ) -> Result<Value, &'static str> {
        match &self.node {
            Node::Literal(value) => Ok(value.clone()),
            Node::Variable(index) => Ok(variables[*index].clone()),
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
    Bang,
    Operator(Binary),
}

fn lex(mut source: &str) -> Result<Vec<Token<'_>>, String> {
    let mut tokens = Vec::new();
    loop {
        source = source.trim_start();
        let Some(ch) = source.chars().next() else {
            return Ok(tokens);
        };
        if tokens.len() == MAX_TOKENS {
            return Err(format!(
                "an expression may have at most {MAX_TOKENS} tokens"
            ));
        }
        let token = match ch {
            '0'..='9' => {
                let len = source.bytes().take_while(u8::is_ascii_digit).count();
                let (number, rest) = source.split_at(len);
                source = rest;
                Token::Number(
                    number
                        .parse()
                        .map_err(|_| "integer literal is out of range")?,
                )
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let len = source
                    .bytes()
                    .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
                    .count();
                let (word, rest) = source.split_at(len);
                source = rest;
                Token::Word(word)
            }
            '"' => {
                let (text, rest) = text_literal(&source[1..])?;
                source = rest;
                Token::Text(text.into())
            }
            _ => {
                let (token, width) = operator(source)?;
                source = &source[width..];
                token
            }
        };
        tokens.push(token);
    }
}

fn text_literal(source: &str) -> Result<(String, &str), String> {
    let mut text = String::new();
    let mut chars = source.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => return Ok((text, &source[index + 1..])),
            '\\' => match chars.next() {
                Some((_, escaped @ ('\\' | '"'))) => text.push(escaped),
                _ => return Err("text escapes are limited to \\\" and \\\\".into()),
            },
            ch if ch.is_control() => {
                return Err("control characters are unavailable in text".into());
            }
            ch => text.push(ch),
        }
        if text.len() > MAX_TEXT_BYTES {
            return Err(format!(
                "text literals are limited to {MAX_TEXT_BYTES} bytes"
            ));
        }
    }
    Err("unterminated text literal".into())
}

fn operator(source: &str) -> Result<(Token<'_>, usize), String> {
    use Binary::*;
    for (symbol, op) in [
        ("||", Or),
        ("&&", And),
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
        b'!' => Token::Bang,
        _ => return Err("unexpected symbol in expression".into()),
    };
    Ok((token, 1))
}

pub(super) fn parse(
    source: &str,
    lookup: impl Fn(&str) -> Option<(usize, Kind)>,
) -> Result<Expr, String> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        remaining: &tokens,
        lookup,
    };
    let value = parser.binary(1)?;
    if !parser.remaining.is_empty() {
        return Err("unexpected token after expression".into());
    }
    Ok(value)
}

pub(super) fn limited_condition(
    source: &str,
    lookup: impl Fn(&str) -> Option<(usize, Kind)>,
) -> Result<(Expr, u64), String> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        remaining: &tokens,
        lookup,
    };
    let condition = parser.binary(1)?;
    match parser.remaining {
        [Token::Word("limit"), Token::Number(limit)] => Ok((condition, *limit)),
        _ => Err("expected 'while <bool expression> limit <integer literal>'".into()),
    }
}

struct Parser<'a, 'source, Lookup> {
    remaining: &'a [Token<'source>],
    lookup: Lookup,
}

impl<Lookup: Fn(&str) -> Option<(usize, Kind)>> Parser<'_, '_, Lookup> {
    fn take(&mut self) -> Option<Token<'_>> {
        let (token, rest) = self.remaining.split_first()?;
        self.remaining = rest;
        Some(token.clone())
    }

    fn binary(&mut self, minimum: u8) -> Result<Expr, String> {
        let mut left = self.prefix()?;
        while let Some(Token::Operator(op)) = self.remaining.first().cloned() {
            let precedence = op.precedence();
            if precedence < minimum {
                break;
            }
            self.take();
            let right = self.binary(precedence + 1)?;
            left = Expr {
                kind: op.result(left.kind, right.kind)?,
                node: Node::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }

    fn prefix(&mut self) -> Result<Expr, String> {
        // Keep the token's source borrow independent of mutable parser access.
        let (token, rest) = self
            .remaining
            .split_first()
            .ok_or("expected an expression")?;
        self.remaining = rest;
        let (kind, node) = match token {
            Token::Number(value) => (
                Kind::Int,
                Node::Literal(Value::Int(
                    i64::try_from(*value).map_err(|_| "integer literal is out of range")?,
                )),
            ),
            Token::Text(value) => (Kind::Text, Node::Literal(Value::Text(value.clone()))),
            Token::Word("true" | "false") => (
                Kind::Bool,
                Node::Literal(Value::Bool(*token == Token::Word("true"))),
            ),
            Token::Word("known" | "stale") => {
                let op = if *token == Token::Word("known") {
                    Unary::Known
                } else {
                    Unary::Stale
                };
                if self.take() != Some(Token::Open) {
                    return Err("expected parentheses around the availability test".into());
                }
                return self.group().map(|child| Expr {
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
                    return Err(format!("unknown variable or client field '{word}'"));
                }
            }
            Token::Open => return self.group(),
            Token::Operator(Binary::Subtract)
                if self.remaining.first() == Some(&Token::Number(i64::MIN.unsigned_abs())) =>
            {
                self.take();
                (Kind::Int, Node::Literal(Value::Int(i64::MIN)))
            }
            Token::Bang | Token::Operator(Binary::Subtract) => {
                let (kind, op) = if *token == Token::Bang {
                    (Kind::Bool, Unary::Not)
                } else {
                    (Kind::Int, Unary::Negate)
                };
                let child = self.prefix()?;
                if child.kind != kind {
                    return Err(format!("unary operator requires a {kind} operand"));
                }
                (kind, Node::Unary(op, Box::new(child)))
            }
            _ => {
                return Err(
                    "expected a literal, variable, client field or parenthesized expression".into(),
                );
            }
        };
        Ok(Expr { kind, node })
    }

    fn group(&mut self) -> Result<Expr, String> {
        let value = self.binary(1)?;
        if self.take() != Some(Token::Close) {
            return Err("expected closing parenthesis".into());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(source: &str, value: Value) -> Result<Value, &'static str> {
        parse(source, |_| None)
            .unwrap()
            .evaluate(&[], &|_| value.clone())
    }

    #[test]
    fn precedence_and_integer_errors_are_checked() {
        assert_eq!(eval("2 + 3 * (4 - 1)", Value::Unknown), Ok(Value::Int(11)));
        assert_eq!(eval("-8 / 3", Value::Unknown), Ok(Value::Int(-2)));
        assert_eq!(
            eval("9223372036854775807 + 1", Value::Unknown),
            Err("integer overflow")
        );
        assert_eq!(eval("9 % 0", Value::Unknown), Err("division by zero"));
        assert_eq!(
            eval("-9223372036854775808", Value::Unknown),
            Ok(Value::Int(i64::MIN))
        );
        assert_eq!(
            eval("-9223372036854775808 / -1", Value::Unknown),
            Err("integer overflow")
        );
        assert!(parse("9223372036854775808", |_| None).is_err());
        assert!(parse("-9223372036854775809", |_| None).is_err());
        assert!(parse("true + 1", |_| None).is_err());
        assert!(parse("1 == \"1\"", |_| None).is_err());
    }

    #[test]
    fn unavailable_observations_keep_their_distinctions_and_can_be_guarded() {
        assert_eq!(eval("player.mana == 0", Value::Unknown), Ok(Value::Unknown));
        assert_eq!(eval("player.mana == 0", Value::Stale), Ok(Value::Stale));
        assert_eq!(
            eval("player.mana == 0", Value::Int(0)),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            eval("stale(player.mana)", Value::Stale),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            eval("stale(player.mana)", Value::Unknown),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            eval("known(player.mana) && player.mana > 0", Value::Unknown),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            eval("true || 1 / 0 > 0", Value::Unknown),
            Ok(Value::Bool(true))
        );
    }
}

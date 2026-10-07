//! Text to a small tree: numbers, parameter names, `+ - * /`, signs,
//! brackets, and a unit after a number or a bracketed group.
//!
//! ```text
//! sum     := product (("+" | "-") product)*
//! product := signed (("*" | "/") signed)*
//! signed  := ("-" | "+") signed | unitful
//! unitful := primary unit?
//! primary := number | name | "(" sum ")"
//! ```
//!
//! A word (`[A-Za-z_][A-Za-z0-9_]*`) right after a number or a `)` is a
//! unit, or an unknown one: `2 yd` is refused, not 2 times `yd`. Anywhere
//! else a word that isn't a unit is a parameter's name; unit words are
//! never names ([`check_name`](crate::check_name)).
//!
//! A unit binds tightest, so `-5 mm` is `-(5 mm)` and `2 * 3 in` is
//! `2 * (3 in)`. Numbers use `.` for decimals, with an optional exponent
//! (`1.5e-3`). Also taken: `"` for inches, `°` for degrees, units in any
//! case, `−` (the minus sign), `×` and `÷`, and any Unicode whitespace.

use crate::{AngleUnit, Error, ErrorKind, LengthUnit, MAX_DEPTH, MAX_LEN, Span, Unit};

/// A node of the tree and the text it came from, brackets included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Expr {
    pub node: Node,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Number(f64),
    /// A parameter, by name.
    Name(String),
    /// A bracketed expression.
    Group(Box<Expr>),
    /// A number or group with a unit after it.
    Unit(Box<Expr>, Unit),
    Neg(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// Parses `text`: at most [`MAX_LEN`] bytes, not blank.
pub(crate) fn parse(text: &str) -> Result<Expr, Error> {
    let all = Span::new(0, text.len());
    if text.len() > MAX_LEN {
        return Err(Error {
            kind: ErrorKind::TooLong,
            span: all,
        });
    }
    let tokens = lex(text)?;
    if tokens.is_empty() {
        return Err(Error {
            kind: ErrorKind::Empty,
            span: all,
        });
    }
    let mut parser = Parser {
        text,
        tokens,
        at: 0,
        depth: 0,
    };
    let expr = parser.sum()?;
    match parser.tokens.get(parser.at) {
        None => Ok(expr),
        Some(token) => Err(parser.unexpected(token.span)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Number(f64),
    Unit(Unit),
    /// A word that isn't a unit, its text at the token's span.
    Name,
    Plus,
    Minus,
    Star,
    Slash,
    Open,
    Close,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    tok: Tok,
    span: Span,
}

/// The spans of the parameter names in `text`, in order: renaming a
/// parameter rewrites them. Lexing goes on past what doesn't lex, so a
/// text in error still shows every name it uses.
pub(crate) fn name_spans(text: &str) -> Vec<Span> {
    if text.len() > MAX_LEN {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let _ = lex_into(text, true, &mut |token| {
        if token.tok == Tok::Name {
            spans.push(token.span);
        }
    });
    spans
}

fn lex(text: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    lex_into(text, false, &mut |token| tokens.push(token))?;
    Ok(tokens)
}

/// Lexes `text`, handing each token to `emit` until the first error, or
/// past errors when `past_errors`, skipping what's in error.
fn lex_into(text: &str, past_errors: bool, emit: &mut dyn FnMut(Token)) -> Result<(), Error> {
    // The previous token, for whether a word is a unit after it.
    let mut last: Option<Tok> = None;
    macro_rules! fail {
        ($error:expr) => {{
            if !past_errors {
                return Err($error);
            }
            last = None;
            continue;
        }};
    }
    let bytes = text.as_bytes();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let mut end = start + c.len_utf8();
        let tok = match c {
            c if c.is_whitespace() => continue,
            '+' => Tok::Plus,
            '-' | '−' => Tok::Minus,
            '*' | '×' => Tok::Star,
            '/' | '÷' => Tok::Slash,
            '(' => Tok::Open,
            ')' => Tok::Close,
            '"' => Tok::Unit(LengthUnit::In.into()),
            '°' => Tok::Unit(AngleUnit::Deg.into()),
            ',' => {
                fail!(Error {
                    kind: ErrorKind::Comma,
                    span: Span::new(start, end),
                })
            }
            '0'..='9' | '.' => {
                end = number_end(bytes, start);
                // The number is ASCII, so `end` is a boundary; skip the
                // characters after the first.
                while chars.next_if(|&(i, _)| i < end).is_some() {}
                let span = Span::new(start, end);
                let Ok(value) = text[start..end].parse::<f64>() else {
                    fail!(Error {
                        kind: ErrorKind::Unexpected(text[start..end].to_owned()),
                        span,
                    })
                };
                if !value.is_finite() {
                    fail!(Error {
                        kind: ErrorKind::BadNumber,
                        span,
                    })
                }
                Tok::Number(value)
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                while chars.next_if(|&(_, c)| word_char(c)).is_some() {}
                end = chars.peek().map_or(text.len(), |&(i, _)| i);
                let word = &text[start..end];
                let after_value = matches!(last, Some(Tok::Number(_) | Tok::Close));
                match unit(word) {
                    Some(unit) => Tok::Unit(unit),
                    None if after_value => fail!(Error {
                        kind: ErrorKind::UnknownUnit(word.to_owned()),
                        span: Span::new(start, end),
                    }),
                    None => Tok::Name,
                }
            }
            c => fail!(Error {
                kind: ErrorKind::Unexpected(c.to_string()),
                span: Span::new(start, end),
            }),
        };
        last = Some(tok);
        emit(Token {
            tok,
            span: Span::new(start, end),
        });
    }
    Ok(())
}

/// Whether `c` may be in a word after its first character.
pub(crate) fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The end of the number starting at `start`: digits with at most one
/// `.`, then an exponent if `e` or `E` is followed by digits (with a sign
/// or not), so that `2em` is 2 and a word.
fn number_end(bytes: &[u8], start: usize) -> usize {
    let digits = |mut at: usize| {
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        at
    };
    let mut end = digits(start);
    if bytes.get(end) == Some(&b'.') {
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let first = end + 1 + sign;
        if bytes.get(first).is_some_and(u8::is_ascii_digit) {
            end = digits(first);
        }
    }
    end
}

/// The unit a word names, in any case.
pub(crate) fn unit(word: &str) -> Option<Unit> {
    let units: [(&str, Unit); 7] = [
        ("mm", LengthUnit::Mm.into()),
        ("cm", LengthUnit::Cm.into()),
        ("m", LengthUnit::M.into()),
        ("in", LengthUnit::In.into()),
        ("ft", LengthUnit::Ft.into()),
        ("deg", AngleUnit::Deg.into()),
        ("rad", AngleUnit::Rad.into()),
    ];
    units
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))
        .map(|(_, unit)| unit)
}

struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    at: usize,
    /// Brackets and signs open around the current point.
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Tok> {
        self.tokens.get(self.at).map(|token| token.tok)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).copied();
        self.at += usize::from(token.is_some());
        token
    }

    fn sum(&mut self) -> Result<Expr, Error> {
        let mut left = self.product()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Op::Add,
                Some(Tok::Minus) => Op::Sub,
                _ => return Ok(left),
            };
            self.at += 1;
            let right = self.product()?;
            left = binary(op, left, right);
        }
    }

    fn product(&mut self) -> Result<Expr, Error> {
        let mut left = self.signed()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Op::Mul,
                Some(Tok::Slash) => Op::Div,
                _ => return Ok(left),
            };
            self.at += 1;
            let right = self.signed()?;
            left = binary(op, left, right);
        }
    }

    fn signed(&mut self) -> Result<Expr, Error> {
        let Some(token) = self.tokens.get(self.at).copied() else {
            return self.unitful();
        };
        let negate = match token.tok {
            Tok::Minus => true,
            Tok::Plus => false,
            _ => return self.unitful(),
        };
        self.at += 1;
        let inner = self.deeper(token.span, Parser::signed)?;
        let span = token.span.to(inner.span);
        Ok(if negate {
            Expr {
                node: Node::Neg(Box::new(inner)),
                span,
            }
        } else {
            // `+x` is `x`, but its span takes the sign in.
            Expr { span, ..inner }
        })
    }

    fn unitful(&mut self) -> Result<Expr, Error> {
        let inner = self.primary()?;
        let Some(Tok::Unit(unit)) = self.peek() else {
            return Ok(inner);
        };
        let token = self.next().expect("peeked");
        if let Some(Token {
            tok: Tok::Unit(_),
            span,
        }) = self.tokens.get(self.at)
        {
            return Err(Error {
                kind: ErrorKind::UnitOnUnit,
                span: *span,
            });
        }
        Ok(Expr {
            span: inner.span.to(token.span),
            node: Node::Unit(Box::new(inner), unit),
        })
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        let Some(token) = self.next() else {
            return Err(self.expected_number());
        };
        match token.tok {
            Tok::Number(value) => Ok(Expr {
                node: Node::Number(value),
                span: token.span,
            }),
            Tok::Name => Ok(Expr {
                node: Node::Name(self.text[token.span.range()].to_owned()),
                span: token.span,
            }),
            Tok::Open => {
                let inner = self.deeper(token.span, Parser::sum)?;
                match self.next() {
                    Some(Token {
                        tok: Tok::Close,
                        span,
                    }) => Ok(Expr {
                        node: Node::Group(Box::new(inner)),
                        span: token.span.to(span),
                    }),
                    Some(other) => Err(self.unexpected(other.span)),
                    None => Err(Error {
                        kind: ErrorKind::Unclosed,
                        span: token.span,
                    }),
                }
            }
            _ => {
                self.at -= 1;
                Err(self.expected_number())
            }
        }
    }

    /// Runs `inner` one level deeper, refusing to go past [`MAX_DEPTH`];
    /// `span` is the sign or bracket that opens the level.
    fn deeper(
        &mut self,
        span: Span,
        inner: fn(&mut Self) -> Result<Expr, Error>,
    ) -> Result<Expr, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(Error {
                kind: ErrorKind::TooDeep,
                span,
            });
        }
        self.depth += 1;
        let expr = inner(self);
        self.depth -= 1;
        expr
    }

    /// No number where one should be: at the token there, or at the end.
    fn expected_number(&self) -> Error {
        let span = match self.tokens.get(self.at) {
            Some(token) => token.span,
            None => Span::new(self.text.len(), self.text.len()),
        };
        Error {
            kind: ErrorKind::ExpectedNumber,
            span,
        }
    }

    fn unexpected(&self, span: Span) -> Error {
        Error {
            kind: ErrorKind::Unexpected(self.text[span.range()].to_owned()),
            span,
        }
    }
}

fn binary(op: Op, left: Expr, right: Expr) -> Expr {
    Expr {
        span: left.span.to(right.span),
        node: Node::Binary(op, Box::new(left), Box::new(right)),
    }
}

#[cfg(test)]
mod tests;

//! Evaluating the tree: every step checked finite and for units, bare
//! numbers given the design's length unit or degrees where they meet a
//! length or an angle, the result checked against the [`Ask`].

use crate::parse::{Expr, Node, Op, parse};
use crate::{AngleUnit, Ask, Error, ErrorKind, Kind, LengthUnit, MAX_LEN, Quantity, Span, Unit};

/// Evaluates `text` for `ask`, in model units: millimetres for lengths,
/// radians for angles.
pub fn evaluate(text: &str, ask: &Ask) -> Result<f64, Error> {
    let expr = parse(text)?;
    Evaluator::new(ask).result(&expr)
}

/// `text` with the design's length unit written in after each bare number
/// or group of them that took it, so that it means the same in any
/// design units: `1 in + 3` in millimetres is `1 in + 3 mm`, `40 / 2` is
/// `(40 / 2) mm`. Degrees, which bare numbers take in angles whatever the
/// design, aren't written. Refused as [`ErrorKind::TooLong`] where the
/// units take the text over [`MAX_LEN`]; the caller can show the value
/// with [`format()`](crate::format()) instead.
pub fn pin_units(text: &str, ask: &Ask) -> Result<String, Error> {
    let expr = parse(text)?;
    let mut evaluator = Evaluator::new(ask);
    evaluator.result(&expr)?;
    let mut pinned = text.to_owned();
    // Pinned parts don't overlap, so inserting from the last keeps the
    // earlier ones' spans.
    evaluator
        .pins
        .sort_by_key(|pin| std::cmp::Reverse(pin.span.start));
    for pin in &evaluator.pins {
        pinned.insert_str(pin.span.end, &format!(" {}", ask.units.symbol()));
        if pin.wrap {
            pinned.insert(pin.span.end, ')');
            pinned.insert(pin.span.start, '(');
        }
    }
    if pinned.len() > MAX_LEN {
        return Err(Error {
            kind: ErrorKind::TooLong,
            span: Span::new(0, text.len()),
        });
    }
    Ok(pinned)
}

/// Powers of length and angle: a length is `{ length: 1, angle: 0 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dim {
    length: i16,
    angle: i16,
}

impl Dim {
    const NONE: Dim = Dim {
        length: 0,
        angle: 0,
    };
    const LENGTH: Dim = Dim {
        length: 1,
        angle: 0,
    };
    const ANGLE: Dim = Dim {
        length: 0,
        angle: 1,
    };

    fn of(unit: Unit) -> Dim {
        match unit {
            Unit::Length(_) => Dim::LENGTH,
            Unit::Angle(_) => Dim::ANGLE,
        }
    }

    fn kind(self) -> Kind {
        match self {
            Dim::NONE => Kind::Number,
            Dim::LENGTH => Kind::Length,
            Dim::ANGLE => Kind::Angle,
            Dim {
                length: 2,
                angle: 0,
            } => Kind::Area,
            _ => Kind::Mixed,
        }
    }

    /// The powers of a product (`sign` 1) or quotient (`sign` -1).
    fn combine(self, other: Dim, sign: i16) -> Option<Dim> {
        Some(Dim {
            length: self.length.checked_add(other.length.checked_mul(sign)?)?,
            angle: self.angle.checked_add(other.angle.checked_mul(sign)?)?,
        })
    }
}

/// A value on the way: `bare` if no unit went into it, so it may still
/// take the design's.
#[derive(Debug, Clone, Copy)]
struct Quant {
    value: f64,
    dim: Dim,
    bare: bool,
}

/// A bare part that took the design's length unit, and whether it needs
/// brackets for a unit after it.
struct Pin {
    span: Span,
    wrap: bool,
}

struct Evaluator<'a> {
    ask: &'a Ask,
    pins: Vec<Pin>,
}

impl<'a> Evaluator<'a> {
    fn new(ask: &'a Ask) -> Self {
        Evaluator {
            ask,
            pins: Vec::new(),
        }
    }

    /// The whole expression's value, as asked for.
    fn result(&mut self, expr: &Expr) -> Result<f64, Error> {
        let ask = self.ask;
        let quant = self.eval(expr)?;
        let want = match ask.quantity {
            Quantity::Length => Dim::LENGTH,
            Quantity::Angle => Dim::ANGLE,
            Quantity::Number => Dim::NONE,
        };
        let quant = if quant.dim == want {
            quant
        } else if quant.bare {
            self.adopt(quant, want, expr)?
        } else {
            return Err(Error {
                kind: ErrorKind::Wrong {
                    want: ask.quantity,
                    got: quant.dim.kind(),
                },
                span: expr.span,
            });
        };
        let value = quant.value;
        // Not `!(abs <= max)`, so a NaN `max` refuses everything.
        let over = if ask.under {
            value.abs() >= ask.max
        } else {
            value.abs() > ask.max
        };
        if over || ask.max.is_nan() {
            return Err(Error {
                kind: ErrorKind::TooLarge {
                    max: ask.max,
                    unit: ask.unit(),
                    under: ask.under,
                },
                span: expr.span,
            });
        }
        if ask.positive && value <= 0.0 {
            return Err(Error {
                kind: ErrorKind::NotPositive,
                span: expr.span,
            });
        }
        // Not `value < min`, so a NaN `min` refuses everything.
        if let Some(min) = ask.min
            && value.partial_cmp(&min).is_none_or(|order| order.is_lt())
        {
            // A least length shows in millimetres: in inches, say, a
            // micrometre shows as zero.
            let unit = match ask.unit() {
                Some(Unit::Length(_)) => Some(LengthUnit::Mm.into()),
                unit => unit,
            };
            return Err(Error {
                kind: ErrorKind::TooSmall { min, unit },
                span: expr.span,
            });
        }
        if ask.whole && value.fract() != 0.0 {
            return Err(Error {
                kind: ErrorKind::NotWhole,
                span: expr.span,
            });
        }
        Ok(value)
    }

    fn eval(&mut self, expr: &Expr) -> Result<Quant, Error> {
        let span = expr.span;
        match &expr.node {
            &Node::Number(value) => Ok(Quant {
                value,
                dim: Dim::NONE,
                bare: true,
            }),
            Node::Group(inner) => self.eval(inner),
            &Node::Unit(ref inner, unit) => {
                let quant = self.eval(inner)?;
                if !quant.bare {
                    return Err(Error {
                        kind: ErrorKind::UnitOnUnit,
                        span,
                    });
                }
                Ok(Quant {
                    value: finite(quant.value * unit.factor(), span)?,
                    dim: Dim::of(unit),
                    bare: false,
                })
            }
            Node::Neg(inner) => {
                let quant = self.eval(inner)?;
                Ok(Quant {
                    value: -quant.value,
                    ..quant
                })
            }
            &Node::Binary(op, ref left_expr, ref right_expr) => {
                let mut left = self.eval(left_expr)?;
                let mut right = self.eval(right_expr)?;
                let dim = match op {
                    Op::Add | Op::Sub => {
                        if left.dim != right.dim {
                            if left.bare && adoptable(right.dim) {
                                left = self.adopt(left, right.dim, left_expr)?;
                            } else if right.bare && adoptable(left.dim) {
                                right = self.adopt(right, left.dim, right_expr)?;
                            } else {
                                return Err(Error {
                                    kind: ErrorKind::Mismatch {
                                        add: op == Op::Add,
                                        left: left.dim.kind(),
                                        right: right.dim.kind(),
                                    },
                                    span,
                                });
                            }
                        }
                        Some(left.dim)
                    }
                    Op::Mul => left.dim.combine(right.dim, 1),
                    Op::Div => {
                        if right.value == 0.0 {
                            return Err(Error {
                                kind: ErrorKind::DivideByZero,
                                span: right_expr.span,
                            });
                        }
                        left.dim.combine(right.dim, -1)
                    }
                };
                let dim = dim.ok_or(Error {
                    kind: ErrorKind::Overflow,
                    span,
                })?;
                let value = match op {
                    Op::Add => left.value + right.value,
                    Op::Sub => left.value - right.value,
                    Op::Mul => left.value * right.value,
                    Op::Div => left.value / right.value,
                };
                Ok(Quant {
                    value: finite(value, span)?,
                    dim,
                    bare: left.bare && right.bare,
                })
            }
        }
    }

    /// The bare `quant`, from `expr`, in the default unit for `dim`: the
    /// design's length unit, pinned, or degrees.
    fn adopt(&mut self, quant: Quant, dim: Dim, expr: &Expr) -> Result<Quant, Error> {
        let factor = if dim == Dim::LENGTH {
            self.pins.push(Pin {
                span: expr.span,
                wrap: needs_brackets(expr),
            });
            self.ask.units.mm()
        } else {
            AngleUnit::Deg.rad()
        };
        Ok(Quant {
            value: finite(quant.value * factor, expr.span)?,
            dim,
            bare: false,
        })
    }
}

/// Whether a bare value can take a default unit to become one of `dim`.
fn adoptable(dim: Dim) -> bool {
    dim == Dim::LENGTH || dim == Dim::ANGLE
}

/// Whether a unit after `expr` needs brackets around it to apply to all
/// of it: a unit binds tighter than anything but a number or a group, and
/// `-x mm` is `-(x mm)`, the same value.
fn needs_brackets(expr: &Expr) -> bool {
    match &expr.node {
        Node::Number(_) | Node::Group(_) => false,
        Node::Neg(inner) => needs_brackets(inner),
        Node::Unit(..) | Node::Binary(..) => true,
    }
}

fn finite(value: f64, span: Span) -> Result<f64, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error {
            kind: ErrorKind::Overflow,
            span,
        })
    }
}

#[cfg(test)]
mod tests;

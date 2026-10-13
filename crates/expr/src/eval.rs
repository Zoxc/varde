//! Evaluating the tree: every step checked finite and for units, bare
//! numbers given the design's length unit or degrees where they meet a
//! length or an angle, parameters looked up, the result checked against
//! the [`Ask`] (or, for a parameter's own text, taken as whatever
//! quantity it comes to).

use crate::parse::{Expr, Node, Op, parse};
use crate::{
    AngleUnit, Ask, Error, ErrorKind, Kind, LengthUnit, MAX_LEN, Params, Quantity, Resolved, Span,
    Unit,
};

/// Evaluates `text` for `ask`, in model units: millimetres for lengths,
/// radians for angles.
pub fn evaluate(text: &str, ask: &Ask) -> Result<f64, Error> {
    let expr = parse(text)?;
    Evaluator::new(ask.units, ask.params).result(&expr, ask)
}

/// Evaluates a parameter's `text` in a design in `units`, with the
/// parameters it may use: a length, an angle or a number, whichever it
/// comes to. Bare numbers alone are a number, so `50` is a number, not a
/// length; `50 mm` is a length. An unknown name looks for a near one to
/// suggest if `suggest`: not while [`Params::evaluate`] runs, which
/// suggests within a budget after.
pub(crate) fn evaluate_param(
    text: &str,
    units: LengthUnit,
    params: &Params,
    suggest: bool,
) -> Result<Resolved, Error> {
    let expr = parse(text)?;
    let mut evaluator = Evaluator::new(units, params);
    evaluator.suggest = suggest;
    evaluator.free(&expr)
}

/// [`pin_units`] for a parameter's `text`, as [`Params::pin_units`]
/// gives it, with the value the pins came from.
pub(crate) fn pin_param_units(
    text: &str,
    units: LengthUnit,
    params: &Params,
) -> Result<(String, Resolved), Error> {
    let expr = parse(text)?;
    let mut evaluator = Evaluator::new(units, params);
    let before = evaluator.free(&expr)?;
    Ok((evaluator.pinned(text)?, before))
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
    let mut evaluator = Evaluator::new(ask.units, ask.params);
    evaluator.result(&expr, ask)?;
    evaluator.pinned(text)
}

/// Powers of length and angle: a length is `{ length: 1, angle: 0 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dim {
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

    fn of_quantity(quantity: Quantity) -> Dim {
        match quantity {
            Quantity::Length => Dim::LENGTH,
            Quantity::Angle => Dim::ANGLE,
            Quantity::Number => Dim::NONE,
        }
    }

    /// The quantity of these powers, if they're one's.
    fn quantity(self) -> Option<Quantity> {
        match self {
            Dim::NONE => Some(Quantity::Number),
            Dim::LENGTH => Some(Quantity::Length),
            Dim::ANGLE => Some(Quantity::Angle),
            _ => None,
        }
    }

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
/// take the design's. A parameter's value isn't bare, whatever it is: its
/// units were settled where it was defined.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Quant {
    value: f64,
    dim: Dim,
    bare: bool,
}

impl From<Resolved> for Quant {
    fn from(resolved: Resolved) -> Quant {
        Quant {
            value: resolved.value,
            dim: Dim::of_quantity(resolved.quantity),
            bare: false,
        }
    }
}

/// A bare part that took the design's length unit, and whether it needs
/// brackets for a unit after it.
struct Pin {
    span: Span,
    wrap: bool,
}

struct Evaluator<'a> {
    /// The design's length unit, which bare numbers take.
    units: LengthUnit,
    params: &'a Params,
    /// Whether an unknown name looks for a near one to suggest.
    suggest: bool,
    pins: Vec<Pin>,
}

impl<'a> Evaluator<'a> {
    fn new(units: LengthUnit, params: &'a Params) -> Self {
        Evaluator {
            units,
            params,
            suggest: true,
            pins: Vec::new(),
        }
    }

    /// The text evaluated, with the design's length unit written in where
    /// bare parts took it, see [`pin_units`].
    fn pinned(mut self, text: &str) -> Result<String, Error> {
        let mut pinned = text.to_owned();
        // Pinned parts don't overlap, so inserting from the last keeps the
        // earlier ones' spans.
        self.pins
            .sort_by_key(|pin| std::cmp::Reverse(pin.span.start));
        for pin in &self.pins {
            pinned.insert_str(pin.span.end, &format!(" {}", self.units.symbol()));
            if pin.wrap {
                pinned.insert(pin.span.end, ')');
                pinned.insert(pin.span.start, '(');
            }
        }
        if pinned.len() > MAX_LEN {
            return Err(ErrorKind::TooLong.over(text));
        }
        Ok(pinned)
    }

    /// A parameter's value: whatever quantity it comes to, a number if
    /// it's bare, but not an area or a mix of units.
    fn free(&mut self, expr: &Expr) -> Result<Resolved, Error> {
        let quant = self.eval(expr)?;
        let Some(quantity) = quant.dim.quantity() else {
            return Err(ErrorKind::ParamKind(quant.dim.kind()).at(expr.span));
        };
        Ok(Resolved {
            value: quant.value,
            quantity,
        })
    }

    /// The whole expression's value, as asked for.
    fn result(&mut self, expr: &Expr, ask: &Ask) -> Result<f64, Error> {
        let quant = self.eval(expr)?;
        let want = Dim::of_quantity(ask.quantity);
        let quant = if quant.dim == want {
            quant
        } else if quant.bare {
            self.adopt(quant, want, expr)?
        } else {
            return Err(ErrorKind::Wrong {
                want: ask.quantity,
                got: quant.dim.kind(),
            }
            .at(expr.span));
        };
        let value = quant.value;
        // Not `!(abs <= max)`, so a NaN `max` refuses everything.
        let over = if ask.under {
            value.abs() >= ask.max
        } else {
            value.abs() > ask.max
        };
        if over || ask.max.is_nan() {
            return Err(ErrorKind::TooLarge {
                max: ask.max,
                unit: ask.unit(),
                under: ask.under,
            }
            .at(expr.span));
        }
        if ask.positive && value <= 0.0 {
            return Err(ErrorKind::NotPositive.at(expr.span));
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
            return Err(ErrorKind::TooSmall { min, unit }.at(expr.span));
        }
        if ask.whole && value.fract() != 0.0 {
            return Err(ErrorKind::NotWhole.at(expr.span));
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
            Node::Name(name) => self.params.quant(name, span, self.suggest),
            Node::Group(inner) => self.eval(inner),
            &Node::Unit(ref inner, unit) => {
                let quant = self.eval(inner)?;
                if !quant.bare {
                    let kind = match &inner.node {
                        Node::Name(name) => ErrorKind::UnitOnParam(name.clone()),
                        _ => ErrorKind::UnitOnUnit,
                    };
                    return Err(kind.at(span));
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
                                return Err(ErrorKind::Mismatch {
                                    add: op == Op::Add,
                                    left: left.dim.kind(),
                                    right: right.dim.kind(),
                                }
                                .at(span));
                            }
                        }
                        Some(left.dim)
                    }
                    Op::Mul => left.dim.combine(right.dim, 1),
                    Op::Div => {
                        if right.value == 0.0 {
                            return Err(ErrorKind::DivideByZero.at(right_expr.span));
                        }
                        left.dim.combine(right.dim, -1)
                    }
                };
                let dim = dim.ok_or(ErrorKind::Overflow.at(span))?;
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
            self.units.mm()
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
        Node::Number(_) | Node::Name(_) | Node::Group(_) => false,
        Node::Neg(inner) => needs_brackets(inner),
        Node::Unit(..) | Node::Binary(..) => true,
    }
}

fn finite(value: f64, span: Span) -> Result<f64, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ErrorKind::Overflow.at(span))
    }
}

#[cfg(test)]
mod tests;

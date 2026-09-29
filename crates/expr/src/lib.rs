//! Typed values: what the user types where a length or an angle is asked
//! for, such as `40 / 2`, `1 in + 3 mm` or `90 deg - 15`, parsed,
//! checked for units and evaluated to model units (millimetres, radians).
//!
//! [`evaluate`] is the whole of it for one value; [`Value`] keeps the text
//! as typed with its value, as a dimension stores it, and re-checks the
//! two agree ([`Value::check`]); [`pin_units`] writes the design's unit into
//! the text where a bare number took it, for when the design's units
//! change; [`format()`] shows a value back in a unit.
//!
//! The input is text from a form or a file, so everything is bounded: at
//! most [`MAX_LEN`] bytes and [`MAX_DEPTH`] nested brackets and signs,
//! every step's result is checked finite, and the result is checked
//! against the caller's [`Ask`].

mod eval;
mod parse;

use std::f64::consts::PI;
use std::fmt;

pub use eval::{evaluate, pin_units};

/// The most bytes of text an expression may have.
pub const MAX_LEN: usize = 256;

/// The most brackets and signs (`-`, `+` before a value) an expression may
/// nest, so that parsing it can't overflow the stack.
pub const MAX_DEPTH: usize = 32;

/// A unit of length, as a design's units or typed after a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 5] = [
        LengthUnit::Mm,
        LengthUnit::Cm,
        LengthUnit::M,
        LengthUnit::In,
        LengthUnit::Ft,
    ];

    /// Millimetres in one of it.
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::In => 25.4,
            LengthUnit::Ft => 304.8,
        }
    }

    /// As typed and shown: "mm", "in".
    pub fn symbol(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
            LengthUnit::In => "in",
            LengthUnit::Ft => "ft",
        }
    }

    /// Decimals [`format()`] shows: a few micrometres at most.
    fn decimals(self) -> usize {
        match self {
            LengthUnit::Mm => 3,
            LengthUnit::Cm => 4,
            LengthUnit::M => 6,
            LengthUnit::In => 4,
            LengthUnit::Ft => 5,
        }
    }
}

/// A unit of angle, typed after a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AngleUnit {
    #[default]
    Deg,
    Rad,
}

impl AngleUnit {
    /// Radians in one of it.
    pub fn rad(self) -> f64 {
        match self {
            AngleUnit::Deg => PI / 180.0,
            AngleUnit::Rad => 1.0,
        }
    }

    /// As typed and shown: "deg", "rad". [`format()`] shows degrees as "°".
    pub fn symbol(self) -> &'static str {
        match self {
            AngleUnit::Deg => "deg",
            AngleUnit::Rad => "rad",
        }
    }

    fn decimals(self) -> usize {
        match self {
            AngleUnit::Deg => 3,
            AngleUnit::Rad => 5,
        }
    }
}

/// Any unit an expression knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Length(LengthUnit),
    Angle(AngleUnit),
}

impl Unit {
    /// Model units (millimetres or radians) in one of it.
    pub fn factor(self) -> f64 {
        match self {
            Unit::Length(unit) => unit.mm(),
            Unit::Angle(unit) => unit.rad(),
        }
    }

    pub fn quantity(self) -> Quantity {
        match self {
            Unit::Length(_) => Quantity::Length,
            Unit::Angle(_) => Quantity::Angle,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Unit::Length(unit) => unit.symbol(),
            Unit::Angle(unit) => unit.symbol(),
        }
    }
}

impl From<LengthUnit> for Unit {
    fn from(unit: LengthUnit) -> Unit {
        Unit::Length(unit)
    }
}

impl From<AngleUnit> for Unit {
    fn from(unit: AngleUnit) -> Unit {
        Unit::Angle(unit)
    }
}

/// The kind of value asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quantity {
    Length,
    Angle,
    /// A plain number, such as a count or a ratio.
    Number,
}

impl Quantity {
    /// "a length", for messages.
    pub fn name(self) -> &'static str {
        match self {
            Quantity::Length => "a length",
            Quantity::Angle => "an angle",
            Quantity::Number => "a number",
        }
    }

    /// The unit it's shown in, in a design in `units`: the design's for
    /// lengths, degrees for angles, none for numbers.
    pub fn unit(self, units: LengthUnit) -> Option<Unit> {
        match self {
            Quantity::Length => Some(units.into()),
            Quantity::Angle => Some(AngleUnit::Deg.into()),
            Quantity::Number => None,
        }
    }
}

/// What the caller asks an expression for, and how bare numbers are read.
///
/// A bare number (or a sum or product of only bare numbers) takes the
/// design's length unit `units` where it's added to a length or a length
/// is asked for, and degrees where it's added to an angle or an angle is
/// asked for: `40 / 2` asked as a length is 20 of the design's units, `90
/// deg - 15` is 75°. Anything else must come out as the quantity asked
/// for: `2 mm * 3 mm` is an area, refused where a length is asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ask {
    pub quantity: Quantity,
    /// The design's length unit.
    pub units: LengthUnit,
    /// The largest the value may be either side of zero, in model units:
    /// the size of the design for lengths, a turn for angles.
    pub max: f64,
    /// Whether the value must be above zero.
    pub positive: bool,
    /// The least the value may be, if there's a least, in model units.
    pub min: Option<f64>,
    /// Whether the value must be under `max`, rather than at most `max`.
    pub under: bool,
    /// Whether the value must be a whole number, such as a count.
    pub whole: bool,
}

impl Ask {
    /// A length within `max` millimetres of zero, bare numbers in `units`.
    pub fn length(units: LengthUnit, max: f64) -> Ask {
        Ask {
            quantity: Quantity::Length,
            units,
            max,
            positive: false,
            min: None,
            under: false,
            whole: false,
        }
    }

    /// An angle within `max` radians of zero, bare numbers in degrees
    /// (and in `units` where added to a length inside it).
    pub fn angle(units: LengthUnit, max: f64) -> Ask {
        Ask {
            quantity: Quantity::Angle,
            ..Ask::length(units, max)
        }
    }

    /// A plain number within `max` of zero.
    pub fn number(units: LengthUnit, max: f64) -> Ask {
        Ask {
            quantity: Quantity::Number,
            ..Ask::length(units, max)
        }
    }

    /// The same, but the value must be above zero.
    pub fn positive(self) -> Ask {
        Ask {
            positive: true,
            ..self
        }
    }

    /// The same, but the value must be at least `min`, in model units.
    pub fn at_least(self, min: f64) -> Ask {
        Ask {
            min: Some(min),
            ..self
        }
    }

    /// The same, but the value must be under `max` rather than at most
    /// it: an angle short of a turn.
    pub fn under_max(self) -> Ask {
        Ask {
            under: true,
            ..self
        }
    }

    /// The same, but the value must be a whole number: a count.
    pub fn whole(self) -> Ask {
        Ask {
            whole: true,
            ..self
        }
    }

    /// The unit results are shown in, see [`Quantity::unit`].
    pub fn unit(&self) -> Option<Unit> {
        self.quantity.unit(self.units)
    }
}

/// A typed value as it's kept: the text as typed, and what it came to in
/// model units. From a file, both are untrusted until [`Value::check`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Value {
    pub text: String,
    pub value: f64,
}

impl Value {
    /// Evaluates `text` for `ask`, keeping it without the whitespace
    /// around it.
    pub fn new(text: &str, ask: &Ask) -> Result<Value, Error> {
        let value = evaluate(text, ask)?;
        Ok(Value {
            text: text.trim().to_owned(),
            value,
        })
    }

    /// Re-evaluates the text for `ask` and checks it comes to the stored
    /// value exactly: evaluation is deterministic, so a value that
    /// differs was changed, or the text was read with other units.
    pub fn check(&self, ask: &Ask) -> Result<(), Error> {
        let value = evaluate(&self.text, ask)?;
        if value == self.value {
            Ok(())
        } else {
            Err(Error {
                kind: ErrorKind::Disagrees {
                    stored: self.value,
                    evaluated: value,
                },
                span: Span::new(0, self.text.len()),
            })
        }
    }
}

/// `value`, in model units, shown in `unit` with its symbol, to a few
/// micrometres (or a thousandth of a degree), without trailing zeros:
/// `12.5 mm`, `0.5 in`, `90°`. Without a unit, a number to six decimals.
pub fn format(value: f64, unit: Option<Unit>) -> String {
    let Some(unit) = unit else {
        return number(value, None);
    };
    let text = number(value / unit.factor(), Some(unit));
    match unit {
        Unit::Angle(AngleUnit::Deg) => format!("{text}°"),
        unit => format!("{text} {}", unit.symbol()),
    }
}

/// `value`, already in `unit`, to the unit's decimals.
fn number(value: f64, unit: Option<Unit>) -> String {
    let decimals = match unit {
        Some(Unit::Length(unit)) => unit.decimals(),
        Some(Unit::Angle(unit)) => unit.decimals(),
        None => 6,
    };
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        let kept = text.trim_end_matches('0').trim_end_matches('.').len();
        text.truncate(kept);
    }
    if text == "-0" {
        text.remove(0);
    }
    text
}

/// A byte range of an expression's text, on character boundaries, for
/// highlighting what an [`Error`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    pub fn range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// From the start of `self` to the end of `other`.
    fn to(self, other: Span) -> Span {
        Span::new(self.start, other.end)
    }
}

/// Why an expression was refused, and the part of its text it's about.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl std::error::Error for Error {}

/// Why an expression was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    /// Nothing but whitespace.
    Empty,
    /// Over [`MAX_LEN`] bytes.
    TooLong,
    /// Brackets or signs nested over [`MAX_DEPTH`] deep.
    TooDeep,
    /// Something that isn't part of an expression, or not where it is.
    Unexpected(String),
    /// A comma, as a decimal point in some locales.
    Comma,
    /// A word that isn't a unit.
    UnknownUnit(String),
    /// The end, or an operator or `)`, where a number was expected.
    ExpectedNumber,
    /// A `(` without its `)`.
    Unclosed,
    /// A number too large to hold.
    BadNumber,
    /// A unit after something that already has units: `(1 in) mm`.
    UnitOnUnit,
    /// Adding or subtracting values of different kinds.
    Mismatch {
        add: bool,
        left: Kind,
        right: Kind,
    },
    /// A step's result is too large to hold.
    Overflow,
    DivideByZero,
    /// The result isn't the quantity asked for.
    Wrong {
        want: Quantity,
        got: Kind,
    },
    /// Farther from zero than `max` (model units), shown in `unit`, or
    /// not `under` it where it must be.
    TooLarge {
        max: f64,
        unit: Option<Unit>,
        under: bool,
    },
    /// Zero or below where it must be above zero.
    NotPositive,
    /// Less than `min` (model units), shown in `unit`: a length in
    /// millimetres, whatever the design's units, as coarser ones may show
    /// it as zero.
    TooSmall {
        min: f64,
        unit: Option<Unit>,
    },
    /// Not a whole number where a count is asked for.
    NotWhole,
    /// A stored [`Value`] whose text evaluates to something else.
    Disagrees {
        stored: f64,
        evaluated: f64,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Empty => write!(f, "enter a value"),
            ErrorKind::TooLong => write!(f, "too long, at most {MAX_LEN} characters"),
            ErrorKind::TooDeep => write!(f, "nested too deeply"),
            ErrorKind::Unexpected(what) => write!(f, "unexpected '{what}'"),
            ErrorKind::Comma => write!(f, "use '.' for decimals"),
            ErrorKind::UnknownUnit(word) => write!(f, "unknown unit '{word}'"),
            ErrorKind::ExpectedNumber => write!(f, "expected a number"),
            ErrorKind::Unclosed => write!(f, "missing ')'"),
            ErrorKind::BadNumber => write!(f, "number too large"),
            ErrorKind::UnitOnUnit => write!(f, "that already has a unit"),
            ErrorKind::Mismatch { add, left, right } => {
                let verb = if *add { "add" } else { "subtract" };
                if left == right {
                    // Only a mixed kind or an area on both sides can get
                    // here: equal kinds that differ in powers.
                    write!(f, "can't {verb} values of different units")
                } else {
                    write!(f, "can't {verb} {} and {}", left.name(), right.name())
                }
            }
            ErrorKind::Overflow => write!(f, "too large"),
            ErrorKind::DivideByZero => write!(f, "division by zero"),
            ErrorKind::Wrong { want, got } => {
                write!(f, "that's {}, not {}", got.name(), want.name())
            }
            ErrorKind::TooLarge { max, unit, under } => {
                let bound = if *under { "under" } else { "at most" };
                write!(f, "must be {bound} {}", format(*max, *unit))
            }
            ErrorKind::NotPositive => write!(f, "must be above zero"),
            ErrorKind::NotWhole => write!(f, "must be a whole number"),
            ErrorKind::TooSmall { min, unit } => {
                write!(f, "must be at least {}", format(*min, *unit))
            }
            ErrorKind::Disagrees { stored, evaluated } => write!(
                f,
                "the stored value {stored} doesn't match its expression, {evaluated}"
            ),
        }
    }
}

/// The kind of a value inside an expression, for messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Number,
    Length,
    Angle,
    /// A length times a length.
    Area,
    /// Any other mix of units, such as a length times an angle.
    Mixed,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Number => "a number",
            Kind::Length => "a length",
            Kind::Angle => "an angle",
            Kind::Area => "an area",
            Kind::Mixed => "a mix of units",
        }
    }
}

#[cfg(test)]
mod tests;

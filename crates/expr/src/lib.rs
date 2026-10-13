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
//! Values may use a design's parameters by name, `height / 2`
//! ([`Params`], looked up through the [`Ask`]); [`names`], [`uses`] and
//! [`rename`] find and rewrite the names in a text.
//!
//! The input is text from a form or a file, so everything is bounded: at
//! most [`MAX_LEN`] bytes and [`MAX_DEPTH`] nested brackets and signs,
//! at most [`MAX_PARAMS`] parameters, resolved without recursion,
//! every step's result is checked finite, and the result is checked
//! against the caller's [`Ask`].

mod eval;
mod format;
mod params;
mod parse;

use std::f64::consts::PI;
use std::fmt;

pub use eval::{evaluate, pin_units};
pub use format::{
    Power, Shown, exact, format, format_number, format_power, full, full_number, full_power,
};
pub use params::{
    MAX_NAME_LEN, MAX_PARAMS, NameError, Params, Resolved, check_name, names, rename, uses,
};

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
    pub fn decimals(self) -> usize {
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
    pub const ALL: [AngleUnit; 2] = [AngleUnit::Deg, AngleUnit::Rad];

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
///
/// Names are the design's parameters, `params`: none unless
/// [`Ask::with_params`] gives them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ask<'a> {
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
    /// The design's parameters, which names in the text are.
    pub params: &'a Params,
}

impl Ask<'static> {
    /// A length within `max` millimetres of zero, bare numbers in `units`.
    pub fn length(units: LengthUnit, max: f64) -> Ask<'static> {
        Ask {
            quantity: Quantity::Length,
            units,
            max,
            positive: false,
            min: None,
            under: false,
            whole: false,
            params: Params::EMPTY,
        }
    }

    /// An angle within `max` radians of zero, bare numbers in degrees
    /// (and in `units` where added to a length inside it).
    pub fn angle(units: LengthUnit, max: f64) -> Ask<'static> {
        Ask {
            quantity: Quantity::Angle,
            ..Ask::length(units, max)
        }
    }

    /// A plain number within `max` of zero.
    pub fn number(units: LengthUnit, max: f64) -> Ask<'static> {
        Ask {
            quantity: Quantity::Number,
            ..Ask::length(units, max)
        }
    }

    /// A factor: a plain number from `1 / max` to `max` (`max` at least
    /// 1), such as a scale's. It has no unit, but a bare number added to
    /// a length inside it takes `units`, as any number's does.
    pub fn factor(units: LengthUnit, max: f64) -> Ask<'static> {
        Ask::number(units, max).positive().at_least(1.0 / max)
    }
}

impl<'a> Ask<'a> {
    /// The same, with names in the text the parameters `params`.
    pub fn with_params<'b>(self, params: &'b Params) -> Ask<'b> {
        Ask {
            quantity: self.quantity,
            units: self.units,
            max: self.max,
            positive: self.positive,
            min: self.min,
            under: self.under,
            whole: self.whole,
            params,
        }
    }

    /// The same, but the value must be above zero.
    pub fn positive(self) -> Ask<'a> {
        Ask {
            positive: true,
            ..self
        }
    }

    /// The same, but the value must be at least `min`, in model units.
    pub fn at_least(self, min: f64) -> Ask<'a> {
        Ask {
            min: Some(min),
            ..self
        }
    }

    /// The same, but the value must be under `max` rather than at most
    /// it: an angle short of a turn.
    pub fn under_max(self) -> Ask<'a> {
        Ask {
            under: true,
            ..self
        }
    }

    /// The same, but the value must be a whole number: a count.
    pub fn whole(self) -> Ask<'a> {
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
    pub fn new(text: &str, ask: &Ask<'_>) -> Result<Value, Error> {
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

    /// Writes `ask`'s length unit into the text after each bare number
    /// that took it ([`pin_units`]), so it means the same in any units:
    /// for changing the design's units, `ask` being for the units before.
    /// The value stays exactly as it was. A text the units would take past
    /// [`MAX_LEN`], or that doesn't give the value, is replaced by the
    /// value, exactly ([`exact`]).
    pub fn pin_units(&mut self, ask: &Ask) {
        let pinned = pin_units(&self.text, ask)
            .ok()
            .filter(|text| evaluate(text, ask) == Ok(self.value));
        self.text = pinned.unwrap_or_else(|| exact(self.value, ask.quantity));
    }
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
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{kind}")]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

/// Why an expression was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ErrorKind {
    /// Nothing but whitespace.
    #[error("enter a value")]
    Empty,
    /// Over [`MAX_LEN`] bytes.
    #[error("too long, at most {MAX_LEN} characters")]
    TooLong,
    /// Brackets or signs nested over [`MAX_DEPTH`] deep.
    #[error("nested too deeply")]
    TooDeep,
    /// Something that isn't part of an expression, or not where it is.
    #[error("unexpected '{0}'")]
    Unexpected(String),
    /// A comma, as a decimal point in some locales.
    #[error("use '.' for decimals")]
    Comma,
    /// A word that isn't a unit, after a number or a bracket.
    #[error("unknown unit '{0}'")]
    UnknownUnit(String),
    /// The end, or an operator or `)`, where a number was expected.
    #[error("expected a number")]
    ExpectedNumber,
    /// A `(` without its `)`.
    #[error("missing ')'")]
    Unclosed,
    /// A number too large to hold.
    #[error("number too large")]
    BadNumber,
    /// A unit after something that already has units: `(1 in) mm`.
    #[error("that already has a unit")]
    UnitOnUnit,
    /// A unit after a parameter's name, `count mm`: a parameter's units
    /// are its own.
    #[error("a parameter takes no unit after it: multiply by one, as '{0} * 1 mm'")]
    UnitOnParam(String),
    /// Adding or subtracting values of different kinds.
    #[error(fmt = mismatch)]
    Mismatch { add: bool, left: Kind, right: Kind },
    /// A step's result is too large to hold.
    #[error("too large")]
    Overflow,
    #[error("division by zero")]
    DivideByZero,
    /// The result isn't the quantity asked for.
    #[error("that's {}, not {}", got.name(), want.name())]
    Wrong { want: Quantity, got: Kind },
    /// Farther from zero than `max` (model units), shown in `unit`, or
    /// not `under` it where it must be.
    #[error("must be {} {}", if *under { "under" } else { "at most" }, format(*max, *unit))]
    TooLarge {
        max: f64,
        unit: Option<Unit>,
        under: bool,
    },
    /// Zero or below where it must be above zero.
    #[error("must be above zero")]
    NotPositive,
    /// Less than `min` (model units), shown in `unit`: a length in
    /// millimetres, whatever the design's units, as coarser ones may show
    /// it as zero.
    #[error("must be at least {}", format(*min, *unit))]
    TooSmall { min: f64, unit: Option<Unit> },
    /// Not a whole number where a count is asked for.
    #[error("must be a whole number")]
    NotWhole,
    /// A stored [`Value`] whose text evaluates to something else.
    #[error("the stored value {stored} doesn't match its expression, {evaluated}")]
    Disagrees { stored: f64, evaluated: f64 },
    /// A name no parameter has, and the one it may be a misspelling of.
    #[error(fmt = unknown_name)]
    UnknownName {
        name: String,
        suggestion: Option<String>,
    },
    /// A use of a parameter that's in error itself.
    #[error("the parameter '{0}' has an error")]
    BrokenParam(String),
    /// A parameter that uses itself, through others or not: the span is
    /// the use that leads round.
    #[error("a parameter can't use itself, even through others")]
    Cycle,
    /// A parameter that comes to an area or a mix of units: a parameter
    /// is a length, an angle or a number.
    #[error("that's {}: a parameter is a length, an angle or a number", .0.name())]
    ParamKind(Kind),
    /// A parameter's name that can't be one.
    #[error("{0}")]
    BadName(NameError),
    /// A parameter named as an earlier one is.
    #[error("there's already a parameter '{0}'")]
    Duplicate(String),
    /// A parameter past [`MAX_PARAMS`].
    #[error("too many parameters, at most {MAX_PARAMS}")]
    TooManyParams,
}

/// [`ErrorKind::Mismatch`]'s message.
fn mismatch(add: &bool, left: &Kind, right: &Kind, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let verb = if *add { "add" } else { "subtract" };
    if left == right {
        // Only a mixed kind or an area on both sides can get here: equal
        // kinds that differ in powers.
        write!(f, "can't {verb} values of different units")
    } else {
        write!(f, "can't {verb} {} and {}", left.name(), right.name())
    }
}

/// [`ErrorKind::UnknownName`]'s message.
fn unknown_name(
    name: &str,
    suggestion: &Option<String>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    write!(f, "unknown parameter '{name}'")?;
    match suggestion {
        Some(suggestion) => write!(f, ", did you mean '{suggestion}'?"),
        None => Ok(()),
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

//! Showing values: [`format()`] in a unit to its decimals, [`full`] to
//! full precision, [`exact`] as text that evaluates back to the value,
//! and [`Shown`], the user's display settings.

use crate::{AngleUnit, LengthUnit, Quantity, Unit};

/// `value`, in model units, as a text that evaluates to it exactly in any
/// design units: its shortest exact decimal, with the model unit, `mm` or
/// `rad`, if it has one, such as `1.25e1 mm`.
pub fn exact(value: f64, quantity: Quantity) -> String {
    match quantity {
        Quantity::Length => format!("{value:e} mm"),
        Quantity::Angle => format!("{value:e} rad"),
        Quantity::Number => format!("{value:e}"),
    }
}

/// `value`, in model units, shown in `unit` with its symbol, to a few
/// micrometres (or a thousandth of a degree), without trailing zeros:
/// `12.5 mm`, `0.5 in`, `90°`. Without a unit, a number to six decimals.
pub fn format(value: f64, unit: Option<Unit>) -> String {
    let Some(unit) = unit else {
        return number(value, None);
    };
    with_symbol(number(value / unit.factor(), Some(unit)), unit)
}

/// What power of a length a value is: a length, an area or a volume,
/// shown in the length unit's square or cube.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Power {
    Length,
    Area,
    Volume,
}

impl Power {
    /// Model units (millimetres, square or cubic) in one of `unit`'s.
    fn factor(self, unit: LengthUnit) -> f64 {
        let mm = unit.mm();
        match self {
            Power::Length => mm,
            Power::Area => mm * mm,
            Power::Volume => mm * mm * mm,
        }
    }

    /// `unit`'s symbol raised to it: "mm", "mm²", "mm³".
    fn symbol(self, unit: LengthUnit) -> String {
        let power = match self {
            Power::Length => "",
            Power::Area => "²",
            Power::Volume => "³",
        };
        format!("{}{power}", unit.symbol())
    }
}

/// `value`, in model units (millimetres, square or cubic, as `power`
/// says), shown in `unit`'s `power` with its symbol, to the unit's
/// decimals as [`format()`] shows lengths, without trailing zeros:
/// `12.5 mm²`, `0.5 in³`.
pub fn format_power(value: f64, unit: LengthUnit, power: Power) -> String {
    let text = number(value / power.factor(unit), Some(unit.into()));
    format!("{text} {}", power.symbol(unit))
}

/// `value`, in model units, in `unit` with its symbol at full precision:
/// the shortest decimal that reads back as the same `f64` once divided
/// into `unit`, such as `12.700000000000001 mm` or `33.333333333333336°`.
/// What copying a measure gives. Without a unit, the number alone.
pub fn full(value: f64, unit: Option<Unit>) -> String {
    let Some(unit) = unit else {
        return shortest(value);
    };
    with_symbol(shortest(value / unit.factor()), unit)
}

/// [`full`] for a length, an area or a volume, as [`format_power`] shows
/// one: `1.0e2 mm²` as `100 mm²`.
pub fn full_power(value: f64, unit: LengthUnit, power: Power) -> String {
    let text = shortest(value / power.factor(unit));
    format!("{text} {}", power.symbol(unit))
}

/// `value`, in model units, as a number of `unit` without its symbol,
/// as [`format()`] shows it: for the parts of a point or a vector, shown
/// with the symbol once after them all.
pub fn format_number(value: f64, unit: Option<Unit>) -> String {
    number(unit.map_or(value, |unit| value / unit.factor()), unit)
}

/// `value`, in model units, as a number of `unit` without its symbol, as
/// [`full`] gives it.
pub fn full_number(value: f64, unit: Option<Unit>) -> String {
    shortest(unit.map_or(value, |unit| value / unit.factor()))
}

/// How values are shown to the user, as their settings have it: angles
/// in [`Shown::angle`], and lengths and angles to at most
/// [`Shown::decimals`] places, or each unit's own count
/// ([`LengthUnit::decimals`], 3 for degrees, 5 for radians) without one.
///
/// Only for what's read: a label, a readout, a list's note. Text that
/// becomes a value, a field filled in for the user to take or a
/// dimension made where the geometry is, is [`format()`]'s, in the
/// design's units and degrees to each unit's decimals, whatever the
/// settings say, so a setting never changes the model. Bare numbers typed
/// for an angle stay degrees too: a design's texts must evaluate as they
/// did ([`Value::check`](crate::Value::check)).
///
/// The UI thread keeps the settings' in [`Shown::current`]: the
/// view's functions show values in them without each being handed them.
/// Any other thread has the defaults, which show as [`format()`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Shown {
    pub angle: AngleUnit,
    /// The most decimal places shown, at most [`Shown::MAX_DECIMALS`]
    /// (more are taken as that many), or `None` for each unit's own.
    pub decimals: Option<u8>,
}

thread_local! {
    static SHOWN: std::cell::Cell<Shown> = const {
        std::cell::Cell::new(Shown {
            angle: AngleUnit::Deg,
            decimals: None,
        })
    };
}

impl Shown {
    /// The most decimal places a setting may show.
    pub const MAX_DECIMALS: u8 = 6;

    /// How this thread shows values: as last [`Shown::set`], or the
    /// defaults.
    pub fn current() -> Shown {
        SHOWN.with(std::cell::Cell::get)
    }

    /// Has this thread show values as `self` says from now on.
    pub fn set(self) {
        SHOWN.with(|shown| shown.set(self));
    }

    /// The unit a value of `quantity` is shown in, in a design in
    /// `units`: as [`Quantity::unit`], but angles in [`Shown::angle`].
    pub fn unit(self, quantity: Quantity, units: LengthUnit) -> Option<Unit> {
        match quantity {
            Quantity::Angle => Some(self.angle.into()),
            quantity => quantity.unit(units),
        }
    }

    /// `unit` as it's shown: an angle in [`Shown::angle`].
    fn shown_unit(self, unit: Unit) -> Unit {
        match unit {
            Unit::Angle(_) => Unit::Angle(self.angle),
            unit => unit,
        }
    }

    /// The decimals `unit` is shown with.
    fn places_for(self, unit: Option<Unit>) -> usize {
        match (unit, self.decimals) {
            (Some(_), Some(places)) => places.min(Self::MAX_DECIMALS).into(),
            (unit, _) => decimals(unit),
        }
    }

    /// `value`, in model units, as [`format()`] shows it, but in these
    /// settings: an angle in [`Shown::angle`], to [`Shown::decimals`].
    pub fn format(self, value: f64, unit: Option<Unit>) -> String {
        let Some(unit) = unit.map(|unit| self.shown_unit(unit)) else {
            return number(value, None);
        };
        let text = number_to(value / unit.factor(), self.places_for(Some(unit)));
        with_symbol(text, unit)
    }

    /// [`format_number`], in these settings.
    pub fn format_number(self, value: f64, unit: Option<Unit>) -> String {
        match unit.map(|unit| self.shown_unit(unit)) {
            Some(unit) => number_to(value / unit.factor(), self.places_for(Some(unit))),
            None => number(value, None),
        }
    }

    /// [`format_power`], in these settings.
    pub fn format_power(self, value: f64, unit: LengthUnit, power: Power) -> String {
        let decimals = self.places_for(Some(unit.into()));
        let text = number_to(value / power.factor(unit), decimals);
        format!("{text} {}", power.symbol(unit))
    }

    /// [`full`], an angle in [`Shown::angle`].
    pub fn full(self, value: f64, unit: Option<Unit>) -> String {
        full(value, unit.map(|unit| self.shown_unit(unit)))
    }
}

/// `text`, a number of `unit`, with its symbol: `12.5 mm`, `90°`.
fn with_symbol(text: String, unit: Unit) -> String {
    match unit {
        Unit::Angle(AngleUnit::Deg) => format!("{text}°"),
        unit => format!("{text} {}", unit.symbol()),
    }
}

/// `value`'s shortest decimal that reads back as it, never `-0`.
fn shortest(value: f64) -> String {
    // `Display` gives the shortest round-tripping digits.
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{value}")
}

/// The decimals [`format()`] shows `unit` with: the unit's, 6 without
/// one.
fn decimals(unit: Option<Unit>) -> usize {
    match unit {
        Some(Unit::Length(unit)) => unit.decimals(),
        Some(Unit::Angle(unit)) => unit.decimals(),
        None => 6,
    }
}

/// `value`, already in `unit`, to the unit's decimals.
fn number(value: f64, unit: Option<Unit>) -> String {
    number_to(value, decimals(unit))
}

/// `value` to `decimals`, without trailing zeros, never `-0`.
fn number_to(value: f64, decimals: usize) -> String {
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

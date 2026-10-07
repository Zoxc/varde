//! The pattern feature: bodies already made repeated along a line or
//! about an axis, as a step of the history. The copies stay in the body
//! they're copies of ("Join to original", the default), or each is a new
//! body of its own.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use varde_expr::{Ask, Value};

use crate::motion::{Referred, check_bodies};
use crate::revolve::TURN_ROUNDING;
use crate::{AxisRef, BodyId, Design, MAX_COORD, MotionError};

/// The most copies one pattern makes of each body, the original among
/// them. Every copy is as many patches as the body, so the regeneration
/// also bounds the count times the body's patches.
pub const MAX_PATTERN_COUNT: u32 = 1024;

/// The most new bodies one pattern whose copies aren't joined to their
/// originals makes: its bodies times its count less one. Each is a body
/// of the document, listed in Objects, so far fewer than the copies a
/// joined pattern may make.
pub const MAX_PATTERN_BODIES: usize = 1024;

/// A pattern: each of its bodies, as the features before it leave it,
/// together with copies of itself placed along a line or about an axis.
/// Each body keeps its id, and holds its copies ([`Copies::Joined`]):
/// side by side where they don't meet, united where they do; or each
/// copy is a new body of its own ([`Copies::Separate`]). Its own faces
/// keep their names; copy `k` (the original is copy 0) names its faces
/// as that copy of the pattern, wherever it is, so later features find
/// each copy's faces apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    /// As a move's: `1..=`[`MAX_FEATURE_BODIES`](crate::MAX_FEATURE_BODIES)
    /// bodies features before it make, sorted without repeats.
    pub bodies: Vec<BodyId>,
    pub kind: PatternKind,
    /// Where the copies go: "Join to original". Files from before the
    /// choice read as joined, its default.
    #[serde(default)]
    pub copies: Copies,
}

/// Where a pattern's copies go: the panel's "Join to original".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Copies {
    /// Into the bodies they're copies of (ticked): one body each.
    #[default]
    Joined,
    /// Each a new body of its own (unticked), made by the pattern: copy
    /// `k` of its `i`th body (`1 ≤ k < count`, `i` from 0) is the body at
    /// `(k − 1) · n + i`, `n` its body count, so a higher count adds
    /// bodies at the end. The bodies may overlap each other and their
    /// originals: they're never united. Adding or setting a pattern
    /// ([`Command::AddFeature`], [`Command::SetFeature`]) lays these out
    /// whatever they held, keeping the body of each copy (by its
    /// original and `k`) the feature it replaces made, and adding new
    /// ones for the rest, as a new extrude's [`BodyId::NEW`]; see
    /// [`Pattern::copy_body`].
    ///
    /// [`Command::AddFeature`]: crate::Command::AddFeature
    /// [`Command::SetFeature`]: crate::Command::SetFeature
    /// [`BodyId::NEW`]: crate::BodyId::NEW
    Separate(Vec<BodyId>),
}

/// Where a pattern's copies go. Counts take in the original:
/// `2..=`[`MAX_PATTERN_COUNT`] in all, whole numbers ([`Pattern::count_ask`]).
/// New kinds are appended, as [`AxisRef`]'s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    /// Copy `k` moved `k · spacing` along the axis's direction (its point
    /// doesn't matter). The spacing is a length within
    /// [`MAX_COORD`] of zero but not zero ([`Pattern::spacing_ask`]); a
    /// negative one runs the other way.
    Linear {
        along: AxisRef,
        count: Value,
        spacing: Value,
    },
    /// Copy `k` turned about the axis, right-handed about its direction.
    /// The angle is the span the copies are spread over, above zero and
    /// at most a turn ([`Pattern::angle_ask`]): a whole turn shares its
    /// ends, `count` copies `turn / count` apart; a span short of one has
    /// a copy at each end, `count` copies `angle / (count − 1)` apart
    /// ([`Pattern::step_degrees`]).
    Circular {
        about: AxisRef,
        count: Value,
        angle: Value,
    },
}

impl PatternKind {
    /// The axis it names.
    pub fn axis(&self) -> &AxisRef {
        match self {
            PatternKind::Linear { along, .. } => along,
            PatternKind::Circular { about, .. } => about,
        }
    }

    /// The same, to change.
    pub fn axis_mut(&mut self) -> &mut AxisRef {
        match self {
            PatternKind::Linear { along, .. } => along,
            PatternKind::Circular { about, .. } => about,
        }
    }

    /// Its count, as typed.
    pub fn count_value(&self) -> &Value {
        match self {
            PatternKind::Linear { count, .. } | PatternKind::Circular { count, .. } => count,
        }
    }
}

impl Pattern {
    /// What a count is checked against in `design`: a whole number from 2
    /// to [`MAX_PATTERN_COUNT`].
    pub fn count_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        Ask::number(design.units, f64::from(MAX_PATTERN_COUNT))
            .whole()
            .at_least(2.0)
            .with_params(design.params)
    }

    /// What a linear pattern's spacing is checked against in `design`: a
    /// length within [`MAX_COORD`] of zero, bare numbers in its units
    /// (zero is refused apart, see [`Pattern::check_own`]).
    pub fn spacing_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        Ask::length(design.units, f64::from(MAX_COORD)).with_params(design.params)
    }

    /// What a circular pattern's angle is checked against in `design`:
    /// above zero and at most a turn, bare numbers in degrees.
    pub fn angle_ask<'p>(design: &Design<'p>) -> Ask<'p> {
        Ask::angle(design.units, TAU)
            .positive()
            .with_params(design.params)
    }

    /// How many copies it makes of each body, the original among them, if
    /// its count is one [`Pattern::count_ask`] takes (as a checked
    /// document's is).
    pub fn count(&self) -> Option<u32> {
        let value = self.kind.count_value().value;
        let range = 2.0..=f64::from(MAX_PATTERN_COUNT);
        // Whole and in range, so the conversion is exact.
        (range.contains(&value) && value.fract() == 0.0).then_some(value as u32)
    }

    /// Whether its copies are joined to their originals
    /// ([`Copies::Joined`]).
    pub fn joins(&self) -> bool {
        self.copies == Copies::Joined
    }

    /// How many new bodies it makes unjoined: its bodies times its count
    /// less one, if the count is one [`Pattern::count`] takes (a checked
    /// product all the same: both come from the user). Joined, or not, it
    /// is what [`Copies::Separate`] is to list.
    pub fn separate_count(&self) -> Option<usize> {
        let steps = usize::try_from(self.count()? - 1).ok()?;
        self.bodies.len().checked_mul(steps)
    }

    /// The body of copy `k` (`1 ≤ k < count`) of its `source`th body, if
    /// its copies are bodies of their own and it lists one there: see
    /// [`Copies::Separate`].
    pub fn copy_body(&self, source: usize, k: u32) -> Option<BodyId> {
        let Copies::Separate(made) = &self.copies else {
            return None;
        };
        if source >= self.bodies.len() {
            return None;
        }
        let at = usize::try_from(k.checked_sub(1)?)
            .ok()?
            .checked_mul(self.bodies.len())?
            .checked_add(source)?;
        made.get(at).copied()
    }

    /// Its copy bodies, if they're bodies of their own: each with the
    /// body it's a copy of and its `k`, in the order listed.
    pub fn copy_bodies(&self) -> impl Iterator<Item = (BodyId, u32, BodyId)> + '_ {
        let made: &[BodyId] = match &self.copies {
            Copies::Separate(made) => made,
            Copies::Joined => &[],
        };
        let n = self.bodies.len().max(1);
        (made.iter().enumerate()).filter_map(move |(at, &body)| {
            let source = *self.bodies.get(at % n)?;
            let k = u32::try_from(at / n).ok()?.checked_add(1)?;
            Some((source, k, body))
        })
    }

    /// Whether a circular pattern's angle comes to a whole turn, to the
    /// rounding of angles typed in degrees ("360", "180 + 180"): the
    /// copies then share the span's ends. False for a linear pattern.
    pub fn full_turn(&self) -> bool {
        match &self.kind {
            PatternKind::Circular { angle, .. } => angle.value >= TAU - TURN_ROUNDING,
            PatternKind::Linear { .. } => false,
        }
    }

    /// A circular pattern's angle between neighbouring copies, in degrees,
    /// see [`PatternKind::Circular`]; `None` for a linear one or a count
    /// [`Pattern::count`] doesn't take. Copy `k` is turned by `k` times
    /// it, worked out as `k · span / n` (`n` the count, or the count
    /// less one), so whole turns split evenly are exact.
    pub fn step_degrees(&self) -> Option<f64> {
        let (span, steps) = self.span_steps()?;
        Some(span / f64::from(steps))
    }

    /// A circular pattern's span in degrees (360 for a whole turn) and
    /// the steps it's split into: copy `k` turns `k · span / steps`.
    pub fn span_steps(&self) -> Option<(f64, u32)> {
        let count = self.count()?;
        match &self.kind {
            PatternKind::Circular { angle, .. } => Some(if self.full_turn() {
                (360.0, count)
            } else {
                // Typed in degrees, the angle comes back to the number
                // typed.
                (angle.value / (std::f64::consts::PI / 180.0), count - 1)
            }),
            PatternKind::Linear { .. } => None,
        }
    }

    /// Checks what needs only the pattern and `design`: the body count and
    /// order, the count, unjoined at most [`MAX_PATTERN_BODIES`] new
    /// bodies, the spacing or angle, and an axis edge's or face's own
    /// parts. What the bodies and the axis name, and the copy bodies
    /// listed, is
    /// [`Document::check`](crate::Document::check)'s. Cheap, for a panel
    /// to run on every view.
    pub fn check_own(&self, design: &Design) -> Result<(), MotionError> {
        check_bodies(&self.bodies)?;
        (self.kind.count_value())
            .check(&Pattern::count_ask(design))
            .map_err(|_| MotionError::Count)?;
        if !self.joins() {
            // The count is checked, so this is a product in range.
            let bodies = self.separate_count().unwrap_or(usize::MAX);
            if bodies > MAX_PATTERN_BODIES {
                return Err(MotionError::Separate(bodies));
            }
        }
        match &self.kind {
            PatternKind::Linear { spacing, .. } => {
                spacing
                    .check(&Pattern::spacing_ask(design))
                    .map_err(|_| MotionError::Spacing)?;
                if spacing.value == 0.0 {
                    return Err(MotionError::Spacing);
                }
            }
            PatternKind::Circular { angle, .. } => angle
                .check(&Pattern::angle_ask(design))
                .map_err(|_| MotionError::Angle)?,
        }
        if let Some(referred) = self.kind.axis().refers() {
            referred.check_own()?;
        }
        Ok(())
    }

    /// The edge or face its axis names, if it names one.
    pub(crate) fn referred(&self) -> Option<Referred<'_>> {
        self.kind.axis().refers()
    }

    /// Its values: the count, and the spacing or angle, with the ask
    /// each is checked against in `design`.
    pub(crate) fn values_mut<'p>(&mut self, design: &Design<'p>) -> [(&mut Value, Ask<'p>); 2] {
        match &mut self.kind {
            PatternKind::Linear { count, spacing, .. } => [
                (count, Pattern::count_ask(design)),
                (spacing, Pattern::spacing_ask(design)),
            ],
            PatternKind::Circular { count, angle, .. } => [
                (count, Pattern::count_ask(design)),
                (angle, Pattern::angle_ask(design)),
            ],
        }
    }
}

#[cfg(test)]
mod tests;

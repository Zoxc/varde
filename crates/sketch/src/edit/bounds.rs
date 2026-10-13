//! Bounding an edit from outside the app ([`SketchEdit::check_bounds`]):
//! counts no more than a sketch may hold and every number finite and
//! within a limit, before it's applied.

use glam::DVec2;
use varde_expr::Value;

use crate::{
    Curve, Id, MAX_CONSTRAINTS, MAX_CURVES, MAX_DIMENSIONS, MAX_LINKS, MAX_POINTS, Setback,
    SketchEdit, SketchError,
};

/// The most items a delete may name: every point, curve and constraint
/// (a const, so a sum that overflowed wouldn't build).
const MAX_DELETE: usize = MAX_POINTS + MAX_CURVES + MAX_CONSTRAINTS;
/// The most items a mirror may name: every point and curve.
const MAX_MIRROR: usize = MAX_POINTS + MAX_CURVES;

/// Why an edit or a drag's targets fail their bounds.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum BoundsError {
    /// More of something than an edit may name: `what` names it, as a
    /// plural ("points", "bytes of a dimension's expression").
    #[error("{count} {what}, over the limit of {limit}")]
    TooMany {
        what: &'static str,
        count: usize,
        limit: usize,
    },
    /// A place or radius past `max`, or not a number.
    #[error("a sketch value of {value}, outside the limit of {max}")]
    Value { value: f64, max: f64 },
    /// A dimension's value (or an offset's, a fillet's, ...) not a number.
    #[error("a dimension value of {0}, not a number")]
    NotFinite(f64),
    /// A new spline or a link's shape that doesn't fit.
    #[error("{0}")]
    Sketch(#[source] SketchError),
}

impl SketchEdit {
    /// Checks the counts and the numbers of the edit, places and radii
    /// within `max`. Whether what it names is there is applying it's to
    /// say.
    pub fn check_bounds(&self, max: f64) -> Result<(), BoundsError> {
        let place = |at| check_place(at, max);
        match self {
            SketchEdit::Add(add) => {
                check_count("points", add.points.len(), MAX_POINTS)?;
                check_count("curves", add.curves.len(), MAX_CURVES)?;
                check_count("constraints", add.constraints.len(), MAX_CONSTRAINTS)?;
                check_count("inferred constraints", add.auto.len(), MAX_CONSTRAINTS)?;
                check_count("dimensions", add.dimensions.len(), MAX_DIMENSIONS)?;
                add.points.iter().try_for_each(|&(_, at)| place(at))?;
                add.curves.iter().try_for_each(|new| match &new.curve {
                    &Curve::Circle { radius, .. } => check_value(radius, max),
                    Curve::Spline(spline) if !spline.fits() => {
                        Err(BoundsError::Sketch(SketchError::Spline(new.id)))
                    }
                    Curve::Line { .. } | Curve::Arc { .. } | Curve::Spline(_) => Ok(()),
                })?;
                add.dimensions.iter().try_for_each(|dimension| {
                    place(dimension.label)?;
                    check_dimension_value(&dimension.value)
                })
            }
            SketchEdit::Delete(ids) => check_count("items", ids.len(), MAX_DELETE),
            SketchEdit::Move { points, radii } => check_targets(points, radii, max),
            SketchEdit::SetConstruction { ids, .. } => check_count("curves", ids.len(), MAX_CURVES),
            SketchEdit::SetDimension { value, .. } => check_dimension_value(value),
            SketchEdit::SetDriving { .. } => Ok(()),
            SketchEdit::MoveLabel { label, .. } => place(*label),
            SketchEdit::Trim { near, .. } => place(*near),
            SketchEdit::Extend { .. } => Ok(()),
            SketchEdit::Mirror { ids, .. } => check_count("items", ids.len(), MAX_MIRROR),
            SketchEdit::Offset {
                chain, distance, ..
            } => {
                check_count("curves", chain.len(), MAX_CURVES)?;
                check_dimension_value(distance)
            }
            SketchEdit::Fillet { radius, .. } => check_dimension_value(radius),
            SketchEdit::Chamfer { setback, .. } => match setback {
                Setback::Equal(d) => check_dimension_value(d),
                Setback::Two(a, b) | Setback::Angle(a, b) => {
                    check_dimension_value(a)?;
                    check_dimension_value(b)
                }
            },
            SketchEdit::Convert { .. } => Ok(()),
            SketchEdit::AddHandles(points) => check_count("points", points.len(), MAX_POINTS),
            SketchEdit::InsertPoint { near, .. } => place(*near),
            SketchEdit::AddLink { .. }
            | SketchEdit::SetLinkProfiles { .. }
            | SketchEdit::Detach(_)
            | SketchEdit::CloseArc(_)
            | SketchEdit::CloseSpline(_)
            | SketchEdit::OpenSpline { .. } => Ok(()),
            SketchEdit::Relink(found) => {
                check_count("links", found.len(), MAX_LINKS)?;
                match (found.iter()).find(|(_, shape)| !shape.fits(max)) {
                    Some(&(link, _)) => Err(BoundsError::Sketch(SketchError::Link(link))),
                    None => Ok(()),
                }
            }
        }
    }
}

/// Checks a drag's (or a move's) targets: no more than the points and
/// circles a sketch may have, every number finite and within `max`.
pub fn check_targets(
    points: &[(Id, DVec2)],
    radii: &[(Id, f64)],
    max: f64,
) -> Result<(), BoundsError> {
    check_count("points", points.len(), MAX_POINTS)?;
    check_count("radii", radii.len(), MAX_CURVES)?;
    points
        .iter()
        .try_for_each(|&(_, at)| check_place(at, max))?;
    radii
        .iter()
        .try_for_each(|&(_, radius)| check_value(radius, max))
}

/// A dimension's value: its text no longer than an expression may be,
/// and its value a number. Whether they agree is the sketch's check's to
/// say, once the edit is applied.
fn check_dimension_value(value: &Value) -> Result<(), BoundsError> {
    check_count(
        "bytes of a dimension's expression",
        value.text.len(),
        varde_expr::MAX_LEN,
    )?;
    if !value.value.is_finite() {
        return Err(BoundsError::NotFinite(value.value));
    }
    Ok(())
}

/// `count` of `what` (a plural, such as "points"), at most `limit`.
fn check_count(what: &'static str, count: usize, limit: usize) -> Result<(), BoundsError> {
    if count > limit {
        return Err(BoundsError::TooMany { what, count, limit });
    }
    Ok(())
}

fn check_place(at: DVec2, max: f64) -> Result<(), BoundsError> {
    check_value(at.x, max)?;
    check_value(at.y, max)
}

/// A coordinate or radius: finite and within `max` (not a number isn't).
fn check_value(value: f64, max: f64) -> Result<(), BoundsError> {
    if value.abs() <= max {
        Ok(())
    } else {
        Err(BoundsError::Value { value, max })
    }
}

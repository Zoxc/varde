//! Rational quadratic curves and triangles: the pure math the kernel's
//! solids are made of, with no mesh.
//!
//! A curve ([`Conic`]) runs from `p0` to `p1` and is pulled towards its
//! control point `c` by its weight `w`:
//!
//! ```text
//!         (1-t)²·p0 + 2t(1-t)·w·c + t²·p1
//! C(t) = ─────────────────────────────────
//!          (1-t)² + 2t(1-t)·w + t²
//! ```
//!
//! A triangle ([`Patch`]) has corners `p[0..3]`, and edge `i` runs from
//! corner `i` to corner `i + 1` with control point `c[i]` and weight `w[i]`.
//! At barycentric `(u0, u1, u2)`, with `ij` running over the edges 01, 12
//! and 20:
//!
//! ```text
//!        Σ ui²·pi + Σ 2·ui·uj·wij·cij
//! P  =  ──────────────────────────────
//!        Σ ui²    + Σ 2·ui·uj·wij
//! ```
//!
//! Both are in the *standard form*: the corner weights are 1, so an edge's
//! curve depends only on its own two ends, control point and weight, and
//! two patches that share those numbers share the curve exactly.
//!
//! Splits work on the homogeneous control points `(w·p, w)`, where the
//! curve is polynomial and blossoming is exact, and then return to the
//! standard form. With positive weights every curve and patch lies in the
//! convex hull of its control points. The formulas and conventions are
//! written down in `agents/kernel.md`.

use glam::{DVec2, DVec3};

mod conic;
mod fold;
mod strip;
mod triangle;

pub use conic::{Conic, Conic2, Conic3, Point};
pub use fold::NormalCone;
pub use strip::cylinder_strip;
pub use triangle::Patch;

/// The smallest weight a curve or patch edge may have. Smaller weights
/// make near-degenerate conics that hug their chord.
pub const W_MIN: f64 = 1.0 / 64.0;

/// The largest weight a curve or patch edge may have. Larger weights make
/// near-degenerate conics that hug their control polygon.
pub const W_MAX: f64 = 64.0;

/// The largest coordinate a control point may have. Ten times
/// [`MAX_COORD`](crate::MAX_COORD), so the control points of curves near
/// the edge of the model space still fit, and every product the patch
/// math forms stays finite.
pub const MAX_CONTROL: f64 = 1e7;

/// The fold check's margin: a direction `d` passes when every normal
/// coefficient `c` has `c·d > FOLD_MARGIN·|c|·|d|`, so a patch doesn't
/// flicker between valid and invalid under rounding.
pub const FOLD_MARGIN: f64 = 1e-6;

/// A normal coefficient smaller than this, relative to the size of the
/// terms it was summed from, is taken as zero: rounding decides its
/// direction. The fold check fails on it.
pub const FOLD_FLOOR: f64 = 1e-8;

/// An axis-aligned box of points of type `P`, from `min` to `max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds<P> {
    pub min: P,
    pub max: P,
}

pub type Bounds2 = Bounds<DVec2>;
pub type Bounds3 = Bounds<DVec3>;

impl<P: Point> Bounds<P> {
    /// The box around `points`, or `None` if there are none.
    pub fn around(points: &[P]) -> Option<Self> {
        let (&first, rest) = points.split_first()?;
        Some(rest.iter().fold(
            Bounds {
                min: first,
                max: first,
            },
            |b, &p| Bounds {
                min: b.min.min(p),
                max: b.max.max(p),
            },
        ))
    }

    /// The box around both.
    pub fn union(self, other: Self) -> Self {
        Bounds {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// Why a curve or patch is refused, or can't be built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PatchError {
    /// A coordinate that isn't finite or lies past [`MAX_CONTROL`].
    Coordinate(f64),
    /// A weight outside [`W_MIN`]`..=`[`W_MAX`], or not finite.
    Weight(f64),
    /// A parameter out of range: a split position outside `(0, 1)`, an
    /// arc's radius or angles.
    Parameter(f64),
    /// Split halves that don't run between the ends of the edge they are
    /// said to split.
    Mismatch,
    /// Input the construction can't use, such as a straight edge where a
    /// curved one is needed.
    Degenerate,
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Coordinate(x) => {
                write!(f, "a control point coordinate of {x} is past {MAX_CONTROL}")
            }
            PatchError::Weight(w) => {
                write!(f, "a weight of {w} is outside {W_MIN}..={W_MAX}")
            }
            PatchError::Parameter(x) => write!(f, "a parameter of {x} is out of range"),
            PatchError::Mismatch => write!(f, "split halves don't match the edge they split"),
            PatchError::Degenerate => write!(f, "the geometry is degenerate"),
        }
    }
}

impl std::error::Error for PatchError {}

/// Refuses a point that isn't finite or lies past [`MAX_CONTROL`].
fn check_point<P: Point>(p: P) -> Result<(), PatchError> {
    let m = p.max_abs();
    if p.is_finite() && m <= MAX_CONTROL {
        Ok(())
    } else {
        Err(PatchError::Coordinate(m))
    }
}

/// Refuses a weight outside [`W_MIN`]`..=`[`W_MAX`] (NaN included).
fn check_weight(w: f64) -> Result<(), PatchError> {
    if (W_MIN..=W_MAX).contains(&w) {
        Ok(())
    } else {
        Err(PatchError::Weight(w))
    }
}

#[cfg(test)]
mod tests;

//! Booleans of solids, whether two touch, and the split tools.
//!
//! The old kernel's booleans are gone. With the empty solid, the only one
//! the facade has, a boolean's result is known without one: a union is
//! the other operand, a difference the first, an intersection empty, and
//! the empty solid touches nothing. Two non-empty solids give
//! [`KernelError::NotImplemented`] until the Varde kernel's booleans land.

use crate::{Budget, Failure, KernelError, Solid, Tolerance};

mod split;

pub use split::{ToolError, chain_tool, half_space, split, surface_tool};

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    /// Everything in either.
    Union,
    /// Everything in the first and not the second.
    Difference,
    /// Everything in both.
    Intersection,
}

/// Why a boolean gives no solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanError {
    /// The decisions don't fit together: near ties decided as ties that
    /// no one configuration has, rarely, with curved operands (flat ones
    /// are then decided again exactly).
    Inconsistent,
    /// A cut face's kept part couldn't be triangulated, or the triangles
    /// don't pair up into a closed surface.
    Degenerate,
    /// The result would touch itself along an edge or at a point, so it
    /// isn't a manifold: solids touching only along an edge or at a corner,
    /// united, or one taking from another a part that touches its skin
    /// from inside along a line or at a point. Parts closer than the
    /// resolution come to the same thing at the kernel's resolution, and
    /// are named so too. Found where the result fails to repair or pass
    /// `check` and has two vertices within the clean-up's short length
    /// before repair, or two separate shells whose hulls come within the
    /// resolution; the decisions may also give it before any mesh is
    /// built, where they show a pinch.
    NotManifold,
}

impl std::fmt::Display for BooleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BooleanError::Inconsistent => "where the solids cross doesn't add up",
            BooleanError::Degenerate => "a cut face couldn't be triangulated",
            BooleanError::NotManifold => {
                "the result would touch itself along an edge or at a point, or come closer \
                 to itself than the resolution"
            }
        })
    }
}

impl std::error::Error for BooleanError {}

/// `a op b`, the faces keeping their names. With an empty operand the
/// result is known (see the module's docs); two non-empty solids give
/// [`KernelError::NotImplemented`].
pub fn boolean(
    a: &Solid,
    b: &Solid,
    op: Op,
    _tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, Failure> {
    match (a.is_empty(), b.is_empty(), op) {
        (true, _, Op::Union) => Ok(b.clone()),
        (_, true, Op::Union | Op::Difference) => Ok(a.clone()),
        (true, _, _) | (_, true, _) => Ok(Solid::empty()),
        _ => Err(KernelError::NotImplemented("booleans").into()),
    }
}

/// Whether `a` and `b` touch or overlap. The empty solid touches nothing;
/// two non-empty solids give [`KernelError::NotImplemented`].
pub fn touches(a: &Solid, b: &Solid, _tol: &Tolerance, _budget: &Budget) -> Result<bool, Failure> {
    if a.is_empty() || b.is_empty() {
        Ok(false)
    } else {
        Err(KernelError::NotImplemented("whether solids touch").into())
    }
}

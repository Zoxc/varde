//! The operation's fixed bands: the rounding bound `ρ_op`, each edge's
//! counting band `β_g = edge.tol + ρ_op` and its near radius
//! `r_g = 3·β_g` ([`OpBands`]).
//!
//! **One rounding bound per operation.** `ρ_op` is the first level's
//! rounding bound of a seam evaluation (end weights about 1, no local
//! frame) at the operation's largest operand coordinate magnitude `M`:
//! [`SEAM_OPS`] rounded operations of unit roundoff `u = 2⁻⁵³` each, so
//! `ρ_op = 50·u·M` (5.5·10⁻⁹ mm at `M = 10⁶` mm). It is computed once,
//! when the operation starts, from the operands alone. Every band, near
//! radius and key reads it (the edges' bands, near radii, feet, the
//! point–vertex records' `near`), never a precision level's own
//! rounding, which stays in the values' errors: so the key sets are the
//! same at every precision level, and the precision steps refine values,
//! not which records exist.
//!
//! **Per edge.** An edge's band is its tolerance (the greatest seam bound
//! of its coedges, the ribbons' width) plus `ρ_op`; its **near radius**
//! is three bands (a piece's diameter, a ribbon's width and a hull's
//! slack, each at most a band). A near radius only decides which records
//! are shared (a foot, a vertex near a point), never an error. A vertex's
//! near radius is the greatest of its edges'.
//!
//! Each value is rounded upwards ([`add_up`], [`mul_up`]), so it is at
//! least the real one; being functions of the stored operands, they are
//! the same bits on every run and thread count.

use crate::num::bounded::{add_up, mul_up};

/// The rounded operations of a seam evaluation chain that `ρ_op` counts.
pub const SEAM_OPS: f64 = 50.0;

/// The unit roundoff of `f64`, `2⁻⁵³`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// The bands of one operation (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpBands {
    rho_op: f64,
}

impl OpBands {
    /// The bands of an operation whose operands' largest coordinate
    /// magnitude is `max_coord` (its sign ignored); `None` unless it is
    /// finite.
    pub fn new(max_coord: f64) -> Option<OpBands> {
        if !max_coord.is_finite() {
            return None;
        }
        // `50·u` is exact; the product rounded upwards.
        let rho_op = mul_up(SEAM_OPS * U, max_coord.abs());
        rho_op.is_finite().then_some(OpBands { rho_op })
    }

    /// `ρ_op`.
    pub fn rho_op(&self) -> f64 {
        self.rho_op
    }

    /// The counting band `β_g = tol + ρ_op` of an edge of tolerance
    /// `tol`, rounded upwards; `None` unless `tol` is finite and `≥ 0`.
    pub fn band(&self, tol: f64) -> Option<f64> {
        let b = add_up(fine(tol)?, self.rho_op);
        b.is_finite().then_some(b)
    }

    /// The near radius `r_g = 3·β_g` of an edge of tolerance `tol`,
    /// rounded upwards; `None` unless `tol` is finite and `≥ 0`.
    pub fn near_radius(&self, tol: f64) -> Option<f64> {
        let r = mul_up(3.0, self.band(tol)?);
        r.is_finite().then_some(r)
    }
}

fn fine(x: f64) -> Option<f64> {
    (x.is_finite() && x >= 0.0).then_some(x)
}

#[cfg(test)]
mod tests;

//! Edge–face crossing counts by identity: `x12(e, f)`, how many times
//! (with sign) an edge `e = a→b` of `A` passes through a face `f` of `B`,
//! and `x21(g, f)`, an edge `g = g0→g1` of `B` through a face `f` of `A`,
//! from the records alone:
//!
//! ```text
//! x12(e, f) = s02(b, f)  − s02(a, f)  − Σ_{c∈∂f} sgn(c)·s11(e, g_c)
//! x21(g, f) = s20(g1, f) − s20(g0, f) − Σ_{c∈∂f} sgn(c)·s11(e_c, g)
//! ```
//!
//! **Conventions.** Each edge runs along its stored direction (from its
//! lower-id end), and `sgn(c)` is `+1` exactly when coedge `c` runs that
//! way ([`Coedge::sgn`]). `s02` is the upward ray's winding through the
//! face, `s20` the downward ray's (`+1` per down-facing hit below the
//! point, [`crate::count::s02`]). `s11(e, g)` is the sum of the degrees
//! of the edge pair's clusters with `g` above `e`
//! ([`XRec::s11`](crate::records::xrec::XRec::s11)), the **same** value
//! in both identities, `e` always `A`'s edge: no sign change for `x21`.
//! A crossing counts `−sign(C′·n_f)`, entering the face's solid `+1`, in
//! both. The other three combinations (`s20` as an upward ray, `+Σ s11`,
//! or both) fail on random geometry; the tests check them.
//!
//! **Why it holds.** The column of a point running along `e` from `a` to
//! `b` changes its hit count through `f` exactly where `e` crosses `f`
//! or where the column sweeps past `f`'s boundary, i.e. where `e`'s
//! shadow crosses a trim `g_c` with `g_c` above `e`; the sum of those
//! sweeps is `Σ sgn(c)·s11(e, g_c)`. For `x21` the ray points down and
//! "above" swaps sides, and the two sign flips cancel.
//!
//! **Algebraic consequences** (for any record values, not checks):
//! - per face pair `(f_A, f_B)`, `Σ_{c∈∂f_A} sgn(c)·x12(e_c, f_B) =
//!   Σ_{c∈∂f_B} sgn(c)·x21(g_c, f_A)`: the `s02`/`s20` terms telescope
//!   round each closed loop, leaving `−Σ Σ sgn·sgn·s11` on both sides;
//! - `Σ_f x12(e, f)` over **all** faces of `B` is `w(b) − w(a)` with
//!   `w(v) = Σ_f s02(v, f)`, as each edge of a closed `B` has two
//!   coedges of opposite sense whose `s11` terms cancel; so it sums to 0
//!   round every closed walk of `A`'s edges. Per face it does not.
//!
//! The values are integers and the sums exact; a sum that leaves `i64`
//! is [`CrossError::Overflow`], never a wrapped count.

use crate::ids::EdgeId;

/// A coedge of a face's boundary (any of its loops): the edge it runs
/// along and whether it runs along the edge's stored direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Coedge {
    /// The edge.
    pub edge: EdgeId,
    /// Whether the coedge runs from the edge's first end to its last.
    pub forward: bool,
}

impl Coedge {
    /// `sgn(c)`: `+1` along the edge's stored direction, else `−1`.
    pub fn sgn(self) -> i64 {
        if self.forward { 1 } else { -1 }
    }
}

/// Why a crossing count has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossError<E> {
    /// Reading an `s11` failed (a record still missing, say).
    Read(E),
    /// A sum left `i64`.
    Overflow,
}

/// `x12(e, f)` for an edge `e = a→b` of `A` and a face `f` of `B`:
/// `ends` are `[s02(a, f), s02(b, f)]`, `boundary` the coedges of all
/// of `f`'s loops, and `s11(g)` reads `s11(e, g)` for a trim's edge `g`
/// (module docs).
pub fn x12<E>(
    ends: [i64; 2],
    boundary: &[Coedge],
    s11: impl FnMut(EdgeId) -> Result<i64, E>,
) -> Result<i64, CrossError<E>> {
    by_identity(ends, boundary, s11)
}

/// `x21(g, f)` for an edge `g = g0→g1` of `B` and a face `f` of `A`:
/// `ends` are `[s20(g0, f), s20(g1, f)]` (the downward ray's),
/// `boundary` the coedges of all of `f`'s loops, and `s11(e)` reads
/// `s11(e, g)` for a trim's edge `e`, the same value as `x12` reads, `g`
/// above `e` (module docs).
pub fn x21<E>(
    ends: [i64; 2],
    boundary: &[Coedge],
    s11: impl FnMut(EdgeId) -> Result<i64, E>,
) -> Result<i64, CrossError<E>> {
    by_identity(ends, boundary, s11)
}

/// `ends[1] − ends[0] − Σ sgn(c)·s11(c.edge)`, checked.
fn by_identity<E>(
    ends: [i64; 2],
    boundary: &[Coedge],
    mut s11: impl FnMut(EdgeId) -> Result<i64, E>,
) -> Result<i64, CrossError<E>> {
    let mut x = ends[1].checked_sub(ends[0]).ok_or(CrossError::Overflow)?;
    for c in boundary {
        let s = s11(c.edge).map_err(CrossError::Read)?;
        let term = s.checked_mul(c.sgn()).ok_or(CrossError::Overflow)?;
        x = x.checked_sub(term).ok_or(CrossError::Overflow)?;
    }
    Ok(x)
}

#[cfg(test)]
mod tests;

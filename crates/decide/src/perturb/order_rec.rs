//! The crossing-order row (`OrderRec`): which of two faces an edge
//! crosses first, `‖C′‖·(t₂ − t₁)`, `tᵢ` the edge's parameter where it
//! crosses face `fᵢ`, `+` with `f₂`'s crossing after `f₁`'s. Its orders
//! are `‖C′‖·(ṫ₂ − ṫ₁)` with `ṫᵢ = (nᵢ·Δfᵢ − nᵢ·Δe)/(nᵢ·C′)`: the
//! crossing slides along the edge by the face's motion across its plane
//! less the edge's, over the edge's rate across the plane. `Δfᵢ` is the
//! face's rate and `Δe` the edge's at that crossing (each element's field
//! there; zero on `B`), so the two crossings read the edge at their own
//! points.
//!
//! **Flat** ([`OrderRow`]): an edge on the line `a + t·(b − a)` and two
//! planes `nᵢ·x = cᵢ`, `tᵢ = (cᵢ − nᵢ·a)/(nᵢ·C′)` with `C′ = b − a`. The
//! two quotients share one positive factor: the row is multiplied by
//! `|n₁·C′|·|n₂·C′|`, the facings `sign(nᵢ·C′)` certified exactly once
//! ([`OrderRow::new`]), so it is the polynomial `s₁s₂·((c₂ − n₂·a)·(n₁·C′)
//! − (c₁ − n₁·a)·(n₂·C′))`, `sᵢ` the facings, a positive multiple of the
//! spatial value over `‖C′‖`. Each plane's offset moves by `n·Δf`, the
//! edge as a translation at its rate at each crossing, so its orders are
//! the derivatives exactly.
//!
//! **Curved** ([`order_row`]): the value and orders at a level, lengths,
//! from the edge's derivative at the crossings and the faces' normals
//! there; no answer where a facing isn't certified.

use std::cmp::Ordering;

use glam::DVec3;

use super::{Coeffs, Curved, Rate, dot3, moved, still, unit3};
use crate::num::decide::Sign;
use crate::num::exact::{self, Num, ORDERS, Pred, Work, dot, point, sub};
use crate::num::preds;
use crate::num::schedule::PredKind;

/// One face of a flat crossing-order row: its plane and the rates at its
/// crossing.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderFace {
    /// The plane's normal `n`, oriented as its face (scaled freely: a
    /// stored normal of largest component 1).
    pub normal: DVec3,
    /// The offset `c` of the plane `n·x = c`.
    pub offset: f64,
    /// The face's rates at the crossing.
    pub rates: [Rate; ORDERS],
    /// The edge's rates at the crossing.
    pub edge_rates: [Rate; ORDERS],
}

/// The crossing-order row, flat (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct OrderRow {
    /// The edge's line, from `a` toward `b`.
    pub line: [DVec3; 2],
    /// The faces `f₁`, `f₂`.
    pub faces: [OrderFace; 2],
    /// `sign(nᵢ·C′)`, certified.
    facing: [Sign; 2],
}

/// `n·(b − a)`: which way an edge crosses a plane.
struct Facing {
    n: DVec3,
    line: [DVec3; 2],
}

impl Pred for Facing {
    fn eval<N: Num>(&self) -> N {
        let [a, b] = self.line.map(point::<N>);
        preds::along(&point(self.n), &a, &b)
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

impl OrderRow {
    /// The row of the edge on `line` crossing `faces`, with each facing
    /// `sign(nᵢ·C′)` certified exactly, and what that took. `None` where
    /// the edge runs along a face's plane (no crossing to order), or an
    /// input isn't finite.
    pub fn new(line: [DVec3; 2], faces: [OrderFace; 2]) -> (Option<OrderRow>, Work) {
        let mut work = Work::default();
        let mut facing = [Sign::Pos; 2];
        for (s, f) in facing.iter_mut().zip(&faces) {
            let (sign, w) = exact::value_sign(&Facing { n: f.normal, line });
            work = work.and(w);
            match sign.ok().and_then(Sign::of) {
                Some(x) => *s = x,
                None => return (None, work),
            }
        }
        (
            Some(OrderRow {
                line,
                faces,
                facing,
            }),
            work,
        )
    }

    /// The facings `sign(nᵢ·C′)`.
    pub fn facing(&self) -> [Sign; 2] {
        self.facing
    }
}

impl Pred for OrderRow {
    fn eval<N: Num>(&self) -> N {
        let [a, b] = self.line;
        let c = sub(&point::<N>(b), &point::<N>(a));
        // `cᵢ − nᵢ·a` moved, and `nᵢ·C′`.
        let [(o1, f1), (o2, f2)] = self.faces.each_ref().map(|f| {
            let n = point::<N>(f.normal);
            let shift = f.rates.each_ref().map(|r| dot(&n, &r.vec()));
            let off = N::lit(f.offset).moved_by(&shift);
            let at = moved::<N>(a, &f.edge_rates);
            (off.sub(&dot(&n, &at)), dot(&n, &c))
        });
        let v = o2.mul(&f1).sub(&o1.mul(&f2));
        if self.facing[0] == self.facing[1] {
            v
        } else {
            v.neg()
        }
    }

    fn kind(&self) -> PredKind {
        PredKind::Crossing
    }
}

/// A crossing-order record at a level, for [`order_row`].
#[derive(Clone, Debug, PartialEq)]
pub struct OrderAt<S> {
    /// The edge's derivative `C′ = dC/dt` at the crossings (the true
    /// derivative: the value is a length through it).
    pub tangent: [S; 3],
    /// The parameters `t₁`, `t₂` of the crossings.
    pub t: [S; 2],
    /// Each face's normal at its crossing (any positive multiple).
    pub normals: [[S; 3]; 2],
    /// Each face's rates at its crossing.
    pub face_rates: [[[S; 3]; ORDERS]; 2],
    /// The edge's rates at each crossing.
    pub edge_rates: [[[S; 3]; ORDERS]; 2],
}

/// The crossing-order row, curved (module docs): `‖C′‖·(t₂ − t₁)` and its
/// orders `‖C′‖·(ṫ₂ − ṫ₁)`, lengths with certified errors. An order along
/// which neither element at a crossing moves has an exact zero `ṫ` there
/// (so an order no element moves is an exact zero). `None` where a
/// facing `sign(nᵢ·C′)` isn't certified (the edge too near a face's
/// plane).
pub fn order_row<S: Curved>(r: &OrderAt<S>) -> Option<Coeffs<S>> {
    let like = &r.tangent[0];
    let c = unit3(&r.tangent);
    let len = dot3(&r.tangent, &r.tangent).sqrt_nonneg();
    let mut den = Vec::with_capacity(2);
    let mut ns = Vec::with_capacity(2);
    for n in &r.normals {
        let n = unit3(n);
        let f = dot3(&n, &c);
        if f.sign()? == Ordering::Equal {
            return None;
        }
        den.push(f);
        ns.push(n);
    }
    let rate = |i: usize, k: usize| -> S {
        let (df, de) = (&r.face_rates[i][k], &r.edge_rates[i][k]);
        if still(df) && still(de) {
            S::int(0, like)
        } else {
            let d = [0, 1, 2].map(|j| df[j].sub(&de[j]));
            dot3(&ns[i], &d).div(&den[i])
        }
    };
    Some(Coeffs {
        g: len.mul(&r.t[1].sub(&r.t[0])),
        ord: std::array::from_fn(|k| {
            let (t1, t2) = (rate(0, k), rate(1, k));
            if t1.is_int(0) && t2.is_int(0) {
                S::int(0, like)
            } else {
                t2.sub(&t1)
            }
        }),
    })
}

#[cfg(test)]
mod tests;

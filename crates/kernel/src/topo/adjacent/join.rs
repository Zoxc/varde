//! The join-zone embedding of an edge ([`certify_join`]), the edge's
//! `embedded_join` flag: inside its join zone the two ribbon-closed faces
//! meet only along the edge's curve. In each cross-section of the edge
//! the two **arms** (the ribbon segment from `C(t)` to `S(p(t))`, then the
//! face from `p(t)` inwards) cross neither each other nor the other
//! face's ribbon. It is a flag, not a validity condition: an edge it
//! doesn't certify is valid, and counting near it may take the lobe
//! fallback. Never an error: whatever doesn't settle leaves it unset.
//!
//! **Certified here: planes along a line**, exactly. Where both faces lie
//! on planes whose tiles are affine (every control point the affine image
//! of its node, the weights equal), the coedges' p-curve spans are
//! straight and the edge's curve is a straight segment (its control
//! points on one line, the middle one between the others), per piece of
//! the edge (`super::Edge`'s pieces):
//! - the plane `Π` through the curve's line spanned by its direction `D`
//!   and `m = N₁/λ₁ + N₂/λ₂` (the faces' outward normals, `λᵢ` the
//!   squared lengths' nearest powers of two), certified between the
//!   normals (`m·Nᵢ > 0`), so `G(X) = (X − P₀)·(m × D)` is one affine
//!   function for the whole piece, `P₀` the curve's first control point;
//! - each face on its own side: the face of the coedge running the edge's
//!   way with `G > 0`, the other with `G < 0`. On a face's plane `G∘S` is
//!   affine in UV, so along the straight p-curve span its sign is its
//!   ends', and into the face along the UV normal `ν` of the span (left
//!   or right by the coedge's direction and the face's flip) its
//!   derivative is one number. Required: that derivative of the required
//!   sign, and at each end of the span `G∘S` of the required sign, or zero
//!   where the ribbon is: `S(p) = C` there, bit for bit as rationals.
//!
//! Then each arm lies strictly on its side of `Π` but for the point
//! `C(t)` (the ribbon a segment from `C(t)` to `S(p(t))`, the face beyond
//! it), so the arms meet only there, at every `t` and beyond the join
//! zone too. Every value is an exact rational of the stored values.
//!
//! **Not certified yet** (the flag left unset): curved faces, curved
//! p-curves or curves, faces on one surface, edges at a pole or of zero
//! length. Their certificate in the edge certificate's `(t, s)` family is
//! a later part.
//!
//! **Work.** One unit per piece and face.

use malachite::Rational;
use malachite::base::num::arithmetic::traits::{FloorLogBase2, PowerOf2, Sign};
use std::cmp::Ordering;

use super::{Edge, Piece, Skip, edge_of};
use crate::geom::pcurve::PSpan;
use crate::geom::surface::Surface;
use crate::infra::budget::{Ledger, Step, Stop};
use crate::topo::brep::{Brep, EdgeId};
use varde_decide::num::exact::rat as q;

type Q3 = [Rational; 3];

fn sub(a: &Q3, b: &Q3) -> Q3 {
    std::array::from_fn(|k| &a[k] - &b[k])
}

fn add(a: &Q3, b: &Q3) -> Q3 {
    std::array::from_fn(|k| &a[k] + &b[k])
}

fn scale(a: &Q3, s: &Rational) -> Q3 {
    std::array::from_fn(|k| &a[k] * s)
}

fn cross(a: &Q3, b: &Q3) -> Q3 {
    let x = |i: usize, j: usize| &a[i] * &b[j] - &a[j] * &b[i];
    [x(1, 2), x(2, 0), x(0, 1)]
}

fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn point(p: [f64; 3]) -> Q3 {
    p.map(q)
}

/// A tile's affine map: `S(s, t) = o + s·eu + t·ev` on its local square.
struct Affine {
    i: i64,
    j: i64,
    o: Q3,
    eu: Q3,
    ev: Q3,
}

impl Affine {
    /// The point at global `uv`.
    fn at(&self, uv: &[Rational; 2]) -> Q3 {
        let s = &uv[0] - Rational::from(self.i);
        let t = &uv[1] - Rational::from(self.j);
        add(&add(&self.o, &scale(&self.eu, &s)), &scale(&self.ev, &t))
    }

    /// The point's derivative along the UV direction `nu`.
    fn along(&self, nu: &[Rational; 2]) -> Q3 {
        add(&scale(&self.eu, &nu[0]), &scale(&self.ev, &nu[1]))
    }

    /// The outward normal of the surface (`eu × ev`), against it on a
    /// flipped face.
    fn normal(&self, flipped: bool) -> Q3 {
        let n = cross(&self.eu, &self.ev);
        if flipped { n.map(|x| -x) } else { n }
    }
}

/// The affine map of the tile of `surface` holding the global UV points
/// `a` and `b` (both in one tile, its closed square), if it is affine.
fn affine_tile(surface: &Surface, a: [f64; 2], b: [f64; 2]) -> Option<Affine> {
    let at = surface.locate([a[0] * 0.5 + b[0] * 0.5, a[1] * 0.5 + b[1] * 0.5])?;
    let (i, j) = (at.i, at.j);
    let inside = |p: [f64; 2]| {
        let (s, t) = (q(p[0]) - Rational::from(i), q(p[1]) - Rational::from(j));
        let unit = |x: &Rational| *x >= 0u32 && *x <= 1u32;
        unit(&s) && unit(&t)
    };
    if !(inside(a) && inside(b)) {
        return None;
    }
    let tile = surface.tile(i, j)?;
    let w = tile.net()[0][3];
    if tile.net().iter().any(|p| p[3].to_bits() != w.to_bits()) {
        return None;
    }
    let o = point(tile.point(0, 0));
    let eu = sub(&point(tile.point(2, 0)), &o);
    let ev = sub(&point(tile.point(0, 2)), &o);
    let half = Rational::from_signeds(1i32, 2i32);
    for a in 0..3u32 {
        for b in 0..3u32 {
            let want = add(
                &add(&o, &scale(&eu, &(Rational::from(a) * &half))),
                &scale(&ev, &(Rational::from(b) * &half)),
            );
            if point(tile.point(a as usize, b as usize)) != want {
                return None;
            }
        }
    }
    Some(Affine { i, j, o, eu, ev })
}

/// Whether the join zone of edge `e` of `body` is certified embedded
/// (module docs), charged to `ledger`; `false` wherever the certificate
/// doesn't settle or isn't built. Running out of the budget stops it.
pub(crate) fn certify_join(
    body: &Brep,
    e: EdgeId,
    ledger: &mut Ledger<'_>,
) -> Result<bool, Stop<Skip>> {
    let Ok(edge) = edge_of(body, e) else {
        return Ok(false);
    };
    if edge.one_surface {
        return Ok(false);
    }
    let Some(pieces) = edge.pieces() else {
        return Ok(false);
    };
    for piece in &pieces {
        for _ in 0..2 {
            ledger.charge(Step::Work, 1)?;
        }
        if !piece_holds(&edge, piece) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The certificate on one piece (module docs).
fn piece_holds(edge: &Edge<'_>, piece: &Piece) -> bool {
    let (sc, sp) = (piece.sc, piece.sp);
    // The piece one whole span of the curve and of each p-curve, so that
    // the spans' ends are the piece's.
    let whole = |k: &[f64], i: usize| {
        k[i].to_bits() == piece.a.to_bits() && k[i + 1].to_bits() == piece.b.to_bits()
    };
    if !whole(edge.curve.knots(), sc) || (0..2).any(|k| !whole(edge.sides[k].pcurve.knots(), sp[k]))
    {
        return false;
    }
    let span = &edge.curve.spans()[sc];
    let [p0, p1, p2] = span.map(|c| point([c[0], c[1], c[2]]));
    let d = sub(&p2, &p0);
    // A straight segment, its middle control point between the ends.
    if d.iter().all(|x| *x == 0u32)
        || cross(&sub(&p1, &p0), &d).iter().any(|x| *x != 0u32)
        || dot(&sub(&p1, &p0), &d) < 0u32
        || dot(&sub(&p2, &p1), &d) < 0u32
    {
        return false;
    }
    let mut faces = Vec::with_capacity(2);
    for (k, side) in edge.sides.iter().enumerate() {
        let PSpan::Linear(ends) = side.pcurve.spans()[sp[k]] else {
            return false;
        };
        let uv = ends.map(|h| [h[0] / h[2], h[1] / h[2]]);
        if uv.iter().flatten().any(|x| !x.is_finite()) {
            return false;
        }
        let Some(map) = affine_tile(side.surface, uv[0], uv[1]) else {
            return false;
        };
        faces.push((side, uv, map));
    }
    let normals: Vec<Q3> = faces.iter().map(|(s, _, m)| m.normal(s.flipped)).collect();
    // `m = N₁/λ₁ + N₂/λ₂`, `λᵢ` a power of two near `|Nᵢ|` (from the
    // squared length's binary exponent).
    let lambda = |n: &Q3| -> Option<Rational> {
        let l2 = dot(n, n);
        if l2 == 0u32 {
            return None;
        }
        let e: i64 = (&l2).floor_log_base_2();
        Some(Rational::power_of_2(e.div_euclid(2)))
    };
    let (Some(l0), Some(l1)) = (lambda(&normals[0]), lambda(&normals[1])) else {
        return false;
    };
    let m = add(
        &scale(&normals[0], &(Rational::from(1u32) / l0)),
        &scale(&normals[1], &(Rational::from(1u32) / l1)),
    );
    if normals.iter().any(|n| dot(&m, n) <= 0u32) {
        return false;
    }
    let w = cross(&m, &d);
    let g = |x: &Q3| dot(&sub(x, &p0), &w);
    // The curve's point at the span's ends: its end control points.
    let c_end = [&p0, &p2];
    for (k, (side, uv, map)) in faces.iter().enumerate() {
        // The face of the coedge running the edge's way: `G > 0`.
        let want = if k == 0 {
            Ordering::Greater
        } else {
            Ordering::Less
        };
        let uvq = uv.map(|p| p.map(q));
        let du = [&uvq[1][0] - &uvq[0][0], &uvq[1][1] - &uvq[0][1]];
        // Into the face: left of the span's direction where the face lies
        // left of the edge's direction.
        let nu = if side.left() {
            [-du[1].clone(), du[0].clone()]
        } else {
            [du[1].clone(), -du[0].clone()]
        };
        if dot(&map.along(&nu), &w).sign() != want {
            return false;
        }
        // The span runs the edge's way on face 0, against it on face 1
        // (its p-curve over the edge's parameter, so its ends are the
        // curve's ends in order on both).
        for end in 0..2 {
            let x = map.at(&uvq[end]);
            match g(&x).sign() {
                s if s == want => {}
                Ordering::Equal if x == *c_end[end] => {}
                _ => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;

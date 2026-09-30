//! The ray tests: how often the shadow of an edge crosses a ray from a
//! vertex of the other operand, seen along [`UP`](super::UP).
//!
//! The ray from a vertex `v` runs along [`RAY`], a fixed horizontal
//! direction, in the projection plane: the points `v + s·RAY` (for any
//! height). Which side of the line through `v` a point `c` is on is the
//! sign of `(c − v)·ACROSS`. For a vertex of one operand and an end of an
//! edge of the other, that sign is decided exactly, with `A`'s vertices
//! perturbed as [`Flat`](super::super::flat) perturbs them, so it is
//! never zero and the same whichever of the two asks. Everything else
//! about a straight edge is exact too; about a curved one, where its
//! shadow crosses the line is a quadratic's roots, worked out in floating
//! point, whose number the ends' exact signs fix up to pairs.

use glam::DVec3;

use super::super::exact::{self, Num, Pred, Pt, dir, dot, sub};
use super::Axes;
use super::bernstein;
use crate::patch::Conic3;

/// The direction rays run along: horizontal (square to [`UP`](super::UP)),
/// and square to no axis, so edges along the axes don't lie along rays.
pub(crate) const RAY: DVec3 = DVec3::new(3.0, -2.0, 0.0);

/// `UP × RAY`: across the rays, in the projection plane. `(c − v)·ACROSS`
/// is positive when `c` is to the left of the ray from `v` seen from
/// `+UP`.
pub(crate) const ACROSS: DVec3 = DVec3::new(64.0, 96.0, -13.0);

/// An edge as the ray tests take it: its ends as exact points, its curve
/// in the same direction, and whether it is straight (taken as the
/// segment between its ends).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RayEdge {
    pub(crate) c: Pt,
    pub(crate) d: Pt,
    pub(crate) conic: Conic3,
    pub(crate) straight: bool,
}

/// `(c − v)·ACROSS`.
struct Beside {
    v: Pt,
    c: Pt,
}

impl Pred for Beside {
    fn eval<N: Num>(&self) -> N {
        dot(&sub(&self.c.v3(), &self.v.v3()), &dir(ACROSS))
    }

    fn scale(&self) -> f64 {
        ACROSS.length()
    }
}

/// With `a = c − v` and `b = d − v`: `(b·RAY)(a·ACROSS) − (a·RAY)(b·ACROSS)`,
/// whose sign is that of `a·ACROSS − b·ACROSS` when the segment from `c`
/// to `d` crosses the line through `v` ahead of `v` (along `RAY`).
struct Ahead {
    v: Pt,
    c: Pt,
    d: Pt,
}

impl Pred for Ahead {
    fn eval<N: Num>(&self) -> N {
        let v = self.v.v3::<N>();
        let (a, b) = (sub(&self.c.v3(), &v), sub(&self.d.v3(), &v));
        let (ray, across) = (dir(RAY), dir(ACROSS));
        dot(&b, &ray)
            .mul(&dot(&a, &across))
            .sub(&dot(&a, &ray).mul(&dot(&b, &across)))
    }

    fn scale(&self) -> f64 {
        // The crossing's distance along the ray times the edge's length
        // across it, roughly.
        RAY.length() * ACROSS.length() * (self.d.p - self.c.p).length()
    }
}

/// Which side of the line along [`RAY`] through `v` the point `c` is on
/// (+1 left, seen from `+UP`), exactly. One of the two is a perturbed
/// vertex of `A` and the other a vertex of `B`, so it is never zero in
/// fact; should it be, it is +1 asked from `A`'s vertex, and so always
/// the opposite the other way round.
pub(crate) fn beside(v: Pt, c: Pt, tie: f64) -> i8 {
    let nonzero = |s: i8| if s == 0 { 1 } else { s };
    if v.n.is_some() || c.n.is_none() {
        nonzero(exact::sign_tied(&Beside { v, c }, tie))
    } else {
        -nonzero(exact::sign_tied(&Beside { v: c, c: v }, tie))
    }
}

/// `ρ`: the signed number of times the shadow of `g` crosses the ray from
/// `v` (ahead of `v` along [`RAY`] if `ahead`, behind it if not), +1
/// where `g` crosses it going left (towards `+ACROSS`). `v` and `g`
/// belong to different operands.
pub(crate) fn ray(v: Pt, g: &RayEdge, ahead: bool, axes: &Axes, tie: f64) -> i32 {
    let (sc, sd) = (beside(v, g.c, tie), beside(v, g.d, tie));
    if g.straight {
        if sc == sd {
            return 0;
        }
        let front = exact::sign_tied(&Ahead { v, c: g.c, d: g.d }, tie);
        let hit = if ahead { front == sc } else { front == -sc };
        return if hit { i32::from(sd) } else { 0 };
    }
    // The shadow's distance across the line, `q(s)`, as a quadratic in
    // Bernstein form, its ends with their exact signs.
    let conic = &g.conic;
    let across = |p: DVec3| (p - v.p).dot(axes.across);
    let end = |x: f64, s: i8| {
        if x != 0.0 && (x > 0.0) == (s > 0) {
            x
        } else {
            f64::from(s) * f64::MIN_POSITIVE
        }
    };
    let q = [
        end(across(conic.p0), sc),
        conic.w * across(conic.c),
        end(across(conic.p1), sd),
    ];
    let mut rho = 0;
    for (k, s) in bernstein::roots(&q).into_iter().enumerate() {
        // Before the k-th root the shadow is on the side `sc·(−1)^k`, and
        // it crosses to the other.
        let before = if k % 2 == 0 { sc } else { -sc };
        let h = hom_eval(conic, s, v.p);
        let along = h.dot(axes.along);
        let front = if along.abs() <= tie * weight(conic, s) {
            tied_ahead(v, g, s, axes)
        } else {
            sign(along)
        };
        let hit = if ahead { front > 0 } else { front < 0 };
        if hit {
            rho -= i32::from(before);
        }
    }
    rho
}

/// Whether the crossing of `g`'s shadow at `s` with the line through `v`
/// is ahead of `v` (+1) or behind it (−1) once the operands are
/// perturbed, where it is at `v` itself as far as rounding tells (`v` an
/// end of `g`, or on it). The relative motion of `v` from `g`'s point
/// there is `δ`, and with `T` the curve's tangent the crossing moves to
/// `ε·(δ_across·T_along / T_across − δ_along)` along the ray: `δ` of the
/// first order (each vertex's own direction, interpolated along `g`),
/// else the generic translations after it.
fn tied_ahead(v: Pt, g: &RayEdge, s: f64, axes: &Axes) -> i8 {
    let (_, tangent) = g.conic.eval_deriv(s);
    let (ta, tb) = (tangent.dot(axes.along), tangent.dot(axes.across));
    // The motion of `v` less that of `g`'s point: one of them is `A`'s.
    let (mine, sign_t) = match (v.n, g.c.n, g.d.n) {
        (Some(n), _, _) => (n, 1.0),
        (None, Some(c), Some(d)) => (-(c * (1.0 - s) + d * s), -1.0),
        _ => return 1,
    };
    let e = |d: DVec3| {
        let (da, db) = (d.dot(axes.along), d.dot(axes.across));
        (db * ta - da * tb) * tb
    };
    [mine, exact::T2 * sign_t, exact::T3 * sign_t]
        .into_iter()
        .map(|d| sign(e(d)))
        .find(|&x| x != 0)
        .unwrap_or(1)
}

fn sign(x: f64) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// The weight of `conic`'s homogeneous point at `s` (positive).
fn weight(conic: &Conic3, s: f64) -> f64 {
    let r = 1.0 - s;
    r * r + 2.0 * s * r * conic.w + s * s
}

/// The point of `conic` at `s` less `origin`, times the curve's positive
/// weight there (the sign of any component is that of the point's).
fn hom_eval(conic: &Conic3, s: f64, origin: DVec3) -> DVec3 {
    let r = 1.0 - s;
    let (b0, b1, b2) = (r * r, 2.0 * s * r * conic.w, s * s);
    (conic.p0 - origin) * b0 + (conic.c - origin) * b1 + (conic.p1 - origin) * b2
}

//! Where the shadows of two edges cross, seen along [`UP`](super::UP):
//! two conics in the projection plane.
//!
//! The shadow of a rational quadratic curve is a rational quadratic curve
//! in the plane, with homogeneous control points `H0`, `H1`, `H2`. A
//! point `X` (homogeneous) is `λ0·H0 + λ1·H1 + λ2·H2` with `λ = M⁻¹·X`
//! (`M` the matrix of columns `H0, H1, H2`), and it lies on the conic
//! when `λ1² = 4·λ0·λ2`: the curve's own point at `s` has `λ = ((1−s)²,
//! 2s(1−s), s²)`. With the adjugate for the inverse, whose rows are
//! `H1 × H2`, `H2 × H0` and `H0 × H1`, the other curve put in gives a
//! quartic in its parameter, whose roots in `(0, 1)` are isolated in
//! Bernstein form. Where it crosses, `s` comes back from the ratios of
//! `λ`, and the crossing counts if `s` is in `[0, 1]`: the arc, not the
//! rest of its conic. The curvier of the two is the one written
//! implicitly; a shadow that is straight is a line, and gives a
//! quadratic.

use glam::DVec3;

use super::Axes;
use super::bernstein::{self, product2};
use crate::patch::Conic3;

/// A shadow bending less than this, relative to its control points'
/// sizes (`|det M| / (|H0|·|H1|·|H2|)`), is taken as its line.
const STRAIGHT: f64 = 1e-9;

/// A crossing of the shadows of `e` (at `t`) and `h` (at `s`): `sigma`
/// is +1 where `e` crosses `h` from its right to its left seen from
/// `+UP` (the sign of `det[h', e', UP]`), and `dh` how far `e`'s point is
/// above `h`'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ArcCross {
    pub(crate) t: f64,
    pub(crate) s: f64,
    pub(crate) sigma: i8,
    pub(crate) dh: f64,
}

/// Where the shadows of `e` and `h` cross, in order along `e`. Tangencies
/// (a double root) count as no crossing.
pub(crate) fn cross(e: &Conic3, h: &Conic3, axes: &Axes) -> Vec<ArcCross> {
    let bounds = e.bounds().union(h.bounds());
    let origin = (bounds.min + bounds.max) * 0.5;
    let scale = (bounds.max - bounds.min).max_element();
    if !(scale > 0.0 && scale.is_finite()) {
        return Vec::new();
    }
    let (he, hh) = (
        shadow(e, origin, scale, axes),
        shadow(h, origin, scale, axes),
    );
    let pairs = if bend(&hh) >= bend(&he) {
        solve(&he, &hh)
    } else {
        solve(&hh, &he).into_iter().map(|(s, t)| (t, s)).collect()
    };
    let mut out = Vec::with_capacity(pairs.len());
    for (t, s) in pairs {
        let sigma = cross2(tangent(&hh, s), tangent(&he, t));
        if sigma == 0 {
            continue;
        }
        let dh = (e.eval(t) - h.eval(s)).dot(axes.up);
        out.push(ArcCross { t, s, sigma, dh });
    }
    out.sort_by(|x, y| x.t.total_cmp(&y.t));
    out
}

/// The homogeneous control points of `c`'s shadow, `(W·x, W·y, W)` with
/// `x`, `y` its coordinates along the axes from `origin`, over `scale`.
fn shadow(c: &Conic3, origin: DVec3, scale: f64, axes: &Axes) -> [DVec3; 3] {
    let at = |p: DVec3, w: f64| {
        let d = (p - origin) / scale;
        DVec3::new(d.dot(axes.along) * w, d.dot(axes.across) * w, w)
    };
    [at(c.p0, 1.0), at(c.c, c.w), at(c.p1, 1.0)]
}

/// How much a shadow bends: `|det M|` over its control points' sizes.
fn bend(h: &[DVec3; 3]) -> f64 {
    let size = h[0].length() * h[1].length() * h[2].length();
    let det = h[0].dot(h[1].cross(h[2])).abs();
    if size > 0.0 { det / size } else { 0.0 }
}

/// The homogeneous point of a shadow at `s`.
fn at(h: &[DVec3; 3], s: f64) -> DVec3 {
    let r = 1.0 - s;
    h[0] * (r * r) + h[1] * (2.0 * s * r) + h[2] * (s * s)
}

/// A positive multiple of the shadow's tangent at `s`.
fn tangent(h: &[DVec3; 3], s: f64) -> (f64, f64) {
    let x = at(h, s);
    let dx = ((h[1] - h[0]) * (1.0 - s) + (h[2] - h[1]) * s) * 2.0;
    (dx.x * x.z - x.x * dx.z, dx.y * x.z - x.y * dx.z)
}

/// The sign of the 2D cross product `a × b`.
fn cross2(a: (f64, f64), b: (f64, f64)) -> i8 {
    let c = a.0 * b.1 - a.1 * b.0;
    if c > 0.0 {
        1
    } else if c < 0.0 {
        -1
    } else {
        0
    }
}

/// The parameters `(t, s)` where the shadow `sub` (at `t`) meets the
/// shadow `imp` (at `s`, written implicitly).
fn solve(sub: &[DVec3; 3], imp: &[DVec3; 3]) -> Vec<(f64, f64)> {
    let roots = if bend(imp) < STRAIGHT {
        let line = imp[0].cross(imp[2]);
        bernstein::roots(&sub.map(|x| line.dot(x)))
    } else {
        let r = [
            imp[1].cross(imp[2]),
            imp[2].cross(imp[0]),
            imp[0].cross(imp[1]),
        ];
        let lam = r.map(|r| sub.map(|x| r.dot(x)));
        let sq = product2(lam[1], lam[1]);
        let pr = product2(lam[0], lam[2]);
        let quartic: Vec<f64> = (0..5).map(|k| sq[k] - 4.0 * pr[k]).collect();
        bernstein::roots(&quartic)
    };
    roots
        .into_iter()
        .filter_map(|t| on_arc(imp, at(sub, t)).map(|s| (t, s)))
        .collect()
}

/// Where along the shadow `h` the homogeneous point `x`, on its conic, is,
/// if on the arc (`s` in `[0, 1]`).
fn on_arc(h: &[DVec3; 3], x: DVec3) -> Option<f64> {
    let s = if bend(h) < STRAIGHT {
        along_line(h, x)?
    } else {
        let r = [h[1].cross(h[2]), h[2].cross(h[0]), h[0].cross(h[1])];
        let [l0, l1, l2] = r.map(|r| r.dot(x));
        // λ0 : λ1 : λ2 = (1−s)² : 2s(1−s) : s², from the end it is nearer.
        if l0.abs() >= l2.abs() {
            l1 / (l1 + 2.0 * l0)
        } else {
            2.0 * l2 / (l1 + 2.0 * l2)
        }
    };
    (s.is_finite() && (0.0..=1.0).contains(&s)).then_some(s)
}

/// Where along the straight shadow `h` the point `x` is: the root in
/// `[0, 1]` of `h(s) = x` along the chord's longer coordinate, the one
/// whose point is nearest if there are two.
fn along_line(h: &[DVec3; 3], x: DVec3) -> Option<f64> {
    let (p0, p2) = (h[0] / h[0].z, h[2] / h[2].z);
    let k = if (p2.x - p0.x).abs() >= (p2.y - p0.y).abs() {
        0
    } else {
        1
    };
    let c = h.map(|hi| hi[k] * x.z - hi.z * x[k]);
    let mut candidates = bernstein::roots(&c);
    for (end, s) in [(c[0], 0.0), (c[2], 1.0)] {
        if end == 0.0 {
            candidates.push(s);
        }
    }
    let point = x / x.z;
    candidates.into_iter().min_by(|&a, &b| {
        let (pa, pb) = (at(h, a), at(h, b));
        let da = (pa / pa.z - point).length_squared();
        let db = (pb / pb.z - point).length_squared();
        da.total_cmp(&db)
    })
}

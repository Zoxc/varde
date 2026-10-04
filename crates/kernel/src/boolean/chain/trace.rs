//! Tracing where two patches meet, and fitting the traced curve with
//! rational quadratic edges.
//!
//! A point on the curve solves `P(u) = Q(v)` for `u` in one patch's
//! domain and `v` in the other's: three equations in four unknowns, made
//! square by a fourth, that the point lies on a given plane. Newton's
//! method solves the four (the corrector); marching steps along the
//! curve's tangent `n_P × n_Q` and corrects on the plane square to it
//! through the predicted point. The traced points then take conics
//! fitted along their tangents, each checked against the curve at
//! sampled points and split where it strays past the fit tolerance.
//! Only `+ − × ÷ √`, so the answers are the same everywhere.

use glam::DVec3;

use super::super::segment;
use super::super::surface::MAX_TURN_COS;
use crate::patch::{Conic3, Patch, W_MAX, W_MIN};

/// A point where the two patches meet: its position, where it is in each
/// patch (barycentric) and the curve's unit tangent there, along the arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    pub(crate) x: DVec3,
    pub(crate) u: DVec3,
    pub(crate) v: DVec3,
    pub(crate) tan: DVec3,
}

/// How often a fitted conic may be halved.
const MAX_FIT_DEPTH: u32 = 16;

/// The most Newton steps of a solve.
const MAX_NEWTON: usize = 40;

/// The plane each fitted conic lies in, through its chord.
///
/// The hull rule for two patches sharing a curved edge wants the plane
/// through the edge's control points to have one patch on each side. A
/// planar face's cut lies in its plane, which the other face leaves; two
/// curved faces meeting at a crease along the cut are on either side of
/// the plane through the cut's tangent that bisects the crease: spanned
/// by the tangent and the sum of the result's outward normals there.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Crease {
    /// The plane with this unit normal (a planar face's).
    Plane(DVec3),
    /// Bisecting the crease; `sign` is how `q`'s normal counts in the
    /// result's (−1 where its faces are turned over).
    Bisect { sign: f64 },
}

/// The patches of a pair of faces, `p` of `A` and `q` of `B`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pair<'a> {
    pub(crate) p: &'a Patch,
    pub(crate) q: &'a Patch,
    /// About how large they are: the largest side of their boxes.
    pub(crate) scale: f64,
    pub(crate) crease: Crease,
    /// Whether each patch is curved (not planar): whose parametrization
    /// a cut's curves should follow.
    pub(crate) curved: [bool; 2],
}

impl<'a> Pair<'a> {
    pub(crate) fn new(p: &'a Patch, q: &'a Patch, crease: Crease, curved: [bool; 2]) -> Pair<'a> {
        let size = |b: crate::patch::Bounds3| (b.max - b.min).max_element();
        Pair {
            p,
            q,
            scale: size(p.bounds()).max(size(q.bounds())),
            crease,
            curved,
        }
    }

    /// The weight a curve of the cut from `a` to `b` best has for the
    /// triangles along it: that of the curved patches' own curves over
    /// the straight domain segments between the two (their geometric
    /// mean), which the triangles' other sides follow. A weight far from
    /// it reparametrizes the triangle, and its inside leaves the surface.
    fn weight(&self, a: &Point, b: &Point) -> f64 {
        let ws: Vec<f64> = [(self.p, a.u, b.u), (self.q, a.v, b.v)]
            .into_iter()
            .zip(self.curved)
            .filter(|(_, curved)| *curved)
            .filter_map(|((patch, x, y), _)| patch.curve(x, y).ok())
            .map(|c| c.w)
            .collect();
        match ws[..] {
            [w] => w,
            [w0, w1] => (w0 * w1).sqrt(),
            _ => 1.0,
        }
    }

    /// The unit normal of the plane a conic along `chord` through the
    /// curve's point `s` lies in (see [`Crease`]).
    fn plane(&self, s: &Point, chord: DVec3) -> Option<DVec3> {
        match self.crease {
            Crease::Plane(n) => Some(n),
            Crease::Bisect { sign } => {
                let np = self.p.normal(s.u).try_normalize()?;
                let nq = self.q.normal(s.v).try_normalize()?;
                (np + nq * sign).cross(chord).try_normalize()
            }
        }
    }

    /// The point where the patches meet on the plane through `y` square to
    /// `tau`, by Newton's method from `u` and `v`.
    pub(crate) fn solve(
        &self,
        u: DVec3,
        v: DVec3,
        y: DVec3,
        tau: DVec3,
    ) -> Option<(DVec3, DVec3, DVec3)> {
        let (mut u, mut v) = (u, v);
        for _ in 0..MAX_NEWTON {
            let [p, pu, pv] = self.p.eval_derivs(u);
            let [q, qu, qv] = self.q.eval_derivs(v);
            let f = p - q;
            let g = (p - y).dot(tau);
            let rows = [
                [pu.x, pv.x, -qu.x, -qv.x, -f.x],
                [pu.y, pv.y, -qu.y, -qv.y, -f.y],
                [pu.z, pv.z, -qu.z, -qv.z, -f.z],
                [pu.dot(tau), pv.dot(tau), 0.0, 0.0, -g],
            ];
            let d = solve4(rows)?;
            u = bary(u.x + d[0], u.y + d[1]);
            v = bary(v.x + d[2], v.y + d[3]);
            if u.abs().max_element() > 4.0 || v.abs().max_element() > 4.0 {
                return None;
            }
            if d.iter().all(|x| x.abs() <= 1e-15) {
                break;
            }
        }
        let (p, q) = (self.p.eval(u), self.q.eval(v));
        ((p - q).length() <= 1e-11 * self.scale).then(|| ((p + q) * 0.5, u, v))
    }

    /// The curve's unit tangent at `u` and `v`: the patches' normals'
    /// cross product.
    pub(crate) fn tangent(&self, u: DVec3, v: DVec3) -> Option<DVec3> {
        let np = self.p.normal(u).try_normalize()?;
        let nq = self.q.normal(v).try_normalize()?;
        np.cross(nq).try_normalize()
    }

    /// The curve point on the plane through `y` square to `tau`, near
    /// `guess`, with its tangent turned to run along `along`.
    fn point(&self, guess: &Point, y: DVec3, tau: DVec3, along: DVec3) -> Option<Point> {
        let du = domain_step(self.p, guess.u, y - guess.x);
        let dv = domain_step(self.q, guess.v, y - guess.x);
        let (x, u, v) = self.solve(guess.u + du, guess.v + dv, y, tau)?;
        let t = self.tangent(u, v)?;
        let tan = if t.dot(along) < 0.0 { -t } else { t };
        Some(Point { x, u, v, tan })
    }
}

/// The barycentric point with first coordinates `a` and `b`.
fn bary(a: f64, b: f64) -> DVec3 {
    DVec3::new(a, b, 1.0 - a - b)
}

/// Solves the 4 × 4 system whose augmented rows are `m`, by elimination
/// with partial pivoting.
fn solve4(mut m: [[f64; 5]; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let pivot = (col..4)
            .max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))
            .expect("rows");
        if m[pivot][col] == 0.0 || m[pivot][col].is_nan() {
            return None;
        }
        m.swap(col, pivot);
        for r in col + 1..4 {
            let f = m[r][col] / m[col][col];
            for c in col..5 {
                m[r][c] -= f * m[col][c];
            }
        }
    }
    let mut x = [0.0; 4];
    for r in (0..4).rev() {
        let mut s = m[r][4];
        for c in r + 1..4 {
            s -= m[r][c] * x[c];
        }
        x[r] = s / m[r][r];
    }
    x.iter().all(|x| x.is_finite()).then_some(x)
}

/// The change of barycentric position in `patch` at `u` that moves its
/// point by about `d` (least squares on the tangent plane).
pub(crate) fn domain_step(patch: &Patch, u: DVec3, d: DVec3) -> DVec3 {
    let [_, pu, pv] = patch.eval_derivs(u);
    let (a, b, c) = (pu.dot(pu), pu.dot(pv), pv.dot(pv));
    let (e, f) = (pu.dot(d), pv.dot(d));
    let det = a * c - b * b;
    if det <= 0.0 || det.is_nan() {
        return DVec3::ZERO;
    }
    let (x, y) = ((c * e - b * f) / det, (a * f - b * e) / det);
    if !(x.is_finite() && y.is_finite()) {
        return DVec3::ZERO;
    }
    DVec3::new(x, y, -x - y)
}

/// The barycentric position in `patch` of the point `x` on it, by
/// Gauss–Newton from `guess`; `guess` if it doesn't settle.
pub(crate) fn invert(patch: &Patch, x: DVec3, guess: DVec3) -> DVec3 {
    let mut u = guess;
    for _ in 0..MAX_NEWTON {
        let d = domain_step(patch, u, x - patch.eval(u));
        u = bary(u.x + d.x, u.y + d.y);
        if !u.is_finite() || u.abs().max_element() > 4.0 {
            return guess;
        }
        if d.x.abs().max(d.y.abs()) <= 1e-15 {
            break;
        }
    }
    u
}

/// Traces the curve from `a` to `b` (both on it, `a`'s tangent along the
/// arc): the points of a march along it, `a` first and `b` last, or
/// `None` if it strays or runs out of steps.
pub(crate) fn trace(pair: &Pair, a: Point, b: Point) -> Option<Vec<Point>> {
    let span = a.x.distance(b.x);
    let mut h = (pair.scale / 8.0).min(span.max(pair.scale / 64.0) / 2.0);
    let min_h = pair.scale * 1e-9;
    if !(h > 0.0 && h.is_finite()) {
        return None;
    }
    // How far the trace may run before it counts as lost.
    let mut left = 16.0 * (pair.scale + span);
    let mut points = vec![a];
    for _ in 0..crate::MAX_TRACE_STEPS {
        let cur = *points.last().expect("a point");
        let to_b = b.x - cur.x;
        if to_b.length() <= 1.5 * h && to_b.dot(cur.tan) >= 0.0 {
            points.push(b);
            return Some(points);
        }
        let y = cur.x + cur.tan * h;
        let step = pair.point(&cur, y, cur.tan, cur.tan).filter(|n| {
            let turn = n.tan.dot(cur.tan);
            let ahead = (n.x - cur.x).dot(cur.tan) > 0.0;
            let inside = n.u.min_element() >= -0.5 && n.v.min_element() >= -0.5;
            turn >= 0.94 && ahead && inside && n.x.distance(y) <= 0.5 * h
        });
        match step {
            Some(next) => {
                left -= next.x.distance(cur.x);
                if left < 0.0 {
                    return None;
                }
                if next.tan.dot(cur.tan) >= 0.996 {
                    h *= 1.5;
                }
                points.push(next);
            }
            None => {
                h *= 0.5;
                if h < min_h {
                    return None;
                }
            }
        }
    }
    None
}

/// The chain of conics along the traced `points`, each within `fit` of
/// the curve at its sampled points: the curve's points where the conics
/// meet (not the ends), and the conics. `None` if some part can't be
/// fitted.
pub(crate) fn fit(pair: &Pair, points: &[Point], fit: f64) -> Option<(Vec<Point>, Vec<Conic3>)> {
    let (first, last) = (points.first()?, points.last()?);
    let mut inner = Vec::new();
    let mut conics = Vec::new();
    fit_between(
        pair,
        first,
        last,
        &points[1..points.len() - 1],
        fit,
        0,
        &mut inner,
        &mut conics,
    )?;
    Some((inner, conics))
}

#[allow(clippy::too_many_arguments)]
fn fit_between(
    pair: &Pair,
    a: &Point,
    b: &Point,
    between: &[Point],
    fit: f64,
    depth: u32,
    inner: &mut Vec<Point>,
    conics: &mut Vec<Conic3>,
) -> Option<()> {
    let shoulder = shoulder(pair, a, b, between);
    if let Some(s) = &shoulder
        && let Some(conic) = conic_through(pair, a, b, s, between, fit)
    {
        conics.push(conic);
        return Some(());
    }
    if depth >= MAX_FIT_DEPTH {
        return None;
    }
    // Split at the middle traced point, else at the curve's middle.
    let (m, left, right) = if between.is_empty() {
        (shoulder?, &between[..0], &between[..0])
    } else {
        let k = between.len() / 2;
        (between[k], &between[..k], &between[k + 1..])
    };
    fit_between(pair, a, &m, left, fit, depth + 1, inner, conics)?;
    inner.push(m);
    fit_between(pair, &m, b, right, fit, depth + 1, inner, conics)
}

/// The fitted conic `old` from `a` to `b` halved: the curve's point
/// where it crosses the plane through `old`'s middle square to it, and
/// the two conics either side of it along the tangents, each within
/// `fit` where they can be fitted (else `old`'s own halves).
pub(crate) fn split(
    pair: &Pair,
    a: &Point,
    b: &Point,
    old: &Conic3,
    fit: f64,
) -> Option<(Point, [Conic3; 2])> {
    let (y, d) = old.eval_deriv(0.5);
    let tau = d.try_normalize()?;
    let guess = Point {
        x: y,
        u: (a.u + b.u) * 0.5,
        v: (a.v + b.v) * 0.5,
        tan: tau,
    };
    let m = pair.point(&guess, y, tau, tau)?;
    let halves = old.split_half().ok()?;
    let fitted = |p: &Point, q: &Point, fallback: Conic3| {
        shoulder(pair, p, q, &[])
            .and_then(|s| conic_through(pair, p, q, &s, &[], fit))
            .unwrap_or(Conic3 {
                p0: p.x,
                p1: q.x,
                ..fallback
            })
    };
    Some((m, [fitted(a, &m, halves[0]), fitted(&m, b, halves[1])]))
}

/// The traced point nearest the plane through `y` square to `tau`, or
/// `a` or `b` (whichever is nearer) if there is none between them.
fn nearest<'p>(
    a: &'p Point,
    b: &'p Point,
    between: &'p [Point],
    y: DVec3,
    tau: DVec3,
) -> &'p Point {
    let off = |p: &Point| (p.x - y).dot(tau).abs();
    between
        .iter()
        .chain([a, b])
        .min_by(|x, z| off(x).total_cmp(&off(z)))
        .expect("points")
}

/// The curve's point on the plane square to the chord from `a` to `b`
/// through its middle.
fn shoulder(pair: &Pair, a: &Point, b: &Point, between: &[Point]) -> Option<Point> {
    let chord = b.x - a.x;
    let tau = chord.try_normalize()?;
    let m = (a.x + b.x) * 0.5;
    let guess = nearest(a, b, between, m, tau);
    pair.point(guess, m, tau, a.tan + b.tan)
}

/// The conic from `a` to `b` along their tangents (put into the plane
/// [`Pair::plane`] gives, at the curve's point `s` between them) through
/// the curve's point on the line from the chord's middle to the control
/// point, if it stays within `fit` of the curve at sampled points: with
/// the weight the patches' own curves have (see [`Pair::weight`]) if that
/// stays within `fit`, else the one through that point.
fn conic_through(
    pair: &Pair,
    a: &Point,
    b: &Point,
    s: &Point,
    between: &[Point],
    fit: f64,
) -> Option<Conic3> {
    let m = pair.plane(s, b.x - a.x)?;
    let flat = |t: DVec3| (t - m * m.dot(t)).try_normalize();
    let a_in = Point {
        tan: flat(a.tan)?,
        ..*a
    };
    let b_in = Point {
        tan: flat(b.tan)?,
        ..*b
    };
    let shaped = conic_along(&a_in, &b_in, |mid, c| {
        // The plane holding the line from `mid` to `c` and square to the
        // conic's.
        let tau = (c - mid).cross(m).try_normalize()?;
        let on = pair.point(s, mid, tau, a.tan + b.tan)?;
        Some(on.x - m * m.dot(on.x - a.x))
    })?;
    let own = pair.weight(a, b);
    let near = |conic: &Conic3| -> bool {
        [0.25, 0.5, 0.75].into_iter().all(|t| {
            let (x, d) = conic.eval_deriv(t);
            let Some(tau) = d.try_normalize() else {
                return false;
            };
            let guess = nearest(a, b, between, x, tau);
            pair.point(guess, x, tau, tau)
                .is_some_and(|on| on.x.distance(x) <= fit)
        })
    };
    let followed = Conic3 { w: own, ..shaped };
    [followed, shaped]
        .into_iter()
        .filter(|c| (2.0 * W_MIN..=W_MAX / 2.0).contains(&c.w))
        .find(|c| near(c))
}

/// The conic from `a.x` to `b.x` whose control point is where their
/// tangents come closest and whose middle is the point `shoulder` gives
/// (from the chord's middle and the control point) on the line between
/// the two: or a straight segment where both tangents run along the
/// chord.
pub(crate) fn conic_along(
    a: &Point,
    b: &Point,
    shoulder: impl Fn(DVec3, DVec3) -> Option<DVec3>,
) -> Option<Conic3> {
    let d = b.x - a.x;
    let len = d.length();
    if len <= 0.0 || len.is_nan() {
        return None;
    }
    let m = (a.x + b.x) * 0.5;
    let straight = |t: DVec3| t.cross(d).length() <= 1e-9 * len;
    if straight(a.tan) && straight(b.tan) {
        return Some(segment(a.x, b.x));
    }
    // Closest points of a + s·ta and b + r·tb.
    let bb = a.tan.dot(b.tan);
    if bb < MAX_TURN_COS {
        return None;
    }
    let den = 1.0 - bb * bb;
    if den <= 1e-12 || den.is_nan() {
        return None;
    }
    let (e, f) = (a.tan.dot(d), b.tan.dot(d));
    let s_a = (e - bb * f) / den;
    let r_b = (bb * e - f) / den;
    if !(s_a > 0.0 && r_b < 0.0) {
        return None;
    }
    let c = (a.x + a.tan * s_a + b.x + b.tan * r_b) * 0.5;
    let mc = c - m;
    let s = shoulder(m, c)?;
    let sigma = (s - m).dot(mc) / mc.length_squared();
    if !(sigma > 0.0 && sigma < 1.0) {
        return None;
    }
    let w = sigma / (1.0 - sigma);
    (2.0 * W_MIN..=W_MAX / 2.0).contains(&w).then_some(Conic3 {
        p0: a.x,
        c,
        w,
        p1: b.x,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::cylinder_strip;

    /// Two exact cylinder patches crossing square: radius 1 along `z`
    /// and radius 0.7 along `x`, a quarter of each.
    fn crossing() -> (Patch, Patch) {
        let arc = |r: f64, a: DVec3, b: DVec3| Conic3 {
            p0: a * r,
            c: (a + b) * r,
            w: std::f64::consts::FRAC_1_SQRT_2,
            p1: b * r,
        };
        let upright = cylinder_strip(
            &arc(1.0, DVec3::X, DVec3::Y).translated(DVec3::Z * -2.0),
            DVec3::Z * 4.0,
        )
        .unwrap()[0];
        let across = cylinder_strip(
            &arc(0.7, DVec3::Y, DVec3::Z).translated(DVec3::X * -2.0),
            DVec3::X * 4.0,
        )
        .unwrap()[0];
        (upright, across)
    }

    #[test]
    fn solves_and_traces_along_both_cylinders() {
        let (p, q) = crossing();
        let pair = Pair::new(&p, &q, Crease::Bisect { sign: 1.0 }, [true; 2]);
        let start = |y: DVec3, tau: DVec3| {
            let u = invert(&p, y, DVec3::splat(1.0 / 3.0));
            let v = invert(&q, y, DVec3::splat(1.0 / 3.0));
            let (x, u, v) = pair.solve(u, v, y, tau).unwrap();
            let tan = pair.tangent(u, v).unwrap();
            Point { x, u, v, tan }
        };
        // Points of the curve x² + y² = 1, y² + z² = 0.49.
        let on = |p: DVec3| {
            let a = (p.x * p.x + p.y * p.y).sqrt() - 1.0;
            let b = (p.y * p.y + p.z * p.z).sqrt() - 0.7;
            a.abs().max(b.abs())
        };
        let a = start(DVec3::new(0.9, 0.3, 0.6), DVec3::Z);
        assert!(on(a.x) < 1e-12, "{a:?}");
        let mut b = start(DVec3::new(0.8, 0.6, 0.2), DVec3::Z);
        if b.tan.dot(b.x - a.x) < 0.0 {
            b.tan = -b.tan;
        }
        let a = Point {
            tan: if a.tan.dot(b.x - a.x) < 0.0 {
                -a.tan
            } else {
                a.tan
            },
            ..a
        };
        let points = trace(&pair, a, b).unwrap();
        assert!(points.len() > 2);
        for p in &points {
            assert!(on(p.x) < 1e-12, "{p:?}");
        }
        for tol in [1e-3, 1e-6] {
            let (inner, conics) = fit(&pair, &points, tol).unwrap();
            assert_eq!(inner.len() + 1, conics.len());
            for c in &conics {
                for k in 0..=32 {
                    assert!(on(c.eval(k as f64 / 32.0)) < 2.0 * tol);
                }
            }
        }
    }

    #[test]
    fn a_hairpin_whose_ends_are_within_a_step_is_followed() {
        // A plane nearly tangent to a cylinder (radius 1 along `z`) cuts it
        // in a narrow parabola: `y² ≈ 2k·(z_t − z)` with its tip at
        // `z_t = 0.1`. Its ends at `z = −0.9` are 0.009 apart, within the
        // first step, and the arc runs a whole unit up to the tip and back.
        // (The two patches' normals are nearly parallel along it: a pair
        // of operands' patches like these isn't certified to meet in one
        // arc, and is split until the arc in each turns little.)
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Conic3 {
            p0: DVec3::new(s, -s, -1.0),
            c: DVec3::new(2.0 * s, 0.0, -1.0),
            w: s,
            p1: DVec3::new(s, s, -1.0),
        };
        let [p, _] = cylinder_strip(&arc, DVec3::Z * 2.0).unwrap();
        let k = 1e-5;
        let plane = |y: f64, z: f64| DVec3::new(1.0 - 1e-6 + k * z, y, z);
        let q = Patch::flat([plane(-3.0, -3.0), plane(3.0, -3.0), plane(0.0, 3.0)]).unwrap();
        let pair = Pair::new(
            &p,
            &q,
            Crease::Plane(DVec3::new(1.0, 0.0, -k).normalize()),
            [true, false],
        );
        let start = |y: DVec3, tau: DVec3| {
            let u = invert(&p, y, DVec3::splat(1.0 / 3.0));
            let v = invert(&q, y, DVec3::splat(1.0 / 3.0));
            let (x, u, v) = pair.solve(u, v, y, tau).unwrap();
            let tan = pair.tangent(u, v).unwrap();
            Point { x, u, v, tan }
        };
        let y = (2.0 * k).sqrt();
        let mut a = start(DVec3::new(1.0, y, -0.9), DVec3::Y);
        let mut b = start(DVec3::new(1.0, -y, -0.9), DVec3::Y);
        // Up the hairpin from `a`, down it into `b`.
        if a.tan.z < 0.0 {
            a.tan = -a.tan;
        }
        if b.tan.z > 0.0 {
            b.tan = -b.tan;
        }
        assert!(a.x.distance(b.x) < 0.01, "{a:?} {b:?}");
        let on = |x: DVec3| {
            let cyl = (x.x * x.x + x.y * x.y).sqrt() - 1.0;
            let pl = x.x - (1.0 - 1e-6 + k * x.z);
            cyl.abs().max(pl.abs())
        };
        // The march joins the ends at once (the other end is ahead of
        // `a`'s tangent, within its first step), the short way across;
        // fitting finds no conic along the two tangents that follows the
        // curve, and the trace fails, so the chain falls back to curves
        // checked against the true cut. A traced chain, if any, must run
        // up to the tip.
        let points = trace(&pair, a, b).unwrap();
        assert_eq!(points.len(), 2);
        let Some((_, conics)) = fit(&pair, &points, 1e-3) else {
            return;
        };
        let top = conics
            .iter()
            .flat_map(|c| (0..=16).map(|i| c.eval(f64::from(i) / 16.0)))
            .map(|x| x.z)
            .fold(f64::NEG_INFINITY, f64::max);
        for c in &conics {
            for i in 0..=16 {
                assert!(on(c.eval(f64::from(i) / 16.0)) < 1e-3);
            }
        }
        assert!(top > 0.09, "points {}, top {top}", points.len());
    }

    #[test]
    fn inverts_points_of_a_patch() {
        let (p, _) = crossing();
        let u = DVec3::new(0.2, 0.3, 0.5);
        let x = p.eval(u);
        let found = invert(&p, x, DVec3::splat(1.0 / 3.0));
        assert!((found - u).abs().max_element() < 1e-12, "{found}");
    }
}

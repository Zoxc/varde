//! The control-hull rules between two patches.
//!
//! A patch lies in the convex hull of its six control points while its
//! weights are positive. Three rules, by what two patches share, keep the
//! surface from passing through itself:
//!
//! - **Non-neighbours** (no shared vertex): the hulls are more than the
//!   margin apart ([`non_neighbours_apart`], by GJK).
//! - **Edge neighbours**: a plane through the shared edge's three control
//!   points has the other three control points of one patch on one side
//!   and the other patch's on the other ([`edge_neighbours_apart`]).
//! - **Vertex neighbours** (sharing one vertex only): a plane through the
//!   vertex has the other five control points of each on opposite sides
//!   ([`vertex_neighbours_apart`]).
//!
//! A point of a patch is a Bernstein-weighted mean of its control points,
//! so it can lie on the plane only where every control point off the
//! plane has weight zero: on the shared edge, or at the shared vertex.
//! With one patch strictly off the plane, the two can meet only there.
//!
//! "Apart" always means by more than a margin (the resolution), so a
//! check doesn't flip under rounding. Every test here is conservative: it
//! says apart only when it has shown it.

use glam::DVec3;

use crate::patch::Patch;

/// The most GJK steps before giving up (and not calling the hulls apart).
/// Two hulls of six points converge in a handful.
const MAX_GJK_STEPS: usize = 64;

/// Two patches sharing no vertex: their control hulls are more than
/// `margin` apart.
pub(crate) fn non_neighbours_apart(a: &Patch, b: &Patch, margin: f64) -> bool {
    apart(&a.hull(), &b.hull(), margin)
}

/// Two patches sharing corner `ka` of `a`, which is corner `kb` of `b`,
/// and no other: some plane through the corner has the other five control
/// points of `a` more than `margin` on one side and those of `b` more than
/// `margin` on the other.
///
/// A plane through `v` with unit normal `n` does it when `n·x > margin`
/// for every `x` in `{a_i - v} ∪ {v - b_i}`, and the best such `n` clears
/// them by the distance from the origin to their convex hull.
pub(crate) fn vertex_neighbours_apart(
    a: &Patch,
    ka: usize,
    b: &Patch,
    kb: usize,
    margin: f64,
) -> bool {
    let v = a.p[ka];
    let mut points = [DVec3::ZERO; 10];
    let others = |x: &Patch, k: usize| {
        let hull = x.hull();
        let mut out = [DVec3::ZERO; 5];
        // Corner k is hull point 2k.
        for (o, i) in out.iter_mut().zip((0..6).filter(|&i| i != 2 * k)) {
            *o = hull[i];
        }
        out
    };
    for (i, x) in others(a, ka).into_iter().enumerate() {
        points[i] = x - v;
    }
    for (i, x) in others(b, kb).into_iter().enumerate() {
        points[5 + i] = v - x;
    }
    apart(&points, &[DVec3::ZERO], margin)
}

/// Two patches sharing edge `ea` of `a`, which is edge `eb` of `b` (run
/// the other way): a plane through the edge's control points has the
/// other three control points of `a` and those of `b` on opposite sides.
///
/// A straight edge (its control point within `margin` of the line through
/// its ends) leaves the plane free to turn about the line, and both sides
/// must clear it by more than `margin`: the distance from the origin to
/// the hull of `a`'s points and `b`'s mirrored, projected along the line.
/// A curved edge fixes the plane. Then one side must clear it by more than
/// `margin` and the other must not cross it by more than `margin`, so a
/// flat cap whose curved edge meets a wall passes: the cap lies in the
/// plane, and the wall meets the plane only along the edge.
pub(crate) fn edge_neighbours_apart(
    a: &Patch,
    ea: usize,
    b: &Patch,
    eb: usize,
    margin: f64,
) -> bool {
    let (p, c, q) = (a.p[ea], a.c[ea], a.p[(ea + 1) % 3]);
    let others = |x: &Patch, e: usize| [x.p[(e + 2) % 3], x.c[(e + 1) % 3], x.c[(e + 2) % 3]];
    let (oa, ob) = (others(a, ea), others(b, eb));
    let Some(u) = (q - p).try_normalize() else {
        return false;
    };
    let across = |x: DVec3| {
        let d = x - p;
        d - u * d.dot(u)
    };
    let bulge = across(c);
    if bulge.length() <= margin {
        let mut points = [DVec3::ZERO; 6];
        for i in 0..3 {
            points[i] = across(oa[i]);
            points[3 + i] = -across(ob[i]);
        }
        return apart(&points, &[DVec3::ZERO], margin);
    }
    let n = u.cross(bulge).normalize();
    let side = |x: DVec3| n.dot(x - p);
    let (sa, sb) = (oa.map(side), ob.map(side));
    [1.0, -1.0].into_iter().any(|sign: f64| {
        let a_min = sa.iter().map(|&s| sign * s).fold(f64::INFINITY, f64::min);
        let b_max = sb
            .iter()
            .map(|&s| sign * s)
            .fold(f64::NEG_INFINITY, f64::max);
        (a_min > margin && b_max < margin) || (a_min > -margin && b_max < -margin)
    })
}

/// Whether the convex hulls of `a` and `b` (neither empty) are more than
/// `margin` apart, by GJK on their Minkowski difference.
///
/// Each step has the closest point `v` of the current simplex, a point of
/// the difference and so an upper bound on the distance, and the support
/// point `w` furthest along `-v`, which gives the lower bound `v·w / |v|`.
/// It answers apart only once the lower bound clears `margin`, and not
/// apart once `|v|` is within it, or when it stops making progress.
pub(crate) fn apart(a: &[DVec3], b: &[DVec3], margin: f64) -> bool {
    // Measured from a point of `a`, so rounding is relative to the hulls'
    // size and distance, not their distance from the model's origin.
    let origin = a[0];
    let furthest = |points: &[DVec3], d: DVec3| {
        let mut best = points[0] - origin;
        let mut best_dot = best.dot(d);
        for &p in &points[1..] {
            let p = p - origin;
            let dot = p.dot(d);
            if dot > best_dot {
                (best, best_dot) = (p, dot);
            }
        }
        best
    };
    let support = |d: DVec3| furthest(a, d) - furthest(b, -d);

    let mut simplex = Simplex {
        points: [a[0] - b[0]; 4],
        len: 1,
    };
    let mut v = simplex.points[0];
    for _ in 0..MAX_GJK_STEPS {
        let vv = v.dot(v);
        // Also stops on NaN.
        if vv.is_nan() || vv <= margin * margin {
            return false;
        }
        let w = support(-v);
        let vw = v.dot(w);
        if vw > margin * vv.sqrt() {
            return true;
        }
        if vv - vw <= 1e-12 * vv || simplex.points[..simplex.len].contains(&w) {
            return false;
        }
        simplex.points[simplex.len] = w;
        simplex.len += 1;
        (v, simplex) = simplex.closest();
        if simplex.len == 4 {
            // The origin is inside the tetrahedron.
            return false;
        }
    }
    false
}

/// Up to four points of the Minkowski difference.
#[derive(Debug, Clone, Copy)]
struct Simplex {
    points: [DVec3; 4],
    len: usize,
}

impl Simplex {
    /// The point of the simplex's hull closest to the origin, and the
    /// smallest face of the simplex holding it.
    ///
    /// Every face (every non-empty subset of affinely independent points)
    /// is tried: the origin's projection onto the face's affine hull,
    /// kept when its barycentric coordinates are all non-negative. The
    /// closest point lies inside some face and is that face's projection,
    /// so the nearest projection kept is it. Subsets that are nearly
    /// degenerate are skipped; their points are covered by smaller faces.
    /// Ties keep the first found, smaller faces first.
    fn closest(&self) -> (DVec3, Simplex) {
        let mut best: Option<(f64, DVec3, Simplex)> = None;
        for mask in 1..(1u32 << self.len) {
            let mut face = Simplex {
                points: [DVec3::ZERO; 4],
                len: 0,
            };
            for (i, &p) in self.points[..self.len].iter().enumerate() {
                if mask & (1 << i) != 0 {
                    face.points[face.len] = p;
                    face.len += 1;
                }
            }
            if let Some(q) = face.projection() {
                let qq = q.dot(q);
                let better = match &best {
                    Some((d, _, s)) => qq < *d || (qq == *d && face.len < s.len),
                    None => true,
                };
                if better {
                    best = Some((qq, q, face));
                }
            }
        }
        // A single point always projects to itself.
        let (_, q, face) = best.expect("a closest point");
        (q, face)
    }

    /// The origin's projection onto the affine hull of the points, if its
    /// barycentric coordinates are all non-negative and the points are
    /// clearly affinely independent.
    fn projection(&self) -> Option<DVec3> {
        let x0 = self.points[0];
        let e = [1, 2, 3].map(|i| self.points[i.min(self.len - 1)] - x0);
        // Solve G·μ = r, G the Gram matrix of the edges from x0, r_i =
        // -e_i·x0; the point is x0 + Σ μ_i·e_i.
        let g = |i: usize, j: usize| e[i].dot(e[j]);
        let r = |i: usize| -e[i].dot(x0);
        let mu: [f64; 3] = match self.len {
            1 => return Some(x0),
            2 => {
                let g00 = g(0, 0);
                if g00.is_nan() || g00 <= 0.0 {
                    return None;
                }
                [r(0) / g00, 0.0, 0.0]
            }
            3 => {
                let (g00, g01, g11) = (g(0, 0), g(0, 1), g(1, 1));
                let det = g00 * g11 - g01 * g01;
                if det.is_nan() || det <= 1e-12 * g00 * g11 {
                    return None;
                }
                let (r0, r1) = (r(0), r(1));
                [
                    (r0 * g11 - r1 * g01) / det,
                    (g00 * r1 - g01 * r0) / det,
                    0.0,
                ]
            }
            _ => {
                let m = glam::DMat3::from_cols(
                    DVec3::new(g(0, 0), g(1, 0), g(2, 0)),
                    DVec3::new(g(0, 1), g(1, 1), g(2, 1)),
                    DVec3::new(g(0, 2), g(1, 2), g(2, 2)),
                );
                let det = m.determinant();
                if det.is_nan() || det <= 1e-12 * g(0, 0) * g(1, 1) * g(2, 2) {
                    return None;
                }
                let rhs = DVec3::new(r(0), r(1), r(2));
                // Cramer's rule.
                let solve = |k: usize| {
                    let mut cols = [m.x_axis, m.y_axis, m.z_axis];
                    cols[k] = rhs;
                    glam::DMat3::from_cols(cols[0], cols[1], cols[2]).determinant() / det
                };
                [solve(0), solve(1), solve(2)]
            }
        };
        let k = self.len - 1;
        let first = 1.0 - mu[..k].iter().sum::<f64>();
        if first < 0.0 || mu[..k].iter().any(|&m| m < 0.0) {
            return None;
        }
        Some((0..k).fold(x0, |q, i| q + e[i] * mu[i]))
    }
}

#[cfg(test)]
mod tests;

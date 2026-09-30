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
    let Some((u, across)) = across_line(p, q) else {
        return false;
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

/// The unit direction from `p` to `q`, and the part of `x - p` across the
/// line through them, or `None` if they are the same point.
fn across_line(p: DVec3, q: DVec3) -> Option<(DVec3, impl Fn(DVec3) -> DVec3)> {
    let u = (q - p).try_normalize()?;
    Some((u, move |x: DVec3| {
        let d = x - p;
        d - u * d.dot(u)
    }))
}

/// Whether every edge's control point is within `margin` of the line
/// through its ends: the patch is then within `margin` of its flat
/// triangle, and the hull rules on it are about the triangle itself.
pub(crate) fn flat(patch: &Patch, margin: f64) -> bool {
    (0..3).all(|i| {
        across_line(patch.p[i], patch.p[(i + 1) % 3])
            .is_some_and(|(_, across)| across(patch.c[i]).length() <= margin)
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
    /// smallest face of the simplex holding it. The last point is the
    /// support point just added, and the face holds it.
    ///
    /// Every face holding the last point (every subset of affinely
    /// independent points with it) is tried: the origin's projection onto
    /// the face's affine hull, kept when its barycentric coordinates are
    /// all non-negative. The closest point lies inside some face and is
    /// that face's projection, so the nearest projection kept is it.
    /// Subsets that are nearly degenerate are skipped; their points are
    /// covered by smaller faces. Ties keep the first found, smaller faces
    /// first.
    ///
    /// Only faces with the last point: the support point `w` was added
    /// because `v·w < v·v`, so the segment from the old closest point `v`
    /// towards `w` comes closer to the origin, and the new closest point
    /// is on a face with `w` (faces without it are the old simplex, no
    /// closer than `v`). When `w` is square to `v` and far out, the
    /// segment comes closer by less than the rounding of a squared length;
    /// trying the old faces too then kept `v` and dropped `w`, and the
    /// next step found `w` again, until GJK gave up.
    fn closest(&self) -> (DVec3, Simplex) {
        let last = 1u32 << (self.len - 1);
        let mut best: Option<(f64, DVec3, Simplex)> = None;
        for mask in (1..(1u32 << self.len)).filter(|mask| mask & last != 0) {
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
        // The last point alone always projects to itself.
        let (_, q, face) = best.expect("a closest point");
        (q, face)
    }

    /// The origin's projection onto the affine hull of the points, if its
    /// barycentric coordinates are all non-negative and the points are
    /// clearly affinely independent.
    ///
    /// Triangles and tetrahedra are solved with cross and triple products
    /// of the edges, not the Gram matrix: that squares the conditioning,
    /// and on a long thin triangle (a hull thousands of times longer than
    /// it is wide) lost enough digits to put the point off the closest, so
    /// that GJK stopped short and called hulls far apart not apart.
    fn projection(&self) -> Option<DVec3> {
        let x0 = self.points[0];
        let e = [1, 2, 3].map(|i| self.points[i.min(self.len - 1)] - x0);
        match self.len {
            1 => Some(x0),
            2 => {
                let g = e[0].dot(e[0]);
                if g.is_nan() || g <= 0.0 {
                    return None;
                }
                let mu = -e[0].dot(x0) / g;
                (0.0..=1.0).contains(&mu).then(|| x0 + e[0] * mu)
            }
            3 => {
                // With n = e0 × e1, x0 + μ0·e0 + μ1·e1 is the projection
                // for μ0 = -n·(x0 × e1)/n² and μ1 = -n·(e0 × x0)/n².
                let n = e[0].cross(e[1]);
                let nn = n.dot(n);
                if nn.is_nan() || nn <= DEGENERATE * DEGENERATE * e[0].dot(e[0]) * e[1].dot(e[1]) {
                    return None;
                }
                let mu0 = -n.dot(x0.cross(e[1])) / nn;
                let mu1 = -n.dot(e[0].cross(x0)) / nn;
                if mu0 < 0.0 || mu1 < 0.0 || mu0 + mu1 > 1.0 {
                    return None;
                }
                // The point itself straight from the normal, which rounds
                // relative to its distance rather than to x0's.
                Some(n * (n.dot(x0) / nn))
            }
            _ => {
                // x0 + Σ μi·ei = 0 by Cramer's rule on the edges.
                let det = e[0].dot(e[1].cross(e[2]));
                let size = e[0].length() * e[1].length() * e[2].length();
                if det.is_nan() || det.abs() <= DEGENERATE * size {
                    return None;
                }
                let r = -x0;
                let mu = [
                    r.dot(e[1].cross(e[2])) / det,
                    e[0].dot(r.cross(e[2])) / det,
                    e[0].dot(e[1].cross(r)) / det,
                ];
                if mu.iter().any(|&m| m < 0.0) || mu.iter().sum::<f64>() > 1.0 {
                    return None;
                }
                Some(DVec3::ZERO)
            }
        }
    }
}

/// Faces of a simplex flatter than this (the sine of the angle between
/// two edges, or the volume of three over their lengths' product) are
/// skipped: their points lie within that much of a smaller face.
const DEGENERATE: f64 = 1e-12;

#[cfg(test)]
mod tests;

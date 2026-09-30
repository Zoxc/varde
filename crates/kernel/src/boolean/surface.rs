//! The exact paths the face tags select: what surface a patch lies on,
//! where an edge crosses a plane or a quadric, and the conic in which a
//! plane cuts a quadric.
//!
//! A plane cut of a quadric is a conic, which a rational quadratic edge
//! holds exactly: its ends, its control point where the end tangents
//! meet, and its weight from the point where the line from the chord's
//! middle to the control point meets the curve (the curve's point at
//! `½` is `(M + w·C) / (1 + w)`). Only `+ − × ÷ √`, so the bits are the
//! same everywhere.

use glam::DVec3;

use super::input::Input;
use super::segment;
use crate::mesh::{Quadric, Surface};
use crate::patch::{Conic3, W_MAX};

/// What a patch is known to lie on, exactly.
#[derive(Debug, Clone, Copy)]
pub(super) enum Shape {
    /// The plane `n·x = d`, `n` a unit vector.
    Plane {
        n: DVec3,
        d: f64,
    },
    Quadric(Quadric),
    Other,
}

impl Shape {
    /// What triangle `t` of `input` lies on: the plane of its face's tag
    /// (or of its corners, untagged) for a planar patch, its face's
    /// quadric, or nothing known.
    pub(super) fn of(input: &Input, t: u32) -> Shape {
        let surface = input.mesh.faces()[input.face(t) as usize].surface;
        if input.planar[t as usize] {
            if let Surface::Plane { n, d } = surface {
                let scale = n.abs().max_element();
                if scale > 0.0 && scale.is_finite() {
                    let (n, d) = (n / scale, d / scale);
                    let len = n.length();
                    return Shape::Plane {
                        n: n / len,
                        d: d / len,
                    };
                }
            }
            let [p0, p1, p2] = input.corners(t);
            return match (p1 - p0).cross(p2 - p0).try_normalize() {
                Some(n) => Shape::Plane { n, d: n.dot(p0) },
                None => Shape::Other,
            };
        }
        match surface {
            Surface::Quadric(q) => Shape::Quadric(q),
            _ => Shape::Other,
        }
    }

    /// How far `x` is from the surface: infinite where nothing is known.
    pub(super) fn distance(&self, x: DVec3) -> f64 {
        match self {
            Shape::Plane { n, d } => (n.dot(x) - d).abs(),
            Shape::Quadric(q) => q.distance(x),
            Shape::Other => f64::INFINITY,
        }
    }

    /// `x` moved onto the plane, for a plane; as it is otherwise.
    pub(super) fn onto(&self, x: DVec3) -> DVec3 {
        match *self {
            Shape::Plane { n, d } => x - n * (n.dot(x) - d),
            _ => x,
        }
    }
}

/// How far along an edge (in its parameter) a crossing of a quadric may
/// move when its position is solved again: past this Newton's method
/// found another root, and the position stays.
const MAX_SHIFT: f64 = 1e-6;

/// The parameter near `t` where `conic` crosses `shape`: solved exactly
/// for a plane (a quadratic: the root in the edge nearest `t`, however
/// far, so the vertex lies on the plane whose tag its triangles keep,
/// even where the search for the crossing found none and `t` is only
/// where the two came closest), by Newton's method for a quadric, and
/// `t` as it is for anything else, or where nothing is found near it.
pub(super) fn polish(conic: &Conic3, t: f64, shape: &Shape) -> f64 {
    let found = match *shape {
        Shape::Plane { n, d } => {
            let side = |x: DVec3| n.dot(x) - d;
            let (f0, f1, f2) = (side(conic.p0), conic.w * side(conic.c), side(conic.p1));
            // f0·(1−t)² + 2f1·t(1−t) + f2·t² in powers of t.
            let (a, b, c) = (f0 - 2.0 * f1 + f2, 2.0 * (f1 - f0), f0);
            // A root a rounding off an end (a crossing at the edge's
            // end, on the plane) is at that end.
            return quadratic_roots(a, b, c)
                .into_iter()
                .flatten()
                .filter(|s| (-1e-9..=1.0 + 1e-9).contains(s))
                .map(at_end)
                .min_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()))
                .unwrap_or(t);
        }
        Shape::Quadric(q) => {
            let mut s = t;
            for _ in 0..32 {
                let (x, dx) = conic.eval_deriv(s);
                let g = q.gradient(x).dot(dx);
                let step = q.value(x) / g;
                if !step.is_finite() {
                    break;
                }
                s -= step;
                if step.abs() <= 1e-16 {
                    break;
                }
            }
            s.is_finite().then_some(s)
        }
        Shape::Other => None,
    };
    match found {
        Some(s) if (s - t).abs() <= MAX_SHIFT && (-1e-9..=1.0 + 1e-9).contains(&s) => at_end(s),
        _ => t,
    }
}

/// A parameter `s` near `[0, 1]` in it, and one within [`END`] of an end
/// at that end: a crossing at an edge's end (where a vertex of one
/// operand lies on the other's surface) placed at the vertex to the bit,
/// so the vertices of the tie coincide exactly.
fn at_end(s: f64) -> f64 {
    if s < END {
        0.0
    } else if s > 1.0 - END {
        1.0
    } else {
        s
    }
}

/// How near an end of an edge (in its parameter) a crossing is put at
/// that end.
const END: f64 = 1e-12;

/// The real roots of `a·t² + b·t + c`, by the stable formula; a linear
/// equation where `a` vanishes.
fn quadratic_roots(a: f64, b: f64, c: f64) -> [Option<f64>; 2] {
    if a == 0.0 {
        return [(b != 0.0).then(|| -c / b), None];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 || disc.is_nan() {
        return [None, None];
    }
    let q = -0.5 * (b + disc.sqrt().copysign(b));
    if q == 0.0 {
        return [Some(0.0), None];
    }
    [Some(q / a), Some(c / q)]
}

/// Where the line through `v` (on the quadric) along `dir` meets `q`
/// again, if it does: the unit direction and the reciprocal `t` of the
/// distance along it, so the point is `v + dir / t`. `t` is 0 where the
/// line meets `q` again only at infinity: along the axis of a parabolic
/// cylinder, where the planes of all the curves of a patch on it meet
/// (their common point is at infinity). Solved in `t` (`c·t² + b·t + a =
/// 0` for `F(v + s·dir) = a·s² + b·s + c`), whose root nearest 0 is well
/// conditioned however far the point: in `s` it was the huge root of a
/// quadratic whose leading term was all rounding, and put a band's cut
/// through a point off in a random direction.
pub(super) fn second_point(q: &Quadric, v: DVec3, dir: DVec3) -> Option<(DVec3, f64)> {
    let dir = dir.try_normalize()?;
    let a = dir.dot(q.a * dir);
    let b = q.gradient(v).dot(dir);
    let c = q.value(v);
    // The root nearer 0; the other, near infinity, is `v` itself.
    let t = quadratic_roots(c, b, a)
        .into_iter()
        .flatten()
        .min_by(|x, y| x.abs().total_cmp(&y.abs()))?;
    t.is_finite().then_some((dir, t))
}

/// Which of the two points where a chord's bisector meets the conic an
/// arc goes through: the one nearer a point, or the one on the other side
/// of the chord from a point.
#[derive(Debug, Clone, Copy)]
pub(super) enum Guide {
    Near(DVec3),
    Away(DVec3),
}

/// How often [`section`] may halve an arc.
const MAX_SECTION_DEPTH: u32 = 6;

/// The cosine of the most one cut's curve may turn between its ends:
/// about 45°. Short arcs bulge little off their chords, which the faces
/// they cut are triangulated along.
pub(super) const MAX_TURN_COS: f64 = 0.7;

/// The arc of the conic in which the plane through `x` and `y` with unit
/// normal `n` cuts `q`, from `x` to `y` (both on it), through the point
/// `guide` picks: as exact rational quadratic arcs, one or more (halved
/// where one would turn by more than about 45°), or `None` where the
/// plane doesn't cut `q` in a curve through them there.
pub(super) fn section(
    q: &Quadric,
    n: DVec3,
    x: DVec3,
    y: DVec3,
    guide: Guide,
) -> Option<Vec<Conic3>> {
    let mut out = Vec::new();
    section_into(q, n, x, y, guide, 0, &mut out)?;
    Some(out)
}

fn section_into(
    q: &Quadric,
    n: DVec3,
    x: DVec3,
    y: DVec3,
    guide: Guide,
    depth: u32,
    out: &mut Vec<Conic3>,
) -> Option<()> {
    let chord = y - x;
    let len = chord.length();
    if len <= 0.0 || len.is_nan() {
        return None;
    }
    let (tx, ty) = (n.cross(q.gradient(x)), n.cross(q.gradient(y)));
    let (lx, ly) = (tx.length(), ty.length());
    if !(lx > 0.0 && ly > 0.0) {
        return None;
    }
    let m = (x + y) * 0.5;
    // Straight: both tangents along the chord (a plane along a
    // cylinder's rulings).
    let along = |t: DVec3, l: f64| t.cross(chord).length() <= 1e-9 * l * len;
    if along(tx, lx) && along(ty, ly) {
        return (q.distance(m) <= 1e-9 * len).then(|| out.push(segment(x, y)));
    }
    // Across the chord, in the plane: which side of it the arc is on.
    let across = n.cross(chord);
    let side = |p: DVec3| (p - m).dot(across);
    // The points where the line from `m` along `dir` meets the conic,
    // by how far along.
    let meets = |dir: DVec3| {
        let roots = quadratic_roots(dir.dot(q.a * dir), q.gradient(m).dot(dir), q.value(m));
        roots.into_iter().flatten().collect::<Vec<f64>>()
    };
    let split = |at: DVec3, out: &mut Vec<Conic3>| -> Option<()> {
        if depth >= MAX_SECTION_DEPTH {
            return None;
        }
        section_into(q, n, x, at, Guide::Away(y), depth + 1, out)?;
        section_into(q, n, at, y, Guide::Away(x), depth + 1, out)
    };
    // Which side of the chord the arc wanted is on.
    let wanted = |bulge: f64| match guide {
        Guide::Near(g) => side(g) * bulge > 0.0,
        Guide::Away(g) => side(g) * bulge < 0.0,
    };
    let den = tx.cross(ty).dot(n);
    if den.abs() <= 1e-9 * lx * ly || den.is_nan() {
        // Tangents parallel: half the conic. Split where the chord's
        // bisector meets the side wanted.
        let at = meets(across)
            .into_iter()
            .map(|s| m + across * s)
            .find(|&p| wanted(side(p)))?;
        return split(at, out);
    }
    // Where the tangents meet, in the plane.
    let c = x + tx * (chord.cross(ty).dot(n) / den);
    let mc = c - m;
    let roots = meets(mc);
    if !wanted(side(c)) {
        // The arc on the other side from the control point: split at
        // its point on the line through `m` and `c`, behind `m`.
        let behind = roots
            .iter()
            .copied()
            .filter(|&s| s < 0.0)
            .fold(f64::NEG_INFINITY, f64::max);
        return if behind.is_finite() {
            split(m + mc * behind, out)
        } else {
            None
        };
    }
    // The curve's middle is where that line meets it between `m` and
    // `c`: `(m + w·c) / (1 + w)`.
    let sigma = roots.into_iter().find(|&s| s > 0.0 && s < 1.0)?;
    let w = sigma / (1.0 - sigma);
    let (a, b) = (c - x, y - c);
    if !(0.5..=W_MAX).contains(&w) || a.dot(b) < MAX_TURN_COS * a.length() * b.length() {
        return split(m + mc * sigma, out);
    }
    out.push(Conic3 { p0: x, c, w, p1: y });
    Some(())
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_cuts_a_cylinder_in_exact_ellipse_arcs() {
        let q = Quadric::cylinder(DVec3::new(0.3, -0.2, 0.0), DVec3::Z, 2.0).unwrap();
        // A tilted plane through the axis point at height 1.
        let n = DVec3::new(0.3, 0.1, 1.0).normalize();
        let d = n.dot(DVec3::new(0.3, -0.2, 1.0));
        let on = |angle: f64| {
            let (s, c) = angle.sin_cos();
            let xy = DVec3::new(0.3 + 2.0 * c, -0.2 + 2.0 * s, 0.0);
            // Height where the vertical line meets the plane.
            xy + DVec3::Z * ((d - n.dot(xy)) / n.z)
        };
        for (a0, a1) in [(0.1, 1.2), (0.0, 3.0), (-2.0, 2.5)] {
            let (x, y) = (on(a0), on(a1));
            let arcs = section(&q, n, x, y, Guide::Near(on((a0 + a1) / 2.0))).unwrap();
            assert!(!arcs.is_empty());
            assert_eq!(arcs[0].p0, x);
            assert_eq!(arcs.last().unwrap().p1, y);
            for arc in &arcs {
                for k in 0..=16 {
                    let p = arc.eval(k as f64 / 16.0);
                    assert!(q.distance(p) < 1e-12, "{p}");
                    assert!((n.dot(p) - d).abs() < 1e-12, "{p}");
                }
                assert!(arc.w >= 0.5);
            }
            // The arc through the guide, not the other one.
            let mid = arcs[arcs.len() / 2].p0;
            let angle = (mid.y + 0.2).atan2(mid.x - 0.3);
            assert!(angle > a0 - 1e-9 && angle < a1 + 1e-9, "{angle}");
        }
    }

    #[test]
    fn crossings_solve_exactly() {
        let arc = Conic3 {
            p0: DVec3::new(1.0, 0.0, 0.0),
            c: DVec3::new(1.0, 1.0, 3.0),
            w: std::f64::consts::FRAC_1_SQRT_2,
            p1: DVec3::new(0.0, 1.0, 3.0),
        };
        // Roughly where it crosses, by bisection, then a little off.
        let near = |f: &dyn Fn(f64) -> f64| {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if f(lo) * f(mid) <= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            lo + 1e-8
        };
        let plane = Shape::Plane {
            n: DVec3::Z,
            d: 1.0,
        };
        let t = polish(&arc, near(&|t| arc.eval(t).z - 1.0), &plane);
        assert!((arc.eval(t).z - 1.0).abs() < 1e-15);
        let q = Quadric::cylinder(DVec3::ZERO, DVec3::X, 0.9).unwrap();
        let t = polish(&arc, near(&|t| q.value(arc.eval(t))), &Shape::Quadric(q));
        assert!(q.distance(arc.eval(t)) < 1e-15);
        // Far from it: on the plane all the same.
        let t = polish(&arc, 0.9, &plane);
        assert!((arc.eval(t).z - 1.0).abs() < 1e-15);
        // Nowhere in the edge: left as it is.
        let above = Shape::Plane {
            n: DVec3::Z,
            d: 5.0,
        };
        assert_eq!(polish(&arc, 0.9, &above), 0.9);
        // At the edge's end, on the plane: the end itself, to the bit,
        // however the root rounds.
        let end = Shape::Plane {
            n: DVec3::Z,
            d: 3.0,
        };
        assert_eq!(polish(&arc, 0.999_999, &end), 1.0);
        assert_eq!(arc.eval(polish(&arc, 0.999_999, &end)), arc.p1);
    }

    #[test]
    fn second_points() {
        let q = Quadric::cylinder(DVec3::ZERO, DVec3::Z, 1.0).unwrap();
        let (dir, t) = second_point(&q, DVec3::X, DVec3::new(-1.0, 1.0, 0.3)).unwrap();
        let o = DVec3::X + dir / t;
        assert!((o - DVec3::new(0.0, 1.0, 0.3)).length() < 1e-15, "{o}");
        // Along a parabolic cylinder's axis: at infinity, however the
        // leading term rounds.
        let p = Quadric {
            origin: DVec3::ZERO,
            a: glam::DMat3::from_diagonal(DVec3::new(1.0, 0.0, 0.0)),
            b: DVec3::new(0.0, -1.0, 0.0),
            c: 0.0,
        };
        let v = DVec3::new(2.0, 2.0, 1.0);
        assert!(p.distance(v) == 0.0);
        let (_, t) = second_point(&p, v, DVec3::new(1e-17, 1.0, 0.0)).unwrap();
        assert!(t.abs() < 1e-15, "{t}");
        // Along the axis: no second point.
        assert!(second_point(&q, DVec3::X, DVec3::Z).is_none());
    }
}

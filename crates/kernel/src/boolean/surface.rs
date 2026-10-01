//! The exact paths the face tags select: what surface a patch lies on,
//! where an edge crosses a plane or a quadric, and the conic in which a
//! plane cuts a quadric.
//!
//! A plane cut of a quadric is a conic, which a rational quadratic edge
//! holds exactly: its ends, its control point where the end tangents
//! meet, and its weight from the point where the line from the chord's
//! middle to the control point meets the curve (the curve's point at
//! `½` is `(M + w·C) / (1 + w)`), or on an elliptic cylinder from the
//! arc's angle about the axis. Only `+ − × ÷ √`, so the bits are the
//! same everywhere.

use glam::{DMat3, DVec3};

use super::curved::bernstein;
use super::curved::solve::near_patch;
use super::input::Input;
use super::segment;
use crate::mesh::{Quadric, Surface};
use crate::patch::{Conic3, Patch, Point, W_MAX};

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

/// How far along an edge (in its parameter) a crossing the search solved
/// may move onto a quadric with no further check: that near, the root is
/// the one the search found.
const MAX_SHIFT: f64 = 1e-6;

/// What a crossing's new position is checked against where it moves
/// further than [`polish`] trusts: the patch it crosses, its sign (+1
/// where the edge enters the patch's solid, −1 where it leaves), whether
/// the search solved it or only placed it for the count, and the
/// resolution.
pub(super) struct Crossed<'a> {
    pub(super) patch: &'a Patch,
    pub(super) x: i8,
    pub(super) solved: bool,
    pub(super) resolution: f64,
}

/// The parameter near `t` where `conic` crosses `shape`, and how many
/// pieces of the patch crossed checking it looked at.
///
/// Every root of the edge against the plane or quadric (a quadratic for a
/// plane or a straight edge, a quartic in Bernstein form for a conic on a
/// quadric), in or a rounding outside the edge, is a candidate, nearest
/// `t` first:
///
/// - Where the search solved the crossing, the nearest root on a plane
///   (however far: `t` is a point of the patch, which is the plane), or
///   on a quadric within [`MAX_SHIFT`] of `t`.
/// - Otherwise (a crossing only placed for the count, at the middle of
///   the smallest pieces the search looked at, or a solved one whose
///   quadric root is further), the nearest root with the crossing's sign
///   whose point is within the resolution of the patch crossed
///   ([`near_patch`], certified): this pair's crossing, not another
///   patch's or the edge's way back out. A placed crossing lay up to
///   `1.6e-3` of its edge from its root, `5.6e-4` off the cylinder it
///   crossed.
/// - Where no root qualifies, as before: the nearest root on a plane, one
///   within [`MAX_SHIFT`] on a quadric, else `t`; the check on crossings
///   only placed (in `assemble`) then decides.
///
/// For anything else, `t` as it is.
pub(super) fn polish(conic: &Conic3, t: f64, shape: &Shape, crossed: &Crossed) -> (f64, usize) {
    let mut roots = roots(conic, shape);
    roots.sort_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()).then(x.total_cmp(y)));
    let Some(&nearest) = roots.first() else {
        return (t, 0);
    };
    let trusted = match shape {
        Shape::Plane { .. } => crossed.solved,
        Shape::Quadric(_) => crossed.solved && (nearest - t).abs() <= MAX_SHIFT,
        Shape::Other => false,
    };
    if trusted {
        return (nearest, 0);
    }
    let mut nodes = 0;
    if let Some(facing) = facing(shape, crossed.patch) {
        for &s in &roots {
            if !sign_fits(conic, s, shape, facing, crossed.x) {
                continue;
            }
            let (near, n) = near_patch(point(conic, s), crossed.patch, crossed.resolution);
            nodes += n;
            if near.is_some() {
                return (s, nodes);
            }
        }
    }
    let fallback = match shape {
        Shape::Plane { .. } => nearest,
        _ if (nearest - t).abs() <= MAX_SHIFT => nearest,
        _ => t,
    };
    (fallback, nodes)
}

/// The point of `conic` at `s`, as the crossings' vertices are placed:
/// by interpolation on an exactly straight edge, else by [`on_curve`].
pub(super) fn point(conic: &Conic3, s: f64) -> DVec3 {
    if straight(conic) {
        lerp(conic.p0, conic.p1, s)
    } else {
        on_curve(conic, s)
    }
}

/// The point of `conic` at `t`, from its blossom, so the ends of pieces
/// split there are it to the bit.
pub(super) fn on_curve(conic: &Conic3, t: f64) -> DVec3 {
    let h = conic.blossom(t, t);
    (h.truncate() / h.w).shared(&conic.hull())
}

/// The point at `t` from `s` to `e`, exactly `s` at 0 and `e` at 1.
pub(super) fn lerp(s: DVec3, e: DVec3, t: f64) -> DVec3 {
    if t <= 0.5 {
        s + (e - s) * t
    } else {
        e + (s - e) * (1.0 - t)
    }
}

/// Whether `conic` is exactly the segment between its ends, as
/// [`segment`] makes them.
pub(super) fn straight(conic: &Conic3) -> bool {
    conic.w == 1.0 && conic.c == (conic.p0 + conic.p1) * 0.5
}

/// Which way `shape`'s gradient points against `patch`'s outward normal
/// (+1 the same way, −1 the other), at the patch's middle: a patch on the
/// surface doesn't fold, so it is the same all over. `None` where it
/// can't be told.
fn facing(shape: &Shape, patch: &Patch) -> Option<f64> {
    let middle = DVec3::splat(1.0 / 3.0);
    let g = gradient(shape, patch.eval(middle))?;
    let d = g.dot(patch.normal(middle));
    (d != 0.0 && d.is_finite()).then(|| d.signum())
}

fn gradient(shape: &Shape, x: DVec3) -> Option<DVec3> {
    match shape {
        Shape::Plane { n, .. } => Some(*n),
        Shape::Quadric(q) => Some(q.gradient(x)),
        Shape::Other => None,
    }
}

/// Whether the edge crossing `shape` at `s` does so with sign `x` (+1
/// into the patch's solid, against its outward normal), or so nearly
/// along the surface (a tangency, a double root) that either sign fits.
fn sign_fits(conic: &Conic3, s: f64, shape: &Shape, facing: f64, x: i8) -> bool {
    let (p, d) = conic.eval_deriv(s);
    let Some(g) = gradient(shape, p) else {
        return false;
    };
    let along = g.dot(d) * facing;
    if along.abs() <= 1e-9 * g.length() * d.length() {
        return true;
    }
    (along < 0.0) == (x > 0)
}

/// The parameters in the edge (or a rounding outside it, `1e-9`) where
/// `conic` meets `shape`, each within [`END`] of an end put at it.
fn roots(conic: &Conic3, shape: &Shape) -> Vec<f64> {
    let found: Vec<f64> = match *shape {
        Shape::Plane { n, d } => {
            let side = |x: DVec3| n.dot(x) - d;
            let (f0, f1, f2) = (side(conic.p0), conic.w * side(conic.c), side(conic.p1));
            // f0·(1−t)² + 2f1·t(1−t) + f2·t² in powers of t.
            let (a, b, c) = (f0 - 2.0 * f1 + f2, 2.0 * (f1 - f0), f0);
            touching_roots(a, b, c).into_iter().flatten().collect()
        }
        Shape::Quadric(q) if straight(conic) => {
            // F(p0 + s·d) = F(p0) + s·∇F(p0)·d + s²·d·A·d.
            let d = conic.p1 - conic.p0;
            let (a, b, c) = (
                d.dot(q.a * d),
                q.gradient(conic.p0).dot(d),
                q.value(conic.p0),
            );
            touching_roots(a, b, c)
                .into_iter()
                .flatten()
                .map(|s| newton(conic, &q, s))
                .collect()
        }
        Shape::Quadric(q) => {
            let mut found: Vec<f64> = bernstein::roots(&quartic(conic, &q))
                .into_iter()
                .map(|s| newton(conic, &q, s))
                .collect();
            // A root at or a rounding past an end (a crossing at the
            // edge's end, a vertex on the quadric), which the isolation,
            // on the open interval, doesn't give.
            for end in [0.0, 1.0] {
                let s = newton(conic, &q, end);
                let past = !(0.0..=1.0).contains(&s);
                if (s - end).abs() <= 1e-9 && (past || q.value(point(conic, s)) == 0.0) {
                    found.push(s);
                }
            }
            found
        }
        Shape::Other => Vec::new(),
    };
    found
        .into_iter()
        .filter(|s| (-1e-9..=1.0 + 1e-9).contains(s))
        .map(at_end)
        .collect()
}

/// [`quadratic_roots`], with a discriminant a rounding below 0 taken as
/// 0: an edge touching the surface (a double root) touches it, where
/// rounding had it miss.
fn touching_roots(a: f64, b: f64, c: f64) -> [Option<f64>; 2] {
    let disc = b * b - 4.0 * a * c;
    if a != 0.0 && disc < 0.0 && disc >= -1e-14 * (b * b + 4.0 * (a * c).abs()) {
        return [Some(-b / (2.0 * a)), None];
    }
    quadratic_roots(a, b, c)
}

/// The Bernstein coefficients of `F(C(t))` times the square of the
/// conic's denominator (positive, so the roots are `F∘C`'s): `C`'s
/// homogeneous control points `Hᵢ = (wᵢ·(Pᵢ − P₀), wᵢ)` put in the
/// quadric's 4 × 4 form about the first end, coefficient `k` the sum over
/// `i + j = k` of `C(2,i)·C(2,j)/C(4,k)·Hᵢ·Q·Hⱼ`.
fn quartic(conic: &Conic3, q: &Quadric) -> [f64; 5] {
    let o = conic.p0;
    // F(o + y) = y·A·y + g·y + f.
    let (g, f) = (q.gradient(o), q.value(o));
    let h = [
        (DVec3::ZERO, 1.0),
        ((conic.c - o) * conic.w, conic.w),
        (conic.p1 - o, 1.0),
    ];
    let m = |i: usize, j: usize| {
        let ((ui, hi), (uj, hj)) = (h[i], h[j]);
        0.5 * (ui.dot(q.a * uj) + uj.dot(q.a * ui)) + 0.5 * g.dot(ui * hj + uj * hi) + f * hi * hj
    };
    [
        m(0, 0),
        m(0, 1),
        (m(0, 2) + 2.0 * m(1, 1)) / 3.0,
        m(1, 2),
        m(2, 2),
    ]
}

/// `s` moved by up to two steps of Newton's method on `F(C(s))`, each kept
/// only if it brings `F` nearer 0: the isolation's root is to the last
/// bit of the polynomial, whose coefficients round.
fn newton(conic: &Conic3, q: &Quadric, mut s: f64) -> f64 {
    let mut f = q.value(point(conic, s));
    for _ in 0..2 {
        let (x, dx) = conic.eval_deriv(s);
        let step = f / q.gradient(x).dot(dx);
        let next = s - step;
        if !(step.is_finite() && step.abs() <= 1e-6) {
            break;
        }
        let g = q.value(point(conic, next));
        if g.is_nan() || g.abs() >= f.abs() {
            break;
        }
        (s, f) = (next, g);
    }
    s
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
    // On an elliptic cylinder with both ends on it, the control point (as
    // an offset from `m`) and the weight by the arc's angle.
    let exact = elliptic_arc(q, n, x, y);
    // Straight: both tangents along the chord (a plane along a
    // cylinder's rulings, or nearly).
    let along = |t: DVec3, l: f64| t.cross(chord).length() <= 1e-9 * l * len;
    if along(tx, lx) && along(ty, ly) {
        let arc = match exact {
            // Exactly a ruling.
            Some((mc, _)) if mc == DVec3::ZERO => segment(x, y),
            // Straight only to a billionth: the shorter arc, with no
            // look at the guide, whose side of a chord this near the arc
            // is a rounding's. The other arc has the same tangent lines
            // but runs round the far side of the cylinder, more than
            // half way, which no patch spans, and both callers want an
            // arc on one patch (their ends and guide on it).
            Some((mc, w)) => Conic3 {
                p0: x,
                c: m + mc,
                w,
                p1: y,
            },
            None if q.distance(m) <= 1e-9 * len => segment(x, y),
            None => return None,
        };
        out.push(arc);
        return Some(());
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
    // Where the tangents meet, in the plane: by the angle on an elliptic
    // cylinder (well conditioned however straight the arc), else from
    // the tangents.
    let (c, mc, angle) = match exact {
        Some((mc, w)) => (m + mc, mc, Some(w)),
        None => {
            let c = x + tx * (chord.cross(ty).dot(n) / den);
            (c, c - m, None)
        }
    };
    let roots = meets(mc);
    if !wanted(mc.dot(across)) {
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
    let w = match angle {
        Some(w) => w,
        None => {
            let sigma = roots.into_iter().find(|&s| s > 0.0 && s < 1.0)?;
            sigma / (1.0 - sigma)
        }
    };
    let sigma = w / (1.0 + w);
    let (a, b) = (c - x, y - c);
    if !(0.5..=W_MAX).contains(&w) || a.dot(b) < MAX_TURN_COS * a.length() * b.length() {
        return split(m + mc * sigma, out);
    }
    out.push(Conic3 { p0: x, c, w, p1: y });
    Some(())
}

/// The arc from `x` to `y` of the section of `q` by the plane through
/// them with unit normal `n`, where `q` is an elliptic (or circular)
/// cylinder and both ends lie on it (to about `1e-12` of its size): the
/// shorter arc's control point, as its offset from the chord's middle
/// `m` (zero for a ruling), and its weight. `None` anywhere else, or
/// where the plane is along the rulings and the ends aren't on one.
///
/// Seen along the axis, a plane section is an affine image of the
/// cylinder's cross-section, itself one of a circle, and affine maps
/// keep weights and control points: the weight is a circle arc's,
/// `cos(Δφ/2) = |r̂x + r̂y| / 2` with `r̂` the vectors from the axis to the
/// ends made unit in the cylinder's own metric (`|v|² = ±v·S·v`, `S` its
/// matrix's symmetric part: on a circular cylinder, plain lengths
/// square to the axis), and the control point is `m − tan²(Δφ/2)·(Ĉ −
/// m)`, `Ĉ` the plane's point on the axis and `tan²(Δφ/2) = |r̂x − r̂y|² /
/// |r̂x + r̂y|²`. Both exact in relative terms however straight the arc.
/// The weight from the point where the line from `m` to the control
/// point meets the quadric was noise where the arc bulges by a rounding
/// (`1.000135` on an ellipse arc, whose weight is under 1), and pulled
/// the bands beside it `1e-7` off the cylinder; and the tangents of a
/// nearly straight arc meet only roughly. Ends off the quadric keep
/// those: there the old weight had made up for them, and the angle's
/// made the bands worse.
fn elliptic_arc(q: &Quadric, n: DVec3, x: DVec3, y: DVec3) -> Option<(DVec3, f64)> {
    let cylinder = elliptic_cylinder(q)?;
    let axis = cylinder.axis;
    let flat = |p: DVec3| {
        let r = p - cylinder.centre;
        r - axis * axis.dot(r)
    };
    let norm = |v: DVec3| v.dot(cylinder.metric * v).max(0.0).sqrt();
    let on = |p: DVec3| q.distance(p) <= 1e-12 * flat(p).length().max((p - q.origin).length());
    if !(on(x) && on(y)) {
        return None;
    }
    let unit = |p: DVec3| {
        let r = flat(p);
        let l = norm(r);
        (l > 0.0 && l.is_finite()).then(|| r / l)
    };
    let (rx, ry) = (unit(x)?, unit(y)?);
    let (sum, diff) = (rx + ry, rx - ry);
    if diff == DVec3::ZERO {
        return Some((DVec3::ZERO, 1.0));
    }
    let m = (x + y) * 0.5;
    // `Ĉ − m`: in the plane, square to the axis as `centre − m` is.
    let to_axis = (cylinder.centre - m) + axis * (n.dot(m - cylinder.centre) / n.dot(axis));
    let (sum, diff) = (norm(sum), norm(diff));
    let mc = to_axis * -((diff / sum) * (diff / sum));
    let w = 0.5 * sum;
    (mc.is_finite() && w.is_finite()).then_some((mc, w))
}

/// An elliptic cylinder's axis and metric (see [`elliptic_cylinder`]).
struct Cylinder {
    /// A point on the axis.
    centre: DVec3,
    /// The unit axis.
    axis: DVec3,
    /// The quadric's matrix's symmetric part, its sign made positive
    /// square to the axis: `v·metric·v` is the square of `v`'s length in
    /// the cross-section's units, up to a scale.
    metric: DMat3,
}

/// `q`'s axis and metric where it is an elliptic (or circular) cylinder:
/// its matrix's symmetric part `S` takes the axis to 0 and is definite
/// square to it, and its linear part `b` is square to the axis, to about
/// `1e-12` relative, `F` of the other sign from `S` on the axis. The
/// centre solves `S·y = −b` square to the axis: an extruded cylinder's
/// origin is off its axis (`b ≠ 0`).
fn elliptic_cylinder(q: &Quadric) -> Option<Cylinder> {
    let s = (q.a + q.a.transpose()) * 0.5;
    let rows = [s.row(0), s.row(1), s.row(2)];
    // The axis spans the null space: the longest cross product of two
    // rows.
    let axis = [(0, 1), (1, 2), (2, 0)]
        .map(|(i, j)| rows[i].cross(rows[j]))
        .into_iter()
        .max_by(|u, v| u.length_squared().total_cmp(&v.length_squared()))?
        .try_normalize()?;
    let e1 = axis.any_orthonormal_vector();
    let e2 = axis.cross(e1);
    let (s11, s12, s22) = (e1.dot(s * e1), e1.dot(s * e2), e2.dot(s * e2));
    let size = s11.abs() + s22.abs();
    let det = s11 * s22 - s12 * s12;
    let along = (s * axis).length();
    if !(det > 1e-12 * size * size
        && along <= 1e-12 * size
        && q.b.dot(axis).abs() <= 1e-12 * q.b.length())
    {
        return None;
    }
    let sign = s11.signum();
    // `S·z = −b` in the plane square to the axis.
    let (b1, b2) = (-e1.dot(q.b), -e2.dot(q.b));
    let z = e1 * ((b1 * s22 - b2 * s12) / det) + e2 * ((s11 * b2 - s12 * b1) / det);
    let centre = q.origin + z;
    let squared = -q.value(centre) * sign;
    (squared > 0.0 && squared.is_finite() && centre.is_finite()).then(|| Cylinder {
        centre,
        axis,
        metric: s * sign,
    })
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]
mod tests {
    use super::*;
    use glam::DVec2;

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

    /// The elliptic cylinder round the line through `centre` along the
    /// unit `axis`, of semi-axes `r.0` along `u` and `r.1` along `v` (a
    /// right-handed frame with the axis), as `k·(((y − y₀)·u / r.0)² +
    /// ((y − y₀)·v / r.1)² − 1)` about `origin` (`y₀ = centre − origin`
    /// square to the axis): a scale and an origin off the axis, as
    /// extruded cylinders have.
    struct Elliptic {
        q: Quadric,
        centre: DVec3,
        axis: DVec3,
        u: DVec3,
        v: DVec3,
        r: (f64, f64),
    }

    impl Elliptic {
        fn new(centre: DVec3, axis: DVec3, r: (f64, f64), origin: DVec3, k: f64) -> Elliptic {
            let u = axis.any_orthonormal_vector();
            let v = axis.cross(u);
            let outer = |d: DVec3| DMat3::from_cols(d * d.x, d * d.y, d * d.z);
            let a = (outer(u) / (r.0 * r.0) + outer(v) / (r.1 * r.1)) * k;
            let y0 = centre - origin;
            let y0 = y0 - axis * axis.dot(y0);
            let q = Quadric {
                origin,
                a,
                b: -(a * y0),
                c: y0.dot(a * y0) - k,
            };
            Elliptic {
                q,
                centre,
                axis,
                u,
                v,
                r,
            }
        }

        /// The point at eccentric angle `phi`, `h` along the axis.
        fn at(&self, phi: f64, h: f64) -> DVec3 {
            self.centre
                + self.u * (self.r.0 * phi.cos())
                + self.v * (self.r.1 * phi.sin())
                + self.axis * h
        }

        /// Where `p` is seen along the axis, on the unit circle's scale.
        fn seen(&self, p: DVec3) -> DVec2 {
            let d = p - self.centre;
            DVec2::new(d.dot(self.u) / self.r.0, d.dot(self.v) / self.r.1)
        }

        /// The eccentric angle from `p` to `q`.
        fn angle(&self, p: DVec3, q: DVec3) -> f64 {
            let (a, b) = (self.seen(p), self.seen(q));
            a.perp_dot(b).abs().atan2(a.dot(b))
        }
    }

    #[test]
    fn elliptic_cylinders_are_told_with_their_axes() {
        let axis = DVec3::new(0.3, -0.5, 0.8).normalize();
        let centre = DVec3::new(1.5, -0.7, 2.0);
        for r in [(1.3, 1.3), (1.3, 0.4)] {
            for (origin, k) in [(centre, 1.0), (DVec3::new(3.0, 1.0, -2.0), -2.5)] {
                let e = Elliptic::new(centre, axis, r, origin, k);
                let cylinder = elliptic_cylinder(&e.q).unwrap();
                assert!(cylinder.axis.cross(axis).length() < 1e-15);
                let off = cylinder.centre - centre;
                assert!((off - axis * axis.dot(off)).length() < 1e-14, "{off}");
                // The metric is the cross-section's, up to a scale.
                let unit = |d: DVec3| d.dot(cylinder.metric * d) * r.0 * r.0;
                assert!((unit(e.u * r.0) / unit(e.v * r.1) - 1.0).abs() < 1e-14);
            }
        }
        // A cone, a sphere, a paraboloid, hyperbolic and parabolic
        // cylinders.
        let quadric = |a: DVec3, b: DVec3| Quadric {
            origin: centre,
            a: DMat3::from_diagonal(a),
            b,
            c: -1.0,
        };
        for q in [
            quadric(DVec3::new(1.0, 1.0, -1.0), DVec3::ZERO),
            quadric(DVec3::ONE, DVec3::ZERO),
            quadric(DVec3::new(1.0, 1.0, 0.0), DVec3::Z),
            quadric(DVec3::new(1.0, -1.0, 0.0), DVec3::ZERO),
            quadric(DVec3::new(1.0, 0.0, 0.0), DVec3::Y),
        ] {
            assert!(elliptic_cylinder(&q).is_none(), "{q:?}");
        }
    }

    #[test]
    fn nearly_straight_sections_of_cylinders_take_the_angles_weight() {
        // Planes from 1e-9 to 1 radian off the rulings' direction cut a
        // tilted circular and elliptic cylinder (each its origin off the
        // axis, scaled) in ellipses from long and thin to round; arcs of
        // them a thousandth to three long, from nearly straight to
        // halved. Each arc's weight is the eccentric angle's, to a
        // rounding, and its middle is where that angle is halved; from
        // the point on the line from the chord's middle to the control
        // point the weight was off by up to 3.3e-3 on the circle, and
        // arcs within a billionth of straight were straight edges, up
        // to 2.7e-11 off the cylinder. Along the rulings, a straight
        // edge.
        let axis = DVec3::new(0.3, -0.5, 0.8).normalize();
        let centre = DVec3::new(1.5, -0.7, 2.0);
        let origin = DVec3::new(3.0, 1.0, -2.0);
        let (mut worst, mut arcs_seen) = (0.0f64, 0);
        for r in [(1.3, 1.3), (1.3, 0.4)] {
            let cyl = Elliptic::new(centre, axis, r, origin, -2.5);
            let phi0 = 0.7;
            // Square to the axis, not square to the curve at `phi0`.
            let e = (cyl.at(phi0 + 0.3, 0.0) - centre).normalize();
            for delta in [1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-3, 0.1, 1.0] {
                let n = (e + axis * delta).normalize();
                // Along the section, `e·(at(φ) − centre) + delta·h` is
                // constant.
                let at = |phi: f64| {
                    let flat = |phi: f64| e.dot(cyl.at(phi, 0.0) - centre);
                    cyl.at(phi, 0.4 - (flat(phi) - flat(phi0)) / delta)
                };
                let slope = e.dot(cyl.u * -(r.0 * phi0.sin()) + cyl.v * (r.1 * phi0.cos())) / delta;
                for span in [1e-3, 0.05, 0.5, 3.0] {
                    // About `span` long, or half the ellipse.
                    let dphi = (span / (slope.abs() + r.0)).min(3.0);
                    let (x, y) = (at(phi0 - 0.3 * dphi), at(phi0 + 0.7 * dphi));
                    let arcs = section(&cyl.q, n, x, y, Guide::Near(at(phi0))).unwrap();
                    for arc in &arcs {
                        assert_ne!(segment(arc.p0, arc.p1), *arc, "{r:?} {delta} {span}");
                        let want = (0.5 * cyl.angle(arc.p0, arc.p1)).cos();
                        worst = worst.max((arc.w - want).abs());
                        let mid = arc.eval(0.5);
                        let halves = (cyl.angle(arc.p0, mid) - cyl.angle(mid, arc.p1)).abs();
                        assert!(halves < 4e-15, "{r:?} {delta} {span}: {halves:e}");
                        arcs_seen += 1;
                        for k in 0..=16 {
                            let p = arc.eval(k as f64 / 16.0);
                            assert!(cyl.q.distance(p) < 1e-14, "{r:?} {delta} {span}: {p}");
                        }
                    }
                }
            }
            // A plane along the rulings, the ends on one.
            let x = cyl.at(phi0, 0.0);
            let y = x + axis * 2.0;
            let arcs = section(&cyl.q, e, x, y, Guide::Near(x)).unwrap();
            assert_eq!(arcs, vec![segment(x, y)]);
        }
        assert!(arcs_seen >= 64, "{arcs_seen}");
        assert!(worst <= 1e-15, "{worst:e}");
    }

    /// A crossing the search solved, of some patch (the checks away from
    /// its position don't come into it).
    fn solved(patch: &Patch) -> Crossed<'_> {
        Crossed {
            patch,
            x: 1,
            solved: true,
            resolution: 1e-6,
        }
    }

    fn any_patch() -> Patch {
        Patch::flat([DVec3::ZERO, DVec3::X, DVec3::Y]).unwrap()
    }

    #[test]
    fn crossings_solve_exactly() {
        let arc = Conic3 {
            p0: DVec3::new(1.0, 0.0, 0.0),
            c: DVec3::new(1.0, 1.0, 3.0),
            w: std::f64::consts::FRAC_1_SQRT_2,
            p1: DVec3::new(0.0, 1.0, 3.0),
        };
        let patch = any_patch();
        let polish = |t: f64, shape: &Shape| polish(&arc, t, shape, &solved(&patch)).0;
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
        let t = polish(near(&|t| arc.eval(t).z - 1.0), &plane);
        assert!((arc.eval(t).z - 1.0).abs() < 1e-15);
        let q = Quadric::cylinder(DVec3::ZERO, DVec3::X, 0.9).unwrap();
        let t = polish(near(&|t| q.value(arc.eval(t))), &Shape::Quadric(q));
        assert!(q.distance(arc.eval(t)) < 1e-15);
        // Far from it: on the plane all the same.
        let t = polish(0.9, &plane);
        assert!((arc.eval(t).z - 1.0).abs() < 1e-15);
        // Nowhere in the edge: left as it is.
        let above = Shape::Plane {
            n: DVec3::Z,
            d: 5.0,
        };
        assert_eq!(polish(0.9, &above), 0.9);
        // At the edge's end, on the plane: the end itself, to the bit,
        // however the root rounds.
        let end = Shape::Plane {
            n: DVec3::Z,
            d: 3.0,
        };
        assert_eq!(polish(0.999_999, &end), 1.0);
        assert_eq!(arc.eval(polish(0.999_999, &end)), arc.p1);
    }

    /// A tilted cylinder, a strip of its wall (the patch holding the point
    /// `p` on it), `p`, and at `p` the unit vectors out of the cylinder
    /// and along its axis.
    struct Wall {
        q: Quadric,
        patch: Patch,
        p: DVec3,
        out: DVec3,
        axis: DVec3,
    }

    fn wall() -> Wall {
        let axis = DVec3::new(0.3, -0.5, 0.8).normalize();
        let x = axis.any_orthonormal_vector();
        let y = axis.cross(x);
        let (origin, r) = (DVec3::new(1.5, -0.7, 2.0), 1.3);
        let q = Quadric::cylinder(origin, axis, r).unwrap();
        let bottom = Conic3::arc(origin, x, y, r, 0.2, 1.4).unwrap();
        let strip = crate::patch::cylinder_strip(&bottom, axis * 2.0).unwrap();
        // At the strip's middle angle, a third of the way up.
        let out = x * 0.9f64.cos() + y * 0.9f64.sin();
        let p = origin + out * r + axis * 0.7;
        let patch = strip
            .into_iter()
            .find(|patch| near_patch(p, patch, 1e-9).0.is_some())
            .unwrap();
        Wall {
            q,
            patch,
            p,
            out,
            axis,
        }
    }

    fn placed(patch: &Patch, x: i8) -> Crossed<'_> {
        Crossed {
            patch,
            x,
            solved: false,
            resolution: 1e-6,
        }
    }

    /// The parameter where `conic` passes through `p`, by bisection on
    /// the distance's derivative, for a root known to be at `p`.
    fn at(conic: &Conic3, p: DVec3) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..200 {
            let mid = (lo + hi) / 2.0;
            let (x, d) = conic.eval_deriv(mid);
            if (x - p).dot(d) < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    #[test]
    fn crossings_only_placed_go_to_their_root_on_the_patch() {
        let w = wall();
        let shape = Shape::Quadric(w.q);
        // A segment into the cylinder through `p`, on through its axis and
        // out on the far side (a root of the other sign, off the patch).
        let tilt = w.axis * 0.4;
        let line = segment(w.p + (w.out + tilt) * 0.6, w.p - (w.out + tilt) * 3.2);
        // An ellipse arc through `p`, leaving the cylinder there.
        let side = w.out.cross(w.axis);
        let centre = w.p + w.axis * 0.8;
        let ellipse =
            Conic3::arc(centre, w.axis, (side - w.out * 2.5) * 0.4, 0.8, 2.6, 1.1).unwrap();
        for (conic, x) in [(line, 1), (ellipse, -1)] {
            let root = at(&conic, w.p);
            assert!(
                conic.eval(root).distance(w.p) < 1e-12,
                "{}",
                conic.eval(root)
            );
            for off in [-1e-3, 1e-3, 0.05] {
                let (t, _) = polish(&conic, root + off, &shape, &placed(&w.patch, x));
                assert!((t - root).abs() < 1e-15, "{off}: {t} not {root}");
                assert!(w.q.distance(point(&conic, t)) < 1e-15);
                // Solved but that far off: checked the same way.
                let (t, _) = polish(
                    &conic,
                    root + off,
                    &shape,
                    &Crossed {
                        solved: true,
                        ..placed(&w.patch, x)
                    },
                );
                assert!((t - root).abs() < 1e-15, "{off}: {t} not {root}");
                // The other sign: not this root, and no other on the
                // patch, so the position stays.
                let (t, _) = polish(&conic, root + off, &shape, &placed(&w.patch, -x));
                assert_eq!(t, root + off);
            }
        }
        // The segment's root on the far side, nearest: it has the other
        // sign and isn't on the patch.
        let far = at(&line, w.p - (w.out + tilt) * 2.6);
        let (t, _) = polish(&line, 0.9, &shape, &placed(&w.patch, -1));
        assert_eq!(t, 0.9);
        assert!((far - 3.2 / 3.8).abs() < 1e-12, "{far}");
        // A root off the patch crossed: the position stays.
        let other = Patch::flat([
            w.p + w.axis * 5.0,
            w.p + w.axis * 6.0,
            w.p + w.axis * 5.0 + side,
        ])
        .unwrap();
        let (t, _) = polish(&line, 0.2, &shape, &placed(&other, 1));
        assert_eq!(t, 0.2);
        // No root in the edge: the position stays.
        let short = segment(w.p + w.out * 0.6, w.p + w.out * 0.1);
        assert_eq!(polish(&short, 0.5, &shape, &placed(&w.patch, 1)).0, 0.5);
    }

    #[test]
    fn a_grazing_line_placed_far_off_goes_to_its_root() {
        // Nearly along a ruling (1e-5 off it), into the wall at `p`, half
        // way along: where a search runs out of pieces and places the
        // crossing a third of the edge off.
        let w = wall();
        let shape = Shape::Quadric(w.q);
        for alpha in [1e-5, 1e-3] {
            let dir = (w.axis - w.out * alpha).normalize();
            let line = segment(w.p - dir * 0.6, w.p + dir * 0.6);
            for t in [0.166, 0.9] {
                let (s, _) = polish(&line, t, &shape, &placed(&w.patch, 1));
                // To `p`'s own rounding over the slant: 1e-16 / 1e-5.
                assert!((s - 0.5).abs() < 1e-10, "{alpha} {t}: {s}");
                assert!(w.q.distance(point(&line, s)) < 1e-15);
            }
        }
    }

    #[test]
    fn a_tangent_crossing_goes_to_the_touching_point() {
        let w = wall();
        let shape = Shape::Quadric(w.q);
        // Along the cylinder's surface at `p`, square to the axis: a
        // double root.
        let along = w.out.cross(w.axis);
        let line = segment(w.p - along * 0.3, w.p + along * 0.5);
        let root = 0.3 / 0.8;
        for x in [1, -1] {
            let (t, _) = polish(&line, root + 1e-4, &shape, &placed(&w.patch, x));
            assert!(point(&line, t).distance(w.p) < 1e-7, "{t}");
        }
    }

    #[test]
    fn a_curved_edge_from_a_point_on_the_cylinder_crosses_there() {
        // An arc from a point exactly on a cylinder (`F` exactly 0) into
        // it: the quartic's root at the end, which the isolation (on the
        // open interval) doesn't give and Newton's method from the end
        // doesn't move, was lost, and the crossing stayed where placed.
        let q = Quadric::cylinder(DVec3::ZERO, DVec3::Z, 1.0).unwrap();
        let bottom = Conic3::arc(DVec3::ZERO, DVec3::X, DVec3::Y, 1.0, -0.5, 0.5).unwrap();
        let strip = crate::patch::cylinder_strip(&bottom, DVec3::Z * 2.0).unwrap();
        let start = DVec3::new(1.0, 0.0, 0.5);
        assert_eq!(q.value(start), 0.0);
        let patch = strip
            .into_iter()
            .find(|patch| near_patch(start, patch, 1e-9).0.is_some())
            .unwrap();
        let arc = Conic3 {
            p0: start,
            c: DVec3::new(0.5, 0.5, 0.5),
            w: 0.8,
            p1: DVec3::new(0.2, 0.3, 0.8),
        };
        let (t, _) = polish(&arc, 0.3, &Shape::Quadric(q), &placed(&patch, 1));
        assert_eq!(t, 0.0);
    }

    #[test]
    fn a_plane_crossed_twice_takes_the_root_of_the_crossings_sign() {
        // An arc bulging down through the plane z = 1: in (+1, against
        // the patch's normal, up) at the first root, out (−1) at the
        // second.
        let arc = Conic3 {
            p0: DVec3::new(0.0, 0.0, 2.0),
            c: DVec3::new(1.0, 0.0, -1.0),
            w: 1.0,
            p1: DVec3::new(2.0, 0.0, 2.0),
        };
        let plane = Shape::Plane {
            n: DVec3::Z,
            d: 1.0,
        };
        let up = Patch::flat([
            DVec3::new(-1.0, -1.0, 1.0),
            DVec3::new(3.0, -1.0, 1.0),
            DVec3::new(1.0, 3.0, 1.0),
        ])
        .unwrap();
        assert!(up.normal(DVec3::splat(1.0 / 3.0)).z > 0.0);
        // z(t) = 2(1−t)² − 2t(1−t) + 2t² = 1: 6t² − 6t + 1 = 0.
        let first = (3.0 - 3.0f64.sqrt()) / 6.0;
        let second = (3.0 + 3.0f64.sqrt()) / 6.0;
        // Wherever the search put them, each goes to its own root.
        for t in [0.1, 0.5, 0.7, 0.95] {
            let (s, _) = polish(&arc, t, &plane, &placed(&up, 1));
            assert!((s - first).abs() < 1e-15, "{t}: {s}");
            let (s, _) = polish(&arc, t, &plane, &placed(&up, -1));
            assert!((s - second).abs() < 1e-15, "{t}: {s}");
        }
        // Solved: the nearest root, as before.
        let (s, _) = polish(
            &arc,
            0.9,
            &plane,
            &Crossed {
                solved: true,
                ..placed(&up, 1)
            },
        );
        assert!((s - second).abs() < 1e-15);
        // Only the second root on the patch: the first, of the sign asked
        // for, isn't taken; the nearest root is, and the check on the
        // crossings decides.
        let right = Patch::flat([
            DVec3::new(1.5, -1.0, 1.0),
            DVec3::new(3.0, -1.0, 1.0),
            DVec3::new(1.5, 3.0, 1.0),
        ])
        .unwrap();
        let (s, _) = polish(&arc, 0.4, &plane, &placed(&right, 1));
        assert!((s - first).abs() < 1e-15, "{s}");
        let (s, _) = polish(&arc, 0.4, &plane, &placed(&right, -1));
        assert!((s - second).abs() < 1e-15, "{s}");
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

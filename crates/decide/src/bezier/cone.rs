//! Normal cones, support functions and gaps: a certified circular cone of
//! directions holding every non-negative combination of a vector form's
//! coefficients, whether two cones are apart, and certified separations
//! of point sets along a direction.
//!
//! **Cones.** A vector form `N = (N_x, N_y, N_z)` in the Bernstein basis
//! takes at each point a non-negative combination of its coefficient
//! vectors (the basis is non-negative). A circular cone of half-angle
//! `θ < 90°` is convex, so if it holds every coefficient it holds every
//! value, and the values, all with a positive component along its axis,
//! are never zero. For a patch's normal numerator (`W³·(x_u × x_v)`,
//! positive weights) it holds every unit normal and certifies the patch
//! regular; its certified `cos` is the margin. The same holds for the
//! coefficients of several forms together, such as the pieces of one form
//! split in its domain ([`NormalCone::of_forms`]): every value lies in
//! one piece and is a combination of its coefficients. Splitting brings
//! the coefficients closer to the values, so a cone that fails on the
//! whole form may be certified on its pieces.
//!
//! **Axes.** Any axis is sound, as the cone is certified on the
//! coefficients' enclosures whichever axis is tried; the axis only makes
//! a tight cone likely. [`Axis::Smallest`] tries the axis of the smallest
//! cone around the coefficients' directions (the exhaustive search over
//! the candidates fixing it is `O(n⁴)`, so it runs on a growing core set
//! of the directions: for one patch's numerator); [`Axis::Mean`] the
//! normalised sum of the directions, `O(n)`, for many coefficients (split
//! pieces) and for callers that only need some certified cone (leaf
//! certificates).
//!
//! **Failure.** Where no cone is certified, [`ConeFail`] says what may
//! help, the escalation rule of the precision schedule (an item
//! escalates precision when its rounding decides, else in space): a
//! coefficient whose enclosure is wide enough to hold zero, or whose
//! centre lies in the axis's open half-space while its enclosure doesn't,
//! fails by rounding ([`ConeFail::Precision`]); a coefficient whose
//! centre itself lies outside, or no axis at all, by the spread of the
//! coefficients ([`ConeFail::Space`]: split).
//!
//! **Apart.** Two cones are apart ([`NormalCone::apart`]) when no
//! direction of one is parallel to a direction of the other, either way
//! round: the cones of `a` and `±b` disjoint.
//!
//! **Support and gaps.** The support function of a point set along `ℓ`
//! is `h(ℓ) = max ℓ·p`; for a patch's control points (or its hull box's
//! corners) it bounds the patch, whose points are convex combinations of
//! them (positive weights). [`support`] encloses it; [`gap`] encloses the
//! separation `min ℓ·b − max ℓ·a` of two point sets along `ℓ` as a
//! [`Bounded`] value, certified positive when the sets' hulls are
//! apart; [`Obb`] keeps the slabs of a point set along three directions.
//! The direction itself is only proposed ([`propose_axis`], a
//! Gilbert–Johnson–Keerthi style descent in `f64`): only the gap
//! certifies.

use super::scalar::Scalar;
use super::tensor::{Dir, Tensor};
use crate::num::bounded::{Bounded, add_up, down, mul_up, up};

/// A certified cone of directions: every direction whose angle to `axis`
/// is at most `θ`, `0 ≤ θ < 90°`.
///
/// `θ` is kept as bounds on its cosine and sine, so decisions with it use
/// correctly rounded arithmetic only (no `acos`): `cos` is at most
/// `cos θ` and positive, `sin` at least `sin θ` and at most 1. The axis's
/// length is near 1, not exactly 1: only its direction matters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalCone {
    /// The axis's direction.
    pub axis: [f64; 3],
    /// A positive lower bound of `cos θ`.
    pub cos: f64,
    /// An upper bound of `sin θ`, at most 1.
    pub sin: f64,
}

/// The axis a cone is tried about (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// The axis of the smallest cone around the coefficients' directions
    /// (module docs).
    Smallest,
    /// The normalised sum of the coefficients' directions, `O(n)`.
    Mean,
}

/// Why no cone was certified, and what may help (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConeFail {
    /// The coefficients spread too far for the axis tried: split.
    Space,
    /// Rounding decides: a higher precision level.
    Precision,
}

impl NormalCone {
    /// The cone of `n`'s coefficients about [`Axis::Smallest`]'s axis,
    /// `None` where none is certified ([`of_forms`](Self::of_forms)).
    pub fn of_form<S: Scalar>(n: &[Tensor<S>; 3]) -> Option<NormalCone> {
        NormalCone::of_forms([n], Axis::Smallest).ok()
    }

    /// The cone about `axis`'s choice of axis that holds every
    /// coefficient's enclosure of every form in `forms`, so every value of
    /// each (module docs), else why not: none unless every enclosure lies
    /// certainly in the open half-space `axis·c > 0`.
    ///
    /// The bound: for `c` in a coefficient's box, `a·c ≥ d` (the box's
    /// least dot product, rounded down) and `|c| ≤ m` (rounded up), so
    /// `cos ∠(a, c) ≥ d / (|a|·m)` once `d > 0`; `cos` is the least of
    /// these and `sin = √(1 − cos²)`, rounded up.
    ///
    /// Within a form, components of different degrees are first raised to
    /// the highest degree in each direction, so that the coefficients with
    /// one index belong to one Bernstein basis function. A form with no
    /// coefficients can't occur ([`Tensor`]); no forms at all is
    /// [`ConeFail::Space`].
    pub fn of_forms<'a, S: Scalar + 'a>(
        forms: impl IntoIterator<Item = &'a [Tensor<S>; 3]>,
        axis: Axis,
    ) -> Result<NormalCone, ConeFail> {
        let mut boxes = Vec::new();
        for n in forms {
            push_boxes(n, &mut boxes);
        }
        certify(&boxes, axis)
    }

    /// Whether no direction in this cone is parallel to one in `o`,
    /// either way round, certified: the axes are `φ` apart and the
    /// half-angles add up to `R < 90°` with `R < φ < 180° − R`. Two
    /// patches whose normal cones are apart have no tangency between
    /// them.
    ///
    /// With `θ ≤ θ'` for `cos θ' = cos` (and `sin θ' ≤ sin`), `R` is at
    /// most `R' = θ'₁ + θ'₂`, and `cos R' ≥ c₁c₂ − s₁s₂`, `sin R' ≤
    /// s₁c₂ + c₁s₂` once the first is positive. Apart then follows from
    /// `sin φ·cos R' > |cos φ|·sin R'` (both `sin(φ − R')` and
    /// `sin(180° − φ − R')` positive), compared as `|a × b|·cos R' >
    /// |a·b|·sin R'` with the cross product's length rounded down and the
    /// dot product's rounded up.
    pub fn apart(&self, o: &NormalCone) -> bool {
        if !(self.cos > 0.0 && o.cos > 0.0) {
            return false;
        }
        let cos_r = down(down(self.cos * o.cos) - up(self.sin * o.sin));
        if cos_r.is_nan() || cos_r <= 0.0 {
            return false;
        }
        let sin_r = add_up(mul_up(self.sin, o.cos), mul_up(self.cos, o.sin));
        let (a, b) = (self.axis, o.axis);
        let lo = |x: f64, y: f64| down(x * y);
        let hi = |x: f64, y: f64| up_signed(x * y);
        // `|a × b|²`, rounded down: each component's interval and the
        // least magnitude in it.
        let mut cross2 = 0.0;
        for (i, j) in [(1, 2), (2, 0), (0, 1)] {
            let c_lo = down(lo(a[i], b[j]) - hi(a[j], b[i]));
            let c_hi = up_signed(hi(a[i], b[j]) - lo(a[j], b[i]));
            let m = if c_lo > 0.0 {
                c_lo
            } else if c_hi < 0.0 {
                -c_hi
            } else {
                0.0
            };
            cross2 = down(cross2 + down(m * m));
        }
        let cross = if cross2 > 0.0 {
            down(cross2.sqrt())
        } else {
            0.0
        };
        // `|a·b|`, rounded up.
        let point = |v: [f64; 3]| v.map(|x| [x, x]);
        let dot = most(dot_up(&a, &point(b)).abs(), dot_down(&a, &point(b)).abs());
        let left = down(cross * cos_r);
        let right = mul_up(dot, sin_r);
        left > 0.0 && left > right
    }
}

/// An upper bound of the exact value `t` was rounded from, any sign.
#[inline]
fn up_signed(t: f64) -> f64 {
    -down(-t)
}

/// The larger of two bounds, `+∞` for a NaN.
#[inline]
fn most(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::INFINITY
    } else if b > a {
        b
    } else {
        a
    }
}

/// Appends the coefficient enclosures of `n`, its components raised to
/// common degrees first.
fn push_boxes<S: Scalar>(n: &[Tensor<S>; 3], boxes: &mut Vec<[[f64; 2]; 3]>) {
    let m = n.iter().map(|f| f.degree(Dir::U)).max().unwrap_or(0);
    let k = n.iter().map(|f| f.degree(Dir::V)).max().unwrap_or(0);
    let raised;
    let n = if n.iter().all(|f| f.degrees() == (m, k)) {
        n
    } else {
        raised = n.clone().map(|f| {
            f.elevate_to(m, k)
                .expect("within the components' own degrees")
        });
        &raised
    };
    boxes.extend((0..n[0].coefs().len()).map(|i| [0, 1, 2].map(|k| n[k].coefs()[i].enclose())));
}

/// The cone about `mode`'s axis holding every box (module docs).
fn certify(boxes: &[[[f64; 2]; 3]], mode: Axis) -> Result<NormalCone, ConeFail> {
    if boxes
        .iter()
        .flatten()
        .any(|&[lo, hi]| !(lo.is_finite() && hi.is_finite()))
    {
        // An uncertified coefficient: its rounding, not the form, failed.
        return Err(ConeFail::Precision);
    }
    let dirs: Vec<[f64; 3]> = boxes.iter().filter_map(|b| unit(centre(b))).collect();
    let axis = match mode {
        Axis::Mean => {
            let mut s = [0.0; 3];
            for d in &dirs {
                for k in 0..3 {
                    s[k] += d[k];
                }
            }
            unit(s)
        }
        Axis::Smallest => smallest_cone(&dirs),
    };
    let Some(axis) = axis else {
        return Err(if boxes.iter().any(rounding_holds_zero) {
            ConeFail::Precision
        } else {
            ConeFail::Space
        });
    };
    // `|a|`, rounded up.
    let a_len = up(axis
        .iter()
        .fold(0.0, |s, &a| add_up(s, mul_up(a.abs(), a.abs())))
        .sqrt());
    let mut cos = f64::INFINITY;
    let mut fail = None;
    for b in boxes {
        let dot = dot_down(&axis, b);
        if dot.is_nan() || dot <= 0.0 {
            // Rounding decides when the box is wide enough to hold zero
            // or its centre lies on the axis's side; the spread does when
            // the centre itself lies outside.
            let c = centre(b);
            let by_rounding =
                rounding_holds_zero(b) || axis[0] * c[0] + axis[1] * c[1] + axis[2] * c[2] > 0.0;
            if !by_rounding {
                return Err(ConeFail::Space);
            }
            fail = Some(ConeFail::Precision);
            continue;
        }
        // `|c|` rounded up over the box.
        let len2 = b.iter().fold(0.0, |s, &[lo, hi]| {
            let m = if -lo > hi { -lo } else { hi };
            add_up(s, mul_up(m, m))
        });
        let c = down(dot / mul_up(a_len, up(len2.sqrt())));
        if c < cos {
            cos = c;
        }
    }
    if let Some(f) = fail {
        return Err(f);
    }
    if cos.is_nan() || cos <= 0.0 {
        // Every dot product is positive: only rounding (an underflow)
        // can leave the bound at zero.
        return Err(ConeFail::Precision);
    }
    // `sin θ = √(1 − cos² θ) ≤ √(1 − cos²)` for `cos ≤ cos θ`.
    let sin = up(up(1.0 - down(cos * cos)).sqrt());
    Ok(NormalCone {
        axis,
        cos,
        sin: if sin > 1.0 { 1.0 } else { sin },
    })
}

/// Whether the box holds zero with a positive width: its rounding, not
/// its value, leaves the coefficient's direction open. An exact zero
/// (a box of width zero) is the form's own value.
fn rounding_holds_zero(b: &[[f64; 2]; 3]) -> bool {
    b.iter().all(|&[lo, hi]| lo <= 0.0 && 0.0 <= hi) && b.iter().any(|&[lo, hi]| lo < hi)
}

/// The box's centre.
fn centre(b: &[[f64; 2]; 3]) -> [f64; 3] {
    b.map(|[lo, hi]| lo * 0.5 + hi * 0.5)
}

/// `l·c` over the box `b`, rounded down: its least value.
fn dot_down(l: &[f64; 3], b: &[[f64; 2]; 3]) -> f64 {
    let mut dot = 0.0;
    for (&a, &[lo, hi]) in l.iter().zip(b) {
        let term = if a > 0.0 {
            down(a * lo)
        } else if a < 0.0 {
            down(a * hi)
        } else {
            0.0
        };
        dot = down(dot + term);
    }
    dot
}

/// `l·c` over the box `b`, rounded up: its greatest value.
fn dot_up(l: &[f64; 3], b: &[[f64; 2]; 3]) -> f64 {
    -dot_down(&l.map(|x| -x), b)
}

/// `v`'s direction, near unit length, scaled first so no square
/// overflows or underflows; `None` for a zero or non-finite `v`.
fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let big = v
        .iter()
        .fold(0.0, |m, x| if x.abs() > m { x.abs() } else { m });
    if !(big > 0.0 && v.iter().all(|x| x.is_finite())) {
        return None;
    }
    let v = v.map(|x| x / big);
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    Some(v.map(|x| x / len))
}

fn dot3(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The unit axis whose least dot product with the unit `dirs` is
/// largest, when that is positive: the axis of the smallest cone around
/// them (a proposal in `f64`; the cone is certified afterwards). `None`
/// for no directions or no axis with every dot product positive.
///
/// The cone is fixed by at most three directions on its rim, so it is
/// that of a small core set: the exhaustive search ([`smallest_of`]) runs
/// on the core, and the direction farthest outside the core's cone joins
/// it, until none lies outside (each round the core grows, so at most
/// `n` rounds; a handful in practice, against `O(n⁴)` for the search on
/// all directions). Ties keep the first found, so the answer depends only
/// on the input order.
fn smallest_cone(dirs: &[[f64; 3]]) -> Option<[f64; 3]> {
    let mut core = vec![*dirs.first()?];
    let mut used = vec![0];
    loop {
        let (axis, score) = smallest_of(&core)?;
        if score.is_nan() || score <= 0.0 {
            return None;
        }
        // The first direction with the least dot product.
        let (j, least) = dirs
            .iter()
            .enumerate()
            .fold((0, f64::INFINITY), |(j, m), (i, d)| {
                let x = dot3(d, &axis);
                if x < m { (i, x) } else { (j, m) }
            });
        if least.is_nan() || least >= score || used.contains(&j) {
            return (least > 0.0).then_some(axis);
        }
        core.push(dirs[j]);
        used.push(j);
    }
}

/// [`smallest_cone`]'s exhaustive search on `dirs` (not empty): each
/// direction, the bisector of each pair, and the point of the sphere
/// equally far from each triple, with the least dot product of the best.
fn smallest_of(dirs: &[[f64; 3]]) -> Option<([f64; 3], f64)> {
    let least = |d: &[f64; 3]| {
        dirs.iter()
            .map(|c| dot3(c, d))
            .fold(f64::INFINITY, |m, x| if x < m { x } else { m })
    };
    let mut best: Option<([f64; 3], f64)> = None;
    let mut try_axis = |d: Option<[f64; 3]>| {
        if let Some(d) = d {
            let score = least(&d);
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((d, score));
            }
        }
    };
    for (i, a) in dirs.iter().enumerate() {
        try_axis(Some(*a));
        for (j, b) in dirs.iter().enumerate().skip(i + 1) {
            try_axis(unit([a[0] + b[0], a[1] + b[1], a[2] + b[2]]));
            for c in &dirs[j + 1..] {
                let n = unit(cross3(&sub3(b, a), &sub3(c, a)));
                try_axis(n.map(|n| if dot3(&n, a) < 0.0 { n.map(|x| -x) } else { n }));
            }
        }
    }
    best
}

/// An enclosure `[lo, hi]` of the support function `max ℓ·p` of the
/// exact points `points` (module docs); `[−∞, −∞]` for no points, a side
/// infinite where a product overflows.
pub fn support(points: &[[f64; 3]], l: &[f64; 3]) -> [f64; 2] {
    let mut s = [f64::NEG_INFINITY; 2];
    for p in points {
        let b = p.map(|x| [x, x]);
        let (lo, hi) = (dot_down(l, &b), dot_up(l, &b));
        if lo > s[0] || lo.is_nan() {
            s[0] = if lo.is_nan() { f64::NEG_INFINITY } else { lo };
        }
        if hi > s[1] || hi.is_nan() {
            s[1] = if hi.is_nan() { f64::INFINITY } else { hi };
        }
    }
    s
}

/// The separation `min ℓ·b − max ℓ·a` of the exact point sets `a` and
/// `b` along `ℓ` (module docs), certified: positive when every point of
/// `b`'s hull lies beyond every point of `a`'s along `ℓ`, so the hulls
/// are apart by at least the separation over `|ℓ|`. `ℓ` needs no unit
/// length (a proposed direction, rounded). Uncertified for empty sets
/// or an overflow.
pub fn gap(a: &[[f64; 3]], b: &[[f64; 3]], l: &[f64; 3]) -> Bounded<f64> {
    let [a_lo, a_hi] = support(a, l);
    let neg = l.map(|x| -x);
    // `min ℓ·b = −max (−ℓ)·b`.
    let [nb_lo, nb_hi] = support(b, &neg);
    let (b_lo, b_hi) = (-nb_hi, -nb_lo);
    let lo = down(b_lo - a_hi);
    let hi = up_signed(b_hi - a_lo);
    if !(lo.is_finite() && hi.is_finite()) {
        return Bounded::new(0.0, 0.0, f64::INFINITY);
    }
    let v = lo * 0.5 + hi * 0.5;
    let e = most(up(v - lo), up(hi - v));
    Bounded::new(v, 0.0, e)
}

/// A direction `ℓ` along which `b` likely lies beyond `a`, for [`gap`]
/// to certify: the point of the hull of the differences `b − a` nearest
/// the origin, approached by Gilbert's descent (the support of the
/// difference set towards the origin, then the nearest point of the
/// segment to it) for a bounded number of steps, in `f64` alone. `None`
/// where the hulls seem to meet (the nearest point reaches zero) or for
/// empty or non-finite sets.
pub fn propose_axis(a: &[[f64; 3]], b: &[[f64; 3]]) -> Option<[f64; 3]> {
    const STEPS: usize = 64;
    let (a0, b0) = (a.first()?, b.first()?);
    // The point of `b − a` farthest along `−v`: `b`'s least and `a`'s
    // greatest along `v`.
    let lowest = |v: &[f64; 3]| {
        let pick = |s: &[[f64; 3]], sign: f64| {
            *s.iter()
                .fold(None, |best: Option<(&[f64; 3], f64)>, p| {
                    let d = sign * dot3(p, v);
                    if best.is_none_or(|(_, m)| d < m) {
                        Some((p, d))
                    } else {
                        best
                    }
                })
                .expect("not empty")
                .0
        };
        sub3(&pick(b, 1.0), &pick(a, -1.0))
    };
    let mut v = sub3(b0, a0);
    for _ in 0..STEPS {
        let vv = dot3(&v, &v);
        if !(vv.is_finite() && vv > 0.0) {
            return None;
        }
        let w = lowest(&v);
        // Converged: `w` adds nothing below `v`'s level.
        if vv - dot3(&w, &v) <= 1e-12 * vv {
            break;
        }
        let d = sub3(&w, &v);
        let dd = dot3(&d, &d);
        if dd.is_nan() || dd <= 0.0 {
            break;
        }
        let t = (-dot3(&v, &d) / dd).clamp(0.0, 1.0);
        v = [v[0] + t * d[0], v[1] + t * d[1], v[2] + t * d[2]];
    }
    let w = lowest(&v);
    (dot3(&w, &v) > 0.0).then(|| unit(v)).flatten()
}

/// An oriented box: the slabs `lo[k] ≤ axes[k]·p ≤ hi[k]` holding every
/// point of a set, certified, for three directions near orthonormal (any
/// directions would be sound: each slab is certified on its own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb {
    /// The slabs' directions, near unit length.
    pub axes: [[f64; 3]; 3],
    /// Lower bounds of `axes[k]·p` over the points.
    pub lo: [f64; 3],
    /// Upper bounds of `axes[k]·p` over the points.
    pub hi: [f64; 3],
}

impl Obb {
    /// The box of the exact `points` with its first axis along `normal`
    /// (a normal cone's axis puts a near-flat patch in a thin slab) and
    /// the others completing a frame; `None` for no points, a zero or
    /// non-finite `normal` or an overflow.
    pub fn of_points(points: &[[f64; 3]], normal: &[f64; 3]) -> Option<Obb> {
        let n = unit(*normal)?;
        // The coordinate axis least along `n`, made orthogonal to it.
        let k = (0..3)
            .min_by(|&i, &j| n[i].abs().total_cmp(&n[j].abs()))
            .expect("three axes");
        let mut e = [0.0; 3];
        e[k] = 1.0;
        let t1 = unit(sub3(&e, &n.map(|x| x * dot3(&e, &n))))?;
        let t2 = unit(cross3(&n, &t1))?;
        let axes = [n, t1, t2];
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for k in 0..3 {
            let [_, h] = support(points, &axes[k]);
            let [_, nl] = support(points, &axes[k].map(|x| -x));
            (lo[k], hi[k]) = (-nl, h);
            if !(lo[k].is_finite() && hi[k].is_finite()) {
                return None;
            }
        }
        Some(Obb { axes, lo, hi })
    }

    /// A certified lower bound of the distance from the box to the hull
    /// of the exact `points` along the box's own axes: positive when one
    /// of its slabs leaves every point out (the points on one side of a
    /// plane, the box on the other); zero or below otherwise, and for an
    /// overflow.
    pub fn gap(&self, points: &[[f64; 3]]) -> f64 {
        let mut best = f64::NEG_INFINITY;
        for k in 0..3 {
            let a = &self.axes[k];
            let [_, hi] = support(points, a);
            let [_, nlo] = support(points, &a.map(|x| -x));
            // Points beyond `hi[k]`, or below `lo[k]` (a NaN, from an
            // overflow, certifies nothing).
            let (above, below) = (down(-nlo - self.hi[k]), down(self.lo[k] - hi));
            let g = match (above.is_nan(), below.is_nan()) {
                (true, true) => continue,
                (true, false) => below,
                (false, true) => above,
                (false, false) => {
                    if below > above {
                        below
                    } else {
                        above
                    }
                }
            };
            let len = up(a
                .iter()
                .fold(0.0, |s, &x| add_up(s, mul_up(x.abs(), x.abs())))
                .sqrt());
            let g = if g > 0.0 { down(g / len) } else { g };
            if g > best {
                best = g;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests;

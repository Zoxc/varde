//! The closest point on spans and patches whose feet are known: a
//! circular arc and a cubic, a paraboloid and a cylinder piece, at every
//! level. A certified foot is checked exactly (the exact gradient changes
//! sign across a span's box and has one root in its unique box; the
//! subdivision route finds a patch's foot in its box; the foot point's
//! box holds the exact points of the box's corners), the upper bound
//! against the exact distance at the point reached, the curvature bounds
//! against the exact curvatures at samples. Feet at a curvature centre
//! and past the boundary are not certified, the upper bound holding
//! there too; reversal mirrors every result to the bit.

use malachite::Rational as Q;

use super::*;
use crate::bezier::tests::{Level, Rng, pow2, q};
use crate::bezier::{Exact, Limits, NdFound, Poly, isolate_exact, isolate_nd};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

/// `1/√2` rounded: a circular arc's middle weight, near enough.
const S2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The quarter arc of radius `r` about `(cx, cy, cz)` in the plane
/// `z = cz`, from `+x` to `+y`.
fn arc(r: f64, c: [f64; 3]) -> Vec<[f64; 4]> {
    vec![
        [c[0] + r, c[1], c[2], 1.0],
        [c[0] + r, c[1] + r, c[2], S2],
        [c[0], c[1] + r, c[2], 1.0],
    ]
}

/// A polynomial cubic, not planar.
fn cubic() -> Vec<[f64; 4]> {
    vec![
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 2.0, 0.5, 1.0],
        [3.0, 2.5, -0.5, 1.0],
        [4.0, 0.5, 0.25, 1.0],
    ]
}

/// The paraboloid `z = c₂ + k·(s² + t²)`, `(x, y) = (c₀ + s, c₁ + t)`,
/// `s = 2u − 1`, `t = 2v − 1`: biquadratic, polynomial, its curvature `2k`
/// at the centre `(½, ½)`, the axis through `(c₀, c₁)`.
fn paraboloid(k: f64, c: [f64; 3]) -> Vec<[f64; 4]> {
    let x = [-1.0, 0.0, 1.0];
    let a = [k, -k, k];
    let mut net = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            net.push([c[0] + x[i], c[1] + x[j], c[2] + a[i] + a[j], 1.0]);
        }
    }
    net
}

/// A quarter of the cylinder of radius `r` about the `z` axis, `z` from 0
/// to `h` (linear in `v`, elevated to degree 2).
fn cylinder(r: f64, h: f64) -> Vec<[f64; 4]> {
    let mut net = Vec::new();
    for p in arc(r, [0.0, 0.0, 0.0]) {
        for z in [0.0, 0.5 * h, h] {
            net.push([p[0], p[1], z, p[3]]);
        }
    }
    net
}

fn like<S: Level>() -> S {
    S::of(0.0, 0.0)
}

fn zero() -> Q {
    Q::from(0u32)
}

/// The exact point `S(z) − x` and `W(z)` of the exact piece at `z`.
fn exact_at(c: &Closest<Q>, z: &[Q]) -> ([Q; 3], Q) {
    match &c.geom {
        Geom::Curve(k) => k.eval_hom(&z[0]),
        Geom::Patch(p) => {
            let [x, y, w3, w] = p.eval_hom(&z[0], &z[1]);
            ([x, y, w3], w)
        }
    }
}

/// `|S(z) − x|²` exactly.
fn exact_dist2(c: &Closest<Q>, z: &[Q]) -> Q {
    let (d, w) = exact_at(c, z);
    let w2 = &w * &w;
    d.iter().map(|x| x * x).sum::<Q>() / w2
}

fn sq(x: f64) -> Q {
    q(x) * q(x)
}

/// The bound holds the exact distance at the point reached, and is within
/// a relative `2⁻⁴⁰` of it (and `2⁻¹⁰⁰⁰` absolutely).
fn upper_holds(exact: &Closest<Q>, r: &Reached) {
    let z: Vec<Q> = r.local.iter().map(|&t| q(t)).collect();
    let d2 = exact_dist2(exact, &z);
    assert!(sq(r.upper) >= d2, "upper bound below the distance");
    let slack = q(1.0 + pow2(-40));
    assert!(
        sq(r.upper) <= q(pow2(-1000)) + &slack * &slack * &d2,
        "upper bound loose: {}",
        r.upper
    );
}

/// The exact gradient of a span changes sign across the certified box and
/// has exactly one root in the unique box.
fn span_foot_exact(exact: &Closest<Q>, f: &FootBox) {
    let g = exact.grad[0].clone();
    let p = Poly::new(g.coefs().iter().cloned()).expect("a polynomial");
    let [a, b] = f.near[0];
    let (ga, gb) = (p.eval(&q(a)), p.eval(&q(b)));
    assert!(
        (ga < 0u32 && gb > 0u32) || (ga > 0u32 && gb < 0u32),
        "no sign change across the foot's box"
    );
    let [lo, hi] = f.unique[0];
    assert!(lo <= a && b <= hi, "unique box holds the foot's box");
    let left = if hi < 1.0 {
        p.split_at(&q(hi))[0].clone()
    } else {
        p
    };
    let piece = if lo > 0.0 {
        left.split_at(&(q(lo) / q(hi)))[1].clone()
    } else {
        left
    };
    let roots = isolate_exact(
        &piece,
        Limits {
            max_depth: 60,
            max_splits: 10_000,
        },
    )
    .expect("within the limits");
    let mut count = 0;
    for e in roots.found {
        count += match e {
            Exact::Point(_) | Exact::Root { .. } => 1,
            Exact::Cluster { mult, .. } => mult,
        };
    }
    assert_eq!(count, 1, "one stationary point in the unique box");
}

/// The foot point's box holds the exact points of the foot box's
/// corners, and the spread their distance from the point reached.
fn point_holds(exact: &Closest<Q>, r: &Reached, f: &FootBox) {
    let corners: Vec<Vec<Q>> = match f.near.len() {
        1 => f.near[0].iter().map(|&t| vec![q(t)]).collect(),
        _ => {
            let mut v = Vec::new();
            for &u in &f.near[0] {
                for &w in &f.near[1] {
                    v.push(vec![q(u), q(w)]);
                }
            }
            v
        }
    };
    let at: Vec<Q> = r.local.iter().map(|&t| q(t)).collect();
    let (da, wa) = exact_at(exact, &at);
    let x = exact.point();
    for z in &corners {
        let (d, w) = exact_at(exact, z);
        let mut gap = zero();
        for k in 0..3 {
            let p = &d[k] / &w + q(x[k]);
            assert!(
                q(f.point[k][0]) <= p && p <= q(f.point[k][1]),
                "foot point box"
            );
            let e = &d[k] / &w - &da[k] / &wa;
            gap += &e * &e;
        }
        assert!(gap <= sq(f.spread), "spread below a corner's distance");
    }
}

fn certified(r: &Reached) -> &FootBox {
    match &r.foot {
        Foot::Certified(f) => f,
        other => panic!("foot not certified: {other:?}"),
    }
}

/// `t` within `tol` of the box `[a, b]`.
fn near(b: [f64; 2], t: f64, tol: f64) -> bool {
    b[0] - tol <= t && t <= b[1] + tol
}

fn spans<S: Level>() {
    let l = like::<S>();
    // Inside the arc, off its middle; on the far side of its centre, on
    // the arc's axis and seeded there, the farthest point (a maximum,
    // where the gradient is 0 by symmetry).
    for (r, c, x, min, seed) in [
        (2.0, [0.0, 0.0, 0.0], [0.7, 0.4, 0.3], true, None),
        (1.5, [10.0, -3.0, 0.25], [10.5, -2.0, 0.25], true, None),
        (1.0, [0.0, 0.0, 0.0], [3.0, 1.0, -1.0], true, None),
        (1.0, [0.0, 0.0, 0.0], [-0.3, -0.3, 0.0], false, Some([0.5])),
    ] {
        let pts = arc(r, c);
        let cl = Closest::curve(&pts, x, &l).expect("a span");
        let ex = Closest::curve(&pts, x, &zero()).expect("a span");
        let reached = cl.closest(seed.as_ref().map(|s| &s[..])).expect("a seed");
        upper_holds(&ex, &reached);
        let f = certified(&reached);
        assert_eq!(f.minimum, min, "{} minimum", S::NAME);
        span_foot_exact(&ex, f);
        point_holds(&ex, &reached, f);
        // The foot is on the ray from the centre through `x` (or away).
        let a = [x[0] - c[0], x[1] - c[1]];
        let p = [f.point[0][0] - c[0], f.point[1][0] - c[1]];
        let (cr, dt) = (a[0] * p[1] - a[1] * p[0], a[0] * p[0] + a[1] * p[1]);
        assert!(
            cr.abs() < 1e-9 * dt.abs() && (dt > 0.0) == min,
            "foot direction"
        );
    }
    // Off the axis, Newton from beside the maximum descends to the
    // nearer end instead (no stationary point between it and the end).
    let pts = arc(1.0, [0.0; 3]);
    let x = [-0.3, -0.2, 0.0];
    let cl = Closest::curve(&pts, x, &l).expect("a span");
    for seed in [None, Some([0.45]), Some([0.7])] {
        let r = cl.closest(seed.as_ref().map(|s| &s[..])).expect("a seed");
        assert_eq!(r.foot, Foot::Boundary, "{} from {seed:?}", S::NAME);
        assert_eq!(r.local[0], 1.0, "{} from {seed:?}", S::NAME);
    }
    let pts = cubic();
    for x in [[2.0, 1.0, 1.0], [0.5, 1.5, -1.0], [3.5, 1.5, 0.0]] {
        let cl = Closest::curve(&pts, x, &l).expect("a span");
        let ex = Closest::curve(&pts, x, &zero()).expect("a span");
        let reached = cl.closest(None).expect("a seed");
        upper_holds(&ex, &reached);
        let f = certified(&reached);
        assert!(f.minimum);
        span_foot_exact(&ex, f);
        point_holds(&ex, &reached, f);
    }
}

#[test]
fn span_feet_are_certified_f64() {
    spans::<Bounded<f64>>();
}

#[test]
fn span_feet_are_certified_dd() {
    spans::<Bounded<Dd>>();
}

#[test]
fn span_feet_are_certified_big() {
    spans::<Bounded<Big>>();
}

/// The foot box against the subdivision route's root of the same
/// gradient system, at double-double.
fn subdivision_agrees(pts: &[[f64; 4]], x: [f64; 3], f: &FootBox) {
    let dd = Closest::patch(2, 2, pts, x, &Bounded::exact(Dd::ZERO)).expect("a patch");
    let roots = isolate_nd(
        dd.gradient(),
        &[0.0, 0.0],
        Limits {
            max_depth: 40,
            max_splits: 200_000,
        },
    )
    .expect("a square system");
    let hit = roots.found.iter().any(|r| match r {
        NdFound::Root(r) => (0..2).all(|a| {
            let (p, b) = (r.near[a], f.near[a]);
            p[0] <= b[1] && b[0] <= p[1]
        }),
        NdFound::Cluster(_) => false,
    });
    assert!(hit, "the subdivision route finds the foot");
}

fn patches<S: Level>() {
    let l = like::<S>();
    // Below the curvature centre on the axis: the centre, a minimum;
    // above it, a maximum; off the axis, a minimum elsewhere.
    for (k, c, x, min, at) in [
        (
            1.0,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.25],
            true,
            Some([0.5, 0.5]),
        ),
        (
            1.0,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 2.0],
            false,
            Some([0.5, 0.5]),
        ),
        (
            0.75,
            [1.0e6, -2.0e6, 3.0],
            [1.0e6 + 0.3, -2.0e6 - 0.2, 2.5],
            true,
            None,
        ),
        (1.0, [0.0, 0.0, 0.0], [0.4, 0.1, -0.5], true, None),
    ] {
        let pts = paraboloid(k, c);
        let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
        let ex = Closest::patch(2, 2, &pts, x, &zero()).expect("a patch");
        let seed = at.map(|a| a.to_vec());
        let reached = cl.closest(seed.as_deref()).expect("a seed");
        upper_holds(&ex, &reached);
        let f = certified(&reached);
        assert_eq!(f.minimum, min, "{} minimum at {x:?}", S::NAME);
        point_holds(&ex, &reached, f);
        if let Some(a) = at {
            assert!(near(f.near[0], a[0], 0.0) && near(f.near[1], a[1], 0.0));
        }
        // (On the axis the other stationary points are a circle.)
        if S::NAME == "f64" && at.is_none() {
            subdivision_agrees(&pts, x, f);
        }
    }
    // Above the curvature centre, seeded beside the maximum there: Newton
    // descends to a minimum (at `s² + t² = 3/2`, distance `√7/2`) rather
    // than to the maximum it starts near (seeded off the axes: along an
    // axis the radial minimum is past the boundary).
    let pts = paraboloid(1.0, [0.0; 3]);
    let x = [0.0, 0.0, 2.0];
    let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
    let ex = Closest::patch(2, 2, &pts, x, &zero()).expect("a patch");
    for seed in [[0.45, 0.46], [0.53, 0.56], [0.56, 0.45]] {
        let reached = cl.closest(Some(&seed)).expect("a seed");
        upper_holds(&ex, &reached);
        assert!(
            reached.upper < 1.33,
            "{} from {seed:?}: {reached:?}",
            S::NAME
        );
        if let Foot::Certified(f) = &reached.foot {
            assert!(f.minimum, "{} from {seed:?}", S::NAME);
            point_holds(&ex, &reached, f);
        }
    }
    // A cylinder: the foot of a point inside is on the radius through it.
    let pts = cylinder(2.0, 3.0);
    for x in [[0.8, 0.5, 1.0], [3.0, 2.0, 2.5]] {
        let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
        let ex = Closest::patch(2, 2, &pts, x, &zero()).expect("a patch");
        let reached = cl.closest(None).expect("a seed");
        upper_holds(&ex, &reached);
        let f = certified(&reached);
        assert!(f.minimum);
        point_holds(&ex, &reached, f);
        assert!(near(f.near[1], x[2] / 3.0, 1e-12), "height");
        let p = [f.point[0][0], f.point[1][0]];
        let (cr, dt) = (x[0] * p[1] - x[1] * p[0], x[0] * p[0] + x[1] * p[1]);
        assert!(cr.abs() < 1e-9 * dt && dt > 0.0, "on the radius");
    }
}

#[test]
fn patch_feet_are_certified_f64() {
    patches::<Bounded<f64>>();
}

#[test]
fn patch_feet_are_certified_dd() {
    patches::<Bounded<Dd>>();
}

#[test]
fn patch_feet_are_certified_big() {
    patches::<Bounded<Big>>();
}

/// At the curvature centre the Jacobian is singular (every point of the
/// arc, or the centre of the paraboloid, is stationary to first order):
/// no certificate, and the upper bound holds; past the boundary the foot
/// is the boundary's.
fn uncertified<S: Level>() {
    let l = like::<S>();
    let pts = arc(1.0, [0.0, 0.0, 0.0]);
    let x = [0.0, 0.0, 0.0];
    let r = Closest::curve(&pts, x, &l)
        .expect("a span")
        .closest(None)
        .expect("a seed");
    assert!(!matches!(r.foot, Foot::Certified(_)), "centre of the arc");
    upper_holds(&Closest::curve(&pts, x, &zero()).expect("a span"), &r);

    let pts = paraboloid(1.0, [0.0, 0.0, 0.0]);
    let x = [0.0, 0.0, 0.5];
    let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
    let r = cl.closest(Some(&[0.5, 0.5])).expect("a seed");
    assert_eq!(
        r.foot,
        Foot::Subdivide,
        "curvature centre of the paraboloid"
    );
    upper_holds(
        &Closest::patch(2, 2, &pts, x, &zero()).expect("a patch"),
        &r,
    );

    for x in [[3.0, 0.2, 1.0], [-0.5, -4.0, 0.0]] {
        let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
        let r = cl.closest(None).expect("a seed");
        assert_eq!(r.foot, Foot::Boundary, "past the boundary at {x:?}");
        assert!(r.local.iter().any(|&t| t == 0.0 || t == 1.0));
        upper_holds(
            &Closest::patch(2, 2, &pts, x, &zero()).expect("a patch"),
            &r,
        );
    }
    let pts = cubic();
    let x = [-2.0, -1.0, 0.0];
    let r = Closest::curve(&pts, x, &l)
        .expect("a span")
        .closest(None)
        .expect("a seed");
    assert_eq!(r.foot, Foot::Boundary, "before the span's start");
    assert_eq!(r.local[0], 0.0);
}

#[test]
fn uncertified_feet_keep_the_upper_bound() {
    uncertified::<Bounded<f64>>();
    uncertified::<Bounded<Dd>>();
    uncertified::<Bounded<Big>>();
}

/// `1 − t`, exactly for a lattice point.
fn flip(t: f64) -> f64 {
    let r = 1.0 - t;
    assert_eq!(1.0 - r, t, "a lattice point mirrors exactly");
    r
}

fn mirrored(a: &Reached, b: &Reached, axis: usize) {
    for k in 0..a.local.len() {
        let (x, y) = (a.local[k], b.local[k]);
        if k == axis {
            assert_eq!(flip(x).to_bits(), y.to_bits(), "mirrored point");
        } else {
            assert_eq!(x.to_bits(), y.to_bits(), "same point");
        }
    }
    assert_eq!(a.upper.to_bits(), b.upper.to_bits(), "same bound");
    assert_eq!(a.work, b.work, "same work");
    match (&a.foot, &b.foot) {
        (Foot::Certified(f), Foot::Certified(g)) => {
            for k in 0..f.near.len() {
                let (n, m) = (f.near[k], g.near[k]);
                let (u, w) = (f.unique[k], g.unique[k]);
                if k == axis {
                    assert_eq!([flip(n[1]), flip(n[0])], m, "mirrored box");
                    assert_eq!([flip(u[1]), flip(u[0])], w, "mirrored unique box");
                } else {
                    assert_eq!(n, m);
                    assert_eq!(u, w);
                }
            }
            assert_eq!(f.minimum, g.minimum);
            assert_eq!(f.point, g.point, "same point box");
            assert_eq!(f.spread.to_bits(), g.spread.to_bits(), "same spread");
        }
        (x, y) => assert_eq!(x, y, "same outcome"),
    }
}

/// The patch net reversed in `u` (rows) or `v` (columns).
fn reversed(pts: &[[f64; 4]], axis: usize) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let (a, b) = if axis == 0 { (2 - i, j) } else { (i, 2 - j) };
            out.push(pts[a * 3 + b]);
        }
    }
    out
}

fn mirror<S: Level>() {
    let l = like::<S>();
    for (pts, x) in [
        (arc(2.0, [0.0, 0.0, 0.0]), [0.7, 0.4, 0.3]),
        (cubic(), [2.0, 1.0, 1.0]),
        (cubic(), [-2.0, -1.0, 0.0]),
        (arc(1.0, [0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]),
    ] {
        let rev: Vec<[f64; 4]> = pts.iter().rev().copied().collect();
        let a = Closest::curve(&pts, x, &l).expect("a span");
        let b = Closest::curve(&rev, x, &l).expect("a span");
        // The arc's centre ties its end control points: there only the
        // mirrored seeds mirror.
        if x != [0.0; 3] {
            mirrored(
                &a.closest(None).expect("a seed"),
                &b.closest(None).expect("a seed"),
                0,
            );
        }
        for t in [0.125, 0.3, 0.75] {
            let ra = a.closest(Some(&[t])).expect("a seed");
            let rb = b.closest(Some(&[1.0 - t])).expect("a seed");
            mirrored(&ra, &rb, 0);
        }
    }
    for (pts, x) in [
        (paraboloid(1.0, [0.0, 0.0, 0.0]), [0.4, 0.1, -0.5]),
        (paraboloid(0.75, [0.0, 0.0, 0.0]), [-0.3, 0.6, 0.2]),
        (cylinder(2.0, 3.0), [0.8, 0.5, 1.0]),
        (paraboloid(1.0, [0.0, 0.0, 0.0]), [3.0, 0.2, 1.0]),
    ] {
        for axis in [0, 1] {
            let rev = reversed(&pts, axis);
            let a = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
            let b = Closest::patch(2, 2, &rev, x, &l).expect("a patch");
            let seed = [0.25, 0.375];
            let mut mseed = seed;
            mseed[axis] = 1.0 - seed[axis];
            for (sa, sb) in [(None, None), (Some(&seed[..]), Some(&mseed[..]))] {
                let ra = a.closest(sa).expect("a seed");
                let rb = b.closest(sb).expect("a seed");
                mirrored(&ra, &rb, axis);
            }
        }
    }
}

#[test]
fn reversal_mirrors_the_result_to_the_bit() {
    mirror::<Bounded<f64>>();
    mirror::<Bounded<Dd>>();
    mirror::<Bounded<Big>>();
}

/// A foot found on a half is the same foot, in the original parameters.
#[test]
fn halves_find_the_same_foot() {
    let l = Bounded::exact(0.0);
    let pts = paraboloid(1.0, [0.0, 0.0, 0.0]);
    let x = [0.4, 0.1, -0.5];
    let whole = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
    let r = whole.closest(None).expect("a seed");
    let f = certified(&r).clone();
    let mut piece = whole;
    for axis in [0, 1, 0, 1] {
        let [lo, hi] = piece.split_half(axis).expect("a half");
        let cell = lo.cell();
        let mid = cell.axes[axis][1].bounds()[0];
        piece = if f.near[axis][1] <= mid { lo } else { hi };
    }
    let r = piece.closest(None).expect("a seed");
    let g = certified(&r);
    for a in 0..2 {
        assert!(f.near[a][0] <= g.near[a][1] && g.near[a][0] <= f.near[a][1]);
        let cell = piece.cell();
        let [c0, c1] = [cell.axes[a][0].bounds()[0], cell.axes[a][1].bounds()[1]];
        assert!(c0 <= r.at[a][0] && r.at[a][1] <= c1, "in the cell");
    }
    assert!(g.minimum);
    let gap = (g.point[2][0] - f.point[2][0]).abs();
    assert!(gap < 1e-12, "same foot point");
    // The seed is a parameter of the cell.
    assert!(piece.closest(Some(&[2.0, 0.5])).is_some());
    assert!(piece.closest(Some(&[0.5])).is_none());
    assert!(piece.closest(Some(&[f64::NAN, 0.5])).is_none());
}

/// The exact curvature of a span at `t`, squared.
fn span_kappa2(c: &HomCurve<Q, 3>, t: &Q) -> Q {
    let tn = c.tangent_numerator().expect("in range");
    let tv = tn.clone().map(|p| p.eval(t));
    let dv = tn.map(|p| p.derivative().eval(t));
    let cr = [
        &tv[1] * &dv[2] - &tv[2] * &dv[1],
        &tv[2] * &dv[0] - &tv[0] * &dv[2],
        &tv[0] * &dv[1] - &tv[1] * &dv[0],
    ];
    let w = c.weights().eval(t);
    let n2: Q = cr.iter().map(|x| x * x).sum();
    let t2: Q = tv.iter().map(|x| x * x).sum();
    let w4 = (&w * &w) * (&w * &w);
    w4 * n2 / (&t2 * &t2 * &t2)
}

/// The exact normal curvatures of a patch at `(u, v)` in the parameter
/// directions `dirs`, squared: `II(a, b)² / I(a, b)²` from the
/// derivatives of the point.
fn patch_kappa2(p: &HomPatch<Q>, u: &Q, v: &Q, dirs: &[(f64, f64)]) -> Vec<Q> {
    let d = p.derivatives_at(u, v, 2);
    let g = |i, j| d.get(i, j).expect("order 2").clone();
    let (su, sv) = (g(1, 0), g(0, 1));
    let (suu, suv, svv) = (g(2, 0), g(1, 1), g(0, 2));
    let dotq = |x: &[Q; 3], y: &[Q; 3]| -> Q { (0..3).map(|k| &x[k] * &y[k]).sum() };
    let n = [
        &su[1] * &sv[2] - &su[2] * &sv[1],
        &su[2] * &sv[0] - &su[0] * &sv[2],
        &su[0] * &sv[1] - &su[1] * &sv[0],
    ];
    let (l, m, nn) = (dotq(&suu, &n), dotq(&suv, &n), dotq(&svv, &n));
    let (e, f, gg) = (dotq(&su, &su), dotq(&su, &sv), dotq(&sv, &sv));
    let n2 = dotq(&n, &n);
    let two = Q::from(2u32);
    dirs.iter()
        .map(|&(a, b)| {
            let (a, b) = (q(a), q(b));
            let ii = &l * &a * &a + &two * &m * &a * &b + &nn * &b * &b;
            let i = &e * &a * &a + &two * &f * &a * &b + &gg * &b * &b;
            (&ii * &ii) / (&n2 * &i * &i)
        })
        .collect()
}

#[test]
fn curvature_bounds_hold() {
    let l = Bounded::exact(0.0);
    let samples = [0.0, 0.3, 0.5, 1.0];
    // Spans: the bound holds the exact curvature at samples, and on the
    // arc it is near `1/r` once split.
    for pts in [arc(2.0, [1.0, 2.0, 3.0]), cubic()] {
        let x = [0.0, 0.0, 0.0];
        let cl = Closest::curve(&pts, x, &l).expect("a span");
        let ex = Closest::curve(&pts, x, &zero()).expect("a span");
        let Geom::Curve(c) = &ex.geom else {
            unreachable!()
        };
        let mut pieces = vec![(cl, 0.0, 1.0)];
        for _ in 0..3 {
            pieces = pieces
                .into_iter()
                .flat_map(|(p, a, b)| {
                    let [l, r] = p.split_half(0).expect("a half");
                    let m = 0.5 * (a + b);
                    [(l, a, m), (r, m, b)]
                })
                .collect();
        }
        for (p, a, b) in &pieces {
            let k = p.curvature_bound().expect("a bound");
            for s in samples {
                let t = q(a + (b - a) * s);
                assert!(sq(k) >= span_kappa2(c, &t), "span curvature bound");
            }
        }
    }
    // On the arc it is at least `1/r` and tends to it, to first order in
    // the piece's length.
    let mut arc_pieces = vec![Closest::curve(&arc(2.0, [0.0; 3]), [0.0; 3], &l).expect("a span")];
    for depth in 0..5 {
        for p in &arc_pieces {
            let k = p.curvature_bound().expect("a bound");
            assert!(k >= 0.5 * (1.0 - 1e-9), "arc bound {k} below 1/r");
            assert!(
                depth < 4 || k <= 0.5 * 1.125,
                "arc bound {k} at depth {depth}"
            );
        }
        arc_pieces = arc_pieces
            .iter()
            .flat_map(|p| p.split_half(0).expect("halves"))
            .collect();
    }
    // Patches: the bound holds every normal curvature at samples; the
    // paraboloid's centre has `2k`, the cylinder's largest is `1/r`.
    let dirs = [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, -0.5)];
    for (pts, top) in [
        (paraboloid(1.0, [0.0, 0.0, 0.0]), 2.0),
        (paraboloid(0.5, [5.0, 5.0, 5.0]), 1.0),
        (cylinder(2.0, 3.0), 0.5),
    ] {
        let x = [0.0, 0.0, 0.0];
        let cl = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
        let ex = Closest::patch(2, 2, &pts, x, &zero()).expect("a patch");
        let Geom::Patch(p) = &ex.geom else {
            unreachable!()
        };
        let mut pieces = vec![(cl, [0.0, 0.0], [1.0, 1.0])];
        for axis in [0, 1] {
            pieces = pieces
                .into_iter()
                .flat_map(|(c, lo, hi)| {
                    let [l, r] = c.split_half(axis).expect("a half");
                    let m = 0.5 * (lo[axis] + hi[axis]);
                    let (mut hi_l, mut lo_r) = (hi, lo);
                    hi_l[axis] = m;
                    lo_r[axis] = m;
                    [(l, lo, hi_l), (r, lo_r, hi)]
                })
                .collect();
        }
        for (c, lo, hi) in &pieces {
            let k = c.curvature_bound().expect("a bound");
            for s in samples {
                for t in samples {
                    let u = q(lo[0] + (hi[0] - lo[0]) * s);
                    let v = q(lo[1] + (hi[1] - lo[1]) * t);
                    for kappa2 in patch_kappa2(p, &u, &v, &dirs) {
                        assert!(sq(k) >= kappa2, "normal curvature");
                    }
                }
            }
            if lo[0] <= 0.5 && 0.5 <= hi[0] && lo[1] <= 0.5 && 0.5 <= hi[1] || top == 0.5 {
                assert!(k >= top * (1.0 - 1e-9), "at least the largest curvature");
            }
        }
        // Down to the cell at the centre's lower left: the bound tends to
        // the largest curvature, to first order in the cell's size.
        let mut c = Closest::patch(2, 2, &pts, x, &l).expect("a patch");
        for depth in 0..16 {
            let [lo, hi] = c.split_half(depth % 2).expect("halves");
            c = if depth < 2 { lo } else { hi };
        }
        let k = c.curvature_bound().expect("a bound");
        assert!(
            k >= top * (1.0 - 1e-9) && k <= top * 1.125,
            "bound {k}, largest {top}"
        );
    }
}

/// Inputs that can't be set up.
#[test]
fn shapes() {
    let l = Bounded::exact(0.0);
    let mut pts = cubic();
    pts[1][3] = 0.0;
    assert!(
        Closest::curve(&pts, [0.0; 3], &l).is_none(),
        "a zero weight"
    );
    let pts = cubic();
    assert!(Closest::curve(&pts, [f64::NAN, 0.0, 0.0], &l).is_none());
    assert!(
        Closest::patch(2, 2, &pts, [0.0; 3], &l).is_none(),
        "too few points"
    );
    let long: Vec<[f64; 4]> = (0..23).map(|i| [f64::from(i), 0.0, 0.0, 2.0]).collect();
    assert!(
        Closest::curve(&long, [0.0; 3], &l).is_none(),
        "gradient past the degree"
    );
    let cl = Closest::curve(&cubic(), [1.0, 1.0, 1.0], &l).expect("a span");
    assert_eq!(cl.upper_bound(&[1.5]), f64::INFINITY);
    assert_eq!(cl.upper_bound(&[0.5, 0.5]), f64::INFINITY);
}

fn random_pts(rng: &mut Rng, count: usize, rational: bool) -> Vec<[f64; 4]> {
    (0..count)
        .map(|_| {
            let mut c = || 4.0 * rng.0.unit() - 2.0;
            let p = [c(), c(), c()];
            let w = if rational {
                0.5 + 1.5 * rng.0.unit()
            } else {
                1.0
            };
            [p[0], p[1], p[2], w]
        })
        .collect()
}

/// Random spans and patches and points: the upper bound always holds,
/// every certified foot checks exactly, and reversal mirrors; from the
/// default seed, Newton's descent reaches a saddle or maximum rarely.
fn random<S: Level>(seeds: std::ops::Range<u64>) {
    let l = like::<S>();
    let mut stats = [0u32; 2];
    let mut not_min = [0u32; 2];
    let total = seeds.end - seeds.start;
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let n = 1 + rng.below(4) as usize;
        let rational = rng.below(2) == 0;
        let pts = random_pts(&mut rng, n + 1, rational);
        let x = random_pts(&mut rng, 1, false)[0];
        let x = [x[0], x[1], x[2]];
        let cl = Closest::curve(&pts, x, &l).expect("a span");
        let ex = Closest::curve(&pts, x, &zero()).expect("a span");
        let r = cl.closest(None).expect("a seed");
        upper_holds(&ex, &r);
        if let Foot::Certified(f) = &r.foot {
            stats[0] += 1;
            not_min[0] += u32::from(!f.minimum);
            span_foot_exact(&ex, f);
            point_holds(&ex, &r, f);
        }
        let rev: Vec<[f64; 4]> = pts.iter().rev().copied().collect();
        let t = r.local[0];
        let a = cl.closest(Some(&[t])).expect("a seed");
        let b = Closest::curve(&rev, x, &l)
            .expect("a span")
            .closest(Some(&[flip(t)]))
            .expect("a seed");
        mirrored(&a, &b, 0);

        let (m, n) = (1 + rng.below(3) as usize, 1 + rng.below(3) as usize);
        let pts = random_pts(&mut rng, (m + 1) * (n + 1), rational);
        let cl = Closest::patch(m, n, &pts, x, &l).expect("a patch");
        let ex = Closest::patch(m, n, &pts, x, &zero()).expect("a patch");
        let r = cl.closest(None).expect("a seed");
        upper_holds(&ex, &r);
        if let Foot::Certified(f) = &r.foot {
            stats[1] += 1;
            not_min[1] += u32::from(!f.minimum);
            point_holds(&ex, &r, f);
        }
    }
    // Many feet certify (the rest: past the boundary mostly).
    assert!(
        total < 24 || (u64::from(stats[0]) >= total / 3 && u64::from(stats[1]) >= total / 6),
        "{} certified: {stats:?} of {total}",
        S::NAME
    );
    // Before the descent safeguard about one in ten patches' feet were
    // saddles or maxima.
    assert!(
        u64::from(not_min[0] + not_min[1]) * 50 <= total,
        "{} not minima: {not_min:?} of {total}",
        S::NAME
    );
}

#[test]
fn random_pieces_hold_f64() {
    random::<Bounded<f64>>(varde_testing::seeds(48, 400));
}

#[test]
fn random_pieces_hold_dd() {
    random::<Bounded<Dd>>(varde_testing::seeds(48, 400));
}

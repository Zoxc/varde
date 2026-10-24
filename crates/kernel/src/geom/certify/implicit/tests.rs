//! Distances to implicit surfaces against the true distances of sampled
//! points, computed in big floats: random curves of one to three
//! weighted spans near planes, spheres, cylinders and ring tori in random
//! frames, each bound checked against the distance sampled densely and
//! maximised locally; curves on the surface bounded by rounding; the
//! ball condition refusing a curve through the sphere's centre; charges
//! the same on any number of threads and the same bits everywhere.

use malachite::Float;
use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::HomCurve;
use varde_decide::leaves::near_quadric::Quadric;
use varde_decide::num::trig::{cos, sin};
use varde_testkit::rng::Rng;

use super::calibration::{CellsError, Terms, alone, arcs, cells};
use super::*;
use crate::geom::certify::TIGHT;
use crate::geom::curve::Span3;
use crate::infra::budget::{Budget, Halt, MAX_SUBDIV_DEPTH, MAX_WORK, Pass, Work};
use crate::infra::error::{Cause, KernelError};

/// A target: a quarter of the least tolerance, `10⁻⁵` mm.
const TARGET: f64 = 2.5e-6;

/// The big floats' precision, in bits.
const PREC: u64 = 320;

/// A surface with its true distance.
enum Surf {
    /// `n·(x − o) = 0`.
    Plane(Terms, [f64; 3], [f64; 3]),
    /// About the centre, of radius `r`.
    Sphere(Terms, [f64; 3], f64),
    /// The cylinder as the quadric stores it.
    Cylinder(Quadric),
    /// About the centre and coordinate axis `k`, of radii `R` and `r`.
    Torus(Terms, [f64; 3], usize, f64, f64),
}

impl Surf {
    /// The bound towards `target` on a budget of `budget` units.
    fn bound(
        &self,
        c: &Curve3D,
        target: f64,
        budget: u64,
    ) -> Result<DistanceBound, Halt<DistanceError>> {
        let mut work = Work::new(&Budget::new(budget));
        let pass = Pass {
            stage: "distance",
            rung: 0,
        };
        let out = work.pass(pass, &[()], |_, ledger| match self {
            Surf::Plane(t, ..) | Surf::Sphere(t, ..) | Surf::Torus(t, ..) => {
                implicit_distance(t, c, target, ledger)
            }
            Surf::Cylinder(q) => implicit_distance(q, c, target, ledger),
        });
        out.map(|b| b[0])
    }

    /// The true distance of the exact point `x`.
    fn distance(&self, x: &[Q; 3]) -> Float {
        let y = |o: &[f64; 3]| -> [Float; 3] { std::array::from_fn(|k| big(&(&x[k] - q(o[k])))) };
        match self {
            Surf::Plane(_, o, n) => {
                let y = y(o);
                let n = n.map(|v| big(&q(v)));
                let dot = dot(&y, &n);
                let len = sqrt(dot_self(&n));
                abs(dot / len)
            }
            Surf::Sphere(_, c, r) => abs(sqrt(dot_self(&y(c))) - big(&q(*r))),
            Surf::Cylinder(cyl) => {
                let y = y(&cyl.foot());
                let a = cyl.axis().map(|v| big(&q(v)));
                let along = dot(&y, &a);
                let off = dot_self(&y) - along.clone() * along / dot_self(&a);
                abs(sqrt(off) - big(&q(cyl.radius())))
            }
            Surf::Torus(_, c, k, big_r, r) => {
                let y = y(c);
                let h = y[*k].clone();
                let rho = sqrt(dot_self(&y) - h.clone() * h.clone());
                let d = rho - big(&q(*big_r));
                abs(sqrt(d.clone() * d + h.clone() * h) - big(&q(*r)))
            }
        }
    }
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn big(x: &Q) -> Float {
    Float::from_rational_prec(x.clone(), PREC).0
}

fn sqrt(x: Float) -> Float {
    let zero = Float::from_rational_prec(Q::from(0u32), PREC).0;
    let x = if x < zero { zero } else { x };
    x.sqrt_prec(PREC).0
}

fn abs(x: Float) -> Float {
    let zero = Float::from_rational_prec(Q::from(0u32), PREC).0;
    if x < zero { -x } else { x }
}

fn dot(a: &[Float; 3], b: &[Float; 3]) -> Float {
    a[0].clone() * b[0].clone() + a[1].clone() * b[1].clone() + a[2].clone() * b[2].clone()
}

fn dot_self(a: &[Float; 3]) -> Float {
    dot(a, a)
}

fn to_f64(x: &Float) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The exact point of `c` at `t` in span `k`'s local parameter.
fn point(c: &Curve3D, k: usize, t: &Q) -> [Q; 3] {
    let zero = Q::from(0u32);
    HomCurve::<Q, 3>::from_affine(
        c.spans()[k].iter().map(|h| ([h[0], h[1], h[2]], h[3])),
        &zero,
    )
    .unwrap()
    .eval(t)
}

/// The greatest true distance found on `c`: `n` samples per span, then a
/// golden-section search about the best of each.
fn max_distance(s: &Surf, c: &Curve3D, n: u32) -> Float {
    let mut most = Float::from_rational_prec(Q::from(0u32), PREC).0;
    for k in 0..c.spans().len() {
        let at = |i: u32| Q::from(i) / Q::from(n);
        let dist = |t: &Q| s.distance(&point(c, k, t));
        let mut best = (0u32, dist(&at(0)));
        for i in 1..=n {
            let d = dist(&at(i));
            if d > best.1 {
                best = (i, d);
            }
        }
        let (mut lo, mut hi) = (at(best.0.saturating_sub(1)), at((best.0 + 1).min(n)));
        let mut top = best.1;
        let g = Q::from_signeds(618_034i64, 1_000_000);
        for _ in 0..24 {
            let m1 = &hi - (&hi - &lo) * &g;
            let m2 = &lo + (&hi - &lo) * &g;
            let (d1, d2) = (dist(&m1), dist(&m2));
            if d1 > top {
                top = d1.clone();
            }
            if d2 > top {
                top = d2.clone();
            }
            if d1 > d2 {
                hi = m2;
            } else {
                lo = m1;
            }
            // Keep the rationals short.
            lo = q(f64::rounding_from(&lo, Nearest).0);
            hi = q(f64::rounding_from(&hi, Nearest).0);
        }
        if top > most {
            most = top;
        }
    }
    most
}

/// The bound towards `target` if it can be given, checked against the
/// distance sampled; with that distance.
fn assert_holds(s: &Surf, c: &Curve3D, target: f64, what: &str) -> Option<(DistanceBound, f64)> {
    let b = s.bound(c, target, MAX_WORK).ok()?;
    let d = max_distance(s, c, 12);
    // The samples' distances are within `2^-300` of the true ones, far
    // below the bound's rounding.
    let slack = Float::from_rational_prec(Q::from_signeds(1i64, 1i64 << 62), PREC).0;
    let slack = slack.clone() * slack.clone() * slack.clone() * slack;
    assert!(
        big(&q(b.bound)) + slack >= d,
        "{what}: bound {} below the distance {}",
        b.bound,
        to_f64(&d)
    );
    Some((b, to_f64(&d)))
}

/// A radius of up to 256 mm on a grid of `2⁻⁴`, so that its square and
/// the torus's coefficients are exact.
fn radius(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    let r = (rng.log_range(lo, hi) * 16.0).round() / 16.0;
    if r > 1.0 / 16.0 { r } else { 1.0 / 16.0 }
}

/// Two unit vectors at right angles to `a` and to each other (in
/// `f64`).
fn frame(a: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let a = a.map(|x| x / n);
    let t = if a[0].abs() < 0.6 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let d = t[0] * a[0] + t[1] * a[1] + t[2] * a[2];
    let e1 = [t[0] - d * a[0], t[1] - d * a[1], t[2] - d * a[2]];
    let n1 = (e1[0] * e1[0] + e1[1] * e1[1] + e1[2] * e1[2]).sqrt();
    let e1 = e1.map(|x| x / n1);
    let e2 = [
        a[1] * e1[2] - a[2] * e1[1],
        a[2] * e1[0] - a[0] * e1[2],
        a[0] * e1[1] - a[1] * e1[0],
    ];
    (e1, e2)
}

fn axpy(o: [f64; 3], terms: &[(f64, [f64; 3])]) -> [f64; 3] {
    std::array::from_fn(|k| o[k] + terms.iter().map(|(s, v)| s * v[k]).sum::<f64>())
}

/// A random surface and a map from two parameters to its points, the
/// second parameter's range a small step.
#[allow(clippy::type_complexity)]
fn surface(rng: &mut Rng) -> (Surf, Box<dyn Fn(f64, f64) -> [f64; 3]>) {
    let centre = rng.point(100.0).to_array();
    match rng.next_u64() % 4 {
        0 => {
            let n = rng
                .direction()
                .to_array()
                .map(|x| x * rng.log_range(0.1, 10.0));
            let (e1, e2) = frame(n);
            let scale = rng.log_range(1.0, 300.0);
            (
                Surf::Plane(Terms::plane(centre, n), centre, n),
                Box::new(move |a, b| axpy(centre, &[(scale * a, e1), (scale * b, e2)])),
            )
        }
        1 => {
            let r = radius(rng, 0.5, 200.0);
            let axis = rng.direction().to_array();
            let (e1, e2) = frame(axis);
            let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
            let axis = axis.map(|x| x / n);
            (
                Surf::Sphere(Terms::sphere(centre, r), centre, r),
                Box::new(move |a, b| {
                    let (ca, sa, cb, sb) = (cos(a), sin(a), cos(b), sin(b));
                    axpy(
                        centre,
                        &[(r * cb * ca, e1), (r * cb * sa, e2), (r * sb, axis)],
                    )
                }),
            )
        }
        2 => {
            let r = rng.log_range(0.5, 200.0);
            let axis = rng
                .direction()
                .to_array()
                .map(|x| x * rng.log_range(0.1, 10.0));
            let (e1, e2) = frame(axis);
            let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
            let unit = axis.map(|x| x / n);
            (
                Surf::Cylinder(Quadric::cylinder(centre, axis, r).unwrap()),
                Box::new(move |a, b| {
                    axpy(
                        centre,
                        &[(r * cos(a), e1), (r * sin(a), e2), (r * b * 4.0, unit)],
                    )
                }),
            )
        }
        _ => {
            let k = (rng.next_u64() % 3) as usize;
            let big_r = radius(rng, 2.0, 200.0);
            let r = (big_r * rng.range(0.1, 0.8) * 16.0).round();
            let r = if r > 1.0 { r } else { 1.0 } / 16.0;
            let mut axis = [0.0; 3];
            axis[k] = 1.0;
            let (e1, e2) = ([(k + 1) % 3, (k + 2) % 3])
                .map(|i| {
                    let mut e = [0.0; 3];
                    e[i] = 1.0;
                    e
                })
                .into();
            (
                Surf::Torus(Terms::torus(centre, k, big_r, r), centre, k, big_r, r),
                Box::new(move |a, b| {
                    // `b` the tube's angle, anywhere round.
                    let b = b * 8.0;
                    let rho = big_r + r * cos(b);
                    axpy(
                        centre,
                        &[(rho * cos(a), e1), (rho * sin(a), e2), (r * sin(b), axis)],
                    )
                }),
            )
        }
    }
}

/// A weight: 1, a power of two, or any value in `[1/8, 8]`.
fn weight(rng: &mut Rng) -> f64 {
    match rng.next_u64() % 3 {
        0 => 1.0,
        1 => [0.125, 8.0, 0.5, 2.0][(rng.next_u64() % 4) as usize],
        _ => rng.log_range(0.125, 8.0),
    }
}

/// A random curve of one to three spans near the surface: its knot
/// points on it, each middle control point through the surface's point
/// between them, moved by up to `wiggle`, every weight random.
fn curve(rng: &mut Rng, at: &dyn Fn(f64, f64) -> [f64; 3], wiggle: f64) -> Curve3D {
    let n = 1 + (rng.next_u64() % 3) as usize;
    let mut uv = vec![[rng.range(-3.0, 3.0), rng.range(-0.5, 0.5)]];
    for _ in 0..n {
        let [a, b] = *uv.last().unwrap();
        uv.push([a + rng.range(-0.15, 0.15), b + rng.range(-0.02, 0.02)]);
    }
    let ends: Vec<[f64; 3]> = uv.iter().map(|p| at(p[0], p[1])).collect();
    let spans: Vec<Span3> = (0..n)
        .map(|k| {
            let (a, b) = (ends[k], ends[k + 1]);
            let m = at(
                0.5 * (uv[k][0] + uv[k + 1][0]),
                0.5 * (uv[k][1] + uv[k + 1][1]),
            );
            let [wa, wm, wb] = [(); 3].map(|_| weight(rng));
            let m: [f64; 3] = std::array::from_fn(|c| {
                2.0 * m[c] - 0.5 * (a[c] + b[c]) + rng.range(-wiggle, wiggle)
            });
            [
                [a[0], a[1], a[2], wa],
                [m[0], m[1], m[2], wm],
                [b[0], b[1], b[2], wb],
            ]
        })
        .collect();
    let mut knots = vec![rng.range(-2.0, 2.0)];
    for _ in 0..n {
        let last = *knots.last().unwrap();
        knots.push(last + rng.range(0.25, 2.0));
    }
    Curve3D::new(spans, knots).unwrap()
}

#[test]
fn distances_hold_against_big_float_samples() {
    let mut certified = 0;
    let seeds = varde_testing::seeds(40, 1000);
    let total = seeds.end - seeds.start;
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let (s, at) = surface(&mut rng);
        let wiggle = [0.0, 1e-6, 1e-2][(rng.next_u64() % 3) as usize];
        let c = curve(&mut rng, &*at, wiggle);
        let target = [TARGET, 1e-9][(rng.next_u64() % 2) as usize];
        let what = format!("seed {seed}");
        if let Some((b, seen)) = assert_holds(&s, &c, target, &what) {
            certified += 1;
            // Above the target only where the distance is: within a small
            // factor of it (sampled densely before giving up).
            let seen = if b.bound <= target || b.bound <= 2.0 * seen {
                seen
            } else {
                to_f64(&max_distance(&s, &c, 400))
            };
            assert!(
                b.bound <= target || b.bound <= 2.0 * seen,
                "{what}: {} for {seen}, target {target}",
                b.bound
            );
        }
    }
    assert_eq!(certified, total, "{certified} of {total} certified");
}

/// The quarter circle of radius `r` about `c` in the plane of `e1` and
/// `e2` (exact unit vectors), end weights `w` and `1/w`.
fn quarter(c: [f64; 3], e1: [f64; 3], e2: [f64; 3], r: f64, w: f64) -> Curve3D {
    let mid = std::f64::consts::FRAC_1_SQRT_2;
    let p = |a: f64, b: f64| axpy(c, &[(a, e1), (b, e2)]);
    let [a, m, b] = [p(r, 0.0), p(r, r), p(0.0, r)];
    Curve3D::new(
        vec![[
            [a[0], a[1], a[2], w],
            [m[0], m[1], m[2], mid],
            [b[0], b[1], b[2], 1.0 / w],
        ]],
        vec![0.0, 1.0],
    )
    .unwrap()
}

#[test]
fn curves_on_the_surface_are_bounded_by_rounding() {
    // Quarter circles lying on a sphere, a torus (its outer, inner and
    // top circles) and a plane, their control points exact but for the
    // middle weight `√2/2`: each bound is that rounding's, far below the
    // target, at the first level.
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let o = [12.5, -3.0, 40.0];
    let cases: Vec<(Surf, Curve3D, f64)> = vec![
        (
            Surf::Sphere(Terms::sphere(o, 50.0), o, 50.0),
            quarter(o, x, z, 50.0, 1.0),
            50.0,
        ),
        (
            Surf::Sphere(Terms::sphere(o, 50.0), o, 50.0),
            quarter(o, y, x, 50.0, 4.0),
            50.0,
        ),
        (
            Surf::Torus(Terms::torus(o, 2, 64.0, 16.0), o, 2, 64.0, 16.0),
            quarter(o, x, y, 80.0, 1.0),
            80.0,
        ),
        (
            Surf::Torus(Terms::torus(o, 2, 64.0, 16.0), o, 2, 64.0, 16.0),
            quarter(o, x, y, 48.0, 2.0),
            80.0,
        ),
        (
            Surf::Torus(Terms::torus(o, 2, 64.0, 16.0), o, 2, 64.0, 16.0),
            quarter(axpy(o, &[(16.0, z)]), x, y, 64.0, 0.5),
            80.0,
        ),
        (
            Surf::Plane(Terms::plane(o, [0.0, 0.0, 3.0]), o, [0.0, 0.0, 3.0]),
            quarter(o, x, y, 50.0, 3.0),
            50.0,
        ),
    ];
    for (i, (s, c, scale)) in cases.iter().enumerate() {
        let what = format!("case {i}");
        let (b, _) = assert_holds(s, c, TARGET, &what).expect(&what);
        assert_eq!(b.level, 0, "{what}: {b:?}");
        assert!(b.bound <= 1e-12 * scale, "{what}: {b:?}");
    }
}

#[test]
fn offset_lines_are_bounded_near_their_distance() {
    // A line at `d` above a plane: `|F|/|∇F|` is `d` exactly, so the
    // bound is `d` and the rounding.
    let n = [0.0, 0.0, 2.0];
    let s = Surf::Plane(Terms::plane([0.0; 3], n), [0.0; 3], n);
    for d in [1e-3, 0.25, 7.0] {
        let p = |t: f64| [t, 2.0 * t - 1.0, d, 1.0];
        let c = Curve3D::new(vec![[p(-20.0), p(0.0), p(30.0)]], vec![0.0, 1.0]).unwrap();
        let (b, seen) = assert_holds(&s, &c, TARGET, &format!("{d}")).unwrap();
        assert!(b.bound <= d * (1.0 + 1e-12), "{d}: {b:?}");
        assert!(seen >= d * (1.0 - 1e-12), "{d}: {seen}");
    }
}

#[test]
fn the_ball_condition_refuses_a_curve_through_the_centre() {
    // The sphere's implicit has no gradient at its centre: a line through
    // it has no ball about the centre with `|∇F|` bounded away from 0, at
    // any depth. The bound stops at the deepest halving, a validity
    // failure; moved a fifth of the radius from the surface the line is
    // bounded.
    let s = Surf::Sphere(Terms::sphere([0.0; 3], 10.0), [0.0; 3], 10.0);
    let line = |y: f64| {
        let p = |t: f64| [t, y, 0.0, 1.0];
        Curve3D::new(vec![[p(-1.0), p(0.0), p(1.0)]], vec![0.0, 1.0]).unwrap()
    };
    match s.bound(&line(0.0), TARGET, MAX_WORK) {
        Err(Halt::OutOfBudget(KernelError::OutOfBudget { cause, .. })) => {
            assert_eq!(cause, Cause::SizeLimit);
        }
        other => panic!("{other:?}"),
    }
    let (b, seen) = assert_holds(&s, &line(9.0), TARGET, "inside").unwrap();
    assert!(b.bound <= 1.5 * seen, "{b:?} for {seen}");
    assert_eq!(b.level, 0, "{b:?}");
}

#[test]
fn rows_far_along_a_profile_cylinder_escalate_instead_of_halving() {
    // A conic-chain cylinder's base tiles reach `CELL/2` along its axis:
    // there the first level's `|∇F|²` net and `|F|` are rounding (the
    // span's conic read through the dual basis cancels `2·10⁶` mm along
    // the axis), not geometry. Their profile rows take the next level and
    // are bounded there by their rounding, in a few cells. The profile's
    // plane at random to the axis.
    use crate::geom::conic::Conic;
    use crate::geom::form::ConicCylinderKey;
    use crate::geom::lattice::Lattice;
    let mut rng = Rng::new(17);
    for _ in 0..4 {
        let a = rng.direction();
        let (e1, e2) = rng.direction().any_orthonormal_pair();
        let r = rng.log_range(0.1, 100.0);
        let c = rng.point(1e3).to_array();
        let e = Conic::ellipse(c, [e1.to_array(), e2.to_array()], [r, 0.6 * r]).unwrap();
        let spans: Vec<_> = (0..3).map(|k| e.quarter::<4>(k).unwrap()).collect();
        let key = ConicCylinderKey::new(a.to_array(), &spans).unwrap();
        let lattice = Lattice::new(key.into()).unwrap();
        for k in 0..3 {
            let tile = lattice.base_tile(k, 0).unwrap();
            let imp = lattice.implicit(k, 0).unwrap();
            let net = tile.net();
            for row in [[0, 3, 6], [2, 5, 8]] {
                let span = row.map(|n| net[n]);
                let curve = Curve3D::new(vec![span], vec![0.0, 1.0]).unwrap();
                let (b, used) = alone(&imp, &curve, 1e-8, MAX_WORK);
                let b = b.unwrap_or_else(|| panic!("cell {k} row {row:?}: no bound"));
                assert!(
                    b.bound <= 1e-8 && b.level >= 1,
                    "cell {k} row {row:?}: {b:?}"
                );
                assert!(used < 20_000, "cell {k} row {row:?}: {used} units");
            }
        }
    }
}

#[test]
fn far_weighted_arcs_settle_within_tight_of_their_distance() {
    // The fixtures' arcs, their middle control point moved by 10⁻³ mm:
    // out of reach of the target, each bound settles within `TIGHT` of
    // its distance.
    let (sphere, near_sphere, torus, near_torus) = arcs();
    for (s, c) in [
        (Surf::Sphere(sphere, [0.0; 3], 50.0), near_sphere),
        (Surf::Torus(torus, [0.0; 3], 2, 64.0, 16.0), near_torus),
    ] {
        let (b, seen) = assert_holds(&s, &c, TARGET, "arc").unwrap();
        assert!(seen > 1e-4, "{seen}");
        assert!(b.bound <= TIGHT * seen * 1.01, "{b:?} for {seen}");
    }
}

#[test]
fn degrees_outside_the_table_are_refused() {
    let c = arcs().1;
    let constant = Terms {
        degree: 0,
        origin: [0.0; 3],
        terms: vec![([0, 0, 0], 1.0)],
    };
    let quintic = Terms {
        degree: 5,
        origin: [0.0; 3],
        terms: vec![([5, 0, 0], 1.0), ([0, 0, 0], -1.0)],
    };
    for (t, d) in [(constant, 0), (quintic, 5)] {
        let mut work = Work::new(&Budget::new(MAX_WORK));
        let pass = Pass {
            stage: "distance",
            rung: 0,
        };
        let out = work.pass(pass, &[()], |_, ledger| {
            implicit_distance(&t, &c, TARGET, ledger)
        });
        assert_eq!(
            out,
            Err(Halt::Failed {
                item: 0,
                error: DistanceError::Degree(d)
            })
        );
    }
}

#[test]
fn calibration_cells_refuse_a_level_or_depth_the_distance_lacks() {
    let (sphere, c, ..) = arcs();
    assert_eq!(cells(&sphere, &c, LEVELS - 1, 2), Ok(4));
    assert_eq!(
        cells(&sphere, &c, LEVELS, 0),
        Err(CellsError::Level(LEVELS))
    );
    let deep = MAX_SUBDIV_DEPTH + 1;
    assert_eq!(cells(&sphere, &c, 0, deep), Err(CellsError::Depth(deep)));
}

/// The distances' escalations as text: per target, each arc's outcome
/// and units used, then budgets running out along it. The fixtures' arcs
/// near the sphere and the torus, and quarter circles on them, which
/// climb the schedule towards the smaller targets.
fn escalations() -> String {
    let (sphere, near_sphere, torus, near_torus) = arcs();
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let curves = [
        (&sphere, near_sphere),
        (&torus, near_torus),
        (&sphere, quarter([0.0; 3], x, z, 50.0, 3.0)),
        (&torus, quarter([0.0; 3], y, x, 48.0, 2.0)),
    ];
    let mut text = String::new();
    for target in [TARGET, 1e-15, 0.0] {
        for (k, (f, c)) in curves.iter().enumerate() {
            let out = alone(*f, c, target, MAX_WORK);
            text += &format!("{target:e} {k}: {out:?}\n");
            for budget in [1, out.1 / 2, out.1 - 1] {
                text += &format!("{budget}: {:?}\n", alone(*f, c, target, budget));
            }
        }
    }
    text
}

/// The distances' escalations and charges ([`escalations`]) as bits:
/// the same on every platform and thread count. A change of the
/// distance, the schedule or the unit table changes the hash, which then
/// needs updating here.
#[test]
fn distance_escalations_are_the_same_bits_everywhere() {
    #[cfg(not(target_arch = "wasm32"))]
    let text = crate::infra::par::assert_deterministic(escalations);
    #[cfg(target_arch = "wasm32")]
    let text = escalations();
    let h = text.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    });
    println!("hash of the escalations: {h:#018x}");
    assert_eq!(
        h, 0xef0f_4197_edab_bcbc,
        "escalations differ from the reference bits"
    );
}

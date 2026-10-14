//! Sharp-corner records on the plane `(u, v) ↦ (u, v, 0)` and a sheared
//! one: a line tangent to a circle (contact order 2, the gap bounds
//! against the circle's exact offset), two lines `2⁻⁶` rad apart (order
//! 1), two parabolas of equal curvature (order 3, against sampled
//! offsets), angles either side of `θ_SHARP`, the cut at the coarsest
//! dyadic past the bands, each curve's reach within it, and the corner
//! refused where the curves are one.

use super::*;

fn id_jacobian() -> ([Bounded<f64>; 3], [Bounded<f64>; 3]) {
    let e = Bounded::exact;
    ([e(1.0), e(0.0), e(0.0)], [e(0.0), e(1.0), e(0.0)])
}

fn line(a: [f64; 2], b: [f64; 2]) -> CuspSpan {
    [
        [a[0], a[1], 1.0],
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, 1.0],
        [b[0], b[1], 1.0],
    ]
}

const W: [f64; 2] = [1.25, 0.5];

/// A quarter of the circle of radius 1 tangent to `+u` at `W`, centre
/// `W + (0, 1)`, from `W`: control point `W + (1, 0)`, weight `√½`
/// rounded (a conic, its curvature at `W` within an ulp of 1).
fn quarter() -> CuspSpan {
    [
        [W[0], W[1], 1.0],
        [W[0] + 1.0, W[1], std::f64::consts::FRAC_1_SQRT_2],
        [W[0] + 1.0, W[1] + 1.0, 1.0],
    ]
}

fn sharp(a: &CuspSpan, b: &CuspSpan, bands: f64) -> Result<Sharp, CuspError> {
    let (du, dv) = id_jacobian();
    cusp_rec(a, b, &du, &dv, bands)
}

fn rec(a: &CuspSpan, b: &CuspSpan, bands: f64) -> CuspRec {
    match sharp(a, b, bands) {
        Ok(Sharp::Cusp(r)) => *r,
        other => panic!("not a cusp: {other:?}"),
    }
}

fn f(x: &Rational) -> f64 {
    f64::rounding_from(x, RoundingMode::Nearest).0
}

/// The point of a span at `t`, in floats.
fn at(s: &CuspSpan, t: f64) -> [f64; 2] {
    let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let w: f64 = (0..3).map(|i| b[i] * s[i][2]).sum();
    let c = |k: usize| (0..3).map(|i| b[i] * s[i][2] * s[i][k]).sum::<f64>() / w;
    [c(0), c(1)]
}

/// The offset (`v − W_v`) of a span from `W` that is a graph over `u`
/// near it, at `u = W_u + s`, by bisection on its parameter.
fn offset(sp: &CuspSpan, s: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if at(sp, mid)[0] - W[0] < s {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(sp, 0.5 * (lo + hi))[1] - W[1]
}

/// The cut's global coordinate.
fn kappa(r: &CuspRec) -> f64 {
    r.cut.0 as f64 + r.cut.1.bounds()[0]
}

/// The common checks: the bounds against sampled offsets, the cut past
/// the bands and as coarse as can be, the reaches within it.
fn check(a: &CuspSpan, b: &CuspSpan, r: &CuspRec, bands: f64, slack: f64) {
    let m = r.m as i32;
    let (lo, hi) = (f(&r.a_lo), f(&r.a_hi));
    let s_c = r.s_c_f64();
    assert!(s_c > 0.0);
    for i in 1..=200 {
        let s = s_c * i as f64 / 200.0;
        #[allow(clippy::disallowed_methods, reason = "a test reference")]
        let sm = s.powi(m);
        let gap = (offset(b, s) - offset(a, s)).abs();
        assert!(
            gap >= lo * sm * (1.0 - slack) && gap <= hi * sm * (1.0 + slack),
            "gap {gap} at {s} outside [{}, {}]",
            lo * sm,
            hi * sm
        );
    }
    // The cut: past the bands, within `s_c`, on the grid of its depth
    // and no grid point of a coarser depth would do.
    let s_beta = r.s_beta_f64();
    assert!(s_beta <= s_c && s_beta > 0.0);
    assert!(&r.a_lo * r.s_beta.clone().pow(r.m as u64) > Rational::try_from(bands).unwrap());
    assert_eq!(r.axis, Dir::U);
    assert_eq!(r.toward, 1);
    let k = kappa(r);
    assert!((k - W[0] - s_beta).abs() <= 1e-12 * (1.0 + k.abs()));
    assert!(r.cut.1.exp() <= CUT_DEPTH);
    // Each reach: its point within the cut, the next grid point past it.
    for (sp, reach) in [a, b].into_iter().zip(r.reach) {
        let t = reach.bounds()[0];
        assert!(at(sp, t)[0] - W[0] <= s_beta * (1.0 + 1e-12) + 1e-15);
        assert!(t > 0.0);
    }
}

#[test]
fn a_line_tangent_to_a_circle_is_order_two() {
    let b = line(W, [W[0] + 1.0, W[1]]);
    let a = quarter();
    for bands in [2e-8, 2e-5, 1.0 / 64.0] {
        let r = rec(&a, &b, bands);
        assert_eq!(r.m, 2);
        // Half the curvature: `1/2`, within the rounded weight.
        let g = f(&r.a_lo) * 2.0;
        assert!((g - 0.5).abs() < 1e-12, "{g}");
        check(&a, &b, &r, bands, 1e-9);
        // The spike's length about `√(2β/A_lo)`… within a factor of two
        // of the threshold `√(β/A_lo)`, once the grid is fine enough.
        #[allow(clippy::disallowed_methods, reason = "a test reference")]
        let threshold = (bands / f(&r.a_lo)).sqrt();
        assert!(r.s_beta_f64() >= threshold && r.s_beta_f64() <= 2.0 * threshold + 1e-15);
    }
    // The order of the curves doesn't change the record, but for the
    // reaches' order.
    let (r1, r2) = (rec(&a, &b, 2e-8), rec(&b, &a, 2e-8));
    assert_eq!(
        (r1.m, &r1.a_lo, &r1.s_beta, r1.cut),
        (r2.m, &r2.a_lo, &r2.s_beta, r2.cut)
    );
    assert_eq!(r1.reach, [r2.reach[1], r2.reach[0]]);
}

#[test]
fn an_acute_corner_is_order_one() {
    // Two lines `2⁻⁶` rad apart: slopes 0 and `tan 2⁻⁶`.
    let slope = crate::num::trig::tan(1.0 / 64.0);
    let a = line(W, [W[0] + 1.0, W[1] + slope]);
    let b = line(W, [W[0] + 1.0, W[1]]);
    let r = rec(&a, &b, 1e-8);
    assert_eq!(r.m, 1);
    assert!((f(&r.a_lo) * 2.0 - slope).abs() < 1e-15);
    check(&a, &b, &r, 1e-8, 1e-9);
}

#[test]
fn equal_curvatures_are_a_higher_order() {
    // `v = u²/2` from `W`, and `u = 2t + t²/2`, `v = 2t²`: the same
    // curvature at `W`, apart at order 3 by `1/8`.
    let p = |c: f64| -> CuspSpan {
        [
            [W[0], W[1], 1.0],
            [W[0] + 1.0, W[1], 1.0],
            [W[0] + 2.0 + c, W[1] + 2.0, 1.0],
        ]
    };
    let (a, b) = (p(0.0), p(0.5));
    let r = rec(&a, &b, 1e-8);
    assert_eq!(r.m, 3);
    assert_eq!(r.a_lo, Rational::from(1) / Rational::from(16));
    check(&a, &b, &r, 1e-8, 1e-6);
    // The circle against the parabola of its curvature: the weight's
    // rounding leaves the curvatures an ulp apart, order 2 with a
    // coefficient so small that the spike reaches past the bounds' stretch.
    match sharp(&quarter(), &a, 1e-8) {
        Ok(Sharp::Cusp(r)) => check(&quarter(), &a, &r, 1e-8, 1e-6),
        other => assert_eq!(other, Err(CuspError::Reach)),
    }
}

#[test]
fn angles_either_side_of_the_threshold() {
    let b = line(W, [W[0] + 1.0, W[1]]);
    let tilted = |rad: f64| {
        let (s, c) = (crate::num::trig::sin(rad), crate::num::trig::cos(rad));
        line(W, [W[0] + c, W[1] + s])
    };
    // At `2⁻⁴` rad the angle isn't certified above it: sharp.
    assert!(matches!(
        sharp(&tilted(1.0 / 16.0), &b, 1e-8),
        Ok(Sharp::Cusp(_))
    ));
    assert_eq!(sharp(&tilted(0.07), &b, 1e-8), Ok(Sharp::Apart));
    assert_eq!(sharp(&tilted(1.5), &b, 1e-8), Ok(Sharp::Apart));
    // Opposite tangents (a straight boundary through the vertex).
    assert_eq!(
        sharp(&line(W, [W[0] - 1.0, W[1]]), &b, 1e-8),
        Ok(Sharp::Apart)
    );
    // In space: the surface `(u, v) ↦ (u, 64v, 0)` stretches `v` so that
    // two lines `2⁻⁸` apart in UV are far apart.
    let e = Bounded::exact;
    let (du, dv) = ([e(1.0), e(0.0), e(0.0)], [e(0.0), e(64.0), e(0.0)]);
    let a = line(W, [W[0] + 1.0, W[1] + 1.0 / 256.0]);
    assert_eq!(cusp_rec(&a, &b, &du, &dv, 1e-8), Ok(Sharp::Apart));
    assert!(matches!(sharp(&a, &b, 1e-8), Ok(Sharp::Cusp(_))));
}

#[test]
fn one_curve_or_bad_input_is_refused() {
    let b = line(W, [W[0] + 1.0, W[1]]);
    assert_eq!(sharp(&b, &b, 1e-8), Err(CuspError::Contact));
    let other = line([W[0], W[1] + 0.5], [W[0] + 1.0, W[1]]);
    assert_eq!(sharp(&other, &b, 1e-8), Err(CuspError::Span));
    let dot = [
        [W[0], W[1], 1.0],
        [W[0], W[1], 1.0],
        [W[0] + 1.0, W[1], 1.0],
    ];
    assert_eq!(sharp(&dot, &b, 1e-8), Err(CuspError::Degenerate));
    let mut bad = b;
    bad[1][2] = 0.0;
    assert_eq!(sharp(&quarter(), &bad, 1e-8), Err(CuspError::Span));
    assert_eq!(sharp(&quarter(), &b, f64::NAN), Err(CuspError::Span));
    // A circle's bounds hold only so far: bands wider than the stretch
    // they hold over leave no cut.
    assert_eq!(sharp(&quarter(), &b, 4.0), Err(CuspError::Reach));
}

#[test]
fn the_same_bits_each_time() {
    let b = line(W, [W[0] + 1.0, W[1]]);
    let r = rec(&quarter(), &b, 2e-8);
    for _ in 0..3 {
        assert_eq!(rec(&quarter(), &b, 2e-8), r);
    }
}

/// A cusp of each contact order 1 to 4 (two lines `2⁻⁴` rad apart, a
/// line on a circle, equal-curvature parabolas, a circle and an ellipse
/// of its curvature at their common vertex): its record is the same under
/// the plane's eight symmetries about the vertex, either curve first and
/// the weights scaled by `1/64` to `64`, and under a reparametrisation of
/// the weights its order and coefficient stay and its reaches stay within
/// the cut.
#[test]
fn records_agree_under_symmetries_and_weights() {
    let c = [0.75, 0.75];
    let line_ = |b: [f64; 2]| line(c, b);
    let quarter_ = || -> CuspSpan {
        [
            [c[0], c[1], 1.0],
            [c[0] + 1.0, c[1], std::f64::consts::FRAC_1_SQRT_2],
            [c[0] + 1.0, c[1] + 1.0, 1.0],
        ]
    };
    let slope = crate::num::trig::tan(1.0 / 16.0);
    let pa = |cc: f64| -> CuspSpan {
        [
            [c[0], c[1], 1.0],
            [c[0] + 1.0, c[1], 1.0],
            [c[0] + 2.0 + cc, c[1] + 2.0, 1.0],
        ]
    };
    let orders = [2, 1, 3, 4];
    let cases: Vec<(CuspSpan, CuspSpan)> = vec![
        (quarter_(), line_([c[0] + 1.0, c[1]])),
        (line_([c[0] + 1.0, c[1] + slope]), line_([c[0] + 1.0, c[1]])),
        (pa(0.0), pa(0.5)),
        // `(2t, 2t²)/(1 + t²)`: the unit circle from its lowest point, and
        // the ellipse of half-axes 2 and 4 (the same curvature there).
        (
            [
                [c[0], c[1], 1.0],
                [c[0] + 1.0, c[1], 1.0],
                [c[0] + 1.0, c[1] + 1.0, 2.0],
            ],
            [
                [c[0], c[1], 1.0],
                [c[0] + 2.0, c[1], 1.0],
                [c[0] + 2.0, c[1] + 4.0, 2.0],
            ],
        ),
    ];
    for ((a, b), m) in cases.into_iter().zip(orders) {
        let base = rec(&a, &b, 1e-8);
        assert_eq!(base.m, m);
        for sym in 0..8 {
            let tr = |p: [f64; 3]| {
                let (mut x, mut y) = (p[0] - c[0], p[1] - c[1]);
                if sym & 1 != 0 {
                    x = -x;
                }
                if sym & 2 != 0 {
                    y = -y;
                }
                if sym & 4 != 0 {
                    std::mem::swap(&mut x, &mut y);
                }
                [x + c[0], y + c[1], p[2]]
            };
            for wscale in [1.0 / 64.0, 1.0, 64.0] {
                let t = |s: &CuspSpan| {
                    s.map(|p| {
                        let mut q = tr(p);
                        q[2] *= wscale;
                        q
                    })
                };
                let (ta, tb) = (t(&a), t(&b));
                for swap in [false, true] {
                    let r = if swap {
                        sharp(&tb, &ta, 1e-8)
                    } else {
                        sharp(&ta, &tb, 1e-8)
                    };
                    let r = match r {
                        Ok(Sharp::Cusp(r)) => r,
                        o => panic!("sym {sym} w {wscale} swap {swap}: {o:?}"),
                    };
                    assert_eq!(
                        (r.m, &r.a_lo, &r.s_c, &r.s_beta),
                        (base.m, &base.a_lo, &base.s_c, &base.s_beta),
                        "sym {sym} w {wscale} swap {swap}"
                    );
                }
            }
        }
        // Möbius reparametrisation of the weights.
        for rr in [1.0 / 64.0, 1.0 / 8.0, 8.0, 64.0] {
            let rp = |s: &CuspSpan| {
                let mut s = *s;
                s[1][2] *= rr;
                s[2][2] *= rr * rr;
                s
            };
            match sharp(&rp(&a), &rp(&b), 1e-8) {
                Ok(Sharp::Cusp(r)) => {
                    assert_eq!((r.m, &r.a_lo), (base.m, &base.a_lo), "r {rr}");
                    check_c(&rp(&a), &rp(&b), &r, c);
                }
                o => panic!("reparametrised by {rr}: {o:?}"),
            }
        }
    }
}

/// Each reach's point within the cut and not far short of it.
fn check_c(a: &CuspSpan, b: &CuspSpan, r: &CuspRec, c: [f64; 2]) {
    let s_beta = r.s_beta_f64();
    for (sp, reach) in [a, b].into_iter().zip(r.reach) {
        let t = reach.bounds()[0];
        let u = at(sp, t)[0] - c[0];
        assert!(
            u <= s_beta * (1.0 + 1e-9) && u >= 0.25 * s_beta,
            "reach {t} u {u} sb {s_beta}"
        );
    }
}

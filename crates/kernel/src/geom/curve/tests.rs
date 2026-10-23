//! Canonical points and splits: a split's new knots carry the canonical
//! point of the chain it was cut from, whichever other knots go in with
//! it; the spans it doesn't cut keep their bits; the split chain lies on
//! the original to within its rounding; slices take whole spans. The
//! range split: a span whose middle control point would leave the
//! `MAX_COORD` box is split, the result the chain's split at the knots
//! added; curves leaving or touching the box are refused; conic chains
//! are their cells or arcs, bounded the same way.

use malachite::Rational;
use varde_testkit::rng::Rng;

use super::*;

/// A chain of two weighted spans through `(0, 0, 0)`, `(4, 0, 0)` and
/// `(4, 4, 1)`, a quarter-like bend and a near-straight run.
fn bent() -> Curve3D {
    Curve3D::new(
        vec![
            [
                [0.0, 0.0, 0.0, 1.0],
                [2.0, 0.1, 0.0, 0.75],
                [4.0, 0.0, 0.0, 1.0],
            ],
            [
                [4.0, 0.0, 0.0, 2.0],
                [4.3, 2.0, 0.5, 1.0],
                [4.0, 4.0, 1.0, 0.5],
            ],
        ],
        vec![0.0, 1.5, 3.0],
    )
    .expect("a chain")
}

/// The exact point of `c` at `t`, homogeneous on the stored values.
fn exact(c: &Curve3D, t: f64) -> [Rational; 3] {
    let k = c.knots().partition_point(|&x| x < t).max(1) - 1;
    let (a, b) = (c.knots()[k], c.knots()[k + 1]);
    let s = local_exact(t, a, b);
    let h = c.spans()[k].each_ref().map(hom_exact);
    let p = blossom(&h, &s, &s);
    [0, 1, 2].map(|i| &p[i] / &p[3])
}

#[test]
fn a_split_knot_carries_the_canonical_point_whatever_else_is_split() {
    let c = bent();
    let mut rng = Rng::new(3);
    for _ in 0..varde_testing::pick(200, 2_000) {
        let t = rng.range(0.0, 3.0);
        let p = c.point_at(t).expect("in range");
        // The canonical point is the exact one, rounded.
        let x = exact(&c, t);
        for k in 0..3 {
            assert_eq!(p[k].to_bits(), nearest(&x[k]).to_bits(), "{t}");
        }
        // Split alone, or with other knots on either side.
        let others = [rng.range(0.0, 3.0), rng.range(0.0, 3.0)];
        for ts in [vec![t], vec![others[0], t, others[1]], vec![others[1], t]] {
            let s = c.split(&ts).expect("a valid chain");
            if c.knots().contains(&t) {
                continue;
            }
            let k = s.knots().iter().position(|&x| x == t).expect("a new knot");
            assert_eq!(s.knot_point(k), Some(p), "{t} in {ts:?}");
            assert_eq!(s.point_at(t), Some(p));
        }
    }
}

#[test]
fn spans_not_cut_keep_their_bits_and_cut_ones_stay_on_the_curve() {
    let c = bent();
    let s = c.split(&[2.0, 2.5, 7.0, f64::NAN, 1.5]).expect("a chain");
    // The first span, uncut, is the same bits; the knot already there and
    // the ones outside the range add nothing.
    assert_eq!(s.knots(), [0.0, 1.5, 2.0, 2.5, 3.0]);
    assert_eq!(s.spans()[0], c.spans()[0]);
    assert_eq!(s.knot_point(1), c.knot_point(1));
    assert_eq!(s.knot_point(4), c.knot_point(2));
    // The original's last weights stay at its ends.
    assert_eq!(s.spans()[1][0], c.spans()[1][0]);
    assert_eq!(s.spans()[3][2], c.spans()[1][2]);
    // Along the cut span, the split chain is the original to within a
    // few ulps of its coordinates.
    for i in 0..=64 {
        let t = 1.5 + 1.5 * f64::from(i) / 64.0;
        let a = c.eval(t).expect("in range");
        let b = s.eval(t).expect("in range");
        for k in 0..3 {
            let d = (a[k].v - b[k].v).abs();
            assert!(d <= 1e-14, "{t}: {d}");
        }
    }
    // Nothing to insert: the same chain.
    assert_eq!(c.split(&[]).expect("a chain"), c);
}

#[test]
fn slices_take_the_spans_between_two_knots() {
    let c = bent().split(&[2.0]).expect("a chain");
    let mid = c.slice(1.5, 3.0).expect("knots");
    assert_eq!(mid.knots(), [1.5, 2.0, 3.0]);
    assert_eq!(mid.spans(), &c.spans()[1..]);
    assert_eq!(c.slice(0.0, 3.0), Some(c.clone()));
    assert_eq!(c.slice(1.5, 1.5), None);
    assert_eq!(c.slice(3.0, 1.5), None);
    assert_eq!(c.slice(0.5, 3.0), None);
    // Outside the range there is no point.
    assert_eq!(c.point_at(3.5), None);
    assert_eq!(c.point_at(f64::NAN), None);
}

/// Whether every control point of `c` lies in the `MAX_COORD` box.
fn all_in_box(c: &Curve3D) -> bool {
    c.spans().iter().flatten().all(in_box)
}

/// The knots of `b` that `a` doesn't have.
fn added(a: &Curve3D, b: &Curve3D) -> Vec<f64> {
    b.knots()
        .iter()
        .copied()
        .filter(|t| !a.knots().contains(t))
        .collect()
}

/// A chain whose first span bulges far out of the box at its least
/// middle weight, its curve inside, and whose second span is inside.
fn bulging() -> (Vec<Span3>, Vec<f64>) {
    let (p0, p1, p2) = ([0.0, 0.0, 0.0], [2e5, 0.0, 0.0], [2e5, 1e5, 0.0]);
    (
        vec![
            [
                [p0[0], p0[1], p0[2], 1.0],
                [1e5, 3e7, 5e5, MIN_WEIGHT],
                [p1[0], p1[1], p1[2], 1.0],
            ],
            [
                [p1[0], p1[1], p1[2], 2.0],
                [3e5, 5e4, 1e3, 1.0],
                [p2[0], p2[1], p2[2], 1.0],
            ],
        ],
        vec![0.0, 1.0, 2.0],
    )
}

#[test]
fn a_middle_point_outside_the_box_is_split_as_the_chain_splits() {
    let (spans, knots) = bulging();
    let c = Curve3D::new(spans.clone(), knots.clone()).expect("a chain");
    assert!(!all_in_box(&c));
    let b = Curve3D::bounded(spans, knots).expect("a bounded chain");
    assert!(all_in_box(&b));
    // The first span split, at midpoints, the second kept.
    let new = added(&c, &b);
    assert!(!new.is_empty() && new.iter().all(|&t| 0.0 < t && t < 1.0));
    assert_eq!(b.spans().last(), c.spans().last());
    // The same bits as the chain split at the added knots.
    assert_eq!(c.split(&new).expect("a chain"), b);
    // Bounded already: the same chain.
    let again = Curve3D::bounded(b.spans().to_vec(), b.knots().to_vec());
    assert_eq!(again.as_ref(), Ok(&b));
    // A split of a bounded chain stays bounded.
    let mut rng = Rng::new(5);
    for _ in 0..varde_testing::pick(20, 200) {
        let ts = [rng.range(0.0, 2.0), rng.range(0.0, 2.0)];
        assert!(all_in_box(&b.split(&ts).expect("a chain")));
    }
}

#[test]
fn random_bulging_spans_split_into_the_box_or_are_refused() {
    let mut rng = Rng::new(11);
    let mut split = 0;
    for _ in 0..varde_testing::pick(40, 400) {
        let end = |rng: &mut Rng| -> [f64; 4] {
            let p = rng.point(9e5);
            [p.x, p.y, p.z, rng.log_range(MIN_WEIGHT, MAX_WEIGHT)]
        };
        let (p0, p2) = (end(&mut rng), end(&mut rng));
        let m = rng.point(4e7);
        let p1 = [m.x, m.y, m.z, rng.log_range(MIN_WEIGHT, 1.0)];
        let span = [p0, p1, p2];
        let c = Curve3D::new(vec![span], vec![0.0, 1.0]).expect("a span");
        match Curve3D::bounded(vec![span], vec![0.0, 1.0]) {
            Ok(b) => {
                assert!(all_in_box(&b));
                assert_eq!(c.split(&added(&c, &b)).expect("a chain"), b);
                split += usize::from(b.spans().len() > 1);
            }
            // A knot point outside, or the depth cap.
            Err(e) => assert_eq!(e, CurveError::Range { span: 0 }),
        }
    }
    assert!(split > 0);
}

#[test]
fn a_curve_leaving_the_box_is_refused() {
    let line = |a: [f64; 3], b: [f64; 3]| -> Span3 {
        [
            [a[0], a[1], a[2], 1.0],
            [
                (a[0] + b[0]) / 2.0,
                (a[1] + b[1]) / 2.0,
                (a[2] + b[2]) / 2.0,
                1.0,
            ],
            [b[0], b[1], b[2], 1.0],
        ]
    };
    let (o, x, far) = ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.5e6]);
    // An end outside.
    assert_eq!(
        Curve3D::bounded(vec![line(o, x), line(x, far)], vec![0.0, 1.0, 2.0]),
        Err(CurveError::Range { span: 1 })
    );
    // The curve itself through `y ≈ 1.97e6`: its first halving's knot
    // point is outside.
    let heavy = [
        [0.0, 0.0, 0.0, 1.0],
        [0.5, 2e6, 0.0, MAX_WEIGHT],
        [1.0, 0.0, 0.0, 1.0],
    ];
    assert_eq!(
        Curve3D::bounded(vec![heavy], vec![0.0, 1.0]),
        Err(CurveError::Range { span: 0 })
    );
    // The chain's own refusals come first.
    assert_eq!(
        Curve3D::bounded(vec![line(o, x)], vec![1.0, 0.0]),
        Err(CurveError::Knots { knot: 1 })
    );
}

#[test]
fn a_curve_touching_the_box_from_inside_ends_at_the_depth_cap() {
    // `y = x²/4f − 10⁶`: tangent to the box's face `y = −10⁶` at `t = 0`,
    // where every span around it has its middle point outside.
    let p = Conic::parabola(
        [0.0, -MAX_COORD, 0.0],
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        1e3,
    )
    .expect("a parabola");
    // A knot at the vertex settles it: one split.
    let c = Curve3D::conic_arcs(&p, &[-1.0, 1.0]).expect("bounded");
    assert_eq!(c.knots(), [-1.0, 0.0, 1.0]);
    assert_eq!(c.knot_point(1), Some([0.0, -MAX_COORD, 0.0]));
    assert!(all_in_box(&c));
    // Midpoints of `[−1, 2]` never reach 0.
    assert_eq!(
        Curve3D::conic_arcs(&p, &[-1.0, 2.0]),
        Err(CurveError::Range { span: 0 })
    );
}

#[test]
fn ellipse_chains_are_the_cells_at_their_level() {
    let axes = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let e = Conic::circle([1.0, 2.0, 3.0], axes, 5.0).expect("a circle");
    let c = Curve3D::ellipse(&e, 0, 0, 4).expect("a circle");
    assert_eq!(c.knots(), [0.0, 1.0, 2.0, 3.0, 4.0]);
    for k in 0..4 {
        assert_eq!(c.spans()[k], e.quarter::<4>(k as i64).expect("a cell"));
    }
    // Closed: the last point is the first, bit for bit.
    assert_eq!(c.knot_point(4), c.knot_point(0));
    // Level 2, across a full turn's start.
    let c = Curve3D::ellipse(&e, 2, -3, 5).expect("an arc");
    for (k, i) in (-3..5).enumerate() {
        assert_eq!(c.knots()[k], i as f64 / 4.0);
        assert_eq!(c.spans()[k], e.quarter_at::<4>(2, i).expect("a span"));
    }
    // Refusals.
    for (level, from, to) in [(0, 1, 1), (0, 2, 1), (0, 0, 5), (1, -4, 5), (0, 0, 1 << 53)] {
        assert_eq!(
            Curve3D::ellipse(&e, level, from, to),
            Err(CurveError::Shape),
            "{level} {from} {to}"
        );
    }
    assert_eq!(
        Curve3D::ellipse(&e, MAX_LEVEL + 1, 0, 1),
        Err(CurveError::Conic(ConicError::Level))
    );
    let p = Conic::parabola([0.0; 3], axes, 1.0).expect("a parabola");
    assert_eq!(
        Curve3D::ellipse(&p, 0, 0, 1),
        Err(CurveError::Conic(ConicError::Kind))
    );
}

#[test]
fn ellipse_cells_near_the_box_split_into_finer_cells() {
    // Radius `ρ√2 ≈ 9.2e5`, turned 45°: cell corners at `2ρ = 1.3e6`.
    let rho = 6.5e5;
    let axes = [[1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]];
    let e = Conic::ellipse([0.0; 3], axes, [rho, rho]).expect("a circle");
    let c = Curve3D::ellipse(&e, 0, 0, 4).expect("bounded");
    assert!(c.spans().len() > 4 && all_in_box(&c));
    assert_eq!(c.knot_point(c.spans().len()), c.knot_point(0));
    // Every span is a cell's span at a finer level, as the lattice's.
    for (k, span) in c.spans().iter().enumerate() {
        let (t0, t1) = (c.knots()[k], c.knots()[k + 1]);
        // The width `2^−L`, and `t0 = i·2^−L`.
        let n = (1.0 / (t1 - t0)) as u64;
        assert!(n.is_power_of_two() && t1 - t0 == 1.0 / n as f64);
        let i = t0 * n as f64;
        assert_eq!(i.fract(), 0.0);
        let level = n.trailing_zeros();
        assert_eq!(*span, e.quarter_at::<4>(level, i as i64).expect("a span"));
    }
}

#[test]
fn conic_arcs_join_bitwise() {
    let axes = [[1.0, 0.0, 0.0], [0.0, 0.6, 0.8]];
    let h = Conic::hyperbola([1.0, -2.0, 0.5], axes, [2.0, 1.0]).expect("a hyperbola");
    let ss = [0.5, 1.0, 2.0, 4.0];
    let c = Curve3D::conic_arcs(&h, &ss).expect("arcs");
    assert_eq!(c.knots(), ss);
    for k in 0..3 {
        assert_eq!(c.spans()[k], h.arc::<4>(ss[k], ss[k + 1]).expect("an arc"));
    }
    let p = Conic::parabola([0.0; 3], axes, 0.25).expect("a parabola");
    let c = Curve3D::conic_arcs(&p, &[-3.0, -1.0, 0.0, 2.5]).expect("arcs");
    assert_eq!(c.knot_point(2), Some([0.0; 3]));
    // Refusals.
    assert_eq!(Curve3D::conic_arcs(&p, &[1.0]), Err(CurveError::Shape));
    assert_eq!(
        Curve3D::conic_arcs(&p, &[0.0, 1.0, 1.0]),
        Err(CurveError::Conic(ConicError::Parameter))
    );
    assert_eq!(
        Curve3D::conic_arcs(&h, &[-1.0, 1.0]),
        Err(CurveError::Conic(ConicError::Parameter))
    );
}

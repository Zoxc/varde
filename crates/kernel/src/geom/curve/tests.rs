//! Canonical points and splits: a split's new knots carry the canonical
//! point of the chain it was cut from, whichever other knots go in with
//! it; the spans it doesn't cut keep their bits; the split chain lies on
//! the original to within its rounding; slices take whole spans.

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
fn p_curves_split_the_same_way() {
    let p = PCurve::new(
        vec![[[0.25, 0.25, 1.0], [0.5, 0.375, 1.5], [0.75, 0.25, 1.0]]],
        vec![0.0, 1.0],
    )
    .expect("a p-curve");
    let t = 0.3;
    let at = p.point_at(t).expect("in range");
    let s = p.split(&[t]).expect("a p-curve");
    assert_eq!(s.knots(), [0.0, t, 1.0]);
    assert_eq!(s.knot_point(1), Some(at));
    assert_eq!(s.knot_point(0), p.knot_point(0));
    assert_eq!(s.knot_point(2), p.knot_point(1));
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

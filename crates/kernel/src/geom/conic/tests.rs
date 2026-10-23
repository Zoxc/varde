//! Conic spans: exact to rounding against their conics' implicits (and
//! planes) at every level; an ellipse's cells and their ½ splits share
//! their ends bit for bit and are a cylinder lattice's rows; splits keep
//! the parametrisation; the refusals.

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Abs;
use varde_decide::num::bounded::Bounded;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::curve::Curve3D;
use crate::geom::form::CylinderKey;
use crate::geom::lattice::{Lattice, Level};

/// The exact point of the stored `span` at the local parameter `t`.
fn exact_point<const N: usize, const D: usize>(span: &Span<D>, t: &Rational) -> [Rational; N] {
    let h = blossom(&span.each_ref().map(hom_exact), t, t);
    std::array::from_fn(|i| &h[i] / &h[D - 1])
}

/// `|F(p)| / |∇F(p)|` of `c`'s implicit `F`: the distance from `p` to
/// the conic (in space: to the cylinder over it), to first order.
fn distance<const N: usize>(c: &Conic<N>, p: &[Rational; N]) -> f64 {
    let f = c.implicit(p);
    let (centre, [a, b]) = c.exact();
    let d: [Rational; N] = std::array::from_fn(|i| &p[i] - &centre[i]);
    let (aa, ab, bb) = (dot(&a, &a), dot(&a, &b), dot(&b, &b));
    let g = &aa * &bb - &ab * &ab;
    let alpha = &bb * dot(&a, &d) - &ab * dot(&b, &d);
    let beta = &aa * dot(&b, &d) - &ab * dot(&a, &d);
    let grad: [Rational; N] = std::array::from_fn(|i| {
        let da = &bb * &a[i] - &ab * &b[i];
        let db = &aa * &b[i] - &ab * &a[i];
        let two = Rational::from(2u32);
        match c.kind() {
            ConicKind::Ellipse => &two * &alpha * da + &two * &beta * db,
            ConicKind::Parabola => &two * &alpha * da - &g * db,
            ConicKind::Hyperbola => &two * &alpha * da - &two * &beta * db,
        }
    });
    let norm = grad
        .iter()
        .map(|x| {
            let v = nearest(x);
            v * v
        })
        .sum::<f64>()
        .sqrt();
    nearest(&f.abs()) / norm
}

/// The largest coordinate magnitude of `span`'s control points.
fn scale<const D: usize>(span: &Span<D>) -> f64 {
    span.iter()
        .flat_map(|p| p[..D - 1].iter())
        .fold(0.0, |m: f64, x| if x.abs() > m { x.abs() } else { m })
}

/// The local parameters the checks sample, ends included.
fn samples() -> Vec<Rational> {
    [(0, 1), (1, 7), (1, 3), (1, 2), (5, 8), (6, 7), (1, 1)]
        .map(|(a, b)| Rational::from_signeds(a, b))
        .to_vec()
}

/// Every sample of the stored `span` lies within `4ε` of its control
/// points' scale from `c`.
fn on_conic<const N: usize, const D: usize>(c: &Conic<N>, span: &Span<D>) {
    let tol = 4.0 * f64::EPSILON * scale(span);
    for t in samples() {
        let p = exact_point::<N, D>(span, &t);
        let d = distance(c, &p);
        assert!(d <= tol, "{c:?} {span:?} at {t}: {d:e} > {tol:e}");
    }
}

/// As [`on_conic`], and in the conic's plane.
fn on_conic3(c: &Conic<3>, span: &Span<4>) {
    on_conic(c, span);
    let [e1, e2] = c.axes().map(|e| e.map(q));
    let n = [
        &e1[1] * &e2[2] - &e1[2] * &e2[1],
        &e1[2] * &e2[0] - &e1[0] * &e2[2],
        &e1[0] * &e2[1] - &e1[1] * &e2[0],
    ];
    let norm = n
        .iter()
        .map(|x| {
            let v = nearest(x);
            v * v
        })
        .sum::<f64>()
        .sqrt();
    let tol = 4.0 * f64::EPSILON * scale(span);
    for t in samples() {
        let p = exact_point::<3, 4>(span, &t);
        let off = nearest(&c.plane(&p).abs()) / norm;
        assert!(off <= tol, "{c:?} at {t}: off the plane by {off:e}");
    }
}

fn vec3(rng: &mut Rng, extent: f64) -> [f64; 3] {
    rng.point(extent).to_array()
}

/// A random conic of `kind` in space: centre within 1e3, scales over six
/// decades, axes of any lengths at any angle (not near parallel).
fn conic3(rng: &mut Rng, kind: ConicKind) -> Conic<3> {
    loop {
        let centre = vec3(rng, 1e3);
        let e1 = rng.direction().to_array().map(|x| x * rng.range(0.5, 2.0));
        let e2 = rng.direction().to_array();
        let s = [rng.log_range(1e-3, 1e3), rng.log_range(1e-3, 1e3)];
        let cos = (e1[0] * e2[0] + e1[1] * e2[1] + e1[2] * e2[2]).abs()
            / (e1.iter().map(|x| x * x).sum::<f64>().sqrt());
        if cos < 0.9 {
            return Conic::new(kind, centre, [e1, e2], s).expect("a conic");
        }
    }
}

#[test]
fn ellipse_cells_and_their_splits_lie_on_the_ellipse() {
    let mut rng = Rng::new(11);
    for _ in 0..varde_testing::pick(20, 300) {
        let c = conic3(&mut rng, ConicKind::Ellipse);
        for k in -1..4 {
            on_conic3(&c, &c.quarter(k).expect("a cell"));
        }
        for level in [1, 3, 9, 30] {
            let i = (rng.next_u64() >> 20) as i64 - (1 << 43);
            on_conic3(&c, &c.quarter_at(level, i).expect("a split"));
        }
        // In the plane, on a circle.
        let o = [rng.range(-1e3, 1e3), rng.range(-1e3, 1e3)];
        let r = rng.log_range(1e-3, 1e3);
        let circle = Conic::circle(o, [[1.0, 0.0], [0.0, 1.0]], r).expect("a circle");
        for k in 0..4 {
            on_conic(&circle, &circle.quarter::<3>(k).expect("a cell"));
            on_conic(
                &circle,
                &circle.quarter_at::<3>(2, 4 * k + 1).expect("a split"),
            );
        }
    }
}

#[test]
fn parabola_and_hyperbola_arcs_lie_on_their_conics() {
    let mut rng = Rng::new(12);
    for _ in 0..varde_testing::pick(20, 300) {
        let p = conic3(&mut rng, ConicKind::Parabola);
        let t0 = rng.range(-4.0, 4.0);
        let t1 = t0 + rng.log_range(1e-3, 8.0);
        let span = p.arc(t0, t1).expect("a parabola arc");
        assert!(span.iter().all(|x| x[3] == 1.0), "{span:?}");
        on_conic3(&p, &span);
        on_conic3(&p, &sub_span(&span, 5, 17).expect("a split"));

        let h = conic3(&mut rng, ConicKind::Hyperbola);
        let s0 = rng.log_range(0.05, 20.0);
        let s1 = s0 * rng.log_range(1.0001, 100.0);
        let span = h.arc(s0, s1).expect("a hyperbola arc");
        assert!((1.0..=2.0).contains(&span[1][3]), "{span:?}");
        on_conic3(&h, &span);
        on_conic3(&h, &sub_span(&span, 4, 9).expect("a split"));
    }
    // A unit hyperbola in the plane through its vertex: s = 1 at (1, 0).
    let h =
        Conic::hyperbola([0.0, 0.0], [[1.0, 0.0], [0.0, 1.0]], [1.0, 1.0]).expect("a hyperbola");
    let span = h.arc::<3>(0.5, 1.0).expect("an arc");
    // Weights ½, ¾, 1 scaled by 2 (the middle into [1, 2)).
    assert_eq!(span[0], [1.25, -0.75, 1.0]);
    assert_eq!(span[1][2], 1.5);
    assert_eq!(span[2], [1.0, 0.0, 2.0], "the vertex at s = 1");
    on_conic(&h, &span);
}

#[test]
fn cells_have_root_half_weights_shared_ends_and_period_four() {
    let c =
        Conic::circle([1.0, 2.0, 3.0], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], 2.0).expect("a circle");
    let q0: Span<4> = c.quarter(0).expect("a cell");
    assert_eq!(
        q0,
        [
            [3.0, 2.0, 3.0, 1.0],
            [3.0, 4.0, 3.0, FRAC_1_SQRT_2],
            [1.0, 4.0, 3.0, 1.0]
        ]
    );
    for k in -6..6 {
        let a: Span<4> = c.quarter(k).expect("a cell");
        let b: Span<4> = c.quarter(k + 1).expect("a cell");
        assert_eq!(a, c.quarter(k + 4).expect("a cell"), "{k}");
        assert_eq!(a[2].map(f64::to_bits), b[0].map(f64::to_bits), "{k}");
        assert_eq!(a.map(|p| p[3]), [1.0, FRAC_1_SQRT_2, 1.0]);
    }
}

#[test]
fn circle_cells_are_the_cylinder_lattice_rows() {
    let mut rng = Rng::new(13);
    for _ in 0..varde_testing::pick(8, 100) {
        let key = CylinderKey::new(
            vec3(&mut rng, 1e3),
            rng.direction().to_array(),
            rng.log_range(1e-2, 1e3),
        )
        .expect("a cylinder");
        let lattice = Lattice::new(key.into()).expect("a lattice");
        let [e1, e2, _] = lattice.axes();
        let circle = Conic::circle(key.foot(), [e1, e2], key.radius()).expect("a circle");
        // The tile's middle row (axial node 0, the foot point's plane).
        let row = |t: &crate::geom::tile::Tile| [1, 4, 7].map(|n| t.net()[n]);
        for k in -2..3 {
            let tile = lattice.base_tile(k, 0).expect("a tile");
            assert_eq!(row(&tile), circle.quarter(k).expect("a cell"), "{k}");
        }
        for level in [1, 2, 5] {
            for i in [-3, 0, 1, (3 << level) + 1] {
                let tile = lattice
                    .tile(Level { u: level, v: 0 }, i, 0)
                    .expect("a tile");
                assert_eq!(
                    row(&tile),
                    circle.quarter_at(level, i).expect("a split"),
                    "{level} {i}"
                );
            }
        }
    }
}

#[test]
fn level_spans_share_their_ends_and_match_chain_splits() {
    let mut rng = Rng::new(14);
    for _ in 0..varde_testing::pick(8, 100) {
        let c = conic3(&mut rng, ConicKind::Ellipse);
        for level in [1u32, 2, 4] {
            let n = 1i64 << level;
            // Across a cell boundary and around the turn.
            let spans: Vec<Span<4>> = (-n - 1..=n + 1)
                .map(|i| c.quarter_at(level, i).expect("a split"))
                .collect();
            for w in spans.windows(2) {
                assert_eq!(w[0][2].map(f64::to_bits), w[1][0].map(f64::to_bits));
            }
            // The cell's chain split at the level's knots: the same bits.
            let cell = c.quarter(1).expect("a cell");
            let chain = Curve3D::new(vec![cell], vec![0.0, 1.0]).expect("a chain");
            let knots: Vec<f64> = (1..n).map(|j| j as f64 / n as f64).collect();
            let split = chain.split(&knots).expect("a split chain");
            for j in 0..n {
                assert_eq!(
                    split.spans()[j as usize],
                    c.quarter_at(level, n + j).expect("a split"),
                    "{level} {j}"
                );
            }
        }
        // Level 0 is the cell.
        assert_eq!(
            c.quarter_at::<4>(0, 2).expect("a cell"),
            c.quarter(2).expect("a cell")
        );
    }
}

#[test]
fn splits_keep_the_parametrisation() {
    // A sub-span's end weights are the denominator there, not 1.
    let c = Conic::circle([0.0, 0.0], [[1.0, 0.0], [0.0, 1.0]], 1.0).expect("a circle");
    let [lo, hi] = [0, 1].map(|j| c.quarter_at::<3>(1, j).expect("a half"));
    assert_eq!(lo[0][2], 1.0);
    assert_eq!(lo[2][2].to_bits(), hi[0][2].to_bits());
    let w_mid = nearest(&((Rational::ONE + q(FRAC_1_SQRT_2)) / Rational::from(2u32)));
    assert_eq!(lo[2][2], w_mid, "W(½) = ½ + √½/2");
    // A parabola's half is the arc over the half interval, bit for bit.
    let p = Conic::parabola([0.5, -1.0, 2.0], [[1.0, 0.25, 0.0], [0.0, 1.0, 3.0]], 0.75)
        .expect("a parabola");
    let arc: Span<4> = p.arc(-1.5, 2.5).expect("an arc");
    assert_eq!(
        sub_span(&arc, 1, 0).expect("a half"),
        p.arc(-1.5, 0.5).expect("an arc")
    );
    assert_eq!(
        sub_span(&arc, 2, 3).expect("a quarter"),
        p.arc(1.5, 2.5).expect("an arc")
    );
    // A hyperbola's: the same points but for rounding, its weights a
    // power of two apart.
    let h = Conic::hyperbola(
        [1.0, 2.0, -3.0],
        [[1.0, 0.0, 0.5], [0.0, 2.0, 0.0]],
        [3.0, 0.5],
    )
    .expect("a hyperbola");
    let arc: Span<4> = h.arc(0.25, 4.0).expect("an arc");
    for (sub, part) in [
        (
            sub_span(&arc, 1, 1).expect("a half"),
            h.arc::<4>(2.125, 4.0).expect("an arc"),
        ),
        (
            sub_span(&arc, 3, 2).expect("an eighth"),
            h.arc::<4>(1.1875, 1.65625).expect("an arc"),
        ),
    ] {
        let ratio = sub[0][3] / part[0][3];
        // A power of two: a zero significand.
        assert_eq!(ratio.to_bits() & ((1 << 52) - 1), 0, "{ratio}");
        // The points to within the rounding of the stored arc they are
        // split from.
        let tol = 4.0 * f64::EPSILON * scale(&arc);
        for (a, b) in sub.iter().zip(&part) {
            assert!((0..3).all(|i| (a[i] - b[i]).abs() <= tol), "{a:?} {b:?}");
            assert_eq!(a[3] / b[3], ratio);
        }
    }
}

#[test]
fn implicits_vanish_on_the_conic_and_tell_the_sides() {
    let axes = [[1.0, 0.0], [0.0, 1.0]];
    let e = Conic::ellipse([1.0, 1.0], axes, [3.0, 2.0]).expect("an ellipse");
    let p = Conic::parabola([0.0, 0.0], axes, 0.25).expect("a parabola");
    let h = Conic::hyperbola([0.0, 0.0], axes, [1.0, 1.0]).expect("a hyperbola");
    let exact = |c: &Conic<2>, x: [f64; 2]| c.implicit(&x.map(q));
    let zero = Rational::from(0u32);
    // On: (4, 1), (1, 3); y = x² at (2, 4); x² − y² = 1 at (1, 0).
    assert_eq!(exact(&e, [4.0, 1.0]), zero);
    assert_eq!(exact(&e, [1.0, 3.0]), zero);
    assert_eq!(exact(&p, [2.0, 4.0]), zero);
    assert_eq!(exact(&h, [1.0, 0.0]), zero);
    // Inside, the focus side, between the branches: negative.
    assert!(exact(&e, [1.0, 1.0]) < zero);
    assert!(exact(&p, [0.0, 1.0]) < zero);
    assert!(exact(&h, [0.0, 0.0]) < zero);
    assert!(exact(&h, [3.0, 0.0]) > zero);
    // At the first level: zero enclosed on the conic, signs certain off it.
    let first = |c: &Conic<2>, x: [f64; 2]| c.implicit(&x.map(Bounded::exact));
    let [lo, hi] = first(&e, [4.0, 1.0]).enclose();
    assert!(lo <= 0.0 && 0.0 <= hi);
    assert_eq!(first(&e, [1.0, 1.5]).sign(), Some(std::cmp::Ordering::Less));
    assert_eq!(
        first(&p, [2.0, 3.0]).sign(),
        Some(std::cmp::Ordering::Greater)
    );
    // In space: the plane through the conic.
    let c =
        Conic::circle([0.0, 0.0, 5.0], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], 2.0).expect("a circle");
    assert_eq!(c.plane(&[7.0, -3.0, 5.0].map(q)), zero);
    assert!(c.plane(&[0.0, 0.0, 6.0].map(q)) > zero);
    assert_eq!(
        c.implicit(&[0.0, 2.0, 9.0].map(q)),
        zero,
        "the cylinder over it"
    );
    assert_eq!(c.implicit_degree(), 2);
}

#[test]
fn refusals() {
    let axes = [[1.0, 0.0], [0.0, 1.0]];
    let e = Conic::ellipse([0.0, 0.0], axes, [1.0, 1.0]).expect("an ellipse");
    let p = Conic::parabola([0.0, 0.0], axes, 1.0).expect("a parabola");
    let h = Conic::hyperbola([0.0, 0.0], axes, [1.0, 1.0]).expect("a hyperbola");
    assert_eq!(
        Conic::circle([f64::NAN, 0.0], axes, 1.0),
        Err(ConicError::NotFinite)
    );
    assert_eq!(
        Conic::circle([0.0, 0.0], axes, f64::INFINITY),
        Err(ConicError::NotFinite)
    );
    assert_eq!(Conic::circle([0.0, 0.0], axes, 0.0), Err(ConicError::Scale));
    assert_eq!(
        Conic::parabola([0.0, 0.0], axes, -1.0),
        Err(ConicError::Scale)
    );
    assert_eq!(
        Conic::ellipse([0.0, 0.0], [[1.0, 2.0], [-0.5, -1.0]], [1.0, 1.0]),
        Err(ConicError::Axes)
    );
    assert_eq!(
        Conic::ellipse([0.0; 3], [[0.0; 3], [0.0, 1.0, 0.0]], [1.0, 1.0]),
        Err(ConicError::Axes)
    );
    assert_eq!(p.quarter::<3>(0), Err(ConicError::Kind));
    assert_eq!(e.arc::<3>(0.0, 1.0), Err(ConicError::Kind));
    assert_eq!(p.arc::<3>(1.0, 1.0), Err(ConicError::Parameter));
    assert_eq!(p.arc::<3>(f64::NAN, 1.0), Err(ConicError::Parameter));
    assert_eq!(h.arc::<3>(0.0, 1.0), Err(ConicError::Parameter));
    assert_eq!(h.arc::<3>(1.0, 1000.0), Err(ConicError::Weight));
    // The corner `a + b` at `(0, 2e308)`.
    let big =
        Conic::ellipse([0.0, 0.0], [[1.0, 1.0], [-1.0, 1.0]], [1e308, 1e308]).expect("an ellipse");
    assert_eq!(big.quarter::<3>(0), Err(ConicError::Range));
    assert_eq!(e.quarter_at::<3>(MAX_LEVEL + 1, 0), Err(ConicError::Level));
    let cell = e.quarter::<3>(0).expect("a cell");
    assert_eq!(sub_span(&cell, 2, 4), Err(ConicError::Level));
    assert_eq!(sub_span(&cell, MAX_LEVEL + 1, 0), Err(ConicError::Level));
}

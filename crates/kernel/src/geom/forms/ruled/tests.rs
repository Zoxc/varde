//! Ruled surfaces against their rails: the first and last rows the
//! rails' spans to the bit, the middle row the exact homogeneous mean
//! rounded once; every point on the ruling between the rails' points at
//! its `u`; neighbours sharing rows; rails over other knots or with
//! uneven joints refused.

use malachite::Rational as Q;
use varde_decide::bezier::HomCurve;
use varde_testkit::rng::Rng;

use super::super::testing::{dot, exact, scale};
use super::*;
use crate::geom::conic::Conic;
use crate::geom::lattice::larger;

const TOL: f64 = 1e-15;

/// Rail pairs over shared knots: two ellipses' quarters in tilted planes
/// (weights `1, √½, 1`, even at the joints), and parabola arcs against
/// lines (weights 1).
fn rails(seed: u64) -> Vec<(Curve3D, Curve3D)> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for _ in 0..6 {
        let ellipse = |rng: &mut Rng| {
            let (e1, e2) = rng.direction().any_orthonormal_pair();
            let r = rng.log_range(1e-2, 1e3);
            let c = Conic::ellipse(
                rng.point(1e3).to_array(),
                [e1.to_array(), e2.to_array()],
                [r, r * rng.range(0.3, 1.0)],
            )
            .unwrap();
            Curve3D::ellipse(&c, 0, 0, 3).unwrap()
        };
        out.push((ellipse(&mut rng), ellipse(&mut rng)));
        let (e1, e2) = rng.direction().any_orthonormal_pair();
        let p = Conic::parabola(
            rng.point(1e3).to_array(),
            [e1.to_array(), e2.to_array()],
            rng.log_range(1e-2, 1e2),
        )
        .unwrap();
        let ts = [-1.0, 0.0, 0.5, 2.0];
        let arcs = Curve3D::conic_arcs(&p, &ts).unwrap();
        // A line through three points, each span's middle the mean.
        let a = rng.point(1e3).to_array();
        let b = rng.point(1e3).to_array();
        let at = |s: f64| std::array::from_fn::<f64, 3, _>(|c| a[c] + s * (b[c] - a[c]));
        let pts = [
            at(0.0),
            at(0.25),
            at(0.5),
            at(0.75),
            at(1.0),
            at(1.25),
            at(1.5),
        ];
        let spans = (0..3)
            .map(|k| {
                [0, 1, 2].map(|n| {
                    let p = pts[2 * k + n];
                    [p[0], p[1], p[2], 1.0]
                })
            })
            .collect();
        let line = Curve3D::new(spans, arcs.knots().to_vec()).unwrap();
        out.push((arcs, line));
    }
    out
}

/// A span's exact homogeneous curve.
fn hom(span: &Span3) -> HomCurve<Q, 3> {
    HomCurve::<Q, 3>::from_affine(span.map(|p| ([p[0], p[1], p[2]], p[3])), &Q::from(0u32)).unwrap()
}

#[test]
fn rows_are_the_rails_and_their_mean() {
    for (r0, r1) in rails(1) {
        let tiles = ruled(&r0, &r1).unwrap();
        assert_eq!(tiles.len(), r0.spans().len());
        for ((t, a), b) in tiles.iter().zip(r0.spans()).zip(r1.spans()) {
            for i in 0..3 {
                let net = t.net();
                assert_eq!(net[3 * i].map(f64::to_bits), a[i].map(f64::to_bits));
                assert_eq!(net[3 * i + 2].map(f64::to_bits), b[i].map(f64::to_bits));
                // `(w₀x₀ + w₁x₁, w₀ + w₁)/2`, projected.
                let (w0, w1) = (q(a[i][3]), q(b[i][3]));
                let hw = (&w0 + &w1) / Q::from(2u32);
                let want: [f64; 4] = std::array::from_fn(|c| {
                    if c == 3 {
                        nearest(&hw)
                    } else {
                        let h = (&w0 * q(a[i][c]) + &w1 * q(b[i][c])) / Q::from(2u32);
                        nearest(&(h / &hw))
                    }
                });
                assert_eq!(net[3 * i + 1].map(f64::to_bits), want.map(f64::to_bits));
            }
        }
        // Neighbours share their row across each joint.
        for p in tiles.windows(2) {
            for j in 0..3 {
                assert_eq!(
                    p[0].net()[6 + j].map(f64::to_bits),
                    p[1].net()[j].map(f64::to_bits)
                );
            }
        }
    }
}

/// A tile's point at `(u, v)` lies on the ruling through the rails'
/// points at `u`, between them, to the rounding of the middle row
/// (`TOL` of the tile's scale).
#[test]
fn points_lie_on_the_rulings() {
    let mut worst: f64 = 0.0;
    for (r0, r1) in rails(2) {
        let tiles = ruled(&r0, &r1).unwrap();
        for ((t, a), b) in tiles.iter().zip(r0.spans()).zip(r1.spans()) {
            let m = scale(t);
            let e = exact(t);
            for (u, v) in [(0, 1), (1, 3), (2, 2), (3, 1), (4, 3)] {
                let (u, v) = (Q::from_signeds(u, 4), Q::from_signeds(v, 4));
                let p = e.eval(&u, &v);
                let (c0, c1) = (hom(a).eval(&u), hom(b).eval(&u));
                let d: [Q; 3] = std::array::from_fn(|c| &c1[c] - &c0[c]);
                let x: [Q; 3] = std::array::from_fn(|c| &p[c] - &c0[c]);
                // The offset from the ruling, and where along it.
                let along = dot(&x, &d) / dot(&d, &d);
                let off: [Q; 3] = std::array::from_fn(|c| &x[c] - &along * &d[c]);
                let off = nearest(&dot(&off, &off)).sqrt() / m;
                assert!(off <= TOL, "{off:e}");
                assert!(along > 0u32 && along < 1u32);
                worst = larger(worst, off);
            }
        }
    }
    eprintln!("worst distance from the ruling over the tile's scale: {worst:e}");
}

#[test]
fn rails_must_share_knots_and_even_joints() {
    let (r0, r1) = rails(3).swap_remove(0);
    // Other knots: the same spans over a shifted parameter.
    let shifted = Curve3D::new(
        r1.spans().to_vec(),
        r1.knots().iter().map(|k| k + 1.0).collect(),
    )
    .unwrap();
    assert_eq!(ruled(&r0, &shifted).unwrap_err(), RuledError::Knots);
    // Span 1's weights doubled: its joints' weights differ.
    let mut spans = r1.spans().to_vec();
    spans[1] = spans[1].map(|p| [p[0], p[1], p[2], 2.0 * p[3]]);
    let uneven = Curve3D::new(spans, r1.knots().to_vec()).unwrap();
    assert_eq!(
        ruled(&r0, &uneven).unwrap_err(),
        RuledError::Joint { joint: 1 }
    );
}

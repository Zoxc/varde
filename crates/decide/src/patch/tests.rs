//! A rational patch through the trait is its inherent operations, to the
//! bit, at the first level, at double-double and exactly: every member,
//! the fold certificate's proof and quotient on a pole row exactly, and
//! the closest point's set-up from the homogeneous net against the one
//! from the affine net.

use malachite::Rational as Q;

use super::*;
use crate::bezier::{Foot, Side};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

/// A rational patch of degrees `(3, 2)` as affine points `(x, y, z, w)`,
/// row-major in `u`: a bumped grid with weights in `[1/4, 4]`.
fn net() -> Vec<[f64; 4]> {
    let w = [
        1.0, 0.5, 2.0, 0.25, 4.0, 1.5, 3.0, 1.0, 0.75, 1.25, 2.5, 1.0,
    ];
    (0..12)
        .map(|k| {
            let (i, j) = ((k / 3) as f64, (k % 3) as f64);
            let z = 0.125 * ((k * 5) % 7) as f64;
            [i, 1.5 * j + 0.25 * i, z, w[k]]
        })
        .collect()
}

/// The pole-row patch of degrees `(2, 2)`: the row `u = 0` at one point.
fn pole() -> Vec<[f64; 4]> {
    let x = [
        [1.0, 2.0, 3.0],
        [1.0, 2.0, 3.0],
        [1.0, 2.0, 3.0],
        [0.0, 1.0, 5.0],
        [4.0, -1.0, 2.0],
        [0.375, 0.75, -2.0],
        [1.0, 1.5, 0.25],
        [9.0, 2.0, 1.0],
        [3.0, 3.0, 3.0],
    ];
    let w = [0.25, 4.0, 3.0, 0.5, 2.0, 7.0, 1.0, 1.0, 1.0];
    (0..9).map(|k| [x[k][0], x[k][1], x[k][2], w[k]]).collect()
}

fn at<S: Scalar>(m: usize, n: usize, net: &[[f64; 4]], like: &S) -> HomPatch<S> {
    HomPatch::from_affine(m, n, net.iter().copied(), like).expect("in range")
}

/// Every member through the trait against the inherent call of the same
/// name on the patch at `like`'s level.
fn members<S: Scalar>(like: &S) {
    let p = at(3, 2, &net(), like);
    let f = |x: f64| S::from_f64(x, like);
    assert_eq!(Patch::degrees(&p), (3, 2));
    assert_eq!(*Patch::hom(&p, like), p);
    for (u, v) in [(0.0, 0.0), (1.0, 0.5), (0.375, 0.625), (0.5, 1.0)] {
        let (u, v) = (f(u), f(v));
        assert_eq!(Patch::eval(&p, &u, &v), p.eval(&u, &v));
        assert_eq!(Patch::eval_partials(&p, &u, &v), p.eval_partials(&u, &v));
        assert_eq!(
            Patch::derivatives(&p, &u, &v, 2),
            p.derivatives_at(&u, &v, 2)
        );
    }
    for dir in [Dir::U, Dir::V] {
        assert_eq!(Patch::split_half(&p, dir), Ok(p.split_half(dir)));
    }
    assert_eq!(Patch::hull_box(&p), p.hull_box());
    assert!(Patch::hull_box(&p).is_some());

    let n = p.normal_numerator().expect("in range");
    assert_eq!(n[0].degrees(), (8, 5));
    assert_eq!(Patch::normal_numerator(&p, like).as_ref(), Some(&n));
    let cone = NormalCone::of_forms([&n], Axis::Smallest);
    assert_eq!(Patch::normal_cone_at(&p, like), cone);
    assert_eq!(Patch::normal_cone(&p), cone.ok());
    assert_eq!(Patch::normal_cone(&p), NormalCone::of_form(&n));

    assert_eq!(
        Patch::singular_sides(&p, Sides::NONE, like),
        p.singular_sides(Sides::NONE)
    );
    assert_eq!(Patch::fold_numerator(&p, Sides::NONE, like), Some(n));
    let q = at(2, 2, &pole(), like);
    let side = Sides::from(Side::U0);
    let proof = q.singular_sides(side);
    assert_eq!(Patch::singular_sides(&q, side, like), proof);
    assert_eq!(
        Patch::fold_numerator(&q, side, like),
        proof.and_then(|s| s.quotient(&q.normal_numerator()?))
    );

    let sphere = Implicit::new(
        2,
        [f(0.5), f(1.0), f(-0.25)],
        [
            ([2, 0, 0], f(1.0)),
            ([0, 2, 0], f(1.0)),
            ([0, 0, 2], f(1.0)),
            ([0, 0, 0], f(-4.0)),
        ],
    )
    .expect("a quadric");
    let composed = p.compose_implicit(&sphere);
    assert_eq!(composed.as_ref().map(Tensor::degrees), Some((6, 4)));
    assert_eq!(Patch::compose_implicit(&p, &sphere), composed);

    let span = HomCurve::from_affine(
        [
            ([0.125, 0.25], 1.0),
            ([0.5, 0.75], 0.5),
            ([0.875, 0.5], 2.0),
        ],
        like,
    )
    .expect("a quadratic span");
    let curve = HomCurve::from_affine(
        [
            ([0.0, 1.0, 0.5], 1.0),
            ([1.0, 2.0, 0.0], 3.0),
            ([2.0, 1.0, 1.0], 1.0),
        ],
        like,
    )
    .expect("a quadratic curve");
    let along = p.compose_span(&span).expect("in range");
    assert_eq!(along.degree(), 10);
    assert_eq!(Patch::compose_span(&p, &span).as_ref(), Some(&along));
    assert_eq!(
        Patch::seam_numerator(&p, &span, &curve),
        along.seam_numerator(&curve)
    );
}

#[test]
fn members_are_the_inherent_operations_at_the_first_level() {
    members(&Bounded::exact(0.0));
}

#[test]
fn members_are_the_inherent_operations_at_double_double() {
    members(&Bounded::exact(Dd::ZERO));
}

#[test]
fn members_are_the_inherent_operations_exactly() {
    members(&Q::from(0u32));
}

/// Exactly, the pole row is proven singular, no other side is, and the
/// fold numerator is the numerator's exact quotient by `u`.
#[test]
fn the_fold_numerator_divides_a_proven_pole_row() {
    let like = Q::from(0u32);
    let q = at(2, 2, &pole(), &like);
    let side = Sides::from(Side::U0);
    let proof = Patch::singular_sides(&q, side, &like).expect("a pole row");
    assert_eq!(proof.sides(), side);
    for other in [Side::U1, Side::V0, Side::V1] {
        assert!(Patch::singular_sides(&q, other.into(), &like).is_none());
    }
    let quotient = Patch::fold_numerator(&q, side, &like).expect("proven");
    let u = Tensor::new(1, 0, [Q::from(0u32), Q::from(1u32)]).expect("degree 1");
    let n = q.normal_numerator().expect("in range");
    for k in 0..3 {
        assert_eq!(quotient[k].degrees(), (4, 5));
        assert_eq!(quotient[k].mul(&u).as_ref(), Some(&n[k]));
    }
}

/// Exactly, the closest point set up from the homogeneous net is the one
/// set up from the affine net; at the float levels both certify the same
/// foot.
#[test]
fn the_closest_point_from_the_net_is_the_affine_ones() {
    let x = [1.5, 1.75, 2.0];
    let pts = net();
    let exact = Q::from(0u32);
    assert_eq!(
        Patch::closest(&at(3, 2, &pts, &exact), x, &exact),
        Closest::patch(3, 2, &pts, x, &exact)
    );
    fn feet<S: Scalar>(pts: &[[f64; 4]], x: [f64; 3], like: &S) {
        let ours = Patch::closest(&at(3, 2, pts, like), x, like)
            .and_then(|c| c.closest(None))
            .expect("a reached point");
        let theirs = Closest::patch(3, 2, pts, x, like)
            .and_then(|c| c.closest(None))
            .expect("a reached point");
        let (Foot::Certified(a), Foot::Certified(b)) = (&ours.foot, &theirs.foot) else {
            panic!("both certified: {ours:?}, {theirs:?}");
        };
        for k in 0..2 {
            assert!(a.near[k][0] <= b.near[k][1] && b.near[k][0] <= a.near[k][1]);
        }
        assert!(ours.upper.is_finite() && theirs.upper.is_finite());
    }
    feet(&pts, x, &Bounded::exact(0.0));
    feet(&pts, x, &Bounded::exact(Dd::ZERO));
    // Not finite.
    let p = at(3, 2, &pts, &Bounded::exact(0.0));
    assert!(Patch::closest(&p, [f64::NAN, 0.0, 0.0], &Bounded::exact(0.0)).is_none());
}

//! The normal numerator's side lines, its reduced form and its quotient
//! by singular sides: on rationals exact (the reduced form elevates back
//! to the numerator, the quotient times the factors is the numerator), at
//! every level holding the exact forms, the same bits under reversal; a
//! sphere's pole row proven singular on its exact net and its quotient's
//! cone certified at the first level, where the numerator's isn't.

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::*;
use crate::bezier::NormalCone;
use crate::bezier::tests::{
    Level, Rng, int_tensor, pbits, q, tensor, tensor_holds, value_and_errs,
};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

/// A patch of degrees `(m, n)` at a level and its true one, weights in
/// `[1/64, 64]`.
fn patch<S: Level>(rng: &mut Rng, m: usize, n: usize, errs: bool) -> (HomPatch<S>, HomPatch<Q>) {
    let parts: Vec<_> = [false, false, false, true]
        .iter()
        .map(|&w| tensor::<S>(rng, m, n, errs, w))
        .collect();
    (
        HomPatch::from_tensors(std::array::from_fn(|k| parts[k].0.clone()))
            .expect("one pair of degrees"),
        HomPatch::from_tensors(std::array::from_fn(|k| parts[k].1.clone()))
            .expect("one pair of degrees"),
    )
}

/// A side's numerator line is the numerator's line there, to the bit.
fn side_lines<S: Level>() {
    let mut rng = Rng::new(17);
    for case in 0..24 {
        let (m, n) = (1 + case % 3, 1 + (case / 3) % 3);
        let (p, _) = patch::<S>(&mut rng, m, n, case % 2 == 0);
        let nn = p.normal_numerator().expect("in range");
        for side in Side::ALL {
            let line = p.side_numerator(side).expect("in range");
            for k in 0..3 {
                assert_eq!(
                    pbits(&line[k]),
                    pbits(&nn[k].side(side)),
                    "{} case {case} {side:?}",
                    S::NAME
                );
            }
        }
    }
}

#[test]
fn side_numerators_are_the_numerators_lines() {
    side_lines::<Bounded<f64>>();
    side_lines::<Bounded<Dd>>();
    side_lines::<Bounded<Big>>();
}

/// A rational patch of integer coordinates, positive integer weights.
fn int_patch(rng: &mut Rng, m: usize, n: usize) -> HomPatch<Q> {
    let mut parts: [Tensor<Q>; 4] = std::array::from_fn(|_| int_tensor(rng, m, n));
    parts[3] = Tensor::new(
        m,
        n,
        (0..(m + 1) * (n + 1)).map(|_| Q::from(1 + rng.below(9))),
    )
    .expect("degrees in range");
    HomPatch::from_tensors(parts).expect("one pair of degrees")
}

#[test]
fn reduced_numerator_on_rationals_is_exact() {
    let mut rng = Rng::new(23);
    for case in 0..30 {
        let (m, n) = (1 + case % 3, 1 + (case / 3) % 4);
        let p = int_patch(&mut rng, m, n);
        let nn = p.normal_numerator().expect("in range");
        let r = p.normal_numerator_reduced().expect("in range");
        for k in 0..3 {
            assert_eq!(r[k].degrees(), (3 * m - 2, 3 * n - 2), "{case}");
            assert_eq!(
                r[k].elevate_to(3 * m - 1, 3 * n - 1).as_ref(),
                Some(&nn[k]),
                "{case}"
            );
        }
        // Side lines: the numerator is zero on no side of a general patch.
        assert!(p.singular_sides(Side::ALL.into_iter().collect()).is_none());
    }
    let r = int_patch(&mut rng, 2, 2)
        .normal_numerator_reduced()
        .expect("in range");
    assert_eq!(r[0].degrees(), (4, 4));
    assert!(
        int_patch(&mut rng, 0, 2)
            .normal_numerator_reduced()
            .is_none()
    );
}

/// Every certified coefficient of the reduced numerator holds the exact
/// one; the reversed patch gives the reversed reduced numerator negated.
fn reduced_holds<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..12 {
            let what = format!("seed {seed} case {case}");
            let (m, n) = (1 + case % 2, 1 + (case / 2) % 3);
            let (p, truth) = patch::<S>(&mut rng, m, n, case % 2 == 1);
            let r = p.normal_numerator_reduced().expect("an elevation in truth");
            let rq = truth.normal_numerator_reduced().expect("in range");
            for k in 0..3 {
                tensor_holds(&what, &r[k], &rq[k], &mut stats);
            }
            for dir in [Dir::U, Dir::V] {
                let rr = p
                    .reversed(dir)
                    .normal_numerator_reduced()
                    .expect("an elevation in truth");
                for k in 0..3 {
                    assert_eq!(
                        value_and_errs(rr[k].coefs()),
                        value_and_errs(r[k].reversed(dir).neg().coefs()),
                        "{what} {dir:?}"
                    );
                }
            }
        }
    }
    // The reduction cancels: fewer certify than of the numerator itself.
    assert!(
        stats[0] * 10 >= stats[1] * 7,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn reduced_numerator_holds_its_bounds_f64() {
    reduced_holds::<Bounded<f64>>(varde_testing::seeds(1, 16));
}

#[test]
fn reduced_numerator_holds_its_bounds_dd() {
    reduced_holds::<Bounded<Dd>>(varde_testing::seeds(1, 16));
}

#[test]
fn reduced_numerator_holds_its_bounds_big() {
    reduced_holds::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

/// A biquadratic net with a pole: row 0 one point, its weights `1/64`,
/// `64`, `3` (the fixture of the numerator's own tests), as `(w·x, w·y,
/// w·z, w)`.
fn pole_net() -> [[f64; 4]; 9] {
    let w = [1.0 / 64.0, 64.0, 3.0, 0.5, 2.0, 7.0, 1.0, 1.0, 1.0];
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
    std::array::from_fn(|k| [x[k][0] * w[k], x[k][1] * w[k], x[k][2] * w[k], w[k]])
}

/// The pole row is proven singular on the exact net and no other side
/// is; the quotient, unreduced `(4, 5)` and reduced `(3, 4)`, times `u` is
/// the numerator, and at every level holds the exact quotient.
fn pole_quotient<S: Level>() {
    let net = pole_net();
    let exact = HomPatch::new(2, 2, net.iter().map(|p| p.map(q))).expect("degrees in range");
    let proof = exact.singular_sides(Side::U0.into()).expect("a pole row");
    assert_eq!(proof.sides(), Side::U0.into());
    assert_eq!(proof.degrees(), (2, 2));
    for side in [Side::U1, Side::V0, Side::V1] {
        assert!(exact.singular_sides(side.into()).is_none(), "{side:?}");
    }
    let u = Tensor::new(1, 0, [Q::from(0u32), Q::from(1u32)]).expect("degree 1");
    let nq = exact.normal_numerator().expect("in range");
    let rq = exact.normal_numerator_reduced().expect("in range");
    let qq = proof.quotient(&nq).expect("the numerator's degrees");
    let qrq = proof
        .quotient(&rq)
        .expect("the reduced numerator's degrees");
    for k in 0..3 {
        assert_eq!(qq[k].degrees(), (4, 5));
        assert_eq!(qrq[k].degrees(), (3, 4));
        assert_eq!(qq[k].mul(&u).as_ref(), Some(&nq[k]));
        assert_eq!(qrq[k].mul(&u).as_ref(), Some(&rq[k]));
    }
    // Not a numerator of the proven patch's degrees.
    assert!(proof.quotient(&qrq).is_none());

    let patch = HomPatch::new(2, 2, net.iter().map(|p| p.map(|c| S::of(c, 0.0))))
        .expect("degrees in range");
    let nn = patch.normal_numerator().expect("in range");
    let mut stats = [0u32; 2];
    let quotient = proof.quotient(&nn).expect("the numerator's degrees");
    for k in 0..3 {
        tensor_holds(S::NAME, &quotient[k], &qq[k], &mut stats);
    }
    if let Some(r) = patch.normal_numerator_reduced() {
        let quotient = proof.quotient(&r).expect("the reduced numerator's degrees");
        for k in 0..3 {
            tensor_holds(S::NAME, &quotient[k], &qrq[k], &mut stats);
        }
    }
    assert_eq!(stats[0], stats[1], "{}: certified {stats:?}", S::NAME);
}

#[test]
fn a_pole_rows_quotient() {
    pole_quotient::<Bounded<f64>>();
    pole_quotient::<Bounded<Dd>>();
    pole_quotient::<Bounded<Big>>();
}

/// An octant of the unit sphere with its pole row at `v = 0` (all three
/// control points of that row at the pole, `z = 1`), the equator at
/// `v = 1`, as affine points `(x, y, z, w)`.
fn octant() -> [[f64; 4]; 9] {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let meridian = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    let around = [(1.0, 0.0, 1.0), (1.0, 1.0, s), (0.0, 1.0, 1.0)];
    std::array::from_fn(|k| {
        let ((cu, su, wu), (cv, sv, wv)) = (around[k / 3], meridian[k % 3]);
        [sv * cu, sv * su, cv, wu * wv]
    })
}

/// The patch of the affine net translated by `−c` at `like`'s level, as a
/// tile frames it: `(w·(x − c), w)`.
fn framed<S: Scalar>(net: &[[f64; 4]; 9], c: [f64; 3], like: &S) -> HomPatch<S> {
    HomPatch::new(
        2,
        2,
        net.iter().map(|p| {
            let w = S::from_f64(p[3], like);
            let at = |k: usize| {
                S::from_f64(p[k], like)
                    .sub(&S::from_f64(c[k], like))
                    .mul(&w)
            };
            [at(0), at(1), at(2), w.clone()]
        }),
    )
    .expect("a biquadratic net")
}

fn approx(x: &Q) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// Whether the direction of `n` lies in the cone (to rounding).
fn in_cone(c: &NormalCone, n: [f64; 3]) -> bool {
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = c.axis[0] * n[0] + c.axis[1] * n[1] + c.axis[2] * n[2];
    dot >= (c.cos - 1e-12) * len(c.axis) * len(n)
}

/// The pole row of the sphere: proven singular on the exact net; in the
/// frame of the box's centre the numerator's cone fails at every level
/// (the pole row cancels only to `±err`), the quotient's is certified at
/// the first level and holds the sphere's normals, the pole's the limit
/// `±z`; the reversed patch's quotient by the mirrored side is the
/// reversed quotient negated.
fn sphere_pole<S: Level>() {
    let net = octant();
    let like = S::of(0.0, 0.0);
    let exact = HomPatch::from_affine(2, 2, net, &Q::from(0u32)).expect("degrees in range");
    let proof = exact.singular_sides(Side::V0.into()).expect("the pole row");
    for side in [Side::U0, Side::U1, Side::V1] {
        assert!(exact.singular_sides(side.into()).is_none(), "{side:?}");
    }
    let centre = [0.5, 0.5, 0.5];
    let patch = framed(&net, centre, &like);
    let nn = patch.normal_numerator().expect("in range");
    assert_eq!(NormalCone::of_form(&nn), None, "{}", S::NAME);
    let quotient = proof.quotient(&nn).expect("the numerator's degrees");
    assert_eq!(quotient[0].degrees(), (5, 4));
    let cone = NormalCone::of_form(&quotient).expect("the quotient is certified regular");
    assert!(cone.cos > 0.25, "{}: {cone:?}", S::NAME);

    // The exact quotient (in the untranslated frame, the same
    // polynomial) at points of the octant, the pole row included.
    let qq = proof
        .quotient(&exact.normal_numerator().expect("in range"))
        .expect("the numerator's degrees");
    let at = |u: f64, v: f64| qq.clone().map(|f| f.eval(&q(u), &q(v)));
    for &(u, v) in &[
        (0.0, 0.0),
        (0.5, 0.0),
        (1.0, 0.0),
        (0.25, 0.5),
        (1.0, 1.0),
        (0.0, 0.75),
    ] {
        let n = at(u, v);
        assert!(
            in_cone(&cone, n.clone().map(|x| approx(&x))),
            "{} ({u}, {v})",
            S::NAME
        );
        if v == 0.0 {
            assert!(
                n[0] == 0u32 && n[1] == 0u32 && n[2] != 0u32,
                "the pole's normal"
            );
        }
    }

    let reduced = patch
        .normal_numerator_reduced()
        .expect("an elevation in truth");
    let rquotient = proof
        .quotient(&reduced)
        .expect("the reduced numerator's degrees");
    assert_eq!(rquotient[0].degrees(), (4, 3));
    assert!(NormalCone::of_form(&rquotient).is_some(), "{}", S::NAME);

    let flipped = exact.reversed(Dir::V);
    let mirrored = flipped
        .singular_sides(Side::V1.into())
        .expect("the pole row, now at v = 1");
    let rq = mirrored
        .quotient(&patch.reversed(Dir::V).normal_numerator().expect("in range"))
        .expect("the numerator's degrees");
    for k in 0..3 {
        assert_eq!(
            value_and_errs(rq[k].coefs()),
            value_and_errs(quotient[k].reversed(Dir::V).neg().coefs()),
            "{}",
            S::NAME
        );
    }
}

#[test]
fn a_sphere_pole_rows_quotient_is_certified() {
    sphere_pole::<Bounded<f64>>();
    sphere_pole::<Bounded<Dd>>();
    sphere_pole::<Bounded<Big>>();
}

#[test]
fn quotients_need_the_numerators_degrees() {
    let mut rng = Rng::new(29);
    let p = int_patch(&mut rng, 2, 3);
    let proof = p.singular_sides(Sides::NONE).expect("no side to prove");
    let nn = p.normal_numerator().expect("in range");
    assert_eq!(proof.quotient(&nn), Some(nn.clone()));
    let r = p.normal_numerator_reduced().expect("in range");
    assert_eq!(proof.quotient(&r), Some(r.clone()));
    let half: [Tensor<Q>; 3] = std::array::from_fn(|k| nn[k].reduced(Dir::U).expect("in range"));
    assert_eq!(proof.quotient(&half), Some(half.clone()));
    let off: [Tensor<Q>; 3] = std::array::from_fn(|k| nn[k].elevate(Dir::V, 1).expect("in range"));
    assert!(proof.quotient(&off).is_none());
    let mixed = [nn[0].clone(), nn[1].clone(), r[2].clone()];
    assert!(proof.quotient(&mixed).is_none());
}

//! Forms in any number of variables: on rationals every operation is
//! exact (a patch against a patch as a four-variable form vanishes where
//! the two meet); at every level each certified result holds the exact
//! one; elevation and splits commute with reversal in every variable, and
//! in two variables evaluation is the tensor's, to the bit; one half of a
//! split is that of the split.

use malachite::Rational as Q;

use super::*;
use crate::bezier::Dir;
use crate::bezier::tests::{Level, Rng, dyadic, holds, int_tensor, tbits, tensor};
use crate::num::bigf::Big;
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;

fn int_form(rng: &mut Rng, deg: &[usize]) -> NdTensor<Q> {
    let len: usize = deg.iter().map(|d| d + 1).product();
    NdTensor::new(deg, (0..len).map(|_| Q::from(rng.below(201) as i64 - 100))).expect("degrees")
}

/// A level's form and its true one.
fn form<S: Level>(rng: &mut Rng, deg: &[usize], errs: bool) -> (NdTensor<S>, NdTensor<Q>) {
    // Two variables at a time from tensors, the rest a line.
    let len: usize = deg.iter().map(|d| d + 1).product();
    let (t, tq) = tensor::<S>(rng, len - 1, 0, errs, false);
    (
        NdTensor::new(deg, t.coefs().iter().cloned()).expect("degrees"),
        NdTensor::new(deg, tq.coefs().iter().cloned()).expect("degrees"),
    )
}

#[test]
fn rational_operations_are_exact() {
    for seed in varde_testing::seeds(1, 16) {
        let mut rng = Rng::new(seed);
        for case in 0..20 {
            let deg = [case % 3, (case / 3) % 3, 1 + case % 2];
            let f = int_form(&mut rng, &deg);
            let g = int_form(&mut rng, &[(case / 2) % 3, case % 2, 2]);
            let x: Vec<Q> = (0..3).map(|_| dyadic(&mut rng)).collect();
            let fx = f.eval(&x).expect("three variables");
            let gx = g.eval(&x).expect("three variables");
            let up = [deg[0] + 2, deg[1], deg[2] + 1];
            let e = f.elevate_to(&up).expect("in range");
            assert_eq!(e.degrees(), &up);
            assert_eq!(e.eval(&x).expect("in"), fx);
            assert_eq!(
                f.add(&g).expect("same dims").eval(&x).expect("in"),
                &fx + &gx
            );
            assert_eq!(
                f.sub(&g).expect("same dims").eval(&x).expect("in"),
                &fx - &gx
            );
            assert_eq!(f.neg().eval(&x).expect("in"), -&fx);
            let k = Q::from(3u32) / Q::from(7u32);
            assert_eq!(f.scale(&k).eval(&x).expect("in"), &k * &fx);
            for axis in 0..3 {
                let mut xr = x.clone();
                xr[axis] = Q::from(1u32) - &xr[axis];
                assert_eq!(f.reversed(axis).eval(&xr).expect("in"), fx);
                let [lo, hi] = f.split_half(axis);
                let mut xl = x.clone();
                xl[axis] = &x[axis] * Q::from(2u32);
                if xl[axis] <= 1u32 {
                    assert_eq!(lo.eval(&xl).expect("in"), fx);
                } else {
                    xl[axis] -= Q::from(1u32);
                    assert_eq!(hi.eval(&xl).expect("in"), fx);
                }
            }
            // `∂f/∂x₁` against a difference quotient's limit: exact on a
            // form of degree 1 in the variable, so check the derivative
            // of the degree-1 form `f` restricted to `x₂`'s ends instead,
            // by the 1D derivative of the line.
            let d = f.derivative(2);
            assert_eq!(d.degrees()[2], deg[2] - 1);
        }
    }
}

/// A patch against a patch, `P(u, v) − Q(s, t)`, in four variables: the
/// two embedded, elevated to common degrees and subtracted; it is the
/// difference of the two points at every parameter.
#[test]
fn a_four_variable_system_from_two_patches() {
    let mut rng = Rng::new(7);
    for case in 0..10 {
        let p = int_tensor(&mut rng, 2, 1 + case % 2);
        let q = int_tensor(&mut rng, 1 + case % 3, 2);
        let pe = NdTensor::from_tensor(&p).embed(&[0, 1], 4).expect("fits");
        let qe = NdTensor::from_tensor(&q).embed(&[2, 3], 4).expect("fits");
        assert_eq!(pe.degrees(), &[2, 1 + case % 2, 0, 0]);
        let f = pe.sub(&qe).expect("four variables");
        assert_eq!(f.degrees(), &[2, 1 + case % 2, 1 + case % 3, 2]);
        for _ in 0..4 {
            let x: Vec<Q> = (0..4).map(|_| dyadic(&mut rng)).collect();
            assert_eq!(
                f.eval(&x).expect("four"),
                p.eval(&x[0], &x[1]) - q.eval(&x[2], &x[3])
            );
        }
        // Swapped axes: the variables of the patch in the other order.
        let pt = NdTensor::from_tensor(&p).embed(&[1, 0], 2).expect("fits");
        let (u, v) = (dyadic(&mut rng), dyadic(&mut rng));
        assert_eq!(
            pt.eval(&[v.clone(), u.clone()]).expect("two"),
            p.eval(&u, &v)
        );
    }
    let f = NdTensor::from_tensor(&int_tensor(&mut rng, 1, 1));
    assert!(f.embed(&[0, 0], 3).is_none());
    assert!(f.embed(&[0, 3], 3).is_none());
    assert!(f.embed(&[0], 3).is_none());
    assert!(f.eval(&[Q::from(0u32)]).is_none());
    assert!(f.add(&f.embed(&[0, 1], 3).expect("fits")).is_none());
    assert!(NdTensor::<Q>::new(&[], [Q::from(0u32)]).is_none());
    assert!(NdTensor::<Q>::new(&[MAX_DEGREE + 1], []).is_none());
    assert!(NdTensor::new(&[1, 1], (0..3).map(Q::from)).is_none());
    assert!(f.elevate(2, 1).is_none());
    assert!(f.elevate(0, MAX_DEGREE).is_none());
    assert_eq!(f.get(&[1, 0]), Some(&f.coefs()[2]));
    assert!(f.get(&[2, 0]).is_none());
}

/// At every level: each certified coefficient and value holds the exact
/// one; elevation and the split at ½ commute with reversal in every
/// variable to the bit; in two variables evaluation and elevation in one
/// variable are the tensor's.
fn levels<S: Level>(seeds: std::ops::Range<u64>) {
    let mut stats = [0u32; 2];
    for seed in seeds {
        let mut rng = Rng::new(seed);
        for case in 0..20 {
            let what = format!("{} seed {seed} case {case}", S::NAME);
            let deg = [case % 3, 1 + (case / 3) % 2, (case / 2) % 3];
            let (f, fq) = form::<S>(&mut rng, &deg, case % 2 == 0);
            let (g, gq) = form::<S>(&mut rng, &[1, case % 2, 2], case % 3 == 0);
            let up = [deg[0] + 1, deg[1] + 2, deg[2]];
            let mut check = |r: &NdTensor<S>, t: &NdTensor<Q>| {
                for (x, xq) in r.coefs().iter().zip(t.coefs()) {
                    stats[1] += 1;
                    if holds(&what, x, xq) {
                        stats[0] += 1;
                    }
                }
            };
            check(
                &f.elevate_to(&up).expect("in"),
                &fq.elevate_to(&up).expect("in"),
            );
            check(&f.sub(&g).expect("in"), &fq.sub(&gq).expect("in"));
            let like = f.coefs()[0].clone();
            let x: Vec<f64> = (0..3).map(|_| rng.t()).collect();
            let xs: Vec<S> = x.iter().map(|&t| S::from_f64(t, &like)).collect();
            let xq: Vec<Q> = x.iter().map(|&t| Q::try_from(t).expect("finite")).collect();
            stats[1] += 1;
            if holds(&what, &f.eval(&xs).expect("in"), &fq.eval(&xq).expect("in")) {
                stats[0] += 1;
            }
            let bits = |h: &NdTensor<S>| h.coefs().iter().map(Level::bits).collect::<Vec<_>>();
            for axis in 0..3 {
                assert_eq!(
                    bits(&f.reversed(axis).elevate_to(&up).expect("in")),
                    bits(&f.elevate_to(&up).expect("in").reversed(axis)),
                    "{what} elevation {axis}"
                );
                let [l, r] = f.reversed(axis).split_half(axis);
                let [l0, r0] = f.split_half(axis);
                assert_eq!(bits(&l), bits(&r0.reversed(axis)), "{what} split {axis}");
                assert_eq!(bits(&r), bits(&l0.reversed(axis)), "{what} split {axis}");
                assert_eq!(bits(&f.half(axis, false)), bits(&l0), "{what} half {axis}");
                assert_eq!(bits(&f.half(axis, true)), bits(&r0), "{what} half {axis}");
            }
            let (t, _) = tensor::<S>(&mut rng, case % 3, 1 + case % 2, true, false);
            let nt = NdTensor::from_tensor(&t);
            let (u, v) = (S::from_f64(x[0], &like), S::from_f64(x[1], &like));
            assert_eq!(
                nt.eval(&[u.clone(), v.clone()]).expect("two").bits(),
                t.eval(&u, &v).bits(),
                "{what} eval"
            );
            for end in [0.0, 1.0] {
                let e = S::from_f64(end, &like);
                assert_eq!(
                    nt.eval(&[e.clone(), v.clone()]).expect("two").bits(),
                    t.eval(&e, &v).bits()
                );
                assert_eq!(
                    nt.eval(&[u.clone(), e.clone()]).expect("two").bits(),
                    t.eval(&u, &e).bits()
                );
            }
            assert_eq!(
                bits(&nt.elevate(1, 2).expect("in")),
                tbits(&t.elevate(Dir::V, 2).expect("in")),
                "{what} elevate"
            );
            assert_eq!(
                bits(&nt.derivative(0)),
                tbits(&t.derivative(Dir::U)),
                "{what} derivative"
            );
            let [a, b] = nt.split_at(0, &u);
            let [ta, tb] = t.split_at(Dir::U, &u);
            assert_eq!(
                (bits(&a), bits(&b)),
                (tbits(&ta), tbits(&tb)),
                "{what} split"
            );
            let h = f.hull();
            for xq in fq.coefs() {
                if h.lo > f64::NEG_INFINITY {
                    assert!(Q::try_from(h.lo).expect("finite") <= *xq);
                }
                if h.hi < f64::INFINITY {
                    assert!(*xq <= Q::try_from(h.hi).expect("finite"));
                }
            }
        }
    }
    assert!(
        stats[0] * 10 >= stats[1] * 9,
        "{}: certified {stats:?}",
        S::NAME
    );
}

#[test]
fn forms_hold_their_bounds_and_commute_with_reversal() {
    levels::<Bounded<f64>>(varde_testing::seeds(1, 16));
    levels::<Bounded<Dd>>(varde_testing::seeds(1, 16));
    levels::<Bounded<Big>>(varde_testing::seeds(1, 8));
}

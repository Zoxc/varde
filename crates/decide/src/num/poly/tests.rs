use std::cmp::Ordering;

use malachite::Rational;

use super::*;

fn q(n: i64, d: i64) -> Rational {
    Rational::from_signeds(n, d)
}

fn int(c: &[i64]) -> Poly {
    trim(c.iter().map(|&x| Rational::from(x)).collect())
}

#[test]
fn arithmetic_and_bernstein_form() {
    // (τ − 1)(τ + 2) = τ² + τ − 2.
    let p = mul(&int(&[-1, 1]), &int(&[2, 1]));
    assert_eq!(p, int(&[-2, 1, 1]));
    assert_eq!(deriv(&p), int(&[1, 2]));
    assert_eq!(eval(&p, &Rational::from(3)), Rational::from(10));
    assert_eq!(rem(&p, &int(&[-1, 1])), Vec::<Rational>::new());
    assert_eq!(quot(&p, &int(&[-1, 1])), int(&[2, 1]));
    // p(1 + 2τ) = 4τ² + 6τ.
    assert_eq!(compose_affine(&p, &q(1, 1), &q(2, 1)), int(&[0, 6, 4]));
    // Bernstein (1, 0, 1) over [0, 1]: (1 − τ)² + τ² = 1 − 2τ + 2τ².
    let b = [1, 0, 1].map(Rational::from);
    assert_eq!(from_bernstein(&b), int(&[1, -2, 2]));
    // gcd((τ−1)²(τ+2), (τ−1)(τ+5)) = τ − 1, monic.
    let a = mul(&mul(&int(&[-1, 1]), &int(&[-1, 1])), &int(&[2, 1]));
    assert_eq!(gcd(&a, &mul(&int(&[-1, 1]), &int(&[5, 1]))), int(&[-1, 1]));
    assert_eq!(square_free(&a), mul(&int(&[-1, 1]), &int(&[2, 1])));
    assert_eq!(pow(&int(&[-1, 1]), 0), int(&[1]));
    assert_eq!(pow(&int(&[-1, 1]), 2), int(&[1, -2, 1]));
}

#[test]
fn roots_are_isolated_with_their_multiplicities() {
    // (τ − ⅓)² (τ − ½)(τ² − 2) on [0, 2]: ⅓ double, ½ and √2 simple.
    let third = trim(vec![q(-1, 3), q(1, 1)]);
    let p = mul(
        &mul(&mul(&third, &third), &trim(vec![q(-1, 2), q(1, 1)])),
        &int(&[-2, 0, 1]),
    );
    let rs = roots(&p, &q(0, 1), &q(2, 1));
    assert_eq!(rs.len(), 3);
    let mult: Vec<u32> = rs.iter().map(|r| r.1).collect();
    assert_eq!(mult, [2, 1, 1]);
    assert_eq!(rs[0].0.cmp_rational(&q(1, 3)), Ordering::Equal);
    assert_eq!(rs[1].0.cmp_rational(&q(1, 2)), Ordering::Equal);
    let root2 = &rs[2].0;
    assert!(root2.rational().is_none());
    assert_eq!(root2.cmp_rational(&q(141, 100)), Ordering::Greater);
    assert_eq!(root2.cmp_rational(&q(142, 100)), Ordering::Less);
    // The sign of τ² − 2 at √2 is zero, of τ² − 3 negative, of τ − 1
    // positive.
    assert_eq!(root2.sign_of(&int(&[-2, 0, 1])), Ordering::Equal);
    assert_eq!(root2.sign_of(&int(&[-3, 0, 1])), Ordering::Less);
    assert_eq!(root2.sign_of(&int(&[-1, 1])), Ordering::Greater);
    // A sign at a root nearly cancelling: τ − 1.41421356 at √2.
    let near = trim(vec![q(-141_421_356, 100_000_000), q(1, 1)]);
    assert_eq!(root2.sign_of(&near), Ordering::Greater);
    let [lo, hi] = root2.enclose();
    assert!(lo < std::f64::consts::SQRT_2 && std::f64::consts::SQRT_2 < hi);
}

#[test]
fn roots_at_the_ends_of_the_interval_are_found() {
    // τ(τ − 1) on [0, 1]: both ends, exactly.
    let p = int(&[0, -1, 1]);
    let rs = roots(&p, &q(0, 1), &q(1, 1));
    assert_eq!(rs.len(), 2);
    assert_eq!(rs[0].0.rational(), Some(&q(0, 1)));
    assert_eq!(rs[1].0.rational(), Some(&q(1, 1)));
    // None in (0, 1) for τ² + 1, nor for a constant.
    assert!(roots(&int(&[1, 0, 1]), &q(0, 1), &q(1, 1)).is_empty());
    assert!(roots(&int(&[3]), &q(0, 1), &q(1, 1)).is_empty());
    // The narrowing keeps the root.
    let mut r = roots(&int(&[-2, 0, 1]), &q(0, 1), &q(2, 1)).remove(0).0;
    r.narrow(&q(1, 1 << 40));
    let [lo, hi] = r.interval();
    assert!(hi - lo <= q(1, 1 << 40));
    assert_eq!(r.sign_of(&int(&[-2, 0, 1])), Ordering::Equal);
}

/// Roots closer than the isolation's depth (`2⁻⁸⁰` apart) separate by
/// isolating the cluster again on its own interval; a root on a split
/// point is exact.
#[test]
fn roots_closer_than_the_depth_separate() {
    let a = q(1, 3);
    let b = &a + (Rational::from(1u32) >> 80u64);
    let p = mul(
        &mul(&[-a.clone(), q(1, 1)], &[-b.clone(), q(1, 1)]),
        &[q(-1, 2), q(1, 1)],
    );
    let r = roots(&p, &q(0, 1), &q(1, 1));
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].0.cmp_rational(&a), Ordering::Equal);
    assert_eq!(r[1].0.cmp_rational(&b), Ordering::Equal);
    assert!(r[0].0.interval()[1] <= r[1].0.interval()[0]);
    assert_eq!(r[2].0.rational(), Some(&q(1, 2)));
    assert!(r.iter().all(|(_, m)| *m == 1));
}

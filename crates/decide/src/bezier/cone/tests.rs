//! Normal cones: the cone holds every value of the form, also when its
//! components have different degrees.

use malachite::Rational as Q;

use super::*;
use crate::num::bounded::Bounded;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// Whether the exact `n` lies in the cone: `a·n > 0` and
/// `(a·n)² ≥ cos²·|a|²·|n|²`.
fn in_cone(c: &NormalCone, n: [Q; 3]) -> bool {
    let a = c.axis.map(q);
    let dot = |x: &[Q; 3], y: &[Q; 3]| &x[0] * &y[0] + &x[1] * &y[1] + &x[2] * &y[2];
    let d = dot(&a, &n);
    d > 0u32 && &d * &d >= q(c.cos) * q(c.cos) * dot(&a, &a) * dot(&n, &n)
}

#[test]
fn components_of_different_degrees_are_raised_first() {
    let t = |m: usize, n: usize, c: &[f64]| Tensor::from_f64(m, n, c).unwrap();
    // `N = (1, −10·u, 0)`: a constant `x` and a linear `y` in `u`. Paired
    // index by index, only the first coefficients would count.
    let cases = [
        [t(0, 0, &[1.0]), t(1, 0, &[0.0, -10.0]), t(0, 0, &[0.0])],
        // The same `u` and `v` lines, the degrees swapped between
        // components: `N = (1, −10·u, 10·v)`.
        [
            t(0, 0, &[1.0]),
            t(1, 0, &[0.0, -10.0]),
            t(0, 1, &[0.0, 10.0]),
        ],
    ];
    let mut checked = 0;
    for n in &cases {
        let exact = n.clone().map(|f| {
            Tensor::new(
                f.degrees().0,
                f.degrees().1,
                f.coefs().iter().map(|b| q(b.v)),
            )
            .unwrap()
        });
        let Some(c) = NormalCone::of_form::<Bounded<f64>>(n) else {
            continue;
        };
        checked += 1;
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.25)] {
            let value = exact.clone().map(|f| f.eval(&q(u), &q(v)));
            assert!(in_cone(&c, value), "{c:?} misses N({u}, {v})");
        }
    }
    assert!(checked > 0, "every case uncertified");
}

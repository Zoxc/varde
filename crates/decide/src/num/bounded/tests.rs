//! The bounds against exact rational arithmetic: inputs anywhere in the
//! `f64` range (subnormals included) with any error parts, true values
//! anywhere inside their bounds, single operations and chains of them.
//! Every certified result must hold the exact result within its error, and
//! every computed error term must be at least `η`.

use std::cmp::Ordering;

use num_rational::BigRational as Q;

use super::*;

fn q(x: f64) -> Q {
    Q::from_float(x).expect("finite")
}

fn qabs(x: Q) -> Q {
    if x < q(0.0) { -x } else { x }
}

/// The kernels' seeded test generator, with draws across the `f64` range.
struct Rng(varde_testkit::rng::Rng);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(varde_testkit::rng::Rng::new(seed))
    }

    fn next(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn sign(&mut self, x: f64) -> f64 {
        if self.below(2) == 0 { x } else { -x }
    }

    /// A positive float with a random mantissa and an exponent field in
    /// `lo..=hi` (0 for subnormals, 2046 the top binade).
    fn magnitude(&mut self, lo: u64, hi: u64) -> f64 {
        let exp = lo + self.below(hi - lo + 1);
        f64::from_bits(exp << 52 | self.next() >> 12)
    }

    /// Any finite float, the edges of the range more often than chance.
    fn wide(&mut self) -> f64 {
        const EDGES: [f64; 8] = [
            0.0,
            ETA,
            f64::MIN_POSITIVE,
            f64::from_bits(0x000F_FFFF_FFFF_FFFF),
            1.0,
            1e-170,
            1e154,
            f64::MAX,
        ];
        let x = match self.below(8) {
            0 => EDGES[self.below(EDGES.len() as u64) as usize],
            1 => self.magnitude(0, 1),
            _ => self.magnitude(0, 2046),
        };
        self.sign(x)
    }

    /// A float of moderate magnitude, `2^±40`.
    fn typical(&mut self) -> f64 {
        let x = self.magnitude(1023 - 40, 1023 + 40);
        self.sign(x)
    }

    /// An error part for a value `v`: zero, relative to `v`, tiny or wide.
    fn error(&mut self, v: f64, wide: bool) -> f64 {
        match self.below(if wide { 5 } else { 3 }) {
            0 => 0.0,
            1 | 2 => v.abs() * self.magnitude(1023 - 60, 1023 - 20),
            3 => self.magnitude(0, 2),
            _ => self.wide().abs(),
        }
    }

    /// An input and a true value inside its bounds: an end, the middle or
    /// a point between.
    fn input(&mut self, wide: bool) -> (Bounded, Q) {
        let v = if wide { self.wide() } else { self.typical() };
        let b = Bounded::new(v, self.error(v, wide), self.error(v, wide));
        let e = q(b.err_trunc) + q(b.err_round);
        let lambda = match self.below(4) {
            0 => q(-1.0),
            1 => q(1.0),
            2 => q(0.0),
            _ => q((self.below(2049) as f64 - 1024.0) / 1024.0),
        };
        (b, q(v) + lambda * e)
    }
}

/// Checks a computed result: its error terms are at least `η`, and it
/// holds its exact value (see [`holds`]).
fn check(what: &str, r: Bounded, exact: Option<&Q>) -> bool {
    assert!(
        r.err_trunc >= ETA && r.err_round >= ETA,
        "{what}: an error term below η: {r:?}"
    );
    holds(what, r, exact)
}

/// Checks a result against its exact value (`None` where the operation is
/// undefined at the true operands); returns whether it was certified.
fn holds(what: &str, r: Bounded, exact: Option<&Q>) -> bool {
    if !r.is_certified() {
        return false;
    }
    assert!(r.v.is_finite(), "{what}: certified non-finite value {r:?}");
    let x = exact.unwrap_or_else(|| panic!("{what}: certified an undefined value {r:?}"));
    let dist = qabs(x.clone() - q(r.v));
    assert!(
        dist <= q(r.err_trunc) + q(r.err_round),
        "{what}: bound violated: {r:?}"
    );
    assert!(dist <= q(r.err()), "{what}: err() below the parts: {r:?}");
    if let Some(s) = r.sign() {
        assert_eq!(x.cmp(&q(0.0)), s, "{what}: wrong certified sign {r:?}");
    }
    true
}

/// The square root against `x`: `(v − e)² ≤ x ≤ (v + e)²`, the lower end
/// only when `v − e ≥ 0`.
fn check_sqrt(r: Bounded, x: &Q) -> bool {
    assert!(r.err_trunc >= ETA && r.err_round >= ETA, "sqrt: {r:?}");
    if !r.is_certified() {
        return false;
    }
    assert!(*x >= q(0.0), "sqrt: certified the root of a negative {r:?}");
    let e = q(r.err_trunc) + q(r.err_round);
    let (lo, hi) = (q(r.v) - e.clone(), q(r.v) + e);
    assert!(*x <= hi.clone() * hi, "sqrt: root above the bound {r:?}");
    if lo >= q(0.0) {
        assert!(lo.clone() * lo <= *x, "sqrt: root below the bound {r:?}");
    }
    true
}

/// `2ᵏ` for `k` in the normal range.
fn pow2(k: i32) -> f64 {
    f64::from_bits(u64::try_from(1023 + k).expect("normal") << 52)
}

fn bits(r: Bounded) -> [u64; 3] {
    [r.v.to_bits(), r.err_trunc.to_bits(), r.err_round.to_bits()]
}

fn quotient(a: &Q, b: &Q) -> Option<Q> {
    (*b != q(0.0)).then(|| a.clone() / b.clone())
}

#[test]
fn operations_hold_their_bounds() {
    let mut certified = [0u32; 2];
    let mut total = [0u32; 2];
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for case in 0..2000 {
            let wide = case % 2 == 0;
            let ((a, xa), (b, xb)) = (rng.input(wide), rng.input(wide));
            let results = [
                check("add", a + b, Some(&(xa.clone() + xb.clone()))),
                check("sub", a - b, Some(&(xa.clone() - xb.clone()))),
                check("mul", a * b, Some(&(xa.clone() * xb.clone()))),
                check("div", a / b, quotient(&xa, &xb).as_ref()),
                // Exact: the operand's errors, which may be zero.
                holds("neg", -a, Some(&-xa.clone())),
                holds("abs", a.abs(), Some(&qabs(xa.clone()))),
                check_sqrt(a.abs().sqrt(), &qabs(xa.clone())),
                check_sqrt(a.sqrt(), &xa),
            ];
            // Commutative, bit for bit.
            assert_eq!(bits(a + b), bits(b + a), "add: {a:?}, {b:?}");
            assert_eq!(bits(a * b), bits(b * a), "mul: {a:?}, {b:?}");
            let w = usize::from(wide);
            total[w] += results.len() as u32;
            certified[w] += results.iter().filter(|&&c| c).count() as u32;
        }
    }
    // Not vacuous: moderate inputs certify all but the uncertain divisors
    // and roots; the wide ones still certify most.
    assert!(
        certified[0] * 10 > total[0] * 8,
        "{certified:?} of {total:?}"
    );
    assert!(
        certified[1] * 10 > total[1] * 5,
        "{certified:?} of {total:?}"
    );
}

/// The depth of [`chain`]'s expressions.
const CHAIN_DEPTH: u32 = 4;

/// A random expression of `+ − × ÷` over inputs (an operation at the top),
/// with its exact value (`None` once a true divisor is zero).
fn chain(rng: &mut Rng, depth: u32, wide: bool) -> (Bounded, Option<Q>) {
    if depth < CHAIN_DEPTH && (depth == 0 || rng.below(4) == 0) {
        let (b, x) = rng.input(wide);
        return (b, Some(x));
    }
    let (a, xa) = chain(rng, depth - 1, wide);
    let (b, xb) = chain(rng, depth - 1, wide);
    let both = xa.zip(xb);
    let r = match rng.below(4) {
        0 => (a + b, both.map(|(x, y)| x + y)),
        1 => (a - b, both.map(|(x, y)| x - y)),
        2 => (a * b, both.map(|(x, y)| x * y)),
        _ => (a / b, both.and_then(|(x, y)| quotient(&x, &y))),
    };
    check("chain", r.0, r.1.as_ref());
    r
}

#[test]
fn chains_hold_their_bounds() {
    let mut certified = 0;
    let mut total = 0;
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for case in 0..300 {
            let (r, x) = chain(&mut rng, CHAIN_DEPTH, case % 3 == 0);
            total += 1;
            if check("chain", r, x.as_ref()) {
                certified += 1;
            }
        }
    }
    assert!(certified * 2 > total, "{certified} of {total}");
}

#[test]
fn underflow_is_covered_by_the_floor() {
    let a = Bounded::exact(1e-170);
    let p = a * a;
    assert_eq!(p.v, 0.0);
    assert!(check("1e-170²", p, Some(&(q(1e-170) * q(1e-170)))));
    assert!(p.err_round >= ETA);
    let tiny = Bounded::exact(ETA);
    let half = tiny * Bounded::exact(0.5);
    assert!(check("η·½", half, Some(&(q(ETA) * q(0.5)))));
    let third = tiny / Bounded::exact(3.0);
    assert!(check("η/3", third, Some(&(q(ETA) / q(3.0)))));
    let sum = tiny + tiny;
    assert!(check("η + η", sum, Some(&q(2.0 * ETA))));
}

#[test]
fn overflow_is_uncertified() {
    let big = Bounded::exact(f64::MAX);
    assert!(!(big + big).is_certified());
    assert!(!(big * Bounded::exact(2.0)).is_certified());
    assert!(!(big / Bounded::exact(0.5)).is_certified());
    assert!(!Bounded::exact(f64::INFINITY).is_certified());
    assert!(!Bounded::exact(f64::NAN).is_certified());
    // ∞ · 0 in an error term is ∞, not a NaN.
    let r = Bounded::exact(0.0) * Bounded::new(1.0, f64::INFINITY, 0.0);
    assert_eq!(r.err_trunc, f64::INFINITY);
    let r = Bounded::exact(0.0) / Bounded::exact(0.0);
    assert!(!r.is_certified() && r.sign().is_none());
}

#[test]
fn up_and_down_step_past_the_neighbours() {
    for seed in varde_testing::seeds(4, 64) {
        let mut rng = Rng::new(seed);
        for _ in 0..10_000 {
            let t = rng.wide().abs();
            let u = up(t);
            assert!(u >= t.next_up() && u >= ETA, "up({t:e}) = {u:e}");
            let d = down(t);
            assert!(d <= t.next_down(), "down({t:e}) = {d:e}");
            let d = down(-t);
            assert!(d <= (-t).next_down(), "down({:e}) = {d:e}", -t);
        }
    }
    assert_eq!(up(0.0), ETA);
    assert_eq!(up(f64::MAX), f64::INFINITY);
    assert_eq!(up(f64::NAN), f64::INFINITY);
    assert_eq!(down(f64::NAN), f64::NEG_INFINITY);
    // An overflowed result was rounded from something above the top float.
    assert!(down(f64::INFINITY) <= f64::MAX);
}

#[test]
fn rounding_and_truncation_stay_apart() {
    let a = Bounded::exact(1.0 / 3.0);
    let b = Bounded::exact(0.7);
    for r in [a + b, a - b, a * b, a / b, a.sqrt()] {
        // Only floors of η: no truncation invented.
        assert!(r.err_trunc < 1e-320, "{r:?}");
        assert!(r.rounding_dominates(), "{r:?}");
    }
    let a = a.with_trunc(1e-6);
    for r in [a + b, a - b, a * b, a / b, a.sqrt()] {
        assert!(r.err_trunc >= 1e-7, "{r:?}");
        assert!(!r.rounding_dominates(), "{r:?}");
    }
    let (x, y) = (
        Bounded::new(3.0, 1e-9, 1e-12),
        Bounded::new(-0.1, 1e-11, 1e-14),
    );
    assert_eq!(x * y, y * x);
}

#[test]
fn exact_values_stay_exact() {
    for v in [0.0, -0.0, ETA, -1.5, f64::MAX] {
        let x = Bounded::exact(v);
        assert_eq!(x.err(), 0.0, "{x:?}");
        assert_eq!((-x).err(), 0.0, "{x:?}");
        assert_eq!(x.abs().err(), 0.0, "{x:?}");
    }
    assert_eq!(Bounded::new(1.0, 0.0, 0.25).err(), 0.25);
    assert_eq!(Bounded::exact(ETA).sign(), Some(Ordering::Greater));
    // The root of an exact zero is certified.
    let r = Bounded::exact(0.0).sqrt();
    assert!(check_sqrt(r, &q(0.0)), "{r:?}");
}

#[test]
fn sign_needs_the_value_past_the_error() {
    assert_eq!(Bounded::exact(2.0).sign(), Some(Ordering::Greater));
    assert_eq!(Bounded::exact(-2.0).sign(), Some(Ordering::Less));
    assert_eq!(Bounded::exact(0.0).sign(), None);
    assert_eq!(Bounded::new(1.0, 0.5, 0.5).sign(), None);
    assert_eq!(Bounded::new(-1.0, 0.25, 0.5).sign(), Some(Ordering::Less));
}

#[test]
fn gamma_bounds_n_roundings() {
    let u = q(U);
    for n in (0u32..2000).chain([1 << 20, 1 << 30, u32::MAX]) {
        let nu = q(f64::from(n)) * u.clone();
        let exact = nu.clone() / (q(1.0) - nu);
        assert!(q(gamma(n, U)) >= exact, "γ_{n}");
    }
    assert_eq!(gamma(u32::MAX, 0.5), f64::INFINITY);
    assert_eq!(gamma(2, 0.5), f64::INFINITY);
}

fn de_casteljau(b: &[f64], s: f64, t: f64) -> f64 {
    let mut c = b.to_vec();
    for k in 1..c.len() {
        for i in 0..c.len() - k {
            c[i] = s * c[i] + t * c[i + 1];
        }
    }
    c[0]
}

/// `Σ bᵢ·C(n, i)·sⁿ⁻ⁱ·tⁱ` exactly.
fn bernstein(b: &[f64], s: f64, t: f64) -> Q {
    let n = b.len() - 1;
    let mut sum = q(0.0);
    let mut binom = 1.0;
    for (i, &bi) in b.iter().enumerate() {
        let mut term = q(bi) * q(binom);
        for _ in 0..n - i {
            term *= q(s);
        }
        for _ in 0..i {
            term *= q(t);
        }
        sum += term;
        binom = binom * (n - i) as f64 / (i + 1) as f64;
    }
    sum
}

#[test]
fn gamma_bound_covers_de_casteljau() {
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for case in 0..400 {
            let n = 1 + rng.below(8) as usize;
            // Moderate, anywhere, or a few `η` (every product underflows:
            // the bound is its absolute part alone).
            let b: Vec<f64> = (0..=n)
                .map(|_| match case % 3 {
                    0 => rng.typical(),
                    1 => rng.wide(),
                    _ => {
                        let m = ETA * rng.below(64) as f64;
                        rng.sign(m)
                    }
                })
                .collect();
            let t = match if case % 3 == 2 { 0 } else { rng.below(4) } {
                0 => rng.below(1 << 20) as f64 / (1 << 20) as f64,
                1 => rng.magnitude(0, 1022),
                2 => 1.0 - rng.magnitude(0, 1022),
                _ => 0.5,
            };
            let s = 1.0 - t;
            let p = de_casteljau(&b, s, t);
            let abs: Vec<f64> = b.iter().map(|x| x.abs()).collect();
            let bound = gamma_bound(2 * n as u32, de_casteljau(&abs, s, t), U);
            if !p.is_finite() || !bound.is_finite() {
                continue;
            }
            let dist = qabs(bernstein(&b, s, t) - q(p));
            assert!(dist <= q(bound), "degree {n} at t = {t:e}: {b:?}");
        }
    }
}

/// A divisor whose error nearly reaches its value, `eb = |vb|·(1 − 2⁻ᵏ)`
/// (split between the parts), with the true divisor at the end nearest
/// zero: the divisor's error term dominates the quotient's bound, so a
/// quotient bound `q` a fraction of an ulp short shows. Subnormal
/// quotients make that fraction large.
#[test]
fn near_zero_divisors_hold_their_bounds() {
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for case in 0..2000 {
            let vb = match case % 3 {
                0 => rng.typical(),
                1 => {
                    let m = rng.magnitude(1023, 1023) * 3.0;
                    rng.sign(m)
                }
                _ => rng.wide(),
            };
            let k = 1 + rng.below(60) as i32;
            let eb = vb.abs() * (1.0 - pow2(-k));
            let tb = match rng.below(3) {
                0 => 0.0,
                1 => eb,
                _ => eb / 2.0,
            };
            let rb = eb - tb;
            let b = Bounded::new(vb, tb, rb);
            let e = q(tb) + q(rb);
            let xb = if vb > 0.0 { q(vb) - e } else { q(vb) + e };
            let va = match case % 4 {
                // A subnormal quotient.
                0 => {
                    let m = rng.magnitude(0, 0) * if vb.abs() > 1.0 { vb.abs() } else { 1.0 };
                    rng.sign(m)
                }
                1 => {
                    let m = rng.magnitude(0, 1);
                    rng.sign(m)
                }
                2 => rng.typical(),
                _ => rng.wide(),
            };
            let ea = if rng.below(2) == 0 {
                0.0
            } else {
                rng.error(va, false)
            };
            let a = Bounded::new(va, 0.0, ea);
            for xa in [q(va), q(va) + q(ea), q(va) - q(ea)] {
                check("div", a / b, quotient(&xa, &xb).as_ref());
            }
        }
    }
}

/// Values and error parts at the edges: zeros of both signs, `η`, the
/// subnormal/normal boundary, powers of two and their neighbours (where
/// the ulp changes), the top of the range; errors of zero, `η`, about the
/// value (sign and roots undecided or barely decided), infinite and NaN.
/// Every input through the unary operations, pairs of them (drawn) through
/// the binary ones, the true values at the ends.
#[test]
fn edge_values_hold_their_bounds() {
    let mut values = vec![0.0, -0.0, ETA, f64::MIN_POSITIVE, f64::MAX];
    for x in [f64::MIN_POSITIVE, 1.0, pow2(-60), pow2(600)] {
        values.extend([x.next_down(), x.next_up()]);
    }
    values.extend([1.0, 3.0, f64::MAX.next_down()]);
    let values: Vec<f64> = values.iter().flat_map(|&v| [v, -v]).collect();
    let mut inputs = Vec::new();
    for &v in &values {
        let a = v.abs();
        for e in [
            0.0,
            ETA,
            a,
            if a > 0.0 { a.next_down() } else { 0.0 },
            a.next_up(),
            a * U,
            f64::INFINITY,
            f64::NAN,
        ] {
            for (t, r) in [(0.0, e), (e, 0.0), (e, ETA)] {
                inputs.push(Bounded::new(v, t, r));
            }
        }
    }
    // The true values at both ends, `None` unless the bound is finite.
    let ends = |b: Bounded| -> Vec<Option<Q>> {
        if b.err_trunc.is_finite() && b.err_round.is_finite() {
            let e = q(b.err_trunc) + q(b.err_round);
            vec![Some(q(b.v) - e.clone()), Some(q(b.v) + e)]
        } else {
            vec![None]
        }
    };
    let check_or_uncertified = |what: &str, r: Bounded, x: Option<Option<Q>>| match x {
        // Exact operations: their errors may be zero.
        Some(x) if what == "neg" || what == "abs" => {
            holds(what, r, x.as_ref());
        }
        Some(x) => {
            check(what, r, x.as_ref());
        }
        None => assert!(
            !r.is_certified(),
            "{what}: certified an unbounded input {r:?}"
        ),
    };
    for &a in &inputs {
        for xa in ends(a) {
            check_or_uncertified("neg", -a, xa.clone().map(|x| Some(-x)));
            check_or_uncertified("abs", a.abs(), xa.clone().map(|x| Some(qabs(x))));
            if let Some(x) = &xa {
                check_sqrt(a.sqrt(), x);
                check_sqrt(a.abs().sqrt(), &qabs(x.clone()));
            } else {
                assert!(!a.sqrt().is_certified(), "sqrt: {a:?}");
            }
        }
    }
    let n = inputs.len() as u64;
    for seed in varde_testing::seeds(1, 48) {
        let mut rng = Rng::new(seed);
        for _ in 0..1000 {
            let a = inputs[rng.below(n) as usize];
            let b = inputs[rng.below(n) as usize];
            for xa in ends(a) {
                for xb in ends(b) {
                    let both = xa.clone().zip(xb.clone());
                    check_or_uncertified("add", a + b, both.clone().map(|(x, y)| Some(x + y)));
                    check_or_uncertified("sub", a - b, both.clone().map(|(x, y)| Some(x - y)));
                    check_or_uncertified("mul", a * b, both.clone().map(|(x, y)| Some(x * y)));
                    check_or_uncertified("div", a / b, both.map(|(x, y)| quotient(&x, &y)));
                }
            }
        }
    }
}

/// A value built by hand that isn't finite is uncertified whatever its
/// errors say, and so is everything computed from it.
#[test]
fn hand_built_non_finite_values_are_uncertified() {
    let one = Bounded::exact(1.0);
    for v in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let x = Bounded::new(v, 0.0, 0.0);
        assert!(!x.is_certified());
        assert!(x.sign().is_none());
        for r in [-x, x.abs(), x + one, x * one, one / x, x / one, x.sqrt()] {
            assert!(!r.is_certified(), "{r:?}");
        }
    }
}

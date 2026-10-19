//! Double-double against exact rational arithmetic: the algorithms within
//! their proven bounds, and every certified [`Bounded<Dd>`] result holding
//! the exact result within its error, over the whole `f64` range
//! (subnormal high and low parts, low parts far below the high ones,
//! values near overflow) with any error parts. Plus promotion, and the
//! same bits with and without hardware FMA.

use std::cmp::Ordering;

use malachite::Rational as Q;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::*;
use crate::num::bounded::U;

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

fn qd(x: Dd) -> Q {
    q(x.hi) + q(x.lo)
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

    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    fn sign(&mut self, x: f64) -> f64 {
        if self.below(2) == 0 { x } else { -x }
    }

    /// A positive float with a random mantissa and an exponent field in
    /// `lo..=hi` (0 for subnormals, 2046 the top binade).
    fn magnitude(&mut self, lo: u64, hi: u64) -> f64 {
        let exp = lo + self.below(hi - lo + 1);
        f64::from_bits(exp << 52 | self.0.next_u64() >> 12)
    }

    /// A high part anywhere in the range, the edges more often than chance.
    fn wide(&mut self) -> f64 {
        const EDGES: [f64; 9] = [
            0.0,
            ETA,
            f64::MIN_POSITIVE,
            1.0,
            1e-300,
            1e300,
            RANGE,
            f64::MAX,
            4.5e15,
        ];
        let x = match self.below(8) {
            0 => EDGES[self.below(EDGES.len() as u64) as usize],
            1 => self.magnitude(0, 1),
            2 => self.magnitude(1023 + 1015, 2046),
            _ => self.magnitude(0, 2046),
        };
        self.sign(x)
    }

    /// A high part of moderate magnitude, `2^±40`.
    fn typical(&mut self) -> f64 {
        let x = self.magnitude(1023 - 40, 1023 + 40);
        self.sign(x)
    }

    /// A double-double with high part about `hi`: no low part, one just
    /// below `hi`'s last bit, one far below it (dropped before products),
    /// or one in the subnormal range.
    fn dd(&mut self, hi: f64) -> Dd {
        let rel = match self.below(5) {
            0 => return Dd::from_f64(hi),
            1 | 2 => self.magnitude(1023 - 60, 1023 - 53),
            3 => self.magnitude(1023 - 1000, 1023 - 60),
            _ => self.magnitude(0, 2046),
        };
        let lo = self.sign(hi * rel);
        let x = Dd::sum(hi, lo);
        // Past the top of the range: no low part.
        if x.is_finite() { x } else { Dd::from_f64(hi) }
    }

    /// An error part for a value of magnitude `v`: zero, relative to `v`,
    /// tiny or wide.
    fn error(&mut self, v: f64, wide: bool) -> f64 {
        match self.below(if wide { 6 } else { 4 }) {
            0 => 0.0,
            1 => v.abs() * self.magnitude(1023 - 110, 1023 - 90),
            2 | 3 => v.abs() * self.magnitude(1023 - 60, 1023 - 20),
            4 => self.magnitude(0, 2),
            _ => self.wide().abs(),
        }
    }

    /// An input and a true value inside its bounds: an end, the middle or
    /// a point between.
    fn input(&mut self, wide: bool) -> (Bounded<Dd>, Q) {
        let hi = if wide { self.wide() } else { self.typical() };
        let v = self.dd(hi);
        let b = Bounded::new(v, self.error(hi, wide), self.error(hi, wide));
        let e = q(b.err_trunc) + q(b.err_round);
        let lambda = match self.below(4) {
            0 => q(-1.0),
            1 => q(1.0),
            2 => q(0.0),
            _ => q((self.below(2049) as f64 - 1024.0) / 1024.0),
        };
        (b, qd(v) + lambda * e)
    }
}

/// Whether `x` is a double-double in its form: `hi = fl(hi + lo)`.
fn in_form(x: Dd) -> bool {
    !x.is_finite() || qd(Dd::from_f64(x.hi + x.lo)) == q(x.hi)
}

/// Checks a computed result: in form, its error terms at least `η`, and
/// it holds its exact value (see [`holds`]).
fn check(what: &str, r: Bounded<Dd>, exact: Option<&Q>) -> bool {
    assert!(
        r.err_trunc >= ETA && r.err_round >= ETA,
        "{what}: an error term below η: {r:?}"
    );
    holds(what, r, exact)
}

/// Checks a result against its exact value (`None` where the operation is
/// undefined at the true operands); returns whether it was certified.
fn holds(what: &str, r: Bounded<Dd>, exact: Option<&Q>) -> bool {
    assert!(in_form(r.v), "{what}: not a double-double {r:?}");
    if !r.is_certified() {
        return false;
    }
    let x = exact.unwrap_or_else(|| panic!("{what}: certified an undefined value {r:?}"));
    let dist = qabs(x.clone() - qd(r.v));
    assert!(
        dist <= q(r.err_trunc) + q(r.err_round),
        "{what}: bound violated by {:e}: {r:?}",
        rat_f64(&dist)
    );
    assert!(dist <= q(r.err()), "{what}: err() below the parts: {r:?}");
    if let Some(s) = r.sign() {
        assert_eq!(x.cmp(&q(0.0)), s, "{what}: wrong certified sign {r:?}");
    }
    assert!(
        r.abs_bound() == f64::INFINITY || q(r.abs_bound()) >= qabs(x.clone()),
        "{what}: abs_bound below the value {r:?}"
    );
    true
}

/// A rational's rough `f64` value, for messages and the worst-case
/// report only.
fn rat_f64(x: &Q) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The square root against `x`: `(v − e)² ≤ x ≤ (v + e)²`, the lower end
/// only when `v − e ≥ 0`.
fn check_sqrt(r: Bounded<Dd>, x: &Q) -> bool {
    assert!(r.err_trunc >= ETA && r.err_round >= ETA, "sqrt: {r:?}");
    assert!(in_form(r.v), "sqrt: not a double-double {r:?}");
    if !r.is_certified() {
        return false;
    }
    assert!(*x >= q(0.0), "sqrt: certified the root of a negative {r:?}");
    let e = q(r.err_trunc) + q(r.err_round);
    let (lo, hi) = (qd(r.v) - e.clone(), qd(r.v) + e);
    assert!(*x <= hi.clone() * hi, "sqrt: root above the bound {r:?}");
    if lo >= q(0.0) {
        assert!(lo.clone() * lo <= *x, "sqrt: root below the bound {r:?}");
    }
    true
}

fn bits(r: Bounded<Dd>) -> [u64; 4] {
    [
        r.v.hi.to_bits(),
        r.v.lo.to_bits(),
        r.err_trunc.to_bits(),
        r.err_round.to_bits(),
    ]
}

fn quotient(a: &Q, b: &Q) -> Option<Q> {
    (*b != q(0.0)).then(|| a.clone() / b.clone())
}

/// The raw algorithms on exact operands, against the proven relative
/// bounds of the paper (plus, for products and quotients, the dropped low
/// parts' `2⁻⁸⁹⁸` and the scaling's `η/2`): this checks they are the
/// paper's algorithms, not just that the certified bounds hold.
#[test]
fn algorithms_meet_their_proven_bounds() {
    let u = q(U);
    let u2 = u.clone() * u.clone();
    let u3 = u2.clone() * u.clone();
    let add_bound = q(3.0) * u2.clone() + q(13.0) * u3.clone();
    let mul_bound = q(7.0) * u2.clone() + q(pow2(-898));
    let div_bound = q(15.0) * u2 + q(56.0) * u3 + q(pow2(-898));
    let abs = q(ETA) / q(2.0);
    let mut worst = [0f64; 3];
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for case in 0..3000 {
            let (x, y) = if case % 2 == 0 {
                (rng.typical(), rng.typical())
            } else {
                (rng.wide(), rng.wide())
            };
            let (x, y) = (rng.dd(x), rng.dd(y));
            let (qx, qy) = (qd(x), qd(y));
            let results = [
                (add(x, y), qx.clone() + qy.clone(), &add_bound, q(0.0)),
                (mul(x, y), qx.clone() * qy.clone(), &mul_bound, abs.clone()),
            ];
            let in_range = x.hi.abs() < RANGE && y.hi.abs() < RANGE;
            for (i, (z, exact, rel, abs)) in results.into_iter().enumerate() {
                assert!(in_form(z), "{i}: {x:?}, {y:?} gives {z:?}");
                if !z.is_finite() || (i == 0 && !in_range) {
                    continue;
                }
                let err = qabs(qd(z) - exact.clone());
                let allowed = rel.clone() * qabs(exact.clone()) + abs;
                assert!(err <= allowed, "{i}: {x:?}, {y:?} gives {z:?}");
                if exact != q(0.0) && case % 2 == 0 {
                    let r = rat_f64(&(err / qabs(exact))) / (U * U);
                    worst[i] = if r > worst[i] { r } else { worst[i] };
                }
            }
            if y.hi != 0.0 {
                let z = div(x, y);
                assert!(in_form(z), "div: {x:?}, {y:?} gives {z:?}");
                if z.is_finite() {
                    let exact = qx / qy;
                    let err = qabs(qd(z) - exact.clone());
                    let allowed = div_bound.clone() * qabs(exact.clone()) + abs.clone();
                    assert!(err <= allowed, "div: {x:?}, {y:?} gives {z:?}");
                    if exact != q(0.0) && case % 2 == 0 {
                        let r = rat_f64(&(err / qabs(exact))) / (U * U);
                        worst[2] = if r > worst[2] { r } else { worst[2] };
                    }
                }
            }
        }
    }
    // Not vacuous: errors of the order of u², as the paper reports.
    assert!(worst.iter().all(|&w| w > 0.1), "worst / u²: {worst:?}");
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
            // Commutative, bit for bit (an overflowing sum only in being
            // uncertified).
            if (a + b).is_certified() {
                assert_eq!(bits(a + b), bits(b + a), "add: {a:?}, {b:?}");
            } else {
                assert!(!(b + a).is_certified(), "add: {a:?}, {b:?}");
            }
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

/// A random expression of `+ − × ÷ √` over inputs (an operation at the
/// top), with its exact value (`None` once a true divisor is zero; the
/// root's operand is squared, so its exact root is known).
fn chain(rng: &mut Rng, depth: u32, wide: bool) -> (Bounded<Dd>, Option<Q>) {
    if depth < CHAIN_DEPTH && (depth == 0 || rng.below(4) == 0) {
        let (b, x) = rng.input(wide);
        return (b, Some(x));
    }
    let (a, xa) = chain(rng, depth - 1, wide);
    let (b, xb) = chain(rng, depth - 1, wide);
    let both = xa.clone().zip(xb);
    let r = match rng.below(5) {
        0 => (a + b, both.map(|(x, y)| x + y)),
        1 => (a - b, both.map(|(x, y)| x - y)),
        2 => (a * b, both.map(|(x, y)| x * y)),
        3 => (a / b, both.and_then(|(x, y)| quotient(&x, &y))),
        _ => ((a * a).sqrt(), xa.map(qabs)),
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

/// Promotion keeps the value and both error parts, bit for bit; on exact
/// moderate operands the second level's rounding error is about `2⁻¹⁰⁰`
/// of the result where the first level's is about `2⁻⁵³`.
#[test]
fn promotion_is_exact_and_the_second_level_is_tighter() {
    for seed in varde_testing::seeds(1, 64) {
        let mut rng = Rng::new(seed);
        for _ in 0..1000 {
            let (va, vb) = (rng.typical(), rng.typical());
            let a = Bounded::new(va, rng.error(va, true), rng.error(va, true));
            let p = a.promote();
            assert_eq!(
                (p.v, p.err_trunc, p.err_round),
                (Dd::from_f64(va), a.err_trunc, a.err_round)
            );
            assert_eq!(Bounded::<Dd>::from(a), p);
            assert_eq!(a.is_certified(), p.is_certified());
            let (a, b) = (Bounded::exact(va), Bounded::exact(vb));
            let (pa, pb) = (a.promote(), b.promote());
            let ops = [
                (a + b, pa + pb),
                (a * b, pa * pb),
                (a / b, pa / pb),
                (a.abs().sqrt(), pa.abs().sqrt()),
            ];
            for (l0, l1) in ops {
                // Only floors of η, divided by at least `2⁻⁴¹`.
                assert!(l1.err_trunc < 1e-300, "{l1:?}");
                assert!(l1.rounding_dominates(), "{l1:?}");
                let mag = l1.v.hi.abs();
                assert!(l1.err_round <= mag * pow2(-99) + 1e-300, "{l1:?}");
                assert!(l1.err_round * pow2(40) < l0.err_round, "{l0:?} vs {l1:?}");
            }
        }
    }
}

/// Values and error parts at the edges: zeros of both signs, `η`, the
/// subnormal/normal boundary, powers of two and their neighbours, the
/// certified range of sums and the top of the range, each with low parts
/// of each kind; errors of zero, `η`, about the value, infinite and NaN.
/// Every input through the unary operations, pairs of them (drawn) through
/// the binary ones, the true values at the ends.
#[test]
fn edge_values_hold_their_bounds() {
    let mut highs = vec![0.0, -0.0, ETA, f64::MIN_POSITIVE, f64::MAX, RANGE];
    for x in [f64::MIN_POSITIVE, 1.0, pow2(-60), pow2(600), RANGE] {
        highs.extend([x.next_down(), x.next_up()]);
    }
    highs.extend([1.0, 3.0, f64::MAX.next_down(), pow2(-1000), pow2(-970)]);
    let mut values = Vec::new();
    for &h in &highs {
        values.push(Dd::from_f64(h));
        for rel in [pow2(-54), -pow2(-60), pow2(-200), -pow2(-950)] {
            let lo = h * rel;
            if lo != 0.0 && lo.is_finite() {
                values.push(Dd::sum(h, lo));
            }
        }
        // The largest low part below half an ulp.
        if h.is_finite() && h != 0.0 {
            let half = (h.next_up() - h) * 0.5;
            if half.next_down() > 0.0 {
                values.push(Dd::sum(h, half.next_down()));
            }
        }
    }
    let values: Vec<Dd> = values
        .iter()
        .filter(|v| v.is_finite())
        .flat_map(|&v| [v, -v])
        .collect();
    let mut inputs = Vec::new();
    for &v in &values {
        let a = v.hi.abs();
        for e in [0.0, ETA, a, a.next_up(), a * U * U, f64::INFINITY, f64::NAN] {
            for (t, r) in [(0.0, e), (e, 0.0), (e, ETA)] {
                inputs.push(Bounded::new(v, t, r));
            }
        }
    }
    let ends = |b: Bounded<Dd>| -> Vec<Option<Q>> {
        if b.err_trunc.is_finite() && b.err_round.is_finite() {
            let e = q(b.err_trunc) + q(b.err_round);
            vec![Some(qd(b.v) - e.clone()), Some(qd(b.v) + e)]
        } else {
            vec![None]
        }
    };
    let check_or_uncertified = |what: &str, r: Bounded<Dd>, x: Option<Option<Q>>| match x {
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
        for _ in 0..1500 {
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

#[test]
fn underflow_is_covered() {
    let a = Bounded::<Dd>::exact(Dd::from_f64(1e-170));
    let p = a * a;
    assert!(check("1e-170²", p, Some(&(q(1e-170) * q(1e-170)))));
    // A product landing in the subnormal range with a low part.
    let x = Dd::sum(pow2(-500), pow2(-560));
    let y = Dd::sum(-pow2(-540), pow2(-600) * 3.0);
    let p = Bounded::<Dd>::exact(x) * Bounded::exact(y);
    assert!(check("subnormal product", p, Some(&(qd(x) * qd(y)))));
    let d = Bounded::<Dd>::exact(x) / Bounded::exact(Dd::from_f64(pow2(560)));
    assert!(check(
        "subnormal quotient",
        d,
        Some(&(qd(x) / q(pow2(560))))
    ));
    let tiny = Bounded::<Dd>::exact(Dd::from_f64(ETA));
    assert!(check("η + η", tiny + tiny, Some(&q(2.0 * ETA))));
    assert!(check(
        "η/3",
        tiny / Bounded::exact(Dd::from_f64(3.0)),
        Some(&(q(ETA) / q(3.0)))
    ));
    // Exact sums down to η stay exact: only the floor η is added.
    let s =
        Bounded::<Dd>::exact(Dd::from_f64(f64::MIN_POSITIVE)) - Bounded::exact(Dd::from_f64(ETA));
    assert!(s.err_round <= 8.0 * ETA, "{s:?}");
}

#[test]
fn overflow_is_uncertified() {
    let big = Bounded::<Dd>::exact(Dd::from_f64(f64::MAX));
    let two = Bounded::<Dd>::exact(Dd::from_f64(2.0));
    assert!(!(big + big).is_certified());
    assert!(!(big * two).is_certified());
    assert!(!(big / Bounded::exact(Dd::from_f64(0.5))).is_certified());
    // Products and quotients scale, so they are certified up to overflow.
    let r = big * Bounded::exact(Dd::from_f64(0.25));
    assert!(check("MAX·¼", r, Some(&(q(f64::MAX) * q(0.25)))));
    assert!(!Bounded::<Dd>::exact(Dd::from_f64(f64::INFINITY)).is_certified());
    assert!(!Bounded::<Dd>::exact(Dd::from_f64(f64::NAN)).is_certified());
    let zero = Bounded::<Dd>::exact(Dd::ZERO);
    let r = zero / zero;
    assert!(!r.is_certified() && r.sign().is_none());
    let r = zero * Bounded::new(Dd::from_f64(1.0), f64::INFINITY, 0.0);
    assert_eq!(r.err_trunc, f64::INFINITY);
}

/// Roots near the top of the range are certified, as at the first level:
/// those of values from `2¹⁰⁰⁰` are taken of the value scaled down.
#[test]
fn roots_near_the_top_are_certified() {
    let mut rng = Rng::new(0x7007);
    let mut xs = vec![pow2(1000).next_down(), pow2(1000), RANGE, f64::MAX];
    xs.extend((0..200).map(|_| rng.magnitude(1023 + 990, 2046)));
    for x in xs {
        let v = rng.dd(x);
        for err in [0.0, x * pow2(-80)] {
            let b = Bounded::new(v, err, err);
            let r = b.sqrt();
            assert!(r.is_certified(), "{b:?} gives {r:?}");
            for lambda in [-1.0, 0.0, 1.0] {
                let exact = qd(v) + q(lambda) * (q(b.err_trunc) + q(b.err_round));
                check_sqrt(r, &exact);
            }
            if err == 0.0 {
                assert!(r.err_round <= r.v.hi * pow2(-100), "{b:?} gives {r:?}");
            }
        }
        assert!(Bounded::exact(x).promote().sqrt().is_certified());
    }
}

/// A value built by hand that isn't finite is uncertified whatever its
/// errors say, and so is everything computed from it.
#[test]
fn hand_built_non_finite_values_are_uncertified() {
    let one = Bounded::<Dd>::exact(Dd::from_f64(1.0));
    for v in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let x = Bounded::new(Dd::from_f64(v), 0.0, 0.0);
        assert!(!x.is_certified());
        assert!(x.sign().is_none());
        for r in [-x, x.abs(), x + one, x * one, one / x, x / one, x.sqrt()] {
            assert!(!r.is_certified(), "{r:?}");
        }
    }
}

#[test]
fn sign_needs_the_value_past_the_error() {
    let dd = |x: f64| Dd::from_f64(x);
    assert_eq!(
        Bounded::<Dd>::exact(dd(2.0)).sign(),
        Some(Ordering::Greater)
    );
    assert_eq!(Bounded::<Dd>::exact(dd(-ETA)).sign(), Some(Ordering::Less));
    assert_eq!(Bounded::<Dd>::exact(Dd::ZERO).sign(), None);
    assert_eq!(Bounded::new(dd(1.0), 0.5, 0.5).sign(), None);
    assert_eq!(
        Bounded::new(dd(-1.0), 0.25, 0.5).sign(),
        Some(Ordering::Less)
    );
    // The root of an exact zero is certified, and exact.
    let r = Bounded::<Dd>::exact(Dd::ZERO).sqrt();
    assert!(check_sqrt(r, &q(0.0)), "{r:?}");
    assert!(r.err() <= 8.0 * ETA, "{r:?}");
}

/// FNV-1a over `u64`s.
struct Hash(u64);

impl Hash {
    fn add(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
}

/// The hash of every operation's bits, at both levels, over a fixed input
/// set (seeded, so independent of `VARDE_TESTS`). The same bits on every
/// platform, with or without hardware FMA (CI runs this crate's tests
/// with `-C target-feature=+fma` too): a change of algorithm changes the
/// hash, which then needs updating here.
#[test]
fn results_are_the_same_bits_everywhere() {
    let mut h = Hash(0xCBF2_9CE4_8422_2325);
    let mut rng = Rng::new(0x5EED);
    for case in 0..4000 {
        let wide = case % 2 == 0;
        let ((a, _), (b, _)) = (rng.input(wide), rng.input(wide));
        for r in [a + b, a - b, a * b, a / b, a.abs().sqrt(), a.sqrt()] {
            for x in bits(r) {
                h.add(x);
            }
        }
        let (a, b) = (
            Bounded::new(a.v.hi, a.err_trunc, a.err_round),
            Bounded::new(b.v.hi, b.err_trunc, b.err_round),
        );
        for r in [a + b, a - b, a * b, a / b, a.abs().sqrt(), a.sqrt()] {
            for x in [r.v, r.err_trunc, r.err_round] {
                h.add(x.to_bits());
            }
        }
    }
    println!("hash of the results: {:#018x}", h.0);
    assert_eq!(
        h.0, 0xf904_86d1_41d2_8157,
        "results differ from the reference bits"
    );
}

/// Roots of values near the bottom of the range are as tight as at the
/// first level, so promotion loses neither certification nor the sign:
/// the products' absolute rounding term would otherwise swamp the
/// residual (`√η` came out within `5·√η`).
#[test]
fn roots_near_the_bottom_are_tight() {
    let mut rng = Rng::new(0xB077);
    let mut xs = vec![
        ETA,
        3.0 * ETA,
        f64::MIN_POSITIVE,
        f64::MIN_POSITIVE.next_down(),
    ];
    xs.extend([pow2(-1000), pow2(-960), pow2(-900).next_down(), pow2(-900)]);
    xs.extend((0..200).map(|_| rng.magnitude(0, 130)));
    for x in xs {
        let l0 = Bounded::exact(x).sqrt();
        let l1 = Bounded::exact(x).promote().sqrt();
        assert_eq!(l1.sign(), Some(Ordering::Greater), "√{x:e}: {l1:?}");
        assert!(l1.err() <= l0.err(), "√{x:e}: {l0:?} vs {l1:?}");
        assert!(check_sqrt(l1, &q(x)), "√{x:e}: {l1:?}");
        // Its low part may be the larger: a positive value of any size.
        let v = Bounded::exact(rng.dd(x)).abs().v;
        let x = v.hi;
        for err in [0.0, x * pow2(-80), x * 0.25] {
            let b = Bounded::new(v, err, err);
            let r = b.sqrt();
            assert!(r.is_certified() || err * 2.0 >= x, "{b:?} gives {r:?}");
            for lambda in [-1.0, 0.0, 1.0] {
                check_sqrt(r, &(qd(v) + q(lambda) * (q(b.err_trunc) + q(b.err_round))));
            }
            if err == 0.0 {
                assert!(r.err_round <= r.v.hi * pow2(-100), "{b:?} gives {r:?}");
            }
        }
    }
}

/// Sums are certified up to overflow, as at the first level (from
/// `RANGE` they are taken of the operands scaled down): promotion keeps a
/// certified sum certified, with its sign.
#[test]
fn sums_near_the_top_are_certified() {
    let max = f64::MAX;
    let pairs = [
        (Dd::from_f64(RANGE), Dd::from_f64(2.0)),
        (Dd::from_f64(RANGE.next_down()), Dd::from_f64(RANGE)),
        (Dd::from_f64(max), Dd::from_f64(-max)),
        (Dd::from_f64(max), Dd::from_f64(-max.next_down())),
        (Dd::from_f64(max * 0.75), Dd::from_f64(max * 0.25)),
        (Dd::from_f64(max), Dd::from_f64(-ETA)),
        (Dd::sum(max, -pow2(969)), Dd::sum(-pow2(1020), ETA)),
        (Dd::sum(pow2(1022), ETA), Dd::sum(-pow2(1022), 3.0 * ETA)),
        (
            Dd::sum(pow2(1022), -pow2(968)),
            Dd::sum(-pow2(1022), pow2(968)),
        ),
    ];
    for (x, y) in pairs {
        for (x, y) in [(x, y), (y, x), (-x, -y)] {
            let (a, b) = (Bounded::<Dd>::exact(x), Bounded::exact(y));
            let r = a + b;
            assert!(
                check("sum", r, Some(&(qd(x) + qd(y)))),
                "{x:?} + {y:?}: {r:?}"
            );
            assert_eq!(bits(r), bits(b + a), "{x:?} + {y:?}");
            if x.lo == 0.0 && y.lo == 0.0 {
                let l0 = Bounded::exact(x.hi) + Bounded::exact(y.hi);
                assert_eq!(r.sign(), l0.sign(), "{x:?} + {y:?}: {l0:?} vs {r:?}");
            }
        }
    }
    let big = Bounded::<Dd>::exact(Dd::from_f64(max));
    let r = big + Bounded::exact(Dd::from_f64(max * pow2(-53)));
    assert!(!r.is_certified(), "{r:?}");
}

/// A value just above the top float, `hi = MAX` with a positive low part
/// (a product rounding down to `MAX`), stays certified through further
/// operations, as its first-level rounding does: its magnitude is bounded
/// above without overflowing.
#[test]
fn values_just_above_the_top_float_stay_certified() {
    let a = f64::from_bits(0x7FEC_386B_BC20_4F8A);
    let b = f64::from_bits(0x3FF2_2495_9840_1DE5);
    let half = Bounded::exact(0.5);
    let l0 = (Bounded::exact(a) * Bounded::exact(b)) * half;
    assert!(l0.is_certified(), "{l0:?}");
    let p = Bounded::exact(a).promote() * Bounded::exact(b).promote();
    assert!(p.v.hi == f64::MAX && p.v.lo > 0.0, "{p:?}");
    let exact = q(a) * q(b);
    assert!(check("product", p, Some(&exact)));
    let h = half.promote();
    for (r, x) in [
        (p * h, exact.clone() * q(0.5)),
        (h * p, exact.clone() * q(0.5)),
        (
            p / Bounded::exact(Dd::from_f64(2.0)),
            exact.clone() / q(2.0),
        ),
        (
            p - Bounded::exact(Dd::from_f64(f64::MAX)),
            exact.clone() - q(f64::MAX),
        ),
    ] {
        assert!(check("after", r, Some(&x)), "{r:?}");
    }
    assert!(check_sqrt(p.sqrt(), &exact));
}

/// Promotion never loses a certified sign: on promoted operands every
/// operation certifies each sign the first level certifies, also for
/// products and quotients within a few `η` of zero, where the result is
/// subnormal (the scaling back once added `3η` to their bound, more than
/// the first level's `η`).
#[test]
fn promoted_operations_keep_the_first_levels_signs() {
    // A product of about `−18η` with a small error on one factor.
    let a = Bounded::new(
        -1.0888031146629295e-161,
        0.0,
        1.0888031146629295e-161 * pow2(-20),
    );
    let b = Bounded::exact(8.358840000037525e-162);
    let l0 = a * b;
    assert_eq!(l0.sign(), Some(Ordering::Less), "{l0:?}");
    let l1 = a.promote() * b.promote();
    assert_eq!(l1.sign(), Some(Ordering::Less), "{l1:?}");
    assert!(check("product", l1, Some(&(q(a.v) * q(b.v)))));
    let mut rng = Rng::new(0x5167);
    let mut certified = 0u32;
    for case in 0..200_000 {
        // Magnitudes whose products and quotients land near zero (most
        // cases) or anywhere.
        let (ea, eb) = match case % 4 {
            0 => (1023 - 500 + rng.below(60), 1023 - 600 + rng.below(60)),
            1 => (1023 - 1000 + rng.below(80), 1023 + rng.below(80)),
            2 => (1023 - 40 + rng.below(80), 1023 - 40 + rng.below(80)),
            _ => (rng.below(2047), rng.below(2047)),
        };
        let (ma, mb) = (rng.magnitude(ea, ea), rng.magnitude(eb, eb));
        let (va, vb) = (rng.sign(ma), rng.sign(mb));
        // Errors from none to about the value, so some signs are just
        // certified and some just not.
        let err = |rng: &mut Rng, v: f64| match rng.below(4) {
            0 => 0.0,
            1 => v.abs() * rng.magnitude(1023 - 60, 1023 - 1),
            2 => rng.magnitude(0, 3),
            _ => v.abs() * rng.magnitude(1023 - 3, 1023 - 1),
        };
        let a = Bounded::new(va, err(&mut rng, va), err(&mut rng, va));
        let b = Bounded::new(vb, err(&mut rng, vb), err(&mut rng, vb));
        let (pa, pb) = (a.promote(), b.promote());
        let ops = [
            ("+", a + b, pa + pb),
            ("-", a - b, pa - pb),
            ("*", a * b, pa * pb),
            ("/", a / b, pa / pb),
            ("√", a.abs().sqrt(), pa.abs().sqrt()),
        ];
        for (op, l0, l1) in ops {
            if let Some(s) = l0.sign() {
                certified += 1;
                assert_eq!(l1.sign(), Some(s), "{op}: {a:?}, {b:?}: {l0:?} vs {l1:?}");
            }
        }
    }
    assert!(certified > 100_000, "{certified}");
}

/// The absolute value of either zero is `+0`, as at the first level, and
/// an exact float is its own magnitude bound, the top float included.
#[test]
fn zeros_and_the_top_float_have_their_magnitudes() {
    for z in [Dd::from_f64(-0.0), Dd::sum(-0.0, -0.0), Dd::from_f64(0.0)] {
        let r = Bounded::<Dd>::exact(z).abs();
        assert!(
            r.v.hi.is_sign_positive() && r.v.lo.is_sign_positive(),
            "{r:?}"
        );
        assert_eq!(bits(r), bits(Bounded::exact(Dd::from_f64(0.0))));
    }
    let max = Bounded::<Dd>::exact(Dd::from_f64(-f64::MAX));
    assert_eq!(max.abs_bound(), f64::MAX);
    assert_eq!(Bounded::exact(-f64::MAX).abs_bound(), f64::MAX);
    // Past the top float no `f64` bounds the magnitude.
    let past = Bounded::<Dd>::exact(Dd::sum(f64::MAX, pow2(960)));
    assert_eq!(past.abs_bound(), f64::INFINITY);
    assert!(past.is_certified());
}

/// The root of a value non-negative by construction is certified however
/// near zero the value rounds, as at the first level.
#[test]
fn roots_of_values_non_negative_by_construction_are_certified() {
    let a = Bounded::<Dd>::new(Dd::from_f64(1e-170), 0.0, 1e-170);
    let n2 = a * a + a * a;
    assert!(!n2.sqrt().is_certified(), "{n2:?}");
    let r = n2.sqrt_nonneg();
    for x in [q(0.0), q(2e-170) * q(2e-170) * q(2.0)] {
        assert!(check_sqrt(r, &x), "{x}: {r:?}");
    }
    let cases = [
        (Dd::from_f64(0.0), ETA),
        (Dd::from_f64(-0.0), ETA),
        (Dd::from_f64(-ETA), 2.0 * ETA),
        (Dd::sum(1e-20, -1e-40), 2e-20),
        (Dd::sum(-1e-20, 1e-40), 1e-20),
        (Dd::sum(2.0, pow2(-60)), 1.0),
    ];
    for (v, e) in cases {
        let x = Bounded::new(v, 0.0, e);
        let root = x.sqrt_nonneg();
        let lo = if qd(v) > q(e) { qd(v) - q(e) } else { q(0.0) };
        for t in [lo, qabs(qd(v)), qd(v) + q(e)] {
            assert!(check_sqrt(root, &t), "{v:?} ± {e}: {root:?}");
        }
        if x.sqrt().is_certified() {
            assert_eq!(bits(root), bits(x.sqrt()), "{x:?}");
        }
    }
    let inf = Bounded::<Dd>::new(Dd::from_f64(f64::INFINITY), 0.0, 0.0);
    assert!(!inf.sqrt_nonneg().is_certified());
}

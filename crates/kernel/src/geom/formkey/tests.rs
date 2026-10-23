//! The routes against wide references: rational values rounded there,
//! irrational ones settled on the schedule or exactly past it, near
//! ties at every depth; the algebraic fallback only at the top level;
//! charges in parts, rounded up as they go; running out never a
//! different rounding.

use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::num::bounded::Bounded;
use varde_decide::num::exact::rat as q;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::round::nearest;
use crate::infra::budget::{Budget, Halt, MAX_WORK, Pass, Work};
use crate::infra::error::{Cause, KernelError};

/// `c + n/√d` rounded through a 400-bit root: an independent reference
/// (a double rounding there needs the value within `2⁻³⁴⁰` ulp of a
/// midpoint, which the random cases don't come near).
fn reference(x: &Quadratic) -> f64 {
    let like = Rational::from(0u32);
    let (c, n, d) = (x.c.at(&like), x.n.at(&like), x.d.at(&like));
    let (r, _) = Float::sqrt_rational_prec_round_ref(&d, 400, Nearest);
    let r = Rational::try_from(&r).unwrap();
    zero(nearest(&(c + n / r)))
}

/// `2^e`, subnormal ones included.
fn pow2(e: i32) -> f64 {
    if e >= -1022 {
        f64::from_bits(((1023 + e) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (e + 1074))
    }
}

fn lit(x: f64) -> Expr {
    Expr::Lit(x)
}

/// `x` rounded by an uncharged meter, with the routes it took.
fn round(x: &Quadratic) -> (f64, RouteCounts) {
    let mut free = Uncharged;
    let mut m = KeyMeter::new(&mut free);
    let v = m.irrational(x).unwrap();
    (v, m.counts())
}

/// `√d`, `d` given as a sum of terms (`n = d`).
fn root(terms: &[f64]) -> Quadratic {
    let d = terms[1..].iter().fold(lit(terms[0]), |s, t| s + lit(*t));
    Quadratic::ratio(d.clone(), d)
}

#[test]
fn irrational_values_match_a_wide_reference() {
    let mut rng = Rng::new(11);
    let mut settled = [0u32; LEVELS];
    for _ in 0..varde_testing::pick(500, 10_000) {
        let a = rng.direction() * rng.log_range(1e-3, 1e3);
        let p = rng.point(1e6);
        let (a, p) = (a.to_array().map(lit), p.to_array().map(lit));
        let aa = Expr::dot(&a, &a);
        let cases = [
            Quadratic::ratio(a[0].clone(), aa.clone()),
            Quadratic::ratio(Expr::dot(&a, &p), aa.clone()),
            Quadratic {
                c: p[2].clone(),
                n: Expr::dot(&a, &p),
                d: aa,
            },
        ];
        for x in &cases {
            let (v, counts) = round(x);
            assert_eq!(v.to_bits(), reference(x).to_bits(), "{x:?}");
            for (s, c) in settled.iter_mut().zip(counts.levels) {
                *s += c;
            }
            assert_eq!(counts.fallback, 0);
        }
    }
    // Nearly all settle at double-double; `f64` alone never brackets an
    // irrational value between its midpoints.
    assert_eq!(settled[0], 0, "{settled:?}");
    assert!(settled[1] > 0, "{settled:?}");
}

#[test]
fn rational_values_take_the_rational_route() {
    // A zero numerator, an exact square radicand, and a tie: `√d` exactly
    // the midpoint of 1 and the next `f64`, rounded to the even 1.
    let half_ulp = f64::EPSILON / 2.0;
    let cases = [
        (Quadratic::ratio(lit(0.0), lit(3.0)), 0.0f64),
        (Quadratic::ratio(lit(3.0), lit(9.0) + lit(16.0)), 0.6),
        (root(&[1.0, 2.0 * half_ulp, half_ulp * half_ulp]), 1.0),
        (
            Quadratic {
                c: lit(-1.0),
                n: lit(-0.0),
                d: lit(2.0),
            },
            -1.0,
        ),
    ];
    for (x, want) in cases {
        let (v, counts) = round(&x);
        assert_eq!(v.to_bits(), want.to_bits(), "{x:?}");
        assert_eq!(counts.rational, 1, "{x:?}");
    }
    // A zero is stored as `+0`.
    let (v, _) = round(&Quadratic::ratio(lit(-0.0), lit(2.0)));
    assert_eq!(v.to_bits(), 0);
}

/// Values a distance `2^−e` from the midpoint of 1 and the next `f64`,
/// either side: settled at the first level wide enough, or exactly past
/// the top, always to the right side.
#[test]
fn near_ties_settle_at_the_level_their_distance_needs() {
    let ulp = f64::EPSILON;
    let m2 = [1.0, ulp, ulp * ulp / 4.0];
    let up = 1.0f64.next_up();
    for (e, level) in [
        (150, Some(Level::L2)),
        (300, Some(Level::L3)),
        (700, Some(Level::L4)),
        (1000, None),
        (1060, None),
    ] {
        for sign in [1.0, -1.0] {
            // `d = m² ± 2^−e`, so `√d = m ± 2^−(e+1)` to first order.
            let eps = sign * pow2(-e);
            let x = root(&[m2[0], m2[1], m2[2], eps]);
            let (v, counts) = round(&x);
            let want = if sign > 0.0 { up } else { 1.0 };
            assert_eq!(v, want, "2^-{e} {sign}");
            match level {
                Some(l) => assert_eq!(counts.levels[l.index()], 1, "2^-{e}: {counts:?}"),
                None => assert_eq!(counts.exact, 1, "2^-{e}: {counts:?}"),
            }
        }
    }
}

#[test]
fn values_at_the_ends_of_the_range_round_exactly() {
    // Overflowing parts at the first levels, subnormal results: the loop
    // escalates and the exact rounding stands.
    let cases = [
        Quadratic::ratio(lit(1e300) * lit(1e10), lit(3.0)),
        Quadratic::ratio(lit(5e-324), lit(3.0)),
        Quadratic::ratio(lit(1e-310), lit(7.0)),
        Quadratic::ratio(lit(f64::MAX), lit(1.5)),
    ];
    for x in &cases {
        let (v, counts) = round(x);
        assert_eq!(v.to_bits(), reference(x).to_bits(), "{x:?}");
        assert_eq!(counts.fallback, 0);
    }
    assert_eq!(round(&cases[0]).0, f64::INFINITY);
}

/// An algebraic formula at the levels: `√t`, `t` the sum of `terms`, its
/// error widened by `widen` (to stand for a formula that never settles).
struct Root {
    terms: Vec<f64>,
    widen: f64,
}

impl Algebraic for Root {
    fn at<S: LevelScalar>(&self, like: &S) -> S {
        let t = self.terms[1..]
            .iter()
            .fold(S::from_f64(self.terms[0], like), |s, x| {
                s.add(&S::from_f64(*x, like))
            });
        t.sqrt_nonneg().widened(self.widen)
    }
}

fn algebraic(x: &Root) -> (Result<f64, AlgebraicError<Infallible>>, RouteCounts) {
    let mut free = Uncharged;
    let mut m = KeyMeter::new(&mut free);
    let v = m.algebraic(x);
    (v, m.counts())
}

#[test]
fn the_algebraic_fallback_is_the_even_neighbour_at_the_top_only() {
    let ulp = f64::EPSILON;
    let m2 = vec![1.0, ulp, ulp * ulp / 4.0];
    // Exactly the midpoint of 1 and the next `f64`: no level settles it,
    // the top leaves the two neighbours, and the even one (1) stands.
    let (v, counts) = algebraic(&Root {
        terms: m2.clone(),
        widen: 0.0,
    });
    assert_eq!(v, Ok(1.0));
    assert_eq!(counts.fallback, 1);
    // The midpoint above the next `f64`: its even neighbour is above it.
    // (`(1 + 3u/2)² = 1 + 3u + 9u²/4`, `u` the ulp of 1.)
    let up = 1.0f64.next_up();
    let (v, counts) = algebraic(&Root {
        terms: vec![1.0, 3.0 * ulp, 2.25 * ulp * ulp],
        widen: 0.0,
    });
    assert_eq!(v, Ok(up.next_up()));
    assert_eq!(counts.fallback, 1);
    // Off the tie by `2^−300`: settled at a level, no fallback.
    let mut near = m2.clone();
    near.push(pow2(-300));
    let (v, counts) = algebraic(&Root {
        terms: near,
        widen: 0.0,
    });
    assert_eq!(v, Ok(up));
    assert_eq!((counts.fallback, counts.levels[Level::L3.index()]), (0, 1));
    // An error wider than an ulp at the top: not a tie, so no fallback.
    let (v, counts) = algebraic(&Root {
        terms: vec![2.0],
        widen: 1e-10,
    });
    assert_eq!(v, Err(AlgebraicError::Precision));
    assert_eq!(counts.fallback, 0);
}

/// `x` rounded in a pass of one item on a budget of `budget` units: the
/// outcome and the units used.
fn charged(x: &Quadratic, budget: u64) -> (Result<Vec<f64>, Halt<()>>, u64) {
    let mut work = Work::new(&Budget::new(budget));
    let pass = Pass {
        stage: "keys",
        rung: 0,
    };
    let out = work.pass(pass, std::slice::from_ref(x), |x, ledger| {
        let mut m = KeyMeter::new(ledger);
        Ok(m.irrational(x)?)
    });
    (out, work.used())
}

#[test]
fn rounding_is_charged_in_parts_and_runs_out_as_the_budget() {
    let u = UNITS.form_key;
    let parts_to = |level: usize| u.rational + u.level[..=level].iter().sum::<u64>();
    // Settled at double-double: the exact value formed, two levels.
    let x = Quadratic::ratio(lit(1.0), lit(2.0));
    assert_eq!(round(&x).1.levels[1], 1);
    let (out, used) = charged(&x, MAX_WORK);
    assert_eq!(out.unwrap(), vec![std::f64::consts::FRAC_1_SQRT_2]);
    assert_eq!(used, parts_to(1).div_ceil(UNIT_PARTS));
    // A near tie settled past the top: every level and a round, each
    // charged before it runs. Short of that, the budget runs out at a
    // precision step, never with a value.
    let ulp = f64::EPSILON;
    let x = root(&[1.0, ulp, ulp * ulp / 4.0, pow2(-1000)]);
    let all = (parts_to(LEVELS - 1) + u.settle).div_ceil(UNIT_PARTS);
    let (out, used) = charged(&x, MAX_WORK);
    assert_eq!(out.unwrap(), vec![1.0f64.next_up()]);
    assert_eq!(used, all);
    for budget in [all - 1, parts_to(2).div_ceil(UNIT_PARTS)] {
        let (out, used) = charged(&x, budget);
        let Err(Halt::OutOfBudget(KernelError::OutOfBudget { cause, .. })) = out else {
            panic!("{budget}: {out:?}");
        };
        assert_eq!((cause, used), (Cause::Precision, budget));
    }
}

#[test]
fn a_meter_charges_whole_units_as_its_parts_cross_them() {
    struct Count(Vec<(Step, u64)>);
    impl Charge for Count {
        type Err = Infallible;
        fn charge(&mut self, step: Step, units: u64) -> Result<(), Infallible> {
            self.0.push((step, units));
            Ok(())
        }
    }
    let mut c = Count(Vec::new());
    let mut m = KeyMeter::new(&mut c);
    let x = Quadratic::ratio(lit(1.0), lit(3.0));
    let mut total = 0;
    for _ in 0..40 {
        let _ = m.irrational(&x);
        total += UNITS.form_key.rational + UNITS.form_key.level[0] + UNITS.form_key.level[1];
    }
    let _ = m.rational(|| q(0.5));
    total += UNITS.form_key.rational;
    let charged: u64 = c.0.iter().map(|(_, n)| n).sum();
    assert_eq!(charged, total.div_ceil(UNIT_PARTS));
    assert!(c.0.iter().all(|(_, n)| *n > 0));
    assert!(c.0.iter().any(|(s, _)| *s == Step::Precision { level: 1 }));
}

/// The level formula and the exact value agree: the first level's
/// value encloses the exact one.
#[test]
fn the_level_formula_encloses_the_exact_value() {
    let mut rng = Rng::new(5);
    for _ in 0..varde_testing::pick(200, 2_000) {
        let a = rng.direction().to_array().map(lit);
        let x = Quadratic::ratio(a[1].clone(), Expr::dot(&a, &a));
        let b = x.at(&Bounded::exact(0.0));
        let [lo, hi] = b.enclose();
        let r = reference(&x);
        assert!(lo <= r && r <= hi, "{x:?} {b:?}");
    }
}

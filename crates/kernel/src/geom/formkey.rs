//! Rounding a form key's parameters ([`super::form`]) to their nearest
//! `f64`, ties to even, by one of three routes, charged to the
//! operation's budget as it goes.
//!
//! **Routes.** A key parameter is an exact value of the stored inputs:
//! - **rational** ([`KeyMeter::rational`]): a foot point, an input taken
//!   as it is. Its exact value is formed in rationals and rounded once.
//! - **quadratic irrational** ([`KeyMeter::irrational`], a [`Quadratic`]
//!   `c + n/√d`, `c`, `n`, `d` polynomials in the inputs, `d > 0`): a
//!   normalised direction's component, a plane's offset, a cone's cosine
//!   and sine, a profile's coordinate in the canonical frame. Its exact
//!   `c`, `n` and `d` are formed in rationals first: where `n = 0` or `d`
//!   is an exact square the value is rational and rounded there.
//!   Otherwise a **Ziv loop** runs on the precision schedule
//!   ([`Level`]): at each level, from `f64` up to [`P_MAX_BITS`], the
//!   formula is evaluated with its certified error and settles where the
//!   value lies certainly between the two midpoints around one `f64`.
//!   An irrational value is never a midpoint (midpoints are rational), so
//!   the loop ends; past the top level it goes on exactly
//!   (`settle`): the root enclosed at twice the precision, and again,
//!   until both ends round alike. So the route always ends with the
//!   correctly rounded value, whatever the inputs' exponents.
//! - **algebraic**, of higher degree or nested roots
//!   ([`KeyMeter::algebraic`], an [`Algebraic`] formula): the same loop
//!   on the schedule. Where the top level leaves the value between two
//!   neighbouring `f64`s, it is rounded as an exact tie: the **even** of
//!   the two (the fallback), counted ([`RouteCounts::fallback`]). Every
//!   path to it gives the same bits. No key of the kinds built so far
//!   takes this route.
//!
//! Every route gives the same bits for one exact value, so two
//! constructions of one exact form give the same key whichever route
//! each parameter took. A zero is stored as `+0`.
//!
//! **Charges.** Each step is charged before it runs, in parts of a unit
//! ([`KeyUnits`]): the exact value formed, each level of the loop (a
//! precision step above the first level), each round of settling
//! exactly. A meter ([`KeyMeter`]) sums one item's parts and charges its
//! ledger whole units as the sum crosses them. Running out while
//! rounding a key is the budget running out, never the fallback, so a
//! key's bits never depend on the budget. A key built outside an
//! operation is not charged ([`Uncharged`]).

use std::cmp::Ordering;
use std::convert::Infallible;
use std::ops::{Add, Mul, Neg, Sub};

use malachite::base::num::arithmetic::traits::{CheckedSqrt, Sign};
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor};
use malachite::{Float, Rational};
use varde_decide::bezier::Scalar;
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::{KeyUnits, LEVELS, Level, P_MAX_BITS, UNIT_PARTS, UNITS};

use super::round::nearest;
use crate::infra::budget::{Exhausted, Ledger, Step, level_step};

/// What a key's rounding charges its steps to.
pub(crate) trait Charge {
    /// Why a charge fails.
    type Err;

    /// Charges `units` of `step`.
    fn charge(&mut self, step: Step, units: u64) -> Result<(), Self::Err>;
}

impl Charge for Ledger<'_> {
    type Err = Exhausted;

    fn charge(&mut self, step: Step, units: u64) -> Result<(), Exhausted> {
        Ledger::charge(self, step, units)
    }
}

/// No budget: a key built outside an operation.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Uncharged;

impl Charge for Uncharged {
    type Err = Infallible;

    fn charge(&mut self, _: Step, _: u64) -> Result<(), Infallible> {
        Ok(())
    }
}

/// How many parameters each route rounded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteCounts {
    /// Rounded from their exact rational value.
    pub rational: u32,
    /// Settled by the loop at each level of the schedule.
    pub levels: [u32; LEVELS],
    /// Irrational, settled exactly past the top level.
    pub exact: u32,
    /// Algebraic, rounded as a tie at the top level (the fallback).
    pub fallback: u32,
}

/// Why an algebraic parameter can't be rounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the algebraic keys use it as they land")
)]
pub(crate) enum AlgebraicError<E> {
    /// The budget ran out.
    Budget(E),
    /// The top level leaves more than two `f64`s possible: the formula's
    /// error, not a near tie.
    Precision,
}

/// Rounds one item's key parameters and charges their steps (module
/// docs), counting the routes taken.
#[derive(Debug)]
pub(crate) struct KeyMeter<'c, C: Charge> {
    charge: &'c mut C,
    /// The parts charged so far.
    parts: u64,
    counts: RouteCounts,
}

impl<'c, C: Charge> KeyMeter<'c, C> {
    /// A meter charging `charge`.
    pub(crate) fn new(charge: &'c mut C) -> Self {
        KeyMeter {
            charge,
            parts: 0,
            counts: RouteCounts::default(),
        }
    }

    /// The routes taken so far.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the operations' key stages report it as they land"
        )
    )]
    pub(crate) fn counts(&self) -> RouteCounts {
        self.counts
    }

    /// Charges `parts` more of `step`: the whole units the running sum of
    /// parts crosses.
    fn step(&mut self, step: Step, parts: u64) -> Result<(), C::Err> {
        let before = self.parts.div_ceil(UNIT_PARTS);
        self.parts = self.parts.saturating_add(parts);
        let units = self.parts.div_ceil(UNIT_PARTS) - before;
        if units > 0 {
            self.charge.charge(step, units)?;
        }
        Ok(())
    }

    /// The rational value `x()` rounded to nearest, its forming charged
    /// first; `+0` for a zero.
    pub(crate) fn rational(&mut self, x: impl FnOnce() -> Rational) -> Result<f64, C::Err> {
        self.step(Step::Work, units().rational)?;
        self.counts.rational += 1;
        Ok(zero(nearest(&x())))
    }

    /// `c + n/√d` rounded to nearest (module docs): rationally where it
    /// is rational, else by the loop on the schedule, else exactly.
    pub(crate) fn irrational(&mut self, x: &Quadratic) -> Result<f64, C::Err> {
        self.step(Step::Work, units().rational)?;
        let like = Rational::from(0u32);
        let (c, n, d) = (x.c.at(&like), x.n.at(&like), x.d.at(&like));
        debug_assert!(d > 0u32, "a radicand not positive");
        if n == 0u32 {
            self.counts.rational += 1;
            return Ok(zero(nearest(&c)));
        }
        if let Some(r) = (&d).checked_sqrt() {
            self.counts.rational += 1;
            return Ok(zero(nearest(&(c + n / r))));
        }
        for level in Level::ALL {
            let i = level.index();
            self.step(level_step(i as u8), units().level[i])?;
            if let Near::One(v) = at_level(&mut AtQuadratic(x), level, &mut ()) {
                self.counts.levels[i] += 1;
                return Ok(v);
            }
        }
        let mut prec = 2 * u64::from(P_MAX_BITS);
        loop {
            self.step(level_step(Level::TOP.index() as u8), units().settle)?;
            if let Some(v) = settle(&c, &n, &d, prec) {
                self.counts.exact += 1;
                return Ok(v);
            }
            prec = prec.saturating_mul(2);
        }
    }

    /// The algebraic value `x` rounded to nearest by the loop on the
    /// schedule, the even of the two `f64`s the top level leaves where it
    /// can't settle (the fallback, counted).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the algebraic keys call it as they land")
    )]
    pub(crate) fn algebraic<A: Algebraic>(&mut self, x: &A) -> Result<f64, AlgebraicError<C::Err>> {
        let mut last = Near::Open;
        for level in Level::ALL {
            let i = level.index();
            self.step(level_step(i as u8), units().level[i])
                .map_err(AlgebraicError::Budget)?;
            last = at_level(&mut AtAlgebraic(x), level, &mut ());
            if let Near::One(v) = last {
                self.counts.levels[i] += 1;
                return Ok(v);
            }
        }
        match last {
            Near::Two(a, b) => {
                self.counts.fallback += 1;
                Ok(if a.to_bits() & 1 == 0 { a } else { b })
            }
            _ => Err(AlgebraicError::Precision),
        }
    }
}

fn units() -> KeyUnits {
    UNITS.form_key
}

/// A polynomial in `f64` inputs: evaluated at any precision level, or
/// exactly.
#[derive(Clone, Debug)]
pub(crate) enum Expr {
    /// An input, exactly.
    Lit(f64),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
}

impl Expr {
    /// The value at `like`'s level (exactly for a [`Rational`]).
    pub(crate) fn at<S: Scalar>(&self, like: &S) -> S {
        match self {
            Expr::Lit(x) => S::from_f64(*x, like),
            Expr::Add(a, b) => a.at(like).add(&b.at(like)),
            Expr::Sub(a, b) => a.at(like).sub(&b.at(like)),
            Expr::Mul(a, b) => a.at(like).mul(&b.at(like)),
            Expr::Neg(a) => a.at(like).neg(),
        }
    }

    /// `a·b` summed over three components.
    pub(crate) fn dot(a: &[Expr; 3], b: &[Expr; 3]) -> Expr {
        a[0].clone() * b[0].clone() + a[1].clone() * b[1].clone() + a[2].clone() * b[2].clone()
    }
}

impl From<f64> for Expr {
    fn from(x: f64) -> Expr {
        Expr::Lit(x)
    }
}

impl Add for Expr {
    type Output = Expr;
    fn add(self, o: Expr) -> Expr {
        Expr::Add(Box::new(self), Box::new(o))
    }
}

impl Sub for Expr {
    type Output = Expr;
    fn sub(self, o: Expr) -> Expr {
        Expr::Sub(Box::new(self), Box::new(o))
    }
}

impl Mul for Expr {
    type Output = Expr;
    fn mul(self, o: Expr) -> Expr {
        Expr::Mul(Box::new(self), Box::new(o))
    }
}

impl Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        Expr::Neg(Box::new(self))
    }
}

/// A quadratic irrational parameter `c + n/√d`, `d > 0` exactly.
#[derive(Clone, Debug)]
pub(crate) struct Quadratic {
    pub(crate) c: Expr,
    pub(crate) n: Expr,
    pub(crate) d: Expr,
}

impl Quadratic {
    /// `n/√d`.
    pub(crate) fn ratio(n: Expr, d: Expr) -> Quadratic {
        Quadratic {
            c: Expr::Lit(0.0),
            n,
            d,
        }
    }

    /// The value at `like`'s level.
    fn at<S: LevelScalar>(&self, like: &S) -> S {
        let root = self.d.at(like).sqrt_nonneg();
        self.c.at(like).add(&self.n.at(like).div(&root))
    }
}

/// An algebraic parameter's formula at the levels: of degree above two
/// or with nested roots, so it has no exact settling.
pub(crate) trait Algebraic {
    /// The value at `like`'s level, with its certified error.
    fn at<S: LevelScalar>(&self, like: &S) -> S;
}

struct AtQuadratic<'a>(&'a Quadratic);

impl AtLevel<()> for AtQuadratic<'_> {
    type Out = Near;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Near {
        near(&self.0.at(like))
    }

    fn escalates(out: &Near) -> bool {
        !matches!(out, Near::One(_))
    }
}

struct AtAlgebraic<'a, A>(&'a A);

impl<A: Algebraic> AtLevel<()> for AtAlgebraic<'_, A> {
    type Out = Near;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Near {
        near(&self.0.at(like))
    }

    fn escalates(out: &Near) -> bool {
        !matches!(out, Near::One(_))
    }
}

/// Where a value at a level lies among the `f64`s.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Near {
    /// Certainly nearest to this one (`+0` for a zero).
    One(f64),
    /// Certainly between these two neighbours' outer midpoints, the
    /// midpoint between them undecided.
    Two(f64, f64),
    /// Neither: the error spans more, or the value is near the ends of
    /// the range.
    Open,
}

/// Where `x` lies: the `f64` its value rounds to, `c`, tested against the
/// midpoints around it, and where one test fails, the neighbour beyond
/// it against its own outer midpoint.
fn near<S: LevelScalar>(x: &S) -> Near {
    let c = zero(x.approx());
    let finite = |v: f64| v.is_finite() && v.abs() < f64::MAX;
    if !finite(c) {
        return Near::Open;
    }
    let (down, up) = (c.next_down(), c.next_up());
    // The sign of `x` against the midpoint of `a` and `b`: certain or not.
    let side = |a: f64, b: f64| {
        let m = S::from_f64(a, x).add(&S::from_f64(b, x));
        x.mul(&S::int(2, x)).sub(&m).sign()
    };
    let above = side(down, c) == Some(Ordering::Greater);
    let below = side(c, up) == Some(Ordering::Less);
    match (above, below) {
        (true, true) => Near::One(c),
        (true, false) if finite(up) && side(up, up.next_up()) == Some(Ordering::Less) => {
            Near::Two(c, up)
        }
        (false, true)
            if finite(down) && side(down.next_down(), down) == Some(Ordering::Greater) =>
        {
            Near::Two(zero(down), c)
        }
        _ => Near::Open,
    }
}

/// `c + n/√d` rounded to nearest from the root enclosed at `prec` bits,
/// if both ends of the enclosure round alike; `n ≠ 0`, `d > 0` not a
/// square, so the value is irrational, never a midpoint, and a precision
/// high enough always settles it.
fn settle(c: &Rational, n: &Rational, d: &Rational, prec: u64) -> Option<f64> {
    let root = |mode| {
        let (f, _) = Float::sqrt_rational_prec_round_ref(d, prec, mode);
        Rational::try_from(&f).ok()
    };
    let (lo, hi) = (root(Floor)?, root(Ceiling)?);
    if Sign::sign(&lo) != Ordering::Greater {
        return None;
    }
    let (a, b) = (c + n / &lo, c + n / &hi);
    let (a, b) = (nearest(&a), nearest(&b));
    (a.to_bits() == b.to_bits()).then_some(zero(a))
}

/// `x`, with `+0` for `−0`.
pub(crate) fn zero(x: f64) -> f64 {
    if x == 0.0 { 0.0 } else { x }
}

/// The steps of the routes, for the calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times each against the
/// parts it is charged. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;

    /// A unit direction's component `d[k]/|d|`, as the keys round it.
    pub struct Component(Quadratic);

    impl Component {
        /// The component `k` of `d`'s direction.
        pub fn new(d: [f64; 3], k: usize) -> Component {
            let e = d.map(Expr::Lit);
            Component(Quadratic::ratio(Expr::Lit(d[k]), Expr::dot(&e, &e)))
        }

        /// Its exact value formed and its route read off, as the
        /// irrational route starts: whether it is rational.
        pub fn rational(&self) -> bool {
            let like = Rational::from(0u32);
            let (n, d) = (self.0.n.at(&like), self.0.d.at(&like));
            n == 0u32 || (&d).checked_sqrt().is_some()
        }

        /// One step of the loop at precision level `level` (from 0);
        /// whether it settles there, `None` for a level the schedule
        /// doesn't have.
        pub fn level(&self, level: usize) -> Option<bool> {
            let level = Level::from_index(level)?;
            let out = at_level(&mut AtQuadratic(&self.0), level, &mut ());
            Some(matches!(out, Near::One(_)))
        }
    }

    /// A component's exact parts, for timing a settling round alone.
    pub struct Exact {
        c: Rational,
        n: Rational,
        d: Rational,
    }

    impl Exact {
        /// Its exact parts.
        pub fn of(x: &Component) -> Exact {
            let like = Rational::from(0u32);
            Exact {
                c: x.0.c.at(&like),
                n: x.0.n.at(&like),
                d: x.0.d.at(&like),
            }
        }

        /// One round of settling at the first precision past the top
        /// level; whether it settles.
        pub fn settle(&self) -> bool {
            super::settle(&self.c, &self.n, &self.d, 2 * u64::from(P_MAX_BITS)).is_some()
        }
    }
}

#[cfg(test)]
mod tests;

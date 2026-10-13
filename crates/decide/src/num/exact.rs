//! Exact flat predicates: the signs of polynomials in stored coordinates,
//! with exact ties broken by symbolic perturbation.
//!
//! **A predicate is written once** ([`Pred::eval`]), generic over the
//! number type [`Num`] (`+ − ×` and negation, no division or roots: an
//! unnormalised, sign-equivalent form of fixed degree), and evaluated by
//! [`sign`] in stages, each run only when the one before can't decide:
//!
//! 1. **the filter**, in `f64` with a certified bound on its rounding
//!    error ([`Approx`]): it decides when the value is farther from zero
//!    than the bound, and also when it is an exact zero it can prove (a
//!    difference of equal coordinates, a product with one);
//! 2. **the exact stage**, in floating-point expansions
//!    ([`super::expansion::Exp`]): the exact value, whose sign is that of
//!    its largest component;
//! 3. **the rational stage**, in `malachite`'s `Rational`, only where an
//!    expansion product could underflow or a value overflow (below).
//!
//! There are no near ties: a value is zero only when it is exactly zero.
//! A zero is then broken by the perturbation, coefficient by coefficient.
//!
//! **Perturbation.** Inputs that move ([`Num::moved`]) carry, besides
//! their value, their rate of change along each of [`ORDERS`] motions
//! (the element's own perturbation direction first, then the fixed
//! generic translations): the configuration moved by `δ·(d + η·T₂ + η²·T₃ +
//! η³·T₄)` for an infinitely small `η` and a `δ` infinitely smaller still.
//! To first order in `δ` the predicate is `g + δ·(g₁ + η·g₂ + η²·g₃ +
//! η³·g₄)`, `gₖ` its derivative along motion `k`, so its sign is that of
//! the first of `g, g₁, …, g₄` that isn't zero. That is the moved
//! configuration's own sign also where the predicate isn't linear in the
//! motion (several points moving apart, say): the terms in `δ²` lose to
//! the first-order one. [`sign`] works the orders out one at a time, in order,
//! each through the stages above, as a first-order value in that motion
//! alone (`Dual`: products drop the square of the infinitesimal). Rates
//! needn't be unit: scaling one motion's rates, all inputs alike, by a
//! positive factor scales its `gₖ` and keeps every sign. If all
//! five are zero there is no first-order sign, which callers treat as a
//! missing rule, not a sign: a predicate linear in the motion then
//! doesn't depend on it, but one that isn't can still change at higher
//! powers of `δ` (four coincident points moving apart have an orientation
//! of order `δ³`).
//!
//! **Input range.** Every input, coordinate or rate, is finite and at most
//! [`MAX_INPUT`] (`2³²`) in magnitude, which debug builds assert. The
//! kernel's stored positions are within its coordinate limit, `1e6 < 2²⁰`;
//! points of the lattice tiles of planes lie within a cell of edge `2²²`
//! about a point of that box, below `2²³`; directions are normals scaled to
//! a largest component of 1 and their sums. The exact stage needs more
//! than a bound, though: an expansion product is exact only while the
//! product of its factors' lowest set bits is at least `2⁻¹⁰⁷⁴` (else
//! it underflows, and a nonzero value can come out as zero), its factors
//! are below `2⁹⁹⁶` and their product below `2¹⁰²¹` (the expansion module's
//! range). Coordinates near zero have low set bits however small they
//! are, so no bound on the inputs keeps a product in range. Instead every
//! exact-stage value carries the lowest set bit its components can have
//! (an input's own; the least of a sum's operands'; the sum of a
//! product's factors') and its largest component, and a product that
//! could leave the range is not formed: the evaluation is redone in
//! rationals. A degree-`j` value of inputs below `MAX_INPUT` is below
//! `2^(32·j)` times its number of terms, so the top of the range is out of
//! reach for flat predicates (degree a dozen at most), and in practice
//! only inputs with set bits below about `2^(−1074/j)` take the rational
//! stage (a coordinate under `1e-100` or so in an orientation).
//!
//! **Work.** `varde-decide` has no work budget; each evaluation reports
//! the stages it ran ([`Work`]: filter runs, exact runs and the longest
//! expansion they formed, rational runs), and the caller charges them to
//! its budget, an exact or rational run as one expansion stage.
//!
//! **Same bits everywhere.** The filter rounds every operation to nearest
//! and bounds the error upwards as [`super::bounded`] does, without
//! directed rounding; expansions use no fused multiply-add; rationals are
//! exact. So every stage's answer, and which stage gives it, is the same
//! on every platform.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign;

use super::bounded::{U, add_up, mul_up, up};
use super::expansion::{Exp, bit_span, two_sum};

/// The number of perturbation motions after the unperturbed value: the
/// element's own direction, then the three generic translations.
pub const ORDERS: usize = 4;

/// The largest input magnitude, `2³²` (module docs): far above the
/// kernel's coordinate limit and its lattice tiles' points.
pub const MAX_INPUT: f64 = 4_294_967_296.0;

/// Numbers predicates are written over: the filter's [`Approx`], the exact
/// stage's expansions, rationals, and first-order values in one motion
/// over each. Implemented by this module's stages only.
pub trait Num: Clone {
    /// An input, unmoved: finite and at most [`MAX_INPUT`] in magnitude.
    fn lit(x: f64) -> Self;

    /// An input moved along the motions at the rates `rows`
    /// (`rows[k − 1]` along motion `k`), each finite and at most
    /// [`MAX_INPUT`].
    fn moved(x: f64, rows: [f64; ORDERS]) -> Self {
        Self::lit(x).moved_by(&rows.map(Self::lit))
    }

    /// `self`, moved further along the motions at the rates `rows`: their
    /// values, worked out as expressions (an exact direction from
    /// normals); their own motion, if any, is ignored.
    fn moved_by(self, rows: &[Self; ORDERS]) -> Self;

    /// `self + o`.
    fn add(&self, o: &Self) -> Self;

    /// `self − o`.
    fn sub(&self, o: &Self) -> Self;

    /// `self · o`.
    fn mul(&self, o: &Self) -> Self;

    /// `−self`.
    fn neg(&self) -> Self;
}

/// A predicate: a polynomial in its inputs, written once over [`Num`].
pub trait Pred {
    /// The value, built from the inputs with [`Num::lit`] and
    /// [`Num::moved`] and the operations of [`Num`].
    fn eval<N: Num>(&self) -> N;
}

/// What an evaluation ran, for the caller to charge to its budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Filter runs, one per order tried.
    pub filters: u32,
    /// Exact runs in expansions, one per order the filter left undecided.
    pub expansions: u32,
    /// The most components of any expansion those runs formed.
    pub longest: u32,
    /// Runs in rationals, after an exact run that would have left the
    /// expansions' range.
    pub rationals: u32,
}

/// The sign of a perturbed predicate and what it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signs {
    /// The sign as the motions go to zero and the order that gave it, `0`
    /// for the value `g` itself, `k` for `gₖ`; never `Equal`. `None` when
    /// `g` and every `gₖ` are zero: no first-order sign (module docs).
    pub first: Option<(usize, Ordering)>,
    /// The stages run.
    pub work: Work,
}

/// The sign of `pred`'s value with its inputs unmoved, exactly, and what
/// it took: for certificates on stored data, which are never perturbed.
pub fn value_sign(pred: &impl Pred) -> (Ordering, Work) {
    let mut work = Work::default();
    let s = staged(
        &mut work,
        || pred.eval::<Approx>(),
        || pred.eval::<Exact>(),
        || pred.eval::<Rat>(),
    );
    (s, work)
}

/// The sign of `pred` under the perturbation, exactly: that of the first
/// of `g, g₁, …, g₄` that isn't zero, each worked out only when the ones
/// before are zero (module docs).
pub fn sign<P: Pred>(pred: &P) -> Signs {
    let (s, mut work) = value_sign(pred);
    if s != Ordering::Equal {
        return Signs {
            first: Some((0, s)),
            work,
        };
    }
    let first = (1..)
        .zip(order_signs(pred, &mut work))
        .find(|&(_, s)| s != Ordering::Equal);
    Signs { first, work }
}

/// The signs of `g₁, …, g₄` alone, `pred`'s derivatives along the
/// motions, each worked out through the stages when pulled, its work
/// added to `work`: for a record whose value comes from elsewhere (a
/// height shared by every record of its key) while its orders are its
/// own.
pub fn order_signs<'a, P: Pred>(
    pred: &'a P,
    work: &'a mut Work,
) -> impl Iterator<Item = Ordering> + 'a {
    let orders: [fn(&P, &mut Work) -> Ordering; ORDERS] =
        [order::<P, 1>, order::<P, 2>, order::<P, 3>, order::<P, 4>];
    orders.into_iter().map(move |order| order(pred, work))
}

/// The sign of `gₖ`, `pred`'s derivative along motion `K`, through the
/// stages.
fn order<P: Pred, const K: usize>(pred: &P, work: &mut Work) -> Ordering {
    staged(
        work,
        || pred.eval::<Dual<Approx, K>>().d,
        || pred.eval::<Dual<Exact, K>>().d,
        || pred.eval::<Dual<Rat, K>>().d,
    )
}

/// `gₖ` itself, exactly, for tests comparing it with derivatives worked
/// out otherwise.
#[cfg(test)]
pub(crate) fn order_value<P: Pred, const K: usize>(pred: &P) -> Rational {
    pred.eval::<Dual<Rat, K>>().d.0
}

/// `g` itself, exactly, for tests.
#[cfg(test)]
pub(crate) fn value(pred: &impl Pred) -> Rational {
    pred.eval::<Rat>().0
}

/// One value's sign through the stages: the filter, then expansions,
/// then rationals if the expansions would leave their range.
fn staged(
    work: &mut Work,
    filter: impl FnOnce() -> Approx,
    exact: impl FnOnce() -> Exact,
    rational: impl FnOnce() -> Rat,
) -> Ordering {
    work.filters += 1;
    if let Some(s) = filter().sign() {
        return s;
    }
    work.expansions += 1;
    let e = exact();
    if e.longest > work.longest {
        work.longest = e.longest;
    }
    if e.in_range {
        return e.e.sign();
    }
    work.rationals += 1;
    rational().0.sign()
}

/// Debug builds: an input in the documented range (module docs).
fn input(x: f64) -> f64 {
    debug_assert!(
        x.is_finite() && x.abs() <= MAX_INPUT,
        "predicate input {x:e} beyond 2³²"
    );
    x
}

/// The filter's number: an `f64` value `v` and an upper bound `err` on
/// its distance from the exact value of the same expression; `err` zero
/// means exact. Bounds rounded up as [`super::bounded`]'s, so sound with
/// subnormals; an overflow makes them infinite.
#[derive(Clone, Copy, Debug)]
pub struct Approx {
    v: f64,
    err: f64,
}

impl Approx {
    /// The certified sign: `Equal` only for an exact zero; `None` when the
    /// bound doesn't settle it, or the value isn't finite.
    fn sign(self) -> Option<Ordering> {
        // An infinite `err` decides nothing; `up` never gives a NaN.
        (self.v.is_finite() && (self.err == 0.0 || self.v.abs() > self.err))
            .then(|| cmp_zero(self.v))
    }

    /// A rounded result `v` whose operands' errors propagate to at most
    /// `carried`: that plus its own rounding, at most `u·|v|` (or half the
    /// least subnormal, which [`up`]'s floor covers).
    fn rounded(v: f64, carried: f64) -> Approx {
        Approx {
            v,
            err: add_up(carried, up(U * v.abs())),
        }
    }
}

/// The sign of a finite `x`, a zero of either sign `Equal`.
fn cmp_zero(x: f64) -> Ordering {
    if x > 0.0 {
        Ordering::Greater
    } else if x < 0.0 {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

impl Num for Approx {
    fn lit(x: f64) -> Self {
        Approx {
            v: input(x),
            err: 0.0,
        }
    }

    /// The filter runs first on every predicate, so debug builds check
    /// every rate here, also those of orders never worked out.
    fn moved(x: f64, rows: [f64; ORDERS]) -> Self {
        for r in rows {
            input(r);
        }
        Self::lit(x)
    }

    fn moved_by(self, _: &[Self; ORDERS]) -> Self {
        self
    }

    /// Exact when both operands are and `two_sum` leaves no error (a
    /// difference of equal coordinates is a true zero).
    fn add(&self, o: &Self) -> Self {
        if self.err == 0.0 && o.err == 0.0 {
            let (s, t) = two_sum(self.v, o.v);
            if t == 0.0 && s.is_finite() {
                return Approx { v: s, err: 0.0 };
            }
        }
        Approx::rounded(self.v + o.v, add_up(self.err, o.err))
    }

    fn sub(&self, o: &Self) -> Self {
        self.add(&o.neg())
    }

    /// `|xa·xb − va·vb| ≤ |va|·eb + |vb|·ea + ea·eb`; exactly zero when an
    /// operand is.
    fn mul(&self, o: &Self) -> Self {
        let exact_zero = |a: &Approx| a.err == 0.0 && a.v == 0.0;
        if exact_zero(self) || exact_zero(o) {
            return Approx { v: 0.0, err: 0.0 };
        }
        let carried = add_up(
            add_up(mul_up(self.v.abs(), o.err), mul_up(o.v.abs(), self.err)),
            mul_up(self.err, o.err),
        );
        Approx::rounded(self.v * o.v, carried)
    }

    fn neg(&self) -> Self {
        Approx {
            v: -self.v,
            err: self.err,
        }
    }
}

/// Stands for the lowest set bit of zero, which has none.
const NO_BITS: i32 = i32::MAX;

/// `2¹⁰¹⁶`: every exact-stage value's components stay below it, so sums
/// stay within the expansions' `2¹⁰²¹` with room for the partial sums of a
/// product.
const TOP: f64 = f64::from_bits((1023 + 1016) << 52);

/// `2¹⁰¹⁵`: a product is formed only when its factors' largest components
/// multiply (rounded) to less. Each factor is below twice its largest
/// component, so the product and its partial sums stay below `2¹⁰¹⁸`,
/// within the expansions' range; [`Exact::of`] then tests the result
/// against [`TOP`].
const TOP_PRODUCT: f64 = f64::from_bits((1023 + 1015) << 52);

/// `2⁹⁹⁶`: the expansions' bound on a product's factors.
const SPLIT_MAX: f64 = f64::from_bits((1023 + 996) << 52);

/// The exact stage's number: an expansion, the exponent `lo` of the
/// lowest set bit its components can have (every component a multiple of
/// `2^lo`; `NO_BITS` for zero), the most components any expansion
/// behind it had, and whether every operation stayed in the expansions'
/// range. Out of range, the value is dropped and the evaluation redone in
/// rationals.
#[derive(Clone, Debug)]
pub struct Exact {
    e: Exp,
    lo: i32,
    longest: u32,
    in_range: bool,
}

impl Exact {
    /// `e` from operands with the given `lo` and lengths, checked against
    /// [`TOP`].
    fn of(e: Exp, lo: i32, longest: u32) -> Exact {
        let longest = longest.max(e.components().len() as u32);
        let in_range = top(&e) < TOP;
        Exact {
            e,
            lo,
            longest,
            in_range,
        }
    }

    /// The value given up: out of range.
    fn out(longest: u32) -> Exact {
        Exact {
            e: Exp::zero(),
            lo: NO_BITS,
            longest,
            in_range: false,
        }
    }
}

/// The magnitude of `e`'s largest component, which every other is below.
fn top(e: &Exp) -> f64 {
    e.components().last().map_or(0.0, |x| x.abs())
}

impl Num for Exact {
    fn lit(x: f64) -> Self {
        let x = input(x);
        if !x.is_finite() {
            return Exact::out(0);
        }
        let lo = if x == 0.0 { NO_BITS } else { bit_span(x).0 };
        Exact::of(Exp::from_f64(x), lo, 0)
    }

    fn moved(x: f64, _: [f64; ORDERS]) -> Self {
        Self::lit(x)
    }

    fn moved_by(self, _: &[Self; ORDERS]) -> Self {
        self
    }

    fn add(&self, o: &Self) -> Self {
        let longest = self.longest.max(o.longest);
        if !(self.in_range && o.in_range) {
            return Exact::out(longest);
        }
        Exact::of(&self.e + &o.e, self.lo.min(o.lo), longest)
    }

    fn sub(&self, o: &Self) -> Self {
        self.add(&o.neg())
    }

    /// Formed only in range: the factors' lowest set bits multiply to at
    /// least `2⁻¹⁰⁷⁴`, so every partial product and error term of every
    /// component pair is a representable multiple of that and exact (the
    /// expansion module's argument, with the lowest set bit for the ulp),
    /// the factors are below `2⁹⁹⁶` and their product below `TOP`.
    fn mul(&self, o: &Self) -> Self {
        let longest = self.longest.max(o.longest);
        if !(self.in_range && o.in_range) {
            return Exact::out(longest);
        }
        if self.e.is_zero() || o.e.is_zero() {
            return Exact::of(Exp::zero(), NO_BITS, longest);
        }
        let (ta, tb) = (top(&self.e), top(&o.e));
        let lo = self.lo + o.lo;
        if lo < -1074 || ta >= SPLIT_MAX || tb >= SPLIT_MAX || ta * tb >= TOP_PRODUCT {
            return Exact::out(longest);
        }
        Exact::of(&self.e * &o.e, lo, longest)
    }

    fn neg(&self) -> Self {
        Exact {
            e: -&self.e,
            ..self.clone()
        }
    }
}

/// The rational stage's number: exact, whatever the magnitudes.
#[derive(Clone, Debug)]
pub struct Rat(Rational);

impl Num for Rat {
    /// A non-finite input (a bug, asserted in debug builds) is taken as
    /// zero rather than panicking.
    fn lit(x: f64) -> Self {
        Rat(Rational::try_from(input(x)).unwrap_or_default())
    }

    fn moved(x: f64, _: [f64; ORDERS]) -> Self {
        Self::lit(x)
    }

    fn moved_by(self, _: &[Self; ORDERS]) -> Self {
        self
    }

    fn add(&self, o: &Self) -> Self {
        Rat(&self.0 + &o.0)
    }

    fn sub(&self, o: &Self) -> Self {
        Rat(&self.0 - &o.0)
    }

    fn mul(&self, o: &Self) -> Self {
        Rat(&self.0 * &o.0)
    }

    fn neg(&self) -> Self {
        Rat(-&self.0)
    }
}

/// A value to first order in motion `K` alone: `v + d·ε`, `ε²` dropped,
/// so `d` is the derivative along that motion.
#[derive(Clone, Debug)]
pub struct Dual<N, const K: usize> {
    v: N,
    d: N,
}

impl<N: Num, const K: usize> Dual<N, K> {
    /// `rows[K − 1]`, the rate along motion `K`; a `K` outside
    /// `1..=ORDERS` doesn't compile.
    fn rate<T>(rows: &[T; ORDERS]) -> &T {
        const { assert!(K >= 1 && K <= ORDERS, "a motion is 1..=ORDERS") };
        &rows[K - 1]
    }
}

impl<N: Num, const K: usize> Num for Dual<N, K> {
    fn lit(x: f64) -> Self {
        Dual {
            v: N::lit(x),
            d: N::lit(0.0),
        }
    }

    fn moved(x: f64, rows: [f64; ORDERS]) -> Self {
        Dual {
            v: N::lit(x),
            d: N::lit(*Self::rate(&rows)),
        }
    }

    fn moved_by(self, rows: &[Self; ORDERS]) -> Self {
        Dual {
            v: self.v,
            d: self.d.add(&Self::rate(rows).v),
        }
    }

    fn add(&self, o: &Self) -> Self {
        Dual {
            v: self.v.add(&o.v),
            d: self.d.add(&o.d),
        }
    }

    fn sub(&self, o: &Self) -> Self {
        Dual {
            v: self.v.sub(&o.v),
            d: self.d.sub(&o.d),
        }
    }

    /// `(a + a′ε)(b + b′ε) = ab + (ab′ + a′b)ε`.
    fn mul(&self, o: &Self) -> Self {
        Dual {
            v: self.v.mul(&o.v),
            d: self.v.mul(&o.d).add(&self.d.mul(&o.v)),
        }
    }

    fn neg(&self) -> Self {
        Dual {
            v: self.v.neg(),
            d: self.d.neg(),
        }
    }
}

/// A point or vector as numbers.
pub type V3<N> = [N; 3];

/// The point `p`, unmoved.
pub fn point<N: Num>(p: DVec3) -> V3<N> {
    [p.x, p.y, p.z].map(N::lit)
}

/// The point `p` moved at the rates `rows` (`rows[k − 1]` along motion
/// `k`).
pub fn moved_point<N: Num>(p: DVec3, rows: [DVec3; ORDERS]) -> V3<N> {
    [0, 1, 2].map(|i| N::moved(p[i], rows.map(|r| r[i])))
}

/// `a − b`.
pub fn sub<N: Num>(a: &V3<N>, b: &V3<N>) -> V3<N> {
    [0, 1, 2].map(|i| a[i].sub(&b[i]))
}

/// `a · b`.
pub fn dot<N: Num>(a: &V3<N>, b: &V3<N>) -> N {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

/// `a × b`.
pub fn cross<N: Num>(a: &V3<N>, b: &V3<N>) -> V3<N> {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

/// `det[a, b, c] = (a × b) · c`.
pub fn det3<N: Num>(a: &V3<N>, b: &V3<N>, c: &V3<N>) -> N {
    dot(&cross(a, b), c)
}

/// `det[[a₀, a₁], [b₀, b₁]] = a₀b₁ − a₁b₀`.
pub fn det2<N: Num>(a: [&N; 2], b: [&N; 2]) -> N {
    a[0].mul(b[1]).sub(&a[1].mul(b[0]))
}

#[cfg(test)]
mod tests;

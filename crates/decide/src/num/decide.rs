//! `decide`: the one sign decision of every record, with ties broken by
//! symbolic perturbation.
//!
//! A record's sign question is the sign of
//! `g + δ·(g₁ + η·g₂ + η²·g₃ + η³·g₄)` as `δ` and then `η` go to zero:
//! `g` the record's value (a spatial length, oriented by the caller), `gₖ`
//! its derivative along motion `k` ([`super::exact`]'s motions: the moved
//! element's own direction, then three fixed generic translations of the
//! moving operand). Geometry is never moved; the answer is the sign the
//! moved configuration has, so the answers of all records are realisable
//! together. [`decide`] answers it from the value first:
//!
//! - a value farther from zero than its error plus the [`Band`]'s tie band
//!   is decided by value;
//! - otherwise a **geometric** record (a value that is a certified truth,
//!   never a tie) escalates, and a **curved** one escalates until its error
//!   is at most the rung's target `TAU_SIGN/4^k` ([`rung_target`]);
//! - what is left is a **tie**, decided once, by the first order whose
//!   value is certified non-zero. An order is passed over when it is an
//!   exact zero, or a curved value within its error that has reached the
//!   target (a tie at that order, as at order 0; the ladder's checks, such
//!   as a cyclic crossing order along an edge, catch a configuration such
//!   ties leave unrealisable). A non-zero order not yet at the target
//!   escalates, that record alone.
//! - If every order is passed over, the guard (symbolic perturbation by
//!   operand ids would be the next step) is reached: [`Fail::Internal`],
//!   never answered by a sign. With exact orders this means the record's
//!   motion gradient is zero, a missing perturbation row: a bug. With
//!   curved orders it is also reached when each order is a tie at the
//!   target.
//! - A flat value or order with an input that isn't finite has no sign at
//!   any level, so escalating can't help: [`Fail::Internal`] too (the
//!   kernel's stored data is finite, so this is a caller's bug).
//!
//! **Flat records are exact.** Their value and orders are exact signs from
//! [`super::exact::sign`], which works the orders out lazily, in order;
//! [`exact_orders`] hands them to [`decide`] as [`ExactSign`] coefficients
//! (a value with an input that isn't finite has none: the guard).
//! An exact sign is never in a tie band (bands and `TAU_SIGN` apply to
//! curved values only), so a flat record is a tie only when its value is
//! exactly zero, and only exact zeros are passed over; a flat value that
//! isn't exact escalates. **Curved records**
//! give [`Bounded`] values at their precision level (the first level,
//! double-double or big-float), the orders as an iterator so each is worked
//! out only when reached. Both go through the one function, by the
//! [`Coeff`] trait.
//!
//! [`decide`] never answers zero ([`Sign`] has no zero), and never reads
//! anything but the values: the [`TieKey`] is evidence for the guard only.

use std::cmp::Ordering;

use super::bigf::{BigF, Width};
use super::bounded::{Bounded, Level};
use super::dd::Dd;
use super::exact::{ExactSign, ORDERS, Signs};
use super::expansion::Exp;
use crate::records::keys::RecordKey;

/// A non-zero sign: what [`decide`] answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sign {
    /// Below zero.
    Neg,
    /// Above zero.
    Pos,
}

impl Sign {
    /// The sign of a comparison with zero; `None` for `Equal`.
    pub fn of(o: Ordering) -> Option<Sign> {
        match o {
            Ordering::Less => Some(Sign::Neg),
            Ordering::Equal => None,
            Ordering::Greater => Some(Sign::Pos),
        }
    }

    /// As a comparison with zero: `Less` or `Greater`.
    pub fn ordering(self) -> Ordering {
        match self {
            Sign::Neg => Ordering::Less,
            Sign::Pos => Ordering::Greater,
        }
    }

    /// `−1` or `+1`.
    pub fn as_i8(self) -> i8 {
        match self {
            Sign::Neg => -1,
            Sign::Pos => 1,
        }
    }
}

/// How a record's value is decided: its kind and, for a curved one, its
/// tie band and error target at the current rung of the escalation
/// ladder. Made by [`Band::GEOMETRIC`], [`Band::FLAT`] and
/// [`Band::curved`] only, so a curved band's values are checked: a
/// negative or infinite band, or a target that isn't finite, would make
/// [`decide`] answer signs its errors don't certify.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    kind: BandKind,
    tau: f64,
    tau_sign: f64,
}

/// The kinds of records, by how their values are decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BandKind {
    /// A geometric record (the leaf certificate's edge–edge gap at a
    /// crossing): its value is a truth, never a tie; it escalates until
    /// decided by value.
    Geometric,
    /// A flat record: exact values; only an exact zero is a tie.
    Flat,
    /// A curved record: a value within the tie band `τ_k` plus its error,
    /// once that error is at the target `TAU_SIGN/4^k`, is a tie.
    Curved,
}

impl Band {
    /// A geometric record's band: no tie band, never a tie.
    pub const GEOMETRIC: Band = Band {
        kind: BandKind::Geometric,
        tau: 0.0,
        tau_sign: 0.0,
    };

    /// A flat record's band: no tie band; only exact zeros are ties.
    pub const FLAT: Band = Band {
        kind: BandKind::Flat,
        tau: 0.0,
        tau_sign: 0.0,
    };

    /// A curved record's band at a rung with tie band `tau` (`τ_k`) and
    /// error target `tau_sign` (`TAU_SIGN/4^k`, [`rung_target`]); `None`
    /// unless both are finite and `≥ 0` (a NaN refused too).
    pub fn curved(tau: f64, tau_sign: f64) -> Option<Band> {
        let fine = |x: f64| x.is_finite() && x >= 0.0;
        (fine(tau) && fine(tau_sign)).then_some(Band {
            kind: BandKind::Curved,
            tau,
            tau_sign,
        })
    }

    /// The record's kind.
    pub fn kind(&self) -> BandKind {
        self.kind
    }

    /// `τ_k`, the tie band of curved values at rung `k`: a value within
    /// `τ_k` plus its error is a tie once its error is at the target. Zero
    /// at rungs 0 and 1, and for the other kinds.
    pub fn tau(&self) -> f64 {
        self.tau
    }

    /// The rung's error target, `TAU_SIGN/4^k` ([`rung_target`]): a curved
    /// value, of any order, is a tie only once its error is at most this;
    /// before, it escalates. Zero for the other kinds (an inexact value
    /// handed to a flat or geometric record escalates).
    pub fn tau_sign(&self) -> f64 {
        self.tau_sign
    }
}

/// The error target of curved values at ladder rung `k`, `TAU_SIGN/4^k`:
/// `tau_sign` scaled by `4^−k`, exact while it stays in the normal range
/// (below it, rounded to nearest, the same bits everywhere).
pub fn rung_target(tau_sign: f64, k: u32) -> f64 {
    (0..k).fold(tau_sign, |t, _| t * 0.25)
}

/// What a record's sign is a question about, for evidence: the
/// perturbation row it is an instance of and the record's canonical key
/// (operand ids, never names). [`decide`] reads no part of it for the
/// answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TieKey {
    /// The row.
    pub row: Row,
    /// The record.
    pub record: RecordKey,
}

/// The perturbation rows: every record whose sign [`decide`] gives is an
/// instance of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Row {
    /// The shadow side of a vertex against another vertex's ray, or ahead
    /// or behind along it.
    VertexVertexShadow,
    /// The shadow side of a vertex against a curve.
    VertexCurveShadow,
    /// The height of a column hit on a face (or of a vertex or an edge
    /// the hit lies on exactly), moved against the sheet's tangent plane.
    ColumnHit,
    /// The z-order of two edges at a crossing of their shadows.
    ZOrder,
    /// The z-order of edges along a shared curve.
    SharedCurveZOrder,
    /// A vertex against a box side, the side's points moving with their
    /// face or edge.
    BoxSide,
    /// The order of two crossings along an edge.
    CrossingOrder,
}

/// Why [`decide`] gave no sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// Not decided at this error: refine the value in space, then in
    /// precision, and decide again; past the last precision level or the
    /// budget the operation runs out of budget. At order 0 the whole key
    /// escalates (every record reading the value moves with it); at an
    /// order `≥ 1` the record alone.
    Escalate {
        /// The order that escalates: 0 for the value, `k` for `gₖ`.
        order: usize,
    },
    /// The guard: every order passed over. With exact orders the record's
    /// motion has no first-order effect on it, a missing perturbation
    /// rule, a bug; with curved ones every order may also be a tie at the
    /// target. Also an exact value or order read before that with an input
    /// that isn't finite ([`Coeff::refused`]), a caller's bug no level
    /// resolves. Reported with the record's key.
    Internal {
        /// The record.
        key: TieKey,
    },
}

/// A coefficient [`decide`] reads: the value `g` or an order `gₖ`, exact
/// or with a certified error.
pub trait Coeff {
    /// The sign when the true value is certainly farther from zero than
    /// `band` (`≥ 0`): `|v| > err + band`, compared exactly (no rounding
    /// of the sum); `None` otherwise, and for a value that isn't certified.
    /// An exact sign ignores `band`: bands apply to curved values only.
    fn beyond(&self, band: f64) -> Option<Sign>;

    /// An upper bound of the error; `0` for an exact value, `+∞` or a NaN
    /// for an uncertified one, a value that isn't finite included (either
    /// escalates).
    fn err(&self) -> f64;

    /// Whether the value is exactly zero: proven, not within an error.
    fn is_exact_zero(&self) -> bool;

    /// Whether the value can have no sign at any level (an exact one with
    /// an input that isn't finite): [`decide`] answers [`Fail::Internal`]
    /// rather than escalate. `false` unless a coefficient says so.
    fn refused(&self) -> bool {
        false
    }
}

/// An exact sign, from [`super::exact::sign`]: a flat record's value,
/// whose band is zero. A sign carries no magnitude to compare with a
/// non-zero band (debug builds check); an exact value that needs one is
/// handed over as a [`Bounded`] with zero error.
impl Coeff for Ordering {
    fn beyond(&self, band: f64) -> Option<Sign> {
        debug_assert!(band == 0.0, "an exact sign has no magnitude for a band");
        Sign::of(*self)
    }

    fn err(&self) -> f64 {
        0.0
    }

    fn is_exact_zero(&self) -> bool {
        *self == Ordering::Equal
    }
}

/// An exact sign or none ([`super::exact::NotFinite`]): a flat record's
/// value or order, refused (the guard) when an input it reads isn't
/// finite.
impl Coeff for ExactSign {
    fn beyond(&self, band: f64) -> Option<Sign> {
        self.as_ref().ok().and_then(|s| s.beyond(band))
    }

    fn err(&self) -> f64 {
        match self {
            Ok(_) => 0.0,
            Err(_) => f64::INFINITY,
        }
    }

    fn is_exact_zero(&self) -> bool {
        *self == Ok(Ordering::Equal)
    }

    fn refused(&self) -> bool {
        self.is_err()
    }
}

/// The error of a bounded value, `+∞` when the value isn't finite (never
/// certified, whatever its error parts say).
fn certified_err<T: Level>(b: &Bounded<T>) -> f64 {
    if b.v.is_finite() {
        b.err()
    } else {
        f64::INFINITY
    }
}

/// `err + band` exactly, as the nearest float `s` and the rest `t`
/// (`s + t = err + band`, Knuth's two-sum); `None` when `s` isn't finite:
/// an uncertified error, or a margin past the top float.
fn margin(err: f64, band: f64) -> Option<(f64, f64)> {
    let s = err + band;
    if !s.is_finite() {
        return None;
    }
    let a = s - band;
    let b = s - a;
    Some((s, (err - a) + (band - b)))
}

/// Whether `h + l > s + t` exactly, for `h, s ≥ 0` the nearest floats to
/// `h + l` and `s + t` (`s` finite). `h` a float or more beyond `s` decides
/// (the rests are at most half the spacing on their side); otherwise
/// `h − s` is one spacing or zero, exactly, and the sign of
/// `(h − s) + l − t` decides, in an expansion.
fn exceeds(h: f64, l: f64, s: f64, t: f64) -> bool {
    if h > s.next_up() {
        true
    } else if h < s.next_down() {
        false
    } else {
        let d = &Exp::from_f64(h - s) + &Exp::of_sum(l, -t);
        d.sign() == Ordering::Greater
    }
}

/// The sign of `hi + lo` when `|hi + lo| > err + band` exactly (`hi` the
/// nearest float to the sum); `None` otherwise and for a margin that isn't
/// finite (which no value in `f64`'s range exceeds).
fn pair_beyond(hi: f64, lo: f64, err: f64, band: f64) -> Option<Sign> {
    let (s, t) = margin(err, band)?;
    let neg = hi < 0.0 || (hi == 0.0 && lo < 0.0);
    let (h, l) = if neg { (-hi, -lo) } else { (hi, lo) };
    exceeds(h, l, s, t).then_some(if neg { Sign::Neg } else { Sign::Pos })
}

impl Coeff for Bounded<f64> {
    fn beyond(&self, band: f64) -> Option<Sign> {
        if !self.v.is_finite() {
            return None;
        }
        pair_beyond(self.v, 0.0, self.err(), band)
    }

    fn err(&self) -> f64 {
        certified_err(self)
    }

    fn is_exact_zero(&self) -> bool {
        self.v == 0.0 && Bounded::err(self) == 0.0
    }
}

impl Coeff for Bounded<Dd> {
    fn beyond(&self, band: f64) -> Option<Sign> {
        if !self.v.is_finite() {
            return None;
        }
        pair_beyond(self.v.hi(), self.v.lo(), self.err(), band)
    }

    fn err(&self) -> f64 {
        certified_err(self)
    }

    fn is_exact_zero(&self) -> bool {
        self.v.hi() == 0.0 && Bounded::err(self) == 0.0
    }
}

/// `|v| > err + band` for a big float, exactly: the margin formed exactly
/// (a double-double's sum carries all its bits into a big float; past the
/// `f64` range both terms are near its top, so their sum spans few bits
/// and the widest width holds it), then the sign of `|v|` less it, which
/// rounding to nearest keeps.
fn big_exceeds<B: BigF>(v: &B, err: f64, band: f64) -> bool {
    if band == 0.0 {
        return v.cmp_abs(err) == Some(Ordering::Greater);
    }
    let w = Width::MAX;
    let sum = Dd::sum(err, band);
    let m = if sum.hi().is_finite() {
        B::from_dd(sum, w)
    } else {
        B::from_f64(err, w).add(&B::from_f64(band, w), w).v
    };
    let d = v.abs().sub(&m, w).v;
    d.is_finite() && !d.is_zero() && !d.is_sign_negative()
}

impl<B: BigF> Coeff for Bounded<B> {
    fn beyond(&self, band: f64) -> Option<Sign> {
        let err = self.err();
        if !self.v.is_finite() || !err.is_finite() || !band.is_finite() {
            return None;
        }
        big_exceeds(&self.v, err, band).then_some(if self.v.is_sign_negative() {
            Sign::Neg
        } else {
            Sign::Pos
        })
    }

    fn err(&self) -> f64 {
        certified_err(self)
    }

    fn is_exact_zero(&self) -> bool {
        self.v.is_zero() && Bounded::err(self) == 0.0
    }
}

/// A flat record's coefficients from its exact signs: `g`, then the
/// orders up to the first non-zero one (the ones [`super::exact::sign`]
/// worked out; it stops there, as [`decide`] does), all four zero when
/// every one is.
pub fn exact_orders(signs: &Signs) -> (ExactSign, impl Iterator<Item = ExactSign> + use<>) {
    let (k, s) = signs.first.unwrap_or((ORDERS, Ok(Ordering::Equal)));
    let zero = Ok(Ordering::Equal);
    let g = if k == 0 { s } else { zero };
    let ord = (1..=k).map(move |j| if j == k { s } else { zero });
    (g, ord)
}

/// The sign of `g + δg₁ + δ²g₂ + δ³g₃ + δ⁴g₄` as `δ → 0⁺` (module docs):
/// `g` the record's value, oriented by the caller (`+` = the target above
/// the query, say), `ord` its orders `g₁ … g₄`, each worked out when the
/// loop reaches it. Flat values are exact signs ([`exact_orders`]); curved
/// ones are [`Bounded`] with the error of their precision level. `key` is
/// evidence only.
///
/// The steps, in order:
///
/// 1. the band: `0` for geometric and flat records, `τ_k` for curved ones;
/// 2. `|g| > err + band`: the sign of `g`;
/// 3. otherwise a geometric record escalates (its value is a truth, never
///    a tie), a flat one unless its value is an exact zero, and a curved
///    one whose error is above the rung's target `TAU_SIGN/4^k`;
/// 4. otherwise `g` is a tie: the first order certified non-zero
///    (`|gₖ| > err`) gives the sign; an order that is neither that, nor an
///    exact zero, nor within an error at most the target escalates;
/// 5. every order passed over: the guard, [`Fail::Internal`];
///
/// a value or order that is [`Coeff::refused`] when reached (an exact one
/// with an input that isn't finite) gives the guard at once.
///
/// `ord` is read for at most [`ORDERS`] values, each pulled only once the
/// ones before are passed over (never a pull after the last); the guard
/// is meant to be reached after all of them (debug builds check).
pub fn decide<C: Coeff>(
    g: C,
    ord: impl IntoIterator<Item = C>,
    b: Band,
    key: TieKey,
) -> Result<Sign, Fail> {
    let band = match b.kind {
        BandKind::Geometric => 0.0, // a truth, never a tie
        BandKind::Flat => 0.0,      // exact: only an exact zero is a tie
        BandKind::Curved => b.tau,  // τ_k, 0 at rungs 0 and 1
    };
    if g.refused() {
        return Err(Fail::Internal { key });
    }
    if let Some(s) = g.beyond(band) {
        return Ok(s);
    }
    // At the target: `err ≤ TAU_SIGN/4^k`, false for an uncertified error
    // (`+∞` or a NaN), which escalates whatever the target.
    let at_target = |err: f64| err < f64::INFINITY && err <= b.tau_sign;
    match b.kind {
        BandKind::Geometric => return Err(Fail::Escalate { order: 0 }),
        BandKind::Flat if !g.is_exact_zero() => return Err(Fail::Escalate { order: 0 }),
        BandKind::Curved if !at_target(g.err()) => {
            return Err(Fail::Escalate { order: 0 });
        }
        _ => {} // a tie, decided once
    }
    let mut passed = 0;
    // At most the four orders: none pulled after the last is passed over.
    for (k, t) in (1..).zip(ord.into_iter().take(ORDERS)) {
        if t.refused() {
            return Err(Fail::Internal { key });
        }
        if let Some(s) = t.beyond(0.0) {
            return Ok(s);
        }
        if !t.is_exact_zero() && !at_target(t.err()) {
            return Err(Fail::Escalate { order: k });
        }
        // An exact zero at this order, or a curved tie at it.
        passed = k;
    }
    debug_assert_eq!(passed, ORDERS, "decide reads all {ORDERS} orders");
    Err(Fail::Internal { key }) // no first-order effect: the guard
}

#[cfg(test)]
mod tests;

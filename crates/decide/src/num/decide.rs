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
//!   a record under the **same-surface** band (a height between two
//!   elements on one surface after a snap) likewise, its band `β` at
//!   every rung, beating the geometric kind;
//! - what is left is a **tie**, decided once, by the first order whose
//!   value is certified non-zero. An order is passed over when it is an
//!   exact zero, or a curved value within its error that has reached the
//!   target (a tie at that order, as at order 0; the ladder's checks, such
//!   as a cyclic crossing order along an edge, catch a configuration such
//!   ties leave unrealisable). A non-zero order not yet at the target
//!   escalates, that record alone.
//! - If every order is passed over and at least one of them is a curved
//!   tie, the record is near-degenerate (a small gradient that isn't
//!   zero, say near-parallel curved elements), not a missing rule: it
//!   escalates ([`Fail::NEAR_DEGENERATE`]), the record alone, to a level
//!   whose errors certify an order, or out of budget past the last.
//! - If every order is an exact zero, the guard (symbolic perturbation by
//!   operand ids would be the next step) is reached: [`Fail::Internal`],
//!   never answered by a sign. The record's motion gradient is zero along
//!   all four motions, which are independent, so it is zero: a missing
//!   perturbation row, a bug.
//! - A flat value or order with an input that isn't finite has no sign at
//!   any level, so escalating can't help: [`Fail::Internal`] too (the
//!   kernel's stored data is finite, so this is a caller's bug).
//!
//! **Flat records are exact.** Their value and orders are exact signs from
//! [`super::exact::sign`], which works the orders out lazily, in order;
//! [`exact_orders`] hands them to [`decide`] as [`ExactSign`] coefficients
//! (a value with an input that isn't finite has none: the guard).
//! An exact sign is never in a tie band (`τ_k` and `TAU_SIGN` apply to
//! curved values only), so a flat record is a tie only when its value is
//! exactly zero, and only exact zeros are passed over; a flat value that
//! isn't exact escalates. Under the same-surface band a flat value is
//! compared with `β` exactly, as a rational ([`Coeff`] for
//! [`Rational`]): `|g| > β`, never after rounding; a sign alone has no
//! magnitude there, so only its exact zero is read (any other is the
//! guard, a caller's bug). A record between two members of a
//! storage-scale contact is a tie **by membership** (certified once,
//! against `ρ_flat`): its value is not read, its orders decide
//! ([`decide_tie`]). **Curved records**
//! give [`Bounded`] values at their precision level (the first level,
//! double-double or big-float), the orders as an iterator so each is worked
//! out only when reached. Both go through the one function, by the
//! [`Coeff`] trait.
//!
//! [`decide`] never answers zero ([`Sign`] has no zero), and never reads
//! anything but the values: the [`TieKey`] is evidence for the guard only,
//! and the index a value decided once per key is stored under.
//!
//! **One decision per key.** A value read by several records (a key's
//! order 0) is decided once, by [`decide_value`], under the band of all
//! of its readers' regimes ([`Regime::join`], [`Band::under`]: the
//! same-surface band if any has it, geometric only if all are), at the
//! key's own error target and `τ_k` ([`KeyBands`], the ladder's input per
//! key); each record's orders then decide its tie ([`decide_tie`]).

use std::cmp::Ordering;

use malachite::Rational;

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

/// How a record's value is decided: its kind, its tie band (`τ_k` for a
/// curved record, `β` under the same-surface band) and the error target
/// of its rung. Made by [`Band::GEOMETRIC`], [`Band::FLAT`],
/// [`Band::curved`] and [`Band::same_surface`] only, so a band's values
/// are checked: a negative or infinite band, or a target that isn't
/// finite, would make [`decide`] answer signs its errors don't certify.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    kind: BandKind,
    band: f64,
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
    /// A height-type record between two elements on one surface after a
    /// snap (both on the target): a value within the elements' band `β`
    /// plus its error is a tie, at every rung, a flat value's compared
    /// with `β` exactly. It beats the geometric kind, and its decisions
    /// are proxies at the band's scale ([`Band::proxy`]).
    SameSurface,
}

impl Band {
    /// A geometric record's band: no tie band, never a tie.
    pub const GEOMETRIC: Band = Band {
        kind: BandKind::Geometric,
        band: 0.0,
        tau_sign: 0.0,
    };

    /// A flat record's band: no tie band; only exact zeros are ties.
    pub const FLAT: Band = Band {
        kind: BandKind::Flat,
        band: 0.0,
        tau_sign: 0.0,
    };

    /// A curved record's band at a rung with tie band `tau` (`τ_k`) and
    /// error target `tau_sign` (`TAU_SIGN/4^k`, [`rung_target`]); `None`
    /// unless both are finite and `≥ 0` (a NaN refused too).
    pub fn curved(tau: f64, tau_sign: f64) -> Option<Band> {
        (fine(tau) && fine(tau_sign)).then_some(Band {
            kind: BandKind::Curved,
            band: tau,
            tau_sign,
        })
    }

    /// The same-surface band `beta` (`β`, the larger of the two elements'
    /// bands, each an edge's `β_g` or a vertex's tolerance plus `ρ_op`)
    /// with error target `tau_sign` for a curved value (a flat one is
    /// exact); `None` unless both are finite and `≥ 0`.
    pub fn same_surface(beta: f64, tau_sign: f64) -> Option<Band> {
        (fine(beta) && fine(tau_sign)).then_some(Band {
            kind: BandKind::SameSurface,
            band: beta,
            tau_sign,
        })
    }

    /// The record's kind.
    pub fn kind(&self) -> BandKind {
        self.kind
    }

    /// The tie band a value is compared with: `τ_k` for a curved record,
    /// `β` under the same-surface band, zero for the others.
    pub fn band(&self) -> f64 {
        self.band
    }

    /// `τ_k`, the tie band of curved values at rung `k`: a value within
    /// `τ_k` plus its error is a tie once its error is at the target. Zero
    /// at rungs 0 and 1, and for the other kinds (the same-surface band is
    /// [`band`](Self::band)).
    pub fn tau(&self) -> f64 {
        match self.kind {
            BandKind::Curved => self.band,
            _ => 0.0,
        }
    }

    /// The rung's error target, `TAU_SIGN/4^k` ([`rung_target`]): a curved
    /// value, of any order, is a tie only once its error is at most this;
    /// before, it escalates. Zero for flat and geometric records (an
    /// inexact value handed to one escalates).
    pub fn tau_sign(&self) -> f64 {
        self.tau_sign
    }

    /// Whether a decision under this band is a proxy at the band's scale
    /// (the same-surface band): marked for evidence and the ladder's end
    /// classification, a tracked rule gap where it fails.
    pub fn proxy(&self) -> bool {
        self.kind == BandKind::SameSurface
    }

    /// The band of a key whose value's own band (flat or curved, by its
    /// data) is `self`, read by anchors whose regimes join to `r`
    /// ([`Regime::join`]): geometric only if every anchor is, the
    /// same-surface band if any anchor has it (its target the data's);
    /// `None` for a same-surface band that isn't finite and `≥ 0`.
    pub fn under(self, r: Regime) -> Option<Band> {
        match r {
            Regime::Geometric => Some(Band::GEOMETRIC),
            Regime::Ordinary => Some(self),
            Regime::SameSurface(beta) => Band::same_surface(beta, self.tau_sign),
        }
    }

    /// The regime the band gives a key ([`Band::under`]'s inverse on
    /// kinds): what a later anchor's regime is joined with.
    pub fn regime(&self) -> Regime {
        match self.kind {
            BandKind::Geometric => Regime::Geometric,
            BandKind::Flat | BandKind::Curved => Regime::Ordinary,
            BandKind::SameSurface => Regime::SameSurface(self.band),
        }
    }
}

fn fine(x: f64) -> bool {
    x.is_finite() && x >= 0.0
}

/// What one anchor reading a key asks of the key's order-0 band. A key's
/// value is decided once for every anchor reading it, under the join of
/// their regimes ([`Regime::join`], [`Band::under`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Regime {
    /// The anchor's record is geometric (the leaf certificate's value, a
    /// truth).
    Geometric,
    /// The ordinary rules: flat or curved, by the key's data.
    Ordinary,
    /// Both of the anchor's elements are on one surface after a snap:
    /// the same-surface band `β`.
    SameSurface(f64),
}

impl Regime {
    /// The regime of a key read by anchors of regimes `self` and `other`:
    /// the same-surface band if either has it (the larger `β`), geometric
    /// only if both are, else ordinary. Associative and commutative, with
    /// [`Regime::Geometric`] its identity, so a fold over the anchors in
    /// any order gives the same.
    pub fn join(self, other: Regime) -> Regime {
        use Regime::{Geometric, Ordinary, SameSurface};
        match (self, other) {
            (SameSurface(a), SameSurface(b)) => SameSurface(larger(a, b)),
            (SameSurface(a), _) | (_, SameSurface(a)) => SameSurface(a),
            (Geometric, Geometric) => Geometric,
            _ => Ordinary,
        }
    }
}

/// The larger of two bands, a NaN winning (refused by [`Band::under`]).
fn larger(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if b > a {
        b
    } else {
        a
    }
}

/// The band of curved values at the current rung, per key: the ladder's
/// input. A key's error target (`TAU_SIGN/4^k`) and its `τ_k` are the
/// key's own, as the ladder's precision step and its `τ` rule act on keys
/// (every record sharing one moves with it); the rule choosing them is
/// the ladder's. A curved key's value is decided under it (with its
/// anchors' regimes applied over it, [`Band::under`]), and every curved
/// order of a record reading the key is read at its target; a flat key's
/// value is exact and reads only the target, for a same-surface band. A
/// plain [`Band`] gives every key the same; a function from the key gives
/// each its own. `None` for a key whose band can't be made (a band that
/// isn't finite and `≥ 0`): that key's records fail as the guard.
pub trait KeyBands<K>: Sync {
    /// Key `key`'s curved band at the current rung.
    fn band(&self, key: &K) -> Option<Band>;
}

impl<K> KeyBands<K> for Band {
    fn band(&self, _: &K) -> Option<Band> {
        Some(*self)
    }
}

impl<K, F: Fn(&K) -> Option<Band> + Sync> KeyBands<K> for F {
    fn band(&self, key: &K) -> Option<Band> {
        self(key)
    }
}

/// The error target of curved values at ladder rung `k`, `TAU_SIGN/4^k`:
/// `tau_sign` scaled by `4^−k`, exact while it stays in the normal range
/// (below it, rounded to nearest, the same bits everywhere).
pub fn rung_target(tau_sign: f64, k: u32) -> f64 {
    (0..k).fold(tau_sign, |t, _| t * 0.25)
}

/// What a record's sign is a question about, for evidence: the
/// perturbation row it is an instance of and the record's canonical key:
/// its entities by operand id with their operand side, never names
/// ([`RecordKey`]), inside a snapped zone by the snap's canonical keys
/// ([`RecordKey::Zone`]). It is also the index of a value decided once
/// per key. [`decide`] reads no part of it for the answer.
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
        /// The order that escalates: 0 for the value, `k` for `gₖ`, and
        /// `ORDERS + 1` when every order is passed over, at least one a
        /// curved tie ([`Fail::NEAR_DEGENERATE`]).
        order: usize,
    },
    /// The guard: every order an exact zero, so the record's motion has no
    /// first-order effect on it, a missing perturbation rule, a bug. Also
    /// an exact value or order read before that with an input that isn't
    /// finite ([`Coeff::refused`]), a caller's bug no level resolves.
    /// Reported with the record's key.
    Internal {
        /// The record.
        key: TieKey,
    },
}

impl Fail {
    /// A near-degenerate record: every order passed over, at least one a
    /// curved tie within the target (a gradient too small for this level's
    /// errors, not a zero one). The record escalates, as at an order.
    pub const NEAR_DEGENERATE: Fail = Fail::Escalate { order: ORDERS + 1 };
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

    /// Whether the value carries a magnitude to compare with a band that
    /// isn't zero; an exact sign alone doesn't: under a band, only its
    /// exact zero is read, any other sign is the guard ([`decide`]).
    fn has_magnitude(&self) -> bool {
        true
    }
}

/// An exact sign, from [`super::exact::sign`]: a flat record's value,
/// whose band is zero. A sign carries no magnitude to compare with a
/// non-zero band ([`Coeff::has_magnitude`]); an exact value that needs
/// one is handed over as a rational, or a [`Bounded`] with zero error.
impl Coeff for Ordering {
    fn beyond(&self, band: f64) -> Option<Sign> {
        if band == 0.0 { Sign::of(*self) } else { None }
    }

    fn err(&self) -> f64 {
        0.0
    }

    fn is_exact_zero(&self) -> bool {
        *self == Ordering::Equal
    }

    fn has_magnitude(&self) -> bool {
        false
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

    fn has_magnitude(&self) -> bool {
        false
    }
}

/// An exact rational value: a flat record's value with its magnitude, as
/// the same-surface band compares it (`|g| > β` exactly, `β` a float).
impl Coeff for Rational {
    fn beyond(&self, band: f64) -> Option<Sign> {
        let b = Rational::try_from(band).ok().filter(|_| band >= 0.0)?;
        if *self > b {
            Some(Sign::Pos)
        } else if *self < -b {
            Some(Sign::Neg)
        } else {
            None
        }
    }

    fn err(&self) -> f64 {
        0.0
    }

    fn is_exact_zero(&self) -> bool {
        *self == 0u32
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
/// loop reaches it. Flat values are exact signs ([`exact_orders`]), or
/// exact rationals where a band compares them; curved ones are
/// [`Bounded`] with the error of their precision level. `key` is evidence
/// only.
///
/// The steps, in order ([`decide_value`], then [`decide_tie`]):
///
/// 1. the band: `0` for geometric and flat records, `τ_k` for curved
///    ones, `β` under the same-surface band;
/// 2. `|g| > err + band`, compared exactly: the sign of `g`;
/// 3. otherwise a geometric record escalates (its value is a truth, never
///    a tie), a flat one unless its value is an exact zero, and a curved
///    or same-surface one whose error is above the rung's target
///    `TAU_SIGN/4^k`;
/// 4. otherwise `g` is a tie: the first order certified non-zero
///    (`|gₖ| > err`) gives the sign; an order that is neither that, nor an
///    exact zero, nor within an error at most the target escalates;
/// 5. every order passed over: if one was a curved tie, the record
///    escalates ([`Fail::NEAR_DEGENERATE`]); if all were exact zeros, the
///    guard, [`Fail::Internal`];
///
/// a value or order that is [`Coeff::refused`] when reached (an exact one
/// with an input that isn't finite) gives the guard at once, and so does
/// a non-zero exact sign under a band that isn't zero (no magnitude to
/// compare: a caller's bug).
///
/// `ord` is read for at most [`ORDERS`] values, each pulled only once the
/// ones before are passed over (never a pull after the last); the guard
/// is meant to be reached after all of them (debug builds check).
pub fn decide<G: Coeff, C: Coeff>(
    g: G,
    ord: impl IntoIterator<Item = C>,
    b: Band,
    key: TieKey,
) -> Result<Sign, Fail> {
    match decide_value(&g, b, key)? {
        Some(s) => Ok(s),
        None => decide_tie(ord, b, key), // a tie, decided once
    }
}

/// Whether an error is at the band's target `TAU_SIGN/4^k`: false for an
/// uncertified error (`+∞` or a NaN), which escalates whatever the
/// target.
fn at_target(err: f64, b: Band) -> bool {
    err < f64::INFINITY && err <= b.tau_sign
}

/// The value step of [`decide`] (its steps 1–3), on its own for a value
/// decided once per key: `Some` sign by value, `None` for a tie (the
/// orders decide, per record), or the failure.
pub fn decide_value<G: Coeff>(g: &G, b: Band, key: TieKey) -> Result<Option<Sign>, Fail> {
    let band = b.band; // 0, τ_k or β
    if g.refused() {
        return Err(Fail::Internal { key });
    }
    if band != 0.0 && !g.has_magnitude() && !g.is_exact_zero() {
        // A sign alone can't be compared with a band.
        return Err(Fail::Internal { key });
    }
    if let Some(s) = g.beyond(band) {
        return Ok(Some(s));
    }
    let escalate = Err(Fail::Escalate { order: 0 });
    match b.kind {
        BandKind::Geometric => escalate, // a truth, never a tie
        BandKind::Flat if !g.is_exact_zero() => escalate, // the next exact stage
        BandKind::Curved | BandKind::SameSurface if !at_target(g.err(), b) => escalate,
        _ => Ok(None),
    }
}

/// The orders of a record whose value is a tie (steps 4 and 5 of
/// [`decide`]): the first order certified non-zero gives the sign. Also
/// the whole decision of a record that is a tie by membership (between
/// two members of a storage-scale contact, certified once against
/// `ρ_flat`): its value isn't read. `b` gives the error target the
/// orders are read at.
pub fn decide_tie<C: Coeff>(
    ord: impl IntoIterator<Item = C>,
    b: Band,
    key: TieKey,
) -> Result<Sign, Fail> {
    let mut passed = 0;
    let mut curved_tie = false;
    // At most the four orders: none pulled after the last is passed over.
    for (k, t) in (1..).zip(ord.into_iter().take(ORDERS)) {
        if t.refused() {
            return Err(Fail::Internal { key });
        }
        if let Some(s) = t.beyond(0.0) {
            return Ok(s);
        }
        if !t.is_exact_zero() {
            if !at_target(t.err(), b) {
                return Err(Fail::Escalate { order: k });
            }
            curved_tie = true; // a tie at this order, at the target
        }
        passed = k;
    }
    debug_assert_eq!(passed, ORDERS, "decide reads all {ORDERS} orders");
    if curved_tie {
        return Err(Fail::NEAR_DEGENERATE); // a small gradient, not a zero one
    }
    Err(Fail::Internal { key }) // no first-order effect: the guard
}

#[cfg(test)]
mod tests;

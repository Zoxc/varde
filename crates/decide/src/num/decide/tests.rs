//! `decide` step by step: by value, the geometric and curved escalations
//! at order 0, ties decided by the first certified order, exact zeros and
//! curved ties at the rung's target passed over, escalation of an order
//! above it, a near-degenerate record (every order passed over, one a
//! curved tie) escalating, the guard only when every order is an exact
//! zero; flat records
//! from exact predicates; the same answers at every precision level; and
//! the crossing-order counterexample to passing over curved ties.

use std::cell::Cell;
use std::cmp::Ordering;

use glam::DVec3;

use super::*;
use malachite::Rational as Q;

use crate::ids::{FaceId, VertId};
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::ETA;
use crate::num::exact::{Num, Pred, moved_point, sign};
use crate::num::schedule::PredKind;
use crate::records::keys::{Anchor, HKey, HeightKey, HitIx, Operand, Point, Sheet};

fn key() -> TieKey {
    let q = Point::vert(Operand::A, VertId(3));
    TieKey {
        row: Row::ColumnHit,
        record: RecordKey::HeightAt(HeightKey {
            h: HKey::Hit {
                q,
                f: FaceId(7),
                sheet: Sheet::Tile([0, 0]),
                i: HitIx(0),
            },
            anchor: Anchor::Face(Operand::B, FaceId(7)),
        }),
    }
}

/// A curved band, its values valid.
fn curved(tau: f64, tau_sign: f64) -> Band {
    Band::curved(tau, tau_sign).expect("a valid band")
}

fn exact(v: f64) -> Bounded {
    Bounded::exact(v)
}

/// A value with an error (all of it truncation).
fn approx(v: f64, err: f64) -> Bounded {
    Bounded::new(v, err, 0.0)
}

/// Four exact zero orders.
fn zeros() -> [Bounded; ORDERS] {
    [exact(0.0); ORDERS]
}

/// Orders that must not be read.
fn unread<C>() -> impl Iterator<Item = C> {
    std::iter::from_fn(|| panic!("an order was read"))
}

#[test]
fn a_value_beyond_its_error_and_band_is_decided_by_it() {
    let curved = curved(0.5, 1e-3);
    assert_eq!(
        decide(approx(-1.0, 0.1), unread(), curved, key()),
        Ok(Sign::Neg)
    );
    assert_eq!(
        decide(exact(2.0), unread(), Band::FLAT, key()),
        Ok(Sign::Pos)
    );
    assert_eq!(
        decide(approx(1e-9, 1e-10), unread(), Band::GEOMETRIC, key()),
        Ok(Sign::Pos)
    );
    let (g, ord) = exact_orders(&Signs {
        first: Some((0, Ok(Ordering::Less))),
        work: Default::default(),
    });
    assert_eq!(ord.count(), 0);
    assert_eq!(decide(g, [], Band::FLAT, key()), Ok(Sign::Neg));
}

/// Geometric values are truths: never a tie, the orders never read.
#[test]
fn a_geometric_value_within_its_error_escalates() {
    for g in [exact(0.0), approx(1e-3, 1e-3), approx(0.0, 0.0)] {
        assert_eq!(
            decide(g, unread(), Band::GEOMETRIC, key()),
            Err(Fail::Escalate { order: 0 })
        );
    }
}

/// A curved value within the band plus its error escalates until its error
/// is at the target, then is a tie; a value within `τ_k + err` is a tie
/// even when its error alone would decide it.
#[test]
fn a_curved_value_is_a_tie_only_at_the_target() {
    let band = curved(0.0, 1e-3);
    assert_eq!(
        decide(approx(1e-3, 2e-3), unread(), band, key()),
        Err(Fail::Escalate { order: 0 })
    );
    let ord = [approx(0.5, 0.0)];
    assert_eq!(
        decide(
            approx(1e-4, 1e-3),
            ord.into_iter().chain(unread()),
            band,
            key()
        ),
        Ok(Sign::Pos)
    );
    // In the τ band: |v| > err, but |v| ≤ err + τ.
    let wide = curved(0.5, 1e-3);
    let ord = [approx(-0.5, 0.0)];
    assert_eq!(
        decide(
            approx(0.3, 1e-4),
            ord.into_iter().chain(unread()),
            wide,
            key()
        ),
        Ok(Sign::Neg)
    );
    // Beyond it, by value.
    assert_eq!(
        decide(approx(0.7, 1e-4), unread(), wide, key()),
        Ok(Sign::Pos)
    );
    // An uncertified value escalates whatever its target.
    for g in [approx(0.0, f64::INFINITY), Bounded::new(0.0, f64::NAN, 0.0)] {
        assert_eq!(
            decide(g, unread(), wide, key()),
            Err(Fail::Escalate { order: 0 })
        );
    }
}

/// The orders are read in order, each only once the ones before are
/// passed over: exact zeros, then the first certified non-zero.
#[test]
fn the_first_certified_order_decides_a_tie() {
    for band in [Band::FLAT, curved(0.0, 1e-3)] {
        for k in 0..ORDERS {
            let mut ord = zeros();
            ord[k] = approx(-0.25, 0.125);
            let read = Cell::new(0);
            let lazy = ord.into_iter().inspect(|_| read.set(read.get() + 1));
            assert_eq!(decide(exact(0.0), lazy, band, key()), Ok(Sign::Neg));
            assert_eq!(read.get(), k + 1);
        }
    }
}

/// Orders ≥ 1 pass over curved ties only within the rung's target
/// `TAU_SIGN/4^k`: the same order, error `1e-4`, is passed over at the
/// rungs whose target is at least that and escalates (that record, at
/// that order) at the others.
#[test]
fn curved_orders_pass_over_ties_within_the_rung_target_only() {
    let tau_sign = 1e-3;
    for k in 0..4 {
        let target = rung_target(tau_sign, k);
        assert_eq!(target, tau_sign / f64::from(4u32.pow(k)));
        let band = curved(0.0, target);
        for order in 1..=ORDERS {
            let mut ord = zeros();
            ord[order - 1] = approx(5e-5, 1e-4);
            if order < ORDERS {
                ord[order] = approx(1.0, 0.0);
            }
            let got = decide(exact(0.0), ord, band, key());
            if 1e-4 <= target {
                let want = if order < ORDERS {
                    Ok(Sign::Pos)
                } else {
                    Err(Fail::NEAR_DEGENERATE)
                };
                assert_eq!(got, want, "rung {k}, order {order}");
            } else {
                assert_eq!(
                    got,
                    Err(Fail::Escalate { order }),
                    "rung {k}, order {order}"
                );
            }
        }
    }
}

/// A flat record holds exact values: an inexact order (no target) escalates,
/// an exact zero is passed over.
#[test]
fn flat_orders_pass_over_exact_zeros_only() {
    let ord = [approx(0.0, 1e-300), exact(1.0)];
    assert_eq!(
        decide(exact(0.0), ord, Band::FLAT, key()),
        Err(Fail::Escalate { order: 1 })
    );
    let ord = [exact(0.0), exact(-0.0), exact(1.0)];
    assert_eq!(decide(exact(0.0), ord, Band::FLAT, key()), Ok(Sign::Pos));
}

/// The guard is reached only when every order is an exact zero
/// (`L ≡ 0`), with the record's key. Any certified order decides instead.
#[test]
fn the_guard_fires_only_when_every_order_is_an_exact_zero() {
    for band in [Band::FLAT, curved(0.0, 1e-3), curved(0.5, 1e-3)] {
        assert_eq!(
            decide(exact(0.0), zeros(), band, key()),
            Err(Fail::Internal { key: key() })
        );
        for k in 0..ORDERS {
            let mut ord = zeros();
            ord[k] = exact(1e-300);
            assert_eq!(decide(exact(0.0), ord, band, key()), Ok(Sign::Pos));
        }
    }
}

/// Every order passed over, at least one a curved tie at the target: a
/// near-degenerate record (a gradient too small for the level's errors,
/// not a zero one) escalates, the record alone, after reading all four
/// orders; never the guard. Any mix of ties and exact zeros alike, the
/// tie at any order, under any curved band.
#[test]
fn a_record_whose_orders_are_all_passed_over_with_a_curved_tie_escalates() {
    for band in [curved(0.0, 1e-3), curved(0.5, 1e-3)] {
        for g in [exact(0.0), approx(1e-4, 1e-3)] {
            for mask in 1u32..(1 << ORDERS) {
                let ord: [Bounded; ORDERS] = std::array::from_fn(|k| {
                    if mask & (1 << k) != 0 {
                        approx(1e-5, 1e-4)
                    } else {
                        exact(0.0)
                    }
                });
                let read = Cell::new(0);
                let lazy = ord
                    .into_iter()
                    .inspect(|_| read.set(read.get() + 1))
                    .chain(unread());
                assert_eq!(
                    decide(g, lazy, band, key()),
                    Err(Fail::NEAR_DEGENERATE),
                    "ties {mask:04b}"
                );
                assert_eq!(read.get(), ORDERS);
            }
            // All exact zeros, the value a tie too: the guard.
            assert_eq!(
                decide(g, zeros(), band, key()),
                Err(Fail::Internal { key: key() })
            );
        }
    }
    assert_eq!(Fail::NEAR_DEGENERATE, Fail::Escalate { order: ORDERS + 1 });
}

/// Near-parallel curved elements: the value is a tie (zero within its
/// error) and the gradient along every motion is tiny but not zero,
/// `1e-9` at order 1. At a coarse level each order's error (`1e-8`) is
/// within the target, so every order is a tie and the record escalates;
/// at a finer level, the same rung, the errors shrink below the gradient
/// and order 1 decides, at every precision type.
#[test]
fn a_near_parallel_curved_record_decides_at_a_finer_level() {
    let band = curved(0.0, 1e-6);
    let w = Width::from_bits(212).expect("a width");
    let rows = [1e-9, -3e-10, 2e-10, -1e-10];
    let at = |err: f64| -> (Bounded, [Bounded; ORDERS]) {
        (approx(0.0, err), rows.map(|r| approx(r, err)))
    };
    let (g, ord) = at(1e-8);
    assert_eq!(decide(g, ord, band, key()), Err(Fail::NEAR_DEGENERATE));
    let dd = |x: Bounded| -> Bounded<Dd> { x.promote() };
    assert_eq!(
        decide(dd(g), ord.map(dd), band, key()),
        Err(Fail::NEAR_DEGENERATE)
    );
    let (g, ord) = at(1e-12);
    assert_eq!(decide(g, ord, band, key()), Ok(Sign::Pos));
    assert_eq!(decide(dd(g), ord.map(dd), band, key()), Ok(Sign::Pos));
    let big = |x: Bounded| -> Bounded<Big> { x.promote().promote().widen(w) };
    assert_eq!(decide(big(g), ord.map(big), band, key()), Ok(Sign::Pos));
    // The gradient's sign at the first certified order, whatever the
    // later ones say: order 1 a tie even here, order 2 decides.
    let (g, mut ord) = at(1e-12);
    ord[0] = approx(1e-13, 1e-12);
    assert_eq!(decide(g, ord, band, key()), Ok(Sign::Neg));
}

/// A point against the plane `z = 0`: its height, the point moving at
/// `rows` along the motions.
struct Above {
    q: DVec3,
    rows: [DVec3; ORDERS],
}

impl Pred for Above {
    fn eval<N: Num>(&self) -> N {
        let [_, _, z] = moved_point::<N>(self.q, self.rows);
        z
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

/// Flat records through `exact::sign`: decided by value, at the first
/// non-zero order, and the guard when the motion doesn't move the point
/// off the plane (`L ≡ 0`).
#[test]
fn flat_records_take_their_signs_from_the_exact_predicates() {
    let flat = |q: DVec3, rows: [DVec3; ORDERS]| {
        let signs = sign(&Above { q, rows });
        let (g, ord) = exact_orders(&signs);
        decide(g, ord, Band::FLAT, key())
    };
    let rows = [DVec3::X, DVec3::new(0.0, 1.0, -0.5), DVec3::Z, DVec3::Z];
    assert_eq!(flat(DVec3::new(1.0, 2.0, 1e-30), rows), Ok(Sign::Pos));
    // On the plane, the own direction tangent to it: T₂ decides.
    assert_eq!(flat(DVec3::new(1.0, 2.0, 0.0), rows), Ok(Sign::Neg));
    // Every motion in the plane: no first-order effect.
    let along = [DVec3::X, DVec3::Y, DVec3::X, DVec3::Y];
    assert_eq!(
        flat(DVec3::new(1.0, 2.0, 0.0), along),
        Err(Fail::Internal { key: key() })
    );
    let (g, ord) = exact_orders(&sign(&Above {
        q: DVec3::ZERO,
        rows: along,
    }));
    assert_eq!((g, ord.count()), (Ok(Ordering::Equal), ORDERS));
}

/// A flat record with an input that isn't finite has no sign at any
/// level: the guard at the order that reads it, in every build, never an
/// escalation.
#[test]
fn a_flat_record_with_an_input_that_isnt_finite_is_refused() {
    let flat = |q: DVec3, rows: [DVec3; ORDERS]| {
        let (g, ord) = exact_orders(&sign(&Above { q, rows }));
        decide(g, ord, Band::FLAT, key())
    };
    let rows = [DVec3::X, DVec3::Y, DVec3::Z, DVec3::Z];
    for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert_eq!(
            flat(DVec3::new(1.0, 2.0, bad), rows),
            Err(Fail::Internal { key: key() })
        );
        let mut moved = rows;
        moved[0].z = bad;
        assert_eq!(
            flat(DVec3::new(1.0, 2.0, 0.0), moved),
            Err(Fail::Internal { key: key() })
        );
        // Decided by value, the rates never enter.
        assert_eq!(flat(DVec3::new(1.0, 2.0, -3.0), moved), Ok(Sign::Neg));
    }
}

/// A flat value is a tie only when it is an exact zero: an inexact one
/// within its error escalates, its orders unread.
#[test]
fn a_flat_value_that_is_not_exact_escalates() {
    for g in [approx(0.0, 1e-300), approx(1e-3, 1e-3)] {
        assert_eq!(
            decide(g, unread(), Band::FLAT, key()),
            Err(Fail::Escalate { order: 0 })
        );
    }
}

/// A value that isn't finite is never certified, whatever its error: it
/// escalates, at order 0 and at an order alike, even at a zero error.
#[test]
fn a_value_that_is_not_finite_escalates() {
    let band = curved(0.0, 1e-3);
    for x in [f64::NAN, f64::INFINITY] {
        let x = approx(x, 0.0);
        assert_eq!(
            decide(x, unread(), band, key()),
            Err(Fail::Escalate { order: 0 })
        );
        assert_eq!(
            decide(exact(0.0), [x].into_iter().chain(unread()), band, key()),
            Err(Fail::Escalate { order: 1 })
        );
    }
}

/// A record's value, orders and band, and the answer.
type Case = (Bounded, [Bounded; ORDERS], Band, Result<Sign, Fail>);

/// The same answers from double-double and big-float values.
#[test]
fn every_level_decides_alike() {
    let w = Width::from_bits(212).expect("a width");
    let cases: [Case; 5] = [
        (approx(-1.0, 0.1), zeros(), curved(0.5, 1e-3), Ok(Sign::Neg)),
        (
            approx(0.3, 1e-4),
            [approx(-0.5, 0.0); ORDERS],
            curved(0.5, 1e-3),
            Ok(Sign::Neg),
        ),
        (
            approx(0.0, 1e-2),
            zeros(),
            curved(0.0, 1e-3),
            Err(Fail::Escalate { order: 0 }),
        ),
        (
            exact(0.0),
            [
                approx(1e-5, 1e-4),
                approx(0.0, 1e-2),
                exact(0.0),
                exact(0.0),
            ],
            curved(0.0, 1e-3),
            Err(Fail::Escalate { order: 2 }),
        ),
        (
            exact(0.0),
            zeros(),
            Band::FLAT,
            Err(Fail::Internal { key: key() }),
        ),
    ];
    for (g, ord, band, want) in cases {
        assert_eq!(decide(g, ord, band, key()), want);
        let dd = |x: Bounded| -> Bounded<Dd> { x.promote() };
        assert_eq!(decide(dd(g), ord.map(dd), band, key()), want);
        let big = |x: Bounded| -> Bounded<Big> { x.promote().promote().widen(w) };
        assert_eq!(decide(big(g), ord.map(big), band, key()), want);
    }
}

/// Passing over curved ties: three crossings on one curved edge at one
/// parameter (order 0 exact zeros), crossing-order records
/// `a = t₂ − t₁`, `b = t₃ − t₂`, `c = t₁ − t₃`, whose true coefficients sum
/// to zero at every order. True order-1 values `1e-4, 1e-4, −2e-4`: `a`
/// and `b` evaluate to `0 ± 1e-4`, within the target `TAU_SIGN = 1e-3`,
/// so they are ties at order 1 and order 2 (`−1, −1`) decides them; `c`
/// is `−2e-4 ± 1e-5`, decided at order 1. All three answer `−`, a cyclic
/// order true of no configuration (the true signs are `+, +, −`): passing
/// over a curved tie leaves realisability to the ladder, whose
/// cyclic-crossing-order check fires on exactly this. An order not at the
/// target escalates instead.
#[test]
fn passed_over_curved_ties_can_give_a_cyclic_crossing_order() {
    let band = curved(0.0, 1e-3);
    let a = decide(exact(0.0), [approx(0.0, 1e-4), exact(-1.0)], band, key());
    let c = decide(exact(0.0), [approx(-2e-4, 1e-5), exact(2.0)], band, key());
    assert_eq!((a, a, c), (Ok(Sign::Neg), Ok(Sign::Neg), Ok(Sign::Neg)));
    let fine = curved(0.0, rung_target(1e-3, 2));
    assert_eq!(
        decide(exact(0.0), [approx(0.0, 1e-4), exact(-1.0)], fine, key()),
        Err(Fail::Escalate { order: 1 })
    );
}

#[test]
fn signs_convert() {
    for (s, o, i) in [
        (Sign::Neg, Ordering::Less, -1),
        (Sign::Pos, Ordering::Greater, 1),
    ] {
        assert_eq!(Sign::of(o), Some(s));
        assert_eq!((s.ordering(), s.as_i8()), (o, i));
    }
    assert_eq!(Sign::of(Ordering::Equal), None);
}

/// The guard after the fourth order is passed over: no fifth pull, which
/// a lazy iterator would answer by working out an order that isn't one.
#[test]
fn the_guard_pulls_no_fifth_order() {
    for band in [Band::FLAT, curved(0.0, 1e-3)] {
        let ord = zeros().into_iter().chain(unread());
        assert_eq!(
            decide(exact(0.0), ord, band, key()),
            Err(Fail::Internal { key: key() })
        );
    }
}

/// The model's coefficient: the exact value, its error, and whether it is
/// exact (`err = 0`); `None` for a value that isn't certified (not
/// finite).
#[derive(Clone, Debug)]
struct Model {
    v: Q,
    err: Q,
    exact: bool,
}

/// The model's answer, with the order an escalation is at.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Res {
    Ok(i8),
    Escalate(usize),
    Internal,
}

fn qabs(x: &Q) -> Q {
    if *x < 0u32 { -x.clone() } else { x.clone() }
}

fn qsgn(x: &Q) -> i8 {
    if *x < 0u32 { -1 } else { 1 }
}

/// The model's `decide`, transcribed: `decideOrd` over the orders, then
/// `decideP`, plus the intended differences (an uncertified value
/// escalates at its order; a flat value within its error that isn't an
/// exact zero escalates; every order passed over with a curved tie among
/// them escalates, the guard only for exact zeros). Returns the answer
/// and the orders read.
fn model(g: &Option<Model>, ord: &[Option<Model>], b: Band) -> (Res, usize) {
    let tau = Q::try_from(b.tau).expect("finite τ");
    let tau_sign = Q::try_from(b.tau_sign).expect("finite target");
    let Some(g) = g else {
        return (Res::Escalate(0), 0);
    };
    let band = match b.kind {
        BandKind::Curved => tau,
        BandKind::Geometric | BandKind::Flat => Q::from(0u32),
    };
    if g.err.clone() + band < qabs(&g.v) {
        return (Res::Ok(qsgn(&g.v)), 0);
    }
    let zero = |t: &Model| t.exact && t.v == 0u32;
    match b.kind {
        BandKind::Geometric => return (Res::Escalate(0), 0),
        BandKind::Curved if tau_sign < g.err => return (Res::Escalate(0), 0),
        BandKind::Flat if !zero(g) => return (Res::Escalate(0), 0),
        _ => {}
    }
    let mut curved_tie = false;
    for (i, t) in ord.iter().enumerate() {
        let k = i + 1;
        let Some(t) = t else {
            return (Res::Escalate(k), k);
        };
        if t.err < qabs(&t.v) {
            return (Res::Ok(qsgn(&t.v)), k);
        }
        if !zero(t) {
            if tau_sign < t.err {
                return (Res::Escalate(k), k);
            }
            curved_tie = true;
        }
    }
    if curved_tie {
        return (Res::Escalate(ORDERS + 1), ord.len());
    }
    (Res::Internal, ord.len())
}

/// A coefficient of any level as the model sees it.
fn as_model<C: Coeff>(c: &C, v: Option<Q>) -> Option<Model> {
    let err = c.err();
    let v = v?;
    if !err.is_finite() {
        return None;
    }
    Some(Model {
        v,
        err: Q::try_from(err).expect("finite"),
        exact: err == 0.0,
    })
}

/// Random draws aimed at the comparisons' edges.
struct Draw(varde_testkit::rng::Rng);

impl Draw {
    fn below(&mut self, n: usize) -> usize {
        (self.0.next_u64() % n as u64) as usize
    }

    /// A finite magnitude: zero, subnormals, powers of two, typical
    /// sizes, anything.
    fn magnitude(&mut self) -> f64 {
        match self.below(6) {
            0 => 0.0,
            1 => f64::from_bits(self.0.next_u64() >> 12),
            2 => f64::from_bits(((self.below(2046) + 1) as u64) << 52),
            3 => self.0.log_range(1e-12, 1e3),
            4 => [ETA, f64::MIN_POSITIVE, 1e-3, 1.0, f64::MAX][self.below(5)],
            _ => f64::from_bits(self.0.next_u64() % 0x7FF0_0000_0000_0000),
        }
    }

    /// A target, `τ_k` or `TAU_SIGN/4^k`.
    fn band(&mut self) -> f64 {
        match self.below(3) {
            0 => 0.0,
            _ => self.magnitude(),
        }
    }

    /// An error near the target `t`, or anything.
    fn err(&mut self, t: f64) -> f64 {
        match self.below(9) {
            0 | 1 => 0.0,
            2 => t,
            3 => t.next_up(),
            4 => {
                if t > 0.0 {
                    t.next_down()
                } else {
                    0.0
                }
            }
            5 => f64::INFINITY,
            6 if self.below(4) == 0 => f64::NAN,
            _ => self.magnitude(),
        }
    }

    /// A value near `err` and `err + band`, or anything, of either sign.
    fn value(&mut self, err: f64, band: f64) -> f64 {
        let s = err + band;
        let m = match self.below(12) {
            0 => 0.0,
            1 => err,
            2 => err.next_up(),
            3 => err.next_down(),
            4 => s,
            5 => s.next_up(),
            6 => s.next_down(),
            7 => s.next_up().next_up(),
            8 if self.below(4) == 0 => [f64::INFINITY, f64::NAN][self.below(2)],
            _ => self.magnitude(),
        };
        if self.below(2) == 0 { m } else { -m }
    }

    /// A low part for a double-double over `hi`: none, the rounding error
    /// of `err + band`, or a little of either sign.
    fn low(&mut self, hi: f64, err: f64, band: f64) -> f64 {
        let ulp = (hi.abs().next_up() - hi.abs()) * 0.25;
        let m = match self.below(5) {
            0 => 0.0,
            1 => ETA,
            2 if ulp.is_finite() => ulp,
            3 => {
                let s = err + band;
                (err - (s - band)) + (band - (s - (s - band)))
            }
            _ => self.magnitude() * 1e-300,
        };
        if self.below(2) == 0 { m } else { -m }
    }

    fn kind(&mut self) -> BandKind {
        [BandKind::Geometric, BandKind::Flat, BandKind::Curved][self.below(3)]
    }

    /// An error split into the two parts, or whole.
    fn bounded<T>(&mut self, v: T, err: f64) -> Bounded<T> {
        if self.below(3) == 0 && err.is_finite() {
            Bounded::new(v, err * 0.5, err * 0.5)
        } else {
            Bounded::new(v, err, 0.0)
        }
    }
}

/// Runs `decide` on `g` and the four orders against the model; the
/// orders are counted and never pulled past what the model reads.
fn agree<C: Coeff + Clone + std::fmt::Debug>(
    g: (C, Option<Q>),
    ord: [(C, Option<Q>); ORDERS],
    b: Band,
) {
    let gm = as_model(&g.0, g.1.clone());
    let om: Vec<_> = ord.iter().map(|(c, v)| as_model(c, v.clone())).collect();
    let (want, reads) = model(&gm, &om, b);
    let read = Cell::new(0);
    let lazy = ord
        .iter()
        .map(|(c, _)| c.clone())
        .inspect(|_| read.set(read.get() + 1))
        .chain(unread());
    let got = match decide(g.0.clone(), lazy, b, key()) {
        Ok(s) => Res::Ok(s.as_i8()),
        Err(Fail::Escalate { order }) => Res::Escalate(order),
        Err(Fail::Internal { key: k }) => {
            assert_eq!(k, key());
            Res::Internal
        }
    };
    assert_eq!(
        (got, read.get()),
        (want, reads),
        "g {g:?}, orders {ord:?}, band {b:?}"
    );
}

/// `decide` against a transcription of the model, at every level, on
/// draws at the edges of every comparison.
#[test]
fn decide_agrees_with_the_model() {
    let w = Width::from_bits(212).expect("a width");
    let q = |x: f64| Q::try_from(x).ok();
    let qd = |x: Dd| Some(q(x.hi())? + q(x.lo())?);
    let qb = |x: &Big| Q::try_from(x.as_float()).ok();
    for seed in varde_testing::seeds(4, 64) {
        let mut d = Draw(varde_testkit::rng::Rng::new(seed));
        for _ in 0..4000 {
            let kind = d.kind();
            let (tau, tau_sign) = (d.band(), d.band());
            let b = match kind {
                BandKind::Geometric => Band::GEOMETRIC,
                BandKind::Flat => Band::FLAT,
                BandKind::Curved => curved(tau, tau_sign),
            };
            let (band, tau_sign) = (b.tau(), b.tau_sign());
            // Order 0 a tie half the time, so the orders are reached.
            let mut raw = [(0.0, 0.0, 0.0); ORDERS + 1];
            for (i, r) in raw.iter_mut().enumerate() {
                let tie = i == 0 && d.below(2) == 0;
                let err = if tie { 0.0 } else { d.err(tau_sign) };
                let bd = if i == 0 { band } else { 0.0 };
                let v = if tie { 0.0 } else { d.value(err, bd) };
                *r = (v, err, d.low(v, err, bd));
            }
            let bounded: Vec<_> = raw.iter().map(|&(v, e, _)| d.bounded(v, e)).collect();
            let f = |i: usize| (bounded[i], q(bounded[i].v));
            agree(f(0), [f(1), f(2), f(3), f(4)], b);
            let dds: Vec<_> = raw
                .iter()
                .zip(&bounded)
                .map(|(&(v, _, lo), x)| Bounded::new(Dd::sum(v, lo), x.err_trunc, x.err_round))
                .collect();
            let f = |i: usize| (dds[i], qd(dds[i].v));
            agree(f(0), [f(1), f(2), f(3), f(4)], b);
            let scale = [1.0, p2(750), p2(-750)][d.below(3)];
            let s = Big::from_f64(scale, w);
            let bigs: Vec<_> = dds
                .iter()
                .map(|x| {
                    let v = Big::from_dd(x.v, w);
                    let v = if scale == 1.0 {
                        v
                    } else {
                        v.mul(&s, w).v.mul(&s, w).v
                    };
                    Bounded::new(v, x.err_trunc, x.err_round)
                })
                .collect();
            let f = |i: usize| (bigs[i].clone(), qb(&bigs[i].v));
            agree(f(0), [f(1), f(2), f(3), f(4)], b);
        }
    }
}

/// `|v| > err + band` is compared exactly at every level, the sum not
/// rounded: a value a float past the margin is decided by it, one at the
/// margin isn't (a tie, decided here by an order of the other sign).
#[test]
fn a_value_just_past_its_error_and_band_is_decided_by_it() {
    let w = Width::MIN;
    let ord = || [exact(-1.0)];
    let wide = |tau| curved(tau, f64::MAX);
    // f64: `1 + 2⁻⁵³` is a tie, rounded to 1 (rest `+2⁻⁵³`);
    // `1 + 3·2⁻⁵³` rounds to `1 + 2⁻⁵¹` (rest `−2⁻⁵³`).
    let h = p2(-53);
    let cases = [
        (1.0, h, 1.0, false),
        (1.0, h, 1.0f64.next_up(), true),
        (1.0, 3.0 * h, 1.0f64.next_up().next_up(), true),
        (1e-3, 0.5, (1e-3f64 + 0.5).next_up(), true),
    ];
    let big = |x: Bounded| -> Bounded<Big> { x.promote().promote().widen(w) };
    for (err, tau, v, beyond) in cases {
        let want = Ok(if beyond { Sign::Pos } else { Sign::Neg });
        let g = approx(v, err);
        assert_eq!(decide(g, ord(), wide(tau), key()), want);
        let dd = |x: Bounded| -> Bounded<Dd> { x.promote() };
        assert_eq!(decide(dd(g), ord().map(dd), wide(tau), key()), want);
        assert_eq!(decide(big(g), ord().map(big), wide(tau), key()), want);
    }
    // Double-double: a low part past the error decides, at a zero band.
    let err = 1e-3f64;
    for (hi, lo) in [(err, ETA), (err.next_up(), -ETA)] {
        let v = Bounded::new(Dd::sum(hi, lo), err, 0.0);
        assert_eq!(
            decide(v, [exact(-1.0).promote()], wide(0.0), key()),
            Ok(Sign::Pos)
        );
    }
    let v = Bounded::new(Dd::sum(err, -ETA), err, 0.0);
    assert_eq!(
        decide(v, [exact(-1.0).promote()], wide(0.0), key()),
        Ok(Sign::Neg)
    );
    // Big floats past the `f64` range, against a margin past it too.
    let s = Big::from_f64(p2(1000), w);
    let huge = Bounded::new(s.mul(&s, w).v, f64::MAX, 0.0);
    let ord = [big(exact(-1.0))];
    assert_eq!(decide(huge, ord, wide(f64::MAX), key()), Ok(Sign::Pos));
    // A margin whose bits span past the widest width, `1 + 2⁻¹⁰⁰⁰`: a
    // value at it is a tie, one past it is decided.
    let tau = p2(-1000);
    for (lo, beyond) in [(tau, false), (p2(-999), true)] {
        let want = Ok(if beyond { Sign::Pos } else { Sign::Neg });
        let v = Bounded::new(Big::from_dd(Dd::sum(1.0, lo), Width::MAX), 1.0, 0.0);
        let ord = [big(exact(-1.0))];
        assert_eq!(decide(v, ord, wide(tau), key()), want);
    }
}

/// An uncertified value escalates even at the largest target: its error
/// is no error at the target, so it is never a tie.
#[test]
fn an_uncertified_value_escalates_at_an_unbounded_target() {
    let band = curved(0.0, f64::MAX);
    for x in [
        approx(f64::NAN, 0.0),
        approx(f64::INFINITY, 0.0),
        approx(0.0, f64::INFINITY),
        approx(0.0, f64::NAN),
    ] {
        assert_eq!(
            decide(x, unread(), band, key()),
            Err(Fail::Escalate { order: 0 })
        );
        let ord = [x, exact(1.0)];
        assert_eq!(
            decide(exact(0.0), ord, band, key()),
            Err(Fail::Escalate { order: 1 })
        );
    }
}

/// A curved band is made only with a finite, non-negative tie band and
/// target: a negative band would answer a sign whose error covers zero,
/// an infinite target would pass over any finite error as a tie.
#[test]
fn a_curved_band_refuses_negative_or_infinite_values() {
    for (tau, tau_sign) in [
        (-1.0, 1e-3),
        (0.0, -1e-3),
        (f64::INFINITY, 1e-3),
        (0.0, f64::INFINITY),
        (f64::NAN, 1e-3),
        (0.0, f64::NAN),
    ] {
        assert_eq!(Band::curved(tau, tau_sign), None, "{tau} {tau_sign}");
    }
    let b = curved(0.5, 1e-3);
    assert_eq!(
        (b.kind(), b.tau(), b.tau_sign()),
        (BandKind::Curved, 0.5, 1e-3)
    );
    assert_eq!(curved(-0.0, 0.0).kind(), BandKind::Curved);
    for b in [Band::GEOMETRIC, Band::FLAT] {
        assert_eq!((b.tau(), b.tau_sign()), (0.0, 0.0));
    }
}

/// `2ᵏ` for `k` in the normal range, exactly.
fn p2(k: i32) -> f64 {
    f64::from_bits(u64::from((1023 + k) as u32) << 52)
}

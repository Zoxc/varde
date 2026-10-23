//! Storage: the `f64` values a body keeps, what rounding to them costs,
//! the checks an operation makes on its operands when it starts, the
//! operation's one rounding bound `ρ_op` and the local frames curved
//! evaluations run in.
//!
//! **Rounding once.** A computed value is rounded to `f64` once, when it
//! is stored ([`store`], [`store_exact`], [`store_point`]): to the
//! nearest `f64`, ties to even for an exact value. The result carries a
//! bound on its distance from the true value ([`Stored::err`]: the
//! value's own certified error plus the rounding), which the caller adds
//! to the certificate the value belongs to (a seam bound, a vertex
//! tolerance, a snap displacement). Nothing decides on it again: records
//! read only stored values. Rounding to nearest is monotone, so a strict
//! order of stored scalars never flips; it can only become a tie. Orders
//! read from stored geometry (rotation orders, containment, facing,
//! radial sorts) are compared with the certified unrounded ones
//! ([`compare_orders`]); a mismatch is the realisation repair's to settle,
//! not done here.
//!
//! **The storage floor.** [`eps_min`]`(x) = 2⁶·ulp(x)` is the least
//! tolerance storable at magnitude `x`, [`ulp`]`(x) = 2^(⌊log₂|x|⌋ − 52)`
//! the spacing of the `f64`s in `|x|`'s binade (the upper one at a power
//! of two). A tolerance a result needs below the floor at its magnitude
//! is not an error: it is rounded like any other, and the bound the
//! result keeps is the target plus the rounding, finite
//! ([`achieved`]), flagged so the operation warns and marks its result
//! approximate. There is one behaviour, no policy choosing between
//! refusing and rounding.
//!
//! **Bounded positions.** [`MAX_COORD`] bounds the points a body's
//! geometry is bounded by: vertex positions, the control points of space
//! curves (every one, middle points included) and the key points of
//! forms (origins, feet, centres) that lie inside the body's box. Every
//! other stored value (tile control points and weights, radii, distances,
//! key points outside the box, tolerances, parameters) need only be
//! finite: a form's lattice reaches past `MAX_COORD` by design, and its
//! size costs precision, never storability.
//!
//! **Operation start** ([`OperandScan`]). An operation scans its operands
//! once: a stored value not finite, or a bounded position above
//! `MAX_COORD`, is [`Unrepresentable::OutOfRange`]; the tolerance `ε`
//! below `eps_min(MAX_COORD) = 2⁻²⁷` is [`Unrepresentable::Tolerance`].
//! These two, with a repair that fails, are the only ways a result is
//! unrepresentable; each carries its evidence (the value and its
//! magnitude, or `ε` and the floor). The scan's largest magnitude `M`
//! over the bounded positions fixes `ρ_op`. The scan is a charged loop
//! of the operation's work: one step per stored position and per tile
//! net it visits, at the unit table's operand-scan row ([`ScanSize`]).
//!
//! **`ρ_op`** ([`rho_op`]): the first level's rounding bound of a seam
//! evaluation (end weights about 1, no frame) at `M`: [`SEAM_OPS`]
//! rounded operations of unit roundoff `u = 2⁻⁵³`, `ρ_op = 50·u·M`,
//! rounded up once. Every band, near radius and key of the operation uses
//! it ([`crate::records::near::OpBands`]), never a level's own rounding,
//! so the keys are the same at every level.
//!
//! **Local frames** ([`frame_centre`], [`translate`]). A curved
//! evaluation runs relative to its box's centre, rounded to a grid of a
//! power of two no wider than a quarter of the box, so the centre stays
//! inside the box and has few significant bits. The translation `x − c`
//! is formed exactly and rounded once; its rounding, at most `u·|x − c|`,
//! is the value's only error, and is zero when the difference is a float
//! (always when `x` and `c` are within a factor 2 of each other, or `c`
//! is 0).

use std::ops::RangeInclusive;

use malachite::Rational;
use malachite::base::num::arithmetic::traits::Abs;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::bounded::{Bounded, U, add_up, mul_up, up};
use super::exact::rat;
use super::expansion::two_sum;
use super::level::LevelScalar;
use super::schedule::{UNIT_PARTS, UNITS};

/// The largest coordinate magnitude of a body's bounded positions (mm).
pub const MAX_COORD: f64 = 1e6;

/// The range of the document's tolerance `ε` (mm). Every `ε` in it is
/// at least the storage floor at [`MAX_COORD`].
pub const EPS_RANGE: RangeInclusive<f64> = 1e-5..=1e-1;

/// [`eps_min`] at [`MAX_COORD`]: `2⁶·2⁻³³ = 2⁻²⁷`, about `7.5·10⁻⁹` mm.
pub const EPS_MIN_MAX_COORD: f64 = 1.0 / 134_217_728.0;

/// The rounded operations of a seam evaluation chain that `ρ_op` counts.
pub const SEAM_OPS: f64 = 50.0;

/// The spacing of the `f64`s in `|x|`'s binade, `2^(⌊log₂|x|⌋ − 52)`
/// (the upper binade at a power of two); for `|x|` below the normal
/// range, the subnormal spacing `2⁻¹⁰⁷⁴`. `None` unless `x` is finite.
pub fn ulp(x: f64) -> Option<f64> {
    if !x.is_finite() {
        return None;
    }
    // The biased exponent; 0 (zeros and subnormals) spaces as 1 does.
    let e = ((x.abs().to_bits() >> 52) & 0x7ff).max(1);
    Some(if e > 52 {
        // `2^(e − 1075)`, a normal number.
        f64::from_bits((e - 52) << 52)
    } else {
        // `2^(e − 1075) = 2^(e − 1)·2⁻¹⁰⁷⁴`, a subnormal one.
        f64::from_bits(1u64 << (e - 1))
    })
}

/// The least tolerance storable at coordinate magnitude `x`,
/// `2⁶·`[`ulp`]`(x)`. `None` unless `x` is finite.
pub fn eps_min(x: f64) -> Option<f64> {
    // A power of two at most `2⁹⁷¹·2⁶`: exact.
    ulp(x).map(|s| s * 64.0)
}

/// `ρ_op = up(50·u·M)` for the largest magnitude `m` of an operation's
/// bounded positions (its sign ignored); `None` unless `m` is finite.
pub fn rho_op(m: f64) -> Option<f64> {
    if !m.is_finite() {
        return None;
    }
    // `50·u` is exact; the product rounded upwards once.
    let r = mul_up(SEAM_OPS * U, m.abs());
    r.is_finite().then_some(r)
}

/// Why a value can't be stored (module docs): the storage causes an
/// operation checks itself. Each carries its evidence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unrepresentable {
    /// A stored value isn't finite, or a bounded position's coordinate is
    /// above [`MAX_COORD`] in magnitude.
    OutOfRange {
        /// The value.
        value: f64,
        /// Its magnitude `|value|`.
        magnitude: f64,
    },
    /// The tolerance is below the storage floor at [`MAX_COORD`] (or
    /// isn't a number).
    Tolerance {
        /// The operation's tolerance `ε`.
        eps: f64,
        /// [`EPS_MIN_MAX_COORD`].
        eps_min: f64,
    },
}

fn out_of_range(value: f64) -> Unrepresentable {
    Unrepresentable::OutOfRange {
        value,
        magnitude: value.abs(),
    }
}

/// Checks the coordinates of a bounded position: each finite and at most
/// [`MAX_COORD`] in magnitude. The first failing one is the evidence.
pub fn check_bounded(p: &[f64]) -> Result<(), Unrepresentable> {
    match p.iter().find(|x| !x.is_finite() || x.abs() > MAX_COORD) {
        Some(&x) => Err(out_of_range(x)),
        None => Ok(()),
    }
}

/// Checks stored values that need only be finite.
pub fn check_finite(x: &[f64]) -> Result<(), Unrepresentable> {
    match x.iter().find(|x| !x.is_finite()) {
        Some(&x) => Err(out_of_range(x)),
        None => Ok(()),
    }
}

/// Checks an operation's tolerance against the storage floor at
/// [`MAX_COORD`]: refused below it, and when it isn't a number. Its
/// range ([`EPS_RANGE`]) is the caller's input rule, not storage.
pub fn check_eps(eps: f64) -> Result<(), Unrepresentable> {
    if eps >= EPS_MIN_MAX_COORD {
        Ok(())
    } else {
        Err(Unrepresentable::Tolerance {
            eps,
            eps_min: EPS_MIN_MAX_COORD,
        })
    }
}

/// A value rounded to storage, with a bound on its distance from the
/// true value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stored {
    /// The stored value, finite.
    pub v: f64,
    /// `|true − v|` at most: the value's certified error plus the
    /// rounding, rounded up; 0 for an exact value stored exactly.
    pub err: f64,
}

/// `x` rounded to the nearest `f64`, ties to even (`±∞` past the range).
pub fn nearest(x: &Rational) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// The exact `x` rounded once to the nearest `f64`, ties to even, with
/// its exact rounding error rounded up. Out of range past the `f64`s.
pub fn store_exact(x: &Rational) -> Result<Stored, Unrepresentable> {
    let v = nearest(x);
    if !v.is_finite() {
        return Err(out_of_range(v));
    }
    let r = (x - rat(v)).abs();
    let e = nearest(&r);
    // The nearest float to `r` may lie below it; `up` reaches past it.
    let err = if e == 0.0 && r == 0u32 { 0.0 } else { up(e) };
    Ok(Stored { v, err })
}

/// A value at a precision level rounded once to an `f64` (the level's
/// nearest), its error bounded by the far end of the certified enclosure
/// of `x − v` (formed at the level: exact for a double-double), rounded
/// up; 0 when `x` is `v` exactly, with no error. Out of range when the value
/// isn't certified or its `f64` isn't finite.
pub fn store<S: LevelScalar>(x: &S) -> Result<Stored, Unrepresentable> {
    let v = x.approx();
    let [lo, hi] = x.enclose();
    if !(v.is_finite() && lo.is_finite() && hi.is_finite()) {
        return Err(out_of_range(v));
    }
    let stored = S::from_f64(v, x);
    if *x == stored {
        return Ok(Stored { v, err: 0.0 });
    }
    let [a, b] = x.sub(&stored).enclose();
    let (a, b) = (a.abs(), b.abs());
    let err = up(if a > b { a } else { b });
    if !err.is_finite() {
        return Err(out_of_range(v));
    }
    Ok(Stored { v, err })
}

/// A point rounded to storage ([`store_point`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoredPoint {
    /// The stored coordinates, finite.
    pub p: [f64; 3],
    /// The Euclidean distance from the true point at most, rounded up:
    /// what the point's vertex tolerance or seam bound adds.
    pub displacement: f64,
}

/// A position at a precision level stored ([`store`] per coordinate),
/// with its displacement bound. With `bounded`, the point is a bounded
/// position (a vertex, a space curve's control point), range-checked
/// against [`MAX_COORD`]; else it need only be finite.
pub fn store_point<S: LevelScalar>(
    p: &[S; 3],
    bounded: bool,
) -> Result<StoredPoint, Unrepresentable> {
    let mut out = [0.0; 3];
    let mut sq = 0.0;
    for (o, x) in out.iter_mut().zip(p) {
        let s = store(x)?;
        *o = s.v;
        if s.err != 0.0 {
            sq = add_up(sq, mul_up(s.err, s.err));
        }
    }
    if bounded {
        check_bounded(&out)?;
    }
    // `√` is correctly rounded, so `up` reaches past the true root.
    let displacement = if sq == 0.0 { 0.0 } else { up(sq.sqrt()) };
    if !displacement.is_finite() {
        return Err(out_of_range(f64::INFINITY));
    }
    Ok(StoredPoint {
        p: out,
        displacement,
    })
}

/// A tolerance as stored for a needed target ([`achieved`]): what the
/// storage-rounding warning reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Achieved {
    /// The certified tolerance the result needs.
    pub target: f64,
    /// The coordinate magnitude it is stored at.
    pub magnitude: f64,
    /// The bound kept (an edge's or vertex's tolerance): the target plus
    /// the storage rounding, rounded up; finite.
    pub bound: f64,
    /// Whether the target is below the storage floor
    /// [`eps_min`]`(magnitude)`: then the operation warns with this bound,
    /// and its result is approximate.
    pub below_floor: bool,
}

/// The tolerance kept for a needed `target` at coordinate magnitude
/// `magnitude`, its geometry stored with rounding `rounding` (a
/// [`Stored::err`] or [`StoredPoint::displacement`]): always the target
/// plus the rounding, whether or not the target is below the floor.
/// `None` unless all three are finite and the target and rounding
/// non-negative.
pub fn achieved(target: f64, magnitude: f64, rounding: f64) -> Option<Achieved> {
    let ok = |x: f64| x.is_finite() && x >= 0.0;
    if !(ok(target) && ok(rounding)) {
        return None;
    }
    let floor = eps_min(magnitude)?;
    let bound = if rounding == 0.0 {
        target
    } else {
        add_up(target, rounding)
    };
    bound.is_finite().then_some(Achieved {
        target,
        magnitude: magnitude.abs(),
        bound,
        below_floor: target < floor,
    })
}

/// How an order read from stored values compares with the certified
/// unrounded one ([`compare_orders`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderCheck {
    /// The same sequence.
    Preserved,
    /// The sequences differ first at this position (or in length, at the
    /// shorter one's end): a site for the realisation repair.
    Mismatch {
        /// The first differing position.
        at: usize,
    },
}

/// Compares an order read from stored values with the certified
/// unrounded one, item by item.
pub fn compare_orders<K: PartialEq>(certified: &[K], stored: &[K]) -> OrderCheck {
    let at = certified
        .iter()
        .zip(stored)
        .position(|(a, b)| a != b)
        .unwrap_or(certified.len().min(stored.len()));
    if at == certified.len() && at == stored.len() {
        OrderCheck::Preserved
    } else {
        OrderCheck::Mismatch { at }
    }
}

/// An operation's scan of its operands' stored values at its start
/// (module docs): each value checked as it is visited, the largest
/// magnitude of the bounded positions kept. The caller visits in a fixed
/// order, so the first failure is the same on every run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OperandScan {
    max: f64,
}

impl OperandScan {
    /// An empty scan.
    pub fn new() -> OperandScan {
        OperandScan::default()
    }

    /// Adds a scan of other operands, as if visited after this one's:
    /// `M` the greater of the two.
    pub fn merge(&mut self, other: &OperandScan) {
        if other.max > self.max {
            self.max = other.max;
        }
    }

    /// A bounded position (a vertex position, a space curve's control
    /// point): finite and within [`MAX_COORD`].
    pub fn bounded(&mut self, p: &[f64]) -> Result<(), Unrepresentable> {
        check_bounded(p)?;
        for x in p {
            let a = x.abs();
            if a > self.max {
                self.max = a;
            }
        }
        Ok(())
    }

    /// Stored values that need only be finite (weights, radii, tile
    /// control points, tolerances, parameters).
    pub fn finite(&mut self, x: &[f64]) -> Result<(), Unrepresentable> {
        check_finite(x)
    }

    /// A form's key point: a bounded position if it lies inside the
    /// body's box `[lo, hi]` (of its other bounded positions), else
    /// finite only.
    pub fn key_point(&mut self, p: &[f64; 3], body: &[[f64; 3]; 2]) -> Result<(), Unrepresentable> {
        check_finite(p)?;
        let inside = (0..3).all(|k| body[0][k] <= p[k] && p[k] <= body[1][k]);
        if inside { self.bounded(p) } else { Ok(()) }
    }

    /// The largest magnitude of the bounded positions so far, `M`.
    pub fn max_coord(&self) -> f64 {
        self.max
    }

    /// The operation's start once every operand is scanned: its tolerance
    /// checked ([`check_eps`]) and `ρ_op` fixed at `M`.
    pub fn start(&self, eps: f64) -> Result<OpStart, Unrepresentable> {
        check_eps(eps)?;
        // `M ≤ MAX_COORD`, so `ρ_op` is finite.
        let rho_op = rho_op(self.max).ok_or(out_of_range(self.max))?;
        Ok(OpStart {
            max_coord: self.max,
            rho_op,
        })
    }
}

/// The values an operand's start scan visits ([`OperandScan`]), as its
/// charge counts them: the stored positions and an entity's few values
/// ([`ScanUnits::position`](super::schedule::ScanUnits::position)), and
/// the tile nets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSize {
    /// The positions.
    pub positions: u64,
    /// The tile nets.
    pub nets: u64,
}

impl ScanSize {
    /// The units of the scan: its positions' and nets' parts at the unit
    /// table's operand-scan row, summed and rounded up to whole units
    /// once (saturating, past any budget).
    pub fn units(&self) -> u64 {
        let u = UNITS.operand_scan;
        self.positions
            .saturating_mul(u.position)
            .saturating_add(self.nets.saturating_mul(u.net))
            .div_ceil(UNIT_PARTS)
    }
}

/// What an operation fixes when it starts ([`OperandScan::start`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpStart {
    /// `M`, the largest magnitude of the operands' bounded positions.
    pub max_coord: f64,
    /// `ρ_op` at `M` ([`rho_op`]).
    pub rho_op: f64,
}

/// The origin of a local frame along one axis of the box `[lo, hi]`: the
/// box's centre rounded to the nearest multiple of the greatest power of
/// two at most a quarter of the box's width (module docs), so it lies
/// within an eighth of the width of the centre. A box of width 0 (or
/// below the normal range) keeps its centre; 0 where the centre isn't
/// finite.
pub fn frame_centre(lo: f64, hi: f64) -> f64 {
    let mid = lo * 0.5 + hi * 0.5;
    if !mid.is_finite() {
        return 0.0;
    }
    let quarter = (hi * 0.5 - lo * 0.5) * 0.5;
    let Some(g) = pow2_floor(quarter) else {
        return mid;
    };
    // Scaling by a power of two is exact away from the range's ends.
    let c = (mid / g).round() * g;
    if c.is_finite() && lo <= c && c <= hi {
        c
    } else {
        mid
    }
}

/// The greatest power of two at most `x`, for a normal positive finite
/// `x`.
fn pow2_floor(x: f64) -> Option<f64> {
    (x.is_finite() && x >= f64::MIN_POSITIVE).then(|| f64::from_bits(x.to_bits() & (0x7ff << 52)))
}

/// `x − c` in a local frame at `c`: formed exactly, rounded once, its
/// exact rounding (at most `u·|x − c|`) the rounding error; exact when
/// the difference is a float. Uncertified unless both are finite and
/// the difference too.
pub fn translate(x: f64, c: f64) -> Bounded<f64> {
    let (v, r) = two_sum(x, -c);
    if !(v.is_finite() && r.is_finite()) {
        return Bounded::new(v, 0.0, f64::INFINITY);
    }
    Bounded::new(v, 0.0, r.abs())
}

/// The frame centre of a box `[lo, hi]` in each coordinate
/// ([`frame_centre`]).
pub fn frame_of(lo: &[f64; 3], hi: &[f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| frame_centre(lo[k], hi[k]))
}

#[cfg(test)]
mod tests;

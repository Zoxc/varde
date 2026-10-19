// SPDX-License-Identifier: AGPL-3.0-or-later AND Apache-2.0
// SPDX-FileCopyrightText: 2021 The Manifold Authors
//
// Derived from Manifold (https://github.com/elalish/manifold),
//   commit 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2, file src/boolean3.cpp,
//   function `Shadow01` (lines 53-83),
//   Copyright 2021 The Manifold Authors,
//   licensed under the Apache License, Version 2.0 (LICENSES/Apache-2.0.txt).
// Translated to Rust and modified: the end comparisons (`Shadows`) become
//   point–vertex records decided once per key by `decide` under the
//   perturbation, in a fixed `UP`/`r̂` frame with rays of opposite
//   directions per operand; the crossing's place along the ray is read from
//   those records, and from an exact point–line shadow row only where they
//   don't settle it, instead of interpolating the edge; the answers are
//   stored in record tables and read by every consumer; no crossing point
//   is returned.
// See THIRD_PARTY.md, row L2.

//! The flat shadow records: a point against a vertex of the other operand
//! ([`VvRec`]), and a point's ray against a straight edge of the other
//! operand ([`RayRec`]), each decided once per canonical key and read by
//! every consumer.
//!
//! **The frame.** Shadows are taken along [`UP`](crate::perturb::UP) onto the shadow plane;
//! the ray of a point of `A` goes `−r̂` ([`R_HAT`](crate::perturb::R_HAT)), of a point of `B`
//! `+r̂`, so "`a`'s ray passes `b`" and "`b`'s ray passes `a`" are one
//! fact, stored once ([`VvKey`], `A`'s point first).
//!
//! **Point–vertex record.** [`VvRec`] holds, for a point and a vertex of
//! the other operand, the side of the vertex against the point's `r̂` line
//! (`ν·(w − q)`, [`NU`](crate::perturb::NU)), whether it is ahead of the point along `r̂`
//! (`r̂·(w − q)`), each a sign decided by [`decide`] on the
//! [`VvShadow`] row (never zero: an exact tie is broken by the
//! perturbation), and whether the two are near (`‖w − q‖ ≤ max(r_q,
//! r_w)`, the points' near radii, compared exactly on the stored
//! positions, never perturbed). It is stored oriented from `A`'s point to
//! `B`'s; a reader whose point is the `B` one gets side and ahead negated
//! ([`Shadows::vv`]), `near` and "the ray passes" ([`VvRec::on_ray`]) as
//! stored.
//!
//! **Ray record on a line.** [`RayRec`] holds the signed crossing of a
//! point `q`'s ray with the shadow of a straight edge `g = g0 → g1` of the
//! other operand (canonical, from the lower vertex id): none, or the sign
//! `sign(ρ × T_g)` seen from above, `ρ` the ray's **actual** direction
//! (`−r̂` for `A`, `+r̂` for `B`) and `T_g = g1 − g0`. With this sign the
//! signed crossings of a closed curve's shadow are its winding number
//! about `q` (`+1` about a point inside a counter-clockwise square, for
//! either ray direction). It is decided, as Manifold's `Shadow01`
//! decides an edge's shadow against a vertex, from the two ends:
//!
//! 1. the ends' sides `s0`, `s1` of `q`'s `r̂` line are the point–vertex
//!    records `VVRec(q, g0)`, `VVRec(q, g1)`, read from the table, never
//!    decided again; equal sides, no crossing;
//! 2. otherwise the line of `g`'s shadow crosses `q`'s ray line, with the
//!    sign `s1` for `+r̂` and `−s1` for `−r̂`; it crosses the ray, not the
//!    part behind `q`, when both ends are ahead of `q` along its ray
//!    (both records say so), not when both are behind;
//! 3. with one end on either side, the vertex–curve shadow row on the line
//!    ([`LineShadow`], `q` against `g`'s shadow line, the line moving at
//!    its rates at the foot of `q`'s shadow) decides: the crossing is on
//!    the ray exactly when `q` is on the side of `g`'s shadow the crossing
//!    sign names (left of it for a `+` crossing).
//!
//! **Half-open ends.** A ray through a vertex `w` reads `w`'s side from
//! the one record `VVRec(q, w)`, whichever edge at `w` asks, so the edges
//! at `w` agree on it: the ray passes `w` on one side, and a chain through
//! `w` is crossed once if it goes on across the ray line, not at all if it
//! turns back. Every answer is a sign of the one moved configuration
//! (only `A` moves, by the perturbation's motions), so the records of all
//! points and edges are realisable together.
//!
//! **Decided once.** The records live in two [`Table`]s owned by
//! [`Shadows`]; their deciding functions are private to this module and
//! run only inside [`Shadows::fill_vv`] and [`Shadows::fill_rays`], one
//! charged [`Pass`] per batch of keys not yet stored. A ray record's batch
//! first fills the point–vertex records of its edges' ends (one pass),
//! then decides the rays (another), reading those.
//!
//! **Degenerate lines.** An edge whose shadow is a point (along `UP`) or a
//! zero-length edge has no shadow line: if its ends' records put them on
//! either side of the ray line (only possible when the ends move apart,
//! on `A`) and step 2 doesn't settle the crossing, the row is zero at
//! every order and [`decide`]'s guard is reached, [`Fail::Internal`].
//! Curved edges have their own ray records ([`super::ray`]).
//!
//! [`decide`]: crate::num::decide::decide

use malachite::Rational;

use crate::bezier::roots::Work as RootWork;
use crate::ids::{EdgeId, VertId};
use crate::num::bigf::{Big, BigF, Width};
use crate::num::bounded::Bounded;
use crate::num::dd::Dd;
use crate::num::decide::{Fail, Row, Sign, TieKey};
use crate::num::exact::{self, Num, Pred, Work, dot, point, sub};
use crate::perturb::{LineShadow, Moving, Seg, ShadowAxis, VvShadow, decide_flat};
use crate::records::keys::{Operand, Point, RayKey, RecordKey, VvKey};
use crate::records::ray::RayLevel;
use crate::records::table::{FillError, Fresh, Table, TableError};

/// A point against a vertex of the other operand, oriented from the
/// asking point `q` to the vertex `w` (stored from `A`'s point to `B`'s;
/// module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VvRec {
    /// `w` against `q`'s `r̂` line: its side, `ν·(w − q)`, `Pos` to its
    /// left seen from above, and whether it is ahead along `r̂`, `r̂·(w −
    /// q)`. Whether `w` lies the way the ray of `q` goes
    /// ([`SideAhead::on_ray`]) is the same for both readers of one
    /// record: either point's ray passes the other.
    pub place: SideAhead,
    /// `‖w − q‖ ≤ max(r_q, r_w)` on the stored positions.
    pub near: bool,
}

impl VvRec {
    /// The record read from the other point.
    fn swapped(self) -> VvRec {
        VvRec {
            place: SideAhead {
                side: neg(self.place.side),
                ahead: neg(self.place.ahead),
            },
            near: self.near,
        }
    }
}

/// A point's ray against a straight edge of the other operand (module
/// docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RayRec {
    /// The crossing of the ray with the edge's shadow: its sign
    /// `sign(ρ × T_g)` with the ray's actual direction `ρ`, or `None`.
    pub crossing: Option<Sign>,
}

impl RayRec {
    /// The crossing as a count: `−1`, `0` or `+1`.
    pub fn count(&self) -> i8 {
        self.crossing.map_or(0, Sign::as_i8)
    }
}

/// A point of the other operand against a point's `r̂` line, oriented
/// from the point: the side of it, `ν·(w − q)` (`Pos` to its left seen
/// from above), and whether it is ahead along `r̂`, `r̂·(w − q)`. The
/// record of a knot ([`KnotRec`](super::ray::KnotRec)) and of a face's
/// dyadic point ([`CornerRec`](super::side::CornerRec)), each asked by its
/// point alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SideAhead {
    /// The side against the `r̂` line, `ν·(w − q)`.
    pub side: Sign,
    /// Whether it is ahead along `r̂`, `r̂·(w − q)`.
    pub ahead: Sign,
}

impl SideAhead {
    /// Whether it lies the way the ray of the point (of `of`) goes.
    pub fn on_ray(&self, of: Operand) -> bool {
        self.ahead == ray_ahead(of)
    }
}

/// The geometry the shadow records read: positions with their rates along
/// the perturbation's motions, near radii and straight edges, of both
/// operands.
pub trait ShadowGeometry {
    /// Point `p`'s stored position with its rates (zero on `B`; on `A`
    /// the direction field at the point, then the generic translations).
    fn point(&self, p: Point) -> Moving;

    /// Point `p`'s near radius `r_p` (finite, `≥ 0`).
    fn near_radius(&self, p: Point) -> f64;

    /// The ends of edge `g` of `operand`, lower id first: the stored
    /// direction.
    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2];

    /// Straight edge `g` of the operand other than `q`'s, from its lower-id
    /// end to its higher, moving at its rates at the foot of `q`'s shadow
    /// on its shadow line (the direction field's value there; zero on
    /// `B`). Asked only of straight edges.
    fn line(&self, q: Point, g: EdgeId) -> Seg;
}

/// A record decided, and what deciding it ran, for the pass to charge.
pub type Decided<V> = (Result<V, Fail>, Cost);

/// The precision levels a curved record is decided at, in order: the
/// first (`f64`), double-double, then the big-float widths
/// ([`Width::ALL`]).
pub const LEVELS: usize = 2 + Width::ALL.len();

/// The parts of a unit a [`Cost`] is counted in: the kinds a record runs
/// many of ([`RecordUnits`]) take less than a unit each, so they are
/// priced in parts, summed per record and rounded up to whole units
/// ([`Cost::units`]).
pub const UNIT_PARTS: u64 = 64;

/// The units of one computation of each kind the kernel charges: the one
/// table of unit costs. A unit is about half a microsecond on one thread
/// of the reference machine (an AMD Ryzen 7 5700X3D), each kind's value
/// measured by the kernel's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which fails when a kind's time
/// per unit drifts more than 4× from it; the kernel's `MAX_WORK` is set
/// from the same measurement. The values are constants, so charges are
/// the same on every run, thread count and platform; changing one
/// changes where operations run out, a versioned change like one to the
/// precision schedule. A kind gets its row with the loop that charges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    /// A seam-bound cell (the kernel's `geom::certify`) at each precision
    /// level ([`LEVELS`]): its spans restricted and halved down to it, the
    /// tile composed with the p-curve (degree 8) and the bound formed
    /// against the curve (degree 10).
    pub seam_cell: [u64; LEVELS],
    /// A piece of a face's count tested
    /// ([`count_pieces`](crate::count::s02::count_pieces)) at each level:
    /// a box classified against the trims, split, or tested off the
    /// query point's lines, a side's or a run's degree attempted (settled
    /// or not), a trim span scanned against a loop's side.
    pub count_piece: [u64; LEVELS],
    /// A piece of a face's counting setup
    /// ([`Corners::new`](crate::count::boxes::Corners::new), built once
    /// per operation at the first level): a piece of a trimming curve
    /// whose hull in space a corner's run search forms.
    pub face_setup: u64,
    /// A piece an edge pair's cluster search tests
    /// ([`clusters`](crate::count::xrec::clusters)) at each level: a hull
    /// of a box's two pieces, a tangent cone, a side's turn, a band test.
    pub xrec_piece: [u64; LEVELS],
    /// The work a record's decision reports ([`Cost`]), in parts.
    pub record: RecordUnits,
}

/// The parts of a unit ([`UNIT_PARTS`]) of each kind of work a record's
/// decision reports in its [`Cost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordUnits {
    /// An exact predicate's filter run ([`Work::filters`]).
    pub filter: u64,
    /// An exact run in expansions ([`Work::expansions`]).
    pub expansion: u64,
    /// A run in rationals ([`Work::rationals`]).
    pub rational: u64,
    /// A row or polynomial piece evaluated ([`LevelWork::evals`]), at each
    /// level.
    pub eval: [u64; LEVELS],
    /// A root isolation's split, interval test or Krawczyk step
    /// ([`LevelWork::roots`]), at each level.
    pub root: [u64; LEVELS],
    /// A foot polynomial formed and its roots' first test
    /// ([`LevelWork::feet`]), at each level: a span's distance to the
    /// query point squared less the radius squared, degree 4.
    pub foot_poly: [u64; LEVELS],
    /// A curved height key's value or row formed at a parameter
    /// ([`LevelWork::keys`]), at each level.
    pub key_at: [u64; LEVELS],
    /// An end test of a ray record's crossing beside an end
    /// ([`LevelWork::ends`]), at each level: the span's three polynomials
    /// bounded over the piece from the end.
    pub at_end: [u64; LEVELS],
    /// A sub-box of a curved z-order's crossing localization (a halving
    /// tested apart, a Krawczyk step; [`LevelWork::boxes`]), at each
    /// level.
    pub zorder_box: [u64; LEVELS],
    /// A tile or ribbon span whose column hits are found exactly
    /// ([`Cost::hits`]): the column's equations eliminated to one
    /// polynomial in rationals and its roots isolated.
    pub hit: u64,
}

/// The unit costs ([`Units`]), as measured on the reference machine.
/// The record-evaluation kinds are measured on real record computations
/// at the first level (and a curved key's value at every level); where
/// no fixture reaches a level they are set from a measured row: a foot
/// polynomial at six evaluations, an end test at four, a z-order
/// sub-box at an edge pair's search piece.
pub const UNITS: Units = Units {
    seam_cell: [190, 310, 840, 890, 1060],
    count_piece: [9, 10, 40, 40, 48],
    face_setup: 32,
    xrec_piece: [9, 10, 28, 30, 34],
    record: RecordUnits {
        filter: 190,
        expansion: 54,
        rational: 550,
        eval: [11, 180, 520, 550, 660],
        root: [130, 410, 1400, 1400, 1700],
        foot_poly: [64, 1080, 3120, 3300, 3960],
        key_at: [190, 460, 1240, 1360, 1600],
        at_end: [44, 720, 2080, 2200, 2640],
        zorder_box: [576, 640, 1792, 1920, 2176],
        hit: 5504,
    },
};

/// The most halvings of one item, counted from the item's own start (a
/// seam span, a profile pair, a box from its tile): refinement in space
/// stops there. 48 halvings take the 4·10⁶ mm base tile down to about
/// 10⁻⁸ mm, the kernel's resolution.
pub const MAX_SUBDIV_DEPTH: u32 = 48;

/// One step of a curved decision, run at a precision level by [`ladder`].
pub(crate) trait AtLevel {
    /// The step's answer.
    type Out;

    /// The step at level `level`, `like` a value of that level's scalar
    /// and `work` the level's curved work.
    fn at<S: RayLevel>(&mut self, like: &S, level: usize, work: &mut LevelWork) -> Self::Out;

    /// Whether `out` asks for the next level.
    fn escalates(out: &Self::Out) -> bool;
}

/// Runs `f` at each level from `from` in turn until one doesn't escalate;
/// at the last level its answer stands, whatever it is. `None` when
/// `from` is past the last level.
pub(crate) fn ladder<F: AtLevel>(f: &mut F, from: usize, cost: &mut Cost) -> Option<F::Out> {
    let mut out = None;
    for (level, work) in cost.levels.iter_mut().enumerate().skip(from) {
        let r = match level {
            0 => f.at(&Bounded::exact(0.0), level, work),
            1 => f.at(&Bounded::exact(Dd::from_f64(0.0)), level, work),
            _ => {
                let like = Bounded::exact(Big::from_f64(0.0, Width::ALL[level - 2]));
                f.at(&like, level, work)
            }
        };
        let escalates = F::escalates(&r);
        out = Some(r);
        if !escalates {
            break;
        }
    }
    out
}

/// What deciding one record ran, for the pass to charge: its exact
/// predicates' work, the column hit searches, and per precision level
/// the curved work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    /// The exact predicates' runs.
    pub exact: Work,
    /// The tiles and ribbon spans whose column hits were found exactly
    /// ([`face_hits`](crate::records::height::hit::face_hits)).
    pub hits: u32,
    /// The curved work at each level ([`LEVELS`]), first level first.
    pub levels: [LevelWork; LEVELS],
}

/// The curved work at one precision level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelWork {
    /// Root isolations' splits, tests and narrowing steps.
    pub roots: RootWork,
    /// Rows and polynomial pieces evaluated; in a face count's cost,
    /// the pieces tested
    /// ([`count_pieces`](crate::count::s02::count_pieces)), in a face's
    /// setup those of its corners
    /// ([`Corners::new`](crate::count::boxes::Corners::new)), in an edge
    /// pair's cluster search those of its boxes
    /// ([`clusters`](crate::count::xrec::clusters)).
    pub evals: u32,
    /// Foot polynomials formed ([`RecordUnits::foot_poly`]).
    pub feet: u32,
    /// Curved height key values and rows formed
    /// ([`RecordUnits::key_at`]).
    pub keys: u32,
    /// End tests of ray records' crossings ([`RecordUnits::at_end`]).
    pub ends: u32,
    /// Sub-boxes of curved z-order localizations
    /// ([`RecordUnits::zorder_box`]).
    pub boxes: u32,
}

impl LevelWork {
    /// This work and `o`'s together.
    pub fn and(self, o: LevelWork) -> LevelWork {
        LevelWork {
            roots: self.roots.and(o.roots),
            evals: self.evals.saturating_add(o.evals),
            feet: self.feet.saturating_add(o.feet),
            keys: self.keys.saturating_add(o.keys),
            ends: self.ends.saturating_add(o.ends),
            boxes: self.boxes.saturating_add(o.boxes),
        }
    }
}

impl Cost {
    /// The units this cost is charged at `u`'s prices: its parts
    /// ([`Cost::parts`]) rounded up to whole units. For a record's cost
    /// alone: a face count's cost counts its pieces in
    /// [`LevelWork::evals`], priced per piece by
    /// [`Charge::spent`](crate::count::s02::Charge::spent) instead.
    pub fn units(&self, u: &RecordUnits) -> u64 {
        self.parts(u).div_ceil(UNIT_PARTS)
    }

    /// This cost in parts of a unit ([`UNIT_PARTS`]) at `u`'s prices: the
    /// exact predicates' runs ([`Cost::exact_parts`]), the column hit
    /// searches and each level's curved work ([`Cost::level_parts`]).
    pub fn parts(&self, u: &RecordUnits) -> u64 {
        let hits = u64::from(self.hits).saturating_mul(u.hit);
        (0..LEVELS).fold(self.exact_parts(u).saturating_add(hits), |s, level| {
            s.saturating_add(self.level_parts(level, u))
        })
    }

    /// The exact predicates' runs in parts of a unit at `u`'s prices.
    pub fn exact_parts(&self, u: &RecordUnits) -> u64 {
        let w = &self.exact;
        [
            (w.filters, u.filter),
            (w.expansions, u.expansion),
            (w.rationals, u.rational),
        ]
        .into_iter()
        .fold(0u64, |s, (n, p)| {
            s.saturating_add(u64::from(n).saturating_mul(p))
        })
    }

    /// The curved work at precision level `level` (below [`LEVELS`]) in
    /// parts of a unit at `u`'s prices.
    pub fn level_parts(&self, level: usize, u: &RecordUnits) -> u64 {
        let w = &self.levels[level];
        let r = &w.roots;
        let roots = u64::from(r.splits)
            .saturating_add(u64::from(r.tests))
            .saturating_add(u64::from(r.steps));
        [
            (u64::from(w.evals), u.eval[level]),
            (roots, u.root[level]),
            (u64::from(w.feet), u.foot_poly[level]),
            (u64::from(w.keys), u.key_at[level]),
            (u64::from(w.ends), u.at_end[level]),
            (u64::from(w.boxes), u.zorder_box[level]),
        ]
        .into_iter()
        .fold(0u64, |s, (n, p)| s.saturating_add(n.saturating_mul(p)))
    }

    /// Exact work alone.
    pub fn exact(work: Work) -> Cost {
        Cost {
            exact: work,
            ..Cost::default()
        }
    }

    /// This cost and `o`'s together.
    pub fn and(self, o: Cost) -> Cost {
        Cost {
            exact: self.exact.and(o.exact),
            hits: self.hits.saturating_add(o.hits),
            levels: std::array::from_fn(|i| self.levels[i].and(o.levels[i])),
        }
    }
}

/// One charged pass over a batch of keys, the kernel's: it decides each
/// key with the function it is handed, charges each key's [`Cost`],
/// and gives the answers in the keys' order, or fails (a [`Fail`], or
/// running out), in which case nothing of the batch is stored.
pub trait Pass {
    /// Why a pass failed.
    type Error;

    /// Decides `keys`, sorted, each with `decide`.
    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Self::Error>;
}

/// How filling the shadow records failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShadowError<E> {
    /// A pass failed: nothing of its batch was stored.
    Pass(E),
    /// A pass's answers couldn't be stored (a bug of the pass).
    Table(TableError<RecordKey>),
}

/// The point–vertex and ray records of a run, decided once per key
/// (module docs).
#[derive(Debug, Default)]
pub struct Shadows {
    vv: Table<VvKey, VvRec>,
    ray: Table<RayKey, RayRec>,
}

impl Shadows {
    /// No records.
    pub fn new() -> Shadows {
        Shadows::default()
    }

    /// Decides the point–vertex records of `wanted` not yet stored, in one
    /// pass.
    pub fn fill_vv<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = VvKey>,
        geom: &G,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: ShadowGeometry + Sync,
        P: Pass,
    {
        self.vv
            .fill(wanted, |keys| {
                pass.run(keys, &|k: &Fresh<VvKey>| decide_vv(k, geom))
            })
            .map_err(|e| shadow_error(e, RecordKey::Vv))
    }

    /// Decides the ray records of `wanted` not yet stored: first the
    /// point–vertex records of their edges' ends not yet stored, in one
    /// pass, then the rays, in another. Every ray key's edge must be
    /// straight ([`ShadowGeometry::line`]).
    pub fn fill_rays<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = RayKey>,
        geom: &G,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: ShadowGeometry + Sync,
        P: Pass,
    {
        let batch = self.ray.missing(wanted);
        if batch.is_empty() {
            return Ok(());
        }
        let ends = batch.keys().iter().flat_map(|k| {
            let [g0, g1] = geom.ends(k.q.operand.other(), k.g);
            [VvKey::of(k.q, g0).key, VvKey::of(k.q, g1).key]
        });
        self.fill_vv(ends.collect::<Vec<_>>(), geom, pass)?;
        let vv = &self.vv;
        let values = pass
            .run(batch.keys(), &|k: &Fresh<RayKey>| decide_ray(k, vv, geom))
            .map_err(ShadowError::Pass)?;
        self.ray
            .insert(batch, values)
            .map_err(|e| ShadowError::Table(table_error(e, RecordKey::Ray)))
    }

    /// The point–vertex record of point `q` against `w`, a vertex of the
    /// other operand, oriented from `q` (side and ahead negated for a `B`
    /// point); `None` if it isn't decided.
    pub fn vv(&self, q: Point, w: VertId) -> Option<VvRec> {
        read_vv(&self.vv, q, w)
    }

    /// The ray record of point `q` against straight edge `g` of the other
    /// operand; `None` if it isn't decided.
    pub fn ray(&self, q: Point, g: EdgeId) -> Option<RayRec> {
        self.ray.get(&RayKey { q, g }).copied()
    }

    /// The point–vertex table, for its reads and keys.
    pub fn vv_table(&self) -> &Table<VvKey, VvRec> {
        &self.vv
    }

    /// The ray table, for its reads and keys.
    pub fn ray_table(&self) -> &Table<RayKey, RayRec> {
        &self.ray
    }

    /// Decides every stored record again, on `geom`, and gives the key of
    /// the first whose answer differs (point–vertex records first, each
    /// table in key order): a check that the answers are functions of
    /// their keys. Ray records read their ends' stored records, which
    /// counts as reads of those.
    pub fn replay<G: ShadowGeometry>(&self, geom: &G) -> Result<(), RecordKey> {
        replay(&self.vv, |k| decide_vv(k, geom), RecordKey::Vv)?;
        replay(&self.ray, |k| decide_ray(k, &self.vv, geom), RecordKey::Ray)
    }
}

/// [`Table::replay`] with a deciding function that can fail: the key of
/// the first record, in key order, deciding again fails or differs at.
pub(super) fn replay<K: Ord + Clone, V: PartialEq + Clone>(
    t: &Table<K, V>,
    decide: impl Fn(&Fresh<K>) -> Decided<V>,
    wrap: impl Fn(K) -> RecordKey,
) -> Result<(), RecordKey> {
    // The table replays in key order and stops at the first mismatch, so
    // a failure met before it has the lower key. A failed record replays
    // as its stored answer (read, a failure being a bug) to go on.
    let failed = std::cell::RefCell::new(None);
    let again = t.replay(|k| match decide(k).0 {
        Ok(v) => v,
        Err(_) => {
            failed.borrow_mut().get_or_insert_with(|| k.key().clone());
            t.get(k.key()).cloned().expect("a replayed key is stored")
        }
    });
    if let Some(k) = failed.into_inner() {
        return Err(wrap(k));
    }
    again.map_err(|m| wrap(m.key))
}

/// The point–vertex record of `q` against `w` in `vv`, oriented from `q`.
pub(super) fn read_vv(vv: &Table<VvKey, VvRec>, q: Point, w: VertId) -> Option<VvRec> {
    let o = VvKey::of(q, w);
    let r = *vv.get(&o.key)?;
    Some(if o.swapped { r.swapped() } else { r })
}

/// `A`'s rays go `−r̂`, `B`'s `+r̂`: the sign of `r̂·ρ`.
pub(super) fn ray_ahead(of: Operand) -> Sign {
    match of {
        Operand::A => Sign::Neg,
        Operand::B => Sign::Pos,
    }
}

pub(super) fn neg(s: Sign) -> Sign {
    match s {
        Sign::Neg => Sign::Pos,
        Sign::Pos => Sign::Neg,
    }
}

pub(super) fn table_error<K>(
    e: TableError<K>,
    wrap: impl Fn(K) -> RecordKey,
) -> TableError<RecordKey> {
    match e {
        TableError::Count { keys, values } => TableError::Count { keys, values },
        TableError::Twice(k) => TableError::Twice(wrap(k)),
    }
}

pub(super) fn shadow_error<K, E>(
    e: FillError<K, E>,
    wrap: impl Fn(K) -> RecordKey,
) -> ShadowError<E> {
    match e {
        FillError::Decide(e) => ShadowError::Pass(e),
        FillError::Table(e) => ShadowError::Table(table_error(e, wrap)),
    }
}

/// `r² − ‖w − q‖²`: at least zero when the points are within `r`. The
/// radius is any finite `r ≥ 0`, so it enters as an exact rational, not
/// bound by the coordinates' input range.
pub(super) struct NearGap {
    pub(super) q: glam::DVec3,
    pub(super) w: glam::DVec3,
    pub(super) r: Rational,
}

impl Pred for NearGap {
    fn eval<N: Num>(&self) -> N {
        let d = sub(&point::<N>(self.w), &point(self.q));
        let r = N::rational(&self.r);
        r.mul(&r).sub(&dot(&d, &d))
    }
}

/// Decides a point–vertex record, oriented from `A`'s point to `B`'s.
fn decide_vv<G: ShadowGeometry>(k: &Fresh<VvKey>, geom: &G) -> Decided<VvRec> {
    let key = *k.key();
    let tie = TieKey {
        row: Row::VertexVertexShadow,
        record: RecordKey::Vv(key),
    };
    let (pa, pb) = (
        Point {
            operand: Operand::A,
            q: key.a,
        },
        Point {
            operand: Operand::B,
            q: key.b,
        },
    );
    let (q, w) = (geom.point(pa), geom.point(pb));
    let (ra, rb) = (geom.near_radius(pa), geom.near_radius(pb));
    let (side_ahead, w12) = side_and_ahead(&q, &w, tie);
    let fine = |r: f64| r.is_finite() && r >= 0.0;
    let (near, w3) = if fine(ra) && fine(rb) {
        let r = if ra >= rb { ra } else { rb };
        let r = Rational::try_from(r).expect("a finite radius");
        let (s, w3) = exact::value_sign(&NearGap {
            q: q.at,
            w: w.at,
            r,
        });
        (
            s.map(|s| s.is_ge())
                .map_err(|_| Fail::Internal { key: tie }),
            w3,
        )
    } else {
        (Err(Fail::Internal { key: tie }), Work::default())
    };
    let work = Cost::exact(w12.and(w3));
    let rec = side_ahead.and_then(|place| Ok(VvRec { place, near: near? }));
    (rec, work)
}

/// The flat point–vertex shadow rows of `w` against `q`'s `r̂` line:
/// `[side, ahead]`, `ν·(w − q)` and `r̂·(w − q)` under the perturbation,
/// with the work both took.
pub(super) fn side_and_ahead(
    q: &Moving,
    w: &Moving,
    tie: TieKey,
) -> (Result<SideAhead, Fail>, Work) {
    side_and_ahead_by(
        |axis| VvShadow {
            q: q.clone(),
            w: w.clone(),
            axis,
        },
        tie,
    )
}

/// [`side_and_ahead`] by the flat rows `row` gives along each axis.
pub(super) fn side_and_ahead_by<P: Pred>(
    row: impl Fn(ShadowAxis) -> P,
    tie: TieKey,
) -> (Result<SideAhead, Fail>, Work) {
    let (side, w1) = decide_flat(&row(ShadowAxis::Side), tie);
    let (ahead, w2) = decide_flat(&row(ShadowAxis::Along), tie);
    let rec = side.and_then(|side| {
        Ok(SideAhead {
            side,
            ahead: ahead?,
        })
    });
    (rec, w1.and(w2))
}

/// Decides a ray record from its ends' stored point–vertex records and,
/// where they don't settle it, the point–line shadow row.
fn decide_ray<G: ShadowGeometry>(
    k: &Fresh<RayKey>,
    vv: &Table<VvKey, VvRec>,
    geom: &G,
) -> Decided<RayRec> {
    let key = *k.key();
    let q = key.q;
    let tie = |row| TieKey {
        row,
        record: RecordKey::Ray(key),
    };
    let ends = geom.ends(q.operand.other(), key.g);
    let read = |w: VertId| read_vv(vv, q, w).map(|r| r.place);
    let (Some(r0), Some(r1)) = (read(ends[0]), read(ends[1])) else {
        // The batch's ends are filled first: a missing one is a bug.
        let fail = Fail::Internal {
            key: tie(Row::VertexVertexShadow),
        };
        return (Err(fail), Cost::default());
    };
    if r0.side == r1.side {
        return (Ok(RayRec { crossing: None }), Cost::default());
    }
    // `ρ × T_g` seen from above: `ν·T_g` along `+r̂`, which has the sign
    // of `s1` once the ends are on either side.
    let sign = match q.operand {
        Operand::A => neg(r1.side),
        Operand::B => r1.side,
    };
    let (on0, on1) = (r0.on_ray(q.operand), r1.on_ray(q.operand));
    if on0 == on1 {
        let crossing = on0.then_some(sign);
        return (Ok(RayRec { crossing }), Cost::default());
    }
    let line = geom.line(q, key.g);
    debug_assert!(
        line.a == geom.point(Point::vert(q.operand.other(), ends[0])).at
            && line.b == geom.point(Point::vert(q.operand.other(), ends[1])).at,
        "a ray record's line runs between its edge's ends, lower id first"
    );
    let row = LineShadow {
        q: geom.point(q),
        line,
    };
    let (left, work) = decide_flat(&row, tie(Row::VertexCurveShadow));
    let rec = left.map(|left| RayRec {
        crossing: (left == sign).then_some(sign),
    });
    (rec, Cost::exact(work))
}

#[cfg(test)]
mod tests;

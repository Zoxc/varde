// SPDX-License-Identifier: AGPL-3.0-or-later AND Apache-2.0
// SPDX-FileCopyrightText: 2021 The Manifold Authors
//
// Derived from Manifold (https://github.com/elalish/manifold), commit 3ad9216b2b62f4faaf3ea5342b4c99cdca70dbd2, file src/boolean3.cpp,
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
//! the ray of a point of `A` goes `−r̂` ([`R_HAT`]), of a point of `B`
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
//! ([`Shadows::vv`]), `near` and "the ray passes" ([`SideAhead::on_ray`]) as
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
//! **Along the ray line.** An edge whose shadow lies on `q`'s ray line
//! exactly (both ends on it, by an exact flat predicate on the stored
//! values) has no crossing of its own: the moved edge crosses the moved
//! line where its side, the first-order row `ν·(ΔC − Δq)` along it (the
//! next order where that one is 0), changes, from its start's record's
//! side to its end's, one crossing per change, on the ray where the
//! change lies ahead of `q` ([`along_changes`],
//! the curved levels at the caller's band, the field along the edge from
//! [`ShadowGeometry::straight_rates`]). The record is their sum. Where the
//! field is constant along the edge the row has one sign: the ends'
//! records decide as above. A change of side at `q`'s foot escalates.
//!
//! [`decide`]: crate::num::decide::decide

use malachite::Rational;
use smallvec::SmallVec;

use crate::bezier::roots::Work as RootWork;
use crate::ids::{EdgeId, VertId};
use crate::num::decide::{Band, Fail, Row, Sign, TieKey, decide};
use crate::num::exact::{self, ORDERS, Pred, Work};
use crate::num::level::{AtLevel, LevelScalar, ladder};
use crate::num::preds::NearGap;
use crate::num::schedule::{LEVELS, Level, RecordUnits, UNIT_PARTS};
use crate::perturb::{
    Curved, LineShadow, Moving, MovingAt, R_HAT, Seg, ShadowAxis, VvShadow, between, decide_flat,
    vv_shadow,
};
use crate::records::keys::{Key, Operand, Point, RayKey, RecordKey, VvKey};
use crate::records::ray::{Crossing, along_changes};
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

    /// The rates of straight edge `g`'s point at the parameter `t` (from
    /// its lower-id end, an enclosure at its level) for `q`'s record: the
    /// direction field there on `A`, zero on `B`. Read only where the
    /// edge's shadow lies on `q`'s ray line (module docs, "Along the ray
    /// line"). By default [`line`](Self::line)'s rates, a field constant
    /// along the edge.
    fn straight_rates<S: Curved>(&self, q: Point, g: EdgeId, t: &S) -> [[S; 3]; ORDERS] {
        self.line(q, g).rates.each_ref().map(|r| r.at(t))
    }
}

/// A record decided, and what deciding it ran, for the pass to charge.
pub type Decided<V> = (Result<V, Fail>, Cost);

/// What deciding one record ran, for the pass to charge: its exact
/// predicates' work, its other exact runs, the column hit searches, and
/// per precision level the curved work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    /// The exact predicates' runs.
    pub exact: Work,
    /// Runs in rationals outside a flat predicate: exact polynomials
    /// formed and their roots isolated ([`RecordUnits::rational`]).
    pub rationals: u32,
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
    /// exact runs ([`Cost::exact_parts`]), the column hit searches and
    /// each level's curved work ([`Cost::level_parts`]).
    pub fn parts(&self, u: &RecordUnits) -> u64 {
        let hits = u64::from(self.hits).saturating_mul(u.hit);
        (0..LEVELS).fold(self.exact_parts(u).saturating_add(hits), |s, level| {
            s.saturating_add(self.level_parts(level, u))
        })
    }

    /// The exact runs in parts of a unit at `u`'s prices: the exact
    /// predicates' at their kinds' rows ([`Work::parts`]), the other
    /// runs in rationals at [`RecordUnits::rational`].
    pub fn exact_parts(&self, u: &RecordUnits) -> u64 {
        let others = u64::from(self.rationals).saturating_mul(u.rational);
        self.exact.parts(u).saturating_add(others)
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
            rationals: self.rationals.saturating_add(o.rationals),
            hits: self.hits.saturating_add(o.hits),
            levels: std::array::from_fn(|i| self.levels[i].and(o.levels[i])),
        }
    }
}

/// One charged pass over a batch of keys, the kernel's: it decides each
/// key with the function it is handed, charges each key's [`Cost`],
/// and gives the answers in the keys' order, or fails (a [`Fail`], or
/// running out, naming the record it ran out at by its key,
/// [`Fresh::record`]), in which case nothing of the batch is stored.
pub trait Pass {
    /// Why a pass failed.
    type Error;

    /// Decides `keys`, sorted, each with `decide`.
    fn run<K: Key, V: Send>(
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
    /// straight ([`ShadowGeometry::line`]). `band` is the rung's curved
    /// band, read only by an edge lying on its point's ray line (module
    /// docs, "Along the ray line").
    pub fn fill_rays<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = RayKey>,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), ShadowError<P::Error>>
    where
        G: ShadowGeometry + Sync,
        P: Pass,
    {
        let batch = self
            .ray
            .missing(wanted)
            .map_err(|e| ShadowError::Table(table_error(e, RecordKey::Ray)))?;
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
            .run(batch.keys(), &|k: &Fresh<RayKey>| {
                decide_ray(k, vv, geom, band)
            })
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
    /// counts as reads of those. `band` as in
    /// [`fill_rays`](Self::fill_rays).
    pub fn replay<G: ShadowGeometry>(&self, geom: &G, band: Band) -> Result<(), RecordKey> {
        replay(&self.vv, |k| decide_vv(k, geom), RecordKey::Vv)?;
        replay(
            &self.ray,
            |k| decide_ray(k, &self.vv, geom, band),
            RecordKey::Ray,
        )
    }
}

/// [`Table::replay`] with a deciding function that can fail: the key of
/// the first record, in key order, deciding again fails or differs at.
pub(super) fn replay<K: Key, V: PartialEq + Clone>(
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
            failed.borrow_mut().get_or_insert_with(|| *k.key());
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
        TableError::Foreign => TableError::Foreign,
        TableError::NoneId(k) => TableError::NoneId(wrap(k)),
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
    band: Band,
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
    let (on0, on1) = (r0.on_ray(q.operand), r1.on_ray(q.operand));
    // An edge lying on `q`'s ray line, exactly: crossed where its side
    // moved changes along it (module docs, "Along the ray line").
    let line = geom.line(q, key.g);
    let at = geom.point(q).at;
    let (on_line, work) = on_ray_line(at, line.a, line.b);
    let mut cost = Cost::exact(work);
    if on_line {
        let mut along = AlongLine {
            geom,
            key,
            line: [line.a, line.b],
            ends: [(r0.side, on0), (r1.side, on1)],
            band,
            tie: tie(Row::VertexCurveShadow),
        };
        let r = ladder(&mut along, Level::L0, &mut cost.levels);
        let rec = r.and_then(|cs| {
            let n: i32 = cs.iter().map(|c| i32::from(c.sign.as_i8())).sum();
            let crossing = match n {
                0 => None,
                1 => Some(Sign::Pos),
                -1 => Some(Sign::Neg),
                // The changes alternate: a sum past one is a bug.
                _ => {
                    return Err(Fail::Internal {
                        key: tie(Row::VertexCurveShadow),
                    });
                }
            };
            Ok(RayRec { crossing })
        });
        return (rec, cost);
    }
    let cost = cost;
    if r0.side == r1.side {
        return (Ok(RayRec { crossing: None }), cost);
    }
    // `ρ × T_g` seen from above: `ν·T_g` along `+r̂`, which has the sign
    // of `s1` once the ends are on either side.
    let sign = match q.operand {
        Operand::A => neg(r1.side),
        Operand::B => r1.side,
    };
    if on0 == on1 {
        let crossing = on0.then_some(sign);
        return (Ok(RayRec { crossing }), cost);
    }
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
    (rec, cost.and(Cost::exact(work)))
}

/// Whether the segment from `a` to `b` lies on the `r̂` line of `q`,
/// exactly: `ν·(a − q) = ν·(b − q) = 0` on the stored values, `b` tested
/// only where `a` is on it; and the work.
fn on_ray_line(q: glam::DVec3, a: glam::DVec3, b: glam::DVec3) -> (bool, Work) {
    let mut work = Work::default();
    for p in [a, b] {
        let (s, w) = exact::value_sign(&VvShadow {
            q: Moving::fixed(q),
            w: Moving::fixed(p),
            axis: ShadowAxis::Side,
        });
        work = work.and(w);
        if s != Ok(std::cmp::Ordering::Equal) {
            return (false, work);
        }
    }
    (true, work)
}

/// A straight edge lying on a point's ray line, for [`ladder`]: its
/// crossings at one level ([`along_changes`]).
struct AlongLine<'a, G> {
    geom: &'a G,
    key: RayKey,
    /// The edge's ends, lower id first.
    line: [glam::DVec3; 2],
    /// Each end's side and whether it is on the ray, from its record.
    ends: [(Sign, bool); 2],
    band: Band,
    tie: TieKey,
}

impl<G: ShadowGeometry> AtLevel<LevelWork> for AlongLine<'_, G> {
    type Out = Result<SmallVec<[Crossing; 2]>, Fail>;

    fn at<S: LevelScalar>(&mut self, like: &S, work: &mut LevelWork) -> Self::Out {
        let f = |x: f64| S::from_f64(x, like);
        let q = MovingAt::from_flat(&self.geom.point(self.key.q), like);
        let side_over = |[a, b]: [f64; 2], work: &mut LevelWork| {
            work.evals = work.evals.saturating_add(1);
            let t = between(a, b, like);
            let at = MovingAt {
                at: q.at.clone(),
                rates: self.geom.straight_rates(self.key.q, self.key.g, &t),
            };
            let row = vv_shadow(&q, &at, ShadowAxis::Side);
            decide(row.g, row.ord, self.band, self.tie)
        };
        // `r̂·(C − q)` on the line, linear in its parameter: of one sign
        // over `[a, b]` where certified at both.
        let [p0, p1] = self.line.map(|p| [f(p.x), f(p.y), f(p.z)]);
        let r = [f(R_HAT.x), f(R_HAT.y), f(R_HAT.z)];
        let ahead_over = |[a, b]: [f64; 2], work: &mut LevelWork| {
            work.evals = work.evals.saturating_add(1);
            let at = |t: f64| {
                let t = f(t);
                (0..3).fold(S::int(0, like), |s, k| {
                    let c = p0[k].add(&p1[k].sub(&p0[k]).mul(&t));
                    s.add(&r[k].mul(&c.sub(&q.at[k])))
                })
            };
            let (x, y) = (at(a).sign()?, at(b).sign()?);
            (x == y).then_some(x).and_then(Sign::of)
        };
        along_changes(self.ends, self.key.q.operand, work, side_over, ahead_over)
    }

    fn escalates(out: &Self::Out) -> bool {
        matches!(out, Err(Fail::Escalate { .. }))
    }
}

#[cfg(test)]
mod tests;

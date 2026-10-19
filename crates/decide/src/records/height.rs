//! Height keys and records: which geometry a height above or below is
//! read from ([`height_key`]) and the heights decided ([`Heights`]).
//!
//! **The key.** A point `q` asks its height against a target of the
//! other operand: a vertex `w`, an edge `g` at one of `q`'s feet `k` on
//! it, or a **column hit** of `q` on a face `f` (a point of `f`'s
//! counting surface on `q`'s column, on one sheet: a tile inside the
//! trim, or the ribbon of one span of a coedge; [`hit`]). The answer is a
//! height key ([`HKey`]), the one geometry the height's value is decided
//! from for every anchor that reads it:
//!
//! - against a vertex `w`: the point–vertex key `VV(q, w)`, one for the
//!   pair whichever point asks;
//! - a cut point `Cut(e, g, c)` against its own edge `g`: the crossing
//!   record's cluster itself, `X(e, g, c)`;
//! - against an edge `g` at foot `k`: `VV(q, w)` where `q` lies exactly at
//!   one of `g`'s end vertices `w` in `k` (on the stored positions), else
//!   the point–curve key `Curve(q, g, k)`;
//! - against a hit: the hit's own key `Hit(q, f, sheet, i)`, partition-
//!   and level-free, except where the hit lies exactly on a vertex or an
//!   edge's curve of the face at `q`'s height (the **germ rule**, decided
//!   on the stored data): then the vertex's or edge's key as above, its
//!   value exactly 0, so the faces' own tangent planes decide.
//!
//! No near radius, foot or band chooses a key away from exact incidence.
//! The **edge rule** ([`edge_rule_key`]) is the lobe fallback's alone: a
//! point whose winding the exact hits leave outside `{0, 1}` re-reads its
//! hits near an edge by the edge's key at the point's foot (the lesser
//! near end's `VV` where one is near, else `Curve`).
//!
//! **The hit key's value** is `n_f·(x − q)·sign(n_f·ê)` at the hit `x`:
//! the height of the sheet above `q` along `ê`, positive where the sheet
//! is above whichever way it faces. On flat data it is exact: a tile or
//! ribbon hit by sign conditions at the roots of its hit polynomials on
//! the stored tile or ribbon ([`hit`]). Where a
//! curved anchor's facing `sign(n_f·ê)` isn't certified the in-face fold
//! rule decides instead.
//!
//! **The fold rule.** An anchor whose facing isn't certified at any level
//! (its tangent plane holds `ê`: a silhouette inside the face) has no
//! orders. Its record is then the fold's ([`HeightRec::fold`]): the face
//! there is two sheets folded over in the shadow, and the record holds
//! the upper sheet's facing ([`HeightGeometry::curved_fold`]), the same
//! from either side. The concrete geometry is [`model`]'s.
//!
//! **The records.** A height is decided in two parts ([`Heights`]). The
//! key's value, order 0, is decided **once per key** for every anchor
//! reading it ([`KeyRec`]): its sign, or a tie; its band class (geometric
//! when every anchor asking it is, else flat or curved by the key's
//! data, [`KeyValue`]); and its precision level, the first at which it
//! was decided. So the faces at a shared vertex read one `VV(v, w)` and
//! agree whenever it decides (a silhouette pair cancels). At a tie each
//! anchor's own orders decide ([`HeightRec`]): the anchor's tangent
//! element against the moved point ([`AnchorRow`]), so a moved vertex is
//! compared with the face its column hits (knife wedge, roof ridge); its
//! curved orders start at the key's level. The anchors are the faces of
//! hits and the edges of crossings ([`Anchor`]); a hit key is read at its
//! own face only. Both live in [`Table`]s, decided in one charged
//! [`Pass`] each ([`Heights::fill`]): every anchor of a key is asked in
//! the batch that decides it, as its band is theirs.

use std::collections::BTreeMap;

use crate::ids::{EdgeId, VertId};
use crate::num::decide::{Band, BandKind, Coeff, Fail, Row, Sign, TieKey, decide};
use crate::num::exact::{self, ExactSign, ORDERS, Pred};
use crate::order::{Along, EdgeKey, EntityOrder, VertKey};
use crate::perturb::{Curved, decide_flat_orders};
use crate::records::feet::{Feet, NearFeet};
use crate::records::keys::{
    Anchor, FootId, HKey, HeightKey, Incidence, Operand, Oriented, Point, Query, RayKey, RecordKey,
    Target, VvKey,
};
use crate::records::ray::RayLevel;
use crate::records::shadow::{
    AtLevel, Cost, Decided, LEVELS, LevelWork, Pass, Shadows, VvRec, ladder, neg, replay,
    table_error,
};
use crate::records::table::{Fresh, Table, TableError};

pub mod hit;
pub mod model;

/// What [`height_key`] reads: the geometric order of the other operand's
/// vertices and edges (their stored positions among them), the
/// point–vertex records (their `near`, for the edge rule) and the feet.
pub trait HeightReads: EntityOrder {
    /// Point `q`'s record against vertex `w` of the other operand, read
    /// from `q`'s side; `None` if it isn't decided.
    fn vv(&self, q: Point, w: VertId) -> Option<VvRec>;

    /// Point `q`'s feet on edge `g` of the other operand; `None` if they
    /// aren't decided.
    fn feet(&self, q: Point, g: EdgeId) -> Option<&Feet>;
}

/// The record tables and the order, read together.
#[derive(Clone, Copy, Debug)]
pub struct Records<'a, O> {
    /// The point–vertex records.
    pub shadows: &'a Shadows,
    /// The feet.
    pub feet: &'a NearFeet,
    /// The order.
    pub order: &'a O,
}

impl<O: EntityOrder> EntityOrder for Records<'_, O> {
    type Key = O::Key;

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.order.ends(operand, g)
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<O::Key> {
        self.order.vert_key(operand, v)
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<O::Key> {
        self.order.edge_key(operand, g)
    }
}

impl<O: EntityOrder> HeightReads for Records<'_, O> {
    fn vv(&self, q: Point, w: VertId) -> Option<VvRec> {
        self.shadows.vv(q, w)
    }

    fn feet(&self, q: Point, g: EdgeId) -> Option<&Feet> {
        self.feet.get(q, g)
    }
}

/// Why no height key was found: a record it reads isn't decided, or the
/// target names a foot that isn't there. A caller's bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The record isn't decided.
    Missing(RecordKey),
    /// Point `q` has no foot `foot` on edge `g`.
    NoFoot {
        /// The point.
        q: Point,
        /// The edge.
        g: EdgeId,
        /// The foot.
        foot: FootId,
    },
}

fn near<R: HeightReads>(reads: &R, q: Point, w: VertId) -> Result<bool, KeyError> {
    reads
        .vv(q, w)
        .map(|r| r.near)
        .ok_or(KeyError::Missing(RecordKey::Vv(VvKey::of(q, w).key)))
}

fn feet_of<R: HeightReads>(reads: &R, q: Point, g: EdgeId) -> Result<&Feet, KeyError> {
    reads
        .feet(q, g)
        .ok_or(KeyError::Missing(RecordKey::Ray(RayKey { q, g })))
}

/// The way edge `g` of `of`'s stored parameter runs from its lesser end.
fn along<R: HeightReads>(reads: &R, of: Operand, g: EdgeId) -> Along {
    let [s, e] = reads.ends(of, g);
    Along::of(&reads.vert_key(of, s), &reads.vert_key(of, e))
}

/// Whether point `q` lies exactly at vertex `w` of the other operand, on
/// their stored positions (`−0` as `+0`). A cut point lies at no vertex
/// here: its stored position isn't in the order.
fn at<R: HeightReads>(reads: &R, q: Point, w: VertId) -> bool {
    let Query::Vert(v) = q.q else {
        return false;
    };
    let p = reads.vert_key(q.operand, v).pos().get();
    let o = reads.vert_key(q.operand.other(), w).pos().get();
    (0..3).all(|k| p[k] == o[k])
}

/// The height key point `q` reads against target `t` of the other
/// operand (module docs): oriented as `q` asked it, `swapped` where `q`
/// is the key's `B` point.
pub fn height_key<R: HeightReads>(
    reads: &R,
    q: Point,
    t: Target,
) -> Result<Oriented<HKey>, KeyError> {
    let of = q.operand.other();
    match t {
        Target::Vert(w) => Ok(HKey::vv(q, w)),
        Target::Edge(g, k) => {
            if let Some(x) = HKey::x(q, g) {
                return Ok(x);
            }
            let feet = feet_of(reads, q, g)?;
            let foot = feet.get(k).ok_or(KeyError::NoFoot { q, g, foot: k })?;
            for w in foot.end_vertices(reads.ends(of, g), along(reads, of, g)) {
                if at(reads, q, w) {
                    return Ok(HKey::vv(q, w));
                }
            }
            Ok(own(HKey::Curve { q, g, foot: k }))
        }
        Target::Hit { f, sheet, i, inc } => match inc {
            Some(Incidence::Vert(w)) => height_key(reads, q, Target::Vert(w)),
            Some(Incidence::Edge(g, k)) => height_key(reads, q, Target::Edge(g, k)),
            None => Ok(own(HKey::Hit { q, f, sheet, i })),
        },
    }
}

/// The edge rule's key (module docs, the lobe fallback's): point `q`
/// against edge `g` at foot `k`, `VV(q, w)` for the first of `g`'s end
/// vertices in `k` (geometric order) near `q` (its point–vertex record
/// says so), else `Curve(q, g, k)`.
pub fn edge_rule_key<R: HeightReads>(
    reads: &R,
    q: Point,
    g: EdgeId,
    k: FootId,
) -> Result<Oriented<HKey>, KeyError> {
    let of = q.operand.other();
    if let Some(x) = HKey::x(q, g) {
        return Ok(x);
    }
    let feet = feet_of(reads, q, g)?;
    let foot = feet.get(k).ok_or(KeyError::NoFoot { q, g, foot: k })?;
    for w in foot.end_vertices(reads.ends(of, g), along(reads, of, g)) {
        if near(reads, q, w)? {
            return Ok(HKey::vv(q, w));
        }
    }
    Ok(own(HKey::Curve { q, g, foot: k }))
}

/// A key holding the asking point itself.
fn own(key: HKey) -> Oriented<HKey> {
    Oriented {
        key,
        swapped: false,
    }
}

/// How a height key's value is read ([`HeightGeometry::value`]).
#[derive(Clone, Debug, PartialEq)]
pub enum KeyValue<P> {
    /// An exact flat predicate whose value is a positive multiple of the
    /// key's ([`PointHeight`](crate::perturb::PointHeight),
    /// [`EdgeEdge`](crate::perturb::EdgeEdge)); its orders are not read.
    Flat(P),
    /// An exact sign worked out on flat data otherwise (a column hit's
    /// height, a sign condition at a root: [`hit`]).
    Exact(ExactSign),
    /// A curved value, a length read at each precision level
    /// ([`HeightGeometry::curved_value`]).
    Curved,
}

/// How an anchor's perturbed orders are read ([`HeightGeometry::row`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AnchorRow<P> {
    /// An exact flat predicate whose orders are the anchor's row (its own
    /// value is not read).
    Flat(P),
    /// The orders' exact signs worked out on flat data otherwise (a
    /// ribbon's tangent plane at a hit,
    /// [`ribbon_row_signs`](crate::perturb::ribbon_row_signs)).
    Exact([ExactSign; ORDERS]),
    /// Curved orders read at each precision level
    /// ([`HeightGeometry::curved_row`]).
    Curved,
}

/// The geometry height records read. Every value and row is oriented as
/// the key's stored answer: for a vertex–vertex or crossing key the
/// height of `B`'s point (or edge) above `A`'s, for a point–curve or
/// hit key the height of the target above the key's point.
pub trait HeightGeometry {
    /// The flat predicates of keys and rows.
    type Flat: Pred + Sync;

    /// Height key `h`'s value (order 0), shared by every anchor.
    fn value(&self, h: HKey) -> KeyValue<Self::Flat>;

    /// A curved key's value at the level of `like`; `None` where the
    /// level can't form it (a facing or a hit it can't certify), which
    /// escalates. Work past the one evaluation the record charges (a
    /// z-order localization's sub-boxes) is added to `work`.
    fn curved_value<S: Curved>(&self, h: HKey, like: &S, work: &mut LevelWork) -> Option<S>;

    /// Anchor `k`'s perturbed orders: the anchor's tangent element against
    /// the key's moving point (§ "Rows" in [`crate::perturb`]).
    fn row(&self, k: HeightKey) -> AnchorRow<Self::Flat>;

    /// A curved row's orders `g₁ … g₄` at the level of `like`; `None`
    /// where the level can't certify the anchor's facing, which
    /// escalates (where no level does, the fold rule is the consumer's).
    /// Work past the one evaluation the record charges is added to
    /// `work`.
    fn curved_row<S: Curved>(
        &self,
        k: HeightKey,
        like: &S,
        work: &mut LevelWork,
    ) -> Option<[S; ORDERS]>;

    /// Whether hit key `h`'s value, not formed at the level of `like`
    /// ([`curved_value`](Self::curved_value) `None`), fails only for the
    /// face's facing: the hit and the face there are formed, the
    /// face's normal at the hit not certified off `ê`. At the last level
    /// such a value is a tie (module docs, "The fold rule"); a value
    /// failing for anything else (the hit, the face) escalates. By
    /// default none is.
    fn facing_only<S: Curved>(&self, h: HKey, like: &S) -> bool {
        let _ = (h, like);
        false
    }

    /// Where anchor `k`'s facing isn't certified at any level (the anchor
    /// on a silhouette inside its face), the fold's upper sheet's facing
    /// at the level of `like`: a value whose sign is `+` where the upper
    /// sheet faces up (module docs, "The fold rule"); `None` where the
    /// level can't form it. By default there is no fold rule: such an
    /// anchor's record can't be decided.
    fn curved_fold<S: Curved>(&self, k: HeightKey, like: &S) -> Option<S> {
        let _ = (k, like);
        None
    }

    /// Whether anchor `k`'s record is geometric (its value a certified
    /// truth, never a tie). A key is geometric only when every anchor
    /// reading it is.
    fn geometric(&self, k: HeightKey) -> bool;
}

/// A height key's order-0 decision, shared by every anchor reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyRec {
    /// The value's sign (oriented as the key), or `None` for a tie,
    /// decided per anchor by its orders.
    pub value: Option<Sign>,
    /// The band class the value was decided under: geometric when every
    /// anchor is, else flat or curved by the key's data.
    pub kind: BandKind,
    /// The precision level it was decided at (0 for an exact value), the
    /// level each anchor's curved orders start at.
    pub level: u8,
}

/// A height record at one anchor, oriented as the key (as read through
/// [`Heights::get`], from the reader's side).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HeightRec {
    /// `Pos`: the target is above. On a fold (`fold`), the facing of the
    /// fold's upper sheet instead, `Pos` facing up, read as stored from
    /// either side.
    pub above: Sign,
    /// Whether the key's value was a tie, so the anchor's orders decided.
    pub by_orders: bool,
    /// Whether the anchor's facing wasn't certified at any level, so the
    /// fold rule decided: `above` is the upper sheet's facing.
    pub fold: bool,
}

/// How filling height records failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeightError<E> {
    /// A pass failed: nothing of its batch was stored.
    Pass(E),
    /// A pass's answers couldn't be stored (a bug of the pass).
    Table(TableError<RecordKey>),
    /// A key decided as geometric is asked by an anchor that isn't: its
    /// band would change after it was decided. Every anchor of a key is
    /// asked in the batch that decides it.
    Band(HKey),
}

/// The height records of a run: each key's value decided once
/// ([`KeyRec`]), each anchor's record once ([`HeightRec`]).
#[derive(Debug, Default)]
pub struct Heights {
    keys: Table<HKey, KeyRec>,
    at: Table<HeightKey, HeightRec>,
}

impl Heights {
    /// No records.
    pub fn new() -> Heights {
        Heights::default()
    }

    /// Decides the height records of `wanted` not yet stored: first the
    /// values of their keys not yet stored, in one pass, under the band
    /// class of all the batch's anchors of each (curved values at the
    /// curved band `band`, the current rung's), then the anchors, in
    /// another, reading those.
    pub fn fill<G, P>(
        &mut self,
        wanted: impl IntoIterator<Item = HeightKey>,
        geom: &G,
        band: Band,
        pass: &mut P,
    ) -> Result<(), HeightError<P::Error>>
    where
        G: HeightGeometry + Sync,
        P: Pass,
    {
        let batch = self
            .at
            .missing(wanted)
            .map_err(|e| HeightError::Table(table_error(e, RecordKey::HeightAt)))?;
        if batch.is_empty() {
            return Ok(());
        }
        // A key is geometric only when every anchor asking it is.
        let mut geometric: BTreeMap<HKey, bool> = BTreeMap::new();
        for k in batch.keys() {
            let g = geom.geometric(**k);
            *geometric.entry(k.h).or_insert(true) &= g;
        }
        for (&h, &g) in &geometric {
            if let Some(r) = self.keys.get(&h)
                && r.kind == BandKind::Geometric
                && !g
            {
                return Err(HeightError::Band(h));
            }
        }
        let fresh = self
            .keys
            .missing(geometric.keys().copied())
            .map_err(|e| HeightError::Table(table_error(e, RecordKey::Height)))?;
        if !fresh.is_empty() {
            let values = pass
                .run(fresh.keys(), &|h: &Fresh<HKey>| {
                    decide_key(**h, geometric[&**h], geom, band)
                })
                .map_err(HeightError::Pass)?;
            self.keys
                .insert(fresh, values)
                .map_err(|e| HeightError::Table(table_error(e, RecordKey::Height)))?;
        }
        let keys = &self.keys;
        let values = pass
            .run(batch.keys(), &|k: &Fresh<HeightKey>| {
                decide_at(**k, keys, geom, band)
            })
            .map_err(HeightError::Pass)?;
        self.at
            .insert(batch, values)
            .map_err(|e| HeightError::Table(table_error(e, RecordKey::HeightAt)))
    }

    /// Key `h`'s order-0 decision; `None` if it isn't decided.
    pub fn key(&self, h: HKey) -> Option<KeyRec> {
        self.keys.get(&h).copied()
    }

    /// The height record of key `h` (as a reader asked it, from
    /// [`height_key`]) at `anchor`, from the reader's side: `above`
    /// negated where the reader's point is the key's `B` point. `None` if
    /// it isn't decided.
    pub fn get(&self, h: Oriented<HKey>, anchor: Anchor) -> Option<HeightRec> {
        let r = *self.at.get(&HeightKey { h: h.key, anchor })?;
        Some(if h.swapped && !r.fold {
            HeightRec {
                above: neg(r.above),
                ..r
            }
        } else {
            r
        })
    }

    /// The key values' table, for its reads and keys.
    pub fn key_table(&self) -> &Table<HKey, KeyRec> {
        &self.keys
    }

    /// The anchors' table, for its reads and keys.
    pub fn table(&self) -> &Table<HeightKey, HeightRec> {
        &self.at
    }

    /// Decides every stored record again, on `geom` at `band`, and gives
    /// the key of the first whose answer differs (key values first, each
    /// table in key order). A key's band class is replayed as stored.
    pub fn replay<G: HeightGeometry>(&self, geom: &G, band: Band) -> Result<(), RecordKey> {
        let keys = &self.keys;
        replay(
            keys,
            |h| {
                let g = keys.get(h.key()).map(|r| r.kind) == Some(BandKind::Geometric);
                decide_key(**h, g, geom, band)
            },
            RecordKey::Height,
        )?;
        replay(
            &self.at,
            |k| decide_at(**k, keys, geom, band),
            RecordKey::HeightAt,
        )
    }
}

/// Whether anchor `k` may read its key: a point–curve or hit key's anchor
/// is of the other operand than its point, a hit key's its own face.
fn anchor_fits(k: HeightKey) -> bool {
    let (op, face) = match k.anchor {
        Anchor::Face(op, f) => (op, Some(f)),
        Anchor::Edge(op, _) => (op, None),
    };
    match k.h {
        HKey::VV { .. } | HKey::X { .. } | HKey::XRoot { .. } => true,
        HKey::Curve { q, .. } => op == q.operand.other(),
        HKey::Hit { q, f, .. } => op == q.operand.other() && face == Some(f),
    }
}

/// A record's tie key, for evidence.
fn tie_key(record: RecordKey, anchor: Option<Anchor>) -> TieKey {
    let row = match anchor {
        Some(Anchor::Edge(..)) => Row::ZOrder,
        _ => Row::ColumnHit,
    };
    TieKey { row, record }
}

/// Decides key `h`'s value: exact on flat data, else at each level in
/// turn until one decides it or calls it a tie at the target.
fn decide_key<G: HeightGeometry>(
    h: HKey,
    geometric: bool,
    geom: &G,
    band: Band,
) -> Decided<KeyRec> {
    let tie = tie_key(RecordKey::Height(h), None);
    let mut cost = Cost::default();
    let exact = match geom.value(h) {
        KeyValue::Flat(p) => {
            let (s, work) = exact::value_sign(&p);
            cost.exact = work;
            Some(s)
        }
        KeyValue::Exact(s) => Some(s),
        KeyValue::Curved => None,
    };
    match exact {
        Some(s) => {
            let kind = if geometric {
                BandKind::Geometric
            } else {
                BandKind::Flat
            };
            let value = match s {
                Err(_) => return (Err(Fail::Internal { key: tie }), cost),
                Ok(o) => Sign::of(o),
            };
            if value.is_none() && geometric {
                // A truth is never a tie: refining exact data can't help.
                return (Err(Fail::Escalate { order: 0 }), cost);
            }
            let rec = KeyRec {
                value,
                kind,
                level: 0,
            };
            (Ok(rec), cost)
        }
        None => {
            let b = if geometric { Band::GEOMETRIC } else { band };
            let mut at = KeyAt { h, geom, b };
            let r = ladder(&mut at, 0, &mut cost).expect("the first level runs");
            (r, cost)
        }
    }
}

/// A curved key's value at one level.
struct KeyAt<'a, G> {
    h: HKey,
    geom: &'a G,
    b: Band,
}

impl<G: HeightGeometry> AtLevel for KeyAt<'_, G> {
    type Out = Result<KeyRec, Fail>;

    fn escalates(out: &Self::Out) -> bool {
        matches!(out, Err(Fail::Escalate { .. }))
    }

    fn at<S: RayLevel>(&mut self, like: &S, level: usize, work: &mut LevelWork) -> Self::Out {
        work.keys = work.keys.saturating_add(1);
        let level = u8::try_from(level).expect("few levels");
        let escalate = Err(Fail::Escalate { order: 0 });
        let Some(v) = self.geom.curved_value(self.h, like, work) else {
            // A hit's value can't be formed where the face's facing isn't
            // certified (a silhouette inside the face). At the last level
            // that alone is a tie (an unformed hit escalates): the
            // anchor's own orders decide, and where its facing isn't
            // certified either, the fold rule. A truth is never a tie.
            if matches!(self.h, HKey::Hit { .. })
                && usize::from(level) + 1 == LEVELS
                && self.b.kind() != BandKind::Geometric
                && self.geom.facing_only(self.h, like)
            {
                return Ok(KeyRec {
                    value: None,
                    kind: self.b.kind(),
                    level,
                });
            }
            return escalate;
        };
        let rec = |value| KeyRec {
            value,
            kind: self.b.kind(),
            level,
        };
        if let Some(s) = v.beyond(self.b.tau()) {
            return Ok(rec(Some(s)));
        }
        let err = Coeff::err(&v);
        let at_target = err < f64::INFINITY && err <= self.b.tau_sign();
        if self.b.kind() == BandKind::Geometric || !at_target {
            return escalate;
        }
        Ok(rec(None))
    }
}

/// Decides anchor `k`'s record from its key's stored value and, at a tie,
/// its own orders.
fn decide_at<G: HeightGeometry>(
    k: HeightKey,
    keys: &Table<HKey, KeyRec>,
    geom: &G,
    band: Band,
) -> Decided<HeightRec> {
    let tie = tie_key(RecordKey::HeightAt(k), Some(k.anchor));
    let mut cost = Cost::default();
    let internal = Err(Fail::Internal { key: tie });
    let Some(&key) = keys.get(&k.h).filter(|_| anchor_fits(k)) else {
        // The batch's keys are decided first; a missing one, or an anchor
        // the key can't have, is a bug.
        return (internal, cost);
    };
    if let Some(above) = key.value {
        let rec = HeightRec {
            above,
            by_orders: false,
            fold: false,
        };
        return (Ok(rec), cost);
    }
    let above = match geom.row(k) {
        AnchorRow::Flat(p) => {
            let (s, work) = decide_flat_orders(std::cmp::Ordering::Equal, &p, tie);
            cost.exact = work;
            s.map(|s| (s, false))
        }
        AnchorRow::Exact(ord) => {
            decide(Ok(std::cmp::Ordering::Equal), ord, Band::FLAT, tie).map(|s| (s, false))
        }
        AnchorRow::Curved => {
            let mut at = RowAt { k, geom, band, tie };
            ladder(&mut at, usize::from(key.level), &mut cost)
                .unwrap_or(Err(Fail::Escalate { order: 0 }))
        }
    };
    let rec = above.map(|(above, fold)| HeightRec {
        above,
        by_orders: true,
        fold,
    });
    (rec, cost)
}

/// An anchor's curved orders at one level.
struct RowAt<'a, G> {
    k: HeightKey,
    geom: &'a G,
    band: Band,
    tie: TieKey,
}

impl<G: HeightGeometry> AtLevel for RowAt<'_, G> {
    /// The sign, and whether the fold rule gave it.
    type Out = Result<(Sign, bool), Fail>;

    fn escalates(out: &Self::Out) -> bool {
        matches!(out, Err(Fail::Escalate { .. }))
    }

    fn at<S: RayLevel>(&mut self, like: &S, level: usize, work: &mut LevelWork) -> Self::Out {
        work.keys = work.keys.saturating_add(1);
        let Some(ord) = self.geom.curved_row(self.k, like, work) else {
            // The facing isn't certified here. A later level may certify
            // it; at the last, the anchor is on a fold.
            if level + 1 == LEVELS {
                work.keys = work.keys.saturating_add(1);
                if let Some(f) = self.geom.curved_fold(self.k, like)
                    && let Some(s) = f.beyond(0.0)
                {
                    return Ok((s, true));
                }
            }
            return Err(Fail::Escalate { order: 1 });
        };
        // The value is the key's tie: zero here, the orders deciding.
        let zero = S::int(0, like);
        decide(zero, ord, self.band, self.tie).map(|s| (s, false))
    }
}

#[cfg(test)]
mod tests;

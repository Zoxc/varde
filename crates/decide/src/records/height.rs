//! Height keys, layers and records: which geometry a height above or
//! below is read from ([`height_key`]), the layers of a face at a point
//! whose feature names it ([`Layers`]), and the heights decided
//! ([`Heights`]).
//!
//! **The key.** A point `q` asks its height against a target of the
//! other operand: a vertex `w`, an edge `g` at one of `q`'s feet `k` on
//! it, or a face `f` at one of its layers `l` at `q`. The answer is a
//! height key ([`HKey`]), the one geometry the height's value is decided
//! from for every anchor that reads it:
//!
//! - against a vertex `w`: the point–vertex key `VV(q, w)`, one for the
//!   pair whichever point asks;
//! - a cut point `Cut(e, g, c)` against its own edge `g`: the crossing
//!   record's cluster itself, `X(e, g, c)`;
//! - against an edge `g` at foot `k`: `VV(q, w)` for the first of `g`'s
//!   end vertices lying in `k` (geometric order) that is near `q` (its
//!   point–vertex record says so), so every edge at `w` and `w` itself
//!   share one height; else the point–curve key `Curve(q, g, k)`;
//! - against a face `f` at layer `l`: the key of the layer's **feature**
//!   (its vertex, or its foot's edge at that foot, as above), else the
//!   point–face key `Face(q, f, l)` (a core layer, read at the hit of
//!   `q`'s column on it).
//!
//! **Layers.** A layer of `f` at `q` is a 3D component of the part of
//! `f`'s counting surface within `q`'s near radius. Its features are the
//! vertices of `f` lying in it and the feet of `q` on `f`'s edges lying in
//! it; its **feature** is the first near vertex among them in geometric
//! order, else the first foot (by its edge's place in the geometric order,
//! then its parameter from the edge's lesser end), else none
//! ([`Feature`], [`Place`]). No distance is compared and no id chooses.
//!
//! **Partition-free.** The counting finds a layer as pieces (boxes, strip
//! and corner pieces) it joins where they meet; [`LayerPieces`] takes the
//! pieces in any order and cut, joins the ones it is told meet and the
//! ones holding a common feature (a vertex or a foot lies in one
//! component), and numbers the components ([`LayerId`]) by their feature's
//! place, core layers after, by the geometric key the caller gives each
//! (a point of the layer's own, the hit, the same for every piece of it).
//! So a layer's id, its feature and its key are functions of `(q, f, l)`:
//! every piece of one layer reads one height record, whatever boxes,
//! depths or pieces produced it.
//!
//! **The face key's value** is `n_f·(x_h − q)·sign(n_f·ê)`, `x_h` the hit:
//! the height of the face above `q` along `ê`, positive where the face is
//! above whichever way it faces. Flat, it is
//! [`PlaneHeight`](crate::perturb::PlaneHeight)'s value (a positive
//! multiple); curved, [`face_height`](crate::perturb::face_height). Where
//! the facing `sign(n_f·ê)` isn't certified the in-face fold rule decides
//! instead.
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
//! layers and the edges of crossings ([`Anchor`]); a point–face key is
//! read at its own face only. Both live in [`Table`]s, decided in one
//! charged [`Pass`] each ([`Heights::fill`]): every anchor of a key is
//! asked in the batch that decides it, as its band is theirs.

use std::collections::BTreeMap;

use smallvec::SmallVec;

use crate::ids::{EdgeId, FaceId, VertId};
use crate::num::decide::{Band, BandKind, Coeff, Fail, Row, Sign, TieKey, decide};
use crate::num::exact::{self, ORDERS, Pred};
use crate::order::{Along, EdgeKey, EntityOrder, Pos, VertKey};
use crate::perturb::{Curved, decide_flat_orders};
use crate::records::feet::{Feet, NearFeet};
use crate::records::keys::{
    Anchor, FootId, HKey, HeightKey, LayerId, Operand, Oriented, Point, RayKey, RecordKey, Target,
    VvKey,
};
use crate::records::ray::RayLevel;
use crate::records::shadow::{
    AtLevel, Cost, Decided, LevelWork, Pass, Shadows, VvRec, ladder, neg, replay, table_error,
};
use crate::records::table::{Fresh, Table, TableError};

/// What [`height_key`] and the layers read: the geometric order of the
/// other operand's vertices and edges, the point–vertex records (their
/// `near`) and the feet.
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
/// target names a foot or layer that isn't there. A caller's bug.
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
    /// Face `f` has no layer `layer` at `q`, or its layers at `q` aren't
    /// given.
    NoLayer {
        /// The point.
        q: Point,
        /// The face.
        f: FaceId,
        /// The layer.
        layer: LayerId,
    },
    /// A core layer of face `f` at `q` was given no geometric key.
    NoCore {
        /// The point.
        q: Point,
        /// The face.
        f: FaceId,
    },
    /// Two core layers of face `f` at `q` were given one geometric key.
    CoreTie {
        /// The point.
        q: Point,
        /// The face.
        f: FaceId,
    },
}

/// A feature of a layer: a vertex of the face, or a foot of the point on
/// one of the face's edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// A vertex.
    Vert(VertId),
    /// A foot on an edge.
    Foot(EdgeId, FootId),
}

/// A feature's place in the geometric order: near vertices first, by
/// their place, then feet, by their edge's place and then the foot's rank
/// along the edge from its lesser end. Ids never enter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place<K> {
    /// A near vertex.
    Vert(VertKey<K>),
    /// A foot: its edge's place, then its rank from the edge's lesser end.
    Foot(EdgeKey<K>, u32),
}

/// The place of feature `x` of a face of `of` (the operand other than
/// `q`'s), `None` for a vertex that isn't near `q`.
fn place<R: HeightReads>(
    reads: &R,
    q: Point,
    of: Operand,
    x: Feature,
) -> Result<Option<Place<R::Key>>, KeyError> {
    match x {
        Feature::Vert(w) => Ok(near(reads, q, w)?.then(|| Place::Vert(reads.vert_key(of, w)))),
        Feature::Foot(g, k) => {
            let feet = feet_of(reads, q, g)?;
            if feet.get(k).is_none() {
                return Err(KeyError::NoFoot { q, g, foot: k });
            }
            let rank = feet
                .in_order(along(reads, of, g))
                .position(|f| f == k)
                .expect("the foot is there");
            let rank = u32::try_from(rank).expect("few feet");
            Ok(Some(Place::Foot(reads.edge_key(of, g), rank)))
        }
    }
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

/// One layer of a face at a point: its features and its feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// The face's vertices in it, sorted by id.
    pub verts: SmallVec<[VertId; 2]>,
    /// The point's feet on the face's edges in it, sorted by id.
    pub feet: SmallVec<[(EdgeId, FootId); 2]>,
    /// Its feature (module docs); `None` for a core layer.
    pub feature: Option<Feature>,
    /// A core layer's geometric key, as given.
    pub core: Option<Pos>,
}

/// The layers of a face at a point, numbered by [`LayerId`] (module
/// docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layers {
    layers: Vec<Layer>,
}

impl Layers {
    /// Layer `l`, if there is one.
    pub fn get(&self, l: LayerId) -> Option<&Layer> {
        usize::try_from(l.0).ok().and_then(|i| self.layers.get(i))
    }

    /// The layers, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (LayerId, &Layer)> {
        (0u32..).map(LayerId).zip(&self.layers)
    }

    /// The number of layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The layer holding feature `x`, if one does.
    pub fn holding(&self, x: Feature) -> Option<LayerId> {
        self.iter()
            .find(|(_, l)| match x {
                Feature::Vert(v) => l.verts.contains(&v),
                Feature::Foot(g, k) => l.feet.contains(&(g, k)),
            })
            .map(|(id, _)| id)
    }
}

/// The pieces a face's layers at a point are found as, before they are
/// joined (module docs). Pieces are given in any order.
#[derive(Clone, Debug, Default)]
pub struct LayerPieces {
    pieces: Vec<PieceFeatures>,
    joins: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, Default)]
struct PieceFeatures {
    verts: SmallVec<[VertId; 2]>,
    feet: SmallVec<[(EdgeId, FootId); 2]>,
    core: Option<Pos>,
}

impl LayerPieces {
    /// No pieces.
    pub fn new() -> LayerPieces {
        LayerPieces::default()
    }

    /// Adds a piece holding the features `features` and, where its layer
    /// has none, the layer's geometric key `core` (the same for every
    /// piece of a layer, a `−0` read as `+0`); gives its index.
    pub fn piece(
        &mut self,
        features: impl IntoIterator<Item = Feature>,
        core: Option<Pos>,
    ) -> usize {
        // `−0` reads as `+0`: one point, whichever way its hit was
        // computed, so a layer's key and the ties between keys don't
        // depend on a zero's sign.
        let core = core.map(|c| Pos::new(c.get().map(|x| x + 0.0)).expect("no NaN"));
        let mut p = PieceFeatures {
            core,
            ..PieceFeatures::default()
        };
        for x in features {
            match x {
                Feature::Vert(v) => p.verts.push(v),
                Feature::Foot(g, k) => p.feet.push((g, k)),
            }
        }
        self.pieces.push(p);
        self.pieces.len() - 1
    }

    /// Pieces `a` and `b` meet: they are of one layer.
    pub fn join(&mut self, a: usize, b: usize) {
        assert!(
            a < self.pieces.len() && b < self.pieces.len(),
            "no such piece"
        );
        self.joins.push((a, b));
    }

    /// The layers of face `f` (of the operand other than `q`'s) at `q`:
    /// the components of the pieces, joined where told and where they
    /// share a feature, numbered by their feature's place, core layers
    /// after by their key.
    pub fn layers<R: HeightReads>(
        &self,
        reads: &R,
        q: Point,
        f: FaceId,
    ) -> Result<Layers, KeyError> {
        let of = q.operand.other();
        let n = self.pieces.len();
        let mut parent: Vec<usize> = (0..n).collect();
        let mut joins = self.joins.clone();
        let mut owner: BTreeMap<Feature, usize> = BTreeMap::new();
        for (i, p) in self.pieces.iter().enumerate() {
            let xs = p.verts.iter().map(|&v| Feature::Vert(v));
            let xs = xs.chain(p.feet.iter().map(|&(g, k)| Feature::Foot(g, k)));
            for x in xs {
                if let Some(&j) = owner.get(&x) {
                    joins.push((i, j));
                } else {
                    owner.insert(x, i);
                }
            }
        }
        for (a, b) in joins {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            // The lesser index is the root: the same for any join order.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            parent[hi] = lo;
        }
        let mut comps: BTreeMap<usize, PieceFeatures> = BTreeMap::new();
        for (i, p) in self.pieces.iter().enumerate() {
            let r = root(&mut parent, i);
            let c = comps.entry(r).or_default();
            c.verts.extend_from_slice(&p.verts);
            c.feet.extend_from_slice(&p.feet);
            c.core = match (c.core, p.core) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        type Sort<K> = (bool, Option<Place<K>>, Option<Pos>, Layer);
        let mut out: Vec<Sort<R::Key>> = Vec::with_capacity(comps.len());
        for c in comps.into_values() {
            let mut verts = c.verts;
            verts.sort_unstable();
            verts.dedup();
            let mut feet = c.feet;
            feet.sort_unstable();
            feet.dedup();
            let xs = verts.iter().map(|&v| Feature::Vert(v));
            let xs = xs.chain(feet.iter().map(|&(g, k)| Feature::Foot(g, k)));
            let mut best: Option<(Place<R::Key>, Feature)> = None;
            for x in xs {
                if let Some(p) = place(reads, q, of, x)? {
                    // Ties of the place (bitwise-equal entities) go to
                    // the lesser id.
                    if best.as_ref().is_none_or(|(b, bx)| (&p, x) < (b, *bx)) {
                        best = Some((p, x));
                    }
                }
            }
            let (pl, feature) = best.map_or((None, None), |(p, x)| (Some(p), Some(x)));
            let core = if feature.is_some() { None } else { c.core };
            if feature.is_none() && core.is_none() {
                // A core layer is numbered by its key, even alone: a
                // second one found later would otherwise renumber it.
                return Err(KeyError::NoCore { q, f });
            }
            let layer = Layer {
                verts,
                feet,
                feature,
                core,
            };
            out.push((pl.is_none(), pl, core, layer));
        }
        // Layers whose features tie in place (bitwise-equal entities) go
        // by the feature's id: one layer holds each feature, so the order
        // is total and the pieces' order never enters.
        out.sort_by(|a, b| (a.0, &a.1, a.2, a.3.feature).cmp(&(b.0, &b.1, b.2, b.3.feature)));
        for w in out.windows(2) {
            if w[0].0 && w[1].0 && w[0].2 == w[1].2 {
                return Err(KeyError::CoreTie { q, f });
            }
        }
        Ok(Layers {
            layers: out.into_iter().map(|s| s.3).collect(),
        })
    }
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The layers of the faces at the points of a run, by `(q, f)`.
#[derive(Clone, Debug, Default)]
pub struct LayerTable {
    layers: BTreeMap<(Point, FaceId), Layers>,
}

impl LayerTable {
    /// None.
    pub fn new() -> LayerTable {
        LayerTable::default()
    }

    /// Stores face `f`'s layers at `q`; gives back what was stored before.
    pub fn insert(&mut self, q: Point, f: FaceId, layers: Layers) -> Option<Layers> {
        self.layers.insert((q, f), layers)
    }

    /// Face `f`'s layers at `q`.
    pub fn get(&self, q: Point, f: FaceId) -> Option<&Layers> {
        self.layers.get(&(q, f))
    }
}

/// The height key point `q` reads against target `t` of the other
/// operand (module docs): oriented as `q` asked it, `swapped` where `q`
/// is the key's `B` point.
pub fn height_key<R: HeightReads>(
    reads: &R,
    layers: &LayerTable,
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
                if near(reads, q, w)? {
                    return Ok(HKey::vv(q, w));
                }
            }
            Ok(own(HKey::Curve { q, g, foot: k }))
        }
        Target::Layer(f, l) => {
            let layer = layers
                .get(q, f)
                .and_then(|ls| ls.get(l))
                .ok_or(KeyError::NoLayer { q, f, layer: l })?;
            match layer.feature {
                Some(Feature::Vert(w)) => height_key(reads, layers, q, Target::Vert(w)),
                Some(Feature::Foot(g, k)) => height_key(reads, layers, q, Target::Edge(g, k)),
                None => Ok(own(HKey::Face { q, f, layer: l })),
            }
        }
    }
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
    /// [`PlaneHeight`](crate::perturb::PlaneHeight),
    /// [`EdgeEdge`](crate::perturb::EdgeEdge)); its orders are not read.
    Flat(P),
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
    /// Curved orders read at each precision level
    /// ([`HeightGeometry::curved_row`]).
    Curved,
}

/// The geometry height records read. Every value and row is oriented as
/// the key's stored answer: for a vertex–vertex or crossing key the
/// height of `B`'s point (or edge) above `A`'s, for a point–curve or
/// point–face key the height of the target above the key's point.
pub trait HeightGeometry {
    /// The flat predicates of keys and rows.
    type Flat: Pred + Sync;

    /// Height key `h`'s value (order 0), shared by every anchor.
    fn value(&self, h: HKey) -> KeyValue<Self::Flat>;

    /// A curved key's value at the level of `like`; `None` where the
    /// level can't form it (a facing or a hit it can't certify), which
    /// escalates.
    fn curved_value<S: Curved>(&self, h: HKey, like: &S) -> Option<S>;

    /// Anchor `k`'s perturbed orders: the anchor's tangent element against
    /// the key's moving point (§ "Rows" in [`crate::perturb`]).
    fn row(&self, k: HeightKey) -> AnchorRow<Self::Flat>;

    /// A curved row's orders `g₁ … g₄` at the level of `like`; `None`
    /// where the level can't certify the anchor's facing, which
    /// escalates (where no level does, the fold rule is the consumer's).
    fn curved_row<S: Curved>(&self, k: HeightKey, like: &S) -> Option<[S; ORDERS]>;

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
    /// `Pos`: the target is above.
    pub above: Sign,
    /// Whether the key's value was a tie, so the anchor's orders decided.
    pub by_orders: bool,
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
        let batch = self.at.missing(wanted);
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
        let fresh = self.keys.missing(geometric.keys().copied());
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
        Some(if h.swapped {
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

/// Whether anchor `k` may read its key: a point–curve or point–face key's
/// anchor is of the other operand than its point, a point–face key's its
/// own face.
fn anchor_fits(k: HeightKey) -> bool {
    let (op, face) = match k.anchor {
        Anchor::Face(op, f) => (op, Some(f)),
        Anchor::Edge(op, _) => (op, None),
    };
    match k.h {
        HKey::VV { .. } | HKey::X { .. } => true,
        HKey::Curve { q, .. } => op == q.operand.other(),
        HKey::Face { q, f, .. } => op == q.operand.other() && face == Some(f),
    }
}

/// A record's tie key, for evidence.
fn tie_key(record: RecordKey, anchor: Option<Anchor>) -> TieKey {
    let row = match anchor {
        Some(Anchor::Edge(..)) => Row::ZOrder,
        _ => Row::LayerHeight,
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
    match geom.value(h) {
        KeyValue::Flat(p) => {
            let (s, work) = exact::value_sign(&p);
            cost.exact = work;
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
        KeyValue::Curved => {
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
        work.evals = work.evals.saturating_add(1);
        let level = u8::try_from(level).expect("few levels");
        let escalate = Err(Fail::Escalate { order: 0 });
        let Some(v) = self.geom.curved_value(self.h, like) else {
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
        };
        return (Ok(rec), cost);
    }
    let above = match geom.row(k) {
        AnchorRow::Flat(p) => {
            let (s, work) = decide_flat_orders(std::cmp::Ordering::Equal, &p, tie);
            cost.exact = work;
            s
        }
        AnchorRow::Curved => {
            let mut at = RowAt { k, geom, band, tie };
            ladder(&mut at, usize::from(key.level), &mut cost)
                .unwrap_or(Err(Fail::Escalate { order: 0 }))
        }
    };
    let rec = above.map(|above| HeightRec {
        above,
        by_orders: true,
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
    type Out = Result<Sign, Fail>;

    fn escalates(out: &Self::Out) -> bool {
        matches!(out, Err(Fail::Escalate { .. }))
    }

    fn at<S: RayLevel>(&mut self, like: &S, _: usize, work: &mut LevelWork) -> Self::Out {
        work.evals = work.evals.saturating_add(1);
        let Some(ord) = self.geom.curved_row(self.k, like) else {
            return Err(Fail::Escalate { order: 1 });
        };
        // The value is the key's tie: zero here, the orders deciding.
        let zero = S::int(0, like);
        decide(zero, ord, self.band, self.tie)
    }
}

#[cfg(test)]
mod tests;

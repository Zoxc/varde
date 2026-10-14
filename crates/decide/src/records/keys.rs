//! The canonical keys of records: which entities a record is about, by
//! operand **id**, never by name or position, so each fact has exactly one
//! key and every reader finds the one stored answer.
//!
//! **Operands.** A boolean reads two operands, `A` and `B` ([`Operand`]);
//! their ids are separate, so a key says which operand each id is of,
//! by its slot or by an [`Operand`] beside it. A key between an element
//! of each puts `A`'s first. Edges are canonical in the store (their
//! curve runs from the lower vertex id to the higher), so an edge id
//! fixes a direction too.
//!
//! **Query points.** A record about a point asks it of a [`Query`]: a
//! vertex, or a **cut point**, the point of an edge at one cluster of its
//! crossing record with an edge of the other operand ([`Query::Cut`]).
//! [`Point`] is a query with its operand.
//!
//! **One fact, one key.** Rays from `A` points go `−r̂`, from `B` points
//! `+r̂`, so "`a`'s ray passes `b`" and "`b`'s ray passes `a`" are one
//! fact, and a point–vertex record is keyed by the pair, `A`'s point
//! first, whichever point asked ([`VvKey::of`], [`HKey::vv`]); so is a
//! cut point's height against its own crossing ([`HKey::x`]). A reader
//! whose point is the `B` one gets the key [`Oriented::swapped`]: it reads
//! the stored answer from the other side, never deciding it again. A
//! stored answer is oriented from `A`'s point to `B`'s (which side of
//! `a`'s `r̂`-line `b` is on, whether `b` is ahead of `a` along `r̂`,
//! whether `b` is above `a`), so the `B` reader negates exactly those
//! parts, antisymmetric under the swap; the symmetric ones (`near`, and
//! "the ray passes", as the two rays point opposite ways) it reads as
//! stored. Which parts are which is each record's own rule.
//!
//! **Heights.** A height record's value is decided once per height key
//! ([`HKey`], the plan's `height_key(q, target)`): the point–point,
//! edge–edge, point–curve or point–face geometry the height is read
//! from, shared by every anchor that reads it. Its perturbed orders are
//! the anchor's own, so the per-anchor record is keyed by both
//! ([`HeightKey`]). Which height key a query and target read (a near
//! vertex, a foot, a layer's feature, in geometric order) is worked out
//! from other records; the keys here are what it yields.
//!
//! [`RecordKey`] is any record's key, for evidence: what
//! [`Fail::Internal`](crate::num::decide::Fail::Internal) names.
//!
//! Keys order by their ids: the order a record table stores and decides
//! them in. Ids order work only; no answer reads that order.

use crate::bezier::Dir;
use crate::ids::{EdgeId, FaceId, VertId};

/// One of the two operands of a boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operand {
    /// The first operand: the one that moves under the perturbation, whose
    /// rays go `−r̂`.
    A,
    /// The second operand: fixed, its rays going `+r̂`.
    B,
}

impl Operand {
    /// The other operand.
    pub fn other(self) -> Operand {
        match self {
            Operand::A => Operand::B,
            Operand::B => Operand::A,
        }
    }
}

/// A cluster of the crossing record of an edge pair: its index among the
/// pair's clusters as that record lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XClusterIx(pub u32);

/// A foot of a point on an edge of the other operand: a 3D component of
/// the edge's parameters within the edge's near radius of the point, by
/// its index in parameter order along the stored edge (from its
/// lower-id end), on a closed edge a foot through its vertex first
/// ([`super::feet`]). An identity only: where a choice reads the order of
/// feet it is the geometric one, the parameter from the edge's
/// geometrically lesser end, which runs the other way when that end has
/// the higher id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FootId(pub u32);

/// A layer of a face at a point: a 3D component of the face's counting
/// surface within the point's near radius, by its index among the
/// face's layers at that point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub u32);

/// A point a record is asked of, on its operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Query {
    /// A vertex.
    Vert(VertId),
    /// A cut point `Cut(e, g, c)`: the point of edge `e` (of the query's
    /// operand) at cluster `c` of the crossing record of `e` and `g` (an
    /// edge of the other operand), the record keyed by `A`'s edge first
    /// whichever operand `e` is of.
    Cut(EdgeId, EdgeId, XClusterIx),
}

/// A query point with its operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    /// The operand the point is on.
    pub operand: Operand,
    /// The point.
    pub q: Query,
}

impl Point {
    /// Vertex `v` of `operand`.
    pub fn vert(operand: Operand, v: VertId) -> Point {
        Point {
            operand,
            q: Query::Vert(v),
        }
    }
}

/// What a height is asked against, on the operand other than the query
/// point's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// A vertex.
    Vert(VertId),
    /// An edge, at one of the point's feet on it.
    Edge(EdgeId, FootId),
    /// A face, at one of its layers at the point.
    Layer(FaceId, LayerId),
}

/// A key as a reader asked for it: the canonical key and whether the
/// reader's point is its second (`B`) one, so the reader negates the
/// stored answer's antisymmetric parts (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oriented<K> {
    /// The canonical key.
    pub key: K,
    /// The reader's point is the key's `B` point.
    pub swapped: bool,
}

/// A point against a vertex of the other operand along the shadow frame,
/// either asking, keyed `A` first. The point–vertex shadow record's key
/// (the side of the `r̂` line, ahead or behind, and whether the two are
/// near). One of the two is a vertex; the other is a vertex or a cut
/// point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VvKey {
    /// The point of `A`.
    pub a: Query,
    /// The point of `B`.
    pub b: Query,
}

impl VvKey {
    /// The key of point `q` against `w`, a vertex of the other operand.
    pub fn of(q: Point, w: VertId) -> Oriented<VvKey> {
        let w = Query::Vert(w);
        let (a, b) = match q.operand {
            Operand::A => (q.q, w),
            Operand::B => (w, q.q),
        };
        Oriented {
            key: VvKey { a, b },
            swapped: q.operand == Operand::B,
        }
    }
}

/// A point's ray against an edge `g` of the other operand: the ray
/// record's key (its signed crossings of `g`'s shadow, and its feet on
/// `g`). The direction of the ray is the point's operand's, so the key
/// keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RayKey {
    /// The point whose ray it is.
    pub q: Point,
    /// The edge, of `q`'s other operand.
    pub g: EdgeId,
}

/// A point's ray against one span of a curved edge `g` of the other
/// operand: the curved ray record's key (the signed crossings of the
/// span's shadow). Spans are numbered along the edge's stored direction,
/// from its lower-id end, so the record is the same whichever coedge of
/// `g` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpanKey {
    /// The point whose ray it is.
    pub q: Point,
    /// The edge, of `q`'s other operand.
    pub g: EdgeId,
    /// The span, from 0 at the edge's lower-id end.
    pub span: u32,
}

/// A point against an inner knot of a curved edge `g` of the other
/// operand, the point where span `knot − 1` ends and span `knot` starts:
/// the knot record's key (the side of the knot against the point's `r̂`
/// line, ahead or behind along it), read by the ray records of both
/// spans. Keyed by the asking point: a knot asks no ray of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KnotKey {
    /// The asking point.
    pub q: Point,
    /// The edge, of `q`'s other operand.
    pub g: EdgeId,
    /// The knot, `1 ≤ knot <` the edge's span count.
    pub knot: u32,
}

/// An edge of `A` against an edge of `B` where their shadows cross: the
/// crossing record's key (its clusters).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XKey {
    /// The edge of `A`.
    pub e: EdgeId,
    /// The edge of `B`.
    pub g: EdgeId,
}

/// A tile edge of a face: the maximal dyadic segment of a tile line in
/// one tile, a row of the tile's stored control points, shared by the
/// tiles on its two sides. The line is where the global coordinate in
/// `fixed` is the integer `line`; the edge runs over the cell `[cell,
/// cell + 1]` of the other coordinate, in its increasing direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileEdge {
    /// The face's operand.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// The direction whose coordinate is fixed along the edge.
    pub fixed: Dir,
    /// That coordinate, a tile line.
    pub line: i64,
    /// The cell of the other coordinate the edge runs over.
    pub cell: i64,
}

impl TileEdge {
    /// The edge's two corners, at its start and end.
    pub fn corners(&self) -> [TileCorner; 2] {
        let at = |c: i64| match self.fixed {
            Dir::U => [self.line, c],
            Dir::V => [c, self.line],
        };
        [self.cell, self.cell + 1].map(|c| TileCorner {
            operand: self.operand,
            face: self.face,
            at: at(c),
        })
    }
}

/// A corner of a face's tiles: the lattice point `at` of its global UV,
/// `(u, v)`, shared by the tiles and tile edges meeting there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCorner {
    /// The face's operand.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// The lattice point.
    pub at: [i64; 2],
}

/// A point against a tile edge of a face of the other operand: the side
/// record of a box side on a tile line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SideKey {
    /// The point.
    pub q: Point,
    /// The tile edge.
    pub edge: TileEdge,
}

/// A point against a tile corner of a face of the other operand: the
/// ends of the side records there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CornerKey {
    /// The point.
    pub q: Point,
    /// The corner.
    pub corner: TileCorner,
}

/// A height key: the geometry a height above or below is decided from,
/// once for every anchor reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HKey {
    /// A point against a vertex of the other operand, keyed `A` first
    /// ([`HKey::vv`]).
    VV {
        /// The point of `A`.
        a: Query,
        /// The point of `B`.
        b: Query,
    },
    /// Edge against edge at a cluster of their crossing record: the
    /// crossing's own height, also the height of the cut point there.
    X {
        /// The edge of `A`.
        e: EdgeId,
        /// The edge of `B`.
        g: EdgeId,
        /// The cluster.
        c: XClusterIx,
    },
    /// A point against an edge of the other operand, at one of its feet
    /// on it.
    Curve {
        /// The point.
        q: Point,
        /// The edge.
        g: EdgeId,
        /// The foot.
        foot: FootId,
    },
    /// A point against a face of the other operand at one of its layers
    /// there, the layer having no feature (a core layer).
    Face {
        /// The point.
        q: Point,
        /// The face.
        f: FaceId,
        /// The layer.
        layer: LayerId,
    },
}

impl HKey {
    /// The height key of point `q` against `w`, a vertex of the other
    /// operand: one key for the pair, whichever asks.
    pub fn vv(q: Point, w: VertId) -> Oriented<HKey> {
        let Oriented { key, swapped } = VvKey::of(q, w);
        Oriented {
            key: HKey::VV { a: key.a, b: key.b },
            swapped,
        }
    }

    /// The height key of a cut point against the edge it was cut at, its
    /// own crossing: `Cut(e, g, c)` against `g` is the crossing record's
    /// cluster `c` itself, keyed `A`'s edge first, whichever edge the
    /// point is on. `None` unless `q` is a cut point at edge `h`.
    pub fn x(q: Point, h: EdgeId) -> Option<Oriented<HKey>> {
        let Query::Cut(on, at, c) = q.q else {
            return None;
        };
        if at != h {
            return None;
        }
        let (e, g) = match q.operand {
            Operand::A => (on, at),
            Operand::B => (at, on),
        };
        Some(Oriented {
            key: HKey::X { e, g, c },
            swapped: q.operand == Operand::B,
        })
    }
}

/// What a height record is anchored at: a face whose layer is above or
/// below the point, or the edge of a crossing whose z-order it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Anchor {
    /// A face of this operand.
    Face(Operand, FaceId),
    /// An edge of this operand.
    Edge(Operand, EdgeId),
}

/// A height record at one anchor: its order 0 the height key's, its
/// perturbed orders the anchor's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeightKey {
    /// The height key.
    pub h: HKey,
    /// The anchor.
    pub anchor: Anchor,
}

/// The key of any record, for evidence: what a failure names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKey {
    /// A point–point shadow record.
    Vv(VvKey),
    /// A ray record against a straight edge.
    Ray(RayKey),
    /// A ray record against a span of a curved edge.
    RaySpan(SpanKey),
    /// A point against an inner knot of a curved edge.
    Knot(KnotKey),
    /// A height key's own value (order 0, shared by its anchors).
    Height(HKey),
    /// A height record at an anchor.
    HeightAt(HeightKey),
    /// A crossing record.
    X(XKey),
    /// A point's ray against a tile edge of a face.
    Side(SideKey),
    /// A point against a tile corner of a face.
    Corner(CornerKey),
}

#[cfg(test)]
mod tests;

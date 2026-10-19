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
//! crossing record with an edge of the other operand ([`Query::Cut`]; on
//! two edges of one curve, at a root of their crossing rule,
//! [`Query::Root`]).
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
//! edge–edge or point–curve geometry the height is read from, or a
//! column hit of the point on a sheet of a face ([`HKey::Hit`]), shared
//! by every anchor that reads it. Its perturbed orders are the anchor's
//! own, so the per-anchor record is keyed by both ([`HeightKey`]). Which
//! height key a query and target read (a hit, or the vertex or edge it
//! lies on exactly) is worked out from the hit's exact incidence; the
//! keys here are what it yields.
//!
//! [`RecordKey`] is any record's key, for evidence: what
//! [`Fail::Internal`](crate::num::decide::Fail::Internal) names.
//!
//! Keys order by their ids: the order a record table stores and decides
//! them in. Ids order work only; no answer reads that order.

use crate::bezier::{Dir, Dyadic};
use crate::ids::{CoedgeId, EdgeId, FaceId, VertId};

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

/// The sheet of a face's counting surface a column hit lies on: a tile
/// of the face (the hit's UV inside the trim), or the ribbon of one span
/// of a coedge's p-curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sheet {
    /// The tile `[i, j]` of the face's global UV.
    Tile([i64; 2]),
    /// The ribbon of span `span` (along the edge's stored direction) of
    /// the coedge's p-curve.
    Ribbon(CoedgeId, u32),
}

/// A column hit's ordinal on its sheet: its place along the sheet's
/// parameter in geometric order (from the edge's geometrically lesser
/// end on a ribbon; a plane tile has one hit at most).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HitIx(pub u32);

/// Where a column hit lies exactly on an element of the hit's face,
/// decided on the stored data: at a vertex of the face, or on an edge's
/// curve at one of the point's feet on it (the germ rule: the hit then
/// reads the vertex's or edge's key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Incidence {
    /// At this vertex.
    Vert(VertId),
    /// On this edge's curve, at this foot of the point on it.
    Edge(EdgeId, FootId),
}

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
    /// A shared-curve cut point `Root(e, g, c)`: the point of edge `e` (of
    /// the query's operand) at the root cluster `c` of the crossing record
    /// of `e` and `g`, two edges on one curve, keyed by the pair and the
    /// root's index along the curve, never by its rounded position; the
    /// record keyed by `A`'s edge first whichever operand `e` is of.
    Root(EdgeId, EdgeId, XClusterIx),
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
    /// A face, at a column hit of the point on one of its sheets, with
    /// where the hit lies exactly on the face's vertices or edges.
    Hit {
        /// The face.
        f: FaceId,
        /// The sheet.
        sheet: Sheet,
        /// The hit's ordinal on it.
        i: HitIx,
        /// Its exact incidence, if any.
        inc: Option<Incidence>,
    },
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

/// A dyadic point of an edge, along its stored direction: span `span`'s
/// point at the parameter `at`, in canonical form, so a point has one key
/// whichever span reaches it: `at` in `[0, 1)`, a span's `1` being the
/// next span's `0` ([`EdgeAt::new`]); the edge's last point is span `n`
/// at 0, `n` its span count (a straight edge is one span). Its first and
/// last points are its vertices, `at = 0` of an inner span its knot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeAt {
    /// The span, from 0 at the edge's lower-id end.
    pub span: u32,
    /// The parameter on the span, in `[0, 1)`.
    pub at: Dyadic,
}

impl EdgeAt {
    /// Span `span`'s point at `at` (in `[0, 1]`), canonical; `None` past
    /// the range of `u32`.
    pub fn new(span: u32, at: Dyadic) -> Option<EdgeAt> {
        Some(if at == Dyadic::ONE {
            EdgeAt {
                span: span.checked_add(1)?,
                at: Dyadic::ZERO,
            }
        } else {
            EdgeAt { span, at }
        })
    }

    /// Which end vertex of an edge of `spans` spans the point is (0 its
    /// stored start, 1 its end), if one.
    pub fn end(&self, spans: u32) -> Option<usize> {
        match (self.span, self.at) {
            (0, at) if at == Dyadic::ZERO => Some(0),
            (s, at) if s == spans && at == Dyadic::ZERO => Some(1),
            _ => None,
        }
    }

    /// The inner knot of an edge of `spans` spans the point is, if one:
    /// `1 ≤ knot < spans`.
    pub fn knot(&self, spans: u32) -> Option<u32> {
        (self.at == Dyadic::ZERO && self.span >= 1 && self.span < spans).then_some(self.span)
    }

    /// Whether the point is a span's end control point (a vertex or a
    /// knot): a stored point.
    pub fn stored(&self) -> bool {
        self.at == Dyadic::ZERO
    }
}

/// An edge pair's inner box side (module docs of
/// [`crate::count::xrec`]): the point of one edge of the pair (`held`'s:
/// `e` of `A`, `g` of `B`) at a dyadic parameter against the whole other
/// edge, the side record of every box side at that parameter, keyed by
/// the pair and the point, `(e, g, dyadic side)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XSideKey {
    /// The pair.
    pub x: XKey,
    /// The operand of the edge the point is on.
    pub held: Operand,
    /// The point.
    pub at: EdgeAt,
}

/// A dyadic point of `e` against one of `g`, an edge pair's box corner:
/// the side of `g`'s point against the `r̂` line of `e`'s and whether it
/// is ahead along it, oriented from `e`'s point (`A`'s) to `g`'s (`B`'s).
/// Where both are vertices or one a vertex and the other a knot, the
/// fact is the point–vertex or knot record's, read from there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct XTieKey {
    /// The pair.
    pub x: XKey,
    /// The point of `e`.
    pub e: EdgeAt,
    /// The point of `g`.
    pub g: EdgeAt,
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
    /// The edge's two corners, at its start and end; `None` past the
    /// range of `i64`.
    pub fn corners(&self) -> Option<[TileCorner; 2]> {
        let end = self.cell.checked_add(1)?;
        let at = |c: i64| match self.fixed {
            Dir::U => [self.line, c],
            Dir::V => [c, self.line],
        };
        Some([self.cell, end].map(|c| TileCorner {
            operand: self.operand,
            face: self.face,
            at: at(c),
        }))
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

/// A dyadic point of a face's global UV: tile `tile`'s point at the local
/// coordinates `at`, each in `[0, 1)`, so a point has one key whichever
/// tile or box reaches it (a coordinate at a tile's `1` is the next
/// tile's `0`: [`TilePoint::new`]). A tile corner has `at = [0, 0]`, a
/// point inside a tile edge one coordinate 0; any other lies inside its
/// tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TilePoint {
    /// The face's operand.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// The tile whose lower corner is nearest: `at` lies in `[0, 1)²`.
    pub tile: [i64; 2],
    /// The local coordinates, `(u, v)`.
    pub at: [Dyadic; 2],
}

impl TilePoint {
    /// Tile `tile`'s point at the local coordinates `at` (each in `[0,
    /// 1]`), in its canonical form; `None` past the range of `i64`.
    pub fn new(
        operand: Operand,
        face: FaceId,
        tile: [i64; 2],
        at: [Dyadic; 2],
    ) -> Option<TilePoint> {
        let mut tile = tile;
        let mut at = at;
        for k in 0..2 {
            if at[k] == Dyadic::ONE {
                tile[k] = tile[k].checked_add(1)?;
                at[k] = Dyadic::ZERO;
            }
        }
        Some(TilePoint {
            operand,
            face,
            tile,
            at,
        })
    }

    /// The tile corner, if the point is one.
    pub fn corner(&self) -> Option<TileCorner> {
        (self.at == [Dyadic::ZERO; 2]).then_some(TileCorner {
            operand: self.operand,
            face: self.face,
            at: self.tile,
        })
    }

    /// The tile edge the point lies inside (not at a corner of), and its
    /// parameter there, if it does.
    pub fn on_edge(&self) -> Option<(TileEdge, Dyadic)> {
        let edge = |fixed: Dir, line: i64, cell: i64| TileEdge {
            operand: self.operand,
            face: self.face,
            fixed,
            line,
            cell,
        };
        match self.at {
            [u, v] if u == Dyadic::ZERO && v != Dyadic::ZERO => {
                Some((edge(Dir::U, self.tile[0], self.tile[1]), v))
            }
            [u, v] if v == Dyadic::ZERO && u != Dyadic::ZERO => {
                Some((edge(Dir::V, self.tile[1], self.tile[0]), u))
            }
            _ => None,
        }
    }
}

impl From<TileCorner> for TilePoint {
    fn from(c: TileCorner) -> TilePoint {
        TilePoint {
            operand: c.operand,
            face: c.face,
            tile: c.at,
            at: [Dyadic::ZERO; 2],
        }
    }
}

/// An iso line inside a tile of a face: the maximal segment of the line
/// where the tile's local coordinate in `fixed` is the dyadic `at`
/// (strictly between 0 and 1), across the whole tile, in the increasing
/// direction of the other coordinate. Boxes on its two sides are refined
/// independently, so this, not a box side, is the record's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InnerLine {
    /// The face's operand.
    pub operand: Operand,
    /// The face.
    pub face: FaceId,
    /// The tile.
    pub tile: [i64; 2],
    /// The direction whose coordinate is fixed along the line.
    pub fixed: Dir,
    /// That coordinate, local to the tile.
    pub at: Dyadic,
}

/// A line a box side lies on, as the side records key it: a tile edge, or
/// an iso line inside a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SideLine {
    /// A tile edge, shared by the tiles on its two sides.
    Edge(TileEdge),
    /// An iso line inside one tile.
    Inner(InnerLine),
}

impl SideLine {
    /// The line of face `face` of `operand` through tile `tile` where the
    /// local coordinate in `fixed` is `at`: a tile edge where `at` is 0
    /// or 1, else inside the tile. `None` past the range of `i64`.
    pub fn of(
        operand: Operand,
        face: FaceId,
        tile: [i64; 2],
        fixed: Dir,
        at: Dyadic,
    ) -> Option<SideLine> {
        let (k, o) = match fixed {
            Dir::U => (0, 1),
            Dir::V => (1, 0),
        };
        let edge = |line: i64| {
            SideLine::Edge(TileEdge {
                operand,
                face,
                fixed,
                line,
                cell: tile[o],
            })
        };
        Some(if at == Dyadic::ZERO {
            edge(tile[k])
        } else if at == Dyadic::ONE {
            edge(tile[k].checked_add(1)?)
        } else {
            SideLine::Inner(InnerLine {
                operand,
                face,
                tile,
                fixed,
                at,
            })
        })
    }

    /// The face's operand.
    pub fn operand(&self) -> Operand {
        match self {
            SideLine::Edge(e) => e.operand,
            SideLine::Inner(l) => l.operand,
        }
    }

    /// The direction whose coordinate is fixed along the line.
    pub fn fixed(&self) -> Dir {
        match self {
            SideLine::Edge(e) => e.fixed,
            SideLine::Inner(l) => l.fixed,
        }
    }

    /// The line's point at the parameter `t` in `[0, 1]` (the other
    /// coordinate, local to the tile or cell); `None` past the range of
    /// `i64`.
    pub fn point(&self, t: Dyadic) -> Option<TilePoint> {
        let (operand, face, fixed, tile, at) = match *self {
            SideLine::Edge(e) => {
                let tile = match e.fixed {
                    Dir::U => [e.line, e.cell],
                    Dir::V => [e.cell, e.line],
                };
                (e.operand, e.face, e.fixed, tile, Dyadic::ZERO)
            }
            SideLine::Inner(l) => (l.operand, l.face, l.fixed, l.tile, l.at),
        };
        let at = match fixed {
            Dir::U => [at, t],
            Dir::V => [t, at],
        };
        TilePoint::new(operand, face, tile, at)
    }

    /// The line's two ends, at its start and end.
    pub fn ends(&self) -> Option<[TilePoint; 2]> {
        Some([self.point(Dyadic::ZERO)?, self.point(Dyadic::ONE)?])
    }
}

/// A point against a line of a face of the other operand, a tile edge or
/// an iso line inside a tile: the side record of a box side on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SideKey {
    /// The point.
    pub q: Point,
    /// The line.
    pub line: SideLine,
}

/// A point against a dyadic point of a face of the other operand (a tile
/// corner, or a box corner on a tile edge or inside a tile): the ends of
/// the side records there, and the side of box corners on the point's
/// ray line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CornerKey {
    /// The point.
    pub q: Point,
    /// The face's point.
    pub corner: TilePoint,
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
    /// Two edges on one curve at a root cluster of their crossing record
    /// (a sign change of `h = ê·(T × Δ)` along their overlap): the
    /// shared-curve z-order there, exactly 0 at order 0, also the height
    /// of the cut point there ([`Query::Root`]).
    XRoot {
        /// The edge of `A`.
        e: EdgeId,
        /// The edge of `B`.
        g: EdgeId,
        /// The root cluster.
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
    /// A point against a face of the other operand at a column hit of
    /// the point on one of the face's sheets: the hit's own height,
    /// whatever boxes or pieces found it.
    Hit {
        /// The point.
        q: Point,
        /// The face.
        f: FaceId,
        /// The sheet.
        sheet: Sheet,
        /// The hit's ordinal on it.
        i: HitIx,
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
    /// cluster `c` itself, `X(e, g, c)`, and a shared-curve cut point
    /// `Root(e, g, c)` its root cluster's `XRoot(e, g, c)`, keyed `A`'s
    /// edge first, whichever edge the point is on. `None` unless `q` is a
    /// cut point at edge `h`.
    pub fn x(q: Point, h: EdgeId) -> Option<Oriented<HKey>> {
        let (on, at, c, root) = match q.q {
            Query::Cut(on, at, c) => (on, at, c, false),
            Query::Root(on, at, c) => (on, at, c, true),
            Query::Vert(_) => return None,
        };
        if at != h {
            return None;
        }
        let (e, g) = match q.operand {
            Operand::A => (on, at),
            Operand::B => (at, on),
        };
        Some(Oriented {
            key: if root {
                HKey::XRoot { e, g, c }
            } else {
                HKey::X { e, g, c }
            },
            swapped: q.operand == Operand::B,
        })
    }
}

/// What a height record is anchored at: a face whose column hit is above
/// or below the point, or the edge of a crossing whose z-order it is.
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
    /// A point's ray against a tile edge or an iso line of a face.
    Side(SideKey),
    /// A point against a dyadic point of a face.
    Corner(CornerKey),
    /// An edge pair's inner box side.
    XSide(XSideKey),
    /// An edge pair's box corner.
    XTie(XTieKey),
}

#[cfg(test)]
mod tests;

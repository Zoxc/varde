//! The canonical keys of the entities a snap makes, and of the records
//! read inside a snapped zone ([`ZoneKey`]).
//!
//! A snap rewrites the snapped operand `S` around a cluster: its faces
//! are split along the cluster's rim, the inner pieces moved onto the
//! target `T`. Every entity it makes is keyed from the operands' **ids**,
//! the rim's piece numbering (made from geometry) and the corner rule's
//! anchors, never from a name or a position, so a record inside the zone
//! has one key whichever pass reaches it: the keys of [`SnapVert`],
//! [`SnapEdge`] and [`SnapFace`] (the rewrite's rules, which also choose
//! between two keys of one point). A record between two elements of the
//! zone is keyed by the pair ([`ZoneKey`]).

use crate::ids::{EdgeId, Entity, FaceId, VertId};

use super::{Named, Operand, Oriented};

/// Which end of a `SnapAlong` piece (an `S` edge running along a target
/// edge), in geometric order along the `S` edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapEnd {
    /// The end nearer the edge's lesser end.
    Start,
    /// The other.
    End,
}

/// A vertex's canonical key after a snap: from the operands' ids, the
/// rim's piece numbering and the corner rule's anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapVert {
    /// An operand's vertex: an `S` vertex, or a target vertex.
    Vert(Operand, VertId),
    /// A rim vertex on this `S` edge, the start of this piece.
    Rim { edge: EdgeId, piece: u32 },
    /// A rim vertex on this target edge, the start of this piece.
    RimTarget { edge: EdgeId, piece: u32 },
    /// A rim vertex on box sides only, on this `S` face, the start of this
    /// piece.
    RimBox { face: FaceId, piece: u32 },
    /// A branch vertex on this piece (on this `S` face), the `index`-th
    /// along it.
    Branch {
        face: FaceId,
        piece: u32,
        index: u32,
    },
    /// This `S` edge split at this anchor by the corner rule.
    Split { edge: EdgeId, at: (Operand, VertId) },
    /// An end of the `index`-th `SnapAlong` piece of this `S` edge along
    /// this target edge.
    Bracket {
        edge: EdgeId,
        target: EdgeId,
        index: u32,
        end: SnapEnd,
    },
    /// This `S` edge crossing this target edge.
    Cross { edge: EdgeId, cut: EdgeId },
    /// This target edge crossing this rim piece.
    CutRim { cut: EdgeId, piece: u32 },
}

/// An edge's canonical key after a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapEdge {
    /// The `part`-th piece of this `S` edge from its stored start (the
    /// whole edge where it isn't cut).
    Piece { edge: EdgeId, part: u32 },
    /// The `part`-th part of this rim piece (on this `S` face) along it,
    /// a piece along box sides.
    Rim { face: FaceId, piece: u32, part: u32 },
    /// The `part`-th piece along it of this target edge in this `S` face:
    /// inside the region, or a rim piece along it.
    Cut {
        face: FaceId,
        edge: EdgeId,
        part: u32,
    },
    /// The zero-length edge joining the rim vertices starting these two
    /// pieces (two rim vertices the corner rule makes coincident).
    Zero { from: u32, to: u32 },
}

/// A face's canonical key after a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapFace {
    /// The `part`-th outer piece of this `S` face, on its surface.
    Kept { face: FaceId, part: u32 },
    /// The `part`-th inner piece of this `S` face on this target face, on
    /// the target's surface.
    Snapped {
        face: FaceId,
        target: FaceId,
        part: u32,
    },
}

impl SnapVert {
    /// The operand entities the key is made of, `snapped` the operand
    /// `S`.
    pub fn operand_ids(&self, snapped: Operand) -> Vec<(Operand, Entity)> {
        let (s, t) = (snapped, snapped.other());
        match *self {
            SnapVert::Vert(o, v) => vec![(o, v.into())],
            SnapVert::Rim { edge, .. } => vec![(s, edge.into())],
            SnapVert::RimTarget { edge, .. } => vec![(t, edge.into())],
            SnapVert::RimBox { face, .. } | SnapVert::Branch { face, .. } => {
                vec![(s, face.into())]
            }
            SnapVert::Split { edge, at } => vec![(s, edge.into()), (at.0, at.1.into())],
            SnapVert::Bracket { edge, target, .. } => vec![(s, edge.into()), (t, target.into())],
            SnapVert::Cross { edge, cut } => vec![(s, edge.into()), (t, cut.into())],
            SnapVert::CutRim { cut, .. } => vec![(t, cut.into())],
        }
    }
}

impl SnapEdge {
    /// The operand entities the key is made of, `snapped` the operand
    /// `S`.
    pub fn operand_ids(&self, snapped: Operand) -> Vec<(Operand, Entity)> {
        match *self {
            SnapEdge::Piece { edge, .. } => vec![(snapped, edge.into())],
            SnapEdge::Rim { face, .. } => vec![(snapped, face.into())],
            SnapEdge::Cut { face, edge, .. } => {
                vec![(snapped, face.into()), (snapped.other(), edge.into())]
            }
            SnapEdge::Zero { .. } => Vec::new(),
        }
    }
}

impl SnapFace {
    /// The operand entities the key is made of, `snapped` the operand
    /// `S`.
    pub fn operand_ids(&self, snapped: Operand) -> Vec<(Operand, Entity)> {
        match *self {
            SnapFace::Kept { face, .. } => vec![(snapped, face.into())],
            SnapFace::Snapped { face, target, .. } => {
                vec![(snapped, face.into()), (snapped.other(), target.into())]
            }
        }
    }
}

/// An element a record inside a snapped zone reads, by its canonical key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZoneEntity {
    /// A vertex of the rewrite.
    Vert(SnapVert),
    /// An edge of the rewrite.
    Edge(SnapEdge),
    /// A face of the rewrite.
    Face(SnapFace),
    /// An entity of an operand the rewrite doesn't hold (an edge or a
    /// vertex outside it, or a face of the target).
    Operand(Named),
}

impl ZoneEntity {
    /// The operand entities the key is made of, `snapped` the operand
    /// `S`.
    pub fn operand_ids(&self, snapped: Operand) -> Vec<(Operand, Entity)> {
        match self {
            ZoneEntity::Vert(v) => v.operand_ids(snapped),
            ZoneEntity::Edge(e) => e.operand_ids(snapped),
            ZoneEntity::Face(f) => f.operand_ids(snapped),
            ZoneEntity::Operand(n) => vec![(n.operand, n.entity)],
        }
    }
}

/// A record between two elements inside a snapped zone, keyed by their
/// canonical keys: the lesser first, whichever asked ([`ZoneKey::of`]),
/// with the snapped operand `S` the keys are relative to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneKey {
    /// The snapped operand.
    pub snapped: Operand,
    /// The lesser element.
    pub x: ZoneEntity,
    /// The other.
    pub y: ZoneEntity,
}

impl ZoneKey {
    /// The key of the record of `x` against `y` (the height of `y` above
    /// `x`, say), `swapped` where `y` is the key's first element: the
    /// reader then reads the stored answer from the other side.
    pub fn of(snapped: Operand, x: ZoneEntity, y: ZoneEntity) -> Oriented<ZoneKey> {
        let swapped = y < x;
        let (x, y) = if swapped { (y, x) } else { (x, y) };
        Oriented {
            key: ZoneKey { snapped, x, y },
            swapped,
        }
    }
}

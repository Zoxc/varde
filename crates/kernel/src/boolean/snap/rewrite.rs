//! The snap's rewrite, its topology: the snapped side `S`'s faces the
//! cluster reaches are split along the rim; the **outer** pieces keep
//! `S`'s surfaces, the **inner** ones (the cluster's region) become faces
//! on the target `T`'s surfaces, split only along target edges that
//! separate two different target faces. Every entity made gets a
//! canonical key from the operands' ids ([`SnapVert`], [`SnapEdge`],
//! [`SnapFace`]), never a fresh id, and the [`SnapMap`] records each one's
//! operand parent and pre-image. The vertices carry the corner rule's
//! outcome ([`NewVert::moved`]); where they land and the edges' curves
//! and p-curves are the [`geometry()`] pass's, on this topology.
//!
//! **A face's arrangement.** In each face's global UV, exactly (rational
//! arithmetic): the face's own trims (its whole boundary, the face on
//! their left), the rim's pieces on the face (the region on their left)
//! and the target's edges carried into the face's UV by the caller
//! ([`TargetCut`], each with the target faces on its two sides). Own trims
//! and rim pieces are cut where they meet; the target edges are cut
//! where they meet anything, and only their pieces in the region are kept
//! (a target edge outside the region splits nothing). Each elementary
//! piece's two sides are classed: outside the face, **outer** (in the
//! face, outside the region), or **inner** (in the region) with the target
//! face a target edge gives that side. Whether a side lies in the face is
//! read from the own trims (the piece's own direction where it lies on
//! one, else their winding number); in the region, from the rim's pieces,
//! else from the region's boundary on the face ([`Rim::bounds`], before
//! the rim cancels the parts of own edges both faces cover). A piece is an
//! edge of the rewrite where its sides differ: between the face and its
//! outside, between outer and inner, or between inner sides on two
//! different target faces (a target edge with one face on both sides, a
//! slit, splits nothing).
//!
//! **Cells.** The edges' half-edges, each with the side on its left, are
//! linked round every vertex in counter-clockwise order (exact: by half
//! plane, then by orientation), each half-edge followed by the one next
//! clockwise from its twin, so bounded cells run counter-clockwise. A
//! cycle of positive area is a cell's outer loop; each component's one
//! cycle of negative area is a hole of the least cell of another
//! component holding it (exact winding numbers), or of the face's outside.
//! Every half-edge of a cell's loops has the cell's side on its left, or
//! the face's sides don't close ([`RewriteError::Cells`]). Outer cells are
//! the face's kept pieces; inner cells its snapped pieces, on the target
//! face their target edges give, else the one target face of the rim
//! pieces round them, else of the face's rim pieces, else of the cluster
//! ([`RewriteError::TargetFace`] when that isn't one face).
//!
//! **Vertices** ([`SnapVert`]). A point of the arrangement is, in this
//! priority: an `S` vertex (an end of an own edge); a rim vertex, the
//! start of a rim piece (the end of a piece is the start of the next one
//! along its loop): an operand's vertex, else a point of an `S` edge
//! (`S` edge id, piece index), of a target edge (target edge id, piece
//! index) or of box sides only (`S` face id, piece index), as the corner
//! rule found it ([`Corner::rim_verts`]); a branch vertex the corner rule
//! keeps (`S` face id, piece index, its index among its piece's branch
//! vertices along the piece); a split of an `S` edge at an anchor by the
//! corner rule (edge id, anchor id; [`Corner::splits`], at the anchor's
//! foot as a fraction of the edge's parameter range); an end of a
//! `SnapAlong` piece of an `S` edge along a target edge (both ids, the
//! piece's index, which end; [`SnapAlong`], at its fraction of the edge's
//! parameter range); a target edge's
//! end (a `T` vertex); a target edge crossing an `S` edge (both ids) or a
//! rim piece (target edge id, piece index). A point on an `S` edge takes
//! the first of these over every face the edge bounds, so the edge is cut
//! at the same points with the same vertices on all its faces; the corner
//! rule's points and the `SnapAlong` pieces' ends cut every segment they
//! lie on (an own trim and a rim piece along it alike).
//! Identification is not done here: a rim vertex the corner rule joins to
//! an anchor keeps its own key, the anchor in its [`Moved`].
//!
//! **Edges** ([`SnapEdge`]). An `S` edge (or a rim piece along one) is cut
//! at all its vertices over every face; its pieces are numbered from its
//! stored start; an edge cut into more than one is listed with its pieces
//! ([`Rewrite::replaced`]), for the faces using it outside the cluster
//! too. A rim piece along box sides is cut at its vertices (branch
//! vertices, target edges crossing it), its parts numbered along it. A
//! target edge's pieces in a face, kept inside the region or a rim piece
//! along it, are numbered along it. An edge runs along its parent's
//! direction: an `S` edge's parameter, the rim piece (region on the left),
//! the target edge's parameter. The corner rule's coincident rim vertices
//! ([`Corner::zero_edges`]) are joined by a zero-length edge, used by no
//! face here.
//!
//! **Faces** ([`SnapFace`]). A face's kept pieces and its snapped pieces on
//! each target face are numbered in the order of their outer loops' least
//! points (exact UV). Loops start at their least point; a face's holes
//! follow its outer loop in the order of their least points.
//!
//! **Order.** Every choice reads exact UV positions, parameters along
//! edges and the rim's piece numbering (made from geometry); ids break
//! exact ties only (two names for one point, ranked as above).
//!
//! Target edges carried into faces the region doesn't cover are not read.
//!
//! **Work.** Charged on the operation's budget in passes: the faces'
//! arrangements (an item per face, a unit per segment pair, piece and
//! point test), the vertices on `S` edges (one item, a unit per point),
//! the cells (an item per face, a unit per half-edge, cycle step and
//! containment test).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::count::boxes::UvSpan;
use varde_decide::ids::{EdgeId, Entity, FaceId, VertId};
use varde_decide::records::keys::Operand;

use super::along::{EndKind, SnapAlong};
use super::corner::{Corner, Moved, Placement, RimVertKind};
use super::rim::{self, Along, ExactSpan, Rim, RimInput, RimTrim, SnappedFace};
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point of a face's global UV, exactly.
type P2 = [Q; 2];

/// A target edge carried into a snapped face's UV (module docs): its
/// spans straight, along the edge's parameter.
#[derive(Clone, Copy, Debug)]
pub struct TargetCut<'a> {
    /// The face of `S` whose UV it is carried into.
    pub face: FaceId,
    /// The target edge.
    pub edge: EdgeId,
    /// Its vertices at its start and its end.
    pub ends: [VertId; 2],
    /// Its spans in the face's UV, as [`RimTrim::spans`].
    pub spans: &'a [UvSpan],
    /// Its parameter at the spans' ends, as [`RimTrim::knots`].
    pub knots: &'a [f64],
    /// The target faces on its left and its right, along its parameter in
    /// this UV.
    pub sides: [FaceId; 2],
}

impl TargetCut<'_> {
    fn trim(&self) -> RimTrim<'_> {
        RimTrim {
            edge: self.edge,
            ends: self.ends,
            spans: self.spans,
            knots: self.knots,
            left: true,
        }
    }
}

/// A vertex's canonical key (module docs): from the operands' ids, the
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
        end: EndKind,
    },
    /// This `S` edge crossing this target edge.
    Cross { edge: EdgeId, cut: EdgeId },
    /// This target edge crossing this rim piece.
    CutRim { cut: EdgeId, piece: u32 },
}

/// An edge's canonical key (module docs).
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
    /// pieces ([`Corner::zero_edges`]).
    Zero { from: u32, to: u32 },
}

/// A face's canonical key (module docs).
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

    /// The naming priority (module docs), ids last.
    fn rank(&self, snapped: Operand) -> (u8, SnapVert) {
        let r = match self {
            SnapVert::Vert(o, _) if *o == snapped => 0,
            SnapVert::Vert(..) => 1,
            SnapVert::Rim { .. } => 2,
            SnapVert::RimTarget { .. } => 3,
            SnapVert::RimBox { .. } => 4,
            SnapVert::Branch { .. } => 5,
            SnapVert::Split { .. } => 6,
            SnapVert::Bracket { .. } => 7,
            SnapVert::Cross { .. } => 8,
            SnapVert::CutRim { .. } => 9,
        };
        (r, *self)
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

/// Where in its parent an entity of the rewrite comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum PreImage {
    /// The parent itself (a vertex).
    Whole,
    /// The point of the parent edge at this parameter.
    Param(Q),
    /// The stretch of the parent edge between these parameters, in the
    /// entity's direction.
    Range([Q; 2]),
    /// This point of an `S` face's global UV.
    Point(P2),
    /// This straight stretch of an `S` face's global UV, in the entity's
    /// direction.
    Segment([P2; 2]),
    /// The part of the parent face inside these loops of its global UV
    /// (the corners of the face's loops, in order).
    Region(Vec<Vec<P2>>),
}

/// An entity's operand parent and pre-image.
#[derive(Clone, Debug, PartialEq)]
pub struct Pre {
    /// The parent.
    pub parent: (Operand, Entity),
    /// Where in it.
    pub at: PreImage,
}

/// Each entity of the rewrite's parent and pre-image, by key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapMap {
    pub verts: BTreeMap<SnapVert, Pre>,
    pub edges: BTreeMap<SnapEdge, Pre>,
    pub faces: BTreeMap<SnapFace, Pre>,
}

/// A use of an edge by a face's loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Use {
    /// The edge.
    pub edge: SnapEdge,
    /// Whether the loop runs the edge's way.
    pub forward: bool,
}

/// A face of the rewrite.
#[derive(Clone, Debug, PartialEq)]
pub struct NewFace {
    /// Its key.
    pub key: SnapFace,
    /// The face whose surface it lies on: its `S` face (kept), or its
    /// target face (snapped).
    pub on: (Operand, FaceId),
    /// Its loops, the outer one first, each counter-clockwise round the
    /// face in the `S` face's UV (holes clockwise).
    pub loops: Vec<Vec<Use>>,
}

/// An edge of the rewrite.
#[derive(Clone, Debug, PartialEq)]
pub struct NewEdge {
    /// Its key.
    pub key: SnapEdge,
    /// Its vertices at its start and its end.
    pub ends: [SnapVert; 2],
    /// The rim piece it lies along, if any.
    pub rim: Option<u32>,
}

/// A vertex of the rewrite.
#[derive(Clone, Debug, PartialEq)]
pub struct NewVert {
    /// Its key.
    pub key: SnapVert,
    /// Where the corner rule sends it ([`Placement::Stays`] where it
    /// doesn't decide, or for an operand's vertex it doesn't move).
    pub moved: Moved,
    /// Its points in the global UV of the faces of `S` it lies on, by
    /// face.
    pub at: Vec<(FaceId, P2)>,
}

/// An edge of `S` the rewrite cuts, and the pieces that replace it in
/// every face using it, the faces the cluster doesn't reach included.
#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    /// The edge.
    pub edge: EdgeId,
    /// Its pieces from its stored start, each running its way.
    pub parts: Vec<SnapEdge>,
}

/// The rewrite of the faces of `S` a cluster reaches (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Rewrite {
    /// The faces, by key.
    pub faces: Vec<NewFace>,
    /// The edges the faces use, by key.
    pub edges: Vec<NewEdge>,
    /// The edges' vertices, by key.
    pub verts: Vec<NewVert>,
    /// The edges of `S` cut into more than one piece, by id.
    pub replaced: Vec<Replaced>,
    /// Their parents and pre-images.
    pub map: SnapMap,
}

/// Why the rewrite can't be made.
#[derive(Clone, Debug, PartialEq)]
pub enum RewriteError {
    /// A face of `S` the rim reaches isn't described, or has no own trims.
    Face(FaceId),
    /// A trim or a target edge span of this face isn't straight, or a
    /// value isn't finite, or its knots don't fit its spans.
    Trim(FaceId),
    /// The corner rule's rim vertices don't fit the rim's pieces.
    Corner,
    /// A boundary of this face runs both ways along a stretch (a slit).
    Slit(FaceId),
    /// The sides of this face's edges don't close into cells (a target
    /// edge ending inside the region, or input not bounding regions).
    Cells(FaceId),
    /// A point of this face's arrangement that no rule names (two target
    /// edges crossing away from their vertices).
    Unnamed(FaceId),
    /// An inner piece of this face doesn't lie on exactly one target face.
    TargetFace(FaceId),
    /// Two entities with one key: the input names one point or stretch
    /// two ways.
    Key,
    /// The map isn't along a direction, or carries a point along a
    /// target face's plane, or `ε_seam` isn't finite and `≥ 0`.
    Map,
    /// This target face isn't described, or isn't a plane.
    Chart(FaceId),
    /// This target edge, which the rewrite reuses, isn't described, or
    /// lacks a p-curve on a face using it (on a target face, or carried
    /// into a face of `S`), or its chain doesn't split.
    Reuse(EdgeId),
    /// Two reused target edges put this vertex at different points.
    Shared(SnapVert),
    /// This vertex's position can't be found: its pre-image isn't
    /// described, or the target faces round it carry it to different
    /// points, or a zero-length edge's ends differ.
    Position(SnapVert),
    /// This edge's curve or p-curves can't be made: an end without a
    /// parameter on the target edge it reuses, a rim edge not along an iso
    /// line, or a value the curves refuse.
    Curve(SnapEdge),
    /// This fitted edge lies beyond `ε_seam/2` of the target composed
    /// with its p-curve (a validity failure).
    Fit(SnapEdge),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The rewrite of the faces of `S` the rim `rim` of a cluster made from
/// `rim_input` reaches, with the corner rule's decisions `corner` and the
/// target edges `cuts` carried into those faces (module docs), charged to
/// `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn rewrite(
    rim: &Rim,
    rim_input: &RimInput<'_>,
    corner: &Corner,
    along: &SnapAlong,
    cuts: &[TargetCut<'_>],
    work: &mut Work,
) -> Result<Rewrite, RewriteError> {
    let s_op = rim_input.snapped;
    if corner.rim_verts.len() != rim.pieces.len()
        || corner
            .rim_verts
            .iter()
            .enumerate()
            .any(|(k, r)| r.piece as usize != k)
    {
        return Err(RewriteError::Corner);
    }
    let mut next = vec![u32::MAX; rim.pieces.len()];
    for l in &rim.loops {
        for (k, &p) in l.iter().enumerate() {
            let slot = next.get_mut(p as usize).ok_or(RewriteError::Corner)?;
            *slot = l[(k + 1) % l.len()];
        }
    }
    if next.contains(&u32::MAX)
        || corner
            .branches
            .iter()
            .any(|b| b.piece as usize >= rim.pieces.len())
    {
        return Err(RewriteError::Corner);
    }

    // The faces the region covers, in id order, with their exact trims.
    let ids: BTreeSet<FaceId> = rim
        .bounds
        .iter()
        .map(|b| b.face)
        .chain(rim.pieces.iter().map(|p| p.face))
        .collect();
    let mut faces: Vec<FaceData<'_>> = Vec::with_capacity(ids.len());
    for f in ids {
        let face = *rim_input
            .faces
            .iter()
            .find(|g| g.face == f)
            .ok_or(RewriteError::Face(f))?;
        if face.own.is_empty() {
            return Err(RewriteError::Face(f));
        }
        let own = rim::exact_spans(f, face.own).map_err(|_| RewriteError::Trim(f))?;
        let target = rim::exact_spans(f, face.target).map_err(|_| RewriteError::Trim(f))?;
        let face_cuts: Vec<&TargetCut<'_>> = cuts.iter().filter(|c| c.face == f).collect();
        let trims: Vec<RimTrim<'_>> = face_cuts.iter().map(|c| c.trim()).collect();
        let cut_spans = rim::exact_spans(f, &trims).map_err(|_| RewriteError::Trim(f))?;
        let bounds = rim
            .bounds
            .iter()
            .filter(|b| b.face == f)
            .map(|b| b.ends.clone())
            .collect();
        faces.push(FaceData {
            face,
            own,
            target,
            cuts: face_cuts,
            cut_spans,
            bounds,
        });
    }

    // The names of the rim's vertices and branch vertices.
    let mut rim_names = Vec::with_capacity(rim.pieces.len());
    for (k, rv) in corner.rim_verts.iter().enumerate() {
        rim_names.push(rim_name(k as u32, rv.kind, rim, &faces, s_op)?);
    }
    let mut branch_names = Vec::with_capacity(corner.branches.len());
    let mut on_piece: BTreeMap<u32, u32> = BTreeMap::new();
    for b in &corner.branches {
        let index = on_piece.entry(b.piece).or_insert(0);
        let face = rim.pieces[b.piece as usize].face;
        branch_names.push((
            SnapVert::Branch {
                face,
                piece: b.piece,
                index: *index,
            },
            Pre {
                parent: (s_op, face.into()),
                at: PreImage::Point(b.uv.clone()),
            },
        ));
        *index += 1;
    }
    // The ends of the `SnapAlong` pieces, where they split their edges.
    let brackets: Vec<BracketAt> = along
        .pairs
        .iter()
        .flat_map(|pair| {
            pair.pieces.iter().flat_map(move |piece| {
                piece.ends.iter().map(move |end| BracketAt {
                    key: SnapVert::Bracket {
                        edge: pair.h,
                        target: pair.e,
                        index: piece.index,
                        end: end.which,
                    },
                    edge: pair.h,
                    s: end.s.clone(),
                    moved: end.moved,
                })
            })
        })
        .collect();
    let ctx = Ctx {
        rim,
        corner,
        s_op,
        next: &next,
        rim_names: &rim_names,
        branch_names: &branch_names,
        brackets: &brackets,
        targets: rim_input.targets.iter().map(|t| t.0).collect(),
    };

    let arranged = run(work, "snap rewrite: faces", &faces, |f, ledger| {
        arrange(f, &ctx, ledger)
    })?;
    let on_edges = run(work, "snap rewrite: edge vertices", &[()], |_, ledger| {
        edge_vertices(&arranged, &faces, &ctx, ledger)
    })?
    .swap_remove(0);
    let jobs: Vec<usize> = (0..faces.len()).collect();
    let celled = run(work, "snap rewrite: cells", &jobs, |&i, ledger| {
        cells(&faces[i], &arranged[i], &on_edges, &ctx, ledger)
    })?;

    // Merge, each key once.
    let mut map = SnapMap::default();
    let mut edges: BTreeMap<SnapEdge, NewEdge> = BTreeMap::new();
    let mut at: BTreeMap<SnapVert, Vec<(FaceId, P2)>> = BTreeMap::new();
    let mut out_faces = Vec::new();
    for (i, c) in celled.into_iter().enumerate() {
        let face = faces[i].face.face;
        for (p, (k, pre)) in c.verts {
            at.entry(k).or_default().push((face, p));
            insert(&mut map.verts, k, pre)?;
        }
        for (e, pre) in c.edges {
            insert(&mut map.edges, e.key, pre)?;
            match edges.get_mut(&e.key) {
                Some(old) if old.ends != e.ends => return Err(RewriteError::Key),
                Some(old) => old.rim = old.rim.or(e.rim),
                None => {
                    edges.insert(e.key, e);
                }
            }
        }
        for (f, pre) in c.faces {
            insert(&mut map.faces, f.key, pre)?;
            out_faces.push(f);
        }
    }
    out_faces.sort_by_key(|f| f.key);

    // The zero-length edges between coincident rim vertices, at the
    // vertices the rim vertices became.
    for &[i, j] in &corner.zero_edges {
        let (Some(a), Some(b)) = (
            rim_vertex(i, &rim_names, &on_edges),
            rim_vertex(j, &rim_names, &on_edges),
        ) else {
            return Err(RewriteError::Corner);
        };
        if a == b || !map.verts.contains_key(&a) || !map.verts.contains_key(&b) {
            continue;
        }
        let key = SnapEdge::Zero { from: i, to: j };
        let piece = &rim.pieces[i as usize];
        let pre = Pre {
            parent: (s_op, piece.face.into()),
            at: PreImage::Point(piece.ends[0].clone()),
        };
        insert(&mut map.edges, key, pre)?;
        edges.insert(
            key,
            NewEdge {
                key,
                ends: [a, b],
                rim: Some(i),
            },
        );
    }

    // The edges of `S` cut into pieces, for the faces using them.
    let mut parts: BTreeMap<EdgeId, Vec<(u32, SnapEdge)>> = BTreeMap::new();
    for k in edges.keys() {
        if let SnapEdge::Piece { edge, part } = *k {
            parts.entry(edge).or_default().push((part, *k));
        }
    }
    let replaced = parts
        .into_iter()
        .filter(|(_, ps)| ps.len() > 1)
        .map(|(edge, mut ps)| {
            ps.sort();
            Replaced {
                edge,
                parts: ps.into_iter().map(|p| p.1).collect(),
            }
        })
        .collect();

    let verts = map
        .verts
        .keys()
        .map(|&key| NewVert {
            key,
            moved: moved_of(key, &ctx),
            at: at.remove(&key).unwrap_or_default(),
        })
        .collect();
    Ok(Rewrite {
        faces: out_faces,
        edges: edges.into_values().collect(),
        verts,
        replaced,
        map,
    })
}

/// The vertex the rim vertex starting piece `k` became: on an edge of `S`
/// the vertex there, else its own name.
fn rim_vertex(k: u32, rim_names: &[(SnapVert, Pre)], on_edges: &OnEdges) -> Option<SnapVert> {
    let (name, pre) = rim_names.get(k as usize)?;
    if let (SnapVert::Rim { edge, .. }, PreImage::Param(t)) = (name, &pre.at) {
        return on_edges.names.get(edge).and_then(|m| m.get(t)).map(|n| n.0);
    }
    Some(*name)
}

/// Inserts `pre` at `key`, refusing a different one already there.
fn insert<K: Ord>(map: &mut BTreeMap<K, Pre>, key: K, pre: Pre) -> Result<(), RewriteError> {
    match map.get(&key) {
        Some(old) if *old != pre => Err(RewriteError::Key),
        Some(_) => Ok(()),
        None => {
            map.insert(key, pre);
            Ok(())
        }
    }
}

/// Where the corner rule (or a `SnapAlong` bracket end) sends the vertex
/// `key`.
fn moved_of(key: SnapVert, ctx: &Ctx<'_>) -> Moved {
    let (corner, s_op) = (ctx.corner, ctx.s_op);
    match key {
        SnapVert::Vert(o, v) if o == s_op => corner
            .rim_verts
            .iter()
            .find(|r| r.kind == RimVertKind::Vert(o, v))
            .map(|r| r.moved)
            .or_else(|| corner.placed.iter().find(|p| p.vert == v).map(|p| p.moved))
            .unwrap_or(Moved::STAYS),
        SnapVert::Rim { piece, .. }
        | SnapVert::RimTarget { piece, .. }
        | SnapVert::RimBox { piece, .. } => corner.rim_verts[piece as usize].moved,
        SnapVert::Split { edge, at } => corner
            .splits
            .iter()
            .find(|s| s.edge == edge && s.at == at)
            .map(|s| Moved {
                to: Placement::Vert(at.0, at.1),
                pos: Some(s.pos),
                displacement: s.displacement,
                storage: 0.0,
            })
            .unwrap_or(Moved::STAYS),
        SnapVert::Bracket { .. } => ctx
            .brackets
            .iter()
            .find(|b| b.key == key)
            .map_or(Moved::STAYS, |b| b.moved),
        _ => Moved::STAYS,
    }
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, RewriteError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<RewriteError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => RewriteError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<RewriteError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

fn fail<T>(e: RewriteError) -> Result<T, Stop<RewriteError>> {
    Err(Stop::Failed(e))
}

// ---------------------------------------------------------------------
// Exact plane geometry.

fn sub2(a: &P2, b: &P2) -> P2 {
    [&a[0] - &b[0], &a[1] - &b[1]]
}

fn cross2(a: &P2, b: &P2) -> Q {
    &a[0] * &b[1] - &a[1] * &b[0]
}

fn dot2(a: &P2, b: &P2) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1]
}

/// `a + s·(b − a)`.
fn lerp(a: &P2, b: &P2, s: &Q) -> P2 {
    [&a[0] + s * (&b[0] - &a[0]), &a[1] + s * (&b[1] - &a[1])]
}

fn mid(a: &P2, b: &P2) -> P2 {
    let half = Q::from_signeds(1, 2);
    [(&a[0] + &b[0]) * &half, (&a[1] + &b[1]) * &half]
}

/// The fraction of the way from `a` to `b` of `p`'s foot on their line.
fn frac(p: &P2, a: &P2, b: &P2) -> Q {
    let d = sub2(b, a);
    dot2(&sub2(p, a), &d) / dot2(&d, &d)
}

/// Whether `p` lies on the closed segment `s`.
fn on_seg(p: &P2, s: &[P2; 2]) -> bool {
    let d = sub2(&s[1], &s[0]);
    let w = sub2(p, &s[0]);
    if cross2(&d, &w) != 0u32 {
        return false;
    }
    let x = dot2(&w, &d);
    x >= 0u32 && x <= dot2(&d, &d)
}

/// The winding number of the closed oriented `segs` about `p` (off them).
fn winding(segs: &[[P2; 2]], p: &P2) -> i64 {
    let mut w = 0i64;
    for [a, b] in segs {
        let c = cross2(&sub2(b, a), &sub2(p, a));
        if a[1] <= p[1] && p[1] < b[1] && c > 0u32 {
            w += 1;
        } else if b[1] <= p[1] && p[1] < a[1] && c < 0u32 {
            w -= 1;
        }
    }
    w
}

/// Whether the edge at `m` along `d` lies on one of the oriented `segs`,
/// and if so whether it runs its way.
fn along_any(segs: &[[P2; 2]], m: &P2, d: &P2) -> Option<bool> {
    segs.iter().find_map(|s| {
        let e = sub2(&s[1], &s[0]);
        (cross2(&e, d) == 0u32 && on_seg(m, s)).then(|| dot2(&e, d) > 0u32)
    })
}

/// Twice the signed area of the closed polygon `pts`.
fn area2(pts: &[&P2]) -> Q {
    let n = pts.len();
    (0..n).fold(Q::from(0u32), |s, i| s + cross2(pts[i], pts[(i + 1) % n]))
}

/// The counter-clockwise order of directions from `+u`.
fn ccw(a: &P2, b: &P2) -> Ordering {
    let half = |d: &P2| u8::from(!(d[1] > 0u32 || (d[1] == 0u32 && d[0] > 0u32)));
    half(a).cmp(&half(b)).then_with(|| {
        let c = cross2(a, b);
        if c > 0u32 {
            Ordering::Less
        } else if c < 0u32 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    })
}

// ---------------------------------------------------------------------
// Shared context.

/// A face of `S` the region covers, its trims exact.
struct FaceData<'a> {
    face: SnappedFace<'a>,
    own: Vec<Vec<ExactSpan>>,
    target: Vec<Vec<ExactSpan>>,
    cuts: Vec<&'a TargetCut<'a>>,
    cut_spans: Vec<Vec<ExactSpan>>,
    /// The region's boundary on the face, the region on the left.
    bounds: Vec<[P2; 2]>,
}

impl FaceData<'_> {
    /// The own trims' spans as segments, the face on their left.
    fn own_segs(&self) -> Vec<[P2; 2]> {
        let mut out = Vec::new();
        for (t, ss) in self.face.own.iter().zip(&self.own) {
            for s in ss {
                if s.p[0] != s.p[1] {
                    out.push(if t.left {
                        [s.p[0].clone(), s.p[1].clone()]
                    } else {
                        [s.p[1].clone(), s.p[0].clone()]
                    });
                }
            }
        }
        out
    }

    /// The edge parameter at `p` of own edge `edge`, if `p` is on it.
    fn own_param(&self, edge: EdgeId, p: &P2) -> Option<Q> {
        param_in(self.face.own, &self.own, edge, p)
    }
}

/// The parameter at `p` of `edge` among `trims`, if `p` is on it.
fn param_in(trims: &[RimTrim<'_>], spans: &[Vec<ExactSpan>], edge: EdgeId, p: &P2) -> Option<Q> {
    trims.iter().zip(spans).find_map(|(t, ss)| {
        (t.edge == edge)
            .then(|| {
                ss.iter()
                    .find(|s| s.p[0] != s.p[1] && on_seg(p, &s.p))
                    .map(|s| rim::param_on(s, p))
            })
            .flatten()
    })
}

/// An end of a `SnapAlong` piece, splitting its edge of `S`.
struct BracketAt {
    key: SnapVert,
    /// The edge of `S` and the fraction along it from its stored start.
    edge: EdgeId,
    s: Q,
    moved: Moved,
}

struct Ctx<'a> {
    rim: &'a Rim,
    corner: &'a Corner,
    s_op: Operand,
    /// The piece after each along its loop.
    next: &'a [u32],
    /// The rim vertex starting each piece, named.
    rim_names: &'a [(SnapVert, Pre)],
    /// The corner rule's branch vertices, named.
    branch_names: &'a [(SnapVert, Pre)],
    /// The `SnapAlong` pieces' ends.
    brackets: &'a [BracketAt],
    /// The cluster's target faces.
    targets: BTreeSet<FaceId>,
}

/// The name of the rim vertex starting piece `k`, of kind `kind`.
fn rim_name(
    k: u32,
    kind: RimVertKind,
    rim: &Rim,
    faces: &[FaceData<'_>],
    s_op: Operand,
) -> Result<(SnapVert, Pre), RewriteError> {
    let piece = &rim.pieces[k as usize];
    let p = &piece.ends[0];
    let face = faces
        .iter()
        .find(|f| f.face.face == piece.face)
        .ok_or(RewriteError::Face(piece.face))?;
    Ok(match kind {
        RimVertKind::Vert(o, v) => (
            SnapVert::Vert(o, v),
            Pre {
                parent: (o, v.into()),
                at: PreImage::Whole,
            },
        ),
        RimVertKind::OnEdge(o, g) if o == s_op => {
            let t = face.own_param(g, p).ok_or(RewriteError::Corner)?;
            (
                SnapVert::Rim { edge: g, piece: k },
                Pre {
                    parent: (o, g.into()),
                    at: PreImage::Param(t),
                },
            )
        }
        RimVertKind::OnEdge(o, g) => {
            let t = param_in(face.face.target, &face.target, g, p).ok_or(RewriteError::Corner)?;
            (
                SnapVert::RimTarget { edge: g, piece: k },
                Pre {
                    parent: (o, g.into()),
                    at: PreImage::Param(t),
                },
            )
        }
        RimVertKind::Box => (
            SnapVert::RimBox {
                face: piece.face,
                piece: k,
            },
            Pre {
                parent: (s_op, piece.face.into()),
                at: PreImage::Point(p.clone()),
            },
        ),
    })
}

// ---------------------------------------------------------------------
// A face's arrangement.

/// Where a segment of a face's arrangement comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Src {
    /// An own trim and its span.
    Own(u32, u32),
    /// A rim piece.
    Rim(u32),
    /// A target edge (its index among the face's) and its span.
    Cut(u32, u32),
}

struct Seg {
    a: P2,
    b: P2,
    src: Src,
}

/// A side of an edge of a face's arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// Outside the face.
    Out,
    /// In the face, outside the region.
    Outer,
    /// In the region, on this target face where a target edge says.
    Inner(Option<FaceId>),
}

impl Side {
    fn class(self) -> u8 {
        match self {
            Side::Out => 0,
            Side::Outer => 1,
            Side::Inner(_) => 2,
        }
    }

    /// Whether an edge between `self` and `other` separates two cells.
    fn differs(self, other: Side) -> bool {
        match (self, other) {
            (Side::Inner(a), Side::Inner(b)) => a.is_some() && b.is_some() && a != b,
            _ => self.class() != other.class(),
        }
    }
}

/// An edge of a face's arrangement, from `a` to `b`.
#[derive(Clone, Debug)]
struct El {
    a: P2,
    b: P2,
    left: Side,
    right: Side,
    /// The own edge it lies on and that edge's parameters at `a`, `b`.
    own: Option<(EdgeId, [Q; 2])>,
    /// The rim piece it lies on, whether it runs the piece's way, and the
    /// fractions of the piece at `a`, `b`.
    rim: Option<(u32, bool, [Q; 2])>,
    /// The target edge it lies on and that edge's parameters at `a`, `b`.
    cut: Option<(EdgeId, [Q; 2])>,
}

/// A point of a face's arrangement.
#[derive(Clone, Debug, Default)]
struct PointInfo {
    /// The own edges through it, with their parameters there.
    on: Vec<(EdgeId, Q)>,
    /// Its names by the rules of this face (module docs).
    names: Vec<(SnapVert, Pre)>,
}

/// A face's arrangement: its edges and their ends.
struct Arranged {
    els: Vec<El>,
    points: BTreeMap<P2, PointInfo>,
}

/// Face `f`'s arrangement (module docs).
fn arrange(
    f: &FaceData<'_>,
    ctx: &Ctx<'_>,
    ledger: &mut Ledger<'_>,
) -> Result<Arranged, Stop<RewriteError>> {
    let face = f.face.face;
    let own_segs = f.own_segs();
    // Own trims and rim pieces.
    let mut base: Vec<Seg> = Vec::new();
    for (ti, (t, ss)) in f.face.own.iter().zip(&f.own).enumerate() {
        for (si, s) in ss.iter().enumerate() {
            if s.p[0] == s.p[1] {
                continue;
            }
            let (a, b) = if t.left {
                (s.p[0].clone(), s.p[1].clone())
            } else {
                (s.p[1].clone(), s.p[0].clone())
            };
            base.push(Seg {
                a,
                b,
                src: Src::Own(ti as u32, si as u32),
            });
        }
    }
    for (k, p) in ctx.rim.pieces.iter().enumerate() {
        if p.face == face && p.ends[0] != p.ends[1] {
            base.push(Seg {
                a: p.ends[0].clone(),
                b: p.ends[1].clone(),
                src: Src::Rim(k as u32),
            });
        }
    }
    let rim_segs: Vec<[P2; 2]> = base
        .iter()
        .filter(|s| matches!(s.src, Src::Rim(_)))
        .map(|s| [s.a.clone(), s.b.clone()])
        .collect();
    let n = base.len();
    units(ledger, n as u64)?;
    let mut params: Vec<Vec<Q>> = (0..n).map(|_| vec![Q::from(0u32), Q::from(1u32)]).collect();
    for i in 0..n {
        units(ledger, (n - i) as u64)?;
        for j in i + 1..n {
            let (pi, pj) = rim::meet_params([&base[i].a, &base[i].b], [&base[j].a, &base[j].b]);
            params[i].extend(pi);
            params[j].extend(pj);
        }
    }
    // The corner rule's points (branch vertices on rim pieces, splits on
    // own edges) and the `SnapAlong` pieces' ends, cutting every segment
    // they lie on (an own edge and a rim piece along it alike).
    let mut extra: Vec<P2> = Vec::new();
    for s in &base {
        match s.src {
            Src::Rim(k) => {
                for b in ctx.corner.branches.iter().filter(|b| b.piece == k) {
                    if b.at > 0u32 && b.at < 1u32 {
                        extra.push(lerp(&s.a, &s.b, &b.at));
                    }
                }
            }
            Src::Own(ti, si) => {
                let trim = &f.face.own[ti as usize];
                let span = &f.own[ti as usize][si as usize];
                let mut ts: Vec<Q> = Vec::new();
                for sp in ctx.corner.splits.iter().filter(|x| x.edge == trim.edge) {
                    let Some(t) = split_param(trim, sp.s) else {
                        return fail(RewriteError::Trim(face));
                    };
                    ts.push(t);
                }
                for b in ctx.brackets.iter().filter(|b| b.edge == trim.edge) {
                    let Some(t) = frac_param(trim, &b.s) else {
                        return fail(RewriteError::Trim(face));
                    };
                    ts.push(t);
                }
                let (lo, hi) = if span.t[0] <= span.t[1] {
                    (&span.t[0], &span.t[1])
                } else {
                    (&span.t[1], &span.t[0])
                };
                for t in ts {
                    if t > *lo && t < *hi {
                        let x = (&t - &span.t[0]) / (&span.t[1] - &span.t[0]);
                        extra.push(lerp(&span.p[0], &span.p[1], &x));
                    }
                }
            }
            Src::Cut(..) => {}
        }
    }
    extra.sort();
    extra.dedup();
    for (i, s) in base.iter().enumerate() {
        units(ledger, extra.len() as u64)?;
        let seg = [s.a.clone(), s.b.clone()];
        for p in &extra {
            if *p != s.a && *p != s.b && on_seg(p, &seg) {
                params[i].push(frac(p, &s.a, &s.b));
            }
        }
    }

    // Target edges: cut where they meet anything, their pieces in the
    // region kept, their ends splitting what they lie on.
    let mut cut_segs: Vec<Seg> = Vec::new();
    for (ci, ss) in f.cut_spans.iter().enumerate() {
        for (si, s) in ss.iter().enumerate() {
            if s.p[0] != s.p[1] {
                cut_segs.push(Seg {
                    a: s.p[0].clone(),
                    b: s.p[1].clone(),
                    src: Src::Cut(ci as u32, si as u32),
                });
            }
        }
    }
    let mut kept_cuts: Vec<Seg> = Vec::new();
    for (i, c) in cut_segs.iter().enumerate() {
        units(ledger, (n + cut_segs.len()) as u64)?;
        let mut ps = vec![Q::from(0u32), Q::from(1u32)];
        for s in base.iter().chain(
            cut_segs
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|x| x.1),
        ) {
            ps.extend(rim::meet_params([&c.a, &c.b], [&s.a, &s.b]).0);
        }
        ps.sort();
        ps.dedup();
        for w in ps.windows(2) {
            let (p, q) = (lerp(&c.a, &c.b, &w[0]), lerp(&c.a, &c.b, &w[1]));
            let m = mid(&p, &q);
            let d = sub2(&q, &p);
            let in_region =
                match along_any(&rim_segs, &m, &d).or_else(|| along_any(&f.bounds, &m, &d)) {
                    Some(_) => true,
                    None => winding(&f.bounds, &m) != 0,
                };
            if in_region {
                kept_cuts.push(Seg {
                    a: p,
                    b: q,
                    src: c.src,
                });
            }
        }
    }
    for c in &kept_cuts {
        units(ledger, n as u64)?;
        for (i, s) in base.iter().enumerate() {
            for p in [&c.a, &c.b] {
                if on_seg(p, &[s.a.clone(), s.b.clone()]) {
                    params[i].push(frac(p, &s.a, &s.b));
                }
            }
        }
    }

    // Elementary pieces grouped by their ends (the lesser first).
    let mut groups: BTreeMap<(P2, P2), Vec<(bool, Src)>> = BTreeMap::new();
    fn add(groups: &mut BTreeMap<(P2, P2), Vec<(bool, Src)>>, p: P2, q: P2, src: Src) {
        let fwd = p < q;
        let key = if fwd { (p, q) } else { (q, p) };
        groups.entry(key).or_default().push((fwd, src));
    }
    for (s, ps) in base.iter().zip(params.iter_mut()) {
        ps.sort();
        ps.dedup();
        units(ledger, ps.len() as u64)?;
        for w in ps.windows(2) {
            add(
                &mut groups,
                lerp(&s.a, &s.b, &w[0]),
                lerp(&s.a, &s.b, &w[1]),
                s.src,
            );
        }
    }
    // A target edge's piece along an own trim or a rim piece is cut where
    // that is.
    let base_points: BTreeSet<P2> = groups
        .keys()
        .flat_map(|(p, q)| [p.clone(), q.clone()])
        .collect();
    for c in &kept_cuts {
        units(ledger, base_points.len() as u64)?;
        let seg = [c.a.clone(), c.b.clone()];
        let mut ss: Vec<Q> = base_points
            .iter()
            .filter(|p| **p != c.a && **p != c.b && on_seg(p, &seg))
            .map(|p| frac(p, &c.a, &c.b))
            .collect();
        ss.push(Q::from(0u32));
        ss.push(Q::from(1u32));
        ss.sort();
        for w in ss.windows(2) {
            add(
                &mut groups,
                lerp(&c.a, &c.b, &w[0]),
                lerp(&c.a, &c.b, &w[1]),
                c.src,
            );
        }
    }

    let mut els = Vec::new();
    for ((p, q), entries) in &groups {
        units(ledger, 1)?;
        let m = mid(p, q);
        let d = sub2(q, p);
        let dirs = |want: fn(&Src) -> bool| -> BTreeSet<bool> {
            entries.iter().filter(|e| want(&e.1)).map(|e| e.0).collect()
        };
        let own_dirs = dirs(|s| matches!(s, Src::Own(..)));
        let rim_dirs = dirs(|s| matches!(s, Src::Rim(_)));
        let in_face = match own_dirs.len() {
            0 => {
                let w = winding(&own_segs, &m) != 0;
                (w, w)
            }
            1 => {
                let fwd = own_dirs.contains(&true);
                (fwd, !fwd)
            }
            _ => return fail(RewriteError::Slit(face)),
        };
        let in_region = match rim_dirs.len() {
            0 => match along_any(&f.bounds, &m, &d) {
                Some(fwd) => (fwd, !fwd),
                None => {
                    let w = winding(&f.bounds, &m) != 0;
                    (w, w)
                }
            },
            1 => {
                let fwd = rim_dirs.contains(&true);
                (fwd, !fwd)
            }
            _ => return fail(RewriteError::Slit(face)),
        };
        let mut own = None;
        let mut rim_on = None;
        let mut cut = None;
        let mut cut_faces: [Option<FaceId>; 2] = [None, None];
        for (fwd, src) in entries {
            match *src {
                Src::Own(ti, si) if own.is_none() => {
                    let span = &f.own[ti as usize][si as usize];
                    own = Some((
                        f.face.own[ti as usize].edge,
                        [rim::param_on(span, p), rim::param_on(span, q)],
                    ));
                }
                Src::Rim(k) if rim_on.is_none() => {
                    let e = &ctx.rim.pieces[k as usize].ends;
                    rim_on = Some((k, *fwd, [frac(p, &e[0], &e[1]), frac(q, &e[0], &e[1])]));
                }
                Src::Cut(ci, si) if cut.is_none() => {
                    let c = f.cuts[ci as usize];
                    let span = &f.cut_spans[ci as usize][si as usize];
                    cut = Some((c.edge, [rim::param_on(span, p), rim::param_on(span, q)]));
                    cut_faces = if *fwd {
                        [Some(c.sides[0]), Some(c.sides[1])]
                    } else {
                        [Some(c.sides[1]), Some(c.sides[0])]
                    };
                }
                _ => {}
            }
        }
        // A rim piece along a target edge is a piece of that edge.
        if cut.is_none()
            && let Some((k, ..)) = rim_on
            && let Along::Target(g) = ctx.rim.pieces[k as usize].along
        {
            let (Some(tp), Some(tq)) = (
                param_in(f.face.target, &f.target, g, p),
                param_in(f.face.target, &f.target, g, q),
            ) else {
                return fail(RewriteError::Trim(face));
            };
            cut = Some((g, [tp, tq]));
        }
        let side =
            |in_f: bool, in_r: bool, t: Option<FaceId>| -> Result<Side, Stop<RewriteError>> {
                match (in_f, in_r) {
                    (_, true) if !in_f => fail(RewriteError::Cells(face)),
                    (_, true) => Ok(Side::Inner(t)),
                    (true, false) => Ok(Side::Outer),
                    (false, false) => Ok(Side::Out),
                }
            };
        let left = side(in_face.0, in_region.0, cut_faces[0])?;
        let right = side(in_face.1, in_region.1, cut_faces[1])?;
        if !left.differs(right) {
            continue;
        }
        els.push(El {
            a: p.clone(),
            b: q.clone(),
            left,
            right,
            own,
            rim: rim_on,
            cut,
        });
    }

    // The ends: the own edges through them, their names here.
    let mut points: BTreeMap<P2, PointInfo> = BTreeMap::new();
    for e in &els {
        for p in [&e.a, &e.b] {
            points.entry(p.clone()).or_default();
        }
    }
    let rim_here: Vec<u32> = (0..ctx.rim.pieces.len() as u32)
        .filter(|&k| ctx.rim.pieces[k as usize].face == face)
        .collect();
    for (p, info) in points.iter_mut() {
        units(
            ledger,
            (f.own.len() + rim_here.len() + kept_cuts.len()) as u64,
        )?;
        for (t, ss) in f.face.own.iter().zip(&f.own) {
            if let Some(s) = ss.iter().find(|s| s.p[0] != s.p[1] && on_seg(p, &s.p)) {
                info.on.push((t.edge, rim::param_on(s, p)));
            }
            let first = ss.first().map(|s| &s.p[0]);
            let last = ss.last().map(|s| &s.p[1]);
            for (end, v) in [(first, t.ends[0]), (last, t.ends[1])] {
                if end == Some(p) {
                    info.names.push(whole(ctx.s_op, v));
                }
            }
        }
        for &k in &rim_here {
            let e = &ctx.rim.pieces[k as usize].ends;
            if e[0] == *p {
                info.names.push(ctx.rim_names[k as usize].clone());
            }
            if e[1] == *p {
                info.names
                    .push(ctx.rim_names[ctx.next[k as usize] as usize].clone());
            }
        }
        for (b, name) in ctx.corner.branches.iter().zip(ctx.branch_names) {
            if b.uv == *p && ctx.rim.pieces[b.piece as usize].face == face {
                info.names.push(name.clone());
            }
        }
        // The target edges through the point (by their index here).
        let mut cuts_through: BTreeSet<u32> = BTreeSet::new();
        for c in &kept_cuts {
            if let Src::Cut(ci, _) = c.src
                && on_seg(p, &[c.a.clone(), c.b.clone()])
            {
                cuts_through.insert(ci);
            }
        }
        for &ci in &cuts_through {
            let (c, ss) = (f.cuts[ci as usize], &f.cut_spans[ci as usize]);
            let first = ss.first().map(|s| &s.p[0]);
            let last = ss.last().map(|s| &s.p[1]);
            for (end, v) in [(first, c.ends[0]), (last, c.ends[1])] {
                if end == Some(p) {
                    info.names.push(whole(ctx.s_op.other(), v));
                }
            }
            let e = c.edge;
            for (h, t) in &info.on {
                info.names.push((
                    SnapVert::Cross { edge: *h, cut: e },
                    Pre {
                        parent: (ctx.s_op, (*h).into()),
                        at: PreImage::Param(t.clone()),
                    },
                ));
            }
            for &k in &rim_here {
                let e2 = &ctx.rim.pieces[k as usize].ends;
                if on_seg(p, e2) && *p != e2[0] && *p != e2[1] {
                    let Some(s) = ss.iter().find(|s| s.p[0] != s.p[1] && on_seg(p, &s.p)) else {
                        return fail(RewriteError::Unnamed(face));
                    };
                    info.names.push((
                        SnapVert::CutRim { cut: e, piece: k },
                        Pre {
                            parent: (ctx.s_op.other(), e.into()),
                            at: PreImage::Param(rim::param_on(s, p)),
                        },
                    ));
                }
            }
        }
    }
    Ok(Arranged { els, points })
}

/// An operand's vertex as a name.
fn whole(o: Operand, v: VertId) -> (SnapVert, Pre) {
    (
        SnapVert::Vert(o, v),
        Pre {
            parent: (o, v.into()),
            at: PreImage::Whole,
        },
    )
}

/// The parameter of own trim `trim` the fraction `s` of the way along its
/// parameter range.
fn split_param(trim: &RimTrim<'_>, s: f64) -> Option<Q> {
    frac_param(trim, &Q::try_from(s).ok()?)
}

/// The parameter of trim `trim` the exact fraction `s` of the way along
/// its parameter range.
fn frac_param(trim: &RimTrim<'_>, s: &Q) -> Option<Q> {
    let t0 = Q::try_from(*trim.knots.first()?).ok()?;
    let t1 = Q::try_from(*trim.knots.last()?).ok()?;
    Some(&t0 + s * (t1 - &t0))
}

// ---------------------------------------------------------------------
// Vertices on `S` edges.

/// Per `S` edge, its vertices by parameter, named over all faces.
struct OnEdges {
    names: BTreeMap<EdgeId, BTreeMap<Q, (SnapVert, Pre)>>,
}

/// The vertices of every own edge of the faces (module docs).
fn edge_vertices(
    arranged: &[Arranged],
    faces: &[FaceData<'_>],
    ctx: &Ctx<'_>,
    ledger: &mut Ledger<'_>,
) -> Result<OnEdges, Stop<RewriteError>> {
    let mut cands: BTreeMap<EdgeId, BTreeMap<Q, Vec<(SnapVert, Pre)>>> = BTreeMap::new();
    for a in arranged {
        for info in a.points.values() {
            units(ledger, 1)?;
            for (h, t) in &info.on {
                cands
                    .entry(*h)
                    .or_default()
                    .entry(t.clone())
                    .or_default()
                    .extend(info.names.iter().cloned());
            }
        }
    }
    // The corner rule's splits, on the edges the faces hold.
    for sp in &ctx.corner.splits {
        units(ledger, 1)?;
        let Some((face, trim)) = faces.iter().find_map(|f| {
            f.face
                .own
                .iter()
                .find(|t| t.edge == sp.edge)
                .map(|t| (f.face.face, t))
        }) else {
            continue;
        };
        let Some(t) = split_param(trim, sp.s) else {
            return fail(RewriteError::Trim(face));
        };
        cands
            .entry(sp.edge)
            .or_default()
            .entry(t.clone())
            .or_default()
            .push((
                SnapVert::Split {
                    edge: sp.edge,
                    at: sp.at,
                },
                Pre {
                    parent: (ctx.s_op, sp.edge.into()),
                    at: PreImage::Param(t),
                },
            ));
    }
    // The `SnapAlong` pieces' ends, likewise.
    for b in ctx.brackets {
        units(ledger, 1)?;
        let Some((face, trim)) = faces.iter().find_map(|f| {
            f.face
                .own
                .iter()
                .find(|t| t.edge == b.edge)
                .map(|t| (f.face.face, t))
        }) else {
            continue;
        };
        let Some(t) = frac_param(trim, &b.s) else {
            return fail(RewriteError::Trim(face));
        };
        cands
            .entry(b.edge)
            .or_default()
            .entry(t.clone())
            .or_default()
            .push((
                b.key,
                Pre {
                    parent: (ctx.s_op, b.edge.into()),
                    at: PreImage::Param(t),
                },
            ));
    }
    let mut names = BTreeMap::new();
    for (h, by_t) in cands {
        let mut out = BTreeMap::new();
        for (t, ns) in by_t {
            let Some(best) = ns.into_iter().min_by_key(|n| n.0.rank(ctx.s_op)) else {
                // A point on the edge no rule names: it lies where the
                // edge meets nothing named; the faces' cells refuse it.
                continue;
            };
            out.insert(t, best);
        }
        names.insert(h, out);
    }
    Ok(OnEdges { names })
}

// ---------------------------------------------------------------------
// Cells.

/// A face's part of the rewrite.
struct Celled {
    faces: Vec<(NewFace, Pre)>,
    edges: Vec<(NewEdge, Pre)>,
    /// The vertices at their points.
    verts: Vec<(P2, (SnapVert, Pre))>,
}

/// An edge of a face's arrangement cut at its edge's vertices, keyed.
struct Sub {
    a: P2,
    b: P2,
    left: Side,
    right: Side,
    key: SnapEdge,
    /// Whether `a → b` runs the edge's way.
    forward: bool,
    /// The rim piece it lies on, if it isn't on an own edge.
    piece: Option<u32>,
    /// A target edge's piece's parameters, ascending.
    range: Option<[Q; 2]>,
}

/// The side of a cell whose half-edges have sides `a` and `b` on their
/// left, if they agree.
fn unify(a: Side, b: Side) -> Option<Side> {
    match (a, b) {
        (Side::Inner(x), Side::Inner(y)) => match (x, y) {
            (Some(x), Some(y)) if x != y => None,
            (Some(x), _) | (_, Some(x)) => Some(Side::Inner(Some(x))),
            _ => Some(Side::Inner(None)),
        },
        (x, y) => (x == y).then_some(x),
    }
}

/// Face `f`'s cells and their keys (module docs).
fn cells(
    f: &FaceData<'_>,
    arr: &Arranged,
    on_edges: &OnEdges,
    ctx: &Ctx<'_>,
    ledger: &mut Ledger<'_>,
) -> Result<Celled, Stop<RewriteError>> {
    let face = f.face.face;
    let s_op = ctx.s_op;
    let mut verts: BTreeMap<P2, (SnapVert, Pre)> = BTreeMap::new();
    let name_of =
        |p: &P2, on: Option<(EdgeId, &Q)>| -> Result<(SnapVert, Pre), Stop<RewriteError>> {
            let info = arr.points.get(p);
            let on = on.or_else(|| info.and_then(|i| i.on.first().map(|(h, t)| (*h, t))));
            if let Some((h, t)) = on {
                return on_edges
                    .names
                    .get(&h)
                    .and_then(|m| m.get(t))
                    .cloned()
                    .map_or(fail(RewriteError::Unnamed(face)), Ok);
            }
            info.and_then(|i| i.names.iter().min_by_key(|n| n.0.rank(s_op)).cloned())
                .map_or(fail(RewriteError::Unnamed(face)), Ok)
        };

    // Cut own edges at their edges' vertices; key every piece.
    let mut subs: Vec<Sub> = Vec::new();
    let mut edges: Vec<(NewEdge, Pre)> = Vec::new();
    let mut rim_parts: BTreeMap<u32, Vec<(Q, usize)>> = BTreeMap::new();
    let mut cut_parts: BTreeMap<EdgeId, Vec<(Q, usize)>> = BTreeMap::new();
    for e in &arr.els {
        units(ledger, 1)?;
        if let Some((h, [ta, tb])) = &e.own {
            let ts: Vec<&Q> = on_edges
                .names
                .get(h)
                .map(|m| m.keys().collect())
                .unwrap_or_default();
            let (lo, hi) = if ta < tb { (ta, tb) } else { (tb, ta) };
            let mut cut_at: Vec<Q> = ts
                .iter()
                .filter(|t| *t > &lo && *t < &hi)
                .map(|t| (*t).clone())
                .collect();
            if ta > tb {
                cut_at.reverse();
            }
            let mut pts: Vec<(P2, Q)> = vec![(e.a.clone(), ta.clone())];
            for t in cut_at {
                let x = (&t - ta) / (tb - ta);
                pts.push((lerp(&e.a, &e.b, &x), t));
            }
            pts.push((e.b.clone(), tb.clone()));
            for w in pts.windows(2) {
                let ((p, tp), (q, tq)) = (&w[0], &w[1]);
                let forward = tp < tq;
                let (t0, t1) = if forward { (tp, tq) } else { (tq, tp) };
                let part = ts
                    .iter()
                    .position(|t| *t == t0)
                    .ok_or(Stop::Failed(RewriteError::Unnamed(face)))?;
                if ts.get(part + 1) != Some(&t1) {
                    return fail(RewriteError::Unnamed(face));
                }
                let key = SnapEdge::Piece {
                    edge: *h,
                    part: part as u32,
                };
                let n0 = name_of(if forward { p } else { q }, Some((*h, t0)))?;
                let n1 = name_of(if forward { q } else { p }, Some((*h, t1)))?;
                let (start, end) = if forward { (p, q) } else { (q, p) };
                verts_insert(&mut verts, start, &n0);
                verts_insert(&mut verts, end, &n1);
                edges.push((
                    NewEdge {
                        key,
                        ends: [n0.0, n1.0],
                        rim: e.rim.as_ref().map(|r| r.0),
                    },
                    Pre {
                        parent: (s_op, (*h).into()),
                        at: PreImage::Range([t0.clone(), t1.clone()]),
                    },
                ));
                subs.push(Sub {
                    a: p.clone(),
                    b: q.clone(),
                    left: e.left,
                    right: e.right,
                    key,
                    forward,
                    piece: None,
                    range: None,
                });
            }
            continue;
        }
        let idx = subs.len();
        let mut range = None;
        let (key, forward, piece) = if let Some((c, [ta, tb])) = &e.cut {
            let r = if ta < tb {
                [ta.clone(), tb.clone()]
            } else {
                [tb.clone(), ta.clone()]
            };
            cut_parts.entry(*c).or_default().push((r[0].clone(), idx));
            range = Some(r);
            (
                SnapEdge::Cut {
                    face,
                    edge: *c,
                    part: 0,
                },
                ta < tb,
                e.rim.as_ref().map(|r| r.0),
            )
        } else if let Some((k, fwd, [sa, sb])) = &e.rim {
            rim_parts
                .entry(*k)
                .or_default()
                .push((if sa < sb { sa.clone() } else { sb.clone() }, idx));
            (
                SnapEdge::Rim {
                    face,
                    piece: *k,
                    part: 0,
                },
                *fwd,
                Some(*k),
            )
        } else {
            return fail(RewriteError::Cells(face));
        };
        subs.push(Sub {
            a: e.a.clone(),
            b: e.b.clone(),
            left: e.left,
            right: e.right,
            key,
            forward,
            piece,
            range,
        });
    }
    // Number rim parts and target edge pieces along them.
    for mut v in rim_parts.into_values().chain(cut_parts.into_values()) {
        v.sort();
        for (part, (_, i)) in v.into_iter().enumerate() {
            match &mut subs[i].key {
                SnapEdge::Rim { part: p, .. } | SnapEdge::Cut { part: p, .. } => *p = part as u32,
                SnapEdge::Piece { .. } | SnapEdge::Zero { .. } => {}
            }
        }
    }
    for s in subs
        .iter()
        .filter(|s| !matches!(s.key, SnapEdge::Piece { .. }))
    {
        units(ledger, 1)?;
        let (start, end) = if s.forward {
            (&s.a, &s.b)
        } else {
            (&s.b, &s.a)
        };
        let n0 = name_of(start, None)?;
        let n1 = name_of(end, None)?;
        verts_insert(&mut verts, start, &n0);
        verts_insert(&mut verts, end, &n1);
        let pre = match (s.key, &s.range) {
            (SnapEdge::Cut { edge, .. }, Some(r)) => Pre {
                parent: (s_op.other(), edge.into()),
                at: PreImage::Range(r.clone()),
            },
            _ => Pre {
                parent: (s_op, face.into()),
                at: PreImage::Segment([start.clone(), end.clone()]),
            },
        };
        edges.push((
            NewEdge {
                key: s.key,
                ends: [n0.0, n1.0],
                rim: s.piece,
            },
            pre,
        ));
    }

    // Half-edges: `2i` from `a` to `b`, `2i + 1` back, each with the side
    // on its left.
    let mut ids: BTreeMap<&P2, usize> = BTreeMap::new();
    for s in &subs {
        for p in [&s.a, &s.b] {
            let n = ids.len();
            ids.entry(p).or_insert(n);
        }
    }
    let pts: Vec<&P2> = {
        let mut v: Vec<(&P2, usize)> = ids.iter().map(|(p, i)| (*p, *i)).collect();
        v.sort_by_key(|x| x.1);
        v.into_iter().map(|x| x.0).collect()
    };
    let nh = 2 * subs.len();
    let from = |h: usize| -> usize {
        let s = &subs[h / 2];
        ids[if h % 2 == 0 { &s.a } else { &s.b }]
    };
    let to = |h: usize| from(h ^ 1);
    let left_of = |h: usize| {
        let s = &subs[h / 2];
        if h % 2 == 0 { s.left } else { s.right }
    };
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); pts.len()];
    for h in 0..nh {
        out[from(h)].push(h);
    }
    let dir = |h: usize| sub2(pts[to(h)], pts[from(h)]);
    for o in &mut out {
        units(ledger, o.len() as u64)?;
        o.sort_by(|&x, &y| ccw(&dir(x), &dir(y)));
    }
    let mut next = vec![usize::MAX; nh];
    for h in 0..nh {
        let o = &out[to(h)];
        let i = o
            .iter()
            .position(|&x| x == h ^ 1)
            .expect("the twin leaves the end");
        next[h] = o[(i + o.len() - 1) % o.len()];
    }
    // Cycles.
    let mut cycle_of = vec![usize::MAX; nh];
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..nh {
        if cycle_of[start] != usize::MAX {
            continue;
        }
        let c = cycles.len();
        let mut cyc = Vec::new();
        let mut h = start;
        while cycle_of[h] == usize::MAX {
            units(ledger, 1)?;
            cycle_of[h] = c;
            cyc.push(h);
            h = next[h];
        }
        cycles.push(cyc);
    }
    // Components, by union of the edges' ends.
    let mut comp: Vec<usize> = (0..pts.len()).collect();
    fn root(comp: &mut [usize], mut x: usize) -> usize {
        while comp[x] != x {
            comp[x] = comp[comp[x]];
            x = comp[x];
        }
        x
    }
    for i in 0..subs.len() {
        let (a, b) = (root(&mut comp, from(2 * i)), root(&mut comp, to(2 * i)));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            comp[hi] = lo;
        }
    }
    struct Cyc {
        side: Side,
        area: Q,
        comp: usize,
        segs: Vec<[P2; 2]>,
    }
    let mut cyc_info: Vec<Cyc> = Vec::with_capacity(cycles.len());
    for c in &cycles {
        let mut side = left_of(c[0]);
        for &h in c {
            side = unify(side, left_of(h)).ok_or(Stop::Failed(RewriteError::Cells(face)))?;
        }
        let poly: Vec<&P2> = c.iter().map(|&h| pts[from(h)]).collect();
        cyc_info.push(Cyc {
            side,
            area: area2(&poly),
            comp: root(&mut comp, from(c[0])),
            segs: c
                .iter()
                .map(|&h| [pts[from(h)].clone(), pts[to(h)].clone()])
                .collect(),
        });
    }
    // Holes into the least cell of another component holding them.
    let outers: Vec<usize> = (0..cycles.len())
        .filter(|&c| cyc_info[c].area > 0u32)
        .collect();
    let mut holes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for c in 0..cycles.len() {
        if cyc_info[c].area > 0u32 {
            continue;
        }
        let q = pts[from(cycles[c][0])];
        let mut best: Option<usize> = None;
        for &o in &outers {
            units(ledger, cyc_info[o].segs.len() as u64)?;
            if cyc_info[o].comp == cyc_info[c].comp || winding(&cyc_info[o].segs, q) == 0 {
                continue;
            }
            if best.is_none_or(|b| cyc_info[o].area < cyc_info[b].area) {
                best = Some(o);
            }
        }
        match best {
            Some(o) => holes.entry(o).or_default().push(c),
            None if cyc_info[c].side == Side::Out => {}
            None => return fail(RewriteError::Cells(face)),
        }
    }

    // The cells: kept and snapped pieces.
    let least = |c: usize| -> (&P2, usize) {
        cycles[c]
            .iter()
            .map(|&h| (pts[from(h)], h))
            .min_by(|x, y| x.0.cmp(y.0).then_with(|| pts[to(x.1)].cmp(pts[to(y.1)])))
            .expect("a cycle has a half-edge")
    };
    let mut cells: Vec<(u8, FaceId, P2, usize)> = Vec::new();
    for &o in &outers {
        let mut side = cyc_info[o].side;
        for &hc in holes.get(&o).map(Vec::as_slice).unwrap_or(&[]) {
            side = unify(side, cyc_info[hc].side).ok_or(Stop::Failed(RewriteError::Cells(face)))?;
        }
        let (class, on) = match side {
            Side::Out => continue,
            Side::Outer => (0, face),
            Side::Inner(Some(t)) => (1, t),
            Side::Inner(None) => {
                let mut ts: BTreeSet<FaceId> = BTreeSet::new();
                for &c in std::iter::once(&o).chain(holes.get(&o).map(Vec::as_slice).unwrap_or(&[]))
                {
                    for &h in &cycles[c] {
                        if let Some(k) = subs[h / 2].piece {
                            ts.extend(ctx.rim.pieces[k as usize].targets.iter().copied());
                        }
                    }
                }
                if ts.is_empty() {
                    ts = ctx
                        .rim
                        .pieces
                        .iter()
                        .filter(|p| p.face == face)
                        .flat_map(|p| p.targets.iter().copied())
                        .collect();
                }
                if ts.is_empty() {
                    ts = ctx.targets.clone();
                }
                match (ts.len(), ts.first()) {
                    (1, Some(&t)) => (1, t),
                    _ => return fail(RewriteError::TargetFace(face)),
                }
            }
        };
        cells.push((class, on, least(o).0.clone(), o));
    }
    cells.sort();
    let mut faces_out = Vec::with_capacity(cells.len());
    let mut counts: BTreeMap<(u8, FaceId), u32> = BTreeMap::new();
    for (class, on, _, o) in cells {
        let part = counts.entry((class, on)).or_insert(0);
        let (key, on_face) = if class == 0 {
            (SnapFace::Kept { face, part: *part }, (s_op, face))
        } else {
            (
                SnapFace::Snapped {
                    face,
                    target: on,
                    part: *part,
                },
                (s_op.other(), on),
            )
        };
        *part += 1;
        let mut hs: Vec<usize> = holes.get(&o).cloned().unwrap_or_default();
        hs.sort_by(|&x, &y| least(x).0.cmp(least(y).0));
        let mut loops = Vec::with_capacity(1 + hs.len());
        let mut region = Vec::with_capacity(1 + hs.len());
        for c in std::iter::once(o).chain(hs) {
            units(ledger, cycles[c].len() as u64)?;
            let start = least(c).1;
            let at = cycles[c]
                .iter()
                .position(|&h| h == start)
                .expect("on the cycle");
            let n = cycles[c].len();
            let ordered: Vec<usize> = (0..n).map(|i| cycles[c][(at + i) % n]).collect();
            loops.push(
                ordered
                    .iter()
                    .map(|&h| {
                        let s = &subs[h / 2];
                        Use {
                            edge: s.key,
                            forward: s.forward == (h % 2 == 0),
                        }
                    })
                    .collect(),
            );
            region.push(ordered.iter().map(|&h| pts[from(h)].clone()).collect());
        }
        faces_out.push((
            NewFace {
                key,
                on: on_face,
                loops,
            },
            Pre {
                parent: (s_op, face.into()),
                at: PreImage::Region(region),
            },
        ));
    }
    Ok(Celled {
        faces: faces_out,
        edges,
        verts: verts.into_iter().collect(),
    })
}

/// Records vertex `n` at point `p`.
fn verts_insert(verts: &mut BTreeMap<P2, (SnapVert, Pre)>, p: &P2, n: &(SnapVert, Pre)) {
    verts.entry(p.clone()).or_insert_with(|| n.clone());
}

mod geometry;

#[cfg_attr(
    not(test),
    expect(unused_imports, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) use geometry::geometry;
pub use geometry::{
    Chain, Coedge, EdgeClass, EdgeCurve, EdgeGeom, GeomInput, Geometry, TargetEdge, TargetFace,
    VertGeom,
};

#[cfg(test)]
mod tests;

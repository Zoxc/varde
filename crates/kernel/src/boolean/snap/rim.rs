//! The snap's rim: where a tangency cluster ends on the snapped side `S`,
//! the gap's sign along it, and the branch vertices where an
//! intersection curve of the operands leaves the cluster.
//!
//! **The region.** On each face of `S` the cluster covers the union of
//! its leaves' boxes of that face, closed, in the face's global UV
//! ([`SnappedFace`]), cut to the face itself (its own trims) and to the
//! part over the target's faces (the target's trims carried into this
//! face's UV by the caller). Both trims are given as straight UV spans
//! with the region on a side the caller states ([`RimTrim`]); with none
//! given the region isn't cut. The **rim** is the region's boundary,
//! exactly: the box union's boundary (maximal iso segments, found on the
//! grid of the boxes' sides) and the trims are cut at every point where
//! two of them meet (exact rational arithmetic), and each elementary
//! piece is on the rim when the region lies on exactly one of its sides.
//! A piece lying on a target trim **runs along that target edge**, else
//! one on an own trim along that `S` edge, else it is a **box** piece,
//! on a box side (the level-set part, the only one `b_K` moves). The rim
//! is oriented with the region on its left seen from outside: in the
//! face's UV on its left, or on its right on a face flipped against its
//! UV ([`SnappedFace::flipped`]), so loops run alike across faces whose
//! UVs turn opposite ways. A trim crossing the rim cuts
//! it there (a rim vertex), so every edge not running along the rim
//! crosses it at a vertex: with straight spans that is a transversal
//! crossing by construction.
//!
//! The region's boundary on each face, before anything is cancelled
//! across faces, is kept too ([`Rim::bounds`]): the rewrite reads a
//! face's part of the region from it.
//!
//! **Across faces.** A piece along an `S` edge whose other side is also
//! in the cluster (the edge between two faces of `S` the cluster covers,
//! or a seam) isn't rim: the parts of the edge's parameter range two
//! sides cover cancel, a side being a face and the way its pieces run
//! along the edge (two faces lie on the edge's two sides whichever way
//! their UVs turn). Rim pieces are then linked into **loops** by their end points:
//! a point on an `S` edge is that edge and parameter (an `S` vertex, the
//! vertex) on every face, any other point is its face's UV point. Where
//! several pieces leave one point, a piece arriving takes one on its face
//! first, the rightmost turn. Every loop is closed.
//!
//! **Pieces** ([`Piece`]): maximal straight stretches of one loop on one
//! face, of one kind (box, or along one edge), within one tile, split at
//! every point a trim passes through and where the set of target faces
//! beside the piece changes (the target faces of the leaves whose `S` box
//! meets the piece in a stretch of positive length). Loops start at their
//! least piece (face, then start point), and the pieces are numbered
//! along the loops in that order: ids break no geometric choice.
//!
//! **The gap** ([`PieceGap`]). Along a piece the `S − T` gap's sign
//! against a target surface (a plane, or a cylinder, [`Implicit`]: target
//! faces on one surface share one gap) is the sign of the surface's
//! implicit, oriented outward, along the piece: `G = F(S(uv(t)))`, a
//! Bernstein polynomial in the piece's parameter, exact (in homogeneous
//! form, times the positive weight's power). The piece is cut into
//! dyadic cells, each certified **one-signed** (every coefficient of one
//! strict sign), or a **root cluster** cell once its 3D diameter (the
//! diagonal of its control points' box) is at most `β_rim`, the piece's
//! band ([`RimInput::band`]: the rim edge's band, a target edge's band on
//! a piece along it). A gap identically zero (all coefficients zero) is
//! [`Gap::Zero`]: the piece lies on the target surface exactly, a tie
//! decided later, by perturbation.
//!
//! **Root clusters and branch vertices** ([`RootCluster`], [`Branch`]).
//! Along a loop, per target surface, the cells of the pieces beside that
//! surface form a sequence (cyclic when every piece of the loop is beside
//! it), cut at pieces whose gap is zero. A maximal run of cluster cells
//! is one root cluster; while its cells' boxes together span more than
//! `β_rim`, its cells are halved (refinement in space, exact, to
//! `MAX_SUBDIV_DEPTH`; never by halving `b_K`, which the rim doesn't
//! read). Its **net** sign change is read from the one-signed cells beside
//! it: `0` (a double root, a tangent branch, or two crossings within
//! `β_rim`: no vertex and no branch), or `±1`: one **branch vertex**,
//! placed by clamping: of the cluster's two ends and a Newton point of
//! the gap inside it, the one with the least residual (its exact distance
//! to the target surface), certified within `β_rim`. A cluster beside a
//! zero gap or an end of the sequence has no net (its end there is a rim
//! vertex on a target edge or on the target, decided with the records).
//! No transversality and no separation is asked: a piece reads the sign
//! of its one-signed cells, constant between its branch vertices.
//!
//! **The window** ([`Rim::window`]): the `S` edges and vertices of the
//! own trims, and the target edges and vertices of the target trims,
//! with a point in the closed region (inside the rim, or on it: a target
//! edge under a rim piece); the target edges between two of the
//! cluster's target faces, which no trim gives, are added the same way
//! (`add_inner_target`). Elements within the corner rule's radius of
//! a window element are the corner rule's to add.
//!
//! **Work.** Charged on the operation's budget in passes: the faces (an
//! item per face, a unit per grid cell, segment, pair of segments and
//! piece), the loops (one item, a unit per piece), the gaps (an item per
//! piece and surface, a unit per cell), the clusters (an item per loop, a
//! unit per cell split and per placement). The subdivision stops at
//! `MAX_SUBDIV_DEPTH`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use malachite::Integer;
use malachite::Rational;
use malachite::base::num::arithmetic::traits::Floor;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Floor as Down, Nearest};
use varde_decide::bezier::{Dir, Dyadic, HomPatch, Poly};
use varde_decide::count::boxes::UvSpan;
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::leaves::near_quadric::Quadric;
use varde_decide::num::bounded::{down, up};
use varde_decide::records::keys::Operand;
use varde_decide::records::shadow::MAX_SUBDIV_DEPTH;

use super::cluster::ClusterLeaf;
use crate::infra::budget::{Cap, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point of a face's global UV, exactly.
type P2 = [Q; 2];

/// A trimming edge carried into a snapped face's UV (module docs): its
/// spans straight, along the edge's parameter.
#[derive(Clone, Copy, Debug)]
pub struct RimTrim<'a> {
    /// The edge.
    pub edge: EdgeId,
    /// The edge's vertices at its start and its end.
    pub ends: [VertId; 2],
    /// Its spans in the face's global UV, `(u, v, w)` control points in
    /// the edge's parameter direction: each straight (its weights one
    /// positive value, its middle point the exact midpoint of its ends).
    pub spans: &'a [UvSpan],
    /// The edge's parameter at the spans' ends (one more than spans):
    /// the parameter runs linearly along each span.
    pub knots: &'a [f64],
    /// Whether the region it bounds lies on the left of the edge's
    /// parameter direction in this UV.
    pub left: bool,
}

/// A face of the snapped operand the cluster reaches (module docs).
#[derive(Clone, Copy, Debug)]
pub struct SnappedFace<'a> {
    /// The face.
    pub face: FaceId,
    /// Its tiles' patches, exact, keyed by tile: every tile one of the
    /// cluster's boxes lies in.
    pub tiles: &'a [([i64; 2], HomPatch<Rational>)],
    /// Its own trims, the face on the region's side; none: not cut.
    pub own: &'a [RimTrim<'a>],
    /// The boundary of the target faces' part carried into this face's
    /// UV, the target on the region's side; none: not cut.
    pub target: &'a [RimTrim<'a>],
    /// Whether the face's outward side is against its UV's `x_u × x_v`:
    /// its UV turns the other way about its outward normal.
    pub flipped: bool,
}

/// A target face's surface as the gap reads it: its implicit equation,
/// oriented so it is positive outside the target (on its outward side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Implicit {
    /// The plane `normal·x = offset`, `normal` the outward normal.
    Plane { normal: [f64; 3], offset: f64 },
    /// A cylinder ([`Quadric`]); `outward` when the face's outward normal
    /// points away from the axis.
    Cylinder { quadric: Quadric, outward: bool },
}

/// What a rim piece runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Along {
    /// A side of the union of the cluster's boxes: the level-set part.
    Box,
    /// An edge of the snapped operand.
    Own(EdgeId),
    /// An edge of the target.
    Target(EdgeId),
}

/// An edge or a vertex of either operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Element {
    /// An edge.
    Edge(Operand, EdgeId),
    /// A vertex.
    Vert(Operand, VertId),
}

/// What [`rim`] reads besides the leaves.
#[derive(Clone, Copy)]
pub struct RimInput<'a> {
    /// The snapped operand `S` (the target's other).
    pub snapped: Operand,
    /// The faces of `S` the cluster reaches.
    pub faces: &'a [SnappedFace<'a>],
    /// The target faces the cluster reaches, each with its surface.
    pub targets: &'a [(FaceId, Implicit)],
    /// The band `β_rim` of a rim piece along this (finite, `≥ 0`).
    pub band: &'a (dyn Fn(Along) -> f64 + Sync),
}

/// A piece of the rim (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// The face of `S` it lies on.
    pub face: FaceId,
    /// The tile it lies in.
    pub tile: [i64; 2],
    /// Its start and end in the face's global UV, the region on its left
    /// seen from outside (module docs).
    pub ends: [P2; 2],
    /// What it runs along.
    pub along: Along,
    /// For a piece along an edge, the edge's parameter at its start and
    /// its end.
    pub params: Option<[Q; 2]>,
    /// The target faces of the leaves beside it, sorted.
    pub targets: Vec<FaceId>,
}

/// A cell's gap sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    /// `S` certified outside the target.
    Positive,
    /// `S` certified inside the target.
    Negative,
    /// Identically zero: the piece lies on the target's surface.
    Zero,
    /// Not one-signed: part of a root cluster.
    Cluster,
}

/// The gap along a piece against one target surface (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct PieceGap {
    /// The piece's index.
    pub piece: u32,
    /// The target faces on that surface beside the piece, sorted.
    pub targets: Vec<FaceId>,
    /// Its cells in order along the piece, each a dyadic span of the
    /// piece's parameter with its sign.
    pub cells: Vec<([Dyadic; 2], Gap)>,
}

/// A root cluster of the gap along the rim (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct RootCluster {
    /// The target faces of its surface beside it.
    pub targets: Vec<FaceId>,
    /// Where it starts and ends: a piece's index and the parameter on it.
    pub from: (u32, Dyadic),
    pub to: (u32, Dyadic),
    /// The sign change across it along the rim (`+1` from inside to
    /// outside the target), if both its neighbours are one-signed.
    pub net: Option<i8>,
}

/// A branch vertex (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    /// The piece it lies on, and its parameter there.
    pub piece: u32,
    pub at: Q,
    /// Its point in the face's global UV.
    pub uv: P2,
    /// The target faces of its surface beside it.
    pub targets: Vec<FaceId>,
    /// Whether the gap rises across it along the rim (inside to outside).
    pub rising: bool,
    /// An upper bound of its distance to the target surface, `≤ β_rim`.
    pub residual: f64,
}

/// A stretch of the boundary of the cluster's region on one face of `S`,
/// the region on its left: the rim's pieces before the parts of own edges
/// both sides cover are cancelled, so a face's stretches close round its
/// part of the region.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bound {
    /// The face of `S`.
    pub face: FaceId,
    /// Its start and end in the face's global UV.
    pub ends: [P2; 2],
}

/// A cluster's rim (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Rim {
    /// The pieces, numbered along the loops.
    pub pieces: Vec<Piece>,
    /// The loops, each its pieces' indices in order.
    pub loops: Vec<Vec<u32>>,
    /// The gaps, by piece, then surface.
    pub gaps: Vec<PieceGap>,
    /// The root clusters, along the loops.
    pub clusters: Vec<RootCluster>,
    /// The branch vertices, along the loops.
    pub branches: Vec<Branch>,
    /// The window's elements, sorted.
    pub window: Vec<Element>,
    /// The region's boundary per face, sorted.
    pub bounds: Vec<Bound>,
}

/// Why a rim can't be made.
#[derive(Clone, Debug, PartialEq)]
pub enum RimError {
    /// No leaf has a box of `S`.
    Empty,
    /// The leaf at this position doesn't have a box of each operand.
    Operands(u32),
    /// A face of `S` the leaves reach isn't described, or lacks a tile.
    Face(FaceId),
    /// A target face the leaves reach has no surface given.
    Target(FaceId),
    /// A trim span of this face isn't straight, or a value isn't finite,
    /// or its knots don't fit its spans.
    Trim(FaceId),
    /// The region's boundary runs both ways along a stretch of this face
    /// (a slit), which the rim doesn't take yet.
    Slit(FaceId),
    /// A tile's weights aren't positive, or its degrees too high.
    Patch(FaceId),
    /// A band isn't finite and `≥ 0`.
    Band,
    /// The rim's pieces don't close into loops at a point: the trims
    /// given don't bound consistent regions.
    Open,
    /// A branch vertex's residual isn't certified within the band: the
    /// decisions disagree (a ladder signal).
    Inconsistent,
    /// The budget ran out or a cell reached the depth limit
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The rim of the cluster of `leaves` (module docs), charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn rim<L: ClusterLeaf>(
    leaves: &[L],
    input: &RimInput<'_>,
    work: &mut Work,
) -> Result<Rim, RimError> {
    let target_of = |f: FaceId| input.targets.iter().find(|(g, _)| *g == f).map(|t| t.1);
    // The boxes of `S` per face, each with its leaves' target faces.
    let mut boxes: BTreeMap<FaceId, BTreeMap<[[Q; 2]; 2], BTreeSet<FaceId>>> = BTreeMap::new();
    for (i, l) in leaves.iter().enumerate() {
        let at = u32::try_from(i).unwrap_or(u32::MAX);
        let [a, b] = l.boxes();
        if a.operand() == b.operand() {
            return Err(RimError::Operands(at));
        }
        let (s, t) = if a.operand() == input.snapped {
            (a, b)
        } else {
            (b, a)
        };
        if target_of(t.face()).is_none() {
            return Err(RimError::Target(t.face()));
        }
        let [i0, j0] = s.tile();
        let side = |k: i64, d: Dir| {
            let e = s.extent(d);
            [
                Q::from(k) + e.lo().rational(),
                Q::from(k) + e.hi().rational(),
            ]
        };
        boxes
            .entry(s.face())
            .or_default()
            .entry([side(i0, Dir::U), side(j0, Dir::V)])
            .or_default()
            .insert(t.face());
        let face = input
            .faces
            .iter()
            .find(|f| f.face == s.face())
            .ok_or(RimError::Face(s.face()))?;
        if !face.tiles.iter().any(|(k, _)| *k == s.tile()) {
            return Err(RimError::Face(s.face()));
        }
    }
    if boxes.is_empty() {
        return Err(RimError::Empty);
    }
    // Each face's arrangement.
    let jobs: Vec<FaceJob<'_>> = boxes
        .iter()
        .map(|(f, bs)| FaceJob {
            face: *input
                .faces
                .iter()
                .find(|g| g.face == *f)
                .expect("checked above"),
            boxes: bs.iter().map(|(b, ts)| (b.clone(), ts.clone())).collect(),
        })
        .collect();
    let faces = run(work, "snap rim: faces", &jobs, |job, ledger| {
        face_rim(job, input.snapped, ledger)
    })?;

    let mut bounds: Vec<Bound> = faces
        .iter()
        .flat_map(|f| f.els.iter())
        .map(|e| Bound {
            face: e.face,
            ends: [e.a.clone(), e.b.clone()],
        })
        .collect();
    bounds.sort();

    // Cancel, link, merge and split, in one item.
    let linked = run(work, "snap rim: loops", &[()], |_, ledger| {
        loops(&faces, &jobs, ledger)
    })?
    .into_iter()
    .next()
    .expect("one item");
    let (pieces, loops) = linked;

    // The surfaces: distinct implicits, in the order of their least face.
    let mut surfaces: Vec<(Implicit, Vec<FaceId>)> = Vec::new();
    let mut by_face: Vec<(FaceId, Implicit)> = input.targets.to_vec();
    by_face.sort_by_key(|t| t.0);
    for (f, imp) in by_face {
        match surfaces.iter_mut().find(|s| s.0 == imp) {
            Some(s) => s.1.push(f),
            None => surfaces.push((imp, vec![f])),
        }
    }
    let mut gap_jobs: Vec<GapJob<'_>> = Vec::new();
    for (k, p) in pieces.iter().enumerate() {
        for (si, (imp, fs)) in surfaces.iter().enumerate() {
            let targets: Vec<FaceId> = p
                .targets
                .iter()
                .copied()
                .filter(|f| fs.contains(f))
                .collect();
            if targets.is_empty() {
                continue;
            }
            let face = jobs
                .iter()
                .find(|j| j.face.face == p.face)
                .expect("a piece's face has boxes")
                .face;
            let beta = (input.band)(p.along);
            if !(beta.is_finite() && beta >= 0.0) {
                return Err(RimError::Band);
            }
            gap_jobs.push(GapJob {
                piece: k as u32,
                surface: si as u32,
                implicit: *imp,
                targets,
                patch: patch_of(&face, p.tile).ok_or(RimError::Face(p.face))?,
                ends: p.ends.clone(),
                tile: p.tile,
                face: p.face,
                beta,
            });
        }
    }
    let mut gaps = run(work, "snap rim: gaps", &gap_jobs, |job, ledger| {
        piece_gap(job, ledger)
    })?;

    // Per loop: the sequences per surface, refined, their clusters and
    // branch vertices.
    let mut gap_at: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (g, job) in gap_jobs.iter().enumerate() {
        gap_at.insert((job.piece, job.surface), g);
    }
    let mut loop_jobs: Vec<LoopJob> = Vec::new();
    for l in &loops {
        let mut seqs = Vec::new();
        for si in 0..surfaces.len() as u32 {
            sequences(l, si, &gap_at, &gaps, &mut seqs);
        }
        loop_jobs.push(LoopJob { seqs });
    }
    let per_loop = run(work, "snap rim: clusters", &loop_jobs, |job, ledger| {
        loop_clusters(job, &gaps, &gap_jobs, ledger)
    })?;
    let mut clusters = Vec::new();
    let mut branches = Vec::new();
    for out in per_loop {
        for (g, cells) in out.refined {
            gaps[g].cells = cells;
        }
        clusters.extend(out.clusters);
        branches.extend(out.branches);
    }

    let window: BTreeSet<Element> = faces
        .iter()
        .flat_map(|f| f.window.iter().copied())
        .collect();
    let gaps = gaps
        .into_iter()
        .zip(&gap_jobs)
        .map(|(g, job)| PieceGap {
            piece: job.piece,
            targets: job.targets.clone(),
            cells: g.cells.iter().map(|c| (c.span(), c.gap)).collect(),
        })
        .collect();
    Ok(Rim {
        pieces,
        loops,
        gaps,
        clusters,
        branches,
        window: window.into_iter().collect(),
        bounds,
    })
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(work: &mut Work, stage: &'static str, items: &[T], f: F) -> Result<Vec<R>, RimError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<RimError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => RimError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<RimError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

fn fail<T>(e: RimError) -> Result<T, Stop<RimError>> {
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

fn exact(x: f64) -> Option<Q> {
    Q::try_from(x).ok()
}

// ---------------------------------------------------------------------
// A face's arrangement.

/// A face's part of the rim: its boxes with their target faces.
struct FaceJob<'a> {
    face: SnappedFace<'a>,
    boxes: Vec<([[Q; 2]; 2], BTreeSet<FaceId>)>,
}

/// Where a segment of a face's arrangement comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Src {
    Box,
    /// An own trim and its span.
    Own(u32, u32),
    /// A target trim and its span.
    Target(u32, u32),
}

/// A segment with its region on the left.
struct Seg {
    a: P2,
    b: P2,
    src: Src,
}

/// A trim's span, exactly: its ends in the edge's direction and the
/// edge's parameter there.
#[derive(Clone)]
pub(super) struct ExactSpan {
    pub(super) p: [P2; 2],
    pub(super) t: [Q; 2],
}

/// A point's name on the rim across faces: an `S` vertex, a point of an
/// `S` edge by its parameter, else the face's UV point.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Vert(VertId),
    On(EdgeId, Q),
    At(FaceId, P2),
}

/// An elementary rim piece of one face.
#[derive(Clone, Debug)]
struct El {
    face: FaceId,
    a: P2,
    b: P2,
    along: Along,
    params: Option<[Q; 2]>,
}

/// A face's arrangement result.
struct FaceRim {
    els: Vec<El>,
    /// Points a trim passes through: no merging across them.
    hard: BTreeSet<P2>,
    /// Points on own trims, named across faces.
    alias: BTreeMap<P2, Node>,
    window: Vec<Element>,
}

/// The exact spans of `trims`, checked straight.
pub(super) fn exact_spans(
    face: FaceId,
    trims: &[RimTrim<'_>],
) -> Result<Vec<Vec<ExactSpan>>, RimError> {
    let mut out = Vec::with_capacity(trims.len());
    for t in trims {
        if t.knots.len() != t.spans.len() + 1 {
            return Err(RimError::Trim(face));
        }
        let mut spans = Vec::with_capacity(t.spans.len());
        for (k, s) in t.spans.iter().enumerate() {
            let w = s[0][2];
            if w.partial_cmp(&0.0) != Some(Ordering::Greater) || s.iter().any(|p| p[2] != w) {
                return Err(RimError::Trim(face));
            }
            let pt = |p: &[f64; 3]| -> Option<P2> { Some([exact(p[0])?, exact(p[1])?]) };
            let (Some(p0), Some(p1), Some(p2)) = (pt(&s[0]), pt(&s[1]), pt(&s[2])) else {
                return Err(RimError::Trim(face));
            };
            if mid(&p0, &p2) != p1 {
                return Err(RimError::Trim(face));
            }
            let (Some(t0), Some(t1)) = (exact(t.knots[k]), exact(t.knots[k + 1])) else {
                return Err(RimError::Trim(face));
            };
            spans.push(ExactSpan {
                p: [p0, p2],
                t: [t0, t1],
            });
        }
        out.push(spans);
    }
    Ok(out)
}

/// The boundary of the union of closed `boxes`, as maximal iso segments
/// with the union on their left; the work is a unit per grid cell.
fn union_boundary(
    boxes: &[[[Q; 2]; 2]],
    ledger: &mut Ledger<'_>,
) -> Result<Vec<Seg>, Stop<RimError>> {
    let coords = |k: usize| {
        let mut v: Vec<Q> = boxes.iter().flat_map(|b| b[k].clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    let (xs, ys) = (coords(0), coords(1));
    let (nx, ny) = (xs.len(), ys.len());
    units(ledger, (nx as u64).saturating_mul(ny as u64))?;
    // Cell `(i, j)` is `[xs[i], xs[i+1]] × [ys[j], ys[j+1]]`; covered
    // counts by a difference array.
    let mut d = vec![0i64; (nx + 1) * (ny + 1)];
    let at = |i: usize, j: usize| i * (ny + 1) + j;
    for b in boxes {
        let i0 = xs.binary_search(&b[0][0]).expect("a side");
        let i1 = xs.binary_search(&b[0][1]).expect("a side");
        let j0 = ys.binary_search(&b[1][0]).expect("a side");
        let j1 = ys.binary_search(&b[1][1]).expect("a side");
        d[at(i0, j0)] += 1;
        d[at(i1, j0)] -= 1;
        d[at(i0, j1)] -= 1;
        d[at(i1, j1)] += 1;
    }
    for i in 0..=nx {
        for j in 1..=ny {
            d[at(i, j)] += d[at(i, j - 1)];
        }
    }
    for i in 1..=nx {
        for j in 0..=ny {
            d[at(i, j)] += d[at(i - 1, j)];
        }
    }
    let inside = |i: isize, j: isize| -> bool {
        i >= 0
            && j >= 0
            && (i as usize) + 1 < nx
            && (j as usize) + 1 < ny
            && d[at(i as usize, j as usize)] > 0
    };
    let mut out = Vec::new();
    // Vertical lines: up where the union is on the left (`−u`).
    for (i, x) in xs.iter().enumerate() {
        let mut run: Option<(usize, bool)> = None;
        for j in 0..=ny.saturating_sub(1) {
            let state = if j + 1 < ny {
                let (l, r) = (
                    inside(i as isize - 1, j as isize),
                    inside(i as isize, j as isize),
                );
                (l != r).then_some(l)
            } else {
                None
            };
            match (run, state) {
                (Some((_, up)), Some(s)) if s == up => {}
                _ => {
                    if let Some((j0, up)) = run.take() {
                        let (lo, hi) = ([x.clone(), ys[j0].clone()], [x.clone(), ys[j].clone()]);
                        out.push(if up {
                            Seg {
                                a: lo,
                                b: hi,
                                src: Src::Box,
                            }
                        } else {
                            Seg {
                                a: hi,
                                b: lo,
                                src: Src::Box,
                            }
                        });
                    }
                    run = state.map(|s| (j, s));
                }
            }
        }
    }
    // Horizontal lines: towards `+u` where the union is above (`+v`).
    for (j, y) in ys.iter().enumerate() {
        let mut run: Option<(usize, bool)> = None;
        for i in 0..=nx.saturating_sub(1) {
            let state = if i + 1 < nx {
                let (below, above) = (
                    inside(i as isize, j as isize - 1),
                    inside(i as isize, j as isize),
                );
                (below != above).then_some(above)
            } else {
                None
            };
            match (run, state) {
                (Some((_, above)), Some(s)) if s == above => {}
                _ => {
                    if let Some((i0, above)) = run.take() {
                        let (lo, hi) = ([xs[i0].clone(), y.clone()], [xs[i].clone(), y.clone()]);
                        out.push(if above {
                            Seg {
                                a: lo,
                                b: hi,
                                src: Src::Box,
                            }
                        } else {
                            Seg {
                                a: hi,
                                b: lo,
                                src: Src::Box,
                            }
                        });
                    }
                    run = state.map(|s| (i, s));
                }
            }
        }
    }
    Ok(out)
}

/// The split parameters `segs[i]` takes from `segs[j]` and back, pushed
/// to `params`.
fn meet(segs: &[Seg], i: usize, j: usize, params: &mut [Vec<Q>]) {
    let (s, t) = (&segs[i], &segs[j]);
    let (si, tj) = meet_params([&s.a, &s.b], [&t.a, &t.b]);
    params[i].extend(si);
    params[j].extend(tj);
}

/// The parameters (`0` at the start, `1` at the end) where the straight
/// segments `s` and `t` meet, on each: their crossing, or where
/// collinear, each one's ends strictly inside the other.
pub(super) fn meet_params(s: [&P2; 2], t: [&P2; 2]) -> (Vec<Q>, Vec<Q>) {
    let (mut on_s, mut on_t) = (Vec::new(), Vec::new());
    // Boxes apart: nothing.
    for k in 0..2 {
        let (slo, shi) = minmax(&s[0][k], &s[1][k]);
        let (tlo, thi) = minmax(&t[0][k], &t[1][k]);
        if shi < tlo || thi < slo {
            return (on_s, on_t);
        }
    }
    let (d1, d2) = (sub2(s[1], s[0]), sub2(t[1], t[0]));
    let den = cross2(&d1, &d2);
    let w = sub2(t[0], s[0]);
    if den != 0u32 {
        let si = cross2(&w, &d2) / &den;
        let tj = cross2(&w, &d1) / &den;
        let unit = |x: &Q| *x >= 0u32 && *x <= 1u32;
        if unit(&si) && unit(&tj) {
            on_s.push(si);
            on_t.push(tj);
        }
        return (on_s, on_t);
    }
    if cross2(&w, &d1) != 0u32 {
        return (on_s, on_t);
    }
    // Collinear: each one's ends inside the other.
    let along = |a: &P2, d: &P2, p: &P2| dot2(&sub2(p, a), d) / dot2(d, d);
    for p in t {
        let x = along(s[0], &d1, p);
        if x > 0u32 && x < 1u32 {
            on_s.push(x);
        }
    }
    for p in s {
        let x = along(t[0], &d2, p);
        if x > 0u32 && x < 1u32 {
            on_t.push(x);
        }
    }
    (on_s, on_t)
}

fn minmax<'q>(a: &'q Q, b: &'q Q) -> (&'q Q, &'q Q) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The winding number of the closed oriented `segs` about `p` (off them):
/// the signed crossings of the ray from `p` along `+u`, half-open in `v`.
fn winding<'s>(segs: impl Iterator<Item = &'s Seg>, p: &P2) -> i64 {
    let mut w = 0i64;
    for s in segs {
        let (a, b) = (&s.a, &s.b);
        let c = cross2(&sub2(b, a), &sub2(p, a));
        if a[1] <= p[1] && p[1] < b[1] && c > 0u32 {
            w += 1;
        } else if b[1] <= p[1] && p[1] < a[1] && c < 0u32 {
            w -= 1;
        }
    }
    w
}

/// The three regions a face's rim bounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    Boxes,
    Own,
    Target,
}

fn region_of(src: Src) -> Region {
    match src {
        Src::Box => Region::Boxes,
        Src::Own(..) => Region::Own,
        Src::Target(..) => Region::Target,
    }
}

/// The closed region's boundary on face `face` holds `p`, or `p` lies
/// inside it: by the winding of [`Rim::bounds`] there, exactly.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
fn in_closed_region(rim: &Rim, face: FaceId, p: &P2) -> bool {
    let mut w = 0i64;
    for b in rim.bounds.iter().filter(|b| b.face == face) {
        let (a, c) = (&b.ends[0], &b.ends[1]);
        let d = sub2(c, a);
        let x = cross2(&d, &sub2(p, a));
        if x == 0u32 && dot2(&sub2(p, a), &sub2(p, c)) <= 0u32 {
            return true;
        }
        if a[1] <= p[1] && p[1] < c[1] && x > 0u32 {
            w += 1;
        } else if c[1] <= p[1] && p[1] < a[1] && x < 0u32 {
            w -= 1;
        }
    }
    w != 0
}

/// Adds to `rim`'s window the target edges inside the target faces' part
/// (`inner`: per face of `S`, the edges between two of the cluster's
/// target faces, carried into its UV as [`RimTrim`]s, `left` unread),
/// those with a point in the closed region, and their vertices there.
/// The rim's target trims bound that part, so they don't give its inner
/// edges, which the window holds as well (module docs).
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn add_inner_target(
    rim: &mut Rim,
    target: Operand,
    inner: &[(FaceId, RimTrim<'_>)],
) -> Result<(), RimError> {
    let mut window: BTreeSet<Element> = rim.window.iter().copied().collect();
    for (face, t) in inner {
        let spans = exact_spans(*face, std::slice::from_ref(t))?;
        let Some(ss) = spans.first() else { continue };
        let mut meets = false;
        for s in ss {
            let ends_in = [&s.p[0], &s.p[1]].map(|p| in_closed_region(rim, *face, p));
            let crosses = rim.bounds.iter().filter(|b| b.face == *face).any(|b| {
                !meet_params([&s.p[0], &s.p[1]], [&b.ends[0], &b.ends[1]])
                    .0
                    .is_empty()
            });
            meets |= ends_in[0] || ends_in[1] || crosses;
        }
        if meets {
            window.insert(Element::Edge(target, t.edge));
        }
        if let (Some(first), Some(last)) = (ss.first(), ss.last()) {
            for (p, v) in [(&first.p[0], t.ends[0]), (&last.p[1], t.ends[1])] {
                if in_closed_region(rim, *face, p) {
                    window.insert(Element::Vert(target, v));
                }
            }
        }
    }
    rim.window = window.into_iter().collect();
    Ok(())
}

/// The edge parameter of point `p` on `span`.
pub(super) fn param_on(span: &ExactSpan, p: &P2) -> Q {
    let d = sub2(&span.p[1], &span.p[0]);
    let s = dot2(&sub2(p, &span.p[0]), &d) / dot2(&d, &d);
    &span.t[0] + s * (&span.t[1] - &span.t[0])
}

/// Face `job`'s elementary rim pieces, hard points, names and window.
fn face_rim(
    job: &FaceJob<'_>,
    snapped: Operand,
    ledger: &mut Ledger<'_>,
) -> Result<FaceRim, Stop<RimError>> {
    let face = job.face.face;
    let target_op = match snapped {
        Operand::A => Operand::B,
        Operand::B => Operand::A,
    };
    let own = exact_spans(face, job.face.own).map_err(Stop::Failed)?;
    let target = exact_spans(face, job.face.target).map_err(Stop::Failed)?;
    let boxes: Vec<[[Q; 2]; 2]> = job.boxes.iter().map(|b| b.0.clone()).collect();
    let mut segs = union_boundary(&boxes, ledger)?;
    let mut trim_segs = |trims: &[RimTrim<'_>], spans: &[Vec<ExactSpan>], own: bool| {
        for (ti, (t, ss)) in trims.iter().zip(spans).enumerate() {
            for (si, s) in ss.iter().enumerate() {
                if s.p[0] == s.p[1] {
                    continue;
                }
                let src = if own {
                    Src::Own(ti as u32, si as u32)
                } else {
                    Src::Target(ti as u32, si as u32)
                };
                let (a, b) = if t.left {
                    (s.p[0].clone(), s.p[1].clone())
                } else {
                    (s.p[1].clone(), s.p[0].clone())
                };
                segs.push(Seg { a, b, src });
            }
        }
    };
    trim_segs(job.face.own, &own, true);
    trim_segs(job.face.target, &target, false);
    let n = segs.len();
    units(ledger, n as u64)?;
    let mut params: Vec<Vec<Q>> = (0..n).map(|_| vec![Q::from(0u32), Q::from(1u32)]).collect();
    for i in 0..n {
        units(ledger, (n - i) as u64)?;
        for j in i + 1..n {
            meet(&segs, i, j, &mut params);
        }
    }
    // Elementary pieces grouped by their ends (the lesser first).
    let mut groups: BTreeMap<(P2, P2), Vec<(bool, Src)>> = BTreeMap::new();
    for (s, ps) in segs.iter().zip(params.iter_mut()) {
        ps.sort();
        ps.dedup();
        units(ledger, ps.len() as u64)?;
        let pts: Vec<P2> = ps.iter().map(|x| lerp(&s.a, &s.b, x)).collect();
        for w in pts.windows(2) {
            let (p, q) = (&w[0], &w[1]);
            let fwd = p < q;
            let key = if fwd {
                (p.clone(), q.clone())
            } else {
                (q.clone(), p.clone())
            };
            groups.entry(key).or_default().push((fwd, s.src));
        }
    }
    let inside_boxes = |p: &P2| {
        job.boxes
            .iter()
            .any(|(b, _)| (0..2).all(|k| b[k][0] <= p[k] && p[k] <= b[k][1]))
    };
    let region_segs = |r: Region| segs.iter().filter(move |s| region_of(s.src) == r);
    let has = [true, !job.face.own.is_empty(), !job.face.target.is_empty()];
    let mut els = Vec::new();
    let mut hard = BTreeSet::new();
    let mut alias: BTreeMap<P2, Node> = BTreeMap::new();
    let mut closed_nodes: BTreeSet<P2> = BTreeSet::new();
    // Per trim piece: its trim (own or not, index) and whether it is in
    // the closed region.
    let mut trim_pieces: Vec<(bool, u32, [P2; 2], bool)> = Vec::new();
    for ((p, q), entries) in &groups {
        units(ledger, 1)?;
        let m = mid(p, q);
        let mut left = true;
        let mut right = true;
        for (k, r) in [Region::Boxes, Region::Own, Region::Target]
            .into_iter()
            .enumerate()
        {
            let dirs: BTreeSet<bool> = entries
                .iter()
                .filter(|e| region_of(e.1) == r)
                .map(|e| e.0)
                .collect();
            let (l, rt) = match dirs.len() {
                0 => {
                    let inside = !has[k]
                        || match r {
                            Region::Boxes => inside_boxes(&m),
                            _ => winding(region_segs(r), &m) != 0,
                        };
                    (inside, inside)
                }
                1 => {
                    let fwd = dirs.contains(&true);
                    (fwd, !fwd)
                }
                _ => return fail(RimError::Slit(face)),
            };
            left &= l;
            right &= rt;
        }
        let closed = left || right;
        if closed {
            closed_nodes.insert(p.clone());
            closed_nodes.insert(q.clone());
        }
        let mut target_e: Option<(EdgeId, [Q; 2])> = None;
        let mut own_e: Option<(EdgeId, [Q; 2])> = None;
        for &(_, src) in entries {
            let (trims, spans, is_own, ti, si) = match src {
                Src::Box => continue,
                Src::Own(t, s) => (job.face.own, &own, true, t, s),
                Src::Target(t, s) => (job.face.target, &target, false, t, s),
            };
            hard.insert(p.clone());
            hard.insert(q.clone());
            trim_pieces.push((is_own, ti, [p.clone(), q.clone()], closed));
            let t = &trims[ti as usize];
            let span = &spans[ti as usize][si as usize];
            let pq = [param_on(span, p), param_on(span, q)];
            if is_own {
                for (pt, tp) in [(p, &pq[0]), (q, &pq[1])] {
                    let name = node_on(t, spans[ti as usize].as_slice(), tp);
                    match alias.get(pt) {
                        Some(old) if *old <= name => {}
                        _ => {
                            alias.insert(pt.clone(), name);
                        }
                    }
                }
            }
            let slot = if is_own { &mut own_e } else { &mut target_e };
            if slot.as_ref().is_none_or(|(e, _)| t.edge < *e) {
                *slot = Some((t.edge, pq));
            }
        }
        if left == right {
            continue;
        }
        let (a, b) = if left {
            (p.clone(), q.clone())
        } else {
            (q.clone(), p.clone())
        };
        let (along, params) = match (target_e, own_e) {
            (Some((e, pq)), _) => (Along::Target(e), Some(pq)),
            (None, Some((e, pq))) => (Along::Own(e), Some(pq)),
            (None, None) => (Along::Box, None),
        };
        let params = params.map(|[x, y]| if left { [x, y] } else { [y, x] });
        els.push(El {
            face,
            a,
            b,
            along,
            params,
        });
    }
    // The window: trims with a piece in the closed region or touching it.
    let mut window = BTreeSet::new();
    for (is_own, ti, [p, q], closed) in &trim_pieces {
        let touches = *closed || closed_nodes.contains(p) || closed_nodes.contains(q);
        let (trims, op) = if *is_own {
            (job.face.own, snapped)
        } else {
            (job.face.target, target_op)
        };
        let t = &trims[*ti as usize];
        if touches {
            window.insert(Element::Edge(op, t.edge));
        }
    }
    for (trims, spans, op) in [
        (job.face.own, &own, snapped),
        (job.face.target, &target, target_op),
    ] {
        for (t, ss) in trims.iter().zip(spans) {
            if let (Some(first), Some(last)) = (ss.first(), ss.last()) {
                for (p, v) in [(&first.p[0], t.ends[0]), (&last.p[1], t.ends[1])] {
                    if closed_nodes.contains(p) {
                        window.insert(Element::Vert(op, v));
                    }
                }
            }
        }
    }
    Ok(FaceRim {
        els,
        hard,
        alias,
        window: window.into_iter().collect(),
    })
}

/// The name of the point at parameter `t` of own trim `trim`: its vertex
/// at an end, else the edge and parameter.
fn node_on(trim: &RimTrim<'_>, spans: &[ExactSpan], t: &Q) -> Node {
    match (spans.first(), spans.last()) {
        (Some(f), _) if f.t[0] == *t => Node::Vert(trim.ends[0]),
        (_, Some(l)) if l.t[1] == *t => Node::Vert(trim.ends[1]),
        _ => Node::On(trim.edge, t.clone()),
    }
}

// ---------------------------------------------------------------------
// Loops.

/// A rim piece while loops are made: its face's piece and its end names.
#[derive(Clone, Debug)]
struct Link {
    el: El,
    from: Node,
    to: Node,
    /// Whether its ends are hard (no merging across them).
    hard: [bool; 2],
}

/// The ends of each own edge, from the trims: start parameter and vertex,
/// end parameter and vertex.
type EdgeEnds = BTreeMap<EdgeId, ((Q, VertId), (Q, VertId))>;

fn edge_ends(jobs: &[FaceJob<'_>]) -> EdgeEnds {
    let mut out = EdgeEnds::new();
    for j in jobs {
        for t in j.face.own {
            if let (Some(&k0), Some(&k1)) = (t.knots.first(), t.knots.last())
                && let (Some(a), Some(b)) = (exact(k0), exact(k1))
            {
                out.entry(t.edge)
                    .or_insert(((a, t.ends[0]), (b, t.ends[1])));
            }
        }
    }
    out
}

/// The pieces, numbered along the loops, and the loops.
type Loops = (Vec<Piece>, Vec<Vec<u32>>);

/// Cancels own-edge pieces both sides cover, links the rest into loops,
/// merges straight runs and splits at tile lines and where the target
/// faces beside a piece change; the pieces numbered along the loops.
fn loops(
    faces: &[FaceRim],
    jobs: &[FaceJob<'_>],
    ledger: &mut Ledger<'_>,
) -> Result<Loops, Stop<RimError>> {
    let ends = edge_ends(jobs);
    let name = |f: &FaceRim, face: FaceId, p: &P2| {
        f.alias
            .get(p)
            .cloned()
            .unwrap_or_else(|| Node::At(face, p.clone()))
    };
    let mut links: Vec<Link> = Vec::new();
    for f in faces {
        units(ledger, f.els.len() as u64)?;
        for e in &f.els {
            links.push(Link {
                from: name(f, e.face, &e.a),
                to: name(f, e.face, &e.b),
                hard: [f.hard.contains(&e.a), f.hard.contains(&e.b)],
                el: e.clone(),
            });
        }
    }
    let links = cancel(links, &ends);
    // Seen from outside: on a flipped face the region lies right of a
    // piece in UV, so its pieces turn round to run as its neighbours'.
    let flipped: BTreeSet<FaceId> = jobs
        .iter()
        .filter(|j| j.face.flipped)
        .map(|j| j.face.face)
        .collect();
    let links: Vec<Link> = links
        .into_iter()
        .map(|l| {
            if !flipped.contains(&l.el.face) {
                return l;
            }
            Link {
                from: l.to,
                to: l.from,
                hard: [l.hard[1], l.hard[0]],
                el: El {
                    face: l.el.face,
                    a: l.el.b,
                    b: l.el.a,
                    along: l.el.along,
                    params: l.el.params.map(|[p, q]| [q, p]),
                },
            }
        })
        .collect();

    // Link: at each node, the pieces arriving paired with those leaving.
    let mut links = links;
    links.sort_by(|x, y| (x.el.face, &x.el.a, &x.el.b).cmp(&(y.el.face, &y.el.a, &y.el.b)));
    let mut outs: BTreeMap<&Node, Vec<usize>> = BTreeMap::new();
    let mut ins: BTreeMap<&Node, Vec<usize>> = BTreeMap::new();
    for (k, l) in links.iter().enumerate() {
        outs.entry(&l.from).or_default().push(k);
        ins.entry(&l.to).or_default().push(k);
    }
    if outs.len() != ins.len()
        || outs
            .iter()
            .any(|(n, o)| ins.get(n).map(Vec::len) != Some(o.len()))
    {
        return fail(RimError::Open);
    }
    let mut next = vec![usize::MAX; links.len()];
    for (n, arriving) in &ins {
        let mut free: Vec<usize> = outs[n].clone();
        for &i in arriving {
            units(ledger, free.len() as u64)?;
            let d_in = sub2(&links[i].el.b, &links[i].el.a);
            let back = [-&d_in[0], -&d_in[1]];
            let best = (0..free.len())
                .min_by(|&x, &y| {
                    let (lx, ly) = (&links[free[x]], &links[free[y]]);
                    let same = |l: &Link| l.el.face != links[i].el.face;
                    same(lx).cmp(&same(ly)).then_with(|| {
                        if lx.el.face == links[i].el.face && ly.el.face == links[i].el.face {
                            let t =
                                turn(&back, &sub2(&lx.el.b, &lx.el.a), &sub2(&ly.el.b, &ly.el.a));
                            // The rightmost turn seen from outside.
                            if flipped.contains(&lx.el.face) {
                                t.reverse()
                            } else {
                                t
                            }
                        } else {
                            Ordering::Equal
                        }
                    })
                })
                .expect("balanced");
            next[i] = free.remove(best);
        }
    }
    let mut seen = vec![false; links.len()];
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..links.len() {
        if seen[start] {
            continue;
        }
        let mut cyc = Vec::new();
        let mut k = start;
        while !seen[k] {
            seen[k] = true;
            cyc.push(k);
            k = next[k];
        }
        cycles.push(cyc);
    }

    // Merge straight runs within a loop, split at tile lines and at
    // changes of the target faces beside.
    let mut out_loops: Vec<Vec<Piece>> = Vec::new();
    for cyc in cycles {
        units(ledger, cyc.len() as u64)?;
        let joins = |x: &Link, y: &Link| {
            x.el.face == y.el.face && x.el.along == y.el.along && !x.hard[1] && !y.hard[0] && {
                let (dx, dy) = (sub2(&x.el.b, &x.el.a), sub2(&y.el.b, &y.el.a));
                cross2(&dx, &dy) == 0u32 && dot2(&dx, &dy) > 0u32
            }
        };
        let n = cyc.len();
        let start = (0..n)
            .find(|&k| !joins(&links[cyc[(k + n - 1) % n]], &links[cyc[k]]))
            .unwrap_or(0);
        let mut merged: Vec<El> = Vec::new();
        for s in 0..n {
            let l = &links[cyc[(start + s) % n]];
            match merged.last_mut() {
                Some(last) if s > 0 && joins(&links[cyc[(start + s + n - 1) % n]], l) => {
                    last.b = l.el.b.clone();
                    if let (Some(p), Some(q)) = (&mut last.params, &l.el.params) {
                        p[1] = q[1].clone();
                    }
                }
                _ => merged.push(l.el.clone()),
            }
        }
        let mut pieces = Vec::new();
        for e in merged {
            let job = jobs
                .iter()
                .find(|j| j.face.face == e.face)
                .expect("a piece's face");
            for part in split_tiles(&e) {
                pieces.extend(split_targets(part, &job.boxes, &job.face).map_err(Stop::Failed)?);
            }
        }
        // Start at the least piece.
        let first = (0..pieces.len())
            .min_by(|&x, &y| {
                (pieces[x].face, &pieces[x].ends).cmp(&(pieces[y].face, &pieces[y].ends))
            })
            .unwrap_or(0);
        pieces.rotate_left(first);
        out_loops.push(pieces);
    }
    out_loops.sort_by(|x, y| (x[0].face, &x[0].ends).cmp(&(y[0].face, &y[0].ends)));
    let mut pieces = Vec::new();
    let mut loops = Vec::new();
    for l in out_loops {
        let base = pieces.len() as u32;
        loops.push((base..base + l.len() as u32).collect());
        pieces.extend(l);
    }
    Ok((pieces, loops))
}

/// Whether `a` comes before `b` turning counter-clockwise from `back`
/// (the reversed arrival): the rightmost turn first.
fn turn(back: &P2, a: &P2, b: &P2) -> Ordering {
    let half = |e: &P2| {
        let c = cross2(back, e);
        // The upper half `(0, π]`, then the lower `(π, 2π)`.
        if c > 0u32 || (c == 0u32 && dot2(back, e) < 0u32) {
            0
        } else {
            1
        }
    };
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

/// Removes the parts of own-edge pieces both sides of the edge cover.
///
/// A side is a face and the way its pieces run along the edge's
/// parameter: the region lies on the face's side of the edge, so two
/// faces sharing it cover its two sides whichever way their UVs run (a
/// face flipped against its surface runs its pieces the same way as an
/// unflipped neighbour does), and one face covering both ways is a slit.
fn cancel(links: Vec<Link>, ends: &EdgeEnds) -> Vec<Link> {
    // Per edge and side, the covered intervals.
    type Sides = BTreeMap<(FaceId, bool), Vec<(Q, Q)>>;
    let mut cover: BTreeMap<EdgeId, Sides> = BTreeMap::new();
    for l in &links {
        if let (Along::Own(e), Some([p, q])) = (l.el.along, &l.el.params) {
            let up = p < q;
            let (lo, hi) = if up { (p, q) } else { (q, p) };
            cover
                .entry(e)
                .or_default()
                .entry((l.el.face, up))
                .or_default()
                .push((lo.clone(), hi.clone()));
        }
    }
    let both: BTreeMap<EdgeId, Vec<(Q, Q)>> = cover
        .into_iter()
        .map(|(e, sides)| {
            let sides: Vec<Vec<(Q, Q)>> = sides.into_values().map(union).collect();
            let mut twice = Vec::new();
            for i in 0..sides.len() {
                for j in i + 1..sides.len() {
                    twice.extend(intersect(&sides[i], &sides[j]));
                }
            }
            (e, union(twice))
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let mut out = Vec::new();
    for l in links {
        let (Along::Own(e), Some([p, q])) = (l.el.along, l.el.params.clone()) else {
            out.push(l);
            continue;
        };
        let Some(cut) = both.get(&e) else {
            out.push(l);
            continue;
        };
        let up = p < q;
        let (lo, hi) = if up { (&p, &q) } else { (&q, &p) };
        let mut keep = subtract(lo, hi, cut);
        if !up {
            keep.reverse();
            for k in &mut keep {
                std::mem::swap(&mut k.0, &mut k.1);
            }
        }
        for (x, y) in keep {
            let at = |t: &Q| lerp(&l.el.a, &l.el.b, &((t - &p) / (&q - &p)));
            let name = |t: &Q, old: &Node| {
                if *t == p {
                    l.from.clone()
                } else if *t == q {
                    l.to.clone()
                } else {
                    match ends.get(&e) {
                        Some(((t0, v0), _)) if t0 == t => Node::Vert(*v0),
                        Some((_, (t1, v1))) if t1 == t => Node::Vert(*v1),
                        _ => {
                            let _ = old;
                            Node::On(e, t.clone())
                        }
                    }
                }
            };
            out.push(Link {
                from: name(&x, &l.from),
                to: name(&y, &l.to),
                hard: [x != p || l.hard[0], y != q || l.hard[1]],
                el: El {
                    face: l.el.face,
                    a: at(&x),
                    b: at(&y),
                    along: l.el.along,
                    params: Some([x, y]),
                },
            });
        }
    }
    out
}

/// The union of intervals, as sorted disjoint intervals.
fn union(mut v: Vec<(Q, Q)>) -> Vec<(Q, Q)> {
    v.sort();
    let mut out: Vec<(Q, Q)> = Vec::new();
    for (lo, hi) in v {
        match out.last_mut() {
            Some(last) if lo <= last.1 => {
                if hi > last.1 {
                    last.1 = hi;
                }
            }
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// The intersection of two sorted disjoint interval lists, positive
/// lengths only.
fn intersect(a: &[(Q, Q)], b: &[(Q, Q)]) -> Vec<(Q, Q)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let lo = if a[i].0 > b[j].0 { &a[i].0 } else { &b[j].0 };
        let hi = if a[i].1 < b[j].1 { &a[i].1 } else { &b[j].1 };
        if lo < hi {
            out.push((lo.clone(), hi.clone()));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// `[lo, hi]` less the sorted disjoint `cut`, positive lengths only.
fn subtract(lo: &Q, hi: &Q, cut: &[(Q, Q)]) -> Vec<(Q, Q)> {
    let mut out = Vec::new();
    let mut at = lo.clone();
    for (a, b) in cut {
        if *b <= at || *a >= *hi {
            continue;
        }
        if *a > at {
            out.push((at.clone(), a.clone()));
        }
        if *b > at {
            at = b.clone();
        }
    }
    if at < *hi {
        out.push((at, hi.clone()));
    }
    out
}

/// `e` split where it crosses a tile line.
fn split_tiles(e: &El) -> Vec<El> {
    let mut ss: Vec<Q> = vec![Q::from(0u32), Q::from(1u32)];
    for k in 0..2 {
        let (a, b) = (&e.a[k], &e.b[k]);
        if a == b {
            continue;
        }
        let (lo, hi) = minmax(a, b);
        let mut n = floor(lo) + Integer::from(1u32);
        loop {
            let x = Q::from(n.clone());
            if x >= *hi {
                break;
            }
            ss.push((x - a) / (b - a));
            n += Integer::from(1u32);
        }
    }
    split_at(e, ss)
}

/// `e` cut at the parameters `ss` (with 0 and 1).
fn split_at(e: &El, mut ss: Vec<Q>) -> Vec<El> {
    ss.sort();
    ss.dedup();
    ss.windows(2)
        .map(|w| El {
            face: e.face,
            a: lerp(&e.a, &e.b, &w[0]),
            b: lerp(&e.a, &e.b, &w[1]),
            along: e.along,
            params: e
                .params
                .as_ref()
                .map(|[p, q]| [p + &w[0] * (q - p), p + &w[1] * (q - p)]),
        })
        .collect()
}

fn floor(x: &Q) -> Integer {
    x.floor()
}

/// The parameter interval of the segment `a → b` inside the closed box,
/// if of positive length.
fn clip(a: &P2, b: &P2, bx: &[[Q; 2]; 2]) -> Option<(Q, Q)> {
    let mut lo = Q::from(0u32);
    let mut hi = Q::from(1u32);
    for k in 0..2 {
        let d = &b[k] - &a[k];
        if d == 0u32 {
            if a[k] < bx[k][0] || a[k] > bx[k][1] {
                return None;
            }
            continue;
        }
        let s0 = (&bx[k][0] - &a[k]) / &d;
        let s1 = (&bx[k][1] - &a[k]) / &d;
        let (s0, s1) = if s0 <= s1 { (s0, s1) } else { (s1, s0) };
        if s0 > lo {
            lo = s0;
        }
        if s1 < hi {
            hi = s1;
        }
    }
    (lo < hi).then_some((lo, hi))
}

/// `e` (in one tile) split where the target faces beside it change, as
/// pieces.
fn split_targets(
    e: El,
    boxes: &[([[Q; 2]; 2], BTreeSet<FaceId>)],
    face: &SnappedFace<'_>,
) -> Result<Vec<Piece>, RimError> {
    let meets: Vec<((Q, Q), &BTreeSet<FaceId>)> = boxes
        .iter()
        .filter_map(|(b, ts)| clip(&e.a, &e.b, b).map(|c| (c, ts)))
        .collect();
    let mut ss: Vec<Q> = vec![Q::from(0u32), Q::from(1u32)];
    for ((lo, hi), _) in &meets {
        ss.push(lo.clone());
        ss.push(hi.clone());
    }
    ss.sort();
    ss.dedup();
    let set_on = |lo: &Q, hi: &Q| -> BTreeSet<FaceId> {
        meets
            .iter()
            .filter(|((a, b), _)| a <= lo && hi <= b)
            .flat_map(|(_, ts)| ts.iter().copied())
            .collect()
    };
    let mut cuts: Vec<Q> = vec![Q::from(0u32), Q::from(1u32)];
    let mut prev: Option<BTreeSet<FaceId>> = None;
    for w in ss.windows(2) {
        let s = set_on(&w[0], &w[1]);
        if prev.as_ref().is_some_and(|p| *p != s) {
            cuts.push(w[0].clone());
        }
        prev = Some(s);
    }
    let parts = split_at(&e, cuts);
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        let m = mid(&p.a, &p.b);
        let tile = tile_of(face, &m).ok_or(RimError::Face(p.face))?;
        let targets: BTreeSet<FaceId> = boxes
            .iter()
            .filter(|(b, _)| clip(&p.a, &p.b, b).is_some())
            .flat_map(|(_, ts)| ts.iter().copied())
            .collect();
        out.push(Piece {
            face: p.face,
            tile,
            ends: [p.a, p.b],
            along: p.along,
            params: p.params,
            targets: targets.into_iter().collect(),
        });
    }
    Ok(out)
}

/// The tile of `face` holding the point `m` (of a piece in one tile): its
/// floor, or the one below where a coordinate is a tile line.
fn tile_of(face: &SnappedFace<'_>, m: &P2) -> Option<[i64; 2]> {
    let f = |x: &Q| -> Option<(i64, bool)> {
        let n = floor(x);
        let i = i64::try_from(&n).ok()?;
        Some((i, *x == n))
    };
    let (iu, on_u) = f(&m[0])?;
    let (iv, on_v) = f(&m[1])?;
    let us: &[i64] = if on_u { &[iu, iu - 1] } else { &[iu] };
    let vs: &[i64] = if on_v { &[iv, iv - 1] } else { &[iv] };
    us.iter()
        .flat_map(|&i| vs.iter().map(move |&j| [i, j]))
        .find(|t| face.tiles.iter().any(|(k, _)| k == t))
}

fn patch_of<'a>(face: &SnappedFace<'a>, tile: [i64; 2]) -> Option<&'a HomPatch<Rational>> {
    face.tiles.iter().find(|(k, _)| *k == tile).map(|(_, p)| p)
}

/// Piece `p`'s curve on `S`, homogeneous in its parameter (`(X, Y, Z, W)`,
/// `W > 0`), and the gap against `implicit` along it (module docs), as
/// the rim certifies them; `None` if its face or tile isn't described or
/// the patch doesn't fit.
pub(super) fn piece_forms(
    input: &RimInput<'_>,
    p: &Piece,
    implicit: &Implicit,
) -> Option<([Poly<Q>; 4], Poly<Q>)> {
    let face = input.faces.iter().find(|f| f.face == p.face)?;
    let patch = patch_of(face, p.tile)?;
    let origin = [Q::from(p.tile[0]), Q::from(p.tile[1])];
    let (a, b) = (sub2(&p.ends[0], &origin), sub2(&p.ends[1], &origin));
    let curve = curve_on(patch, &a, &b)?;
    if !curve[3].coefs().iter().all(|w| *w > 0u32) {
        return None;
    }
    let gap = gap_form(implicit, &curve)?;
    Some((curve, gap))
}

// ---------------------------------------------------------------------
// The gap along a piece.

/// A piece's gap against one surface, to compute.
struct GapJob<'a> {
    piece: u32,
    surface: u32,
    implicit: Implicit,
    targets: Vec<FaceId>,
    patch: &'a HomPatch<Rational>,
    ends: [P2; 2],
    tile: [i64; 2],
    face: FaceId,
    beta: f64,
}

/// The piece's curve in homogeneous coordinates and the gap along it.
#[derive(Clone, Debug)]
struct Forms {
    curve: [Poly<Q>; 4],
    gap: Poly<Q>,
}

impl Forms {
    fn halves(&self) -> [Forms; 2] {
        let c = self.curve.each_ref().map(Poly::split_half);
        let g = self.gap.split_half();
        let side = |k: usize| Forms {
            curve: [
                c[0][k].clone(),
                c[1][k].clone(),
                c[2][k].clone(),
                c[3][k].clone(),
            ],
            gap: g[k].clone(),
        };
        [side(0), side(1)]
    }

    /// The control points' box, `[lo, hi]` per coordinate.
    fn bbox(&self) -> [[Q; 3]; 2] {
        let w = self.curve[3].coefs();
        let mut lo: [Option<Q>; 3] = [None, None, None];
        let mut hi: [Option<Q>; 3] = [None, None, None];
        for (i, wi) in w.iter().enumerate() {
            for k in 0..3 {
                let x = &self.curve[k].coefs()[i] / wi;
                if lo[k].as_ref().is_none_or(|l| x < *l) {
                    lo[k] = Some(x.clone());
                }
                if hi[k].as_ref().is_none_or(|h| x > *h) {
                    hi[k] = Some(x);
                }
            }
        }
        let get = |v: [Option<Q>; 3]| v.map(|x| x.expect("a control point"));
        [get(lo), get(hi)]
    }
}

/// A cell of a piece's gap: its dyadic span (depth and index), its sign,
/// and for a cluster cell its forms and box.
#[derive(Clone, Debug)]
struct Cell {
    depth: u32,
    index: u64,
    gap: Gap,
    forms: Option<Box<(Forms, [[Q; 3]; 2])>>,
}

impl Cell {
    fn span(&self) -> [Dyadic; 2] {
        let d = |j: u64| Dyadic::new(j, self.depth).expect("a cell within [0, 1]");
        [d(self.index), d(self.index + 1)]
    }
}

/// A piece's gap against one surface: its cells, and the whole forms.
#[derive(Clone, Debug)]
struct GapData {
    cells: Vec<Cell>,
    forms: Forms,
    zero: bool,
}

/// `Π` of linear Bernstein forms, `C(m, i)·u^i·(1 − u)^{m−i}` along the
/// line `x(t)` from `a` to `b` (each a constant if `a == b`).
fn basis(m: usize, a: &Q, b: &Q) -> Vec<Poly<Q>> {
    let one = Q::from(1u32);
    let lin = |x: &Q, y: &Q| {
        if x == y {
            Poly::new([x.clone()]).expect("a constant")
        } else {
            Poly::new([x.clone(), y.clone()]).expect("a line")
        }
    };
    let u = lin(a, b);
    let v = lin(&(&one - a), &(&one - b));
    (0..=m)
        .map(|i| {
            let mut p = Poly::new([Q::from(binom(m, i))]).expect("a constant");
            for _ in 0..i {
                p = p.mul(&u).expect("a low degree");
            }
            for _ in i..m {
                p = p.mul(&v).expect("a low degree");
            }
            p
        })
        .collect()
}

fn binom(m: usize, i: usize) -> u64 {
    let mut c = 1u64;
    for k in 0..i {
        c = c * (m - k) as u64 / (k + 1) as u64;
    }
    c
}

/// The piece's curve on `patch` from local `a` to `b`, homogeneous.
fn curve_on(patch: &HomPatch<Rational>, a: &P2, b: &P2) -> Option<[Poly<Q>; 4]> {
    let (m, n) = patch.degrees();
    debug_assert!(m > 0 && n > 0, "a patch's degrees");
    let bu = basis(m, &a[0], &b[0]);
    let bv = basis(n, &a[1], &b[1]);
    let t = patch.tensors();
    let mut out: [Option<Poly<Q>>; 4] = [None, None, None, None];
    for (i, pu) in bu.iter().enumerate() {
        for (j, pv) in bv.iter().enumerate() {
            let b = pu.mul(pv)?;
            for (k, o) in out.iter_mut().enumerate() {
                let term = b.scale(t[k].get(i, j));
                *o = Some(match o.take() {
                    None => term,
                    Some(p) => p.add(&term),
                });
            }
        }
    }
    let [x, y, z, w] = out;
    Some([x?, y?, z?, w?])
}

/// The implicit along the curve (module docs), positive outside.
fn gap_form(imp: &Implicit, c: &[Poly<Q>; 4]) -> Option<Poly<Q>> {
    let q = |x: f64| exact(x);
    match imp {
        Implicit::Plane { normal, offset } => {
            let n = [q(normal[0])?, q(normal[1])?, q(normal[2])?];
            let d = q(*offset)?;
            Some(
                c[0].scale(&n[0])
                    .add(&c[1].scale(&n[1]))
                    .add(&c[2].scale(&n[2]))
                    .sub(&c[3].scale(&d)),
            )
        }
        Implicit::Cylinder { quadric, outward } => {
            let f = quadric.foot();
            let a = quadric.axis();
            let a = [q(a[0])?, q(a[1])?, q(a[2])?];
            let rho = q(quadric.radius())?;
            let r: Vec<Poly<Q>> = (0..3)
                .map(|k| Some(c[k].sub(&c[3].scale(&q(f[k])?))))
                .collect::<Option<_>>()?;
            let aa = &a[0] * &a[0] + &a[1] * &a[1] + &a[2] * &a[2];
            let ar = r[0]
                .scale(&a[0])
                .add(&r[1].scale(&a[1]))
                .add(&r[2].scale(&a[2]));
            let rr = r[0]
                .mul(&r[0])?
                .add(&r[1].mul(&r[1])?)
                .add(&r[2].mul(&r[2])?);
            let ww = c[3].mul(&c[3])?;
            let g = rr
                .scale(&aa)
                .sub(&ar.mul(&ar)?)
                .sub(&ww.scale(&(&rho * &rho * &aa)));
            Some(if *outward { g } else { g.neg() })
        }
    }
}

/// The strict sign of every coefficient, if one.
fn one_sign(p: &Poly<Q>) -> Option<Gap> {
    let c = p.coefs();
    if c.iter().all(|x| *x > 0u32) {
        Some(Gap::Positive)
    } else if c.iter().all(|x| *x < 0u32) {
        Some(Gap::Negative)
    } else {
        None
    }
}

fn diag2(b: &[[Q; 3]; 2]) -> Q {
    (0..3)
        .map(|k| {
            let d = &b[1][k] - &b[0][k];
            &d * &d
        })
        .fold(Q::from(0u32), |s, x| s + x)
}

fn beta2(beta: f64) -> Q {
    let b = exact(beta).expect("a checked band");
    &b * &b
}

/// A cell of `forms` at `(depth, index)`: one-signed, or a cluster cell
/// if within `beta2`, else `None` (split it).
fn classify(forms: Forms, depth: u32, index: u64, beta2: &Q) -> Option<Cell> {
    if let Some(gap) = one_sign(&forms.gap) {
        return Some(Cell {
            depth,
            index,
            gap,
            forms: None,
        });
    }
    let b = forms.bbox();
    (diag2(&b) <= *beta2).then(|| Cell {
        depth,
        index,
        gap: Gap::Cluster,
        forms: Some(Box::new((forms, b))),
    })
}

/// The cells of the gap of `job` (module docs).
fn piece_gap(job: &GapJob<'_>, ledger: &mut Ledger<'_>) -> Result<GapData, Stop<RimError>> {
    units(ledger, 1)?;
    let origin = [Q::from(job.tile[0]), Q::from(job.tile[1])];
    let local = |p: &P2| sub2(p, &origin);
    let (a, b) = (local(&job.ends[0]), local(&job.ends[1]));
    let Some(curve) = curve_on(job.patch, &a, &b) else {
        return fail(RimError::Patch(job.face));
    };
    if !curve[3].coefs().iter().all(|w| *w > 0u32) {
        return fail(RimError::Patch(job.face));
    }
    let Some(gap) = gap_form(&job.implicit, &curve) else {
        return fail(RimError::Patch(job.face));
    };
    let forms = Forms { curve, gap };
    if forms.gap.coefs().iter().all(|x| *x == 0u32) {
        return Ok(GapData {
            cells: vec![Cell {
                depth: 0,
                index: 0,
                gap: Gap::Zero,
                forms: None,
            }],
            forms,
            zero: true,
        });
    }
    let b2 = beta2(job.beta);
    let mut cells = Vec::new();
    let mut stack = vec![(0u32, 0u64, forms.clone())];
    while let Some((depth, index, f)) = stack.pop() {
        units(ledger, 1)?;
        if let Some(c) = classify(f.clone(), depth, index, &b2) {
            cells.push(c);
            continue;
        }
        if depth >= MAX_SUBDIV_DEPTH {
            return Err(Stop::Cap(Cap::SubdivisionDepth));
        }
        let [l, r] = f.halves();
        stack.push((depth + 1, 2 * index + 1, r));
        stack.push((depth + 1, 2 * index, l));
    }
    Ok(GapData {
        cells,
        forms,
        zero: false,
    })
}

// ---------------------------------------------------------------------
// Root clusters along a loop.

/// A sequence of gaps along a loop against one surface (module docs).
#[derive(Clone, Debug)]
struct Seq {
    gaps: Vec<usize>,
    cyclic: bool,
}

/// One loop's sequences.
struct LoopJob {
    seqs: Vec<Seq>,
}

/// The sequences of loop `l` against surface `s`, pushed to `out`.
fn sequences(
    l: &[u32],
    s: u32,
    gap_at: &BTreeMap<(u32, u32), usize>,
    gaps: &[GapData],
    out: &mut Vec<Seq>,
) {
    let at: Vec<Option<usize>> = l.iter().map(|&p| gap_at.get(&(p, s)).copied()).collect();
    let usable = |g: &Option<usize>| g.is_some_and(|g| !gaps[g].zero);
    let n = at.len();
    if at.iter().all(usable) {
        out.push(Seq {
            gaps: at.iter().map(|g| g.expect("all usable")).collect(),
            cyclic: true,
        });
        return;
    }
    // Start after a break, so runs don't wrap.
    let start = (0..n).find(|&k| !usable(&at[k])).expect("a break");
    let mut cur: Vec<usize> = Vec::new();
    for k in 1..=n {
        let g = &at[(start + k) % n];
        if usable(g) {
            cur.push(g.expect("usable"));
        } else if !cur.is_empty() {
            out.push(Seq {
                gaps: std::mem::take(&mut cur),
                cyclic: false,
            });
        }
    }
    if !cur.is_empty() {
        out.push(Seq {
            gaps: cur,
            cyclic: false,
        });
    }
}

/// What a loop's clusters give: the gaps refined, the clusters and the
/// branch vertices.
struct LoopOut {
    refined: Vec<(usize, Vec<Cell>)>,
    clusters: Vec<RootCluster>,
    branches: Vec<Branch>,
}

/// A run of cluster cells: positions in a sequence's flattened cells,
/// `first..first + len` (cyclic).
struct RunAt {
    first: usize,
    len: usize,
}

/// The maximal runs of cluster cells in `cells` (cyclic if `cyclic`).
fn runs(cells: &[Gap], cyclic: bool) -> Vec<RunAt> {
    let n = cells.len();
    let is = |k: usize| cells[k] == Gap::Cluster;
    if n == 0 {
        return Vec::new();
    }
    if (0..n).all(is) {
        return vec![RunAt { first: 0, len: n }];
    }
    let start = if cyclic {
        (0..n).find(|&k| !is(k)).expect("a signed cell")
    } else {
        0
    };
    let mut out = Vec::new();
    let mut k = 0;
    while k < n {
        let p = (start + k) % n;
        if is(p) {
            let mut len = 0;
            while k + len < n && is((start + k + len) % n) {
                len += 1;
            }
            out.push(RunAt { first: p, len });
            k += len;
        } else {
            k += 1;
        }
    }
    out
}

/// The clusters of `job`'s sequences (module docs).
fn loop_clusters(
    job: &LoopJob,
    gaps: &[GapData],
    jobs: &[GapJob<'_>],
    ledger: &mut Ledger<'_>,
) -> Result<LoopOut, Stop<RimError>> {
    let mut local: BTreeMap<usize, Vec<Cell>> = BTreeMap::new();
    let mut clusters = Vec::new();
    let mut branches = Vec::new();
    for seq in &job.seqs {
        for &g in &seq.gaps {
            local.entry(g).or_insert_with(|| gaps[g].cells.clone());
        }
        // Refine runs wider than their band.
        loop {
            let flat: Vec<(usize, usize)> = seq
                .gaps
                .iter()
                .flat_map(|&g| (0..local[&g].len()).map(move |c| (g, c)))
                .collect();
            let signs: Vec<Gap> = flat.iter().map(|&(g, c)| local[&g][c].gap).collect();
            let n = flat.len();
            let mut split: BTreeSet<(usize, usize)> = BTreeSet::new();
            for r in runs(&signs, seq.cyclic) {
                let cells: Vec<(usize, usize)> =
                    (0..r.len).map(|k| flat[(r.first + k) % n]).collect();
                let mut lo: Option<[Q; 3]> = None;
                let mut hi: Option<[Q; 3]> = None;
                let mut b2: Option<Q> = None;
                for &(g, c) in &cells {
                    let (_, bx) = local[&g][c].forms.as_deref().expect("a cluster cell");
                    lo = Some(match lo {
                        None => bx[0].clone(),
                        Some(l) => [0, 1, 2].map(|k| {
                            if bx[0][k] < l[k] {
                                bx[0][k].clone()
                            } else {
                                l[k].clone()
                            }
                        }),
                    });
                    hi = Some(match hi {
                        None => bx[1].clone(),
                        Some(h) => [0, 1, 2].map(|k| {
                            if bx[1][k] > h[k] {
                                bx[1][k].clone()
                            } else {
                                h[k].clone()
                            }
                        }),
                    });
                    let b = beta2(jobs[g].beta);
                    if b2.as_ref().is_none_or(|x| b < *x) {
                        b2 = Some(b);
                    }
                }
                let bx = [lo.expect("a run"), hi.expect("a run")];
                if diag2(&bx) > b2.expect("a run") {
                    split.extend(cells);
                }
            }
            if split.is_empty() {
                break;
            }
            for (g, cells) in local.iter_mut() {
                if !split.iter().any(|(h, _)| h == g) {
                    continue;
                }
                let b2 = beta2(jobs[*g].beta);
                let mut out = Vec::with_capacity(cells.len() + 4);
                for (c, cell) in cells.drain(..).enumerate() {
                    if !split.contains(&(*g, c)) {
                        out.push(cell);
                        continue;
                    }
                    units(ledger, 2)?;
                    if cell.depth >= MAX_SUBDIV_DEPTH {
                        return Err(Stop::Cap(Cap::SubdivisionDepth));
                    }
                    let (forms, _) = *cell.forms.expect("a cluster cell");
                    let [l, r] = forms.halves();
                    for (k, f) in [l, r].into_iter().enumerate() {
                        let (d, i) = (cell.depth + 1, 2 * cell.index + k as u64);
                        // Within the band already: one-signed or a cluster.
                        let c = classify(f.clone(), d, i, &b2).unwrap_or_else(|| {
                            let b = f.bbox();
                            Cell {
                                depth: d,
                                index: i,
                                gap: Gap::Cluster,
                                forms: Some(Box::new((f, b))),
                            }
                        });
                        out.push(c);
                    }
                }
                *cells = out;
            }
        }
        // The clusters and their nets.
        let flat: Vec<(usize, usize)> = seq
            .gaps
            .iter()
            .flat_map(|&g| (0..local[&g].len()).map(move |c| (g, c)))
            .collect();
        let signs: Vec<Gap> = flat.iter().map(|&(g, c)| local[&g][c].gap).collect();
        let n = flat.len();
        for r in runs(&signs, seq.cyclic) {
            units(ledger, 1)?;
            let first = flat[r.first];
            let last = flat[(r.first + r.len - 1) % n];
            let before = if seq.cyclic {
                (r.len < n).then(|| signs[(r.first + n - 1) % n])
            } else {
                (r.first > 0).then(|| signs[r.first - 1])
            };
            let after = if seq.cyclic {
                (r.len < n).then(|| signs[(r.first + r.len) % n])
            } else {
                (r.first + r.len < n).then(|| signs[r.first + r.len])
            };
            let net = match (before, after) {
                (Some(a), Some(b)) if a == b => Some(0),
                (Some(Gap::Negative), Some(Gap::Positive)) => Some(1),
                (Some(Gap::Positive), Some(Gap::Negative)) => Some(-1),
                _ => None,
            };
            let span_of = |(g, c): (usize, usize)| local[&g][c].span();
            let cluster = RootCluster {
                targets: jobs[first.0].targets.clone(),
                from: (jobs[first.0].piece, span_of(first)[0]),
                to: (jobs[last.0].piece, span_of(last)[1]),
                net,
            };
            if let Some(d) = net.filter(|&d| d != 0) {
                units(ledger, 3)?;
                branches.push(place(&cluster, d > 0, first, last, &local, gaps, jobs)?);
            }
            clusters.push(cluster);
        }
    }
    Ok(LoopOut {
        refined: local.into_iter().collect(),
        clusters,
        branches,
    })
}

/// A candidate point's distance to the target surface, exactly.
#[derive(Clone, Debug)]
enum Residual {
    /// The squared distance to a plane.
    Plane(Q),
    /// The squared distance `q` from a cylinder's axis, and its radius:
    /// the distance is `|√q − ρ|`.
    Cylinder { q: Q, rho: Q },
}

impl Residual {
    fn of(imp: &Implicit, p: &[Q; 3]) -> Option<Residual> {
        match imp {
            Implicit::Plane { normal, offset } => {
                let n = [exact(normal[0])?, exact(normal[1])?, exact(normal[2])?];
                let d = &n[0] * &p[0] + &n[1] * &p[1] + &n[2] * &p[2] - exact(*offset)?;
                let nn = &n[0] * &n[0] + &n[1] * &n[1] + &n[2] * &n[2];
                (nn != 0u32).then(|| Residual::Plane(&d * &d / nn))
            }
            Implicit::Cylinder { quadric, .. } => {
                let f = quadric.foot();
                let a = quadric.axis();
                let a = [exact(a[0])?, exact(a[1])?, exact(a[2])?];
                let r: Vec<Q> = (0..3)
                    .map(|k| Some(&p[k] - exact(f[k])?))
                    .collect::<Option<_>>()?;
                let aa = &a[0] * &a[0] + &a[1] * &a[1] + &a[2] * &a[2];
                let ar = &a[0] * &r[0] + &a[1] * &r[1] + &a[2] * &r[2];
                let rr = &r[0] * &r[0] + &r[1] * &r[1] + &r[2] * &r[2];
                Some(Residual::Cylinder {
                    q: (rr * &aa - &ar * &ar) / aa,
                    rho: exact(quadric.radius())?,
                })
            }
        }
    }

    /// Whether the distance is at most `beta`.
    fn within(&self, beta: &Q) -> bool {
        match self {
            Residual::Plane(d2) => *d2 <= beta * beta,
            Residual::Cylinder { q, rho } => {
                let hi = rho + beta;
                let lo = rho - beta;
                *q <= &hi * &hi && (lo <= 0u32 || *q >= &lo * &lo)
            }
        }
    }

    /// The order of two residuals of one surface, exactly.
    fn cmp(&self, o: &Residual) -> Ordering {
        match (self, o) {
            (Residual::Plane(a), Residual::Plane(b)) => a.cmp(b),
            (Residual::Cylinder { q: q1, rho }, Residual::Cylinder { q: q2, .. }) => {
                let r2 = rho * rho;
                match (q1 >= &r2, q2 >= &r2) {
                    (true, true) => q1.cmp(q2),
                    (false, false) => q2.cmp(q1),
                    (true, false) => cyl_mixed(q1, q2, rho),
                    (false, true) => cyl_mixed(q2, q1, rho).reverse(),
                }
            }
            _ => Ordering::Equal,
        }
    }

    /// An upper bound of the distance in `f64`.
    fn upper(&self) -> f64 {
        let hi = |x: &Q| f64::rounding_from(x, Ceiling).0;
        let lo = |x: &Q| f64::rounding_from(x, Down).0;
        match self {
            Residual::Plane(d2) => up(hi(d2).sqrt()),
            Residual::Cylinder { q, rho } => {
                let (s_lo, s_hi) = (down(lo(q).sqrt()), up(hi(q).sqrt()));
                let (r_lo, r_hi) = (lo(rho), hi(rho));
                let a = up(s_hi - r_lo);
                let b = up(r_hi - s_lo);
                if a > b { a } else { b }
            }
        }
    }
}

/// `√q1 − ρ` against `ρ − √q2` (`q1 ≥ ρ² > q2`), exactly.
fn cyl_mixed(q1: &Q, q2: &Q, rho: &Q) -> Ordering {
    // √q1 + √q2 against 2ρ, squared: 2√(q1·q2) against 4ρ² − q1 − q2.
    let r = Q::from(4u32) * rho * rho - q1 - q2;
    if r < 0u32 {
        return Ordering::Greater;
    }
    (Q::from(4u32) * q1 * q2).cmp(&(&r * &r))
}

/// The branch vertex of `cluster` (module docs): of its ends and a Newton
/// point, the least residual, certified within the band.
fn place(
    cluster: &RootCluster,
    rising: bool,
    first: (usize, usize),
    last: (usize, usize),
    local: &BTreeMap<usize, Vec<Cell>>,
    gaps: &[GapData],
    jobs: &[GapJob<'_>],
) -> Result<Branch, Stop<RimError>> {
    let mut cands: Vec<(usize, Q)> = vec![
        (first.0, local[&first.0][first.1].span()[0].rational()),
        (last.0, local[&last.0][last.1].span()[1].rational()),
    ];
    if first.0 == last.0 {
        let (lo, hi) = (cands[0].1.clone(), cands[1].1.clone());
        if let Some(t) = newton(&gaps[first.0].forms.gap, &lo, &hi) {
            cands.push((first.0, t));
        }
    }
    let mut best: Option<(usize, Q, Residual)> = None;
    for (g, t) in cands {
        let c = &gaps[g].forms.curve;
        let w = c[3].eval(&t);
        let p = [0, 1, 2].map(|k| c[k].eval(&t) / &w);
        let Some(r) = Residual::of(&jobs[g].implicit, &p) else {
            return fail(RimError::Patch(jobs[g].face));
        };
        if best.as_ref().is_none_or(|b| r.cmp(&b.2) == Ordering::Less) {
            best = Some((g, t, r));
        }
    }
    let (g, t, r) = best.expect("two candidates");
    let beta = exact(jobs[g].beta).expect("a checked band");
    if !r.within(&beta) {
        return fail(RimError::Inconsistent);
    }
    let job = &jobs[g];
    Ok(Branch {
        piece: job.piece,
        uv: lerp(&job.ends[0], &job.ends[1], &t),
        at: t,
        targets: cluster.targets.clone(),
        rising,
        residual: r.upper(),
    })
}

/// A root of `g` in `(lo, hi)` by Newton's method in `f64` from the
/// middle, if it stays there.
fn newton(g: &Poly<Q>, lo: &Q, hi: &Q) -> Option<Q> {
    let c: Vec<f64> = g
        .coefs()
        .iter()
        .map(|x| f64::rounding_from(x, Nearest).0)
        .collect();
    let d: Vec<f64> = c
        .windows(2)
        .map(|w| (w[1] - w[0]) * (c.len() - 1) as f64)
        .collect();
    let eval = |c: &[f64], t: f64| {
        let mut w = c.to_vec();
        for k in 1..w.len() {
            for i in 0..w.len() - k {
                w[i] = (1.0 - t) * w[i] + t * w[i + 1];
            }
        }
        w.first().copied().unwrap_or(0.0)
    };
    let (a, b) = (
        f64::rounding_from(lo, Nearest).0,
        f64::rounding_from(hi, Nearest).0,
    );
    let mut t = 0.5 * a + 0.5 * b;
    for _ in 0..8 {
        let dv = if d.is_empty() { 0.0 } else { eval(&d, t) };
        if dv == 0.0 || !dv.is_finite() {
            break;
        }
        t -= eval(&c, t) / dv;
        if !t.is_finite() {
            return None;
        }
    }
    let q = exact(t)?;
    (q > *lo && q < *hi).then_some(q)
}

#[cfg(test)]
mod tests;

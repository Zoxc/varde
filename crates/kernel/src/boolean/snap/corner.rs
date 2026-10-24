//! The snap's corner rule: where the snapped side `S`'s vertices, the
//! target `T`'s vertices and the rim's vertices land, decided once, before
//! any edge is matched along a target edge, so that incidences that don't
//! move with the gap bound `b_K` (a vertex on an edge, a vertex on a rim
//! edge, two crossings of coincident edges) are placed by rule and never
//! sent to a halving of `b_K`.
//!
//! **On and off.** For two elements `x`, `y` (vertices or edges) with
//! bands `β_x`, `β_y` (an edge's band, or a vertex's tolerance plus the
//! operation's rounding; given by the caller, [`CornerInput::band`]),
//! `R_on = max(b_K, β_x + β_y)` and `R_off = R_on/2`. `x` is **on** `y`
//! when their distance is certified at most `R_on`, **off** when it is
//! certified above `R_off`; "on" is tried first. Distances are taken on
//! pre-snap positions (a rim vertex on an `S` edge: the edge's pre-snap
//! point there), exactly: positions are stored `f64`s or exact points of
//! `S`'s patches, edges straight segments between their end vertices (the
//! flat sheets), and the squared distance is a rational compared exactly
//! with `R_on²`. So every distance certifies on or off at once, and the
//! rule never refines, escalates or halves.
//!
//! **The window** ([`Corner::window`]): the rim's window ([`Rim::window`],
//! `T` elements inside the rim or under a rim piece, `S` elements inside
//! the rim), the vertices an earlier cluster placed
//! ([`CornerInput::fixed`]), and every candidate `S` element
//! ([`CornerInput::candidates`]) on a `T` element of the window.
//!
//! **Anchors.** `T` vertices of the window and the fixed vertices, in
//! geometric order (`varde_decide::order`), never identified with each
//! other and never moved; then the branch vertices the rim keeps (below),
//! in the order of their points.
//!
//! **Placements** ([`Corner::placed`]). An `S` vertex of the window (not a
//! fixed one) on an anchor goes to the first it is on (`pos := w.pos`);
//! else, on a `T` edge of the window, to the first it is on, at the
//! clamped foot `C_e(s*)` (rounded to `f64` once: the rounding is
//! [`Moved::storage`]); else it stays (the snap map moves it by at most
//! `b_K`). A vertex group ([`CornerInput::groups`]) is placed as one, by
//! its first vertex in geometric order. A placed vertex keeps its
//! identity.
//!
//! **Splits** ([`Corner::splits`]). An anchor on an `S` edge of the window
//! splits it at a vertex at the anchor's position, named by the pair
//! (the edge, the anchor); not where the anchor is the edge's own end, or
//! where an end of the edge was placed on that anchor.
//!
//! **Rim vertices** ([`Corner::rim_verts`], one at the start of each rim
//! piece, so one per rim edge end). A rim vertex is an `S` vertex, an
//! anchor, or a point of an `S` edge, a `T` edge or a box side
//! ([`RimVertKind`]). Every one that isn't an anchor joins the first
//! anchor it is on, `T` vertices before branch vertices; else it keeps
//! what the placements gave it (an `S` vertex), or stays. Its band is a
//! vertex's own; a new vertex's the larger of the bands of the two rim
//! pieces meeting there ([`RimInput::band`]) and of the edge it lies
//! on. A branch vertex's band is its piece's. Identification
//! is **anchored**: two non-anchors never join each other, so no chain of
//! joins moves a vertex by more than `R_on`.
//!
//! **Coincident edges** ([`CornerInput::coincident`], by record). Rim
//! vertices on edges of one coincidence class, on different edges, are
//! grouped, each group by its first vertex in the order of their points
//! (later ones join the first they are on, never each other): the group
//! takes its first vertex's outcome, the others bitwise equal to it
//! ([`Placement::With`] when it stays), each joined to the first by a
//! zero-length rim edge ([`Corner::zero_edges`]).
//!
//! **Displacement** ([`Moved::displacement`]): a certified upper bound of
//! the distance a vertex moves from its pre-snap position, at most `R_on`
//! (`b_K`, or `β_x + β_y`, the pin term of the vertex's tolerance) up to
//! the storage rounding; the caller adds it to the vertex's tolerance.
//!
//! **Root clusters at an anchor** ([`Corner::spans`]). Along the rim, per
//! target surface, the gap's cells (the rim's, [`rim::PieceGap`]) are
//! halved until each is certified on a `T`-vertex anchor (every control
//! point of its curve within `R_on`: the curve lies in their hull) or off
//! it (the control points' box farther than `R_off`); the hysteresis makes
//! this finite. A maximal run of cells on an anchor, widened over the
//! rim's root clusters it meets ([`RootCluster`]) to one-signed cells on
//! both sides, is one root cluster carried by the anchor: its net sign
//! change `0`, the anchor carries no branch; `±1`, the anchor **is** the
//! branch vertex, of that degree; none where a side ends at a zero gap or
//! an end of the sequence. Two anchors' spans sharing cells are cut at the
//! first one-signed cell between them (along the rim, outside every root
//! cluster) where each keeps cells it is on; with none, the anchor first
//! in geometric order carries the whole. A sequence round a loop with no
//! cell outside every span is one span of net `0`, carried by the first
//! anchor on it. The rim's root clusters and branch vertices inside a span
//! are absorbed; the rest are kept ([`Corner::clusters`],
//! [`Corner::branches`]).
//!
//! **Order.** Every choice reads the geometric order: anchors and edges by
//! their keys, branch and rim vertices by their points rounded to `f64`
//! (ties: their pieces, numbered along the loops by geometry), points on
//! an edge from its lesser end. Ids break exact ties only.
//!
//! **Work.** Charged on the operation's budget in passes: the window (an
//! item per candidate, a unit per test), the vertices (an item per vertex
//! or group), the splits (an item per `S` edge), the rim vertices (an item
//! per piece), the spans (an item per sequence, a unit per cell test and
//! split) and the identification (an item per rim vertex). The cells stop
//! at `MAX_SUBDIV_DEPTH`.

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::{Ceiling, Nearest};
use varde_decide::bezier::{Dyadic, Poly};
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::num::bounded::up;
use varde_decide::num::schedule::MAX_SUBDIV_DEPTH;
use varde_decide::order::{self, EdgeKey, EntityOrder, Pos, VertKey};
use varde_decide::records::keys::Operand;

use super::rim::{self, Along, Branch, Element, Gap, Implicit, Piece, Rim, RimInput, RootCluster};
use crate::infra::budget::{Cap, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point in space, exactly.
type P3 = [Q; 3];
/// A vertex of either operand.
type Vert = (Operand, VertId);
/// A point along a sequence of the rim's gaps: the gap's position in it
/// and the parameter on its piece.
type SeqPos = (usize, Dyadic);

/// What [`corner`] reads besides the rim.
#[derive(Clone, Copy)]
pub struct CornerInput<'a, O> {
    /// The cluster's gap bound `b_K` (finite, `> 0`).
    pub b_k: f64,
    /// The operands' vertices and edges: their geometric order, the
    /// vertices' positions (their keys') and the edges' ends; edges are
    /// read as the straight segments between their end vertices.
    pub order: &'a O,
    /// The band of an operand's edge or vertex (finite, `≥ 0`).
    pub band: &'a (dyn Fn(Element) -> f64 + Sync),
    /// The snapped operand's edges and vertices the window may add: those
    /// of the faces the cluster reaches.
    pub candidates: &'a [Element],
    /// Vertices an earlier cluster placed: anchors here, never moved.
    pub fixed: &'a [(Operand, VertId)],
    /// The snapped operand's vertex groups, each placed as one.
    pub groups: &'a [&'a [VertId]],
    /// The snapped operand's classes of edges coincident by record.
    pub coincident: &'a [&'a [EdgeId]],
}

/// Where a vertex goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// It stays where it is: the snap map moves it by at most `b_K`.
    Stays,
    /// To this anchor's position.
    Vert(Operand, VertId),
    /// To this target edge at the clamped foot, the fraction `s` of the
    /// way from the edge's stored start.
    Edge { edge: EdgeId, s: f64 },
    /// To the position of this branch vertex (an index into
    /// [`Corner::branches`]), wherever the rewrite puts it.
    Branch(u32),
    /// To the position of this rim vertex (an index into
    /// [`Corner::rim_verts`]), bitwise, wherever the rewrite puts it.
    With(u32),
}

/// A vertex's outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    /// Where it goes.
    pub to: Placement,
    /// Its position, where the rule fixes it: on an anchor or an edge.
    pub pos: Option<[f64; 3]>,
    /// An upper bound of its distance from its pre-snap position to
    /// [`pos`](Self::pos); to the pre-snap position of the branch or rim
    /// vertex it joins otherwise; `0` if it stays.
    pub displacement: f64,
    /// An upper bound of the distance from `pos` to the exact foot on the
    /// edge it was placed on (the storage rounding); `0` otherwise.
    pub storage: f64,
}

impl Moved {
    pub(super) const STAYS: Moved = Moved {
        to: Placement::Stays,
        pos: None,
        displacement: 0.0,
        storage: 0.0,
    };
}

/// A placed vertex of the snapped operand.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedVert {
    /// The vertex, which keeps its identity.
    pub vert: VertId,
    /// Its outcome.
    pub moved: Moved,
}

/// A split of an edge of the snapped operand at an anchor on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// The edge; with [`at`](Self::at) the new vertex's identity.
    pub edge: EdgeId,
    /// The anchor.
    pub at: (Operand, VertId),
    /// The fraction of the way from the edge's stored start to the
    /// anchor's foot on it, rounded.
    pub s: f64,
    /// The new vertex's position: the anchor's.
    pub pos: [f64; 3],
    /// An upper bound of its distance from the edge's pre-snap point at
    /// the foot.
    pub displacement: f64,
}

/// What a rim vertex is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RimVertKind {
    /// A vertex of an operand: of `S`, or an anchor.
    Vert(Operand, VertId),
    /// A point of an operand's edge.
    OnEdge(Operand, EdgeId),
    /// A point of a box side only.
    Box,
}

/// A rim vertex: the start of a rim piece.
#[derive(Clone, Debug, PartialEq)]
pub struct RimVert {
    /// The piece it starts.
    pub piece: u32,
    /// What it is.
    pub kind: RimVertKind,
    /// Its outcome.
    pub moved: Moved,
}

/// A root cluster of the gap along the rim carried by an anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorSpan {
    /// The anchor.
    pub anchor: (Operand, VertId),
    /// The target faces of its surface beside the span's first piece.
    pub targets: Vec<FaceId>,
    /// Where it starts and ends: a piece's index and the parameter on it.
    pub from: (u32, Dyadic),
    pub to: (u32, Dyadic),
    /// The sign change across it along the rim (`+1` from inside to
    /// outside the target): the branch degree the anchor carries.
    pub net: Option<i8>,
}

/// The corner rule's decisions for one cluster (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    /// The window's elements, sorted.
    pub window: Vec<Element>,
    /// The snapped operand's placed vertices, sorted by id.
    pub placed: Vec<PlacedVert>,
    /// The splits, by edge id, then along the edge from its lesser end.
    pub splits: Vec<Split>,
    /// The rim vertices, by piece.
    pub rim_verts: Vec<RimVert>,
    /// Pairs of rim vertices (indices) joined by a zero-length rim edge.
    pub zero_edges: Vec<[u32; 2]>,
    /// The root clusters carried by anchors, along the loops.
    pub spans: Vec<AnchorSpan>,
    /// The rim's root clusters outside every span.
    pub clusters: Vec<RootCluster>,
    /// The rim's branch vertices outside every span.
    pub branches: Vec<Branch>,
}

/// Why the corner rule can't be decided.
#[derive(Clone, Debug, PartialEq)]
pub enum CornerError {
    /// `b_K` isn't finite and `> 0`, or a band isn't finite and `≥ 0`.
    Band,
    /// A position isn't finite.
    Position,
    /// A rim piece's face or tile isn't described, or its patch or a gap
    /// doesn't fit.
    Face(FaceId),
    /// The budget ran out or a cell reached the depth limit
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The corner rule of `rim`, the rim of a cluster made from `rim_input`
/// (module docs), charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn corner<O>(
    rim: &Rim,
    rim_input: &RimInput<'_>,
    input: &CornerInput<'_, O>,
    work: &mut Work,
) -> Result<Corner, CornerError>
where
    O: EntityOrder + Sync,
    O::Key: Send + Sync,
{
    if !(input.b_k.is_finite() && input.b_k > 0.0) {
        return Err(CornerError::Band);
    }
    let s_op = rim_input.snapped;
    let t_op = s_op.other();
    let geo = Geo {
        order: input.order,
        band: input.band,
        b_k: input.b_k,
    };

    // The window: the rim's, the fixed vertices, and the candidates on a
    // `T` element of it.
    let mut t_els: BTreeSet<Element> = rim
        .window
        .iter()
        .copied()
        .filter(|e| op_of(*e) == t_op)
        .collect();
    t_els.extend(input.fixed.iter().map(|&(o, v)| Element::Vert(o, v)));
    let mut window: BTreeSet<Element> = rim.window.iter().copied().collect();
    window.extend(t_els.iter().copied());
    let t_list: Vec<Element> = t_els.iter().copied().collect();
    let cands: Vec<Element> = input
        .candidates
        .iter()
        .copied()
        .filter(|e| op_of(*e) == s_op && !window.contains(e))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let added = run(work, "snap corner: window", &cands, |&c, ledger| {
        for &t in &t_list {
            units(ledger, 1)?;
            if geo.on(c, t).map_err(Stop::Failed)? {
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    window.extend(cands.iter().zip(&added).filter(|p| *p.1).map(|p| *p.0));

    // The anchors and the target edges, in geometric order.
    let fixed: BTreeSet<(Operand, VertId)> = input.fixed.iter().copied().collect();
    let mut anchors: Vec<(VertKey<O::Key>, Vert)> = window
        .iter()
        .filter_map(|e| match *e {
            Element::Vert(o, v) if o == t_op || fixed.contains(&(o, v)) => Some((o, v)),
            _ => None,
        })
        .map(|(o, v)| (input.order.vert_key(o, v), (o, v)))
        .collect();
    order::sort(&mut anchors);
    let anchors: Vec<(Operand, VertId)> = anchors.into_iter().map(|a| a.1).collect();
    let mut t_edges: Vec<(EdgeKey<O::Key>, EdgeId)> = window
        .iter()
        .filter_map(|e| match *e {
            Element::Edge(o, g) if o == t_op => Some((input.order.edge_key(o, g), g)),
            _ => None,
        })
        .collect();
    order::sort(&mut t_edges);
    let t_edges: Vec<EdgeId> = t_edges.into_iter().map(|e| e.1).collect();

    // The placements, by group leader.
    let s_verts: BTreeSet<VertId> = window
        .iter()
        .filter_map(|e| match *e {
            Element::Vert(o, v) if o == s_op && !fixed.contains(&(o, v)) => Some(v),
            _ => None,
        })
        .collect();
    let mut leader_of: BTreeMap<VertId, VertId> = BTreeMap::new();
    let mut members: BTreeMap<VertId, Vec<VertId>> = BTreeMap::new();
    for g in input.groups {
        if !g.iter().any(|v| s_verts.contains(v)) {
            continue;
        }
        let lead = order::first(
            g.iter()
                .filter(|v| !fixed.contains(&(s_op, **v)))
                .map(|&v| (input.order.vert_key(s_op, v), v)),
        );
        let Some(lead) = lead else { continue };
        for &v in g.iter().filter(|v| !fixed.contains(&(s_op, **v))) {
            leader_of.entry(v).or_insert(lead);
        }
    }
    for &v in &s_verts {
        let l = *leader_of.entry(v).or_insert(v);
        members.entry(l).or_default().push(v);
    }
    for (&v, &l) in &leader_of {
        let m = members.entry(l).or_default();
        if !m.contains(&v) {
            m.push(v);
        }
    }
    let leaders: Vec<(VertId, Vec<VertId>)> = members.into_iter().collect();
    let per_leader = run(
        work,
        "snap corner: vertices",
        &leaders,
        |(lead, ms), ledger| {
            let p = geo.vert(s_op, *lead);
            let el = Element::Vert(s_op, *lead);
            let mut to = Placement::Stays;
            for &(o, w) in &anchors {
                units(ledger, 1)?;
                if geo.on(el, Element::Vert(o, w)).map_err(Stop::Failed)? {
                    to = Placement::Vert(o, w);
                    break;
                }
            }
            let mut foot: Option<Q> = None;
            if to == Placement::Stays {
                for &g in &t_edges {
                    units(ledger, 1)?;
                    if geo.on(el, Element::Edge(t_op, g)).map_err(Stop::Failed)? {
                        let [a, b] = geo.seg(t_op, g);
                        let s = to_f64(&point_seg(&p, &a, &b).1);
                        foot = Some(Q::try_from(s).expect("a fraction in [0, 1]"));
                        to = Placement::Edge { edge: g, s };
                        break;
                    }
                }
            }
            let mut out = Vec::with_capacity(ms.len());
            for &v in ms {
                units(ledger, 1)?;
                let pre = geo.vert(s_op, v);
                let moved = match to {
                    Placement::Vert(o, w) => {
                        let pos = geo.pos(o, w);
                        Moved {
                            to,
                            pos: Some(pos),
                            displacement: dist_up(&d2(&pre, &exact3(pos))),
                            storage: 0.0,
                        }
                    }
                    Placement::Edge { edge, .. } => {
                        let [a, b] = geo.seg(t_op, edge);
                        let s = foot.as_ref().expect("a foot");
                        let on: P3 = [0, 1, 2].map(|k| &a[k] + s * (&b[k] - &a[k]));
                        let pos = on.each_ref().map(to_f64);
                        let at = exact3(pos);
                        Moved {
                            to,
                            pos: Some(pos),
                            displacement: dist_up(&d2(&pre, &at)),
                            storage: dist_up(&d2(&on, &at)),
                        }
                    }
                    _ => Moved::STAYS,
                };
                out.push(PlacedVert { vert: v, moved });
            }
            Ok(out)
        },
    )?;
    let mut placed: Vec<PlacedVert> = per_leader.into_iter().flatten().collect();
    placed.sort_by_key(|p| p.vert);
    let placed_at: BTreeMap<VertId, Moved> = placed.iter().map(|p| (p.vert, p.moved)).collect();

    // The splits.
    let s_edges: Vec<EdgeId> = window
        .iter()
        .filter_map(|e| match *e {
            Element::Edge(o, g) if o == s_op => Some(g),
            _ => None,
        })
        .collect();
    let per_edge = run(work, "snap corner: splits", &s_edges, |&h, ledger| {
        let ends = input.order.ends(s_op, h);
        let dir = order::Along::of(
            &input.order.vert_key(s_op, ends[0]),
            &input.order.vert_key(s_op, ends[1]),
        );
        let [a, b] = geo.seg(s_op, h);
        let mut out: Vec<(order::Coord, usize, Split)> = Vec::new();
        for (rank, &(o, w)) in anchors.iter().enumerate() {
            units(ledger, 1)?;
            if o == s_op && ends.contains(&w) {
                continue;
            }
            let shared = ends.iter().any(|v| {
                placed_at
                    .get(v)
                    .is_some_and(|m| m.to == Placement::Vert(o, w))
            });
            if shared
                || !geo
                    .on(Element::Edge(s_op, h), Element::Vert(o, w))
                    .map_err(Stop::Failed)?
            {
                continue;
            }
            let p = geo.vert(o, w);
            let (dd, s) = point_seg(&p, &a, &b);
            let s = to_f64(&s);
            let key = dir.param(s).expect("a fraction");
            out.push((
                key,
                rank,
                Split {
                    edge: h,
                    at: (o, w),
                    s,
                    pos: geo.pos(o, w),
                    displacement: dist_up(&dd),
                },
            ));
        }
        out.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)));
        Ok(out.into_iter().map(|x| x.2).collect::<Vec<_>>())
    })?;
    let splits: Vec<Split> = per_edge.into_iter().flatten().collect();

    // The root clusters at the anchors.
    let spans_out = spans(rim, rim_input, input, &geo, &anchors, work)?;
    let clusters: Vec<RootCluster> = rim
        .clusters
        .iter()
        .enumerate()
        .filter(|(c, _)| !spans_out.absorbed.contains(c))
        .map(|(_, c)| c.clone())
        .collect();
    let branches: Vec<Branch> = rim
        .branches
        .iter()
        .enumerate()
        .filter(|(b, _)| !spans_out.absorbed_branches.contains(b))
        .map(|(_, b)| b.clone())
        .collect();

    // The branch anchors, in the order of their points.
    let mut branch_at: Vec<(Pos, u32, P3, f64)> = Vec::with_capacity(branches.len());
    for (i, b) in branches.iter().enumerate() {
        let piece = &rim.pieces[b.piece as usize];
        let p = point_on(rim_input, piece.face, piece.tile, &b.uv)
            .ok_or(CornerError::Face(piece.face))?;
        let beta = rim_band(rim_input, piece.along)?;
        branch_at.push((rounded_pos(&p)?, i as u32, p, beta));
    }
    branch_at.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.cmp(&y.1)));

    // The rim vertices: what each is, and its pre-snap point.
    let piece_ix: Vec<u32> = (0..rim.pieces.len() as u32).collect();
    let mut prev: Vec<u32> = vec![0; rim.pieces.len()];
    for l in &rim.loops {
        for (k, &p) in l.iter().enumerate() {
            prev[p as usize] = l[(k + l.len() - 1) % l.len()];
        }
    }
    let rvs = run(
        work,
        "snap corner: rim vertices",
        &piece_ix,
        |&k, ledger| {
            units(ledger, 1)?;
            let piece = &rim.pieces[k as usize];
            let kind = rim_kind(rim_input, piece, s_op).map_err(Stop::Failed)?;
            let pre = match kind {
                RimVertKind::Vert(o, v) => geo.vert(o, v),
                _ => point_on(rim_input, piece.face, piece.tile, &piece.ends[0])
                    .ok_or(Stop::Failed(CornerError::Face(piece.face)))?,
            };
            // A vertex's own band; a new vertex's, the larger of the bands
            // of the rim pieces meeting there and of the edge it lies on.
            let rims = larger(
                rim_band(rim_input, piece.along).map_err(Stop::Failed)?,
                rim_band(rim_input, rim.pieces[prev[k as usize] as usize].along)
                    .map_err(Stop::Failed)?,
            );
            let beta = match kind {
                RimVertKind::Vert(o, v) => geo.band(Element::Vert(o, v)).map_err(Stop::Failed)?,
                RimVertKind::OnEdge(o, g) => {
                    larger(rims, geo.band(Element::Edge(o, g)).map_err(Stop::Failed)?)
                }
                RimVertKind::Box => rims,
            };
            let at = rounded_pos(&pre).map_err(Stop::Failed)?;
            Ok((kind, pre, beta, at))
        },
    )?;

    // Coincident groups: their first vertices lead.
    let mut class_of: BTreeMap<EdgeId, usize> = BTreeMap::new();
    for (c, cl) in input.coincident.iter().enumerate() {
        for &h in cl.iter() {
            class_of.entry(h).or_insert(c);
        }
    }
    let mut by_class: BTreeMap<usize, Vec<(Pos, u32)>> = BTreeMap::new();
    for (k, (kind, _, _, at)) in rvs.iter().enumerate() {
        if let RimVertKind::OnEdge(o, h) = kind
            && *o == s_op
            && let Some(&c) = class_of.get(h)
        {
            by_class.entry(c).or_default().push((*at, k as u32));
        }
    }
    let mut lead_of: Vec<u32> = (0..rvs.len() as u32).collect();
    let mut zero_edges: Vec<[u32; 2]> = Vec::new();
    for (_, mut vs) in by_class {
        vs.sort();
        let mut leads: Vec<u32> = Vec::new();
        for &(_, k) in &vs {
            let (kind, pre, beta, _) = &rvs[k as usize];
            let mut joined = None;
            for &l in &leads {
                let (lkind, lpre, lbeta, _) = &rvs[l as usize];
                if lkind == kind {
                    continue;
                }
                if on_q(&d2(pre, lpre), input.b_k, *beta, *lbeta) {
                    joined = Some(l);
                    break;
                }
            }
            match joined {
                Some(l) => {
                    lead_of[k as usize] = l;
                    zero_edges.push([l, k]);
                }
                None => leads.push(k),
            }
        }
    }
    zero_edges.sort();

    // Identification: each leader joins the first anchor it is on.
    let rv_ix: Vec<u32> = (0..rvs.len() as u32)
        .filter(|&k| lead_of[k as usize] == k)
        .collect();
    let anchor_set: BTreeSet<(Operand, VertId)> = anchors.iter().copied().collect();
    let lead_moved = run(work, "snap corner: identify", &rv_ix, |&k, ledger| {
        let (kind, pre, beta, _) = &rvs[k as usize];
        units(ledger, 1)?;
        if let RimVertKind::Vert(o, v) = *kind
            && (o == t_op || anchor_set.contains(&(o, v)))
        {
            let pos = geo.pos(o, v);
            return Ok(Moved {
                to: Placement::Vert(o, v),
                pos: Some(pos),
                displacement: dist_up(&d2(pre, &exact3(pos))),
                storage: 0.0,
            });
        }
        let own = match *kind {
            RimVertKind::Vert(o, v) if o == s_op => placed_at.get(&v).copied(),
            _ => None,
        };
        if let Some(m) = own
            && matches!(m.to, Placement::Vert(..))
        {
            return Ok(m);
        }
        if own.is_none() {
            for &(o, w) in &anchors {
                units(ledger, 1)?;
                let wb = geo.band(Element::Vert(o, w)).map_err(Stop::Failed)?;
                let wp = geo.vert(o, w);
                let dd = d2(pre, &wp);
                if on_q(&dd, input.b_k, *beta, wb) {
                    let pos = geo.pos(o, w);
                    return Ok(Moved {
                        to: Placement::Vert(o, w),
                        pos: Some(pos),
                        displacement: dist_up(&dd),
                        storage: 0.0,
                    });
                }
            }
        }
        for (_, i, bp, bb) in &branch_at {
            units(ledger, 1)?;
            let dd = d2(pre, bp);
            if on_q(&dd, input.b_k, *beta, *bb) {
                return Ok(Moved {
                    to: Placement::Branch(*i),
                    pos: None,
                    displacement: dist_up(&dd),
                    storage: 0.0,
                });
            }
        }
        Ok(own.unwrap_or(Moved::STAYS))
    })?;
    let mut moved_of: BTreeMap<u32, Moved> = rv_ix.iter().copied().zip(lead_moved).collect();
    for k in 0..rvs.len() as u32 {
        let l = lead_of[k as usize];
        if l == k {
            continue;
        }
        let lm = moved_of[&l];
        let (_, pre, _, _) = &rvs[k as usize];
        let (_, lpre, _, _) = &rvs[l as usize];
        let m = match lm.to {
            Placement::Vert(..) | Placement::Edge { .. } => {
                let pos = lm.pos.expect("a fixed position");
                Moved {
                    displacement: dist_up(&d2(pre, &exact3(pos))),
                    ..lm
                }
            }
            Placement::Branch(i) => {
                let bp = &branch_at.iter().find(|b| b.1 == i).expect("a branch").2;
                Moved {
                    displacement: dist_up(&d2(pre, bp)),
                    ..lm
                }
            }
            Placement::Stays | Placement::With(_) => Moved {
                to: Placement::With(l),
                pos: None,
                displacement: dist_up(&d2(pre, lpre)),
                storage: 0.0,
            },
        };
        moved_of.insert(k, m);
    }
    let rim_verts: Vec<RimVert> = rvs
        .iter()
        .enumerate()
        .map(|(k, (kind, ..))| RimVert {
            piece: k as u32,
            kind: *kind,
            moved: moved_of[&(k as u32)],
        })
        .collect();

    Ok(Corner {
        window: window.into_iter().collect(),
        placed,
        splits,
        rim_verts,
        zero_edges,
        spans: spans_out.spans,
        clusters,
        branches,
    })
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, CornerError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<CornerError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => CornerError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<CornerError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

fn op_of(e: Element) -> Operand {
    match e {
        Element::Edge(o, _) | Element::Vert(o, _) => o,
    }
}

/// The larger of two bands (neither a NaN), by comparison.
pub(super) fn larger(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `R_on = max(b_K, β_x + β_y)`, the sum rounded up.
pub(crate) fn r_on(b_k: f64, bx: f64, by: f64) -> f64 {
    larger(b_k, up(bx + by))
}

/// Whether squared distance `dd` certifies on (module docs); else it
/// certifies off.
pub(super) fn on_q(dd: &Q, b_k: f64, bx: f64, by: f64) -> bool {
    let r = Q::try_from(r_on(b_k, bx, by)).expect("a finite threshold");
    let r2 = &r * &r;
    if *dd <= r2 {
        return true;
    }
    debug_assert!(*dd > r2 / Q::from(4u32), "off: beyond R_off");
    false
}

pub(super) fn exact3(p: [f64; 3]) -> P3 {
    p.map(|x| Q::try_from(x).expect("a finite position"))
}

pub(super) fn to_f64(x: &Q) -> f64 {
    f64::rounding_from(x, Nearest).0
}

/// An upper bound of `√dd`.
pub(super) fn dist_up(dd: &Q) -> f64 {
    if *dd == 0u32 {
        return 0.0;
    }
    up(f64::rounding_from(dd, Ceiling).0.sqrt())
}

fn rounded_pos(p: &P3) -> Result<Pos, CornerError> {
    Pos::new(p.each_ref().map(to_f64)).map_err(|_| CornerError::Position)
}

pub(super) fn sub3(a: &P3, b: &P3) -> P3 {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}

pub(super) fn dot3(a: &P3, b: &P3) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

pub(super) fn d2(a: &P3, b: &P3) -> Q {
    let d = sub3(a, b);
    dot3(&d, &d)
}

/// The squared distance from `p` to segment `[a, b]` and the clamped
/// fraction of its foot.
pub(super) fn point_seg(p: &P3, a: &P3, b: &P3) -> (Q, Q) {
    let e = sub3(b, a);
    let ee = dot3(&e, &e);
    if ee == 0u32 {
        return (d2(p, a), Q::from(0u32));
    }
    let s = dot3(&sub3(p, a), &e) / &ee;
    let s = if s < 0u32 {
        Q::from(0u32)
    } else if s > 1u32 {
        Q::from(1u32)
    } else {
        s
    };
    let f: P3 = [0, 1, 2].map(|k| &a[k] + &s * &e[k]);
    (d2(p, &f), s)
}

/// The squared distance between segments `[a0, a1]` and `[b0, b1]`.
fn seg_seg(a0: &P3, a1: &P3, b0: &P3, b1: &P3) -> Q {
    let mut best = point_seg(a0, b0, b1).0;
    for c in [
        point_seg(a1, b0, b1).0,
        point_seg(b0, a0, a1).0,
        point_seg(b1, a0, a1).0,
    ] {
        if c < best {
            best = c;
        }
    }
    let (d, e, r) = (sub3(a1, a0), sub3(b1, b0), sub3(a0, b0));
    let (aa, bb, ee) = (dot3(&d, &d), dot3(&d, &e), dot3(&e, &e));
    let den = &aa * &ee - &bb * &bb;
    if den > 0u32 {
        let (c, f) = (dot3(&d, &r), dot3(&e, &r));
        let s = (&bb * &f - &c * &ee) / &den;
        let t = (&aa * &f - &bb * &c) / &den;
        if s > 0u32 && s < 1u32 && t > 0u32 && t < 1u32 {
            let p: P3 = [0, 1, 2].map(|k| &a0[k] + &s * &d[k]);
            let q: P3 = [0, 1, 2].map(|k| &b0[k] + &t * &e[k]);
            let x = d2(&p, &q);
            if x < best {
                best = x;
            }
        }
    }
    best
}

/// The operands' geometry as the rule reads it.
struct Geo<'a, O> {
    order: &'a O,
    band: &'a (dyn Fn(Element) -> f64 + Sync),
    b_k: f64,
}

impl<O: EntityOrder> Geo<'_, O> {
    fn pos(&self, o: Operand, v: VertId) -> [f64; 3] {
        self.order.vert_key(o, v).pos().get()
    }

    fn vert(&self, o: Operand, v: VertId) -> P3 {
        exact3(self.pos(o, v))
    }

    fn seg(&self, o: Operand, g: EdgeId) -> [P3; 2] {
        self.order.ends(o, g).map(|v| self.vert(o, v))
    }

    fn band(&self, e: Element) -> Result<f64, CornerError> {
        let b = (self.band)(e);
        if b.is_finite() && b >= 0.0 {
            Ok(b)
        } else {
            Err(CornerError::Band)
        }
    }

    fn finite(&self, e: Element) -> Result<(), CornerError> {
        let ok = |o: Operand, v: VertId| self.pos(o, v).iter().all(|x| x.is_finite());
        let fine = match e {
            Element::Vert(o, v) => ok(o, v),
            Element::Edge(o, g) => self.order.ends(o, g).iter().all(|&v| ok(o, v)),
        };
        if fine {
            Ok(())
        } else {
            Err(CornerError::Position)
        }
    }

    /// Whether `x` is on `y` (module docs): certified on, else off.
    fn on(&self, x: Element, y: Element) -> Result<bool, CornerError> {
        self.finite(x)?;
        self.finite(y)?;
        let (bx, by) = (self.band(x)?, self.band(y)?);
        let dd = match (x, y) {
            (Element::Vert(o, v), Element::Vert(p, w)) => d2(&self.vert(o, v), &self.vert(p, w)),
            (Element::Vert(o, v), Element::Edge(p, g))
            | (Element::Edge(p, g), Element::Vert(o, v)) => {
                let [a, b] = self.seg(p, g);
                point_seg(&self.vert(o, v), &a, &b).0
            }
            (Element::Edge(o, g), Element::Edge(p, h)) => {
                let [a0, a1] = self.seg(o, g);
                let [b0, b1] = self.seg(p, h);
                seg_seg(&a0, &a1, &b0, &b1)
            }
        };
        Ok(on_q(&dd, self.b_k, bx, by))
    }
}

/// The band of a rim piece along `a`, checked.
fn rim_band(input: &RimInput<'_>, a: Along) -> Result<f64, CornerError> {
    let b = (input.band)(a);
    if b.is_finite() && b >= 0.0 {
        Ok(b)
    } else {
        Err(CornerError::Band)
    }
}

/// The point of face `face`'s tile `tile` at global UV `uv`, exactly.
pub(super) fn point_on(
    input: &RimInput<'_>,
    face: FaceId,
    tile: [i64; 2],
    uv: &[Q; 2],
) -> Option<P3> {
    let f = input.faces.iter().find(|f| f.face == face)?;
    let (_, patch) = f.tiles.iter().find(|(k, _)| *k == tile)?;
    let u = &uv[0] - Q::from(tile[0]);
    let v = &uv[1] - Q::from(tile[1]);
    let h = patch.tensors();
    let w = h[3].eval(&u, &v);
    if w <= 0u32 {
        return None;
    }
    Some([0, 1, 2].map(|k| h[k].eval(&u, &v) / &w))
}

/// What the start of `piece` is (module docs): an `S` vertex, a `T`
/// vertex, a point of an `S` edge, of a `T` edge, or of a box side only.
fn rim_kind(
    input: &RimInput<'_>,
    piece: &Piece,
    s_op: Operand,
) -> Result<RimVertKind, CornerError> {
    let face = input
        .faces
        .iter()
        .find(|f| f.face == piece.face)
        .ok_or(CornerError::Face(piece.face))?;
    let p = &piece.ends[0];
    let mut found: Option<RimVertKind> = None;
    for (trims, op) in [(face.own, s_op), (face.target, s_op.other())] {
        for t in trims {
            let pt = |x: &[f64; 3]| -> Option<[Q; 2]> {
                Some([Q::try_from(x[0]).ok()?, Q::try_from(x[1]).ok()?])
            };
            let n = t.spans.len();
            for (k, s) in t.spans.iter().enumerate() {
                let (Some(a), Some(b)) = (pt(&s[0]), pt(&s[2])) else {
                    return Err(CornerError::Face(piece.face));
                };
                let kind = if k == 0 && *p == a {
                    RimVertKind::Vert(op, t.ends[0])
                } else if k + 1 == n && *p == b {
                    RimVertKind::Vert(op, t.ends[1])
                } else if on_span(p, &a, &b) {
                    RimVertKind::OnEdge(op, t.edge)
                } else {
                    continue;
                };
                if found.is_none_or(|f| rank(kind, s_op) < rank(f, s_op)) {
                    found = Some(kind);
                }
            }
        }
    }
    Ok(found.unwrap_or(RimVertKind::Box))
}

/// The priority of a rim vertex's kinds: `S` vertex, `T` vertex, `S`
/// edge, `T` edge; ids last.
fn rank(k: RimVertKind, s_op: Operand) -> (u8, RimVertKind) {
    let r = match k {
        RimVertKind::Vert(o, _) if o == s_op => 0,
        RimVertKind::Vert(..) => 1,
        RimVertKind::OnEdge(o, _) if o == s_op => 2,
        RimVertKind::OnEdge(..) => 3,
        RimVertKind::Box => 4,
    };
    (r, k)
}

/// Whether `p` lies on the closed segment `[a, b]` of the plane.
fn on_span(p: &[Q; 2], a: &[Q; 2], b: &[Q; 2]) -> bool {
    let d = [&b[0] - &a[0], &b[1] - &a[1]];
    let r = [&p[0] - &a[0], &p[1] - &a[1]];
    if &d[0] * &r[1] - &d[1] * &r[0] != 0u32 {
        return false;
    }
    let t = &d[0] * &r[0] + &d[1] * &r[1];
    let dd = &d[0] * &d[0] + &d[1] * &d[1];
    t >= 0u32 && t <= dd
}

// ---------------------------------------------------------------------
// Root clusters at the anchors.

/// The spans and what they absorb.
struct SpansOut {
    spans: Vec<AnchorSpan>,
    /// Indices of the rim's root clusters inside a span.
    absorbed: BTreeSet<usize>,
    /// Indices of the rim's branch vertices inside a span.
    absorbed_branches: BTreeSet<usize>,
}

/// A sequence of the rim's gaps along a loop against one surface, as the
/// rim forms them.
struct SeqJob {
    /// Indices into the rim's gaps, in order along the loop.
    gaps: Vec<usize>,
    cyclic: bool,
}

/// An anchor as the cells read it: its point, band and rank.
struct AnchorAt {
    id: (Operand, VertId),
    p: P3,
    beta: f64,
}

/// A cell of a sequence: its gap's index, dyadic span, sign, the rim root
/// cluster holding it, and its forms.
#[derive(Clone)]
struct SpanCell {
    g: usize,
    depth: u32,
    index: u64,
    gap: Gap,
    cluster: Option<usize>,
    curve: [Poly<Q>; 4],
    gapf: Poly<Q>,
}

impl SpanCell {
    fn span(&self) -> [Dyadic; 2] {
        let d = |j: u64| Dyadic::new(j, self.depth).expect("a cell within [0, 1]");
        [d(self.index), d(self.index + 1)]
    }

    fn halves(&self) -> [SpanCell; 2] {
        let c = self.curve.each_ref().map(Poly::split_half);
        let g = self.gapf.split_half();
        let side = |k: usize| {
            let gapf = g[k].clone();
            let gap = match self.gap {
                Gap::Cluster => one_sign(&gapf).unwrap_or(Gap::Cluster),
                s => s,
            };
            SpanCell {
                g: self.g,
                depth: self.depth + 1,
                index: 2 * self.index + k as u64,
                gap,
                cluster: self.cluster,
                curve: [0, 1, 2, 3].map(|i| c[i][k].clone()),
                gapf,
            }
        };
        [side(0), side(1)]
    }

    /// Its control points, affine.
    fn points(&self) -> Vec<P3> {
        let w = self.curve[3].coefs();
        (0..w.len())
            .map(|i| [0, 1, 2].map(|k| &self.curve[k].coefs()[i] / &w[i]))
            .collect()
    }
}

/// A cell against an anchor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Near {
    On,
    Off,
    Split,
}

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

/// `cell` against anchor `a` with threshold `r` (module docs).
fn near(cell: &SpanCell, a: &AnchorAt, r: &Q) -> Near {
    let r2 = r * r;
    let pts = cell.points();
    if pts.iter().all(|p| d2(p, &a.p) <= r2) {
        return Near::On;
    }
    // The box's distance.
    let mut dd = Q::from(0u32);
    for k in 0..3 {
        let lo = pts.iter().map(|p| &p[k]).min().expect("a point");
        let hi = pts.iter().map(|p| &p[k]).max().expect("a point");
        let x = if a.p[k] < *lo {
            lo - &a.p[k]
        } else if a.p[k] > *hi {
            &a.p[k] - hi
        } else {
            Q::from(0u32)
        };
        dd += &x * &x;
    }
    if dd * Q::from(4u32) > r2 {
        Near::Off
    } else {
        Near::Split
    }
}

/// The forms of the dyadic cell `[index, index + 1]/2^depth` of whole
/// forms `curve`, `gap`.
fn sub_forms(
    curve: &[Poly<Q>; 4],
    gap: &Poly<Q>,
    depth: u32,
    index: u64,
) -> ([Poly<Q>; 4], Poly<Q>) {
    let mut c = curve.clone();
    let mut g = gap.clone();
    for k in (0..depth).rev() {
        let right = (index >> k) & 1 == 1;
        let pick = usize::from(right);
        c = c.each_ref().map(|p| p.split_half()[pick].clone());
        g = g.split_half()[pick].clone();
    }
    (c, g)
}

/// A dyadic point's depth and index as a cell's left end, given its right
/// end.
fn cell_of(span: [Dyadic; 2]) -> (u32, u64) {
    let [lo, hi] = span;
    let d = lo.exp().max(hi.exp());
    (d, lo.num() << (d - lo.exp()))
}

/// The spans of the rim's root clusters at `anchors` (module docs).
fn spans<O>(
    rim: &Rim,
    rim_input: &RimInput<'_>,
    input: &CornerInput<'_, O>,
    geo: &Geo<'_, O>,
    anchors: &[(Operand, VertId)],
    work: &mut Work,
) -> Result<SpansOut, CornerError>
where
    O: EntityOrder + Sync,
    O::Key: Send + Sync,
{
    let mut out = SpansOut {
        spans: Vec::new(),
        absorbed: BTreeSet::new(),
        absorbed_branches: BTreeSet::new(),
    };
    if anchors.is_empty() {
        return Ok(out);
    }
    let at: Vec<AnchorAt> = anchors
        .iter()
        .map(|&(o, v)| {
            geo.finite(Element::Vert(o, v))?;
            Ok(AnchorAt {
                id: (o, v),
                p: geo.vert(o, v),
                beta: geo.band(Element::Vert(o, v))?,
            })
        })
        .collect::<Result<_, CornerError>>()?;

    // The surfaces, as the rim groups them: distinct implicits, in the
    // order of their least face.
    let mut by_face: Vec<(FaceId, Implicit)> = rim_input.targets.to_vec();
    by_face.sort_by_key(|t| t.0);
    let mut surfaces: Vec<Implicit> = Vec::new();
    for (_, imp) in &by_face {
        if !surfaces.contains(imp) {
            surfaces.push(*imp);
        }
    }
    let surface_of = |targets: &[FaceId]| -> Option<usize> {
        let f = targets.first()?;
        let imp = by_face.iter().find(|t| t.0 == *f)?.1;
        surfaces.iter().position(|s| *s == imp)
    };

    // The sequences.
    let mut seqs: Vec<SeqJob> = Vec::new();
    for l in &rim.loops {
        for s in 0..surfaces.len() {
            let gs: Vec<Option<usize>> = l
                .iter()
                .map(|&p| {
                    rim.gaps
                        .iter()
                        .position(|g| g.piece == p && surface_of(&g.targets) == Some(s))
                })
                .collect();
            let usable = |g: &Option<usize>| {
                g.is_some_and(|g| rim.gaps[g].cells.iter().all(|c| c.1 != Gap::Zero))
            };
            let n = gs.len();
            if gs.iter().all(usable) {
                seqs.push(SeqJob {
                    gaps: gs.iter().map(|g| g.expect("usable")).collect(),
                    cyclic: true,
                });
                continue;
            }
            let start = (0..n).find(|&k| !usable(&gs[k])).expect("a break");
            let mut cur = Vec::new();
            for k in 1..=n {
                let g = gs[(start + k) % n];
                if usable(&g) {
                    cur.push(g.expect("usable"));
                } else if !cur.is_empty() {
                    seqs.push(SeqJob {
                        gaps: std::mem::take(&mut cur),
                        cyclic: false,
                    });
                }
            }
            if !cur.is_empty() {
                seqs.push(SeqJob {
                    gaps: cur,
                    cyclic: false,
                });
            }
        }
    }

    let per_seq = run(work, "snap corner: spans", &seqs, |seq, ledger| {
        seq_spans(seq, rim, rim_input, input.b_k, &at, &surface_of, ledger)
    })?;
    for s in per_seq {
        out.spans.extend(s.spans);
        out.absorbed.extend(s.absorbed);
        out.absorbed_branches.extend(s.absorbed_branches);
    }
    Ok(out)
}

/// One sequence's spans (module docs).
fn seq_spans(
    seq: &SeqJob,
    rim: &Rim,
    rim_input: &RimInput<'_>,
    b_k: f64,
    anchors: &[AnchorAt],
    surface_of: &(dyn Fn(&[FaceId]) -> Option<usize> + Sync),
    ledger: &mut Ledger<'_>,
) -> Result<SpansOut, Stop<CornerError>> {
    let mut out = SpansOut {
        spans: Vec::new(),
        absorbed: BTreeSet::new(),
        absorbed_branches: BTreeSet::new(),
    };
    let fail = |e: CornerError| Stop::Failed(e);
    let surface = surface_of(&rim.gaps[seq.gaps[0]].targets);
    // The position of a point along the sequence.
    let pos_of = |piece: u32, t: Dyadic| -> Option<(usize, Dyadic)> {
        seq.gaps
            .iter()
            .position(|&g| rim.gaps[g].piece == piece)
            .map(|i| (i, t))
    };
    // The rim's root clusters on this sequence, as position ranges.
    let mut ranges: Vec<(usize, SeqPos, SeqPos)> = Vec::new();
    for (c, rc) in rim.clusters.iter().enumerate() {
        if surface_of(&rc.targets) != surface {
            continue;
        }
        if let (Some(a), Some(b)) = (pos_of(rc.from.0, rc.from.1), pos_of(rc.to.0, rc.to.1)) {
            ranges.push((c, a, b));
        }
    }
    let cluster_at = |i: usize, span: [Dyadic; 2]| -> Option<usize> {
        let (lo, hi) = ((i, span[0]), (i, span[1]));
        ranges
            .iter()
            .find(|(_, a, b)| {
                if a <= b {
                    *a <= lo && hi <= *b
                } else {
                    lo >= *a || hi <= *b
                }
            })
            .map(|r| r.0)
    };

    // The cells, refined against every anchor.
    let mut cells: Vec<SpanCell> = Vec::new();
    for (i, &g) in seq.gaps.iter().enumerate() {
        let gap = &rim.gaps[g];
        let piece = &rim.pieces[gap.piece as usize];
        let imp =
            surfaces_imp(rim_input, &gap.targets).ok_or(fail(CornerError::Face(piece.face)))?;
        let (curve, gapf) =
            rim::piece_forms(rim_input, piece, &imp).ok_or(fail(CornerError::Face(piece.face)))?;
        let beta = rim_band(rim_input, piece.along).map_err(fail)?;
        let rs: Vec<Q> = anchors
            .iter()
            .map(|a| Q::try_from(r_on(b_k, a.beta, beta)).expect("a finite threshold"))
            .collect();
        for (span, sign) in &gap.cells {
            units(ledger, 1)?;
            let (depth, index) = cell_of(*span);
            let (c, gf) = sub_forms(&curve, &gapf, depth, index);
            let cluster = if *sign == Gap::Cluster {
                cluster_at(i, *span)
            } else {
                None
            };
            let mut stack = vec![SpanCell {
                g: i,
                depth,
                index,
                gap: *sign,
                cluster,
                curve: c,
                gapf: gf,
            }];
            while let Some(cell) = stack.pop() {
                let mut split = false;
                for (a, r) in anchors.iter().zip(&rs) {
                    units(ledger, 1)?;
                    if near(&cell, a, r) == Near::Split {
                        split = true;
                        break;
                    }
                }
                if !split {
                    cells.push(cell);
                    continue;
                }
                units(ledger, 2)?;
                if cell.depth >= MAX_SUBDIV_DEPTH {
                    return Err(Stop::Cap(Cap::SubdivisionDepth));
                }
                let [l, r] = cell.halves();
                stack.push(r);
                stack.push(l);
            }
        }
    }
    let n = cells.len();
    if n == 0 {
        return Ok(out);
    }
    units(ledger, (n * anchors.len()) as u64)?;
    let thresholds: Vec<Vec<Q>> = cells
        .iter()
        .map(|c| {
            let piece = &rim.pieces[rim.gaps[seq.gaps[c.g]].piece as usize];
            let beta = rim_band(rim_input, piece.along).expect("a checked band");
            anchors
                .iter()
                .map(|a| Q::try_from(r_on(b_k, a.beta, beta)).expect("a finite threshold"))
                .collect()
        })
        .collect();
    let on: Vec<Vec<bool>> = cells
        .iter()
        .zip(&thresholds)
        .map(|(c, rs)| {
            anchors
                .iter()
                .zip(rs)
                .map(|(a, r)| near(c, a, r) == Near::On)
                .collect()
        })
        .collect();
    if !on.iter().any(|o| o.iter().any(|&x| x)) {
        return Ok(out);
    }

    // A cell outside every span: one-signed, outside every root cluster,
    // off every anchor. A cyclic sequence starts there.
    let free = |k: usize| {
        cells[k].gap != Gap::Cluster && cells[k].cluster.is_none() && !on[k].iter().any(|&x| x)
    };
    let rot = if seq.cyclic {
        match (0..n).find(|&k| free(k)) {
            Some(k) => k,
            None => {
                // One span round the loop, net 0, by the first anchor on it.
                let a = (0..anchors.len())
                    .find(|&a| on.iter().any(|o| o[a]))
                    .expect("an anchor on a cell");
                let span = |k: usize| {
                    let c = &cells[k];
                    (rim.gaps[seq.gaps[c.g]].piece, c.span())
                };
                let (p0, s0) = span(0);
                let (p1, s1) = span(n - 1);
                out.spans.push(AnchorSpan {
                    anchor: anchors[a].id,
                    targets: rim.gaps[seq.gaps[cells[0].g]].targets.clone(),
                    from: (p0, s0[0]),
                    to: (p1, s1[1]),
                    net: Some(0),
                });
                absorb(&cells, seq, rim, surface, surface_of, &mut out);
                return Ok(out);
            }
        }
    } else {
        0
    };
    let ix = |k: usize| (k + rot) % n;

    // Each anchor's runs, widened over the root clusters they meet.
    #[derive(Clone)]
    struct Run {
        anchor: usize,
        start: usize,
        end: usize,
        on_first: usize,
        on_last: usize,
    }
    let mut runs: Vec<Run> = Vec::new();
    for a in 0..anchors.len() {
        let mut k = 0;
        while k < n {
            if !on[ix(k)][a] {
                k += 1;
                continue;
            }
            let first = k;
            while k + 1 < n && on[ix(k + 1)][a] {
                k += 1;
            }
            let last = k;
            let mut touched: BTreeSet<usize> = (first..=last)
                .filter_map(|j| cells[ix(j)].cluster)
                .collect();
            let (mut s, mut e) = (first, last);
            let joins = |j: usize, touched: &BTreeSet<usize>| {
                let c = &cells[ix(j)];
                c.gap == Gap::Cluster || c.cluster.is_some_and(|x| touched.contains(&x))
            };
            loop {
                let mut grew = false;
                if s > 0 && joins(s - 1, &touched) {
                    s -= 1;
                    touched.extend(cells[ix(s)].cluster);
                    grew = true;
                }
                if e + 1 < n && joins(e + 1, &touched) {
                    e += 1;
                    touched.extend(cells[ix(e)].cluster);
                    grew = true;
                }
                if !grew {
                    break;
                }
            }
            runs.push(Run {
                anchor: a,
                start: s,
                end: e,
                on_first: first,
                on_last: last,
            });
            k = e.max(last) + 1;
        }
    }
    runs.sort_by(|x, y| x.start.cmp(&y.start).then(x.anchor.cmp(&y.anchor)));

    // Overlapping runs: cut, or carried by the first anchor.
    let cuttable = |k: usize| {
        let c = &cells[ix(k)];
        c.gap != Gap::Cluster && c.cluster.is_none()
    };
    let mut done: Vec<Run> = Vec::new();
    for r in runs {
        units(ledger, 1)?;
        let Some(cur) = done.last_mut() else {
            done.push(r);
            continue;
        };
        if r.start > cur.end {
            done.push(r);
            continue;
        }
        let lo = r.start.max(cur.on_first + 1);
        let hi = cur.end.min(r.on_last.saturating_sub(1));
        match (lo..=hi).find(|&k| cuttable(k)) {
            Some(k) => {
                cur.end = k - 1;
                cur.on_last = cur.on_last.min(k - 1);
                let mut r = r;
                r.start = k + 1;
                r.on_first = r.on_first.max(k + 1);
                done.push(r);
            }
            None => {
                let keep = if r.anchor < cur.anchor {
                    r.anchor
                } else {
                    cur.anchor
                };
                cur.anchor = keep;
                cur.end = cur.end.max(r.end);
                cur.on_first = cur.on_first.min(r.on_first);
                cur.on_last = cur.on_last.max(r.on_last);
            }
        }
    }

    for r in &done {
        let sign = |j: Option<usize>| j.map(|j| cells[ix(j)].gap);
        let before = if r.start > 0 {
            sign(Some(r.start - 1))
        } else if seq.cyclic {
            sign(Some(n - 1))
        } else {
            None
        };
        let after = if r.end + 1 < n {
            sign(Some(r.end + 1))
        } else if seq.cyclic {
            sign(Some(0))
        } else {
            None
        };
        let net = match (before, after) {
            (Some(Gap::Negative), Some(Gap::Positive)) => Some(1),
            (Some(Gap::Positive), Some(Gap::Negative)) => Some(-1),
            (Some(a), Some(b)) if a == b && a != Gap::Cluster => Some(0),
            _ => None,
        };
        let (c0, c1) = (&cells[ix(r.start)], &cells[ix(r.end)]);
        out.spans.push(AnchorSpan {
            anchor: anchors[r.anchor].id,
            targets: rim.gaps[seq.gaps[c0.g]].targets.clone(),
            from: (rim.gaps[seq.gaps[c0.g]].piece, c0.span()[0]),
            to: (rim.gaps[seq.gaps[c1.g]].piece, c1.span()[1]),
            net,
        });
        let cs: Vec<SpanCell> = (r.start..=r.end).map(|k| cells[ix(k)].clone()).collect();
        absorb(&cs, seq, rim, surface, surface_of, &mut out);
    }
    Ok(out)
}

/// The implicit of the target faces `targets`' surface.
fn surfaces_imp(input: &RimInput<'_>, targets: &[FaceId]) -> Option<Implicit> {
    let f = targets.first()?;
    input.targets.iter().find(|t| t.0 == *f).map(|t| t.1)
}

/// Marks the rim's root clusters and branch vertices inside a span's
/// `cells` absorbed.
fn absorb(
    cells: &[SpanCell],
    seq: &SeqJob,
    rim: &Rim,
    surface: Option<usize>,
    surface_of: &(dyn Fn(&[FaceId]) -> Option<usize> + Sync),
    out: &mut SpansOut,
) {
    let mut absorbed: BTreeSet<usize> = BTreeSet::new();
    for c in cells {
        absorbed.extend(c.cluster);
    }
    for (i, br) in rim.branches.iter().enumerate() {
        if surface_of(&br.targets) != surface {
            continue;
        }
        let inside = cells.iter().any(|c| {
            let piece = rim.gaps[seq.gaps[c.g]].piece;
            let [lo, hi] = c.span();
            piece == br.piece
                && lo.rational() <= br.at
                && br.at <= hi.rational()
                && (c.cluster.is_some_and(|x| absorbed.contains(&x)) || c.gap == Gap::Cluster)
        });
        if inside {
            out.absorbed_branches.insert(i);
        }
    }
    out.absorbed.extend(absorbed);
}

#[cfg(test)]
mod tests;

//! The snap along target edges: for every pair of an edge `h` of the
//! snapped side `S` and an edge `e` of the target `T` in the cluster's
//! window, which parts of `h` run along `e` and reuse it, and where `h`
//! crosses `e`, decided once, per **bracket**, after the corner rule has
//! placed the vertices ([`Corner`]), and never by a plain distance. It
//! never refines `b_K` and never escalates: every certificate is exact, and
//! a cell no class certifies is halved, to `MAX_SUBDIV_DEPTH`
//! ([`AlongError::OutOfBudget`], a resource cap).
//!
//! **Geometry.** The flat sheets: edges are the straight segments between
//! their end vertices' positions (their keys', as the corner rule reads
//! them), and the map carries `S` onto `T` along the common direction `ℓ`
//! ([`Map::Along`]; the closest-point map isn't taken here). `h`'s
//! **pre-snap** points are its segment's. Its **image** after the corner
//! rule runs straight between its **nodes**: its ends and the corner
//! rule's splits of `h` ([`Corner::splits`]), in order of their fraction
//! along `h`; a node is where the corner rule put it (an end placed on an
//! anchor: the anchor; on an edge: the exact point of the edge at the
//! stored fraction; a split: its anchor), else its pre-snap point. A point
//! and its image differ by a multiple of `ℓ`, which neither quantity below
//! reads, so the image needn't be projected onto `T`:
//! - the **side** of `e`: `(p − e₀)·(d × ℓ)`, `d` the edge's direction,
//!   zero exactly on the sheet through `e` along `ℓ`;
//! - the **place** along `e`: `((p − e₀) × ℓ)·(d × ℓ)/|d × ℓ|²`, the
//!   fraction from `e`'s stored start of the point of `e`'s line the image
//!   meets, if it meets it. The image is off `e` where the place lies
//!   outside `[0, 1]`.
//!
//! Both are affine in the point, so linear along each piece of `h`
//! between nodes.
//!
//! **Cells** ([`AlongPair::cells`]). Each piece of `h` (between two nodes
//! of distinct fractions) is cut into dyadic cells of its own parameter,
//! each certified as one of three classes, tried in this order:
//! 1. **vertex-clear** ([`CellClass::VertexClear`]): the cell ends at a
//!    node that is an end vertex `w` of `e` after the corner rule (a split
//!    of `h` at `w`, or an end of `h` placed on `w`), and the image leaves
//!    `w` off `e`: the side divided by the cell parameter from `w` (the
//!    zero at `w` is of order one on a straight piece) is nonzero, or the
//!    image leaves `w` along `e`'s line away from `e`;
//! 2. **near** ([`CellClass::Near`]): `h`'s pre-snap cell and `e` are
//!    graphs over a common interval of positive length of `e`'s range: the
//!    feet of the cell's ends on `e`'s line differ (the tangents within
//!    90°) and lie in `e`'s open range, but where the cell ends at a
//!    vertex of `e` it shares (there `h`'s pre-snap point may lie past
//!    `e`'s end: the common interval is the part in `[0, 1]`, of positive
//!    length), and over the common interval the cell lies within `b_K` of
//!    `e`'s line (at its two ends: the distance is convex);
//! 3. **clear** ([`CellClass::Clear`]) or **crossing**
//!    ([`CellClass::Crossing`]): the image is off `e` on the cell (its
//!    place on one side of `[0, 1]`, or its side one-signed), or the side
//!    changes sign once (at any angle: a straight piece's side is linear,
//!    its slope then nonzero) at a place strictly inside `e`, an ordinary
//!    crossing. A zero of the side at a cell's start belongs to the cell
//!    before it. One at an end of `h` (where the corner rule placed `h`'s
//!    vertex on `e`) is a meeting at the vertex, not a crossing: such a
//!    cell is left to the near class (its smaller cells certify near where
//!    the tangents are within 90°), and only at a right angle, where none
//!    can, is it off `e` but at the vertex. A sign change whose place is
//!    outside `[0, 1]` is off `e`; one at an end of `e` is left open (the
//!    corner rule shares `e`'s end vertices met by `h`, so this ends only
//!    at the depth limit).
//!
//! **Brackets.** A maximal run of near cells is a bracket. Its side is
//! read at the cells' ends, extended over the cell beside it where the
//! side is zero at the bracket's end (so a crossing at the end is the
//! bracket's). A bracket whose side changes sign once, strictly monotone
//! throughout, at a place strictly inside `e`, is an ordinary crossing
//! ([`AlongPair::crossings`]). Every other bracket (a run along `e`, a
//! touch, a non-transversal crossing, several within `b_K`) is a
//! **`SnapAlong` piece** ([`AlongPiece`]): `h` is split at the bracket's
//! ends and the piece reuses `e`.
//!
//! **Fast path.** Where the caller relates `h`'s and `e`'s forms
//! ([`AlongInput::same_form`]: one line), the part of `h` whose feet lie
//! in `e`'s range is one bracket, a `SnapAlong` piece, without cells; the
//! rest of `h` lies on `e`'s line beyond its ends, off `e`.
//!
//! **Bracket ends** ([`BracketEnd`]), by the corner rule: the pre-snap
//! point of `h` at the end joins, the first it is on (`R_on = max(b_K,
//! β_x + β_y)`, `β_x` the larger of `h`'s and `e`'s bands), a vertex of `e`
//! (in geometric order), else a branch vertex the corner rule keeps (in
//! the order of their points), else an `S` vertex the corner rule placed
//! on `e` (in geometric order, at its placed position); else it goes to
//! `e` at the clamped foot of the point, rounded to `f64` once (never a
//! mid-point). The ends are numbered with the pieces, in geometric order
//! along `h` from its lesser end ([`AlongPiece::index`], [`EndKind`]).
//!
//! **Order.** Pairs by their edges' keys; cells and crossings along `h`'s
//! stored parameter; pieces too, with their index from `h`'s lesser end.
//! Ids break exact ties only.
//!
//! **Work.** Charged on the operation's budget in passes: the branch
//! vertices (an item each) and the pairs (an item per pair, a unit per
//! cell test and per bracket-end candidate).

use std::collections::{BTreeMap, BTreeSet};

use malachite::Rational;
use varde_decide::bezier::Dyadic;
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::num::schedule::MAX_SUBDIV_DEPTH;
use varde_decide::order::{self, EdgeKey, EntityOrder, Pos, VertKey};
use varde_decide::records::keys::Operand;
pub use varde_decide::records::keys::SnapEnd as EndKind;

use super::corner::{
    Corner, Moved, Placement, d2, dist_up, dot3, exact3, larger, on_q, point_on, point_seg, sub3,
    to_f64,
};
use super::rim::{Element, Rim, RimInput};
use super::target::Map;
use crate::infra::budget::{Cap, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

type Q = Rational;
/// A point in space, exactly.
type P3 = [Q; 3];
/// The corner rule's splits of an edge: fraction, position, anchor.
type Splits = BTreeMap<EdgeId, Vec<(Q, P3, (Operand, VertId))>>;
/// Vertices with their keys.
type Keyed<K> = Vec<(VertKey<K>, VertId)>;

/// What [`snap_along`] reads besides the rim and the corner rule.
#[derive(Clone, Copy)]
pub struct AlongInput<'a, O> {
    /// The cluster's gap bound `b_K` (finite, `> 0`).
    pub b_k: f64,
    /// The operands' vertices and edges, as for the corner rule: their
    /// geometric order, positions and ends; edges straight.
    pub order: &'a O,
    /// The band of an operand's edge or vertex (finite, `≥ 0`).
    pub band: &'a (dyn Fn(Element) -> f64 + Sync),
    /// The cluster's map ([`Map::Along`]).
    pub map: &'a Map,
    /// Whether the forms of the snapped operand's edge `h` and the target
    /// edge `e` are related (one line): the fast path.
    pub same_form: &'a (dyn Fn(EdgeId, EdgeId) -> bool + Sync),
}

/// A cell's class (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellClass {
    /// It ends at a vertex `h` and `e` share, and is off `e` beside it.
    VertexClear,
    /// Its pre-snap part is within `b_K` of `e` over a common interval.
    Near,
    /// Its image is off `e`.
    Clear,
    /// Its image crosses `e` once (an ordinary crossing).
    Crossing,
}

/// A cell of a piece of `h`.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The piece: between [`AlongPair::nodes`] `piece` and `piece + 1`.
    pub piece: u32,
    /// Its span of the piece's parameter.
    pub span: [Dyadic; 2],
    /// Its class.
    pub class: CellClass,
}

/// An ordinary crossing of `h` and `e`.
#[derive(Clone, Debug, PartialEq)]
pub struct Crossing {
    /// The fraction along `h` from its stored start.
    pub s: Q,
    /// The fraction along `e` from its stored start, strictly inside.
    pub u: Q,
}

/// An end of a `SnapAlong` piece: a split of `h`, placed by the corner
/// rule (module docs). With `h`, `e`, the piece's index and
/// [`which`](Self::which), its identity.
#[derive(Clone, Debug, PartialEq)]
pub struct BracketEnd {
    /// Which end.
    pub which: EndKind,
    /// The fraction along `h` from its stored start.
    pub s: Q,
    /// Where it goes: a vertex ([`Placement::Vert`], a vertex of `e` or
    /// an `S` vertex placed on `e`), a branch vertex
    /// ([`Placement::Branch`], an index into [`Corner::branches`]), or
    /// `e` at a fraction ([`Placement::Edge`]); with its position (none
    /// for a branch vertex) and its displacement from `h`'s pre-snap
    /// point there.
    pub moved: Moved,
}

/// A `SnapAlong` piece: a bracket of `h` that reuses `e`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlongPiece {
    /// Its index among the pair's pieces, from `h`'s lesser end.
    pub index: u32,
    /// Its span of `h`'s fractions, increasing.
    pub span: [Q; 2],
    /// Its ends, at the span's two ends.
    pub ends: [BracketEnd; 2],
    /// Whether the forms' relation gave it (the fast path).
    pub by_form: bool,
}

/// The record of one pair (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct AlongPair {
    /// The snapped operand's edge.
    pub h: EdgeId,
    /// The target edge.
    pub e: EdgeId,
    /// The fractions along `h` of its nodes, increasing (none on the fast
    /// path).
    pub nodes: Vec<Q>,
    /// The cells, along `h`.
    pub cells: Vec<Cell>,
    /// The ordinary crossings, along `h`.
    pub crossings: Vec<Crossing>,
    /// The `SnapAlong` pieces, along `h`'s stored parameter.
    pub pieces: Vec<AlongPiece>,
}

/// The pairs of a cluster.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapAlong {
    /// One per pair of a snapped and a target edge of the window, neither
    /// of zero length, in the order of their edges' keys.
    pub pairs: Vec<AlongPair>,
}

impl SnapAlong {
    /// The pair of `h` and `e`.
    pub fn pair(&self, h: EdgeId, e: EdgeId) -> Option<&AlongPair> {
        self.pairs.iter().find(|p| p.h == h && p.e == e)
    }
}

/// Why the pairs can't be decided.
#[derive(Clone, Debug, PartialEq)]
pub enum AlongError {
    /// `b_K` isn't finite and `> 0`, or a band isn't finite and `≥ 0`.
    Band,
    /// A position isn't finite.
    Position,
    /// The map isn't along a direction, or the direction is along a target
    /// edge.
    Map,
    /// A branch vertex's face or tile isn't described.
    Face(FaceId),
    /// The budget ran out or a cell reached the depth limit
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The `SnapAlong` records of the cluster of `rim` (made from
/// `rim_input`), after its corner rule `corner` (module docs), charged to
/// `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn snap_along<O>(
    rim: &Rim,
    rim_input: &RimInput<'_>,
    corner: &Corner,
    input: &AlongInput<'_, O>,
    work: &mut Work,
) -> Result<SnapAlong, AlongError>
where
    O: EntityOrder + Sync,
    O::Key: Send + Sync,
{
    if !(input.b_k.is_finite() && input.b_k > 0.0) {
        return Err(AlongError::Band);
    }
    let Map::Along(ell) = input.map else {
        return Err(AlongError::Map);
    };
    let s_op = rim_input.snapped;
    let t_op = s_op.other();
    let geo = Geo {
        order: input.order,
        band: input.band,
    };

    // The branch vertices the corner rule keeps, in the order of their
    // points.
    let ix: Vec<u32> = (0..corner.branches.len() as u32).collect();
    let mut branches = run(work, "snap along: branches", &ix, |&i, ledger| {
        units(ledger, 1)?;
        let b = &corner.branches[i as usize];
        let piece = rim
            .pieces
            .get(b.piece as usize)
            .ok_or(Stop::Failed(AlongError::Position))?;
        let p = point_on(rim_input, piece.face, piece.tile, &b.uv)
            .ok_or(Stop::Failed(AlongError::Face(piece.face)))?;
        let beta = (rim_input.band)(piece.along);
        if !(beta.is_finite() && beta >= 0.0) {
            return Err(Stop::Failed(AlongError::Band));
        }
        let at =
            Pos::new(p.each_ref().map(to_f64)).map_err(|_| Stop::Failed(AlongError::Position))?;
        Ok(BranchAt { at, i, p, beta })
    })?;
    branches.sort_by(|x, y| x.at.cmp(&y.at).then(x.i.cmp(&y.i)));

    // The corner rule's outcomes the pairs read.
    let placed_at: BTreeMap<VertId, Moved> =
        corner.placed.iter().map(|p| (p.vert, p.moved)).collect();
    let mut placed_on: BTreeMap<EdgeId, Keyed<O::Key>> = BTreeMap::new();
    for p in &corner.placed {
        if let Placement::Edge { edge, .. } = p.moved.to {
            placed_on
                .entry(edge)
                .or_default()
                .push((input.order.vert_key(s_op, p.vert), p.vert));
        }
    }
    for vs in placed_on.values_mut() {
        order::sort(vs);
    }
    let placed_on: BTreeMap<EdgeId, Vec<VertId>> = placed_on
        .into_iter()
        .map(|(g, vs)| (g, vs.into_iter().map(|v| v.1).collect()))
        .collect();
    let mut splits_of: Splits = BTreeMap::new();
    for sp in &corner.splits {
        if !(sp.s.is_finite() && sp.pos.iter().all(|x| x.is_finite())) {
            return Err(AlongError::Position);
        }
        splits_of.entry(sp.edge).or_default().push((
            Q::try_from(sp.s).expect("a finite fraction"),
            exact3(sp.pos),
            sp.at,
        ));
    }

    // The pairs, by their edges' keys.
    let edges_of = |o: Operand| -> Vec<EdgeId> {
        let mut es: Vec<(EdgeKey<O::Key>, EdgeId)> = corner
            .window
            .iter()
            .filter_map(|el| match *el {
                Element::Edge(p, g) if p == o => Some((input.order.edge_key(o, g), g)),
                _ => None,
            })
            .collect();
        order::sort(&mut es);
        es.into_iter().map(|e| e.1).collect()
    };
    let (hs, es) = (edges_of(s_op), edges_of(t_op));
    let jobs: Vec<(EdgeId, EdgeId)> = hs
        .iter()
        .flat_map(|&h| es.iter().map(move |&e| (h, e)))
        .collect();
    let ctx = Ctx {
        b_k: input.b_k,
        s_op,
        t_op,
        geo: &geo,
        ell,
        same_form: input.same_form,
        placed_at: &placed_at,
        placed_on: &placed_on,
        splits_of: &splits_of,
        branches: &branches,
    };
    let pairs = run(work, "snap along: pairs", &jobs, |&(h, e), ledger| {
        ctx.pair(h, e, ledger)
    })?;
    Ok(SnapAlong {
        pairs: pairs.into_iter().flatten().collect(),
    })
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, AlongError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<AlongError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => AlongError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<AlongError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

/// A branch vertex as the bracket ends read it.
struct BranchAt {
    at: Pos,
    i: u32,
    p: P3,
    beta: f64,
}

/// The operands' geometry as the pairs read it.
struct Geo<'a, O> {
    order: &'a O,
    band: &'a (dyn Fn(Element) -> f64 + Sync),
}

impl<O: EntityOrder> Geo<'_, O> {
    fn pos(&self, o: Operand, v: VertId) -> Result<[f64; 3], AlongError> {
        let p = self.order.vert_key(o, v).pos().get();
        if p.iter().all(|x| x.is_finite()) {
            Ok(p)
        } else {
            Err(AlongError::Position)
        }
    }

    fn vert(&self, o: Operand, v: VertId) -> Result<P3, AlongError> {
        self.pos(o, v).map(exact3)
    }

    fn band(&self, e: Element) -> Result<f64, AlongError> {
        let b = (self.band)(e);
        if b.is_finite() && b >= 0.0 {
            Ok(b)
        } else {
            Err(AlongError::Band)
        }
    }
}

fn cross3(a: &P3, b: &P3) -> P3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn lerp(a: &P3, b: &P3, t: &Q) -> P3 {
    [0, 1, 2].map(|k| &a[k] + t * (&b[k] - &a[k]))
}

fn zero() -> Q {
    Q::from(0u32)
}

fn one() -> Q {
    Q::from(1u32)
}

/// `e`'s line as the pairs read it (module docs).
struct Frame {
    e0: P3,
    d: P3,
    dd: Q,
    ell: P3,
    /// `d × ℓ` and its squared length.
    n: P3,
    nn: Q,
}

impl Frame {
    fn side(&self, p: &P3) -> Q {
        dot3(&sub3(p, &self.e0), &self.n)
    }

    fn place(&self, p: &P3) -> Q {
        dot3(&cross3(&sub3(p, &self.e0), &self.ell), &self.n) / &self.nn
    }

    /// The fraction of `p`'s foot on `e`'s line.
    fn foot(&self, p: &P3) -> Q {
        dot3(&sub3(p, &self.e0), &self.d) / &self.dd
    }

    /// The squared distance from `p` to `e`'s line.
    fn line_d2(&self, p: &P3) -> Q {
        let r = sub3(p, &self.e0);
        let a = dot3(&r, &self.d);
        dot3(&r, &r) - &a * &a / &self.dd
    }
}

/// A node of `h` (module docs): its fraction, its image, and the vertex of
/// `e` it is, if any.
struct Node {
    s: Q,
    img: P3,
    shared: Option<VertId>,
}

/// A cell as the pair records it.
struct CellRec {
    cell: Cell,
    /// Its ends' fractions along `h`.
    s: [Q; 2],
    /// The side at its ends.
    side: [Q; 2],
    /// Its piece's ends' images, and its span of the piece's parameter.
    img: [P3; 2],
    t: [Q; 2],
    /// Its crossing, if it is one: the fraction along `h` and along `e`.
    crossing: Option<(Q, Q)>,
}

/// What a cell test found.
enum Found {
    Class(CellClass, Option<(Q, Q)>),
    Open,
}

/// The pieces' shared data for one pair.
struct Ctx<'a, O: EntityOrder> {
    b_k: f64,
    s_op: Operand,
    t_op: Operand,
    geo: &'a Geo<'a, O>,
    ell: &'a P3,
    same_form: &'a (dyn Fn(EdgeId, EdgeId) -> bool + Sync),
    placed_at: &'a BTreeMap<VertId, Moved>,
    placed_on: &'a BTreeMap<EdgeId, Vec<VertId>>,
    splits_of: &'a Splits,
    branches: &'a [BranchAt],
}

impl<O: EntityOrder> Ctx<'_, O> {
    /// The record of `h` against `e`; none if either has zero length.
    fn pair(
        &self,
        h: EdgeId,
        e: EdgeId,
        ledger: &mut Ledger<'_>,
    ) -> Result<Option<AlongPair>, Stop<AlongError>> {
        let fail = Stop::Failed;
        let (s_op, t_op) = (self.s_op, self.t_op);
        let h_ends = self.geo.order.ends(s_op, h);
        let e_ends = self.geo.order.ends(t_op, e);
        let ha = self.geo.vert(s_op, h_ends[0]).map_err(fail)?;
        let hb = self.geo.vert(s_op, h_ends[1]).map_err(fail)?;
        let e0 = self.geo.vert(t_op, e_ends[0]).map_err(fail)?;
        let e1 = self.geo.vert(t_op, e_ends[1]).map_err(fail)?;
        if ha == hb || e0 == e1 {
            return Ok(None);
        }
        let d = sub3(&e1, &e0);
        let n = cross3(&d, self.ell);
        let nn = dot3(&n, &n);
        if nn == 0u32 {
            return Err(fail(AlongError::Map));
        }
        let frame = Frame {
            dd: dot3(&d, &d),
            e0,
            d,
            ell: self.ell.clone(),
            n,
            nn,
        };
        let dir = order::Along::of(
            &self.geo.order.vert_key(s_op, h_ends[0]),
            &self.geo.order.vert_key(s_op, h_ends[1]),
        );
        let pre = |s: &Q| lerp(&ha, &hb, s);

        // The fast path: one bracket where `h`'s feet lie in `e`'s range.
        if (self.same_form)(h, e) {
            units(ledger, 1)?;
            let (u0, u1) = (frame.foot(&ha), frame.foot(&hb));
            if u0 != u1 {
                let sa = (zero() - &u0) / (&u1 - &u0);
                let sb = (one() - &u0) / (&u1 - &u0);
                let (lo, hi) = if sa < sb { (sa, sb) } else { (sb, sa) };
                let lo = if lo < 0u32 { zero() } else { lo };
                let hi = if hi > 1u32 { one() } else { hi };
                let mut pieces = Vec::new();
                if lo < hi {
                    pieces.push(self.piece(h, e, &frame, [lo, hi], 0, true, &pre, dir, ledger)?);
                }
                return Ok(Some(AlongPair {
                    h,
                    e,
                    nodes: Vec::new(),
                    cells: Vec::new(),
                    crossings: Vec::new(),
                    pieces,
                }));
            }
        }

        // The nodes.
        let end_node = |k: usize| -> Result<Node, AlongError> {
            let v = h_ends[k];
            let s = if k == 0 { zero() } else { one() };
            let (img, shared) = match self.placed_at.get(&v).map(|m| m.to) {
                Some(Placement::Vert(o, w)) => (
                    self.geo.vert(o, w)?,
                    (o == t_op && e_ends.contains(&w)).then_some(w),
                ),
                Some(Placement::Edge { edge, s: f }) => {
                    let [a, b] = self.geo.order.ends(t_op, edge);
                    let (a, b) = (self.geo.vert(t_op, a)?, self.geo.vert(t_op, b)?);
                    let f = Q::try_from(f).map_err(|_| AlongError::Position)?;
                    (lerp(&a, &b, &f), None)
                }
                _ => (if k == 0 { ha.clone() } else { hb.clone() }, None),
            };
            Ok(Node { s, img, shared })
        };
        let mut nodes = vec![end_node(0).map_err(fail)?];
        let mut mid: Vec<Node> = self
            .splits_of
            .get(&h)
            .map(|v| v.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|(s, p, (o, w))| Node {
                s: s.clone(),
                img: p.clone(),
                shared: (*o == t_op && e_ends.contains(w)).then_some(*w),
            })
            .collect();
        mid.sort_by(|x, y| x.s.cmp(&y.s));
        nodes.extend(mid);
        nodes.push(end_node(1).map_err(fail)?);
        let last = nodes.len() - 1;

        // The cells, piece by piece.
        let mut cells: Vec<CellRec> = Vec::new();
        for k in 0..last {
            let (a, b) = (&nodes[k], &nodes[k + 1]);
            if a.s >= b.s {
                continue;
            }
            let piece = PieceAt {
                k: k as u32,
                s: [a.s.clone(), b.s.clone()],
                pre: [pre(&a.s), pre(&b.s)],
                img: [a.img.clone(), b.img.clone()],
                shared: [a.shared, b.shared],
                h_end: [k == 0, k + 1 == last],
            };
            let mut stack: Vec<(u32, u64)> = vec![(0, 0)];
            while let Some((depth, index)) = stack.pop() {
                units(ledger, 1)?;
                let span = [
                    Dyadic::new(index, depth).expect("a cell of [0, 1]"),
                    Dyadic::new(index + 1, depth).expect("a cell of [0, 1]"),
                ];
                let t = span.map(|x| x.rational());
                match self.classify(&frame, &piece, &t, e_ends) {
                    Found::Class(class, crossing) => {
                        let img = t.each_ref().map(|t| lerp(&piece.img[0], &piece.img[1], t));
                        cells.push(CellRec {
                            cell: Cell {
                                piece: piece.k,
                                span,
                                class,
                            },
                            s: t.each_ref().map(|t| piece.at(t)),
                            side: img.each_ref().map(|p| frame.side(p)),
                            img: piece.img.clone(),
                            t,
                            crossing,
                        });
                    }
                    Found::Open => {
                        if depth >= MAX_SUBDIV_DEPTH {
                            return Err(Stop::Cap(Cap::SubdivisionDepth));
                        }
                        stack.push((depth + 1, 2 * index + 1));
                        stack.push((depth + 1, 2 * index));
                    }
                }
            }
        }

        // The brackets.
        let mut crossings: Vec<(Q, Q)> = Vec::new();
        let mut dropped: BTreeSet<usize> = BTreeSet::new();
        let mut spans: Vec<[Q; 2]> = Vec::new();
        let near = |c: &CellRec| c.cell.class == CellClass::Near;
        let beside = |c: &CellRec| matches!(c.cell.class, CellClass::Clear | CellClass::Crossing);
        let mut i = 0;
        while i < cells.len() {
            if !near(&cells[i]) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < cells.len() && near(&cells[j + 1]) {
                j += 1;
            }
            let (mut lo, mut hi) = (i, j);
            if cells[i].side[0] == 0u32 && i > 0 && beside(&cells[i - 1]) {
                lo = i - 1;
                dropped.insert(i - 1);
            }
            if cells[j].side[1] == 0u32 && j + 1 < cells.len() && beside(&cells[j + 1]) {
                hi = j + 1;
                dropped.insert(j + 1);
            }
            match bracket_crossing(&frame, &cells[lo..=hi]) {
                Some(x) => crossings.push(x),
                None => spans.push([cells[i].s[0].clone(), cells[j].s[1].clone()]),
            }
            i = j + 1;
        }
        for (k, c) in cells.iter().enumerate() {
            if let Some(x) = &c.crossing
                && !dropped.contains(&k)
            {
                crossings.push(x.clone());
            }
        }
        crossings.sort();
        crossings.dedup();

        let n = spans.len();
        let mut pieces = Vec::with_capacity(n);
        for (k, span) in spans.into_iter().enumerate() {
            let index = match dir {
                order::Along::Stored => k,
                order::Along::Reversed => n - 1 - k,
            };
            pieces.push(self.piece(h, e, &frame, span, index as u32, false, &pre, dir, ledger)?);
        }
        Ok(Some(AlongPair {
            h,
            e,
            nodes: nodes.into_iter().map(|n| n.s).collect(),
            cells: cells.into_iter().map(|c| c.cell).collect(),
            crossings: crossings
                .into_iter()
                .map(|(s, u)| Crossing { s, u })
                .collect(),
            pieces,
        }))
    }

    /// A cell's class (module docs), or open.
    fn classify(&self, frame: &Frame, piece: &PieceAt, t: &[Q; 2], e_ends: [VertId; 2]) -> Found {
        let img = t.each_ref().map(|t| lerp(&piece.img[0], &piece.img[1], t));

        // 1. Vertex-clear at a shared vertex the cell ends at.
        for (side, at_end) in [(0usize, t[0] == 0u32), (1, t[1] == 1u32)] {
            let Some(w) = piece.shared[side].filter(|_| at_end) else {
                continue;
            };
            let wp = &piece.img[side];
            let far = &img[1 - side];
            if frame.side(far) != 0u32 {
                return Found::Class(CellClass::VertexClear, None);
            }
            // Along `e`'s line: away from `e` past its end.
            let into = if w == e_ends[0] {
                frame.d.clone()
            } else {
                frame.d.each_ref().map(|x| -x)
            };
            if dot3(&sub3(far, wp), &into) < 0u32 {
                return Found::Class(CellClass::VertexClear, None);
            }
        }

        // 2. Near: graphs over a common interval within `b_K`.
        let p = t.each_ref().map(|t| lerp(&piece.pre[0], &piece.pre[1], t));
        let u = p.each_ref().map(|p| frame.foot(p));
        // Feet in `e`'s open range, but at a shared vertex the cell ends
        // at, where `h`'s pre-snap point may lie past `e`'s end.
        let at_shared = [
            t[0] == 0u32 && piece.shared[0].is_some(),
            t[1] == 1u32 && piece.shared[1].is_some(),
        ];
        let inside = |k: usize| at_shared[k] || (u[k] > 0u32 && u[k] < 1u32);
        if u[0] != u[1] && inside(0) && inside(1) {
            let (umin, umax) = if u[0] < u[1] {
                (&u[0], &u[1])
            } else {
                (&u[1], &u[0])
            };
            let lo = if *umin < 0u32 { zero() } else { umin.clone() };
            let hi = if *umax > 1u32 { one() } else { umax.clone() };
            if lo < hi {
                let b = Q::try_from(self.b_k).expect("a finite b_K");
                let b2 = &b * &b;
                let at = |x: &Q| {
                    let f = (x - &u[0]) / (&u[1] - &u[0]);
                    lerp(&p[0], &p[1], &f)
                };
                if frame.line_d2(&at(&lo)) <= b2 && frame.line_d2(&at(&hi)) <= b2 {
                    return Found::Class(CellClass::Near, None);
                }
            }
        }

        // 3. Clear or crossing.
        let a = img.each_ref().map(|p| frame.place(p));
        if (a[0] < 0u32 && a[1] < 0u32) || (a[0] > 1u32 && a[1] > 1u32) {
            return Found::Class(CellClass::Clear, None);
        }
        let sd = img.each_ref().map(|p| frame.side(p));
        let z = [sd[0] == 0u32, sd[1] == 0u32];
        if z[0] && z[1] {
            return Found::Open;
        }
        let h_end = [
            piece.h_end[0] && t[0] == 0u32,
            piece.h_end[1] && t[1] == 1u32,
        ];
        if (z[0] && h_end[0]) || (z[1] && h_end[1]) {
            // `h`'s vertex on `e`'s line, where the corner rule placed it:
            // a meeting at the vertex, not a crossing. Near where the
            // tangents are within 90° (tried first, on smaller cells);
            // at a right angle, off `e` but at the vertex.
            return if u[0] == u[1] {
                Found::Class(CellClass::Clear, None)
            } else {
                Found::Open
            };
        }
        if z[0] {
            // A zero the cell before holds.
            return Found::Class(CellClass::Clear, None);
        }
        if (sd[0] > 0u32) == (sd[1] > 0u32) && !z[1] {
            return Found::Class(CellClass::Clear, None);
        }
        // One sign change, at the zero of a linear side.
        let f = &sd[0] / (&sd[0] - &sd[1]);
        let at = lerp(&img[0], &img[1], &f);
        let place = frame.place(&at);
        if !(zero()..=one()).contains(&place) {
            return Found::Class(CellClass::Clear, None);
        }
        if place == 0u32 || place == 1u32 {
            return Found::Open;
        }
        let tc = &t[0] + &f * (&t[1] - &t[0]);
        Found::Class(CellClass::Crossing, Some((piece.at(&tc), place)))
    }

    /// The `SnapAlong` piece of `h` over `span` (module docs).
    #[expect(clippy::too_many_arguments, reason = "one pair's context")]
    fn piece(
        &self,
        h: EdgeId,
        e: EdgeId,
        frame: &Frame,
        span: [Q; 2],
        index: u32,
        by_form: bool,
        pre: &dyn Fn(&Q) -> P3,
        dir: order::Along,
        ledger: &mut Ledger<'_>,
    ) -> Result<AlongPiece, Stop<AlongError>> {
        let kinds = match dir {
            order::Along::Stored => [EndKind::Start, EndKind::End],
            order::Along::Reversed => [EndKind::End, EndKind::Start],
        };
        let mut ends = Vec::with_capacity(2);
        for (s, which) in span.iter().zip(kinds) {
            let moved = self.bracket_end(h, e, frame, &pre(s), ledger)?;
            ends.push(BracketEnd {
                which,
                s: s.clone(),
                moved,
            });
        }
        let ends: [BracketEnd; 2] = ends.try_into().expect("two ends");
        Ok(AlongPiece {
            index,
            span,
            ends,
            by_form,
        })
    }

    /// Where a bracket end at pre-snap point `p` goes (module docs).
    fn bracket_end(
        &self,
        h: EdgeId,
        e: EdgeId,
        frame: &Frame,
        p: &P3,
        ledger: &mut Ledger<'_>,
    ) -> Result<Moved, Stop<AlongError>> {
        let fail = Stop::Failed;
        let (s_op, t_op) = (self.s_op, self.t_op);
        let bx = larger(
            self.geo.band(Element::Edge(s_op, h)).map_err(fail)?,
            self.geo.band(Element::Edge(t_op, e)).map_err(fail)?,
        );
        let at_vert = |o: Operand, v: VertId, pos: [f64; 3]| {
            let dd = d2(p, &exact3(pos));
            (
                dd,
                Moved {
                    to: Placement::Vert(o, v),
                    pos: Some(pos),
                    displacement: 0.0,
                    storage: 0.0,
                },
            )
        };
        // The vertices of `e`, in geometric order.
        let mut ws: Vec<(VertKey<O::Key>, VertId)> = self
            .geo
            .order
            .ends(t_op, e)
            .into_iter()
            .map(|w| (self.geo.order.vert_key(t_op, w), w))
            .collect();
        order::sort(&mut ws);
        ws.dedup_by(|x, y| x.1 == y.1);
        for (_, w) in ws {
            units(ledger, 1)?;
            let bw = self.geo.band(Element::Vert(t_op, w)).map_err(fail)?;
            let (dd, m) = at_vert(t_op, w, self.geo.pos(t_op, w).map_err(fail)?);
            if on_q(&dd, self.b_k, bx, bw) {
                return Ok(Moved {
                    displacement: dist_up(&dd),
                    ..m
                });
            }
        }
        // The branch vertices.
        for b in self.branches {
            units(ledger, 1)?;
            let dd = d2(p, &b.p);
            if on_q(&dd, self.b_k, bx, b.beta) {
                return Ok(Moved {
                    to: Placement::Branch(b.i),
                    pos: None,
                    displacement: dist_up(&dd),
                    storage: 0.0,
                });
            }
        }
        // The `S` vertices placed on `e`.
        for &v in self
            .placed_on
            .get(&e)
            .map(|v| v.as_slice())
            .unwrap_or_default()
        {
            units(ledger, 1)?;
            let Some(pos) = self.placed_at.get(&v).and_then(|m| m.pos) else {
                continue;
            };
            if !pos.iter().all(|x| x.is_finite()) {
                return Err(fail(AlongError::Position));
            }
            let bv = self.geo.band(Element::Vert(s_op, v)).map_err(fail)?;
            let (dd, m) = at_vert(s_op, v, pos);
            if on_q(&dd, self.b_k, bx, bv) {
                return Ok(Moved {
                    displacement: dist_up(&dd),
                    ..m
                });
            }
        }
        // `e` at the clamped foot.
        let e1: P3 = [0, 1, 2].map(|k| &frame.e0[k] + &frame.d[k]);
        let (_, s) = point_seg(p, &frame.e0, &e1);
        let s = to_f64(&s);
        let sq = Q::try_from(s).expect("a fraction in [0, 1]");
        let on = lerp(&frame.e0, &e1, &sq);
        let pos = on.each_ref().map(to_f64);
        let at = exact3(pos);
        Ok(Moved {
            to: Placement::Edge { edge: e, s },
            pos: Some(pos),
            displacement: dist_up(&d2(p, &at)),
            storage: dist_up(&d2(&on, &at)),
        })
    }
}

/// A piece of `h` between two nodes.
struct PieceAt {
    k: u32,
    /// Its ends' fractions along `h`, pre-snap points and images.
    s: [Q; 2],
    pre: [P3; 2],
    img: [P3; 2],
    /// The vertex of `e` each end is, if any.
    shared: [Option<VertId>; 2],
    /// Whether each end is an end of `h`.
    h_end: [bool; 2],
}

impl PieceAt {
    /// The fraction along `h` at the piece's parameter `t`.
    fn at(&self, t: &Q) -> Q {
        &self.s[0] + t * (&self.s[1] - &self.s[0])
    }
}

/// The crossing a bracket's cells `cells` (with the cells beside it
/// where the side is zero at its ends) make, if they make one: one sign
/// change of the side, strictly monotone throughout, at a place strictly
/// inside `e` (module docs).
fn bracket_crossing(frame: &Frame, cells: &[CellRec]) -> Option<(Q, Q)> {
    let rising = cells[0].side[1] > cells[0].side[0];
    let mono = cells.iter().all(|c| {
        if rising {
            c.side[1] > c.side[0]
        } else {
            c.side[1] < c.side[0]
        }
    });
    if !mono {
        return None;
    }
    let mut changes = 0;
    let mut last: Option<bool> = None;
    let values = std::iter::once(&cells[0].side[0]).chain(cells.iter().map(|c| &c.side[1]));
    for v in values {
        if *v == 0u32 {
            continue;
        }
        let pos = *v > 0u32;
        if last.is_some_and(|l| l != pos) {
            changes += 1;
        }
        last = Some(pos);
    }
    if changes != 1 {
        return None;
    }
    // The zero, unique on a strictly monotone side.
    let c = cells.iter().find(|c| {
        let (a, b) = (&c.side[0], &c.side[1]);
        (*a <= 0u32 && *b >= 0u32) || (*a >= 0u32 && *b <= 0u32)
    })?;
    let f = &c.side[0] / (&c.side[0] - &c.side[1]);
    let t = &c.t[0] + &f * (&c.t[1] - &c.t[0]);
    let at = lerp(&c.img[0], &c.img[1], &t);
    let place = frame.place(&at);
    if place <= 0u32 || place >= 1u32 {
        return None;
    }
    let s = &c.s[0] + &f * (&c.s[1] - &c.s[0]);
    Some((s, place))
}

#[cfg(test)]
mod tests;

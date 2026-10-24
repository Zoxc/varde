//! The check's invariant 4 on linear trims: a face's loops are simple,
//! meet only at contact points without crossing, and nest, decided by
//! exact flat predicates on the stored UVs.
//!
//! **The loops as polygons.** A span of a p-curve is **straight** when it
//! is linear or iso, or a Q span whose three control points are collinear
//! with the middle one between the ends (its point then runs monotonely
//! from end to end, for any positive weights); a Q span whose middle point
//! lies past an end on their line runs back over itself. Each loop, walked
//! in its own direction, is then a closed polygon of its spans' ends: a
//! **node** per point where the walk stays (the consecutive span ends at
//! one point merged into one, so a run of zero-length edges is one node
//! holding the run's vertices in order; a node inside a coedge holds
//! none), a **segment** per span of positive length between them. A loop
//! whose spans all stay at one point (a whole loop of zero-length edges)
//! is one node and no segment.
//!
//! **Slits.** Two coedges of one loop on two edges of one edge contact,
//! sharing one p-curve and running it opposite ways, are a slit's sides:
//! their segments lie on each other, each one's reverse. Such a pair is
//! exempt from simplicity where the face's material lies on both sides of
//! it (the slit is empty, of zero area), which the rays round each of its
//! points show. A loop of slit sides and zero-length edges only bounds
//! zero area: a hole cut by a slit.
//!
//! **The checks** ([`TrimCheck`]), on each face, in order; each runs over
//! all of the face's loops and points before the next starts, so the
//! check failing first and the work charged up to it don't depend on ids:
//! 1. every span is straight and within the exact predicates' input range
//!    (else the face needs the curved part of the invariant, not built:
//!    [`Finding::Unbuilt`]);
//! 2. no Q span runs back over itself;
//! 3. the face has exactly one outer loop;
//! 4. no node's run of zero-length edges returns to a vertex;
//! 5. a loop passes a point once, but for the sides of a slit through it
//!    and for **one pinch**: one vertex, without a zero-length edge,
//!    passed twice;
//! 6. a pinch bounds non-zero area on both sides (each of the two lobes
//!    the loop splits into there);
//! 7. no trims cross ([`Finding::Crossing`]): two segments of the face
//!    meet only at a point both end at, and at each such point the rays
//!    of the segments leaving it and arriving at it alternate round it,
//!    leaving and arriving (the material between a leaving ray and the
//!    next arriving one, counter-clockwise), but for a slit's sides, which
//!    lie on one ray inside the material; no two other rays coincide, and
//!    two loops meet only at nodes of vertices;
//! 8. the vertices at one point are one **contact point**: one vertex,
//!    vertices joined by zero-length edges, or members of one vertex
//!    contact;
//! 9. the outer loop runs counter-clockwise in the face's orientation
//!    (clockwise in UV on a flipped face), each hole the other way, by the
//!    exact sign of its area; a hole of zero area is a slit's;
//! 10. each hole lies in the outer loop and in no other hole, tested from
//!     the middle of its least segment (by its end points), a point off
//!     the other loops and off a pinch's vertex.
//!
//! At a pinch the alternation of 7 makes the two lobes either apart and
//! turning one way or one inside the other and turning opposite ways,
//! and with 9 the lobe outside turns the loop's way.
//!
//! **Work.** Every face is charged one unit, one per coedge and span read,
//! one per vertex visited joining a contact point (with the contacts
//! scanned) and per slit pair compared; each pair of segments a box test
//! ([`BvhUnits::test`](varde_decide::num::schedule::BvhUnits::test)
//! parts), each exact predicate the parts of its stages
//! ([`RecordUnits`](varde_decide::num::schedule::RecordUnits)), rounded
//! up to a unit once per face. The segments are tested pairwise, a face's
//! cost quadratic in its spans.
//!
//! **Reading a broken body.** Where the face's loops or a loop's coedges
//! don't walk (an id naming nothing, a list that doesn't close), the face
//! passes: the topology's to refuse.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ops::Range;

use varde_decide::num::exact::{MAX_INPUT, Num, Pred, value_sign};
use varde_decide::num::preds::{Orient2d, orient2, polygon_area_sign};
use varde_decide::num::schedule::{PredKind, UNIT_PARTS, UNITS};

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::infra::budget::{Ledger, Step, Stop};
use crate::topo::brep::{Brep, CoedgeId, ContactKind, EdgeId, EdgeKind, FaceId, Member, VertId};

/// The checks of invariant 4, numbered in the hook's order (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum TrimCheck {
    /// A span curved or beyond the predicates' range: not built.
    Linear = 1,
    /// A Q span running back over itself.
    SpanFold,
    /// Not exactly one outer loop.
    OuterLoops,
    /// A run of zero-length edges returning to a vertex.
    ZeroLengthReturn,
    /// A loop passing a point again, other than a slit or one pinch.
    Revisit,
    /// A pinch with a lobe of zero area.
    PinchArea,
    /// Trims crossing or meeting other than at a contact point.
    Crossing,
    /// The vertices at one point not one contact point.
    Contact,
    /// A loop turning the wrong way.
    Orientation,
    /// A hole outside the outer loop or inside another hole.
    Nesting,
}

impl TrimCheck {
    fn finding(self) -> Finding {
        match self {
            TrimCheck::Linear => Finding::Unbuilt,
            TrimCheck::Crossing => Finding::Crossing,
            _ => Finding::Defect,
        }
    }
}

type Res<T> = Result<T, Stop<Fault>>;

/// Fails with `check` unless `ok`.
fn need(ok: bool, check: TrimCheck) -> Res<()> {
    if ok {
        Ok(())
    } else {
        Err(Stop::Failed(Fault {
            check: check as u32,
            finding: check.finding(),
        }))
    }
}

/// Invariant 4 on linear trims (module docs).
pub(crate) struct Trims;

impl Invariant for Trims {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Face]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Res<()> {
        let Item::Face(f) = item else {
            return Ok(());
        };
        let mut m = Meter { ledger, parts: 0 };
        m.units(1)?;
        match face(cx, f, &mut m) {
            Err(Stop::Exhausted(e)) => Err(Stop::Exhausted(e)),
            done => {
                m.finish()?;
                done
            }
        }
    }
}

/// A ledger with the parts of a unit charged so far.
struct Meter<'l, 'p> {
    ledger: &'l mut Ledger<'p>,
    parts: u64,
}

impl Meter<'_, '_> {
    /// Charges `n` whole units.
    fn units(&mut self, n: usize) -> Res<()> {
        self.ledger
            .charge(Step::Work, u64::try_from(n).unwrap_or(u64::MAX))?;
        Ok(())
    }

    /// Charges `p` parts of a unit: the whole units they complete now, the
    /// rest with the next parts or at the item's end.
    fn parts(&mut self, p: u64) -> Res<()> {
        self.parts = self.parts.saturating_add(p);
        let whole = self.parts / UNIT_PARTS;
        self.parts %= UNIT_PARTS;
        if whole > 0 {
            self.ledger.charge(Step::Work, whole)?;
        }
        Ok(())
    }

    /// The parts left over, rounded up to a unit.
    fn finish(&mut self) -> Res<()> {
        if self.parts > 0 {
            self.parts = 0;
            self.ledger.charge(Step::Work, 1)?;
        }
        Ok(())
    }

    /// The exact sign of `pred`, a form of `terms` terms of its kind: its
    /// filter charged before, any further stage after, each per term.
    fn sign_terms(&mut self, pred: &impl Pred, terms: u64) -> Res<Ordering> {
        let filter = UNITS.record.of(pred.kind()).filter.saturating_mul(terms);
        self.parts(filter)?;
        let (s, w) = value_sign(pred);
        let all = w.parts(&UNITS.record).saturating_mul(terms);
        self.parts(all.saturating_sub(filter))?;
        // Every input is finite and within range (check 1).
        debug_assert!(s.is_ok(), "a finite input");
        Ok(s.unwrap_or(Ordering::Equal))
    }

    /// The exact sign of `pred`.
    fn sign(&mut self, pred: &impl Pred) -> Res<Ordering> {
        self.sign_terms(pred, 1)
    }

    /// The exact sign of the polygon `pts`' area, positive
    /// counter-clockwise.
    fn area(&mut self, pts: &[[f64; 2]]) -> Res<Ordering> {
        let (s, w) = polygon_area_sign(pts);
        self.parts(w.parts(&UNITS.record))?;
        debug_assert!(s.is_ok(), "a finite input");
        Ok(s.unwrap_or(Ordering::Equal))
    }
}

/// `2·orient2(a, b, (c + d)/2)`: the side of `a → b` the middle of `c`
/// and `d` lies on, two terms of [`PredKind::Orient2`].
struct MidOrient {
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    d: [f64; 2],
}

impl Pred for MidOrient {
    fn eval<N: Num>(&self) -> N {
        let p = |x: [f64; 2]| x.map(N::lit);
        let (a, b) = (p(self.a), p(self.b));
        orient2(&a, &b, &p(self.c)).add(&orient2(&a, &b, &p(self.d)))
    }

    fn kind(&self) -> PredKind {
        PredKind::Orient2
    }
}

/// `2y − (c + d)`: whether `y` lies above the middle of `c` and `d`.
struct MidAbove {
    y: f64,
    c: f64,
    d: f64,
}

impl Pred for MidAbove {
    fn eval<N: Num>(&self) -> N {
        let y = N::lit(self.y);
        y.add(&y).sub(&N::lit(self.c).add(&N::lit(self.d)))
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

/// Whether two UV points are one point (`−0` as `+0`).
fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    a[0] == b[0] && a[1] == b[1]
}

/// A total order on UV points, `−0` as `+0`.
fn point_cmp(a: [f64; 2], b: [f64; 2]) -> Ordering {
    let n = |x: f64| x + 0.0;
    n(a[0])
        .total_cmp(&n(b[0]))
        .then(n(a[1]).total_cmp(&n(b[1])))
}

/// Whether `p` lies in the closed box of `a` and `b`.
fn in_box(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    (0..2).all(|k| {
        let (lo, hi) = if a[k] <= b[k] {
            (a[k], b[k])
        } else {
            (b[k], a[k])
        };
        lo <= p[k] && p[k] <= hi
    })
}

/// A span's point when the loop reaches it, its vertex if it starts a
/// coedge, its coedge and the span in the loop's direction.
struct Entry {
    at: [f64; 2],
    vert: Option<VertId>,
    coedge: CoedgeId,
    span: PSpan,
}

/// A loop as read: whether outer, its spans in its direction.
struct RawLoop {
    outer: bool,
    entries: Vec<Entry>,
}

/// A point where a loop stays: its vertices in the loop's order (none
/// inside a coedge), its loop and its place in it.
struct Node {
    at: [f64; 2],
    verts: Vec<VertId>,
    lp: usize,
    k: usize,
}

/// A segment of a loop, from `a` to `b`, of positive length: its coedge
/// and loop.
struct Seg {
    a: [f64; 2],
    b: [f64; 2],
    coedge: CoedgeId,
    lp: usize,
}

/// A loop as a polygon: its nodes and segments in the face's lists,
/// segment `k` from node `k` to node `k + 1` (none for a loop of one
/// node).
struct Poly {
    outer: bool,
    nodes: Range<usize>,
    segs: Range<usize>,
}

/// A face's loops as polygons (module docs).
struct Polys {
    loops: Vec<Poly>,
    nodes: Vec<Node>,
    segs: Vec<Seg>,
}

impl Polys {
    fn new(raw: Vec<RawLoop>) -> Polys {
        let mut p = Polys {
            loops: Vec::new(),
            nodes: Vec::new(),
            segs: Vec::new(),
        };
        for (lp, l) in raw.into_iter().enumerate() {
            // Nodes as (point, vertices, the coedge leaving it).
            let mut nodes: Vec<([f64; 2], Vec<VertId>, CoedgeId)> = Vec::new();
            for e in &l.entries {
                match nodes.last_mut() {
                    Some(n) if same(n.0, e.at) => {
                        n.1.extend(e.vert);
                        n.2 = e.coedge;
                    }
                    _ => nodes.push((e.at, e.vert.into_iter().collect(), e.coedge)),
                }
            }
            if nodes.len() > 1 && same(nodes[0].0, nodes[nodes.len() - 1].0) {
                let last = nodes.pop().expect("two nodes");
                let first = &mut nodes[0];
                first.1 = last.1.into_iter().chain(first.1.drain(..)).collect();
            }
            let n = nodes.len();
            let node0 = p.nodes.len();
            let seg0 = p.segs.len();
            if n > 1 {
                for k in 0..n {
                    p.segs.push(Seg {
                        a: nodes[k].0,
                        b: nodes[(k + 1) % n].0,
                        coedge: nodes[k].2,
                        lp,
                    });
                }
            }
            for (k, (at, verts, _)) in nodes.into_iter().enumerate() {
                p.nodes.push(Node { at, verts, lp, k });
            }
            p.loops.push(Poly {
                outer: l.outer,
                nodes: node0..p.nodes.len(),
                segs: seg0..p.segs.len(),
            });
        }
        p
    }

    /// Node `n`'s arriving and leaving segments, if its loop has any.
    fn node_segs(&self, n: usize) -> Option<(usize, usize)> {
        let node = &self.nodes[n];
        let l = &self.loops[node.lp];
        let len = l.segs.len();
        (len > 0).then(|| {
            (
                l.segs.start + (node.k + len - 1) % len,
                l.segs.start + node.k,
            )
        })
    }

    /// Loop `l`'s node points, in order.
    fn points(&self, l: usize) -> Vec<[f64; 2]> {
        self.nodes[self.loops[l].nodes.clone()]
            .iter()
            .map(|n| n.at)
            .collect()
    }
}

/// The groups of `ids` at one point each, by `at`.
fn groups(ids: Vec<usize>, at: impl Fn(usize) -> [f64; 2]) -> Vec<Vec<usize>> {
    let mut ids = ids;
    ids.sort_by(|&x, &y| point_cmp(at(x), at(y)).then(x.cmp(&y)));
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in ids {
        match out.last_mut() {
            Some(g) if same(at(g[0]), at(i)) => g.push(i),
            _ => out.push(vec![i]),
        }
    }
    out
}

/// A coedge's vertex at its start, in its own direction; `None` where an
/// id names nothing.
fn start_vert(b: &Brep, c: CoedgeId) -> Option<VertId> {
    let e = *b.coedges.edge.get(c.index())?;
    let rev = *b.coedges.reversed.get(c.index())?;
    let v = if rev {
        *b.edges.v_end.get(e.index())?
    } else {
        *b.edges.v_start.get(e.index())?
    };
    (v.index() < b.vert_count()).then_some(v)
}

/// A coedge's p-curve, `None` where an id names nothing.
fn pcurve(b: &Brep, c: CoedgeId) -> Option<&PCurve> {
    b.pcurves.get(b.coedges.pcurve.get(c.index())?.index())
}

/// The face's loops read in their directions; `None` where a list
/// doesn't walk (an id naming nothing, a list not closing).
fn read(cx: &Cx<'_>, f: FaceId, m: &mut Meter<'_, '_>) -> Res<Option<Vec<RawLoop>>> {
    let b = cx.body();
    let ls = &b.loops;
    let cs = &b.coedges;
    let count = cx.inc.face_loops[f.index()];
    let mut out = Vec::new();
    let mut l = b.faces.first_loop[f.index()];
    while !l.is_none() {
        if out.len() == count || ls.face.get(l.index()) != Some(&f) {
            return Ok(None);
        }
        let size = cx.inc.loop_sizes[l.index()];
        if size == 0 {
            return Ok(None);
        }
        m.units(size)?;
        let first = ls.first_coedge[l.index()];
        let mut entries = Vec::new();
        let mut c = first;
        for _ in 0..size {
            if cs.lp.get(c.index()) != Some(&l) {
                return Ok(None);
            }
            let (Some(p), Some(v)) = (pcurve(b, c), start_vert(b, c)) else {
                return Ok(None);
            };
            m.units(p.spans().len())?;
            let rev = cs.reversed[c.index()];
            let spans: Vec<PSpan> = if rev {
                p.spans().iter().rev().map(PSpan::reversed).collect()
            } else {
                p.spans().to_vec()
            };
            for (k, span) in spans.into_iter().enumerate() {
                entries.push(Entry {
                    at: span.start(),
                    vert: (k == 0).then_some(v),
                    coedge: c,
                    span,
                });
            }
            c = cs.next[c.index()];
        }
        if c != first {
            return Ok(None);
        }
        out.push(RawLoop {
            outer: ls.is_outer[l.index()],
            entries,
        });
        l = ls.next_loop[l.index()];
    }
    Ok(Some(out))
}

/// What a span is as a trim (module docs).
enum Shape {
    Straight,
    Fold,
    Unbuilt,
}

fn shape(s: &PSpan, m: &mut Meter<'_, '_>) -> Res<Shape> {
    let in_range = s
        .coords()
        .iter()
        .all(|c| c.values().iter().all(|x| x[0].abs() <= MAX_INPUT));
    if !in_range {
        return Ok(Shape::Unbuilt);
    }
    Ok(match s {
        PSpan::Linear(_) | PSpan::Iso { .. } => Shape::Straight,
        PSpan::G { .. } => Shape::Unbuilt,
        PSpan::Q(q) => {
            let [a, mid, z] = q.map(|p| [p[0], p[1]]);
            match m.sign(&Orient2d([a, mid, z]))? {
                Ordering::Equal if in_box(a, z, mid) => Shape::Straight,
                Ordering::Equal => Shape::Fold,
                _ => Shape::Unbuilt,
            }
        }
    })
}

/// Whether edges `e1` and `e2` are members of one edge contact.
fn edge_contact(b: &Brep, e1: EdgeId, e2: EdgeId, m: &mut Meter<'_, '_>) -> Res<bool> {
    m.units(b.contacts.len())?;
    Ok(b.contacts.iter().any(|c| {
        let has = |e: EdgeId| c.members.iter().flatten().any(|x| *x == Member::Edge(e));
        c.kind == ContactKind::Edge && has(e1) && has(e2)
    }))
}

/// Whether segments `s` and `t` are a slit's sides (module docs): each
/// the other's reverse, on coedges of one loop on two edges of one edge
/// contact, sharing one p-curve and running it opposite ways.
fn slit(b: &Brep, s: &Seg, t: &Seg, m: &mut Meter<'_, '_>) -> Res<bool> {
    if !(same(s.a, t.b) && same(s.b, t.a) && s.lp == t.lp && s.coedge != t.coedge) {
        return Ok(false);
    }
    m.units(1)?;
    let cs = &b.coedges;
    let (i, j) = (s.coedge.index(), t.coedge.index());
    let (e1, e2) = (cs.edge[i], cs.edge[j]);
    let shared = match (pcurve(b, s.coedge), pcurve(b, t.coedge)) {
        (Some(p), Some(q)) => {
            m.units(p.spans().len())?;
            p == q
        }
        _ => false,
    };
    if e1 == e2 || cs.reversed[i] == cs.reversed[j] || !shared {
        return Ok(false);
    }
    edge_contact(b, e1, e2, m)
}

/// Whether an edge has length zero: not singular, its curve one point.
fn zero_length(b: &Brep, e: EdgeId) -> bool {
    let i = e.index();
    if b.edges.kind.get(i).is_none_or(|&k| k == EdgeKind::Singular) {
        return false;
    }
    let Some(curve) = (b.edges.curve.get(i)).and_then(|c| b.curves3.get(c.index())) else {
        return false;
    };
    let mut points = curve.spans().iter().flatten();
    let Some(first) = points.next() else {
        return false;
    };
    points.all(|p| (0..3).all(|k| p[k] == first[k]))
}

/// Whether `verts` (two or more) are one contact point: joined by
/// zero-length edges and vertex contacts.
fn one_point(cx: &Cx<'_>, verts: &[VertId], m: &mut Meter<'_, '_>) -> Res<bool> {
    let b = cx.body();
    let mut seen = BTreeSet::from([verts[0]]);
    let mut todo = vec![verts[0]];
    while let Some(v) = todo.pop() {
        let edges = &cx.inc.vert_edges[v.index()];
        m.units(1 + edges.len() + b.contacts.len())?;
        let mut next: Vec<VertId> = Vec::new();
        for &e in edges {
            if zero_length(b, e) {
                next.extend([b.edges.v_start[e.index()], b.edges.v_end[e.index()]]);
            }
        }
        for c in &b.contacts {
            let members = || c.members.iter().flatten();
            if c.kind == ContactKind::Vertex && members().any(|x| *x == Member::Vert(v)) {
                next.extend(members().filter_map(|x| match *x {
                    Member::Vert(w) => Some(w),
                    _ => None,
                }));
            }
        }
        for w in next {
            if w.index() < b.vert_count() && seen.insert(w) {
                todo.push(w);
            }
        }
    }
    Ok(verts.iter().all(|v| seen.contains(v)))
}

/// The order of the rays from `p` to `x` and to `y` counter-clockwise
/// from the direction `+u`; `Equal` for one direction.
fn ray_cmp(p: [f64; 2], x: [f64; 2], y: [f64; 2], m: &mut Meter<'_, '_>) -> Res<Ordering> {
    let half = |q: [f64; 2]| !(q[1] > p[1] || (q[1] == p[1] && q[0] > p[0]));
    let by_half = half(x).cmp(&half(y));
    if by_half != Ordering::Equal {
        return Ok(by_half);
    }
    Ok(m.sign(&Orient2d([p, x, y]))?.reverse())
}

/// A ray from a point: towards `to`, along a segment leaving the point
/// (`out`) or arriving at it.
struct Ray {
    to: [f64; 2],
    out: bool,
    seg: usize,
}

/// Whether the segments at `nodes`, all at one point, meet there without
/// crossing (check 7, module docs).
fn rays_alternate(polys: &Polys, nodes: &[usize], b: &Brep, m: &mut Meter<'_, '_>) -> Res<bool> {
    let p = polys.nodes[nodes[0]].at;
    let mut rays = Vec::new();
    for &n in nodes {
        if let Some((arriving, leaving)) = polys.node_segs(n) {
            rays.push(Ray {
                to: polys.segs[leaving].b,
                out: true,
                seg: leaving,
            });
            rays.push(Ray {
                to: polys.segs[arriving].a,
                out: false,
                seg: arriving,
            });
        }
    }
    let k = rays.len();
    // Each ray's rank: how many rays come before it; one direction, one
    // rank. All pairs are compared, whatever the ids' order.
    let mut rank = vec![0usize; k];
    for i in 0..k {
        for j in i + 1..k {
            match ray_cmp(p, rays[i].to, rays[j].to, m)? {
                Ordering::Less => rank[j] += 1,
                Ordering::Greater => rank[i] += 1,
                Ordering::Equal => {}
            }
        }
    }
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&i| (rank[i], i));
    let mut ok = true;
    let mut plain: Vec<usize> = Vec::new();
    let mut slits: Vec<usize> = Vec::new();
    let mut at = 0;
    while at < k {
        let mut end = at + 1;
        while end < k && rank[order[end]] == rank[order[at]] {
            end += 1;
        }
        match order[at..end] {
            [i] => plain.push(i),
            [i, j] if rays[i].out != rays[j].out => {
                let (o, a) = if rays[i].out { (i, j) } else { (j, i) };
                let (so, sa) = (&polys.segs[rays[o].seg], &polys.segs[rays[a].seg]);
                if slit(b, so, sa, m)? {
                    slits.push(rank[i]);
                } else {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        at = end;
    }
    let n = plain.len();
    for i in 0..n {
        if rays[plain[i]].out == rays[plain[(i + 1) % n]].out {
            ok = false;
        }
    }
    for &q in &slits {
        let before = (plain.iter().rev())
            .find(|&&r| rank[r] < q)
            .or(plain.last());
        if before.is_some_and(|&r| !rays[r].out) {
            ok = false;
        }
    }
    let inside_coedge = nodes.iter().any(|&n| polys.nodes[n].verts.is_empty());
    if nodes.len() > 1 && inside_coedge && n > 0 {
        ok = false;
    }
    Ok(ok)
}

/// Whether the closed segments `a b` and `c d` share a point, exactly.
fn segments_meet([a, b]: [[f64; 2]; 2], [c, d]: [[f64; 2]; 2], m: &mut Meter<'_, '_>) -> Res<bool> {
    let o1 = m.sign(&Orient2d([a, b, c]))?;
    let o2 = m.sign(&Orient2d([a, b, d]))?;
    let o3 = m.sign(&Orient2d([c, d, a]))?;
    let o4 = m.sign(&Orient2d([c, d, b]))?;
    let opposite = |x: Ordering, y: Ordering| x != Ordering::Equal && x == y.reverse();
    if opposite(o1, o2) && opposite(o3, o4) {
        return Ok(true);
    }
    let eq = Ordering::Equal;
    Ok((o1 == eq && in_box(a, b, c))
        || (o2 == eq && in_box(a, b, d))
        || (o3 == eq && in_box(c, d, a))
        || (o4 == eq && in_box(c, d, b)))
}

/// Whether the closed boxes of two segments meet.
fn boxes_meet(s: &Seg, t: &Seg) -> bool {
    (0..2).all(|k| {
        let lo = |x: f64, y: f64| if x <= y { x } else { y };
        let hi = |x: f64, y: f64| if x <= y { y } else { x };
        lo(s.a[k], s.b[k]) <= hi(t.a[k], t.b[k]) && lo(t.a[k], t.b[k]) <= hi(s.a[k], s.b[k])
    })
}

/// Whether the middle of `c` and `d`, on no segment of the polygon
/// `poly`, lies inside it: the parity of its crossings with the ray from
/// it along `+u`, each decided exactly.
fn inside(c: [f64; 2], d: [f64; 2], poly: &[[f64; 2]], m: &mut Meter<'_, '_>) -> Res<bool> {
    let mut above = Vec::with_capacity(poly.len());
    for a in poly {
        above.push(
            m.sign(&MidAbove {
                y: a[1],
                c: c[1],
                d: d[1],
            })? == Ordering::Greater,
        );
    }
    let mut odd = false;
    let n = poly.len();
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        if above[k] == above[(k + 1) % n] {
            continue;
        }
        // The crossing lies right of the middle where it is left of the
        // upward-directed segment.
        let o = m.sign_terms(&MidOrient { a, b, c, d }, 2)?;
        let up = b[1] > a[1];
        if (up && o == Ordering::Greater) || (!up && o == Ordering::Less) {
            odd = !odd;
        }
    }
    Ok(odd)
}

/// Invariant 4 on face `f` (module docs).
fn face(cx: &Cx<'_>, f: FaceId, m: &mut Meter<'_, '_>) -> Res<()> {
    use TrimCheck as T;
    let b = cx.body();
    let Some(raw) = read(cx, f, m)? else {
        return Ok(());
    };
    let (mut unbuilt, mut fold) = (false, false);
    for e in raw.iter().flat_map(|l| &l.entries) {
        match shape(&e.span, m)? {
            Shape::Straight => {}
            Shape::Fold => fold = true,
            Shape::Unbuilt => unbuilt = true,
        }
    }
    need(!unbuilt, T::Linear)?;
    need(!fold, T::SpanFold)?;
    need(raw.iter().filter(|l| l.outer).count() == 1, T::OuterLoops)?;
    let polys = Polys::new(raw);
    let returns = polys.nodes.iter().any(|n| {
        let mut v = n.verts.clone();
        v.sort_unstable();
        v.dedup();
        v.len() < n.verts.len()
    });
    need(!returns, T::ZeroLengthReturn)?;
    let pinches = revisits(&polys, b, m)?;
    need(pinches.is_some(), T::Revisit)?;
    let mut flat = false;
    for (l, i, j) in pinches.into_iter().flatten() {
        let pts = polys.points(l);
        let lobe: Vec<[f64; 2]> = pts[j..].iter().chain(&pts[..i]).copied().collect();
        flat |= m.area(&pts[i..j])? == Ordering::Equal;
        flat |= m.area(&lobe)? == Ordering::Equal;
    }
    need(!flat, T::PinchArea)?;
    need(crossings(&polys, b, m)?, T::Crossing)?;
    let mut apart = false;
    for g in groups((0..polys.nodes.len()).collect(), |n| polys.nodes[n].at) {
        let mut verts: Vec<VertId> = g
            .iter()
            .flat_map(|&n| polys.nodes[n].verts.iter().copied())
            .collect();
        verts.sort_unstable();
        verts.dedup();
        if verts.len() > 1 && !one_point(cx, &verts, m)? {
            apart = true;
        }
    }
    need(!apart, T::Contact)?;
    need(oriented(&polys, b.flipped(f), b, m)?, T::Orientation)?;
    need(nested(&polys, m)?, T::Nesting)
}

/// Check 5: each loop's pinch, as the loop and its two nodes there in
/// the loop's order; `None` if a loop passes a point again otherwise.
fn revisits(
    polys: &Polys,
    b: &Brep,
    m: &mut Meter<'_, '_>,
) -> Res<Option<Vec<(usize, usize, usize)>>> {
    let mut pinches = Vec::new();
    let mut ok = true;
    for (l, lp) in polys.loops.iter().enumerate() {
        let mut found = 0;
        for g in groups(lp.nodes.clone().collect(), |n| polys.nodes[n].at) {
            if g.len() < 2 {
                continue;
            }
            // The two sides of a slit passing the point are no visit.
            let mut left: Vec<usize> = Vec::new();
            let mut matched = vec![false; g.len()];
            for x in 0..g.len() {
                if matched[x] {
                    continue;
                }
                let mut partner = None;
                for y in x + 1..g.len() {
                    if !matched[y] && slit_through(polys, g[x], g[y], b, m)? {
                        partner = Some(y);
                        break;
                    }
                }
                match partner {
                    Some(y) => matched[y] = true,
                    None => left.push(g[x]),
                }
            }
            match left[..] {
                [] | [_] => {}
                [x, y] => {
                    let (vx, vy) = (&polys.nodes[x].verts, &polys.nodes[y].verts);
                    if vx.len() == 1 && vx == vy {
                        found += 1;
                        pinches.push((l, polys.nodes[x].k, polys.nodes[y].k));
                    } else {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
        }
        if found > 1 {
            ok = false;
        }
    }
    Ok(ok.then_some(pinches))
}

/// Whether nodes `x` and `y` of one loop at one point are the two sides
/// of a slit passing it: each one's arriving segment the other's leaving
/// one reversed, both slit sides.
fn slit_through(polys: &Polys, x: usize, y: usize, b: &Brep, m: &mut Meter<'_, '_>) -> Res<bool> {
    let (Some((xa, xl)), Some((ya, yl))) = (polys.node_segs(x), polys.node_segs(y)) else {
        return Ok(false);
    };
    let s = &polys.segs;
    Ok(slit(b, &s[xl], &s[ya], m)? && slit(b, &s[yl], &s[xa], m)?)
}

/// Check 7: whether no trims cross (module docs).
fn crossings(polys: &Polys, b: &Brep, m: &mut Meter<'_, '_>) -> Res<bool> {
    let mut ok = true;
    let segs = &polys.segs;
    for (i, s) in segs.iter().enumerate() {
        for t in &segs[i + 1..] {
            // Segments ending at one point meet as their rays there show.
            if same(s.a, t.a) || same(s.a, t.b) || same(s.b, t.a) || same(s.b, t.b) {
                continue;
            }
            m.parts(UNITS.bvh.test)?;
            if boxes_meet(s, t) && segments_meet([s.a, s.b], [t.a, t.b], m)? {
                ok = false;
            }
        }
    }
    for g in groups((0..polys.nodes.len()).collect(), |n| polys.nodes[n].at) {
        if !rays_alternate(polys, &g, b, m)? {
            ok = false;
        }
    }
    Ok(ok)
}

/// Check 9: whether each loop turns its way; a hole of zero area only if
/// each of its segments has its slit side in it.
fn oriented(polys: &Polys, flipped: bool, b: &Brep, m: &mut Meter<'_, '_>) -> Res<bool> {
    let ccw = if flipped {
        Ordering::Less
    } else {
        Ordering::Greater
    };
    let mut ok = true;
    for (l, lp) in polys.loops.iter().enumerate() {
        let want = if lp.outer { ccw } else { ccw.reverse() };
        let sign = m.area(&polys.points(l))?;
        if sign == want {
            continue;
        }
        if sign != Ordering::Equal || lp.outer {
            ok = false;
            continue;
        }
        let segs = &polys.segs[lp.segs.clone()];
        for s in segs {
            // Every pair is compared, whatever the ids' order.
            let mut paired = false;
            for t in segs {
                paired |= slit(b, s, t, m)?;
            }
            ok &= paired;
        }
    }
    Ok(ok)
}

/// Check 10: whether each hole lies in the outer loop and in no other
/// hole (module docs).
fn nested(polys: &Polys, m: &mut Meter<'_, '_>) -> Res<bool> {
    let Some(outer) = polys.loops.iter().position(|l| l.outer) else {
        return Ok(true);
    };
    let mut ok = true;
    for (h, lp) in polys.loops.iter().enumerate() {
        if h == outer {
            continue;
        }
        // The least segment by its ends; a loop of one node, that point.
        let (c, d) = match (polys.segs[lp.segs.clone()].iter())
            .map(|s| (s.a, s.b))
            .min_by(|x, y| point_cmp(x.0, y.0).then(point_cmp(x.1, y.1)))
        {
            Some(ends) => ends,
            None => {
                let at = polys.nodes[lp.nodes.start].at;
                (at, at)
            }
        };
        for (o, _) in polys.loops.iter().enumerate().filter(|&(o, _)| o != h) {
            let pts = polys.points(o);
            ok &= inside(c, d, &pts, m)? == (o == outer);
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests;

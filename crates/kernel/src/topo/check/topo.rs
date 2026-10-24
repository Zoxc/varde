//! The check's first three invariants: the body's topology (1), its
//! bit-exact joins (2) and its domains (3). Each reads stored values only
//! and decides by exact comparisons: ids, links and bits, no geometry
//! computed.
//!
//! **1. Topology** ([`Topology`]), its kinds of item in this order:
//! - an edge names its curve and two vertices, and its stored coedge is
//!   one of its own;
//! - a coedge names its edge, face, loop, p-curve, `next` and `prev`;
//!   `next` and `prev` are inverse; `next` stays in its loop and the loop
//!   is on the coedge's face; it ends at the vertex `next` starts at. A
//!   paired coedge's pair is paired back, another coedge of its edge
//!   running the other way, and the edge has exactly these two coedges:
//!   two solids touching along an edge are two edges of a contact, never
//!   one edge with four coedges. An unpaired coedge is its edge's only
//!   one, and either singular (its edge [`Singular`](EdgeKind::Singular),
//!   both ends one pole vertex, its curve that point, every span of its
//!   p-curve an iso span on a side its surface's tiles declare singular)
//!   or a sheet's open boundary;
//! - a vertex's stored coedge leaves it, and its corners (the coedges
//!   ending there) make **one fan**: from a corner `c` the walk steps to
//!   `pair(next(c))`, or to `next(c)` itself where that is singular (a
//!   self-loop at the pole). On a solid every corner lies on one cycle of
//!   the walk. On a sheet a vertex on the open boundary has one fan
//!   running from the corner arriving along the boundary to the one
//!   leaving along it, every corner on it. A loop visiting a vertex twice
//!   at a pinch has two corners there, both on the one cycle; split into
//!   two loops through the vertex it has a fan per loop, and fails;
//! - a loop names its face and a first coedge of its own, and its
//!   coedges, walked by `next` from the first, are one cycle through all
//!   of them;
//! - a face names its surface; its loops, walked from its first, are its
//!   own, each once, all of them; each side's shell (a solid's both, a
//!   sheet's at least one) is a shell;
//! - a shell's kind is open exactly on a sheet; its sides are faces'
//!   sides naming it, each once, all of them; a solid's shell is in a
//!   region, a sheet's in none;
//! - a region (a solid's only) holds an outer shell of kind outer (none
//!   for the unbounded region, which is empty) and island shells of kind
//!   island, each naming it, each once, all of them; its container is an
//!   empty region other than itself, if any.
//!
//! **2. Joins** ([`Joins`]), bit for bit:
//! - an edge is canonical (`v_start ≤ v_end`), its `t_range` two
//!   increasing knots of its curve, the curve's control points there its
//!   vertices' positions (a), so the stored curve ends exactly at them;
//! - a coedge's p-curve ends (in the coedge's direction) at the point the
//!   next coedge's starts at (b), lies span by span in one tile, so that
//!   each breakpoint on a tile line has its integer coordinate exactly
//!   (c), and runs over its edge's `t_range` exactly (e).
//!
//! The rest of the invariant holds by the types' own rules, which refuse
//! anything else: consecutive spans of a curve or p-curve share their
//! joining control point (f, [`Curve3D`](crate::geom::curve::Curve3D),
//! [`PCurve`]), neighbouring tiles of a surface their boundary rows (d,
//! [`Surface`]); a surface's exact rows
//! hold nothing yet.
//!
//! **3. Domain** ([`Domain`]): a coedge's p-curve lies in its face's
//! surface's closed lattice rectangle (its spans' control values do, so
//! the curve does), and a span on a side of the rectangle (a coordinate's
//! control values all that side's value: the curve runs along it) is an
//! iso span exactly on it.
//!
//! **Reading a broken body.** The hooks of 2 and 3 run where 1 was left
//! out ([`Brep::check_except`]): where an id they follow names nothing,
//! they pass that part, the topology's to refuse. Every item is charged
//! one unit, and one more per step of a walk or span read.

use varde_decide::bezier::{Dir, Side as TileSide};

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::infra::budget::{Ledger, Step, Stop};
use crate::topo::brep::{
    BodyKind, Brep, CoedgeId, EdgeId, EdgeKind, FaceId, LoopId, RegionId, RegionKind, ShellId,
    ShellKind, Side, VertId, VertKind,
};

/// The checks of invariant 1, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum TopoCheck {
    /// An edge's curve or vertices name nothing.
    EdgeIds = 1,
    /// An edge's stored coedge isn't one of its own.
    EdgeCoedge,
    /// A coedge's edge, face, loop, p-curve, `next`, `prev` or pair names
    /// nothing.
    CoedgeIds,
    /// `next` and `prev` aren't inverse.
    NextPrev,
    /// `next` leaves the loop, or the loop is on another face.
    InLoop,
    /// A coedge doesn't end where `next` starts.
    Corner,
    /// A pair isn't paired back, is the coedge itself, is on another edge
    /// or runs the same way.
    Pair,
    /// The edge has other coedges than the coedge and its pair.
    EdgeUses,
    /// An unpaired singular coedge off its pole or its singular side.
    Singular,
    /// An unpaired coedge neither singular nor on a sheet.
    Open,
    /// A vertex's stored coedge doesn't leave it.
    VertCoedge,
    /// A vertex's corners make more than one fan.
    Fan,
    /// A loop's face or first coedge names nothing of its own, or its
    /// `next_loop` names nothing.
    LoopIds,
    /// A loop's coedges aren't one cycle.
    LoopCycle,
    /// A face's surface names nothing.
    FaceIds,
    /// A face's loops aren't its own, each once, all of them.
    FaceLoops,
    /// A face's side has no shell where it needs one, or names nothing.
    FaceShells,
    /// A shell's kind isn't open exactly on a sheet.
    ShellKind,
    /// A shell's sides aren't the faces' sides naming it, each once.
    ShellSides,
    /// A shell's region is missing on a solid, or present on a sheet.
    ShellRegion,
    /// A region on a sheet, or the unbounded region not empty.
    RegionKind,
    /// A region's shells aren't the shells naming it, each once, or of
    /// the wrong kind.
    RegionShells,
    /// A region's container isn't another empty region.
    Container,
}

/// The checks of invariant 2, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum JoinCheck {
    /// An edge's vertices aren't canonical (`v_start > v_end`).
    Canonical = 1,
    /// An edge's `t_range` isn't two increasing knots of its curve.
    Range,
    /// An edge's curve doesn't end at its vertices' positions.
    Ends,
    /// A coedge's p-curve doesn't end where the next one's starts.
    Join,
    /// A span of a coedge's p-curve isn't in one tile.
    Tiles,
    /// A coedge's p-curve doesn't run over its edge's `t_range`.
    SameT,
}

/// The checks of invariant 3, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum DomainCheck {
    /// A coedge's p-curve leaves its surface's closed rectangle.
    Inside = 1,
    /// A span along a side of the rectangle isn't an iso span on it.
    OnSide,
}

type Checked = Result<(), Stop<Fault>>;

fn fail(check: u32) -> Checked {
    Err(Stop::Failed(Fault {
        check,
        finding: Finding::Defect,
    }))
}

/// Fails with `check` unless `ok`.
fn need(ok: bool, check: u32) -> Checked {
    if ok { Ok(()) } else { fail(check) }
}

/// `v[i]`, if there is one.
fn at<T: Copy>(v: &[T], i: u32) -> Option<T> {
    v.get(i as usize).copied()
}

/// Charges `n` units of work.
fn charge(ledger: &mut Ledger<'_>, n: usize) -> Checked {
    ledger.charge(Step::Work, u64::try_from(n).unwrap_or(u64::MAX))?;
    Ok(())
}

fn same_bits<const N: usize>(a: &[f64; N], b: &[f64; N]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// A coedge's vertex at its end (`end`) or start, in its own direction;
/// `None` where an id names nothing.
fn coedge_vert(b: &Brep, c: CoedgeId, end: bool) -> Option<VertId> {
    let e = at(&b.coedges.edge, c.0)?;
    let rev = at(&b.coedges.reversed, c.0)?;
    let v = if end != rev {
        at(&b.edges.v_end, e.0)?
    } else {
        at(&b.edges.v_start, e.0)?
    };
    (v.index() < b.vert_count()).then_some(v)
}

/// A coedge's p-curve, `None` where an id names nothing.
fn pcurve(b: &Brep, c: CoedgeId) -> Option<&PCurve> {
    b.pcurves.get(at(&b.coedges.pcurve, c.0)?.index())
}

/// A coedge's UV at its end (`end`) or start, in its own direction: its
/// p-curve's last or first control point (swapped when reversed).
fn coedge_uv(b: &Brep, c: CoedgeId, end: bool) -> Option<[f64; 2]> {
    let p = pcurve(b, c)?;
    let last = end != at(&b.coedges.reversed, c.0)?;
    p.knot_point(if last { p.spans().len() } else { 0 })
}

/// A face's surface, `None` where an id names nothing.
fn face_surface(b: &Brep, f: FaceId) -> Option<&Surface> {
    b.surfaces.surface.get(at(&b.faces.surface, f.0)?.index())
}

/// Whether coedge `c` is singular: unpaired on a singular edge.
fn is_singular(b: &Brep, c: CoedgeId) -> bool {
    at(&b.coedges.pair, c.0).is_some_and(CoedgeId::is_none)
        && at(&b.coedges.edge, c.0)
            .and_then(|e| at(&b.edges.kind, e.0))
            .is_some_and(|k| k == EdgeKind::Singular)
}

/// Invariant 1, the topology (module docs).
pub(crate) struct Topology;

impl Invariant for Topology {
    fn kinds(&self) -> &[ItemKind] {
        &[
            ItemKind::Edge,
            ItemKind::Coedge,
            ItemKind::Vert,
            ItemKind::Loop,
            ItemKind::Face,
            ItemKind::Shell,
            ItemKind::Region,
        ]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Edge(e) => edge(cx, e),
            Item::Coedge(c) => coedge(cx, c, ledger),
            Item::Vert(v) => fan(cx, v, ledger),
            Item::Loop(l) => lp(cx, l, ledger),
            Item::Face(f) => face(cx, f, ledger),
            Item::Shell(s) => shell(cx, s, ledger),
            Item::Region(r) => region(cx, r, ledger),
            _ => Ok(()),
        }
    }
}

fn edge(cx: &Cx<'_>, e: EdgeId) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let es = &b.edges;
    let nv = b.vert_count();
    let ids = es.curve[e.index()].index() < b.curve_count()
        && es.v_start[e.index()].index() < nv
        && es.v_end[e.index()].index() < nv;
    need(ids, T::EdgeIds as u32)?;
    let held = es.coedge[e.index()];
    need(at(&b.coedges.edge, held.0) == Some(e), T::EdgeCoedge as u32)
}

fn coedge(cx: &Cx<'_>, c: CoedgeId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let cs = &b.coedges;
    let i = c.index();
    let nc = b.coedge_count();
    let (e, next, prev, pair) = (cs.edge[i], cs.next[i], cs.prev[i], cs.pair[i]);
    let ids = e.index() < b.edge_count()
        && cs.face[i].index() < b.face_count()
        && cs.lp[i].index() < b.loop_count()
        && cs.pcurve[i].index() < b.pcurve_count()
        && next.index() < nc
        && prev.index() < nc
        && (pair.is_none() || pair.index() < nc);
    need(ids, T::CoedgeIds as u32)?;
    need(
        cs.prev[next.index()] == c && cs.next[prev.index()] == c,
        T::NextPrev as u32,
    )?;
    let lp = cs.lp[i];
    need(
        cs.lp[next.index()] == lp && at(&b.loops.face, lp.0) == Some(cs.face[i]),
        T::InLoop as u32,
    )?;
    let corner = coedge_vert(b, c, true);
    need(
        corner.is_some() && corner == coedge_vert(b, next, false),
        T::Corner as u32,
    )?;
    let uses = &cx.inc.edge_coedges[e.index()];
    charge(ledger, uses.len())?;
    if !pair.is_none() {
        let p = pair.index();
        need(
            pair != c && cs.pair[p] == c && cs.edge[p] == e && cs.reversed[p] != cs.reversed[i],
            T::Pair as u32,
        )?;
        let both = if pair < c { [pair, c] } else { [c, pair] };
        need(
            uses[..] == both && b.edges.kind[e.index()] != EdgeKind::Singular,
            T::EdgeUses as u32,
        )
    } else {
        need(uses[..] == [c], T::EdgeUses as u32)?;
        if b.edges.kind[e.index()] == EdgeKind::Singular {
            singular(cx, c, ledger)
        } else {
            need(b.kind() == BodyKind::Sheet, T::Open as u32)
        }
    }
}

/// The rest of a singular coedge `c`: both ends one pole vertex, the
/// edge's curve that point, every span of the p-curve iso on a side its
/// surface's tiles declare singular.
fn singular(cx: &Cx<'_>, c: CoedgeId, ledger: &mut Ledger<'_>) -> Checked {
    let b = cx.body();
    let check = TopoCheck::Singular as u32;
    let e = b.coedges.edge[c.index()];
    let [vs, ve] = b.ends(e);
    need(vs == ve && b.vert_kind(vs) == VertKind::Pole, check)?;
    let pos = b.pos(vs);
    let curve = b.edge_curve(e);
    charge(ledger, curve.spans().len())?;
    let point = curve
        .spans()
        .iter()
        .flatten()
        .all(|p| same_bits(&[p[0], p[1], p[2]], &pos));
    need(point, check)?;
    let (Some(p), Some(s)) = (pcurve(b, c), face_surface(b, b.coedges.face[c.index()])) else {
        return fail(check);
    };
    charge(ledger, p.spans().len())?;
    need(
        p.spans().iter().all(|span| on_singular_side(s, span)),
        check,
    )
}

/// Whether `span` is an iso span on a tile line of `s` that a tile beside
/// it declares singular, on that tile's side.
fn on_singular_side(s: &Surface, span: &PSpan) -> bool {
    let PSpan::Iso { fixed, at, .. } = *span else {
        return false;
    };
    let Some([i, j]) = span.tile() else {
        return false;
    };
    let declared =
        |i: i64, j: i64, side: TileSide| s.tile(i, j).is_some_and(|t| t.singular().contains(side));
    // The span's tile is the one above the line (`tile` floors the fixed
    // value), so the line is its side 0 if `at` is that tile's index.
    match fixed {
        Dir::U => {
            at == i as f64 && (declared(i, j, TileSide::U0) || declared(i - 1, j, TileSide::U1))
        }
        Dir::V => {
            at == j as f64 && (declared(i, j, TileSide::V0) || declared(i, j - 1, TileSide::V1))
        }
    }
}

/// The corner after `c` in its vertex's fan: `pair(next(c))`, or
/// `next(c)` where that is singular; `None` at a sheet's open boundary.
fn fan_step(b: &Brep, c: CoedgeId) -> Option<CoedgeId> {
    let n = b.coedges.next[c.index()];
    if is_singular(b, n) {
        return Some(n);
    }
    let p = b.coedges.pair[n.index()];
    (!p.is_none()).then_some(p)
}

fn fan(cx: &Cx<'_>, v: VertId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let held = b.verts.coedge[v.index()];
    need(coedge_vert(b, held, false) == Some(v), T::VertCoedge as u32)?;
    let corners = &cx.inc.vert_corners[v.index()];
    let n = corners.len();
    charge(ledger, n)?;
    // A walk starts at the corner arriving along a sheet's open boundary,
    // if any; else anywhere on the cycle.
    let mut open = corners
        .iter()
        .copied()
        .filter(|&c| b.coedges.pair[c.index()].is_none() && !is_singular(b, c));
    let (start, cycle) = match (open.next(), open.next()) {
        (Some(_), Some(_)) => return fail(T::Fan as u32),
        (Some(c), None) => (c, false),
        (None, _) => match corners.first() {
            Some(&c) => (c, true),
            None => return Ok(()),
        },
    };
    let mut at = start;
    let mut seen = 1;
    loop {
        match fan_step(b, at) {
            Some(next) if cycle && next == start => break,
            Some(next) if seen < n && corners.binary_search(&next).is_ok() => {
                at = next;
                seen += 1;
            }
            None if !cycle => break,
            _ => return fail(T::Fan as u32),
        }
    }
    need(seen == n, T::Fan as u32)
}

fn lp(cx: &Cx<'_>, l: LoopId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let ls = &b.loops;
    let first = ls.first_coedge[l.index()];
    let next_loop = ls.next_loop[l.index()];
    need(
        ls.face[l.index()].index() < b.face_count()
            && at(&b.coedges.lp, first.0) == Some(l)
            && (next_loop.is_none() || next_loop.index() < b.loop_count()),
        T::LoopIds as u32,
    )?;
    let size = cx.inc.loop_sizes[l.index()];
    charge(ledger, size)?;
    // The coedges of the loop pass the coedge checks: `next` stays in the
    // loop, so the walk is one cycle through all of them iff it returns
    // after exactly `size` steps.
    let mut steps = 1;
    let mut c = b.coedges.next[first.index()];
    while c != first && steps <= size {
        c = b.coedges.next[c.index()];
        steps += 1;
    }
    need(c == first && steps == size, T::LoopCycle as u32)
}

fn face(cx: &Cx<'_>, f: FaceId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let fs = &b.faces;
    need(
        fs.surface[f.index()].index() < b.surface_count(),
        T::FaceIds as u32,
    )?;
    let count = cx.inc.face_loops[f.index()];
    charge(ledger, count)?;
    let mut l = fs.first_loop[f.index()];
    let mut seen = 0;
    while !l.is_none() {
        // A loop naming `f` is in no other face's list, so a list of more
        // than `count` repeats one.
        if seen == count || at(&b.loops.face, l.0) != Some(f) {
            return fail(T::FaceLoops as u32);
        }
        seen += 1;
        l = b.loops.next_loop[l.index()];
    }
    need(seen == count && count > 0, T::FaceLoops as u32)?;
    let shells = fs.shells[f.index()];
    let named = shells.iter().filter(|s| !s.is_none()).count();
    let enough = match b.kind() {
        BodyKind::Solid => named == 2,
        BodyKind::Sheet => named >= 1,
    };
    need(
        enough
            && shells
                .iter()
                .all(|s| s.is_none() || s.index() < b.shell_count()),
        T::FaceShells as u32,
    )
}

fn shell(cx: &Cx<'_>, s: ShellId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let ss = &b.shells;
    let sheet = b.kind() == BodyKind::Sheet;
    need(
        (ss.kind[s.index()] == ShellKind::Open) == sheet,
        T::ShellKind as u32,
    )?;
    let sides = &ss.sides[s.index()];
    charge(ledger, sides.len())?;
    let back = sides
        .iter()
        .all(|&(f, side)| at(&b.faces.shells, f.0).is_some_and(|sh| sh[side.index()] == s));
    let mut sorted: Vec<(FaceId, Side)> = sides.clone();
    sorted.sort_unstable();
    sorted.dedup();
    need(
        !sides.is_empty()
            && back
            && sorted.len() == sides.len()
            && sides.len() == cx.inc.shell_sides[s.index()],
        T::ShellSides as u32,
    )?;
    let r = ss.region[s.index()];
    need(
        if sheet {
            r.is_none()
        } else {
            r.index() < b.region_count()
        },
        T::ShellRegion as u32,
    )
}

fn region(cx: &Cx<'_>, r: RegionId, ledger: &mut Ledger<'_>) -> Checked {
    use TopoCheck as T;
    let b = cx.body();
    let rs = &b.regions;
    let outer = rs.outer[r.index()];
    need(
        b.kind() == BodyKind::Solid
            && (!outer.is_none() || rs.kind[r.index()] == RegionKind::Empty),
        T::RegionKind as u32,
    )?;
    let islands = &rs.islands[r.index()];
    charge(ledger, islands.len())?;
    let of_kind = |s: ShellId, kind: ShellKind| {
        at(&b.shells.region, s.0) == Some(r) && at(&b.shells.kind, s.0) == Some(kind)
    };
    let mut all: Vec<ShellId> = islands.clone();
    if !outer.is_none() {
        all.push(outer);
    }
    let held = all.len();
    all.sort_unstable();
    all.dedup();
    need(
        (outer.is_none() || of_kind(outer, ShellKind::Outer))
            && islands.iter().all(|&s| of_kind(s, ShellKind::Island))
            && all.len() == held
            && held == cx.inc.region_shells[r.index()],
        T::RegionShells as u32,
    )?;
    let c = rs.container[r.index()];
    need(
        c.is_none() || (c != r && at(&rs.kind, c.0) == Some(RegionKind::Empty)),
        T::Container as u32,
    )
}

/// Invariant 2, the bit-exact joins (module docs).
pub(crate) struct Joins;

impl Invariant for Joins {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Edge, ItemKind::Coedge]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Edge(e) => edge_ends(cx.body(), e),
            Item::Coedge(c) => coedge_joins(cx.body(), c, ledger),
            _ => Ok(()),
        }
    }
}

/// (a): canonical, the range two knots, the curve's points there the
/// vertices'.
fn edge_ends(b: &Brep, e: EdgeId) -> Checked {
    use JoinCheck as J;
    let es = &b.edges;
    let [vs, ve] = [es.v_start[e.index()], es.v_end[e.index()]];
    need(vs <= ve, J::Canonical as u32)?;
    let Some(curve) = b.curves3.get(es.curve[e.index()].index()) else {
        return Ok(());
    };
    let [t0, t1] = es.t_range[e.index()];
    let knot = |t: f64| {
        curve
            .knots()
            .iter()
            .position(|k| k.to_bits() == t.to_bits())
    };
    let (Some(k0), Some(k1)) = (knot(t0), knot(t1)) else {
        return fail(J::Range as u32);
    };
    need(k0 < k1, J::Range as u32)?;
    let end = |k: usize, v: VertId| match (curve.knot_point(k), at(&b.verts.pos, v.0)) {
        (Some(p), Some(q)) => same_bits(&p, &q),
        // A vertex naming nothing is the topology's to refuse.
        (_, None) => true,
        (None, Some(_)) => false,
    };
    need(end(k0, vs) && end(k1, ve), J::Ends as u32)
}

/// (b), (c) and (e) at coedge `c`.
fn coedge_joins(b: &Brep, c: CoedgeId, ledger: &mut Ledger<'_>) -> Checked {
    use JoinCheck as J;
    let Some(p) = pcurve(b, c) else {
        return Ok(());
    };
    let next = b.coedges.next[c.index()];
    if let (Some(end), Some(start)) = (coedge_uv(b, c, true), coedge_uv(b, next, false)) {
        need(same_bits(&end, &start), J::Join as u32)?;
    }
    charge(ledger, p.spans().len())?;
    need(
        p.spans().iter().all(|s| s.tile().is_some()),
        J::Tiles as u32,
    )?;
    let Some(range) = at(&b.edges.t_range, b.coedges.edge[c.index()].0) else {
        return Ok(());
    };
    let k = p.knots();
    let ends = [k[0], k[k.len() - 1]];
    need(same_bits(&ends, &range), J::SameT as u32)
}

/// Invariant 3, the domain (module docs).
pub(crate) struct Domain;

impl Invariant for Domain {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Coedge]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        let Item::Coedge(c) = item else {
            return Ok(());
        };
        let b = cx.body();
        let (Some(p), Some(s)) = (pcurve(b, c), face_surface(b, b.coedges.face[c.index()])) else {
            return Ok(());
        };
        charge(ledger, p.spans().len())?;
        let Some(sides) = rect_sides(s) else {
            return fail(DomainCheck::Inside as u32);
        };
        let inside = p.spans().iter().all(|span| {
            let [u, v] = span.coords().map(|c| c.range());
            sides[0][0] <= u[0] && u[1] <= sides[0][1] && sides[1][0] <= v[0] && v[1] <= sides[1][1]
        });
        need(inside, DomainCheck::Inside as u32)?;
        need(
            p.spans().iter().all(|span| iso_where_on_side(span, &sides)),
            DomainCheck::OnSide as u32,
        )
    }
}

/// Global UV values up to this magnitude are exact as `f64`s (`2⁵³`).
const EXACT: i64 = 1 << 53;

/// The sides of `s`'s rectangle in global UV, `[[u₀, u₁], [v₀, v₁]]`;
/// `None` for one past `2⁵³`, which has no `f64` UV.
fn rect_sides(s: &Surface) -> Option<[[f64; 2]; 2]> {
    let r = s.rect();
    let side = |x: i64| (-EXACT..=EXACT).contains(&x).then_some(x as f64);
    Some([
        [side(r.u.start)?, side(r.u.end)?],
        [side(r.v.start)?, side(r.v.end)?],
    ])
}

/// Whether `span`, if it runs along a side of the rectangle `sides` (a
/// coordinate's control values all that side's value), is an iso span
/// on one such side.
fn iso_where_on_side(span: &PSpan, sides: &[[f64; 2]; 2]) -> bool {
    let coords = span.coords();
    let mut on = false;
    for (d, dir) in [Dir::U, Dir::V].into_iter().enumerate() {
        for side in sides[d] {
            if coords[d].values().iter().all(|x| x[0] == side) {
                on = true;
                if let PSpan::Iso { fixed, at, .. } = *span
                    && fixed == dir
                    && at == side
                {
                    return true;
                }
            }
        }
    }
    !on
}

#[cfg(test)]
pub(super) mod tests;

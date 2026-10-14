//! Strip and corner pieces: a run of strip boxes of one trimming curve
//! (or of a slit's coedges), or of corner boxes of a vertex, together
//! with the curves' ribbons over the run, and the piece's degree about the
//! query point's shadow, an exact integer.
//!
//! **The counting surface.** A face's counting surface is the face plus,
//! per coedge, the **ribbon** between the trimming curve carried onto the
//! surface, `S(p(t))`, and the edge's own curve `C(t)`, the straight
//! segment between the two at each parameter `t` (the **radial** at `t`),
//! the same `t` on both. The surface's boundary is the chain of the edges'
//! curves, so the degrees of a partition of the counting surface sum to
//! the signed crossings of those curves by the query point's ray, the
//! ray records.
//!
//! **A strip piece** is the part of the face inside its trim within a set
//! of boxes (a run of strip boxes of the coedges `c`), plus `c`'s ribbon
//! over every stretch of `c`'s p-curve inside the run, from the parameter
//! where it enters the run's outer boundary to the one where it leaves.
//! Any set of boxes, not overlapping, is a run: the degree is additive,
//! so runs of any partition of the strip boxes, and their pieces, sum to
//! the same; a run only saves the cuts between its boxes.
//!
//! **The degree** is the winding of the piece's boundary about the query
//! point's shadow `q_xy`, counted as the signed crossings of the query
//! point's own ray ([`Ray::of_query`], the ray its ray records count), in
//! the coedge's direction (counter-clockwise in UV on an unflipped face,
//! clockwise on a flipped one, which is the face's inside winding `W`
//! times counter-clockwise):
//!
//! - **free sides**: the run's outer boundary (the box sides not shared
//!   by two boxes of the run, cut where boxes meet in T-junctions) where
//!   it lies inside the trim, counted counter-clockwise by the side turns
//!   ([`iso_turn`](super::degree::iso_turn)) and multiplied by `W`;
//! - **the ribbon's outer edge**: the crossings of the edge's ray record
//!   (`RayRec(q, g)`, a [`ChainRec`]) whose parameters lie between the
//!   piece's entry and exit, negated for a coedge against its edge;
//! - **the cuts**: where the p-curve crosses the outer boundary, the
//!   piece's boundary turns from a box side into the radial there and on
//!   to the edge's curve. The surface's own trim curve inside the run is
//!   not on the boundary (the ribbon is glued to it).
//!
//! **Cuts off the ray.** A crossing of the p-curve with an outer side is
//! found exactly or certified: a root of the p-span in the side's line
//! equation (isolated, its position along the side certified inside the
//! side's open extent), or a knot of the p-curve exactly on the line (a
//! p-curve is broken at every tile line), crossing where the middle
//! control points of the two spans meeting there lie on opposite sides.
//! The crossing's direction against the side (`σ = sign(T × d)`, `T` the
//! coedge's tangent, `d` the side's counter-clockwise direction) says
//! whether the curve enters the run (`σ < 0`) or leaves it, and on which
//! side of the crossing the side is inside the trim: past the crossing
//! exactly when `W·σ > 0`. Each crossing is then given a **window**: a
//! dyadic stretch `[a, b]` of the side around it, a parameter stretch of
//! the p-curve around it, and the stretch of the edge's curve those
//! parameters map to, whose hulls together (the side's piece, the surface
//! over the p-curve's piece, the edge's piece: a box holding the radial at
//! the crossing too) are certified off the query's ray, on one side of
//! its line or behind the query. The windows are tried from the coarsest
//! dyadic stretch to the finest; none certified is
//! [`PieceError::Unsettled`]: the query lies on or next to the cut (the
//! caller cuts the run elsewhere, or escalates). So every point where the
//! boundary passes from one part to the next is off the ray, and the
//! winding is the sum of the parts' crossings: the free sides' over the
//! side stretches up to the windows (the windows themselves are crossed
//! by nothing), the edge's between the windows. A ray record's crossing
//! is in the piece when its parameter enclosure lies past the entry's
//! window and before the exit's; an enclosure reaching into a window lies
//! on the window's side it reaches past (the window holds no crossing),
//! one reaching across a whole window, or lying inside one, is
//! `Unsettled`.
//!
//! **Sides through the query.** An outer side on a tile line not
//! certified off `q_xy` (through it, or too near for the level) is
//! counted from its side record ([`Sides`]): the signed crossings of its
//! tile edge by the query point's ray, the point moved, over the part of
//! the edge the piece runs along, as
//! [`box_degree_sided`](super::degree::box_degree_sided) reads it; a part
//! certified off `q_xy` but ending on the ray's line ahead of it at a tile
//! corner takes that end's side from the corner's record. The other parts
//! and the windows are certified for the query's whole enclosure, so they
//! hold for the moved point too, and every part counts the one moved
//! configuration: pieces on the two sides of a tile line read the same
//! record and stay additive. A record not yet decided is
//! [`PieceError::Need`]; an outer side inside a tile not certified off
//! `q_xy` has none ([`PieceError::Through`]).
//!
//! **Corner pieces.** A vertex inside the run joins the curves meeting
//! there: the piece's boundary runs along one edge's curve to the vertex's
//! point and on along the next, the two radials at the vertex (from the
//! same UV point to the same point) cancelling. So the ribbons of a run
//! holding curve ends are those of every coedge ending or starting inside
//! it, as many ending as starting at each vertex and UV point
//! ([`PieceError::Open`] otherwise), and a coedge's ribbon is counted from
//! its start, where that is inside the run, to its first exit, and from
//! its last entry to its end where that is inside: the ray records'
//! crossings up to and from the edge's own ends, a crossing at the vertex
//! counted in the one edge its records give it to. A curve end on the
//! outer boundary is [`PieceError::Unsettled`]: cut the run elsewhere. A
//! closed curve (one vertex, one UV point at both ends) wraps round its
//! vertex as before, and counts its whole record when it lies inside the
//! run uncut. The ribbons of a corner have pairwise different p-curves;
//! the coedges of a slit share one.
//!
//! Inside the trim or not changes only at the cuts, so each loop of the
//! outer boundary takes it from its cuts; a loop the curve doesn't cut
//! (a run with a hole, or a part of the run clear of the curve) is
//! `Unsettled` for one coedge: run such boxes as core boxes.
//!
//! **Slits.** The coedges of a slit share one p-curve (bitwise) with the
//! face on both sides of it, so their run's sides are inside the trim all
//! along; each coedge adds its own ribbon, and two coedges of one edge
//! add the same crossings with opposite signs, which cancel.
//!
//! **Parameters.** A p-curve is broken at every tile line, its edge's
//! curve not: the caller says, per p-curve span, which span of the edge's
//! curve it lies on and over which part of that span's parameter
//! ([`SpanMap`]), the same `t` mapped linearly.
//!
//! Generic over the level, as the boxes are; each call reports the
//! pieces it tested.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use malachite::Rational;
use smallvec::SmallVec;

use super::boxes::{
    BoxError, BoxKey, Extent, FaceUv, QShadow, Trim, cover, dy, iso_line, surface_hull, uv_curve,
};
use super::degree::{Ray, RecTurnError, Side, line_edge, rec_turn};
use crate::bezier::roots::MAX_ROOT_DEPTH;
use crate::bezier::{Dir, Dyadic, Found, HomCurve, HomPatch, HullBox, Limits, Scalar, isolate};
use crate::ids::{CoedgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::perturb::{NU, R_HAT};
use crate::records::keys::{Operand, Point, SideKey};
use crate::records::ray::{ChainRec, Span};
use crate::records::side::Sides;

/// The most splits one root isolation of a p-span against a side line
/// takes (a quadratic's isolation stays far below).
pub const CUT_SPLITS: u32 = 4096;

/// The finest window around a cut: `2^-WINDOW_DEPTH` of a tile.
pub const WINDOW_DEPTH: u32 = 52;

/// Where a p-curve span lies on its edge's curve: span `span` of the
/// edge's curve, over its parameters `t[0] < t[1]` in `[0, 1]`, the
/// p-span's parameter `τ` at `t[0] + (t[1] − t[0])·τ`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanMap {
    /// The edge's span, from 0 at its stored start.
    pub span: u32,
    /// The part of that span's parameter.
    pub t: [f64; 2],
}

/// A coedge's ribbon as a strip piece reads it.
#[derive(Clone, Copy, Debug)]
pub struct Ribbon<'a> {
    /// The coedge, one of the face's trims.
    pub coedge: CoedgeId,
    /// Its edge's curve: the spans along the stored direction, the same
    /// the ray records read.
    pub curve: &'a [Span],
    /// Per span of the coedge's p-curve, where it lies on `curve`.
    pub map: &'a [SpanMap],
    /// The ray record of the query point against the edge.
    pub rays: &'a ChainRec,
}

/// A strip piece's degree (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceDegree {
    /// The winding of the piece's boundary about `q_xy`, oriented as the
    /// face's boundary.
    pub degree: i64,
    /// Of it, the ribbons' outer edges' part.
    pub ribbon: i64,
    /// The cuts: the crossings of the p-curves with the run's outer
    /// boundary, each coedge's counted.
    pub cuts: u32,
    /// The pieces tested.
    pub pieces: u64,
}

/// Why a strip piece's degree isn't certified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceError {
    /// A box isn't the face's (or the query point is of the face's
    /// operand), or the patches aren't one per tile.
    Box(BoxError),
    /// No boxes, or two of them overlap.
    Boxes,
    /// The ribbons are none, repeat a coedge, name a coedge that isn't a
    /// trim of the face (or has no spans), are neither bitwise one
    /// p-curve (a slit) nor pairwise different p-curves, or their maps or
    /// curves don't fit the trim.
    Ribbon { coedge: CoedgeId },
    /// The run's outer boundary passes a point twice (boxes meeting at a
    /// corner only): cut the run there.
    Pinch,
    /// An outer side inside a tile isn't certified off `q_xy` (through
    /// it, or too near for the level): no record counts it.
    Through,
    /// An outer side on a tile line isn't certified off `q_xy`, or ends on
    /// the ray's line ahead of it at a tile corner: the piece reads this
    /// side record (and its corners'), not yet decided. Fill it and ask
    /// again.
    Need(SideKey),
    /// The level doesn't settle a cut, a side's ends, or the crossings'
    /// order (a cut at a box corner or next to the query, a tangency):
    /// cut the run elsewhere, or escalate.
    Unsettled,
    /// A coedge's curve ends inside the run and the curves meeting it
    /// there aren't all among the ribbons, or this trim of the face ends
    /// inside the run and isn't among them: a corner piece takes every
    /// coedge at its vertex.
    Open { coedge: CoedgeId },
}

impl From<BoxError> for PieceError {
    fn from(e: BoxError) -> PieceError {
        PieceError::Box(e)
    }
}

/// A segment of the run's outer boundary: the iso line of `tile`'s patch
/// where the local coordinate in `dir` is `at`, over `ext` of the other,
/// run counter-clockwise round the run (against the other coordinate
/// where `back`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seg {
    tile: [i64; 2],
    dir: Dir,
    at: Dyadic,
    ext: Extent,
    back: bool,
}

/// A point of the global UV grid: per coordinate a tile index and a local
/// dyadic in `[0, 1)`.
type GridPt = [(i64, Dyadic); 2];

/// `i + t`, with `t = 1` carried into the index.
fn norm(i: i64, t: Dyadic) -> (i64, Dyadic) {
    if t == Dyadic::ONE {
        (i + 1, Dyadic::ZERO)
    } else {
        (i, t)
    }
}

/// The index of the coordinate `dir` fixes.
fn fixed(dir: Dir) -> usize {
    match dir {
        Dir::U => 0,
        Dir::V => 1,
    }
}

impl Seg {
    fn point(&self, along: Dyadic) -> GridPt {
        let (k, o) = (fixed(self.dir), 1 - fixed(self.dir));
        let mut p = [(0, Dyadic::ZERO); 2];
        p[k] = norm(self.tile[k], self.at);
        p[o] = norm(self.tile[o], along);
        p
    }

    fn start(&self) -> GridPt {
        self.point(if self.back {
            self.ext.hi()
        } else {
            self.ext.lo()
        })
    }

    fn end(&self) -> GridPt {
        self.point(if self.back {
            self.ext.lo()
        } else {
            self.ext.hi()
        })
    }
}

/// Whether two boxes share interior points.
fn overlap(a: &BoxKey, b: &BoxKey) -> bool {
    let cross = |d: Dir| {
        let (x, y) = (a.extent(d), b.extent(d));
        x.lo() < y.hi() && y.lo() < x.hi()
    };
    a.operand() == b.operand()
        && a.face() == b.face()
        && a.tile() == b.tile()
        && cross(Dir::U)
        && cross(Dir::V)
}

/// The run's outer boundary: every box's sides counter-clockwise, the
/// parts two boxes share (run both ways) cancelled, the rest merged along
/// each line.
fn outer(boxes: &[BoxKey]) -> Result<Vec<Seg>, PieceError> {
    type Member = (Extent, i8, [i64; 2], Dyadic);
    let mut groups: BTreeMap<(usize, (i64, Dyadic), i64), Vec<Member>> = BTreeMap::new();
    for b in boxes {
        for side in Side::CCW {
            let (dir, at, along, back) = side.line(b);
            let (k, o) = (fixed(dir), 1 - fixed(dir));
            groups
                .entry((k, norm(b.tile()[k], at), b.tile()[o]))
                .or_default()
                .push((along, if back { -1 } else { 1 }, b.tile(), at));
        }
    }
    let mut segs = Vec::new();
    for ((k, _, _), members) in groups {
        let dir = if k == 0 { Dir::U } else { Dir::V };
        let mut pts: Vec<Dyadic> = members.iter().flat_map(|m| [m.0.lo(), m.0.hi()]).collect();
        pts.sort_unstable();
        pts.dedup();
        let mut last: Option<Seg> = None;
        for w in pts.windows(2) {
            let (x, y) = (w[0], w[1]);
            let covering = members.iter().filter(|m| m.0.lo() <= x && y <= m.0.hi());
            let net: i32 = covering.clone().map(|m| i32::from(m.1)).sum();
            if net == 0 {
                if let Some(s) = last.take() {
                    segs.push(s);
                }
                continue;
            }
            if net.abs() > 1 {
                return Err(PieceError::Boxes);
            }
            let src = covering
                .clone()
                .find(|m| i32::from(m.1) == net)
                .expect("a member gives the net");
            let ext = Extent::new(x, y).expect("box ends");
            let seg = Seg {
                tile: src.2,
                dir,
                at: src.3,
                ext,
                back: net < 0,
            };
            match &mut last {
                Some(s) if s.tile == seg.tile && s.at == seg.at && s.back == seg.back => {
                    s.ext = Extent::new(s.ext.lo(), y).expect("increasing");
                }
                _ => {
                    if let Some(s) = last.replace(seg) {
                        segs.push(s);
                    }
                }
            }
        }
        if let Some(s) = last {
            segs.push(s);
        }
    }
    Ok(segs)
}

/// The outer boundary's closed loops, each its segments in order.
fn loops(segs: &[Seg]) -> Result<Vec<Vec<usize>>, PieceError> {
    let mut by_start: BTreeMap<GridPt, usize> = BTreeMap::new();
    for (k, s) in segs.iter().enumerate() {
        if by_start.insert(s.start(), k).is_some() {
            return Err(PieceError::Pinch);
        }
    }
    let mut seen = vec![false; segs.len()];
    let mut out = Vec::new();
    for first in 0..segs.len() {
        if seen[first] {
            continue;
        }
        let mut l = Vec::new();
        let mut s = first;
        loop {
            seen[s] = true;
            l.push(s);
            let next = *by_start.get(&segs[s].end()).ok_or(PieceError::Pinch)?;
            if next == first {
                break;
            }
            if seen[next] {
                return Err(PieceError::Pinch);
            }
            s = next;
        }
        out.push(l);
    }
    Ok(out)
}

/// A parameter of an edge's curve: a span and a parameter on it,
/// ordered along the stored direction.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct EdgePos {
    span: u32,
    t: f64,
}

/// A certified window round a cut (module docs).
#[derive(Clone, Copy, Debug)]
struct Window {
    /// The side's stretch, local, `a < b`.
    a: Dyadic,
    b: Dyadic,
    /// The side of the ray's line it lies on (`+1` left), `None` behind
    /// the query.
    side: Option<i8>,
    /// The edge's parameters it covers.
    c: [EdgePos; 2],
}

/// A crossing of a p-curve with an outer side.
#[derive(Clone, Debug)]
struct Hit {
    /// The ribbon.
    member: usize,
    /// The segment.
    seg: usize,
    /// Its position along the segment, local, an enclosure.
    pos: [f64; 2],
    /// The p-span and its parameter there (`[0, 0]` at a knot).
    pspan: u32,
    t: [f64; 2],
    /// At the knot starting `pspan`, after the span `prev`.
    knot: Option<u32>,
    /// `sign(T × d)` (module docs).
    sigma: i8,
    window: Option<Window>,
}

/// The exact value of a finite float.
fn rat(x: f64) -> Rational {
    Rational::try_from(x).expect("finite")
}

/// The pieces of `c` over aligned dyadic cells, at most two, covering
/// `[lo, hi] ⊆ [0, 1]`, as fine as two cells allow.
fn sub_pieces<S: Scalar, const N: usize>(
    c: &HomCurve<S, N>,
    [lo, hi]: [f64; 2],
) -> SmallVec<[HomCurve<S, N>; 2]> {
    let (lo, hi) = (lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0));
    let mut best = (0u32, 0u64, 1u64);
    for d in 1..=WINDOW_DEPTH {
        let s = (1u64 << d) as f64;
        // Exact: scaling by a power of two.
        let a = ((lo * s).floor() as u64).min((1u64 << d) - 1);
        let b = ((hi * s).ceil() as u64).max(a + 1);
        if b - a > 2 {
            break;
        }
        best = (d, a, b);
    }
    let (d, a, b) = best;
    let cell = |k: u64| {
        let mut x = c.clone();
        for i in (0..d).rev() {
            let [l, r] = x.split_half();
            x = if (k >> i) & 1 == 1 { r } else { l };
        }
        x
    };
    (a..b).map(cell).collect()
}

/// The union of a hull and a box given per coordinate.
fn join(h: Option<HullBox>, b: [[f64; 2]; 3]) -> HullBox {
    let mut o = h.unwrap_or(HullBox {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
    });
    for (k, [lo, hi]) in b.into_iter().enumerate() {
        if lo < o.lo[k] {
            o.lo[k] = lo;
        }
        if hi > o.hi[k] {
            o.hi[k] = hi;
        }
    }
    o
}

/// Where a box lies against a ray from `q`: `Some(Some(s))` certified on
/// side `s` of the ray's line (`+1` left), `Some(None)` certified behind
/// `q`, `None` neither.
fn off_ray(h: &HullBox, q: &QShadow, ray: Ray) -> Option<Option<i8>> {
    let lin = |c: [f64; 3]| -> Option<[f64; 2]> {
        let mut lo = Bounded::exact(0.0);
        let mut hi = Bounded::exact(0.0);
        for k in 0..3 {
            let (a, b) = (h.lo[k], h.hi[k]);
            if !(a.is_finite() && b.is_finite()) {
                return None;
            }
            let (l, r) = if c[k] >= 0.0 { (a, b) } else { (b, a) };
            lo = lo + Bounded::exact(c[k]) * Bounded::exact(l);
            hi = hi + Bounded::exact(c[k]) * Bounded::exact(r);
        }
        Some([lo.enclose()[0], hi.enclose()[1]])
    };
    let [r, n] = [lin([R_HAT.x, R_HAT.y, R_HAT.z])?, lin([NU.x, NU.y, NU.z])?];
    // The rays of query points run along `±r̂`; left of `+r̂` is `+ν`.
    let a = ray.dir()[0];
    let (left, right) = if n[0] > q.n[1] {
        (a > 0, a < 0)
    } else if n[1] < q.n[0] {
        (a < 0, a > 0)
    } else {
        (false, false)
    };
    if left {
        return Some(Some(1));
    }
    if right {
        return Some(Some(-1));
    }
    let behind = if a > 0 { r[1] < q.r[0] } else { r[0] > q.r[1] };
    behind.then_some(None)
}

/// The context of one piece.
struct Ctx<'f, 'p, 'r, S> {
    face: &'f FaceUv<'f>,
    /// The face's operand and id, for its tile edges.
    of: (Operand, FaceId),
    sides: &'r Sides,
    point: Point,
    patches: &'p [HomPatch<S>],
    ribbons: &'r [Ribbon<'r>],
    trims: SmallVec<[&'f Trim<'f>; 2]>,
    like: S,
    q: QShadow,
    ray: Ray,
    pieces: u64,
}

impl<S: Scalar> Ctx<'_, '_, '_, S> {
    fn patch(&self, tile: [i64; 2]) -> Result<&HomPatch<S>, PieceError> {
        self.face
            .patch_of(self.patches, tile)
            .ok_or(PieceError::Box(BoxError::Patches))
    }

    /// The crossings of ribbon `m`'s p-curve with segment `si`.
    fn hits(
        &mut self,
        m: usize,
        si: usize,
        seg: &Seg,
        out: &mut Vec<Hit>,
    ) -> Result<(), PieceError> {
        let t = self.trims[m];
        let coedge = t.coedge;
        let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
        let line = Rational::from(seg.tile[k]) + rat(dy(seg.at));
        let line_s =
            S::from_f64(seg.tile[k] as f64, &self.like).add(&S::from_f64(dy(seg.at), &self.like));
        let cell = seg.tile[o];
        let along = |x: Dyadic| Rational::from(cell) + rat(dy(x));
        let (alo, ahi) = (along(seg.ext.lo()), along(seg.ext.hi()));
        let n = t.spans.len();
        let closed = is_closed(t);
        // Positive tangent of the coedge along the fixed coordinate.
        let rev: i8 = if t.reversed { -1 } else { 1 };
        // `d`'s component along the other coordinate.
        let d: i8 = if seg.back { -1 } else { 1 };
        // `σ = sign(T × d)`: `T_u·d_v` on a `u` line, `−T_v·d_u` on a `v`
        // line; `s` the sign of the curve's motion in the fixed coordinate
        // along the edge.
        let sigma = |s: i8| {
            let tk = s * rev;
            if k == 0 { tk * d } else { -tk * d }
        };
        let local = |[lo, hi]: [f64; 2]| {
            let c = Bounded::exact(cell as f64);
            [
                (Bounded::exact(lo) - c).enclose()[0],
                (Bounded::exact(hi) - c).enclose()[1],
            ]
        };
        let (elo, ehi) = (dy(seg.ext.lo()), dy(seg.ext.hi()));
        for (j, sp) in t.spans.iter().enumerate() {
            let fix: [Rational; 3] = sp.map(|p| rat(p[k]));
            let lo = fix.iter().min().expect("three");
            let hi = fix.iter().max().expect("three");
            if line < *lo || line > *hi {
                continue;
            }
            // Fits: the face checked the span count.
            let ju = j as u32;
            // The knot at the span's start, exactly on the line.
            if fix[0] == line {
                let a = rat(sp[0][o]);
                if a > alo && a < ahi {
                    let prev = if j > 0 {
                        j - 1
                    } else if closed {
                        n - 1
                    } else {
                        // The curve's start on the outer boundary: a
                        // corner piece's vertex must lie inside the run.
                        return Err(PieceError::Unsettled);
                    };
                    let before = rat(t.spans[prev][1][k]).cmp(&line);
                    let after = fix[1].cmp(&line);
                    match (before, after) {
                        (Ordering::Equal, _) | (_, Ordering::Equal) => {
                            return Err(PieceError::Unsettled);
                        }
                        (x, y) if x != y => {
                            let s: i8 = if y == Ordering::Greater { 1 } else { -1 };
                            let p = local([sp[0][o], sp[0][o]]);
                            out.push(Hit {
                                member: m,
                                seg: si,
                                pos: p,
                                pspan: ju,
                                t: [0.0, 0.0],
                                knot: Some(prev as u32),
                                sigma: sigma(s),
                                window: None,
                            });
                        }
                        _ => (),
                    }
                } else if a == alo || a == ahi {
                    return Err(PieceError::Unsettled);
                }
            }
            // The curve's end on the side (its start is the knot above).
            if j + 1 == n && !closed && fix[2] == line {
                let a = rat(sp[2][o]);
                if a >= alo && a <= ahi {
                    return Err(PieceError::Unsettled);
                }
            }
            let c = uv_curve(sp, &self.like).ok_or(PieceError::Ribbon { coedge })?;
            let mut axis = [S::int(0, &self.like), S::int(0, &self.like)];
            axis[k] = S::int(1, &self.like);
            let g = c.compose_linear(&axis, &line_s.neg());
            let roots = isolate(
                &g,
                Limits {
                    max_depth: MAX_ROOT_DEPTH,
                    max_splits: CUT_SPLITS,
                },
            )
            .map_err(|_| PieceError::Unsettled)?;
            self.pieces = self.pieces.saturating_add(u64::from(roots.work.tests));
            for f in roots.found {
                let (span, root) = match f {
                    Found::Root(r) => (r.near, Some(r.dir)),
                    Found::Cluster(cl) => {
                        let at_start = cl.lo == Dyadic::ZERO && fix[0] == line;
                        let at_end = cl.hi == Dyadic::ONE && fix[2] == line;
                        // The knot itself, and nothing more there.
                        if (at_start || at_end) && cl.mult <= 1 {
                            continue;
                        }
                        ([cl.lo.bounds()[0], cl.hi.bounds()[1]], None)
                    }
                };
                let mut box_o = [f64::INFINITY, f64::NEG_INFINITY];
                for piece in sub_pieces(&c, span) {
                    let b = piece.control_box().ok_or(PieceError::Unsettled)?[o];
                    if b[0] < box_o[0] {
                        box_o[0] = b[0];
                    }
                    if b[1] > box_o[1] {
                        box_o[1] = b[1];
                    }
                }
                let p = local(box_o);
                if p[1] < elo || p[0] > ehi {
                    continue;
                }
                let Some(dir) = root else {
                    return Err(PieceError::Unsettled);
                };
                if !(p[0] > elo && p[1] < ehi) {
                    return Err(PieceError::Unsettled);
                }
                let s: i8 = if dir == Ordering::Greater { 1 } else { -1 };
                out.push(Hit {
                    member: m,
                    seg: si,
                    pos: p,
                    pspan: ju,
                    t: span,
                    knot: None,
                    sigma: sigma(s),
                    window: None,
                });
            }
        }
        Ok(())
    }

    /// The hull of the iso line of `seg` over `[a, b]`.
    fn iso_hull(&mut self, seg: &Seg, a: Dyadic, b: Dyadic) -> Result<Option<HullBox>, PieceError> {
        let patch = self.patch(seg.tile)?;
        let iso = iso_line(patch, seg.dir, seg.at);
        let ext = Extent::new(a, b).expect("a window");
        let mut h: Option<HullBox> = None;
        let mut ok = true;
        let mut n = 0u64;
        cover(&iso, 0, 0, ext.nums(), &mut |p| {
            n += 1;
            match p.control_box() {
                Some(b) => {
                    h = Some(join(h, b));
                    true
                }
                None => {
                    ok = false;
                    false
                }
            }
        });
        self.pieces = self.pieces.saturating_add(n);
        Ok(if ok { h } else { None })
    }

    /// The hull of the surface over the p-curve's pieces `pw` and of the
    /// edge's curve over the parameters they map to, with those.
    fn ribbon_hull(
        &mut self,
        m: usize,
        pw: &[(u32, [f64; 2])],
    ) -> Result<Option<(HullBox, [EdgePos; 2])>, PieceError> {
        let t = self.trims[m];
        let r = &self.ribbons[m];
        let coedge = t.coedge;
        let mut h: Option<HullBox> = None;
        let mut c_lo: Option<EdgePos> = None;
        let mut c_hi: Option<EdgePos> = None;
        for &(j, w) in pw {
            let sp = &t.spans[j as usize];
            let pc = uv_curve(sp, &self.like).ok_or(PieceError::Ribbon { coedge })?;
            for piece in sub_pieces(&pc, w) {
                self.pieces = self.pieces.saturating_add(1);
                let Some(s) = surface_hull(self.face, self.patches, &self.like, &piece) else {
                    return Ok(None);
                };
                h = Some(join(h, [0, 1, 2].map(|k| [s.lo[k], s.hi[k]])));
            }
            let map = r.map[j as usize];
            let (t0, t1) = (Bounded::exact(map.t[0]), Bounded::exact(map.t[1]));
            let at = |x: f64| t0 + (t1 - t0) * Bounded::exact(x);
            let cw = [
                at(w[0]).enclose()[0].clamp(0.0, 1.0),
                at(w[1]).enclose()[1].clamp(0.0, 1.0),
            ];
            let span = &r.curve[map.span as usize];
            let cc = HomCurve::<S, 3>::from_affine(
                span.iter().map(|p| ([p[0], p[1], p[2]], p[3])),
                &self.like,
            )
            .filter(|c| c.weights_positive())
            .ok_or(PieceError::Ribbon { coedge })?;
            for piece in sub_pieces(&cc, cw) {
                self.pieces = self.pieces.saturating_add(1);
                let Some(b) = piece.control_box() else {
                    return Ok(None);
                };
                h = Some(join(h, b));
            }
            let (lo, hi) = (
                EdgePos {
                    span: map.span,
                    t: cw[0],
                },
                EdgePos {
                    span: map.span,
                    t: cw[1],
                },
            );
            if c_lo.is_none_or(|x| lo < x) {
                c_lo = Some(lo);
            }
            if c_hi.is_none_or(|x| hi > x) {
                c_hi = Some(hi);
            }
        }
        Ok(h.map(|h| (h, [c_lo.expect("a piece"), c_hi.expect("a piece")])))
    }

    /// The window of `hit` (module docs).
    fn window(&mut self, hit: &Hit, seg: &Seg) -> Result<Window, PieceError> {
        // At a knot, the finest stretches of the two spans meeting there.
        let pw: SmallVec<[(u32, [f64; 2]); 2]> = match hit.knot {
            None => smallvec::smallvec![(hit.pspan, hit.t)],
            Some(prev) => {
                let h = 1.0 / (1u64 << WINDOW_DEPTH) as f64;
                smallvec::smallvec![(prev, [1.0 - h, 1.0]), (hit.pspan, [0.0, h])]
            }
        };
        let Some((r, c)) = self.ribbon_hull(hit.member, &pw)? else {
            return Err(PieceError::Unsettled);
        };
        let (elo, ehi) = (seg.ext.lo(), seg.ext.hi());
        for d in 1..=WINDOW_DEPTH {
            let s = (1u64 << d) as f64;
            // Exact: scaling by a power of two.
            let a = Dyadic::new((hit.pos[0] * s).floor() as u64, d).expect("in [0, 1]");
            let b =
                Dyadic::new(((hit.pos[1] * s).ceil() as u64).min(1u64 << d), d).expect("in [0, 1]");
            let (a, b) = (a.max(elo), b.min(ehi));
            if a >= b {
                continue;
            }
            let Some(iso) = self.iso_hull(seg, a, b)? else {
                continue;
            };
            let all = join(Some(r), [0, 1, 2].map(|k| [iso.lo[k], iso.hi[k]]));
            if let Some(side) = off_ray(&all, &self.q, self.ray) {
                return Ok(Window { a, b, side, c });
            }
        }
        Err(PieceError::Unsettled)
    }

    /// The turn of `seg` over `[x, y]` (local, along), its ends' sides
    /// where given, counter-clockwise round the run; on a tile line not
    /// certified off the query, the side record's count over `[x, y]`,
    /// and an end at a tile corner on the ray's line ahead of the query
    /// takes its side from the corner's record (module docs).
    fn laps(
        &mut self,
        seg: &Seg,
        x: Dyadic,
        y: Dyadic,
        given: [Option<i8>; 2],
    ) -> Result<i64, PieceError> {
        if x >= y {
            return Ok(0);
        }
        let ext = Extent::new(x, y).expect("within the segment");
        let patch = self.patch(seg.tile)?;
        // Every side reads `TurnError::Off` on such a patch: no record
        // may stand in for it.
        if !patch.weights_positive() {
            return Err(PieceError::Unsettled);
        }
        let edge = line_edge(self.of, seg.tile, seg.dir, seg.at);
        let mut n = 0u64;
        let laps = rec_turn(
            patch, seg.dir, seg.at, ext, &self.q, self.point, edge, self.sides, given, &mut n,
        );
        self.pieces = self.pieces.saturating_add(n);
        let laps = laps.map_err(|e| match e {
            RecTurnError::Need(k) => PieceError::Need(k),
            RecTurnError::Through => PieceError::Through,
            RecTurnError::Unsettled => PieceError::Unsettled,
        })?;
        Ok(if seg.back { -laps } else { laps })
    }
}

/// Whether the edge crossing over `x` lies past the window `w` along the
/// edge's stored direction: `None` where its enclosure reaches across the
/// whole window, or lies inside it (the window holds no crossing: a bad
/// record or span map) (module docs).
fn past(x: [EdgePos; 2], w: [EdgePos; 2]) -> Option<bool> {
    if x[1] < w[0] {
        return Some(false);
    }
    if x[0] > w[1] {
        return Some(true);
    }
    match (x[0] >= w[0], x[1] <= w[1]) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// Whether trim `t`'s p-curve is closed: its edge's two ends one vertex
/// and its first and last points bitwise one UV point.
fn is_closed(t: &Trim<'_>) -> bool {
    let (a, b) = uv_ends(t);
    t.ends[0] == t.ends[1] && a == b
}

/// The UV points (as bits) of trim `t`'s first and last control points,
/// along its edge: its p-curve's ends, `−0` as `0`. Its spans are not
/// empty.
fn uv_ends(t: &Trim<'_>) -> ([u64; 2], [u64; 2]) {
    let bits = |p: [f64; 3]| [(p[0] + 0.0).to_bits(), (p[1] + 0.0).to_bits()];
    let last = t.spans.len() - 1;
    (bits(t.spans[0][0]), bits(t.spans[last][2]))
}

/// The coedge's start and end, along the coedge: per end its vertex and
/// UV point (as bits), and the point.
fn coedge_ends(t: &Trim<'_>) -> [(VertId, [u64; 2], [f64; 2]); 2] {
    let last = t.spans.len() - 1;
    let pt = |p: [f64; 3]| [p[0], p[1]];
    let (a, b) = uv_ends(t);
    let stored = [
        (t.ends[0], a, pt(t.spans[0][0])),
        (t.ends[1], b, pt(t.spans[last][2])),
    ];
    if t.reversed {
        [stored[1], stored[0]]
    } else {
        stored
    }
}

/// Where the UV point `p` lies against the run of `boxes`, whose outer
/// boundary is `segs`: `Some(true)` inside, `Some(false)` outside, `None`
/// on the outer boundary. Exact.
fn place(boxes: &[BoxKey], segs: &[Seg], p: [f64; 2]) -> Option<bool> {
    let p = p.map(rat);
    let at = |i: i64, t: Dyadic| Rational::from(i) + rat(dy(t));
    for s in segs {
        let (k, o) = (fixed(s.dir), 1 - fixed(s.dir));
        if p[k] == at(s.tile[k], s.at)
            && at(s.tile[o], s.ext.lo()) <= p[o]
            && p[o] <= at(s.tile[o], s.ext.hi())
        {
            return None;
        }
    }
    Some(boxes.iter().any(|b| {
        [Dir::U, Dir::V].into_iter().all(|d| {
            let (e, k) = (b.extent(d), fixed(d));
            at(b.tile()[k], e.lo()) <= p[k] && p[k] <= at(b.tile()[k], e.hi())
        })
    }))
}

/// The signed crossings of `rays` past the window `lo` and before `hi`
/// along the edge's stored direction (from the edge's start where `lo` is
/// `None`, to its end where `hi` is), or outside the two where `wrap`
/// (module docs).
fn count_between(
    rays: &ChainRec,
    lo: Option<[EdgePos; 2]>,
    hi: Option<[EdgePos; 2]>,
    wrap: bool,
) -> Result<i64, PieceError> {
    let mut count = 0i64;
    for c in &rays.crossings {
        let pos = [
            EdgePos {
                span: c.span,
                t: c.crossing.t[0],
            },
            EdgePos {
                span: c.span,
                t: c.crossing.t[1],
            },
        ];
        let a = match lo {
            Some(w) => past(pos, w).ok_or(PieceError::Unsettled)?,
            None => true,
        };
        let b = match hi {
            Some(w) => past(pos, w).ok_or(PieceError::Unsettled)?,
            None => false,
        };
        let inside = if wrap { a || !b } else { a && !b };
        if inside {
            count += i64::from(c.crossing.sign.as_i8());
        }
    }
    Ok(count)
}

/// The degree of the piece of the run `boxes` of `face` with the ribbons
/// `ribbons` about the query point `point`, whose shadow is `q` (module
/// docs), `patches` the face's tiles' patches at the level worked at (one
/// per tile, in [`FaceUv::tile_boxes`]' order), `sides` the side and
/// corner records read where an outer side on a tile line isn't certified
/// off `q_xy`. The ribbons are those of the coedges the run's boxes meet:
/// one coedge or a slit's (a strip piece), or the coedges meeting at the
/// vertices inside the run (a corner piece), each with the query point's
/// ray record against its edge.
pub fn strip_degree<S: Scalar>(
    face: &FaceUv<'_>,
    patches: &[HomPatch<S>],
    boxes: &[BoxKey],
    ribbons: &[Ribbon<'_>],
    q: &QShadow,
    point: Point,
    sides: &Sides,
) -> Result<PieceDegree, PieceError> {
    if boxes.is_empty() {
        return Err(PieceError::Boxes);
    }
    for (k, b) in boxes.iter().enumerate() {
        if !face.owns(b) || b.operand() == point.operand {
            return Err(PieceError::Box(BoxError::Foreign));
        }
        if boxes[..k].iter().any(|a| a == b || overlap(a, b)) {
            return Err(PieceError::Boxes);
        }
    }
    if !face.patches_fit(patches) {
        return Err(PieceError::Box(BoxError::Patches));
    }
    let Some(first) = ribbons.first() else {
        return Err(PieceError::Ribbon {
            coedge: CoedgeId::NONE,
        });
    };
    let mut trims: SmallVec<[&Trim<'_>; 2]> = SmallVec::new();
    let mut index: SmallVec<[usize; 2]> = SmallVec::new();
    let i0 = face.trim_index(first.coedge).ok_or(PieceError::Ribbon {
        coedge: first.coedge,
    })?;
    // A slit: two or more coedges sharing one p-curve. Otherwise every
    // ribbon's p-curve is its own.
    let slit = ribbons.len() > 1
        && ribbons.iter().all(|r| {
            face.trim_index(r.coedge)
                .is_some_and(|i| face.same_curve(i0, i))
        });
    for (k, r) in ribbons.iter().enumerate() {
        let bad = PieceError::Ribbon { coedge: r.coedge };
        let i = face.trim_index(r.coedge).ok_or(bad)?;
        let t = &face.trims()[i];
        let fits = !t.spans.is_empty()
            && !ribbons[..k].iter().any(|o| o.coedge == r.coedge)
            && (slit || !index.iter().any(|&j| face.same_curve(j, i)))
            && r.map.len() == t.spans.len()
            && r.map.iter().all(|m| {
                (m.span as usize) < r.curve.len()
                    && m.t[0].is_finite()
                    && m.t[1].is_finite()
                    && 0.0 <= m.t[0]
                    && m.t[0] < m.t[1]
                    && m.t[1] <= 1.0
            })
            && r.curve.iter().all(|s| {
                s.iter()
                    .all(|p| p.iter().all(|x| x.is_finite()) && p[3] > 0.0)
            });
        if !fits {
            return Err(bad);
        }
        trims.push(t);
        index.push(i);
    }
    let like = patches[0].weights().coefs()[0].clone();
    let w = face.inside_winding();
    let mut ctx = Ctx {
        face,
        of: (boxes[0].operand(), boxes[0].face()),
        sides,
        point,
        patches,
        ribbons,
        trims,
        like,
        q: *q,
        ray: Ray::of_query(point.operand),
        pieces: 0,
    };
    let segs = outer(boxes)?;
    let loops = loops(&segs)?;

    // The curves' ends inside the run: every trim ending there is a
    // ribbon, and at each vertex (and UV point) as many curves end as
    // start, so the radials there cancel.
    let mut ends_in: SmallVec<[[bool; 2]; 2]> = smallvec::smallvec![[false; 2]; ribbons.len()];
    let mut balance: BTreeMap<(VertId, [u64; 2]), i32> = BTreeMap::new();
    for (i, t) in face.trims().iter().enumerate() {
        if t.spans.is_empty() || is_closed(t) {
            continue;
        }
        let m = index.iter().position(|&j| j == i);
        for (k, (v, bits, p)) in coedge_ends(t).into_iter().enumerate() {
            match place(boxes, &segs, p) {
                None => return Err(PieceError::Unsettled),
                Some(false) => continue,
                Some(true) => (),
            }
            let Some(m) = m else {
                return Err(PieceError::Open { coedge: t.coedge });
            };
            ends_in[m][k] = true;
            *balance.entry((v, bits)).or_default() += if k == 0 { 1 } else { -1 };
        }
    }
    for (m, t) in ctx.trims.iter().enumerate() {
        if is_closed(t) {
            continue;
        }
        for (k, (v, bits, _)) in coedge_ends(t).into_iter().enumerate() {
            if ends_in[m][k] && balance.get(&(v, bits)) != Some(&0) {
                return Err(PieceError::Open { coedge: t.coedge });
            }
        }
    }

    // The cuts and their windows.
    let mut hits = Vec::new();
    for (si, seg) in segs.iter().enumerate() {
        for m in 0..ribbons.len() {
            ctx.hits(m, si, seg, &mut hits)?;
        }
    }
    for h in hits.iter_mut() {
        h.window = Some(ctx.window(h, &segs[h.seg])?);
    }
    let win = |h: &Hit| h.window.expect("certified");

    // The free sides inside the trim.
    let single = !slit;
    let inside_after = |h: &Hit| w * i64::from(h.sigma) > 0;
    let mut free = 0i64;
    for l in &loops {
        // The cuts along the loop, in order.
        let mut order: Vec<(usize, Vec<usize>)> = Vec::new();
        for &s in l {
            let mut on: Vec<usize> = (0..hits.len()).filter(|&k| hits[k].seg == s).collect();
            on.sort_by(|&a, &b| hits[a].pos[0].total_cmp(&hits[b].pos[0]));
            if segs[s].back {
                on.reverse();
            }
            if single
                && on.windows(2).any(|p| {
                    hits[p[0]].pos[1] >= hits[p[1]].pos[0] && hits[p[1]].pos[1] >= hits[p[0]].pos[0]
                })
            {
                return Err(PieceError::Unsettled);
            }
            order.push((s, on));
        }
        let mut m = if single {
            let last = order
                .iter()
                .rev()
                .find_map(|(_, on)| on.last())
                .ok_or(PieceError::Unsettled)?;
            inside_after(&hits[*last])
        } else {
            true
        };
        for (s, on) in &order {
            let seg = segs[*s];
            let mut cur = if seg.back { seg.ext.hi() } else { seg.ext.lo() };
            let mut cur_side: Option<i8> = None;
            for &k in on {
                let h = &hits[k];
                let wd = win(h);
                let (near, far) = if seg.back { (wd.b, wd.a) } else { (wd.a, wd.b) };
                if m {
                    free += if seg.back {
                        ctx.laps(&seg, near, cur, [wd.side, cur_side])?
                    } else {
                        ctx.laps(&seg, cur, near, [cur_side, wd.side])?
                    };
                }
                if single {
                    if inside_after(h) == m {
                        return Err(PieceError::Unsettled);
                    }
                    m = inside_after(h);
                }
                let further = if seg.back { far < cur } else { far > cur };
                if further {
                    cur = far;
                    cur_side = wd.side;
                }
            }
            if m {
                free += if seg.back {
                    ctx.laps(&seg, seg.ext.lo(), cur, [None, cur_side])?
                } else {
                    ctx.laps(&seg, cur, seg.ext.hi(), [cur_side, None])?
                };
            }
        }
    }

    // The ribbons' outer edges.
    let mut ribbon = 0i64;
    for (m, r) in ribbons.iter().enumerate() {
        let t = ctx.trims[m];
        let closed = is_closed(t);
        let mut mine: Vec<&Hit> = hits.iter().filter(|h| h.member == m).collect();
        mine.sort_by(|a, b| {
            (a.pspan, a.t[0])
                .partial_cmp(&(b.pspan, b.t[0]))
                .expect("finite")
        });
        if mine
            .windows(2)
            .any(|p| p[0].pspan == p[1].pspan && p[0].t[1] >= p[1].t[0])
        {
            return Err(PieceError::Unsettled);
        }
        if t.reversed {
            mine.reverse();
        }
        let entry = |h: &Hit| h.sigma < 0;
        let mut count = 0i64;
        if closed {
            if mine.is_empty() {
                // The whole loop inside the run, or clear of it; its
                // vertex on the outer boundary (touching it uncut) says
                // neither.
                match place(boxes, &segs, coedge_ends(t)[0].2) {
                    Some(true) => count = count_between(r.rays, None, None, false)?,
                    Some(false) => (),
                    None => return Err(PieceError::Unsettled),
                }
            } else {
                if !entry(mine[0]) {
                    mine.rotate_left(1);
                }
                if !mine.len().is_multiple_of(2)
                    || mine
                        .iter()
                        .enumerate()
                        .any(|(k, h)| entry(h) != (k % 2 == 0))
                {
                    return Err(PieceError::Unsettled);
                }
                for pair in mine.chunks(2) {
                    let (e, x) = (win(pair[0]).c, win(pair[1]).c);
                    // Along the stored direction, from `lo` to `hi`.
                    let (lo, hi) = if t.reversed { (x, e) } else { (e, x) };
                    // The windows apart along the edge.
                    if !(lo[1] < hi[0] || hi[1] < lo[0]) {
                        return Err(PieceError::Unsettled);
                    }
                    count += count_between(r.rays, Some(lo), Some(hi), hi[0] < lo[0])?;
                }
            }
        } else {
            // Stretches in the coedge's order: from its start if inside
            // the run, between an entry and the next exit, to its end if
            // inside the run.
            let [start_in, end_in] = ends_in[m];
            let mut marks: Vec<Option<[EdgePos; 2]>> = Vec::new();
            if start_in {
                marks.push(None);
            }
            for (k, h) in mine.iter().enumerate() {
                if entry(h) != ((k + usize::from(start_in)).is_multiple_of(2)) {
                    return Err(PieceError::Unsettled);
                }
                marks.push(Some(win(h).c));
            }
            if end_in {
                marks.push(None);
            }
            if !marks.len().is_multiple_of(2) {
                return Err(PieceError::Unsettled);
            }
            for pair in marks.chunks(2) {
                let (e, x) = (pair[0], pair[1]);
                let (lo, hi) = if t.reversed { (x, e) } else { (e, x) };
                if let (Some(lo), Some(hi)) = (lo, hi)
                    && lo[1] >= hi[0]
                {
                    return Err(PieceError::Unsettled);
                }
                count += count_between(r.rays, lo, hi, false)?;
            }
        }
        ribbon += if t.reversed { -count } else { count };
    }
    Ok(PieceDegree {
        degree: w * free + ribbon,
        ribbon,
        cuts: u32::try_from(hits.len()).unwrap_or(u32::MAX),
        pieces: ctx.pieces,
    })
}

#[cfg(test)]
mod tests;

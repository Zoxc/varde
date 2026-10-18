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
//! equation (isolated, then halved at the certified sign of the equation
//! until its position along the side is certified inside the side's open
//! extent or outside it), or a knot of the p-curve exactly on the line (a
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
//! caller cuts the run elsewhere, or escalates; [`strip_degree_near`]
//! says where, for the caller to refine there). So every point where the
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
//! **Box lines read as `c + 0⁺`.** Box sides lie on UV lines (tile lines
//! and dyadic lines), and trims may meet them exactly: a span along one
//! (a trim placed on a tile line), a knot or a vertex on one, a curve
//! through a box corner. Every box line is read as moved infinitesimally
//! to its high side, the same for every run and every box: a `u` line
//! `u = c` as `u = c + ε`, a `v` line `v = c` as `v = c + √2·ε`, `ε → 0⁺`
//! (a translation of the box grid, so pieces of any partition stay
//! additive), but for the low edges of the face's tiles (the least `u`
//! and the least `v` of its tile ranges), read as moved to their low side
//! (`c − ε`, `c − √2·ε`): so the boxes still hold every trim, one along
//! those edges included, with its ribbon. So no point of a trim lies on a
//! box side: a point exactly on a line lies on its low side (on its high
//! side at a low edge), a box `[lo, hi]` holds a point `p` exactly when
//! `lo < p ≤ hi` in each coordinate (`lo ≤ p` at a low edge), a span
//! along a line crosses none of its sides, and a curve end or vertex on a
//! side lies in one box only. Where a curve's point `P` lies exactly on a side's line,
//! the curve crosses the moved line next to `P` on each span leaving `P`
//! across it, at a place along the line given by the span's
//! exact local shape there (its germ); at a side's end, that place against
//! the moved end decides whether the side is crossed, and crossings at
//! one point are ordered along the side by their shapes (a curve's
//! tangent, then its bend). A span through a box corner exactly, not at a
//! knot, is placed by its exact crossing (its tangent there against the
//! moved corner). A span touching a line tangentially inside it (an exact
//! double root of its equation) is split there at the exact parameter and
//! read as its two halves leaving that point: crossing the moved line
//! twice next to it, or not at all. A curve may so cross both moved sides next to a corner
//! at one point of it (clipping the moved corner): the two cuts are
//! ordered along the curve so entries and exits alternate from its
//! start's side of the run, and nothing of the curve lies between them. The moved sides are infinitesimally apart from the
//! sides the hulls and turns are taken of, so every certificate stays as
//! it is. Slits are read alike.
//!
//! A loop of the outer boundary no curve cuts lies inside the trim or
//! outside it as a whole: where a span runs along one of its sides, the
//! face's side of that span says which; where a curve's end or knot lies
//! on the loop, or a curve passes exactly through a box corner of it or
//! touches one of its sides (its span split there at the exact rational
//! parameter), the face's sector
//! there (towards the moved point of the loop next to it) does, from the
//! curves' exact shapes; otherwise the loop is
//! [`PieceError::Unsettled`].
//!
//! **Sides through the query.** An outer side not certified off `q_xy`
//! (through it, or too near for the level) is counted from the side
//! record of its line ([`Sides`]: a tile edge, or an iso line across a
//! tile): the signed crossings of the line by the query point's ray, the
//! point moved, over the part of the line the piece runs along, as
//! [`box_degree_sided`](super::degree::box_degree_sided) reads it; a part
//! certified off `q_xy` but ending on the ray's line ahead of it takes
//! that end's side from its dyadic point's record. The other parts and
//! the windows are certified for the query's whole enclosure, so they
//! hold for the moved point too, and every part counts the one moved
//! configuration: pieces on the two sides of a line read the same record
//! and stay additive. A record not yet decided is [`PieceError::Need`].
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
//! outer boundary lies on one side of it (box lines read as `c + 0⁺`). A
//! closed curve (one vertex, one UV point at both ends) wraps round its
//! vertex as before, and counts its whole record when it lies inside the
//! run uncut. The ribbons of a corner have pairwise different p-curves;
//! the coedges of a slit share one.
//!
//! Inside the trim or not changes only at the cuts, so each loop of the
//! outer boundary takes it from its cuts, and an uncut loop as above.
//!
//! **The run's trims.** Every trim of the face that ends inside the run,
//! lies inside it (a closed one), or crosses its outer boundary must be
//! among the ribbons: else [`PieceError::Open`] for it (the caller's
//! boxes met a curve their roles don't name).
//!
//! **Ends in corner pieces** ([`corner_end`]). A corner piece may reach
//! far from its vertex `w` (a sharp corner's spike), so an edge's
//! crossing in it is placed at `w` only where the piece's part holding the
//! crossing's cluster lies within `w`'s near radius `r_w` (its hull's
//! farthest point from `w`, rounded upwards, against `r_w` rounded
//! downwards); farther along the spike it is a strip end on the curve its
//! cluster names, and where it names both curves, on the one lower in
//! the geometric order the caller gives (ids only between equal keys).
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
    BoxError, BoxKey, Extent, FaceUv, QShadow, Trim, UvSpan, cover, dy, global, iso_line,
    surface_hull, uv_curve, within_of,
};
use super::degree::{Ray, RecTurnError, Side, low_edge, rec_turn};
use crate::bezier::roots::MAX_ROOT_DEPTH;
use crate::bezier::{Dir, Dyadic, Found, HomCurve, HomPatch, HullBox, Limits, Scalar, isolate};
use crate::ids::{CoedgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::num::exact::rat;
use crate::perturb::{NU, R_HAT};
use crate::records::cusp::{Poly, pderiv, peval};
use crate::records::keys::{Operand, Point};
use crate::records::ray::{ChainRec, Span};
use crate::records::side::{Sides, Want};

mod germ;

use germ::{Germ, Leg, Toward, inside_toward};

/// The most splits one root isolation of a p-span against a side line
/// takes (a quadratic's isolation stays far below).
pub const CUT_SPLITS: u32 = 4096;

/// The most halvings of a root's interval placing it along a side.
const ROOT_HALVINGS: u32 = 64;

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
    /// An outer side isn't certified off `q_xy` (through it, or too near
    /// for the level) and no record counts it (its line past the range of
    /// `i64`): never from tiles a face can have.
    Through,
    /// An outer side isn't certified off `q_xy`, or ends on the ray's
    /// line ahead of it: the piece reads this side record (and its
    /// ends'), or this dyadic point's, not yet decided. Fill it and ask
    /// again.
    Need(Want),
    /// The level doesn't settle a cut, a side's ends, or the crossings'
    /// order (a cut next to the query, a tangency), or a loop no curve
    /// cuts says nothing of the trim (module docs): cut the run elsewhere,
    /// or escalate.
    Unsettled,
    /// A coedge's curve ends inside the run and the curves meeting it
    /// there aren't all among the ribbons, or this trim of the face ends
    /// inside the run, lies inside it (a closed one) or crosses its outer
    /// boundary and isn't among them: a corner piece takes every coedge at
    /// its vertex, and a run every curve it meets.
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
    /// Where the crossing lies at a point of the curve exactly on the
    /// segment's line (a knot, an end, a pass through a box corner): that
    /// point's coordinate along the line, global, and the crossing's germ
    /// there (module docs, "Box lines read as `c + 0⁺`").
    exact: Option<(Rational, Germ)>,
    /// That point, global UV, where `exact` is given.
    point: Option<[Rational; 2]>,
    /// The p-span and its parameter there (`[0, 0]` or `[1, 1]` next to
    /// the span's start or end on the line).
    pspan: u32,
    t: [f64; 2],
    /// `sign(T × d)` (module docs).
    sigma: i8,
    window: Option<Window>,
}

/// A p-span along a UV line (module docs, "Box lines read as `c +
/// 0⁺`"): the coordinate `c` its three control points share exactly, and
/// `n`, the side of that line (the sign along `c`) the face's inside lies
/// on next to it. `None` where it lies on no such line or doesn't run
/// monotonically along it (its middle point strictly between its ends).
/// `w` is the face's inside winding.
fn on_line(sp: &UvSpan, reversed: bool, w: i64) -> Option<(usize, i8)> {
    let c = (0..2).find(|&c| sp[0][c] == sp[1][c] && sp[1][c] == sp[2][c])?;
    let o = 1 - c;
    let (a, m, b) = (sp[0][o], sp[1][o], sp[2][o]);
    let s: i8 = if a < m && m < b {
        1
    } else if a > m && m > b {
        -1
    } else {
        return None;
    };
    // The coedge's direction along the line, and the inside on its left
    // (`W = 1`) or right: left of `+u` is `+v`, left of `+v` is `−u`.
    let s = if reversed { -s } else { s };
    let s = if w > 0 { s } else { -s };
    Some((c, if c == 1 { s } else { -s }))
}

/// A closed rectangle of global UV, `[[u_lo, u_hi], [v_lo, v_hi]]`: where
/// a piece is unsettled, in floats.
pub type UvRect = [[f64; 2]; 2];

/// A closed rectangle of global UV, `[[u_lo, u_hi], [v_lo, v_hi]]`,
/// exactly: a box's, or a [`UvRect`] read exactly ([`exact_rect`]).
pub type ExactRect = [[Rational; 2]; 2];

/// The [`UvRect`] `r` exactly.
pub(crate) fn exact_rect(r: &UvRect) -> ExactRect {
    r.map(|c| c.map(rat))
}

/// The rectangle of the UV point `(p[0], p[1])` alone.
fn point_rect(p: &[f64]) -> UvRect {
    [[p[0], p[0]], [p[1], p[1]]]
}

/// The rectangle of `seg` over `along` (local, along the segment), in
/// global UV: per coordinate the tile index plus the local value, each
/// rounded outwards by an ulp (a hint, not a certificate).
fn seg_rect(seg: &Seg, along: [f64; 2]) -> UvRect {
    let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
    let at = |i: i64, t: f64| [(i as f64 + t).next_down(), (i as f64 + t).next_up()];
    let mut r = [[0.0; 2]; 2];
    let line = at(seg.tile[k], dy(seg.at));
    r[k] = line;
    r[o] = [at(seg.tile[o], along[0])[0], at(seg.tile[o], along[1])[1]];
    r
}

/// The least rectangle holding `a` and `b`.
fn hull(a: UvRect, b: UvRect) -> UvRect {
    let lo = |x: f64, y: f64| if y < x { y } else { x };
    let hi = |x: f64, y: f64| if y > x { y } else { x };
    [0, 1].map(|k| [lo(a[k][0], b[k][0]), hi(a[k][1], b[k][1])])
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
    /// The face's inside winding.
    w: i64,
    /// Whether the ribbons are a slit's (the face on both sides of their
    /// p-curve: every free side inside the trim).
    slit: bool,
    pieces: u64,
    /// Where the piece is unsettled, as far as known: a closed rectangle
    /// of global UV ([`strip_degree_near`]).
    near: Option<UvRect>,
}

impl<S: Scalar> Ctx<'_, '_, '_, S> {
    /// [`PieceError::Unsettled`], noting `r` as (part of) where.
    fn unsettled(&mut self, r: UvRect) -> PieceError {
        self.near = Some(match self.near {
            Some(o) => hull(o, r),
            None => r,
        });
        PieceError::Unsettled
    }

    /// [`PieceError::Unsettled`], noting `r` where given.
    fn unsettled_at(&mut self, r: Option<UvRect>) -> PieceError {
        match r {
            Some(r) => self.unsettled(r),
            None => PieceError::Unsettled,
        }
    }

    /// Whether the line of axis `k` at `at` in tile index `i` is the low
    /// edge of the face's tiles, read as moved to its low side (module
    /// docs, "Box lines read as `c + 0⁺`").
    fn low_line(&self, k: usize, i: i64, at: Dyadic) -> bool {
        low_edge(self.face, k, i, at)
    }

    fn patch(&self, tile: [i64; 2]) -> Result<&HomPatch<S>, PieceError> {
        self.face
            .patch_of(self.patches, tile)
            .ok_or(PieceError::Box(BoxError::Patches))
    }

    /// The crossings of trim `t`'s p-curve with segment `si`, as ribbon
    /// `m`'s (module docs, "Cuts off the ray" and "Box lines read as `c +
    /// 0⁺`").
    fn hits(
        &mut self,
        t: &Trim<'_>,
        m: usize,
        si: usize,
        seg: &Seg,
        out: &mut Vec<Hit>,
    ) -> Result<(), PieceError> {
        let coedge = t.coedge;
        let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
        let (elo, ehi) = (dy(seg.ext.lo()), dy(seg.ext.hi()));
        // A first test in floats, outwards by an ulp of the rounded line
        // and ends.
        let lf = seg.tile[k] as f64 + dy(seg.at);
        let cf = seg.tile[o] as f64;
        let (af, bf) = ((cf + elo).next_down(), (cf + ehi).next_up());
        let line = global(seg.tile[k], seg.at);
        let line_s =
            S::from_f64(seg.tile[k] as f64, &self.like).add(&S::from_f64(dy(seg.at), &self.like));
        let cell = seg.tile[o];
        let along = |x: Dyadic| global(cell, x);
        let (alo, ahi) = (along(seg.ext.lo()), along(seg.ext.hi()));
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
                (Bounded::exact(lo) - c).enclose()[0] + 0.0,
                (Bounded::exact(hi) - c).enclose()[1] + 0.0,
            ]
        };
        // The side's line moved to its low side (the low edge of the
        // face's tiles), and its low end moved so: `−1`.
        let down = self.low_line(k, seg.tile[k], seg.at);
        let lo_s: i8 = if self.low_line(o, seg.tile[o], seg.ext.lo()) {
            -1
        } else {
            1
        };
        // Whether a crossing exactly at the side's end `e` (`alo` or
        // `ahi`) with germ `g` lies on the side: past the moved end.
        let past_end = |e: &Rational, g: &Germ| {
            (*e == alo && g.against(k, lo_s) == Ordering::Greater)
                || (*e == ahi && g.against(k, 1) == Ordering::Less)
        };
        let end_at = |e: &Rational| if *e == alo { elo } else { ehi };
        for (j, sp) in t.spans.iter().enumerate() {
            // Compare and select: the prefilter only (zeros' signs don't
            // matter against the ulp-wide margins).
            let lo = |x: f64, y: f64| if y < x { y } else { x };
            let hi = |x: f64, y: f64| if y > x { y } else { x };
            let (kmin, kmax, omin, omax) = sp.iter().fold(
                (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(a, b, c, e), p| (lo(a, p[k]), hi(b, p[k]), lo(c, p[o]), hi(e, p[o])),
            );
            if kmax < lf.next_down() || kmin > lf.next_up() || omax < af || omin > bf {
                continue;
            }
            let fix: [Rational; 3] = sp.map(|p| rat(p[k]));
            let lo = fix.iter().min().expect("three");
            let hi = fix.iter().max().expect("three");
            if line < *lo || line > *hi {
                continue;
            }
            // Fits: the face checked the span count.
            let ju = j as u32;
            // The span's ends exactly on the line: crossed next to the end
            // where the span rises from it.
            for at_end in [false, true] {
                if fix[if at_end { 2 } else { 0 }] != line {
                    continue;
                }
                let Some(g) = Leg::new(sp, at_end).germ(k, down) else {
                    continue;
                };
                let a = Leg::point(sp, at_end)[o].clone();
                if a < alo || a > ahi {
                    continue;
                }
                let pos = if a == alo || a == ahi {
                    if !past_end(&a, &g) {
                        continue;
                    }
                    let at = end_at(&a);
                    [at, at]
                } else {
                    let x = sp[if at_end { 2 } else { 0 }][o];
                    local([x, x])
                };
                out.push(Hit {
                    member: m,
                    seg: si,
                    pos,
                    exact: Some((a, g)),
                    point: Some(Leg::point(sp, at_end)),
                    pspan: ju,
                    t: if at_end { [1.0, 1.0] } else { [0.0, 0.0] },
                    // Leaving the line across its moved line from the
                    // start along the edge's parameter, coming back to the
                    // end.
                    sigma: sigma(if at_end == down { 1 } else { -1 }),
                    window: None,
                });
            }
            // Along the line: below its moved line all along.
            if fix.iter().all(|x| *x == line) {
                continue;
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
            .map_err(|_| self.unsettled(seg_rect(seg, [elo, ehi])))?;
            self.pieces = self.pieces.saturating_add(u64::from(roots.work.tests));
            for f in roots.found {
                let (span, root) = match f {
                    Found::Root(r) => {
                        // The root at an end on the line is that end's.
                        if (fix[0] == line && r.near[0] <= 0.0)
                            || (fix[2] == line && r.near[1] >= 1.0)
                        {
                            continue;
                        }
                        (r.near, Some(r.dir))
                    }
                    Found::Cluster(cl) => {
                        let at_start = cl.lo == Dyadic::ZERO && fix[0] == line;
                        let at_end = cl.hi == Dyadic::ONE && fix[2] == line;
                        // The end itself, and nothing more there; or an end
                        // the span leaves tangent to the line (its middle
                        // control point on the line too): a double root of
                        // the span's quadratic equation, its only one, and
                        // the end's germ places its crossing.
                        if (at_start || at_end) && (cl.mult <= 1 || fix[1] == line) {
                            continue;
                        }
                        // A tangency inside the span, exactly: crossed
                        // twice next to the touching point where the span
                        // lies on the moved line's side, by its two halves
                        // there, else not at all.
                        if let Some(at) = tangency(sp, k, &line)
                            && rat(cl.lo.bounds()[0]) <= at
                            && at <= rat(cl.hi.bounds()[1])
                        {
                            let (p, legs) = Leg::split(sp, &at);
                            let a = p[o].clone();
                            if a < alo || a > ahi {
                                continue;
                            }
                            let t_at = at.enclose();
                            for (back, leg) in [true, false].into_iter().zip(legs) {
                                let Some(g) = leg.germ(k, down) else {
                                    continue;
                                };
                                let pos = if a == alo || a == ahi {
                                    if !past_end(&a, &g) {
                                        continue;
                                    }
                                    let at = end_at(&a);
                                    [at, at]
                                } else {
                                    let [x0, x1] = a.enclose();
                                    local([x0, x1])
                                };
                                out.push(Hit {
                                    member: m,
                                    seg: si,
                                    pos,
                                    exact: Some((a.clone(), g)),
                                    point: Some(p.clone()),
                                    pspan: ju,
                                    t: t_at,
                                    sigma: sigma(if back == down { 1 } else { -1 }),
                                    window: None,
                                });
                            }
                            continue;
                        }
                        ([cl.lo.bounds()[0], cl.hi.bounds()[1]], None)
                    }
                };
                let whole = seg_rect(seg, [elo, ehi]);
                let place = |span: [f64; 2]| -> Result<[f64; 2], UvRect> {
                    let mut box_o = [f64::INFINITY, f64::NEG_INFINITY];
                    for piece in sub_pieces(&c, span) {
                        let b = piece.control_box().ok_or(whole)?[o];
                        if b[0] < box_o[0] {
                            box_o[0] = b[0];
                        }
                        if b[1] > box_o[1] {
                            box_o[1] = b[1];
                        }
                    }
                    Ok(local(box_o))
                };
                let mut span = span;
                let mut p = place(span).map_err(|r| self.unsettled(r))?;
                if p[1] < elo || p[0] > ehi {
                    continue;
                }
                let Some(dir) = root else {
                    return Err(self.unsettled(seg_rect(seg, p)));
                };
                let s: i8 = if dir == Ordering::Greater { 1 } else { -1 };
                // A root narrowed less than the side needs (its
                // derivative's range too wide for Krawczyk's operator):
                // halve its interval by the certified sign of the side's
                // equation at the middle, until its position is certified
                // inside the side's extent or outside it.
                let mut halvings = 0;
                let mut stuck = false;
                while !(p[0] > elo && p[1] < ehi) {
                    let mid = 0.5 * span[0] + 0.5 * span[1];
                    if halvings >= ROOT_HALVINGS || !(mid > span[0] && mid < span[1]) {
                        stuck = true;
                        break;
                    }
                    halvings += 1;
                    let past = match g.eval(&S::from_f64(mid, &self.like)).sign() {
                        Some(Ordering::Equal) | None => {
                            stuck = true;
                            break;
                        }
                        Some(x) => x == dir,
                    };
                    span = if past { [span[0], mid] } else { [mid, span[1]] };
                    p = place(span).map_err(|r| self.unsettled(r))?;
                    if p[1] < elo || p[0] > ehi {
                        break;
                    }
                }
                if stuck {
                    // Through an end of the side exactly: placed by its
                    // exact crossing there.
                    let pass = [&alo, &ahi].into_iter().find_map(|e| {
                        corner_pass(sp, k, &line, e, span)
                            .map(|f1| (e.clone(), Germ::first(f1, down)))
                    });
                    let Some((e, g)) = pass else {
                        return Err(self.unsettled(seg_rect(seg, p)));
                    };
                    if past_end(&e, &g) {
                        let at = end_at(&e);
                        let mut point = [line.clone(), line.clone()];
                        point[o] = e.clone();
                        out.push(Hit {
                            member: m,
                            seg: si,
                            pos: [at, at],
                            exact: Some((e, g)),
                            point: Some(point),
                            pspan: ju,
                            t: span,
                            sigma: sigma(s),
                            window: None,
                        });
                    }
                    continue;
                }
                if p[1] < elo || p[0] > ehi {
                    continue;
                }
                out.push(Hit {
                    member: m,
                    seg: si,
                    pos: p,
                    exact: None,
                    point: None,
                    pspan: ju,
                    t: span,
                    sigma: sigma(s),
                    window: None,
                });
            }
        }
        Ok(())
    }

    /// Whether the loop `l` of `segs`, no curve cutting it, lies inside the
    /// trim (module docs, "Box lines read as `c + 0⁺`"): from a span along
    /// one of its sides, else from the face's sector at a curve's end or
    /// knot on it, a box corner of it a curve passes through or a point
    /// of a side a curve touches; `None`
    /// where neither says.
    fn uncut(&self, l: &[usize], segs: &[Seg]) -> Option<bool> {
        // A span along a side, the run on the side's high side (the span
        // lies below the side's moved line): the face's side of the span.
        let mut said: Option<bool> = None;
        for &s in l {
            let seg = &segs[s];
            let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
            // The run on the side's high side, the side moved up.
            if (k == 1) == seg.back || self.low_line(k, seg.tile[k], seg.at) {
                continue;
            }
            let line = global(seg.tile[k], seg.at);
            let (alo, ahi) = (
                global(seg.tile[o], seg.ext.lo()),
                global(seg.tile[o], seg.ext.hi()),
            );
            for t in self.face.trims() {
                for sp in t.spans {
                    let Some((c, n)) = on_line(sp, t.reversed, self.w) else {
                        continue;
                    };
                    if c != k || rat(sp[0][k]) != line {
                        continue;
                    }
                    let (x, y) = (rat(sp[0][o]), rat(sp[2][o]));
                    let (x, y) = if x < y { (x, y) } else { (y, x) };
                    if x.max(alo.clone()) < y.min(ahi.clone()) {
                        let inside = n > 0;
                        if said.is_some_and(|v| v != inside) {
                            return None;
                        }
                        said = Some(inside);
                    }
                }
            }
        }
        if said.is_some() {
            return said;
        }
        // A curve's end or knot on the loop: the point next to it on the
        // loop, moved with the lines, lies in the direction `toward`.
        let trims = self.face.trims();
        let ends = || {
            trims.iter().flat_map(|t| {
                t.spans
                    .iter()
                    .flat_map(move |sp| [(sp, false, t.reversed), (sp, true, t.reversed)])
            })
        };
        // The points tried: the curves' ends and knots, then the loop's
        // segment ends (box corners), which a span may pass exactly
        // through without crossing the moved lines.
        let corners = l.iter().flat_map(|&s| {
            let seg = &segs[s];
            [seg.start(), seg.end()].map(|p| p.map(|(i, t)| global(i, t)))
        });
        // And the points where a span touches a side's line tangentially,
        // on the side.
        let mut touch = Vec::new();
        for &s in l {
            let seg = &segs[s];
            let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
            let line = global(seg.tile[k], seg.at);
            let (alo, ahi) = (
                global(seg.tile[o], seg.ext.lo()),
                global(seg.tile[o], seg.ext.hi()),
            );
            for sp in trims.iter().flat_map(|t| t.spans) {
                if let Some(at) = tangency(sp, k, &line) {
                    let (p, _) = Leg::split(sp, &at);
                    if alo <= p[o] && p[o] <= ahi {
                        touch.push(p);
                    }
                }
            }
        }
        let points: Vec<[Rational; 2]> = ends()
            .map(|(sp, at_end, _)| Leg::point(sp, at_end))
            .chain(corners)
            .chain(touch)
            .collect();
        for wp in points {
            // Per axis, whether the point lies on a side of that axis's
            // lines, and which way that line is moved.
            let mut on = [0i8; 2];
            for &s in l {
                let seg = &segs[s];
                let (k, o) = (fixed(seg.dir), 1 - fixed(seg.dir));
                if wp[k] == global(seg.tile[k], seg.at)
                    && global(seg.tile[o], seg.ext.lo()) <= wp[o]
                    && wp[o] <= global(seg.tile[o], seg.ext.hi())
                {
                    on[k] = if self.low_line(k, seg.tile[k], seg.at) {
                        -1
                    } else {
                        1
                    };
                }
            }
            if on == [0, 0] {
                continue;
            }
            let toward = Toward { u: on[0], v: on[1] };
            // Every span leaving that point, with whether its coedge
            // leaves it: those ending there, and those passing through it,
            // split there exactly.
            let mut legs: Vec<(Leg, bool)> = ends()
                .filter(|(usp, end, _)| Leg::point(usp, *end) == wp)
                .map(|(usp, end, reversed)| (Leg::new(usp, end), end == reversed))
                .collect();
            for t in trims {
                for sp in t.spans {
                    if let Some([back, on]) = legs_through(sp, &wp) {
                        legs.push((back, t.reversed));
                        legs.push((on, !t.reversed));
                    }
                }
            }
            if let Some(v) = inside_toward(&legs, toward, self.w) {
                return Some(v);
            }
        }
        None
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
        let pw = [(hit.pspan, hit.t)];
        let here = seg_rect(seg, hit.pos);
        let Some((r, c)) = self.ribbon_hull(hit.member, &pw)? else {
            return Err(self.unsettled(here));
        };
        let (elo, ehi) = (seg.ext.lo(), seg.ext.hi());
        for d in 1..=WINDOW_DEPTH {
            let s = (1u64 << d) as f64;
            // Exact: scaling by a power of two.
            let mut an = (hit.pos[0] * s).floor() as u64;
            let mut bn = ((hit.pos[1] * s).ceil() as u64).min(1u64 << d);
            // A crossing exactly at a dyadic point: the stretch next to
            // it, inwards.
            if an >= bn {
                if Dyadic::new(bn, d).is_some_and(|x| x < ehi) {
                    bn = an + 1;
                } else {
                    an = bn - 1;
                }
            }
            let a = Dyadic::new(an, d).expect("in [0, 1]");
            let b = Dyadic::new(bn, d).expect("in [0, 1]");
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
        Err(self.unsettled(here))
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
        let here = seg_rect(seg, [dy(x), dy(y)]);
        if !patch.weights_positive() {
            return Err(self.unsettled(here));
        }
        let mut n = 0u64;
        let laps = rec_turn(
            patch, self.of, seg.tile, seg.dir, seg.at, ext, &self.q, self.point, self.sides, given,
            &mut n,
        );
        self.pieces = self.pieces.saturating_add(n);
        let laps = laps.map_err(|e| match e {
            RecTurnError::Need(k) => PieceError::Need(k),
            RecTurnError::Through => {
                self.unsettled(here);
                PieceError::Through
            }
            RecTurnError::Unsettled => self.unsettled(here),
        })?;
        Ok(if seg.back { -laps } else { laps })
    }
}

/// Sorts the hits `on` of one segment along it (increasing local
/// position): hits at one exact point by their germs. Whether they are
/// apart: no two enclosures overlap but at one exact point with germs
/// told apart.
fn along_side(hits: &[Hit], on: &mut [usize]) -> bool {
    on.sort_by(|&a, &b| hits[a].pos[0].total_cmp(&hits[b].pos[0]));
    let same = |a: &Hit, b: &Hit| match (&a.exact, &b.exact) {
        (Some((x, _)), Some((y, _))) => x == y,
        _ => false,
    };
    // Each stretch of hits at one exact point, by germ.
    let mut i = 0;
    while i < on.len() {
        let mut j = i + 1;
        while j < on.len() && same(&hits[on[i]], &hits[on[j]]) {
            j += 1;
        }
        let part = &mut on[i..j];
        for x in 1..part.len() {
            let mut y = x;
            while y > 0 {
                let (Some((_, g)), Some((_, h))) = (&hits[part[y - 1]].exact, &hits[part[y]].exact)
                else {
                    return false;
                };
                match g.cmp(h) {
                    None => return false,
                    Some(Ordering::Greater) => part.swap(y - 1, y),
                    Some(_) => break,
                }
                y -= 1;
            }
        }
        i = j;
    }
    on.windows(2).all(|p| {
        let (a, b) = (&hits[p[0]], &hits[p[1]]);
        a.pos[1] < b.pos[0] || same(a, b)
    })
}

/// Whether hits `a` and `b` of one ribbon are where its curve passes one
/// point exactly (a knot or a pass through a box corner) on one p-span
/// (parameters overlapping), crossing two outer sides there: the run's
/// corner next to the point, moved, clipped (module docs, "Box lines read
/// as `c + 0⁺`").
fn twins(a: &Hit, b: &Hit) -> bool {
    a.member == b.member
        && a.point.is_some()
        && a.point == b.point
        && a.pspan == b.pspan
        && a.t[0] <= b.t[1]
        && b.t[0] <= a.t[1]
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

/// Whether the UV point `p` lies in the run of `boxes` of `face`: in a box
/// `[lo, hi]` where `lo < p ≤ hi` in each coordinate, `lo ≤ p` at the low
/// edge of the face's tiles (module docs, "Box lines read as `c + 0⁺`").
/// Exact.
fn place(face: &FaceUv<'_>, boxes: &[BoxKey], p: [f64; 2]) -> bool {
    let p = p.map(rat);
    boxes.iter().any(|b| {
        [Dir::U, Dir::V].into_iter().all(|d| {
            let (e, k) = (b.extent(d), fixed(d));
            let lo = global(b.tile()[k], e.lo());
            let above = if low_edge(face, k, b.tile()[k], e.lo()) {
                lo <= p[k]
            } else {
                lo < p[k]
            };
            above && p[k] <= global(b.tile()[k], e.hi())
        })
    })
}

/// Where span `sp` crosses the line `x_k = line` at the isolated simple
/// root in `span` and passes exactly through the line's point `x_o = e`
/// there: the tangent's slope `dx_o/dx_k` there, its germ's first term
/// (module docs, "Box lines read as `c + 0⁺`"). `None` where it doesn't
/// pass there (exactly, at a rational parameter, or along a line through
/// the point).
fn corner_pass(
    sp: &UvSpan,
    k: usize,
    line: &Rational,
    e: &Rational,
    span: [f64; 2],
) -> Option<Rational> {
    let o = 1 - k;
    let (pk, po) = (offset(sp, k, line), offset(sp, o, e));
    top(&pk)?;
    if top(&po).is_none() {
        // Along the line `x_o = e`.
        return Some(Rational::from(0));
    }
    let g = gcd(pk.clone(), po.clone());
    let slope = match top(&g) {
        Some(1) => {
            let t = -&g[0] / &g[1];
            if t < rat(span[0]) || t > rat(span[1]) {
                return None;
            }
            let a = peval(&pderiv(&pk), &t);
            if a == 0 {
                return None;
            }
            peval(&pderiv(&po), &t) / a
        }
        // One a multiple of the other: a line through the point.
        Some(2) => &po[2] / &pk[2],
        _ => return None,
    };
    Some(slope)
}

/// `Σ wᵢ·(xᵢ − c)·Bᵢ(t)` of span `sp` along `axis` in power form: the
/// weight times `x − c`, exactly.
fn offset(sp: &UvSpan, axis: usize, c: &Rational) -> Poly {
    let two = Rational::from(2);
    let x: [Rational; 3] = [0, 1, 2].map(|i| rat(sp[i][2]) * (rat(sp[i][axis]) - c));
    vec![
        x[0].clone(),
        &two * (&x[1] - &x[0]),
        &x[0] - &two * &x[1] + &x[2],
    ]
}

/// Span `sp` (its weights positive) read from the point `p` it passes
/// exactly through strictly inside it, not at an end: back towards its
/// start, then on towards its end ([`Leg::split`] at the exact parameter
/// there; a span on a line through `p`, running monotonically along it, as
/// its two straight halves). `None` where it doesn't pass there, or passes
/// there twice, or at a parameter that isn't rational.
fn legs_through(sp: &UvSpan, p: &[Rational; 2]) -> Option<[Leg; 2]> {
    // The convex hull holds the span: a first test in floats.
    for c in 0..2 {
        let (lo, hi) = sp
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                (
                    if x[c] < a { x[c] } else { a },
                    if x[c] > b { x[c] } else { b },
                )
            });
        if !(lo.is_finite() && hi.is_finite()) || p[c] < rat(lo) || p[c] > rat(hi) {
            return None;
        }
    }
    let (pu, pv) = (offset(sp, 0, &p[0]), offset(sp, 1, &p[1]));
    let g = gcd(pu.clone(), pv.clone());
    match (top(&pu), top(&pv), top(&g)) {
        (Some(_), Some(_), Some(1)) => {
            let t = -&g[0] / &g[1];
            (t > 0 && t < 1).then(|| Leg::split(sp, &t).1)
        }
        // On a line through `p` (or an iso line of it): through `p` once
        // where the middle control point and `p` lie strictly between the
        // ends along the line.
        (Some(2), Some(2), Some(2)) | (None, Some(_), _) | (Some(_), None, _) => {
            let pts: [[Rational; 2]; 3] = sp.map(|x| [rat(x[0]), rat(x[1])]);
            let between = |x: &[Rational; 2]| {
                (0..2).all(|c| {
                    let (a, b) = (&pts[0][c], &pts[2][c]);
                    (a < &x[c] && &x[c] < b)
                        || (b < &x[c] && &x[c] < a)
                        || (a == &x[c] && &x[c] == b)
                }) && x != &pts[0]
            };
            let on = top(&pu).is_some() && top(&pv).is_some()
                || (0..2).any(|c| pts.iter().all(|x| x[c] == p[c]));
            (on && between(&pts[1]) && between(p))
                .then(|| [Leg::straight(p, &pts[0]), Leg::straight(p, &pts[2])])
        }
        _ => None,
    }
}

/// The parameter strictly inside `(0, 1)` where span `sp` touches the
/// line `x_k = line` tangentially, a double root of its equation, exactly;
/// `None` where it doesn't.
fn tangency(sp: &UvSpan, k: usize, line: &Rational) -> Option<Rational> {
    let p = offset(sp, k, line);
    if p[2] == 0 || &p[1] * &p[1] != Rational::from(4) * &p[0] * &p[2] {
        return None;
    }
    let t = -&p[1] / (Rational::from(2) * &p[2]);
    (t > 0 && t < 1).then_some(t)
}

/// The degree of `p`, `None` for zero.
fn top(p: &[Rational]) -> Option<usize> {
    p.iter().rposition(|x| *x != 0)
}

/// The greatest common divisor of two polynomials in power form, exact
/// (up to a factor).
fn gcd(mut a: Poly, mut b: Poly) -> Poly {
    while let Some(db) = top(&b) {
        // `a mod b`.
        while let Some(da) = top(&a) {
            if da < db {
                break;
            }
            let f = &a[da] / &b[db];
            for i in 0..=db {
                let x = &f * &b[i];
                a[da - db + i] -= x;
            }
        }
        std::mem::swap(&mut a, &mut b);
    }
    a
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
    strip_degree_near(face, patches, boxes, ribbons, q, point, sides).0
}

/// [`strip_degree`], with, where the piece is [`PieceError::Unsettled`]
/// or [`PieceError::Through`], where as far as known: a closed rectangle
/// of global UV holding every cut, side stretch, curve end or uncut loop
/// found unsettled before the piece gave up (`None`: anywhere in the
/// run). A hint for the caller to refine the run's boxes there and leave
/// the rest, not a certificate.
pub fn strip_degree_near<S: Scalar>(
    face: &FaceUv<'_>,
    patches: &[HomPatch<S>],
    boxes: &[BoxKey],
    ribbons: &[Ribbon<'_>],
    q: &QShadow,
    point: Point,
    sides: &Sides,
) -> (Result<PieceDegree, PieceError>, Option<UvRect>) {
    let mut near = None;
    let r = piece_degree(face, patches, boxes, ribbons, q, point, sides, &mut near);
    let near = if matches!(r, Err(PieceError::Unsettled | PieceError::Through)) {
        near
    } else {
        None
    };
    (r, near)
}

#[allow(clippy::too_many_arguments)]
fn piece_degree<S: Scalar>(
    face: &FaceUv<'_>,
    patches: &[HomPatch<S>],
    boxes: &[BoxKey],
    ribbons: &[Ribbon<'_>],
    q: &QShadow,
    point: Point,
    sides: &Sides,
    near: &mut Option<UvRect>,
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
        w,
        slit,
        pieces: 0,
        near: None,
    };
    let r = run_degree(&mut ctx, boxes);
    *near = ctx.near;
    r
}

/// The degree of the run `boxes` (module docs), its ribbons and
/// trims those of `ctx`.
fn run_degree<S: Scalar>(
    ctx: &mut Ctx<'_, '_, '_, S>,
    boxes: &[BoxKey],
) -> Result<PieceDegree, PieceError> {
    let (face, ribbons, w, slit) = (ctx.face, ctx.ribbons, ctx.w, ctx.slit);
    let index: SmallVec<[usize; 2]> = ctx
        .trims
        .iter()
        .map(|t| face.trim_index(t.coedge).expect("a trim of the face"))
        .collect();
    let segs = outer(boxes)?;
    let loops = loops(&segs)?;

    // The curves' ends inside the run: every trim ending there is a
    // ribbon, and at each vertex (and UV point) as many curves end as
    // start, so the radials there cancel.
    let mut ends_in: SmallVec<[[bool; 2]; 2]> = smallvec::smallvec![[false; 2]; ribbons.len()];
    let mut balance: BTreeMap<(VertId, [u64; 2]), i32> = BTreeMap::new();
    for (i, t) in face.trims().iter().enumerate() {
        if t.spans.is_empty() {
            continue;
        }
        let m = index.iter().position(|&j| j == i);
        if is_closed(t) {
            // A closed trim inside the run is one of its ribbons.
            if m.is_none() && place(face, boxes, coedge_ends(t)[0].2) {
                return Err(PieceError::Open { coedge: t.coedge });
            }
            continue;
        }
        for (k, (v, bits, p)) in coedge_ends(t).into_iter().enumerate() {
            if !place(face, boxes, p) {
                continue;
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
            let t = ctx.trims[m];
            ctx.hits(t, m, si, seg, &mut hits)?;
        }
    }
    // No other trim crosses the outer boundary.
    for (i, t) in face.trims().iter().enumerate() {
        if index.contains(&i) {
            continue;
        }
        let mut other = Vec::new();
        for (si, seg) in segs.iter().enumerate() {
            ctx.hits(t, usize::MAX, si, seg, &mut other)?;
            if !other.is_empty() {
                return Err(PieceError::Open { coedge: t.coedge });
            }
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
            if !along_side(&hits, &mut on) && single {
                let r = on
                    .iter()
                    .map(|&k| seg_rect(&segs[s], hits[k].pos))
                    .reduce(hull);
                return Err(ctx.unsettled_at(r));
            }
            if segs[s].back {
                on.reverse();
            }
            order.push((s, on));
        }
        let mut m = if single {
            match order.iter().rev().find_map(|(_, on)| on.last()) {
                Some(last) => inside_after(&hits[*last]),
                None => match ctx.uncut(l, &segs) {
                    Some(inside) => inside,
                    None => {
                        let r = l
                            .iter()
                            .map(|&s| {
                                seg_rect(&segs[s], [dy(segs[s].ext.lo()), dy(segs[s].ext.hi())])
                            })
                            .reduce(hull);
                        return Err(ctx.unsettled_at(r));
                    }
                },
            }
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
                        return Err(ctx.unsettled(seg_rect(&seg, h.pos)));
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
        // Where this ribbon's cuts and ends in the run are, for the
        // failures below.
        let ends = coedge_ends(t);
        let at = mine
            .iter()
            .map(|h| seg_rect(&segs[h.seg], h.pos))
            .chain(
                (0..2)
                    .filter(|&k| {
                        ends_in[m][k] || (closed && k == 0 && place(face, boxes, ends[0].2))
                    })
                    .map(|k| point_rect(&ends[k].2)),
            )
            .reduce(hull);
        if mine
            .windows(2)
            .any(|p| p[0].pspan == p[1].pspan && p[0].t[1] >= p[1].t[0] && !twins(p[0], p[1]))
        {
            return Err(ctx.unsettled_at(at));
        }
        if t.reversed {
            mine.reverse();
        }
        let entry = |h: &Hit| h.sigma < 0;
        // Twins in the order the curve meets them: entries and exits
        // alternate along the coedge from its start's side of the run.
        let mut inside = if closed {
            place(face, boxes, ends[0].2)
        } else {
            ends_in[m][0]
        };
        for k in 0..mine.len() {
            if k + 1 < mine.len()
                && entry(mine[k]) == inside
                && twins(mine[k], mine[k + 1])
                && entry(mine[k + 1]) != inside
            {
                mine.swap(k, k + 1);
            }
            inside = !inside;
        }
        let mut count = 0i64;
        if closed {
            if mine.is_empty() {
                // The whole loop inside the run, or clear of it.
                if place(face, boxes, coedge_ends(t)[0].2) {
                    count = count_between(r.rays, None, None, false)
                        .map_err(|_| ctx.unsettled_at(at))?;
                }
            } else {
                // The last pair wraps past the start where the curve
                // starts inside the run.
                let wraps = !entry(mine[0]);
                if wraps {
                    mine.rotate_left(1);
                }
                if !mine.len().is_multiple_of(2)
                    || mine
                        .iter()
                        .enumerate()
                        .any(|(k, h)| entry(h) != (k % 2 == 0))
                {
                    return Err(ctx.unsettled_at(at));
                }
                let last = mine.len() / 2 - 1;
                for (k, pair) in mine.chunks(2).enumerate() {
                    if twins(pair[0], pair[1]) {
                        // A clip of the run at one point: nothing between;
                        // wrapping, the curve leaves the run only there
                        // (its only cuts): all of it.
                        if wraps && k == last {
                            count += count_between(r.rays, None, None, false)
                                .map_err(|_| ctx.unsettled_at(at))?;
                        }
                        continue;
                    }
                    let (e, x) = (win(pair[0]).c, win(pair[1]).c);
                    // Along the stored direction, from `lo` to `hi`.
                    let (lo, hi) = if t.reversed { (x, e) } else { (e, x) };
                    // The windows apart along the edge.
                    if !(lo[1] < hi[0] || hi[1] < lo[0]) {
                        return Err(ctx.unsettled_at(at));
                    }
                    count += count_between(r.rays, Some(lo), Some(hi), hi[0] < lo[0])
                        .map_err(|_| ctx.unsettled_at(at))?;
                }
            }
        } else {
            // Stretches in the coedge's order: from its start if inside
            // the run, between an entry and the next exit, to its end if
            // inside the run.
            let [start_in, end_in] = ends_in[m];
            let mut marks: Vec<Option<&Hit>> = Vec::new();
            if start_in {
                marks.push(None);
            }
            for (k, h) in mine.iter().enumerate() {
                if entry(h) != ((k + usize::from(start_in)).is_multiple_of(2)) {
                    return Err(ctx.unsettled_at(at));
                }
                marks.push(Some(h));
            }
            if end_in {
                marks.push(None);
            }
            if !marks.len().is_multiple_of(2) {
                return Err(ctx.unsettled_at(at));
            }
            for pair in marks.chunks(2) {
                // A clip of the run at one point: nothing between.
                if let [Some(a), Some(b)] = pair
                    && twins(a, b)
                {
                    continue;
                }
                let (e, x) = (pair[0].map(|h| win(h).c), pair[1].map(|h| win(h).c));
                let (lo, hi) = if t.reversed { (x, e) } else { (e, x) };
                // Windows touching at one point (two cuts of one knot) hold
                // nothing between them.
                if let (Some(lo), Some(hi)) = (lo, hi)
                    && lo[1] > hi[0]
                {
                    return Err(ctx.unsettled_at(at));
                }
                count += count_between(r.rays, lo, hi, false).map_err(|_| ctx.unsettled_at(at))?;
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

/// Where an edge's crossing in a corner piece is placed (module docs,
/// "Ends in corner pieces").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerEnd {
    /// At the corner's vertex.
    Vertex,
    /// A strip end on this coedge's curve, at the crossing's point on it.
    Curve(CoedgeId),
}

/// Where a crossing in a corner piece is placed: `part` the hull of the
/// piece's part holding the crossing's cluster, `w` the vertex's point,
/// `r_w` its near radius, `named` the corner's curves the cluster names,
/// each with its key in the geometric order (module docs). `None` where
/// the part isn't certified within `r_w` and no curve is named.
pub fn corner_end<K: Ord>(
    part: &HullBox,
    w: [f64; 3],
    r_w: f64,
    named: &[(K, CoedgeId)],
) -> Option<CornerEnd> {
    if within_of(part, w, r_w) {
        return Some(CornerEnd::Vertex);
    }
    named
        .iter()
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|&(_, c)| CornerEnd::Curve(c))
}

#[cfg(test)]
mod tests;

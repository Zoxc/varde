//! What a mixed box is to the counting: a **strip** box of one trimming
//! curve (or of a slit's coedges, which share one curve), a **corner**
//! box of a vertex, or a **thin** box that must refine ([`role`]); the
//! **tubes** round the trimming curves in space ([`in_tube`]); and the
//! **corner component** of each vertex ([`Corners`]), which takes an acute
//! or cusp corner's whole spike, not only the box holding the vertex.
//!
//! **Space.** A trimming curve `Γ_c` is the coedge's p-curve carried onto
//! the surface, `S_f(p_c)`, in 3D; its **tube** is `{x : dist(x, Γ_c) ≤
//! β_c}`, `β_c` its edge's counting band ([`Trim::band`]), the width the
//! ribbon between `S_f(p_c)` and the edge's own curve lives in. The 3D
//! hull of a box is its sub-patch's ([`sub_patch`]); that of a piece of a
//! p-curve is the surface's over the piece's UV control box, cut tile by
//! tile from the tiles' patches (one per tile of the face, in
//! [`FaceUv::tile_boxes`]' order). A set in a hull box `A` lies within
//! `β` of a curve when some piece `Q` of the curve has `A`'s farthest
//! point from `Q`'s hull box at most `β` (the boxes' greatest distance,
//! per axis the larger of the two end gaps, rounded upwards, against `β²`
//! rounded downwards): every point of `A` is then within `β` of every
//! point of `Q`, a point of the curve among them. The pieces are the
//! curve's spans halved breadth first, a piece dropped where no point of
//! its hull box has all of `A` within `β` (per axis the least over the
//! piece's box of the greatest distance to `A`, rounded downwards), at
//! most [`NEAR_DEPTH`] deep and [`NEAR_PIECES`] pieces per search; a
//! piece whose hull is within a sixteenth of `β` across and still doesn't
//! certify isn't halved further (its halves would gain less than that). A box is **in the tube** of `c`
//! ([`in_tube`]) when its hull is within `β_c` of `Γ_c` so; the
//! certificate needs the box's diameter below `β_c`, the strip and corner
//! boxes' own stopping size ([`within_band`]).
//!
//! **Strips and slits.** A mixed box ([`Class::Mixed`]) whose met
//! coedges are one coedge, or coedges whose p-curves are bitwise one
//! curve (a slit: the same spans in the same order, or each reversed in
//! the reverse order, by their bits), and none of zero length, is a
//! **strip** box of those coedges. The slit's members with one space
//! curve have ribbons of opposite orientation, which cancel, and members
//! whose space curves differ give one strip each; the pieces built on
//! the boxes tell the two apart, the box only names the set.
//!
//! **Corners.** The vertices joined by zero-length trims are one
//! **corner** ([`Corner`]), named by its least vertex; the corner holds
//! its zero-length trims, and its curves are the other trims with an end
//! at one of its vertices (a vertex a loop passes twice, or two loops
//! share, is one corner). The corner's **component** is the component
//! holding the vertex of the set of points within the band of two of its
//! trims at once, `⋃ {x : dist(x, Γ_a) ≤ β_a, dist(x, Γ_b) ≤ β_b}`
//! over pairs `a ≠ b` of its curves and zero-length trims (whose `Γ` is
//! the vertex's point, so with a curve they add the curve's stretch
//! within a band of the vertex, and a corner of one curve and a
//! zero-length trim has a component too): at a convex or reflex corner about
//! a band round the vertex, at an acute corner the wedge out to where its
//! sides are a band apart, at a cusp (a curve leaving the vertex tangent
//! to another) the whole spike between them, about `√(2Rβ)` long for a
//! line tangent to an arc of radius `R`. On each curve the component
//! holds a **run** from the corner ([`Run`]): the stretch from the
//! curve's end at the corner certified within the band of another of the
//! corner's trims, piece by piece (each piece's hull within `β_b` of
//! `Γ_b`, as above), the span at the end first, each span halved from the
//! end inwards, a piece that doesn't certify halved until one does, and
//! the run ending at the first piece that doesn't once it is within an
//! eighth of the least band across (halving it would move the run's end
//! by less), [`RUN_DEPTH`] deep, or past [`RUN_PIECES`]. Every piece of a run lies on its own
//! curve and within another trim's band, the run is connected and holds
//! the vertex, so the run lies in the component. A mixed box is a
//! **corner** box of the corner when every coedge it meets is a curve or
//! a zero-length trim of the corner, and every piece of a curve that the
//! box isn't separated from ([`classify`]'s test) lies in that curve's
//! runs at the corner: it meets the curves only inside the component. A
//! box at a spike, anywhere along it, meets the two curves only inside
//! their runs once it is a fraction of a band across, so the spike goes
//! to the corner and not to a strip or a refinement that never ends;
//! beyond the runs the curves are apart by about a band, and a box that
//! small meets one of them only.
//!
//! **Keys.** The runs are a function of the operands: the corners are
//! built once per operation at the first level ([`Corners::new`]), from
//! the stored trims, the operation's bands and the first level's
//! patches, never again by a precision step. A run is a certified part
//! of the component, which is all a corner needs (a shorter run leaves
//! more boxes to refine; it never mislabels one). Which boxes are corner
//! boxes then depends on their level only as [`classify`]'s mixed boxes
//! do: a box's pieces separate further at a higher level.
//!
//! **Thin.** Any other mixed box (two curves that are not one corner's,
//! or meeting a curve outside its runs) is **thin**: it refines in space,
//! then in precision.

use std::collections::VecDeque;

use smallvec::SmallVec;

use super::{
    BoxError, BoxKey, Class, Classified, FaceUv, SEP_PIECES, SpanRef, Trim, classify_in, cut,
    met_within, span_error, sub_patch, uv_curve,
};
use crate::bezier::{Dir, Dyadic, HomCurve, HomPatch, HullBox, Scalar};
use crate::ids::{CoedgeId, VertId};
use crate::num::bounded::{Bounded, add_up, down, up};

#[cfg(doc)]
use super::classify;

/// The most halvings of a span building a run.
pub const RUN_DEPTH: u32 = 52;

/// The most pieces one run tests.
pub const RUN_PIECES: u32 = 4096;

/// The most halvings of a span searching for a piece near a hull.
pub const NEAR_DEPTH: u32 = 52;

/// The most pieces one search for a near piece tests.
pub const NEAR_PIECES: u32 = 256;

/// The most tiles the hull of one piece of a p-curve is cut from; a piece
/// spread over more is halved first.
pub const HULL_TILES: u64 = 16;

/// What a box is to the counting (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// Certified outside the trim and clear of every trimming curve.
    Outside,
    /// Certified inside the trim and clear of every trimming curve.
    Core,
    /// A strip box of these coedges, ascending: one coedge, or a slit's
    /// coedges, whose p-curves are bitwise one curve.
    Strip(SmallVec<[CoedgeId; 4]>),
    /// A corner box of the corner named by its least vertex `vertex`,
    /// meeting the coedges `met` (ascending) inside the corner's
    /// component.
    Corner {
        /// The corner's least vertex.
        vertex: VertId,
        /// The coedges the box meets.
        met: SmallVec<[CoedgeId; 4]>,
    },
    /// Meets these coedges (ascending) otherwise: refine.
    Thin(SmallVec<[CoedgeId; 4]>),
}

/// A box's role, with the work it took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roled {
    /// The role.
    pub role: Role,
    /// The span pieces tested.
    pub pieces: u64,
    /// The spans the box isn't separated from ([`Classified::met`]),
    /// for its children ([`role`]'s `among`).
    pub met: SmallVec<[SpanRef; 8]>,
}

/// The run of a curve from its end at a corner (module docs): every
/// piece of the curve between the end and the run's far end. The far end
/// is a span and a dyadic parameter of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    /// The curve's end at the corner: `0` the first span's start, `1` the
    /// last span's end.
    end: u8,
    /// The span the run ends in.
    span: u32,
    /// Where in that span: the run holds `[at, 1]` of it from end `1`,
    /// `[0, at]` from end `0`.
    at: Dyadic,
}

impl Run {
    /// The run of a trim of `n` spans that holds only its end `end`.
    fn empty(end: u8, n: u32) -> Run {
        if end == 0 {
            Run {
                end,
                span: 0,
                at: Dyadic::ZERO,
            }
        } else {
            Run {
                end,
                span: n.saturating_sub(1),
                at: Dyadic::ONE,
            }
        }
    }

    /// The trim's end the run starts from: `0` the first span's start,
    /// `1` the last span's end.
    pub fn end(&self) -> u8 {
        self.end
    }

    /// The span the run ends in and the parameter there.
    pub fn reach(&self) -> (u32, Dyadic) {
        (self.span, self.at)
    }

    /// Whether the run holds the piece `[k/2^d, (k + 1)/2^d]` of span
    /// `span`; `None` unless `k < 2^d` and `d ≤ 62`.
    pub fn holds(&self, span: u32, (k, d): (u64, u32)) -> Option<bool> {
        if d > 62 || k >= 1u64 << d {
            return None;
        }
        let lo = Dyadic::new(k, d)?;
        let hi = Dyadic::new(k + 1, d)?;
        Some(if self.end == 0 {
            span < self.span || (span == self.span && hi <= self.at)
        } else {
            span > self.span || (span == self.span && lo >= self.at)
        })
    }
}

/// A corner of a face (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corner {
    /// Its least vertex, its name.
    pub vertex: VertId,
    /// Its vertices, ascending: one, or several joined by zero-length
    /// trims.
    pub vertices: SmallVec<[VertId; 2]>,
    /// Its curves: the coedges of positive length with an end at one of
    /// its vertices, ascending.
    pub curves: SmallVec<[CoedgeId; 4]>,
    /// Its zero-length coedges, ascending.
    pub zero: SmallVec<[CoedgeId; 2]>,
}

/// The corners of a face and the runs of their curves (module docs),
/// built once per operation.
#[derive(Clone, Debug)]
pub struct Corners {
    corners: Vec<Corner>,
    /// Per trim (the face's order), the corner of each end.
    ends: Vec<[u32; 2]>,
    /// Per trim, the run from each end (unused for zero-length trims).
    runs: Vec<[Run; 2]>,
    pieces: u64,
}

impl Corners {
    /// The corners of `face` and their runs, from the first level's
    /// patches of its tiles (one per tile, in
    /// [`FaceUv::tile_boxes`]' order).
    pub fn new(face: &FaceUv<'_>, patches: &[HomPatch<Bounded<f64>>]) -> Result<Corners, BoxError> {
        if !face.patches_fit(patches) {
            return Err(BoxError::Patches);
        }
        let trims = face.trims();
        // The vertices, joined by the zero-length trims (union by the
        // lesser index, so each set's root is its least vertex).
        let mut verts: Vec<VertId> = trims.iter().flat_map(|t| t.ends).collect();
        verts.sort_unstable();
        verts.dedup();
        let index = |v: VertId| verts.binary_search(&v).expect("collected");
        let mut parent: Vec<usize> = (0..verts.len()).collect();
        fn root(parent: &mut [usize], mut k: usize) -> usize {
            while parent[k] != k {
                parent[k] = parent[parent[k]];
                k = parent[k];
            }
            k
        }
        for t in trims.iter().filter(|t| t.zero_length) {
            let (a, b) = (
                root(&mut parent, index(t.ends[0])),
                root(&mut parent, index(t.ends[1])),
            );
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            parent[hi] = lo;
        }
        // Corners in order of their least vertex.
        let mut slot = vec![u32::MAX; verts.len()];
        let mut corners: Vec<Corner> = Vec::new();
        for k in 0..verts.len() {
            let r = root(&mut parent, k);
            if slot[r] == u32::MAX {
                // Fits: at most twice the trims, whose count fits.
                slot[r] = corners.len() as u32;
                corners.push(Corner {
                    vertex: verts[r],
                    vertices: SmallVec::new(),
                    curves: SmallVec::new(),
                    zero: SmallVec::new(),
                });
            }
            slot[k] = slot[r];
            corners[slot[k] as usize].vertices.push(verts[k]);
        }
        let ends: Vec<[u32; 2]> = trims
            .iter()
            .map(|t| t.ends.map(|v| slot[index(v)]))
            .collect();
        for (t, e) in trims.iter().zip(&ends) {
            for &c in e {
                let corner = &mut corners[c as usize];
                if t.zero_length {
                    corner.zero.push(t.coedge);
                } else {
                    corner.curves.push(t.coedge);
                }
            }
        }
        for c in &mut corners {
            c.curves.sort_unstable();
            c.curves.dedup();
            c.zero.sort_unstable();
            c.zero.dedup();
        }
        let like = Bounded::exact(0.0);
        let mut pieces = 0u64;
        let mut runs = Vec::with_capacity(trims.len());
        for (k, t) in trims.iter().enumerate() {
            // Fits: the face checked the span count fits a `u32`.
            let n = t.spans.len() as u32;
            let mut r = [Run::empty(0, n), Run::empty(1, n)];
            if !t.zero_length && n > 0 {
                for end in 0..2u8 {
                    let corner = &corners[ends[k][usize::from(end)] as usize];
                    let others: SmallVec<[usize; 4]> = corner
                        .curves
                        .iter()
                        .chain(&corner.zero)
                        .filter(|&&c| c != t.coedge)
                        .map(|&c| face.trim_index(c).expect("the face's coedge"))
                        .collect();
                    if others.is_empty() {
                        continue;
                    }
                    let ctx = Space {
                        face,
                        patches,
                        like: &like,
                    };
                    let (run, p) = run_from(&ctx, t, end, &others)?;
                    r[usize::from(end)] = run;
                    pieces = pieces.saturating_add(p);
                }
            }
            runs.push(r);
        }
        Ok(Corners {
            corners,
            ends,
            runs,
            pieces,
        })
    }

    /// The corners, in order of their least vertex.
    pub fn corners(&self) -> &[Corner] {
        &self.corners
    }

    /// The corner named by its least vertex `v`.
    pub fn corner(&self, v: VertId) -> Option<&Corner> {
        self.corners
            .binary_search_by_key(&v, |c| c.vertex)
            .ok()
            .map(|k| &self.corners[k])
    }

    /// The run of `coedge`'s curve from its end `end` (`0` the first
    /// span's start, `1` the last span's end) and the corner it starts
    /// at; `None` for a coedge not of the face or of zero length, or an
    /// end other than 0 and 1.
    pub fn run(&self, face: &FaceUv<'_>, coedge: CoedgeId, end: u8) -> Option<(&Corner, Run)> {
        let k = face.trim_index(coedge)?;
        let (t, e) = (face.trims().get(k)?, usize::from(end));
        if t.zero_length || e > 1 || k >= self.runs.len() {
            return None;
        }
        Some((&self.corners[self.ends[k][e] as usize], self.runs[k][e]))
    }

    /// The pieces tested building the runs.
    pub fn pieces(&self) -> u64 {
        self.pieces
    }
}

/// The face's surface at a level: its trims and its tiles' patches.
struct Space<'f, 'p, 'l, S> {
    face: &'f FaceUv<'f>,
    patches: &'p [HomPatch<S>],
    like: &'l S,
}

/// The run of trim `t`'s curve from its end `end`, within the band of
/// one of the trims `others` (module docs), and the pieces tested.
fn run_from<S: Scalar>(
    ctx: &Space<'_, '_, '_, S>,
    t: &Trim<'_>,
    end: u8,
    others: &[usize],
) -> Result<(Run, u64), BoxError> {
    let n = t.spans.len();
    let mut budget = RUN_PIECES;
    let mut tested = 0u64;
    let order: Vec<usize> = if end == 0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    for j in order {
        let c = uv_curve(&t.spans[j], ctx.like).ok_or(span_error(t, j))?;
        // The piece with the corner's end at its parameter 0.
        let c = if end == 0 { c } else { c.reversed() };
        if let Some((k, d)) = reach(ctx, &c, (0, 0), others, &mut budget, &mut tested)? {
            let at = if end == 0 {
                Dyadic::new(k, d)
            } else {
                // `k ≤ 2^d`, `d ≤ RUN_DEPTH`: the complement fits.
                Dyadic::new((1u64 << d) - k, d)
            }
            .expect("within the run's depth");
            let run = Run {
                end,
                // Fits: the face checked the span count.
                span: j as u32,
                at,
            };
            return Ok((run, tested.saturating_add(u64::from(RUN_PIECES - budget))));
        }
    }
    let run = if end == 0 {
        Run {
            end,
            span: n.saturating_sub(1) as u32,
            at: Dyadic::ONE,
        }
    } else {
        Run {
            end,
            span: 0,
            at: Dyadic::ZERO,
        }
    };
    Ok((run, tested.saturating_add(u64::from(RUN_PIECES - budget))))
}

/// How far from its parameter 0 the piece `c`, `[k/2^d, (k + 1)/2^d]` of
/// its span oriented from the corner, lies within the band of one of
/// `others`: `None` all of it, else the parameter `(k', d')` where the
/// run stops (module docs).
fn reach<S: Scalar>(
    ctx: &Space<'_, '_, '_, S>,
    c: &HomCurve<S, 2>,
    (k, d): (u64, u32),
    others: &[usize],
    budget: &mut u32,
    tested: &mut u64,
) -> Result<Option<(u64, u32)>, BoxError> {
    if *budget == 0 {
        return Ok(Some((k, d)));
    }
    *budget -= 1;
    let h = piece_hull(ctx, c);
    if let Some(h) = &h {
        for &o in others {
            let t = &ctx.face.trims()[o];
            let (near, n) = near_curve(ctx, h, t, t.band)?;
            *tested = tested.saturating_add(n);
            if near {
                return Ok(None);
            }
        }
    }
    // A piece already within an eighth of the least band that doesn't
    // certify ends the run: halving it moves the run's end by less.
    let fine = h.is_some_and(|h| within_band(&h, least_band(ctx, others) * 0.125));
    if d >= RUN_DEPTH || fine {
        return Ok(Some((k, d)));
    }
    let [l, r] = c.split_half();
    if let Some(stop) = reach(ctx, &l, (2 * k, d + 1), others, budget, tested)? {
        return Ok(Some(stop));
    }
    reach(ctx, &r, (2 * k + 1, d + 1), others, budget, tested)
}

/// The least band among the trims `others`.
fn least_band<S>(ctx: &Space<'_, '_, '_, S>, others: &[usize]) -> f64 {
    others
        .iter()
        .map(|&o| ctx.face.trims()[o].band)
        .fold(f64::INFINITY, |m, b| if b < m { b } else { m })
}

/// Whether the hull `h` lies within `beta` of trim `t`'s curve (module
/// docs), and the pieces tested.
fn near_curve<S: Scalar>(
    ctx: &Space<'_, '_, '_, S>,
    h: &HullBox,
    t: &Trim<'_>,
    beta: f64,
) -> Result<(bool, u64), BoxError> {
    let b2 = [down(beta * beta), up(beta * beta)];
    // Breadth first, the spans in order and each piece's halves in order,
    // so the budget goes to the coarsest pieces first.
    let mut queue = VecDeque::new();
    for (k, s) in t.spans.iter().enumerate() {
        queue.push_back((uv_curve(s, ctx.like).ok_or(span_error(t, k))?, 0u32));
    }
    let mut tested = 0u64;
    while let Some((c, depth)) = queue.pop_front() {
        if tested >= u64::from(NEAR_PIECES) {
            break;
        }
        tested += 1;
        let q = piece_hull(ctx, &c);
        if let Some(q) = &q {
            // No point of the piece has all of `h` within `β`.
            if centre_sq(h, q) > b2[1] {
                continue;
            }
            if farthest_sq(h, q) <= b2[0] {
                return Ok((true, tested));
            }
        }
        // A piece within a sixteenth of the band that doesn't certify:
        // its halves wouldn't by more.
        let fine = q.is_some_and(|q| within_band(&q, beta * 0.0625));
        if depth < NEAR_DEPTH && !fine {
            let [l, r] = c.split_half();
            queue.push_back((l, depth + 1));
            queue.push_back((r, depth + 1));
        }
    }
    Ok((false, tested))
}

/// An upper bound of the exact value `x` was rounded from.
fn above(x: f64) -> f64 {
    -down(-x)
}

/// The square of the greatest distance between a point of `a` and one of
/// `b`, rounded upwards (`+∞` where a side isn't finite).
fn farthest_sq(a: &HullBox, b: &HullBox) -> f64 {
    let mut s = 0.0;
    for k in 0..3 {
        let (x, y) = (above(a.hi[k] - b.lo[k]), above(b.hi[k] - a.lo[k]));
        let d = if x > y { x } else { y };
        if !d.is_finite() {
            return f64::INFINITY;
        }
        s = add_up(s, up(d * d));
    }
    s
}

/// The square of the least, over the points `y` of `b`, of the greatest
/// distance from `y` to a point of `a`, rounded downwards (0 where a side
/// isn't finite). Per axis the least is at the point of `b` nearest
/// `a`'s middle: `max(half a's width, b.lo − a.lo, a.hi − b.hi)`.
fn centre_sq(a: &HullBox, b: &HullBox) -> f64 {
    let mut s = 0.0;
    for k in 0..3 {
        let half = down(a.hi[k] - a.lo[k]) * 0.5;
        let (x, y) = (down(b.lo[k] - a.lo[k]), down(a.hi[k] - b.hi[k]));
        let mut g = if x > y { x } else { y };
        if half > g {
            g = half;
        }
        if g > 0.0 && g.is_finite() {
            let sq = down(g * g);
            if sq > 0.0 {
                s = down(s + sq);
            }
        }
    }
    s
}

/// Whether the hull's diameter is at most `beta`, certified: the
/// strip and corner boxes' stopping size.
pub fn within_band(h: &HullBox, beta: f64) -> bool {
    let mut s = 0.0;
    for k in 0..3 {
        let d = above(h.hi[k] - h.lo[k]);
        if !d.is_finite() {
            return false;
        }
        s = add_up(s, up(d * d));
    }
    s <= down(beta * beta)
}

/// The 3D hull of box `b` from its tile's patch ([`sub_patch`]); `None`
/// unless the part's weights are certified positive.
pub fn box_hull<S: Scalar>(patch: &HomPatch<S>, b: &BoxKey) -> Option<HullBox> {
    sub_patch(patch, b).hull_box()
}

/// The 3D hull of a piece of a p-curve: the surface's over the piece's
/// UV control box, tile by tile; `None` where it isn't certified or the
/// box spreads over more than [`HULL_TILES`] tiles.
fn piece_hull<S: Scalar>(ctx: &Space<'_, '_, '_, S>, c: &HomCurve<S, 2>) -> Option<HullBox> {
    let rect = c.control_box()?;
    let tiles = ctx.face.tiles();
    // The tiles the box meets, as index ranges clipped to the face's.
    let span = |k: usize| -> Option<(i64, i64)> {
        let [lo, hi] = rect[k];
        if !(lo.is_finite() && hi.is_finite()) {
            return None;
        }
        let r = &tiles[k];
        let (fl, fh) = (r.start as f64, r.end as f64);
        let lo = if lo < fl { fl } else { lo };
        let hi = if hi > fh { fh } else { hi };
        if lo > hi {
            return None;
        }
        // Within the face's range, so the conversions are exact.
        let first = (lo.floor() as i64).clamp(r.start, r.end - 1);
        let last = (hi.ceil() as i64 - 1).clamp(first, r.end - 1);
        Some((first, last))
    };
    let ((u0, u1), (v0, v1)) = (span(0)?, span(1)?);
    let count = (u1 - u0 + 1).checked_mul(v1 - v0 + 1)?;
    if count > HULL_TILES as i64 {
        return None;
    }
    let mut out: Option<HullBox> = None;
    for i in u0..=u1 {
        for j in v0..=v1 {
            let patch = ctx.face.patch_of(ctx.patches, [i, j])?;
            let local = |k: usize, at: i64| -> Option<[f64; 2]> {
                // The local ends, bounded outwards, clipped to [0, 1].
                let a = down(rect[k][0] - at as f64);
                let b = above(rect[k][1] - at as f64);
                let a = if a < 0.0 { 0.0 } else { a };
                let b = if b > 1.0 { 1.0 } else { b };
                (a <= b).then_some([a, b])
            };
            let (Some([a, b]), Some([c0, c1])) = (local(0, i), local(1, j)) else {
                continue;
            };
            let part = cut(&cut(patch, Dir::U, a, b), Dir::V, c0, c1);
            let h = part.hull_box()?;
            out = Some(match out {
                None => h,
                Some(o) => union(&o, &h),
            });
        }
    }
    out
}

/// The 3D hull of the piece `c` of a p-curve of `face` (module docs,
/// "Space"), from `patches` at `like`'s level (one per tile, in
/// [`FaceUv::tile_boxes`]' order); `None` where it isn't certified or the
/// piece spreads over more than [`HULL_TILES`] tiles.
pub(crate) fn surface_hull<S: Scalar>(
    face: &FaceUv<'_>,
    patches: &[HomPatch<S>],
    like: &S,
    c: &HomCurve<S, 2>,
) -> Option<HullBox> {
    piece_hull(
        &Space {
            face,
            patches,
            like,
        },
        c,
    )
}

/// The least box holding both.
fn union(a: &HullBox, b: &HullBox) -> HullBox {
    let mut o = *a;
    for k in 0..3 {
        if b.lo[k] < o.lo[k] {
            o.lo[k] = b.lo[k];
        }
        if b.hi[k] > o.hi[k] {
            o.hi[k] = b.hi[k];
        }
    }
    o
}

/// Whether a box is in a tube, with the work it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tube {
    /// Certified in the tube: every point of the box on the surface is
    /// within the coedge's band of its trimming curve on the surface.
    pub inside: bool,
    /// The span pieces tested.
    pub pieces: u64,
}

/// Whether box `b` lies in the tube of `coedge`'s trimming curve (module
/// docs) at `patches`' level (one patch per tile of the face, in
/// [`FaceUv::tile_boxes`]' order). `inside: false` is no certificate of
/// anything: the box may be too large, or too near the tube's edge, for
/// the level.
pub fn in_tube<S: Scalar>(
    face: &FaceUv<'_>,
    patches: &[HomPatch<S>],
    b: &BoxKey,
    coedge: CoedgeId,
) -> Result<Tube, BoxError> {
    if !face.owns(b) {
        return Err(BoxError::Foreign);
    }
    if !face.patches_fit(patches) {
        return Err(BoxError::Patches);
    }
    let t = face
        .trim_index(coedge)
        .map(|k| &face.trims()[k])
        .ok_or(BoxError::Coedge { coedge })?;
    let patch = face.patch_of(patches, b.tile()).ok_or(BoxError::Patches)?;
    let Some(h) = box_hull(patch, b) else {
        return Ok(Tube {
            inside: false,
            pieces: 0,
        });
    };
    let like = patch.weights().coefs()[0].clone();
    let ctx = Space {
        face,
        patches,
        like: &like,
    };
    let (inside, pieces) = near_curve(&ctx, &h, t, t.band)?;
    Ok(Tube { inside, pieces })
}

/// The role of box `b` of `face` at `like`'s level (module docs):
/// [`classify`] (or [`classify_among`](super::classify_among) with the
/// spans `among` that met a box holding `b`), then a mixed box's strip,
/// corner or thin role, against `corners` (built for this face).
pub fn role<S: Scalar>(
    face: &FaceUv<'_>,
    corners: &Corners,
    b: &BoxKey,
    like: &S,
    among: Option<&[SpanRef]>,
) -> Result<Roled, BoxError> {
    if corners.runs.len() != face.trims().len() {
        return Err(BoxError::Foreign);
    }
    let Classified {
        class,
        mut pieces,
        met,
    } = classify_in(face, b, like, among)?;
    let coedges = match class {
        Class::Outside => {
            return Ok(Roled {
                role: Role::Outside,
                pieces,
                met,
            });
        }
        Class::Core => {
            return Ok(Roled {
                role: Role::Core,
                pieces,
                met,
            });
        }
        Class::Mixed(c) => c,
    };
    let trims = face.trims();
    let idx: SmallVec<[usize; 4]> = coedges
        .iter()
        .map(|&c| face.trim_index(c).expect("a met coedge is the face's"))
        .collect();
    let zero = idx.iter().any(|&k| trims[k].zero_length);
    if !zero && idx.iter().all(|&k| face.same_curve(idx[0], k)) {
        return Ok(Roled {
            role: Role::Strip(coedges),
            pieces,
            met,
        });
    }
    // The corners every met coedge has an end at.
    let mut cands: SmallVec<[u32; 2]> = SmallVec::from_slice(&corners.ends[idx[0]]);
    cands.dedup();
    cands.retain(|c| idx.iter().all(|&k| corners.ends[k].contains(c)));
    let sides = b.sides(like);
    'corner: for c in cands {
        for r in &met {
            let k = face
                .trim_index(r.coedge)
                .expect("a met coedge is the face's");
            let t = &trims[k];
            if t.zero_length {
                continue;
            }
            let curve =
                uv_curve(&t.spans[r.span as usize], like).ok_or(span_error(t, r.span as usize))?;
            let runs = (0..2)
                .filter(|&e| corners.ends[k][e] == c)
                .map(|e| corners.runs[k][e]);
            let held = |p: (u64, u32)| runs.clone().any(|run| run.holds(r.span, p) == Some(true));
            let mut budget = SEP_PIECES;
            let within = met_within(&curve, &sides, (0, 0), &mut budget, &held);
            pieces = pieces.saturating_add(u64::from(SEP_PIECES - budget));
            if !within {
                continue 'corner;
            }
        }
        return Ok(Roled {
            role: Role::Corner {
                vertex: corners.corners[c as usize].vertex,
                met: coedges,
            },
            pieces,
            met,
        });
    }
    Ok(Roled {
        role: Role::Thin(coedges),
        pieces,
        met,
    })
}

#[cfg(test)]
mod tests;

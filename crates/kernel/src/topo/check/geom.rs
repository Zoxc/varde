//! The check's invariants 5 to 7, on the body's geometry: its seams (5),
//! its regularity and continuity (6) and its forms and fits (7).
//!
//! **5. Seams** ([`Seams`]), its kinds of item in this order:
//! - a coedge's seam bound is finite, at most its edge's tolerance, and a
//!   valid certificate: refined again towards the stored bound itself
//!   ([`seam_bound`]), it comes out at most that. The refinement is a
//!   function of the stored geometry and the target, and every cell the
//!   bound was made of is done again at the same place (its bound is at
//!   most the stored one), so a stored bound refines to itself or below;
//!   one it can't reach is no certificate of this geometry;
//! - an edge's tolerance is at most the body's `max_tol`; a **new** edge's
//!   (every edge in a full check, the changed edges in a scoped one) at
//!   most its origin's target at the body's tolerance `ε`: `ε/4` exact
//!   (constructed exactly, the rounding certified) or a simple branch
//!   (`ε/4` times the branch's arc factor, at most 1, which the edge
//!   doesn't keep), `ε/2` a near cluster's (`ε_dec/2 + ε/4`), `3ε/8` a
//!   contact cluster's (`ε_pen/2 + ε/4`), or a **warned** edge: one whose
//!   tolerance is the bound of a listed warning of the body's status, to
//!   the bit, or within the greatest bound once warnings went unlisted. A
//!   moved join's target reads the clusters' admission bounds, which the
//!   edge doesn't keep: not built ([`Finding::Unbuilt`]);
//! - a vertex's tolerance is finite and at least each incident edge's,
//!   and the surface of each face at its UV there (the p-curve's end) lies
//!   within it of its position. A corner certifies that from its coedge's
//!   seam bound, at most the vertex's tolerance, where the p-curve ends at
//!   the edge's end parameter and the curve's point there is the vertex's
//!   position bit for bit; elsewhere the surface is evaluated there,
//!   certified, at each precision level in turn until the distance is
//!   certified within the tolerance, or beyond it.
//!
//! **6. Regularity and continuity** ([`Regularity`]), in this order:
//! - every tile of a face's surface passes the fold check: a certified
//!   normal cone of the numerator the check reads ([`Tile::normal_cone`]),
//!   its singular sides divided out;
//! - a coedge that isn't singular meets a side its tile declares singular
//!   (or its neighbour across the line does) only at an end of its
//!   p-curve, its other control values off that line, and the loop goes on
//!   there through a singular coedge;
//! - an edge of the same form joins faces on surfaces of bit-identical
//!   form keys, its p-curves iso or linear spans on both; an edge tagged
//!   `Seam` is of the same form;
//! - a new edge's continuity is a valid certificate: a zero-length edge (its
//!   curve one point) is a mixed crease; a mixed crease claims nothing;
//!   curvature continuity holds on an edge of the same form (a join its
//!   construction declares so isn't recorded: not built); a tangent edge
//!   states an angle within the tangent threshold, 1°, that its
//!   certificate reaches again on every cell, no point showing a larger
//!   one; a convex or concave crease holds its sign on every cell, no
//!   point showing the other. Both certificates are the continuity's
//!   ([`Join`]: the cells, their levels and halving, charged as seam
//!   cells; past the top level, the deepest subdivision or the cells an
//!   edge may compute the certificate is still open, [`Cap`]); an edge
//!   with one face has no crease, and isn't tangent.
//!
//! The surfaces' tile lines are `G¹` by construction: every surface is its
//! form's lattice ([`Surface`]), the exact form's tiles each rounded once,
//! so its tiles join as the form does, within its resolution.
//!
//! **7. Forms and fits** ([`Forms`]). A surface lies on its form, and is
//! injective on its closed domain, by construction: its tiles are its
//! form's lattice's over a rectangle the lattice admits (the torus domain
//! rule among its rules). A fitted surface's deviation would be
//! certified again here or its construction certificate read: fits hold
//! no intent yet, so a face on a fitted surface is not built.
//!
//! **Work.** Every item is charged one unit, one more per coedge, edge,
//! span, tile or corner read; a seam bound as its refinement charges, a
//! tile's fold check as [`Tile::normal_cone_charged`] charges, a crease
//! or tangent cell as a seam cell at its level, a vertex's evaluation one unit per
//! level (a precision step above the first).
//!
//! **Reading a broken body.** Where an id a hook follows names nothing, a
//! p-curve doesn't run over its edge's range or a span lies in no tile,
//! the item passes that part, the earlier invariants' to refuse; a seam
//! bound that can't be refined there is no certificate.

use varde_decide::bezier::{Dir, Side as TileSide};
use varde_decide::num::bounded::{add_up, mul_up, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::geom::certify::{seam_bound, tile_of};
use crate::geom::pcurve::PSpan;
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;
use crate::infra::budget::{Cap, LEVELS, Ledger, Step, Stop, level_step};
use crate::topo::brep::{
    Brep, ClusterKind, CoedgeId, Continuity, Convexity, EdgeId, EdgeKind, EdgeTag, FaceId, Origin,
    PlainOrigin, VertId,
};
use crate::topo::continuity::{
    CreaseCheck, Join, THETA_TAN, Tangency, coedge_face, edge_curve, face_surface, knot_index,
    pcurve, same_bits, zero_length,
};

#[cfg(test)]
mod tests;

/// The checks of invariant 5, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum SeamCheck {
    /// A seam bound negative, not finite or above its edge's tolerance.
    SeamTol = 1,
    /// A seam bound that isn't a certificate of its coedge.
    SeamBound,
    /// An edge's tolerance negative or above the body's `max_tol`.
    MaxTol,
    /// A new edge's tolerance above its origin's target, unwarned.
    Target,
    /// A vertex's tolerance not finite, or below an incident edge's.
    VertTol,
    /// A face's surface at the vertex's UV beyond its tolerance.
    Ball,
}

/// The checks of invariant 6, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum RegularityCheck {
    /// A tile without a certified normal cone.
    Fold = 1,
    /// A trim meeting a singular side other than through a singular
    /// coedge.
    SingularSide,
    /// An edge of the same form between surfaces of different form keys,
    /// or with a p-curve neither iso nor linear.
    SameForm,
    /// An edge tagged `Seam` not of the same form.
    SeamTag,
    /// A zero-length edge other than a mixed crease.
    ZeroLength,
    /// A tangent edge: its angle beyond the threshold, else not built.
    Tangent,
    /// Curvature continuity on an edge not of the same form: not built.
    Curvature,
    /// A convex or concave crease not of that sign.
    Crease,
}

/// The checks of invariant 7, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum FormCheck {
    /// A face on a fitted surface: not built.
    Fit = 1,
}

type Checked = Result<(), Stop<Fault>>;

fn fault(check: u32, finding: Finding) -> Checked {
    Err(Stop::Failed(Fault { check, finding }))
}

fn fail(check: u32) -> Checked {
    fault(check, Finding::Defect)
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

/// A coedge's edge, `None` where its id names nothing.
fn coedge_edge(b: &Brep, c: CoedgeId) -> Option<EdgeId> {
    at(&b.coedges.edge, c.0).filter(|e| e.index() < b.edge_count())
}

/// Whether coedge `c` is singular: unpaired on a singular edge.
fn is_singular(b: &Brep, c: CoedgeId) -> bool {
    at(&b.coedges.pair, c.0).is_some_and(CoedgeId::is_none)
        && coedge_edge(b, c).is_some_and(|e| b.edges.kind[e.index()] == EdgeKind::Singular)
}

/// Invariant 5, the seams (module docs).
pub(crate) struct Seams;

impl Invariant for Seams {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Coedge, ItemKind::Edge, ItemKind::Vert]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Coedge(c) => seam(cx.body(), c, ledger),
            Item::Edge(e) => edge_tol(cx, e),
            Item::Vert(v) => vert_tol(cx, v, ledger),
            _ => Ok(()),
        }
    }
}

fn seam(b: &Brep, c: CoedgeId, ledger: &mut Ledger<'_>) -> Checked {
    use SeamCheck as S;
    let Some(e) = coedge_edge(b, c) else {
        return Ok(());
    };
    let bound = b.coedges.seam_bound[c.index()];
    let tol = b.edges.tol[e.index()];
    need(
        bound.is_finite() && bound >= 0.0 && bound <= tol,
        S::SeamTol as u32,
    )?;
    let face = coedge_face(b, c).and_then(|f| face_surface(b, f));
    let (Some(p), Some(s), Some(curve)) = (pcurve(b, c), face, edge_curve(b, e)) else {
        return Ok(());
    };
    match seam_bound(s, curve, p, bound, ledger) {
        Ok(again) => need(again.bound <= bound, S::SeamBound as u32),
        Err(Stop::Failed(_)) => fail(S::SeamBound as u32),
        Err(Stop::Exhausted(x)) => Err(Stop::Exhausted(x)),
        Err(Stop::Cap(cap)) => Err(Stop::Cap(cap)),
    }
}

/// The tolerance target of a new edge of origin `origin` at tolerance
/// `eps` (module docs); `None` for a moved join, not built.
fn target(origin: &Origin, eps: f64) -> Option<f64> {
    // Each a power of two times `ε`, or `3/8` of it: exact.
    match origin {
        Origin::Plain(PlainOrigin::Exact | PlainOrigin::Simple) => Some(eps * 0.25),
        Origin::Plain(PlainOrigin::Cluster(c)) => Some(match c.kind {
            ClusterKind::Near => eps * 0.5,
            ClusterKind::Contact => eps * 0.375,
        }),
        Origin::MovedJoin { .. } => None,
    }
}

/// Whether `tol` is a warned edge's: a listed warning's bound, bit for
/// bit, or within the greatest bound once some went unlisted.
fn warned(b: &Brep, tol: f64) -> bool {
    let op = b.status().op();
    let listed = op.warnings();
    listed.iter().any(|w| w.bound().to_bits() == tol.to_bits())
        || (op.warning_count() > listed.len() as u64 && tol <= op.max_deviation())
}

fn edge_tol(cx: &Cx<'_>, e: EdgeId) -> Checked {
    use SeamCheck as S;
    let b = cx.body();
    let tol = b.edges.tol[e.index()];
    need(tol >= 0.0 && tol <= b.eps().max_tol(), S::MaxTol as u32)?;
    if !cx.is_new(e) {
        return Ok(());
    }
    let Some(target) = target(&b.edges.origin[e.index()], b.eps().eps().eps()) else {
        return fault(S::Target as u32, Finding::Unbuilt);
    };
    need(tol <= target || warned(b, tol), S::Target as u32)
}

fn vert_tol(cx: &Cx<'_>, v: VertId, ledger: &mut Ledger<'_>) -> Checked {
    use SeamCheck as S;
    let b = cx.body();
    let tol = b.verts.tol[v.index()];
    let edges = &cx.inc.vert_edges[v.index()];
    charge(ledger, edges.len())?;
    let covers = edges.iter().all(|e| b.edges.tol[e.index()] <= tol);
    need(tol.is_finite() && covers, S::VertTol as u32)?;
    let corners = &cx.inc.vert_corners[v.index()];
    charge(ledger, corners.len())?;
    // Every corner is tried, so the work charged and the outcome don't
    // depend on the corners' ids: a defect at any, else a corner left
    // open at the top level.
    let (mut defect, mut open) = (false, false);
    for &c in corners {
        match ball(b, c, v, ledger) {
            Ok(()) => {}
            Err(Stop::Failed(_)) => defect = true,
            Err(Stop::Cap(_)) => open = true,
            Err(Stop::Exhausted(x)) => return Err(Stop::Exhausted(x)),
        }
    }
    if defect {
        fail(S::Ball as u32)
    } else if open {
        Err(Stop::Cap(Cap::PrecisionBits))
    } else {
        Ok(())
    }
}

/// The surface of corner `c`'s face at its UV at vertex `v` within `v`'s
/// tolerance of its position (module docs).
fn ball(b: &Brep, c: CoedgeId, v: VertId, ledger: &mut Ledger<'_>) -> Checked {
    let (pos, tol) = (b.verts.pos[v.index()], b.verts.tol[v.index()]);
    let (Some(e), Some(p), Some(s)) = (
        coedge_edge(b, c),
        pcurve(b, c),
        coedge_face(b, c).and_then(|f| face_surface(b, f)),
    ) else {
        return Ok(());
    };
    let reversed = b.coedges.reversed[c.index()];
    let pk = p.knots();
    let (last, t) = if reversed {
        (0, b.edges.t_range[e.index()][0])
    } else {
        (pk.len() - 1, b.edges.t_range[e.index()][1])
    };
    let Some(uv) = p.knot_point(if reversed { 0 } else { p.spans().len() }) else {
        return Ok(());
    };
    // By the seam bound: the p-curve's end is the curve's, at the vertex.
    let at_vertex = edge_curve(b, e)
        .and_then(|curve| curve.knot_point(knot_index(curve.knots(), t)?))
        .is_some_and(|x| same_bits(&x, &pos));
    if at_vertex && pk[last].to_bits() == t.to_bits() && b.coedges.seam_bound[c.index()] <= tol {
        return Ok(());
    }
    // Else by the surface's point there.
    let Some(located) = s.locate(uv) else {
        return fail(SeamCheck::Ball as u32);
    };
    let tile = &s.tiles()[located.index];
    for level in 0..LEVELS {
        ledger.charge(level_step(level), 1)?;
        let mut point = PointAt {
            tile,
            uv,
            ij: [located.i, located.j],
            pos,
        };
        let [lo, hi] = at_level(
            &mut point,
            Level::from_index(usize::from(level)).unwrap_or(Level::TOP),
            &mut (),
        );
        if hi <= tol {
            return Ok(());
        }
        if lo > tol {
            return fail(SeamCheck::Ball as u32);
        }
    }
    Err(Stop::Cap(Cap::PrecisionBits))
}

/// A tile's point at a global UV against a position: a certified lower
/// and upper bound of their distance, at one level.
struct PointAt<'a> {
    tile: &'a Tile,
    uv: [f64; 2],
    ij: [i64; 2],
    pos: [f64; 3],
}

impl AtLevel<()> for PointAt<'_> {
    type Out = [f64; 2];

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> [f64; 2] {
        // Tile indices below 2⁵² in magnitude (`Surface::locate`): exact.
        let [u, v] = [0, 1]
            .map(|k| S::from_f64(self.uv[k], like).sub(&S::from_f64(self.ij[k] as f64, like)));
        let x = self.tile.patch_at(like).eval(&u, &v);
        let (mut lo, mut sq) = (0.0, 0.0);
        for k in 0..3 {
            let [a, z] = x[k].sub(&S::from_f64(self.pos[k], like)).enclose();
            let far = if -a > z { -a } else { z };
            let near = if a > 0.0 {
                a
            } else if z < 0.0 {
                -z
            } else {
                0.0
            };
            if near > lo {
                lo = near;
            }
            // A NaN makes the bound infinite.
            sq = if far.is_nan() {
                f64::INFINITY
            } else {
                add_up(sq, mul_up(far, far))
            };
        }
        [lo, up(sq.sqrt())]
    }

    /// The caller climbs the levels itself.
    fn escalates(_: &[f64; 2]) -> bool {
        false
    }
}

/// Invariant 6, regularity and continuity (module docs).
pub(crate) struct Regularity;

impl Invariant for Regularity {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Face, ItemKind::Coedge, ItemKind::Edge]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        match item {
            Item::Face(f) => fold(cx.body(), f, ledger),
            Item::Coedge(c) => singular_sides(cx.body(), c, ledger),
            Item::Edge(e) => {
                same_form(cx, e, ledger)?;
                if cx.is_new(e) {
                    continuity(cx, e, ledger)
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

fn fold(b: &Brep, f: FaceId, ledger: &mut Ledger<'_>) -> Checked {
    let Some(s) = face_surface(b, f) else {
        return Ok(());
    };
    charge(ledger, s.tiles().len())?;
    for t in s.tiles() {
        let cone = t.normal_cone_charged(ledger)?;
        need(cone.is_some(), RegularityCheck::Fold as u32)?;
    }
    Ok(())
}

/// Whether the line of `side` of tile `(i, j)` of `s` is singular: that
/// side declared so, or the opposite side of the tile across the line.
fn singular_line(s: &Surface, i: i64, j: i64, side: TileSide) -> bool {
    let declared =
        |i: i64, j: i64, side: TileSide| s.tile(i, j).is_some_and(|t| t.singular().contains(side));
    declared(i, j, side)
        || match side {
            TileSide::U0 => declared(i - 1, j, TileSide::U1),
            TileSide::U1 => declared(i + 1, j, TileSide::U0),
            TileSide::V0 => declared(i, j - 1, TileSide::V1),
            TileSide::V1 => declared(i, j + 1, TileSide::V0),
        }
}

fn singular_sides(b: &Brep, c: CoedgeId, ledger: &mut Ledger<'_>) -> Checked {
    let check = RegularityCheck::SingularSide as u32;
    if is_singular(b, c) {
        return Ok(());
    }
    let s = coedge_face(b, c).and_then(|f| face_surface(b, f));
    let (Some(p), Some(s)) = (pcurve(b, c), s) else {
        return Ok(());
    };
    let spans = p.spans();
    charge(ledger, spans.len())?;
    let reversed = b.coedges.reversed[c.index()];
    let n = spans.len();
    for (k, span) in spans.iter().enumerate() {
        let Some((_, i, j)) = tile_of(s, span) else {
            continue;
        };
        for side in TileSide::ALL {
            if !singular_line(s, i, j, side) {
                continue;
            }
            let (dir, line) = match side {
                TileSide::U0 => (Dir::U, i),
                TileSide::U1 => (Dir::U, i + 1),
                TileSide::V0 => (Dir::V, j),
                TileSide::V1 => (Dir::V, j + 1),
            };
            // Tile indices below 2⁵² in magnitude (`tile_of`): exact.
            let line = line as f64;
            let values = span.coord(dir);
            let on: Vec<bool> = values.values().iter().map(|x| x[0] == line).collect();
            if !on.contains(&true) {
                continue;
            }
            // The span leaves the line: it meets it only at a control
            // value there, an end of the p-curve.
            need(on.contains(&false), check)?;
            let top = on.len() - 1;
            for (at, _) in on.iter().enumerate().filter(|(_, o)| **o) {
                let first = at == 0 && k == 0;
                let last = at == top && k + 1 == n;
                need(first || last, check)?;
                // Where the coedge reaches the line, in its own direction.
                let ends = last != reversed;
                let then = if ends {
                    b.coedges.next[c.index()]
                } else {
                    b.coedges.prev[c.index()]
                };
                need(is_singular(b, then), check)?;
            }
        }
    }
    Ok(())
}

/// The coedges of edge `e` and their faces, where it has two.
fn two_faces(cx: &Cx<'_>, e: EdgeId) -> Option<([CoedgeId; 2], [FaceId; 2])> {
    let b = cx.body();
    match cx.inc.edge_coedges[e.index()][..] {
        [c1, c2] => Some(([c1, c2], [coedge_face(b, c1)?, coedge_face(b, c2)?])),
        _ => None,
    }
}

fn same_form(cx: &Cx<'_>, e: EdgeId, ledger: &mut Ledger<'_>) -> Checked {
    use RegularityCheck as R;
    let b = cx.body();
    let kind = b.edges.kind[e.index()];
    if matches!(kind, EdgeKind::SameForm { .. }) {
        let Some(([c1, c2], [f1, f2])) = two_faces(cx, e) else {
            return fail(R::SameForm as u32);
        };
        let keys = (face_surface(b, f1), face_surface(b, f2));
        let (Some(s1), Some(s2)) = keys else {
            return Ok(());
        };
        need(s1.lattice().key() == s2.lattice().key(), R::SameForm as u32)?;
        // Both read before deciding, so the work doesn't depend on ids.
        let mut straight = true;
        for p in [c1, c2].into_iter().filter_map(|c| pcurve(b, c)) {
            charge(ledger, p.spans().len())?;
            straight &= p
                .spans()
                .iter()
                .all(|s| matches!(s, PSpan::Iso { .. } | PSpan::Linear(_)));
        }
        need(straight, R::SameForm as u32)?;
    }
    let seam = b.edges.tag[e.index()] == Some(EdgeTag::Seam);
    need(
        !seam || matches!(kind, EdgeKind::SameForm { .. }),
        R::SeamTag as u32,
    )
}

fn continuity(cx: &Cx<'_>, e: EdgeId, ledger: &mut Ledger<'_>) -> Checked {
    use RegularityCheck as R;
    let b = cx.body();
    let kind = b.edges.kind[e.index()];
    let cont = b.edges.cont[e.index()];
    if kind == EdgeKind::Singular {
        return Ok(());
    }
    charge(ledger, 1)?;
    if zero_length(b, e) {
        return need(
            cont == Continuity::Crease(Convexity::Mixed),
            R::ZeroLength as u32,
        );
    }
    match cont {
        Continuity::Crease(Convexity::Mixed) => Ok(()),
        Continuity::Tangent { angle } => {
            need((0.0..=THETA_TAN).contains(&angle), R::Tangent as u32)?;
            let Some(join) = join(cx, e, ledger)? else {
                // Tangent to nothing: an edge with one face.
                return fail(R::Tangent as u32);
            };
            match join.tangent(angle, angle, ledger)? {
                Tangency::Within(_) => Ok(()),
                Tangency::Beyond => fail(R::Tangent as u32),
                Tangency::Degree => fault(R::Tangent as u32, Finding::Unbuilt),
                Tangency::Cap(cap) => Err(Stop::Cap(cap)),
            }
        }
        Continuity::Curvature if matches!(kind, EdgeKind::SameForm { .. }) => Ok(()),
        Continuity::Curvature => fault(R::Curvature as u32, Finding::Unbuilt),
        Continuity::Crease(sign) => {
            // An open boundary has no crease to certify.
            let Some(join) = join(cx, e, ledger)? else {
                return Ok(());
            };
            match join.check_crease(sign, ledger)? {
                CreaseCheck::Holds => Ok(()),
                CreaseCheck::Wrong => fail(R::Crease as u32),
                CreaseCheck::Degree => fault(R::Crease as u32, Finding::Unbuilt),
                CreaseCheck::Cap(cap) => Err(Stop::Cap(cap)),
            }
        }
    }
}

/// The join of edge `e` between its two faces, its coedge of the lower id
/// first; `None` with fewer or more faces, or where the body can't be
/// read so.
fn join<'c>(
    cx: &'c Cx<'_>,
    e: EdgeId,
    ledger: &mut Ledger<'_>,
) -> Result<Option<Join<'c>>, Stop<Fault>> {
    match two_faces(cx, e) {
        Some((coedges, faces)) => Join::new(cx.body(), e, coedges, faces, ledger),
        None => Ok(None),
    }
}

/// Invariant 7, forms and fits (module docs).
pub(crate) struct Forms;

impl Invariant for Forms {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Face]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        charge(ledger, 1)?;
        let Item::Face(f) = item else {
            return Ok(());
        };
        let b = cx.body();
        let fitted = at(&b.faces.surface, f.0)
            .and_then(|s| b.surfaces.fit.get(s.index()))
            .is_some_and(Option::is_some);
        if fitted {
            fault(FormCheck::Fit as u32, Finding::Unbuilt)
        } else {
            Ok(())
        }
    }
}

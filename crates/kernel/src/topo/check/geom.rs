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
//!   states an angle within the tangent threshold, 1° (its certificate is
//!   not built); a convex or concave crease is certified below.
//!
//! The surfaces' tile lines are `G¹` by construction: every surface is its
//! form's lattice ([`Surface`]), the exact form's tiles each rounded once,
//! so its tiles join as the form does, within its resolution.
//!
//! **The crease certificate.** Along the edge, with `c` one of its coedges
//! on face `f₁`, the other on `f₂`, the sign of `(n₁ × n₂)·T` is the
//! crease's: positive convex, negative concave, `nₖ` the faces' outward
//! normals and `T` the curve's tangent in `c`'s direction. As in the seam
//! bound, the edge is cut at the knots of both p-curves and of the curve
//! inside its range, each piece in one span of each and one tile of each
//! surface. On a cell of a piece the fold numerator `N` of each tile (a
//! positive multiple of its orientation's normal,
//! [`Tile::fold_numerator_at`]) is composed with the p-curve's span in the
//! tile's local coordinates ([`compose_params`]: a positive multiple
//! again), and the curve's tangent numerator `T' = W·X' − W'·X` taken;
//! `det[N₁, N₂, T']` is then a Bernstein form whose sign, with the faces'
//! `flipped` and the coedge's direction, is the crease's (degree `2·(5·k₁ + 5·k₂) + 2n − 1`, 43 for
//! quadratic spans). The cell holds when every coefficient has the claimed
//! sign, certified; the label is a defect when a coefficient at an end of
//! the cell (the value there) is certified of the other sign. Otherwise the
//! cell is computed again one level up where every coefficient's value has
//! the claimed sign (rounding decides), else halved, as the seam bound's
//! cells, charged as its cells are; past the top level or the deepest
//! subdivision the certificate is still open ([`Cap`]).
//!
//! [`compose_params`]: varde_decide::bezier::Tensor::compose_params
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
//! cell as a seam cell at its level, a vertex's evaluation one unit per
//! level (a precision step above the first).
//!
//! **Reading a broken body.** Where an id a hook follows names nothing, a
//! p-curve doesn't run over its edge's range or a span lies in no tile,
//! the item passes that part, the earlier invariants' to refuse; a seam
//! bound that can't be refined there is no certificate.

use varde_decide::bezier::{Dir, HomCurve, Poly, Side as TileSide, cross, dot};
use varde_decide::num::bounded::{add_up, mul_up, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::Level;

use super::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::geom::certify::{restrict, seam_bound, tile_of};
use crate::geom::curve::{Curve3D, local};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;
use crate::infra::budget::{
    Cap, LEVELS, Ledger, MAX_SUBDIV_DEPTH, Step, Stop, level_step, seam_cell_units,
};
use crate::topo::brep::{
    Brep, ClusterKind, CoedgeId, Continuity, Convexity, EdgeId, EdgeKind, EdgeTag, FaceId, Origin,
    PlainOrigin, VertId,
};

#[cfg(test)]
mod tests;

/// The tangent threshold: 1° (radians).
const THETA_TAN: f64 = std::f64::consts::PI / 180.0;

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

fn same_bits<const N: usize>(a: &[f64; N], b: &[f64; N]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// A coedge's edge, `None` where its id names nothing.
fn coedge_edge(b: &Brep, c: CoedgeId) -> Option<EdgeId> {
    at(&b.coedges.edge, c.0).filter(|e| e.index() < b.edge_count())
}

/// A coedge's face, `None` where its id names nothing.
fn coedge_face(b: &Brep, c: CoedgeId) -> Option<FaceId> {
    at(&b.coedges.face, c.0).filter(|f| f.index() < b.face_count())
}

/// A coedge's p-curve, `None` where an id names nothing.
fn pcurve(b: &Brep, c: CoedgeId) -> Option<&PCurve> {
    b.pcurves.get(at(&b.coedges.pcurve, c.0)?.index())
}

/// A face's surface, `None` where an id names nothing.
fn face_surface(b: &Brep, f: FaceId) -> Option<&Surface> {
    b.surfaces.surface.get(at(&b.faces.surface, f.0)?.index())
}

/// An edge's curve, `None` where its id names nothing.
fn edge_curve(b: &Brep, e: EdgeId) -> Option<&Curve3D> {
    b.curves3.get(at(&b.edges.curve, e.0)?.index())
}

/// Whether coedge `c` is singular: unpaired on a singular edge.
fn is_singular(b: &Brep, c: CoedgeId) -> bool {
    at(&b.coedges.pair, c.0).is_some_and(CoedgeId::is_none)
        && coedge_edge(b, c).is_some_and(|e| b.edges.kind[e.index()] == EdgeKind::Singular)
}

/// The position of `t` among `knots`, bit for bit.
fn knot_index(knots: &[f64], t: f64) -> Option<usize> {
    knots.iter().position(|k| k.to_bits() == t.to_bits())
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

/// Whether edge `e`'s curve is one point over its range: a zero-length
/// edge.
fn zero_length(b: &Brep, e: EdgeId) -> bool {
    let Some(curve) = edge_curve(b, e) else {
        return false;
    };
    let [t0, t1] = b.edges.t_range[e.index()];
    let (Some(i), Some(j)) = (knot_index(curve.knots(), t0), knot_index(curve.knots(), t1)) else {
        return false;
    };
    let Some(spans) = curve.spans().get(i..j) else {
        return false;
    };
    let mut points = spans.iter().flatten().map(|p| [p[0], p[1], p[2]]);
    let first = points.next();
    first.is_some_and(|f| points.all(|p| same_bits(&p, &f)))
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
            fault(R::Tangent as u32, Finding::Unbuilt)
        }
        Continuity::Curvature if matches!(kind, EdgeKind::SameForm { .. }) => Ok(()),
        Continuity::Curvature => fault(R::Curvature as u32, Finding::Unbuilt),
        Continuity::Crease(sign) => match two_faces(cx, e) {
            Some((coedges, faces)) => crease(b, e, coedges, faces, sign, ledger),
            // An open boundary has no crease to certify.
            None => Ok(()),
        },
    }
}

/// A piece of an edge: `[a, b]` of `t`, in span `sp[k]` of p-curve `k`
/// on tile `tiles[k]` (with its indices) and span `sc` of the curve.
struct Piece<'a> {
    tiles: [(&'a Tile, i64, i64); 2],
    sp: [usize; 2],
    sc: usize,
    a: f64,
    b: f64,
}

/// Part `k` of `2^depth` of a piece, at a precision level.
#[derive(Clone, Copy)]
struct Cell {
    piece: usize,
    k: u64,
    depth: u32,
    level: u8,
}

/// What a crease cell shows.
enum Verdict {
    /// The claimed sign, certified.
    Holds,
    /// The other sign, certified at an end of the cell.
    Wrong,
    /// A tile whose fold check stops: no normal to certify with.
    Folded,
    /// A form past the greatest degree.
    Degree,
    /// Compute it again one level up.
    Precision,
    /// Halve it.
    Space,
}

/// The crease certificate of edge `e` between `faces`, its coedge
/// `coedges[0]`'s direction its tangent's (module docs).
fn crease(
    b: &Brep,
    e: EdgeId,
    coedges: [CoedgeId; 2],
    faces: [FaceId; 2],
    sign: Convexity,
    ledger: &mut Ledger<'_>,
) -> Checked {
    let check = RegularityCheck::Crease as u32;
    let (Some(curve), Some(p1), Some(p2)) = (
        edge_curve(b, e),
        pcurve(b, coedges[0]),
        pcurve(b, coedges[1]),
    ) else {
        return Ok(());
    };
    let (Some(s1), Some(s2)) = (face_surface(b, faces[0]), face_surface(b, faces[1])) else {
        return Ok(());
    };
    let p = [p1, p2];
    let [t0, t1] = b.edges.t_range[e.index()];
    let ck = curve.knots();
    let over = |pk: &[f64]| {
        pk.first().map(|t| t.to_bits()) == Some(t0.to_bits())
            && pk.last().map(|t| t.to_bits()) == Some(t1.to_bits())
    };
    if !(over(p1.knots()) && over(p2.knots())) || knot_index(ck, t0).is_none() {
        return Ok(());
    }
    let mut cuts: Vec<f64> = p1
        .knots()
        .iter()
        .chain(p2.knots())
        .chain(ck.iter().filter(|&&t| t0 < t && t < t1))
        .copied()
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    charge(ledger, cuts.len())?;
    let mut pieces = Vec::with_capacity(cuts.len());
    for w in cuts.windows(2) {
        let (a, z) = (w[0], w[1]);
        let span = |knots: &[f64]| knots.partition_point(|&k| k <= a) - 1;
        let sp = [span(p1.knots()), span(p2.knots())];
        let (Some(a1), Some(a2)) = (
            tile_of(s1, &p1.spans()[sp[0]]),
            tile_of(s2, &p2.spans()[sp[1]]),
        ) else {
            return Ok(());
        };
        pieces.push(Piece {
            tiles: [a1, a2],
            sp,
            sc: span(ck),
            a,
            b: z,
        });
    }
    // The sign `det[N₁, N₂, T']` has where the crease is convex.
    let flipped = |f: FaceId| b.faces.flipped[f.index()];
    let positive = (sign == Convexity::Convex)
        ^ flipped(faces[0])
        ^ flipped(faces[1])
        ^ b.coedges.reversed[coedges[0].index()];
    // A stack: the next cell last.
    let mut cells: Vec<Cell> = (0..pieces.len())
        .rev()
        .map(|piece| Cell {
            piece,
            k: 0,
            depth: 0,
            level: 0,
        })
        .collect();
    while let Some(cell) = cells.pop() {
        ledger.charge(level_step(cell.level), seam_cell_units(cell.level))?;
        let level = Level::from_index(usize::from(cell.level)).unwrap_or(Level::TOP);
        let mut at = CreaseAt {
            curve,
            p,
            piece: &pieces[cell.piece],
            cell,
            positive,
        };
        match at_level(&mut at, level, &mut ()) {
            Verdict::Holds => {}
            Verdict::Wrong | Verdict::Folded => return fail(check),
            Verdict::Degree => return fault(check, Finding::Unbuilt),
            Verdict::Precision if cell.level + 1 < LEVELS => cells.push(Cell {
                level: cell.level + 1,
                ..cell
            }),
            Verdict::Precision => return Err(Stop::Cap(Cap::PrecisionBits)),
            Verdict::Space if cell.depth < MAX_SUBDIV_DEPTH => {
                // The left half on top.
                for k in [2 * cell.k + 1, 2 * cell.k] {
                    cells.push(Cell {
                        k,
                        depth: cell.depth + 1,
                        ..cell
                    });
                }
            }
            Verdict::Space => return Err(Stop::Cap(Cap::SubdivisionDepth)),
        }
    }
    Ok(())
}

/// One crease cell at one level.
struct CreaseAt<'c, 'p> {
    curve: &'c Curve3D,
    p: [&'c PCurve; 2],
    piece: &'c Piece<'p>,
    cell: Cell,
    positive: bool,
}

/// `c` halved down to `cell`: the halves `cell.k`'s bits name, from the
/// top.
fn halved<S: LevelScalar, const N: usize>(mut c: HomCurve<S, N>, cell: Cell) -> HomCurve<S, N> {
    for bit in (0..cell.depth).rev() {
        let [l, r] = c.split_half();
        c = if (cell.k >> bit) & 1 == 1 { r } else { l };
    }
    c
}

impl AtLevel<()> for CreaseAt<'_, '_> {
    type Out = Verdict;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> Verdict {
        let piece = self.piece;
        let mut normals: Vec<[Poly<S>; 3]> = Vec::with_capacity(2);
        for side in 0..2 {
            let (p, sp) = (self.p[side], piece.sp[side]);
            let pk = p.knots();
            let (a, z) = (
                local(piece.a, pk[sp], pk[sp + 1], like),
                local(piece.b, pk[sp], pk[sp + 1], like),
            );
            let coords = p
                .coords_at(sp, like)
                .expect("a span of the piece")
                .map(|x| halved(restrict(&x, a.clone(), z.clone()), self.cell));
            let (tile, i, j) = piece.tiles[side];
            // Tile indices below 2⁵² in magnitude (`tile_of`): exact.
            let [pu, pv] = [(&coords[0], i), (&coords[1], j)]
                .map(|(x, k)| x.translated(&[S::from_f64(k as f64, like)]));
            let Some(n) = tile.fold_numerator_at(like) else {
                return Verdict::Folded;
            };
            let composed: Option<Vec<Poly<S>>> = n
                .iter()
                .map(|t| {
                    t.compose_params(
                        [&pu.coords()[0], pu.weights()],
                        [&pv.coords()[0], pv.weights()],
                    )
                })
                .collect();
            let Some([x, y, z]) = composed.and_then(|v| <[Poly<S>; 3]>::try_from(v).ok()) else {
                return Verdict::Degree;
            };
            normals.push([x, y, z]);
        }
        let ck = self.curve.knots();
        let sc = piece.sc;
        let c = self.curve.span_at(sc, like).expect("a span of the piece");
        let c = halved(
            restrict(
                &c,
                local(piece.a, ck[sc], ck[sc + 1], like),
                local(piece.b, ck[sc], ck[sc + 1], like),
            ),
            self.cell,
        );
        let g = c
            .tangent_numerator()
            .and_then(|t| dot(&cross(&normals[0], &normals[1])?, &t));
        let Some(g) = g else {
            return Verdict::Degree;
        };
        classify(g.coefs(), self.positive)
    }

    /// The caller climbs the levels and halves the cells itself.
    fn escalates(_: &Verdict) -> bool {
        false
    }
}

/// The verdict on a cell whose sign form has the coefficients `coefs`,
/// claimed positive or negative (module docs).
fn classify<S: LevelScalar>(coefs: &[S], positive: bool) -> Verdict {
    // Each enclosure turned so that the claimed sign is positive.
    let turned = |c: &S| {
        let [lo, hi] = c.enclose();
        if positive { [lo, hi] } else { [-hi, -lo] }
    };
    if coefs.iter().all(|c| turned(c)[0] > 0.0) {
        return Verdict::Holds;
    }
    let ends = [coefs.first(), coefs.last()];
    if ends.into_iter().flatten().any(|c| turned(c)[1] < 0.0) {
        return Verdict::Wrong;
    }
    let value = |c: &S| if positive { c.approx() } else { -c.approx() };
    if coefs.iter().all(|c| value(c) > 0.0) {
        Verdict::Precision
    } else {
        Verdict::Space
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

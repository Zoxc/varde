//! Invariant 9's adjacent part: faces sharing an edge, or only a vertex,
//! meet there and nowhere else nearby. Inside a tube round an edge (a
//! ball round a vertex) a local certificate says so; outside it the
//! pair's leaves do, as for faces that share nothing. This module holds
//! the local certificates: the **ruled-surface certificate** of an edge
//! and the **separating plane** of a vertex; and the tube round an edge,
//! the radius its **window** gives ([`window`]). The leaves beyond the
//! tube, and the ball round a vertex, are later parts ([`Embedding`],
//! below).
//!
//! **The ruled-surface certificate** (one for every kind of edge:
//! creases, convex, concave or mixed, tangent and curvature-continuous
//! edges alike), per **piece** of the edge: the edge's parameter range
//! cut at every knot of its curve `C` and of its two coedges' p-curves,
//! so that a piece lies in one span of each, with the same `t` on all
//! three. Its faces are read as the ribbon-closed faces, each face's
//! side running from `S∘p` at `s = 0` into the face.
//! - **Field.** `m(t) = N₁(t)/λ₁ + N₂(t)/λ₂`, `Nᵢ` the outward normal
//!   numerator of face `i` (its tile's numerator, of degrees `(4, 4)`
//!   where it reduces, negated on a flipped face) along its p-curve
//!   ([`Tensor::compose_params`]; the positive factor of the p-curve's
//!   weights kept, as it doesn't change a sign), and `1/λᵢ` the `f64`
//!   nearest `1/|Nᵢ|` at the cell's middle (not a power of two: off by up
//!   to a factor √2 it would tilt `m` out of the normals at creases past
//!   about 110°): rational, of degree `4k_u +
//!   4k_v` for a span of degrees `(k_u, k_v)`, the higher of the two
//!   faces' (16 for Q spans); on a flat tile (a plane's) the numerator is
//!   its one value, which holds every value over the tile
//!   ([`Tile::flat_normal`]), so between two planes `m` is a constant;
//!   constructed once per cell: its Bernstein
//!   coefficients rounded once to `f64`, the field the strips read being
//!   that polynomial exactly. It is required between the two normals:
//!   `m·N₁ > 0` and `m·N₂ > 0`, certified by Bernstein signs. A cell where
//!   that is unsettled is halved, with new `λᵢ`; no `m` exists only where
//!   the normals are opposite (a knife edge), which stays unsettled to the
//!   depth limit.
//! - **Sign.** On a cell where `m` holds, each face is moved off its
//!   p-curve along a constant UV direction `ν` into the face (`det(p′, ν)`
//!   certified of the face's sign: left of the coedge's direction on a
//!   face whose normal is its surface's, right on a flipped one),
//!   `Xᵢ(t, s) = S(p(t) + s·h·ν)`, `s ∈ [0, 1]`, `h` the strip's width in
//!   global UV (`ν` of largest coordinate 1). The strip stays in the
//!   span's tile, decided on the cell's control values: the tile on the
//!   strip's side where the span runs on a tile line, a coordinate of `ν`
//!   made 0 where the span reaches the tile's side it points to, and `h`
//!   cut to the largest power of two that fits ([`Held::reach`] the least
//!   kept). The surface over the strip is the tile's patch composed with
//!   the strip's map ([`HomPatch::compose_map`](varde_decide::bezier::HomPatch::compose_map)), and the sign function
//!   `Gᵢ(t, s) = (Xᵢ − C(t))·(m(t) × C′(t))` its numerator
//!   `(H·W_C − X_C·H₃)·(m × T)`, `T` the curve's tangent numerator: of
//!   degree `(2k_u + 2k_v) + c + (2c − 2) + deg m` in `t` (`c` the curve
//!   span's degree; (28, 4) for Q spans on both sides and a conic curve),
//!   2 in `s` for each coordinate `ν` moves, lower for linear, G and iso
//!   spans; computed from the operands, never tabled. The face whose
//!   coedge runs the edge's way must have `G > 0`, the other `G < 0`: with
//!   `m` between the normals that is the first-order sign, so no side is
//!   chosen.
//! - **The join zone.** At `s = 0` the face is `S∘p`, off `C` by its
//!   ribbon, so the sign is required only beyond a certified `s₀`, the
//!   strip being decided in boxes of `(t, s)`: a box from `s = 0` holds
//!   when `∂G/∂s ≥ g₁ > 0` (with the required sign) on it and `G ≥ −A` on
//!   its edge `s = 0`, beyond `s₀ = A/g₁`; a box away from `s = 0` holds
//!   when `G` has the required sign on its coefficients. The certificate
//!   holds on the strip beyond the greatest `s₀` ([`Held::s0`]). A box
//!   corner (where a coefficient is the value) of certified wrong sign
//!   away from `s = 0` ends it: the face crosses the ruled surface there
//!   ([`EdgeCert::Fails`]), so the certificate holds on no strip reaching
//!   that far.
//! - **A planar curve** uses its plane as a fast path: on a piece the
//!   curve's span lies in one plane (a line in the plane it spans with
//!   `m` at the piece's middle), and two faces on opposite sides of it
//!   beyond their join zones meet nowhere else there. `Gᵢ = n·(Xᵢ − p₀)`
//!   is of the strip's degree alone; the plane is oriented as `m × C′`
//!   at the piece's middle and tried on few boxes first; where it doesn't
//!   settle at once, the ruled surface decides.
//!
//! **The vertex certificate** ([`certify_vertex`]), for two faces
//! sharing a vertex but no edge there: a plane through the vertex with
//! each face's germ strictly on its side, beyond a join zone round the
//! vertex. A face's germ at a corner lies between its two coedges'
//! curves, which leave the vertex inside the cones of their spans'
//! control points (positive weights); so it lies in the **sector** from
//! the outer side of one cone to the outer side of the other through the
//! face, cut into sectors under a half turn (and at the tile lines
//! through the vertex), each the triangle `uv + h·ρ·((1 − β)·a + β·b)`,
//! `ρ, β ∈ [0, 1]`, in one tile. The plane's sign `ℓ·(X − v)` over a
//! sector is of degrees (4, 4) and is decided as a strip is, `ρ` for `s`
//! ([`VertCert`]). The plane is proposed from the sectors' far points (a
//! cone containment, [`propose_axis`]) and only certified here.
//!
//! **The window** ([`window::certify_window`]), per edge: the tube's
//! radius `ρ_adj`, a power of two with `ρ_min ≤ ρ_adj ≤ ρ_graph` and `r₀ <
//! ρ_adj`, else the window is empty.
//! - **`ρ_min`**: from it on the faces are apart by more than `δ =
//!   ε_exc·(1 + 2⁻⁸)` (`ε_exc = ε/8`), so that the leaves beyond the tube
//!   certify them apart. On a cell where `sin²α` (the angle between the
//!   normals) has a certified positive lower bound, a crease's
//!   `δ/sin α_min`; elsewhere (tangent and curvature-continuous joins, a
//!   G² joint of different surfaces, any order) the separation itself: each
//!   face's strip against the other face's surface across the edge (its
//!   tile's implicit `F`, the form extended), box by box, a box apart where
//!   `|F|` exceeds `δ` times a bound of `|∇F|` around it
//!   ([`window::separation`]); `ρ_min` bounds the distance from the curve of
//!   every box not shown apart. The edge's is its cells' largest. Faces
//!   on **one surface** (one surface object, or a
//!   [`SameForm`](EdgeKind::SameForm) edge) lie on either side of their
//!   common p-curve on one injective surface: no certificate, `ρ_min` 0,
//!   and `ρ_adj` the least power of two above their ribbons; beyond it
//!   their trims' disjointness in the surface's UV decides, a later part.
//! - **`ρ_graph`**: the tube may reach no other edge of the two faces
//!   (but those sharing an end with it, the vertices' balls'), and the
//!   strips must reach out of it: the curve against those edges and
//!   against each strip's far side, by control boxes
//!   ([`window::apart`]); and no failing point of the certificate.
//! - **`r₀`**: the join zone's points (the strip short of `s₀`, the
//!   ribbon included) lie within it of the curve's point at their `t`.
//! - **The search.** The strips start about twice as wide as the nearest
//!   other edge is far. Where the certificate fails at a point, no tube
//!   reaching it holds: the strip narrows below it, and the point's
//!   distance bounds `ρ_graph` from above. The window is **certified
//!   empty** where no power of two lies between a lower bound of `ρ_min`
//!   (a crease's angle at a cell's middle) and an upper bound of
//!   `ρ_graph` (a failing point's distance, two points of the curve and
//!   another edge): [`Finding::EmptyWindow`], a settled fact. Else `ρ_adj` is the least
//!   power of two at least `ρ_min` and above `r₀`, doubled while the other
//!   edges and the far sides stay beyond it. A window neither empty nor
//!   holding a tube is unsettled ([`Cap::PrecisionBits`]).
//!
//! **Precision and work.** Every cell and box is computed at a level of
//! the escalation schedule from the stored values, and escalates where
//! rounding decides (its values settled but its enclosures not), else is
//! halved; each is charged before it runs (the unit table's
//! [`adjacent`](varde_decide::num::schedule::Units::adjacent) row, a
//! precision step above the first level: a field cell's by whether both
//! faces lie on planes, else whether a span is quadratic; a box's by
//! whether a span is quadratic). A tile's numerator at the first level is
//! formed once, the tile's own ([`Tile::fold_numerator_reduced_at`]). A cell still open at the top
//! level stops the certificate at [`Cap::PrecisionBits`], one past
//! [`MAX_SUBDIV_DEPTH`] halvings at [`Cap::SubdivisionDepth`]. Cells and
//! boxes run depth first in `t` order, then `s`, the face whose coedge
//! runs the edge's way first, so the outcome and the work depend on the
//! input alone.

use std::cmp::Ordering;

use malachite::Rational;
use varde_decide::bezier::{
    Dir, HomCurve, HomPatch, Poly, Scalar, Tensor, cone::propose_axis, cross, dot,
};
use varde_decide::num::bounded::{Bounded, div_up, down, up};
use varde_decide::num::level::{AtLevel, LevelScalar, at_level};
use varde_decide::num::schedule::{Level, UNITS};

use super::brep::{Brep, CoedgeId, EdgeId, EdgeKind, FaceId, SurfId, VertId, VertKind};
use super::check::{Cx, Fault, Finding, Invariant, Item, ItemKind};
use crate::geom::certify::restrict;
use crate::geom::curve::{Curve3D, local};
use crate::geom::form::FormKey;
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::geom::tile::Tile;
use crate::infra::budget::{Cap, LEVELS, Ledger, MAX_SUBDIV_DEPTH, Step, Stop, level_step};

// ---------------------------------------------------------------------
// The check's hook.

/// Invariant 9's hook (the check's driver, [`super::check`]): contact
/// records, then edges and vertices (the adjacent part), then faces (the
/// non-adjacent part, and the adjacent pairs beyond their tubes). An
/// edge's faces are certified inside its tube (module docs, "The
/// window"); this build certifies no contact record, no ball round a
/// vertex and no pair by its leaves: so a body with a contact record, a
/// pair of faces at a vertex alone, or a face, needs a part this build
/// doesn't have ([`Finding::Unbuilt`]). An edge without a pair of faces
/// (singular, or a sheet's open boundary) and a vertex whose faces all
/// share edges there need nothing here.
pub(crate) struct Embedding;

/// The checks of invariant 9, numbered in the hook's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum EmbedCheck {
    /// A contact record.
    Contact = 1,
    /// The faces across an edge, inside its tube.
    Edge,
    /// Faces sharing only a vertex, inside its ball.
    Vertex,
    /// A face against the faces it shares nothing with.
    Apart,
}

fn unbuilt(check: EmbedCheck) -> Result<(), Stop<Fault>> {
    Err(Stop::Failed(Fault {
        check: check as u32,
        finding: Finding::Unbuilt,
    }))
}

impl Invariant for Embedding {
    fn kinds(&self) -> &[ItemKind] {
        &[
            ItemKind::Contact,
            ItemKind::Edge,
            ItemKind::Vert,
            ItemKind::Face,
        ]
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        ledger.charge(Step::Work, 1)?;
        let b = cx.body();
        match item {
            Item::Contact(_) => unbuilt(EmbedCheck::Contact),
            Item::Edge(e) => match edge_of(b, e) {
                Ok(edge) => edge_tube(cx, e, &edge, ledger),
                Err(Skip::Staged) => unbuilt(EmbedCheck::Edge),
                Err(Skip::NoPair | Skip::Unreadable) => Ok(()),
            },
            Item::Vert(v) => {
                if b.vert_kind(v) == VertKind::Pole && !cx.vert_faces(v).is_empty() {
                    return unbuilt(EmbedCheck::Vertex);
                }
                if vertex_pairs(cx, v).next().is_some() {
                    ball(EmbedCheck::Vertex)
                } else {
                    Ok(())
                }
            }
            Item::Face(_) => unbuilt(EmbedCheck::Apart),
            _ => Ok(()),
        }
    }
}

/// The ball round a vertex whose faces meet there alone, and its
/// radius's window: a part this build doesn't have.
fn ball(check: EmbedCheck) -> Result<(), Stop<Fault>> {
    unbuilt(check)
}

/// The window's threshold for `body`: `ε_exc = ε/8`, with the margin of
/// `ρ_min`. A result's check reads `ε′ = min(ε_exc, b_K,min/2)` in its
/// place (its operation's least cluster band), which the operations
/// pass as they land.
pub(crate) fn window_delta(body: &Brep) -> f64 {
    body.eps().eps().eps() * 0.125 * window::MARGIN
}

/// The tube of edge `e` (`edge` its faces) on the check's body: the
/// window between `ρ_min` and `ρ_graph` with the faces' other edges
/// ([`near`]), the inside certificate on the strips it reaches. The
/// outside part (the faces' leaves beyond the tube) is the non-adjacent
/// pass's ([`EmbedCheck::Apart`]), which reads the tube ([`window_of`]):
/// a pair beyond its tube is checked as a pair sharing nothing. A window
/// certified empty fails ([`Finding::EmptyWindow`]); one unsettled stops
/// at its cap; a curve the earlier invariants refuse is theirs to report.
/// The faces' edges it reads are charged a unit each.
fn edge_tube(
    cx: &Cx<'_>,
    e: EdgeId,
    edge: &Edge<'_>,
    ledger: &mut Ledger<'_>,
) -> Result<(), Stop<Fault>> {
    let b = cx.body();
    let faces = edge_faces(b, e);
    let read: usize = faces.iter().map(|&f| cx.face_edges(f).len()).sum();
    ledger.charge(Step::Work, read as u64)?;
    let near = near(
        b,
        e,
        faces.iter().flat_map(|&f| cx.face_edges(f).iter().copied()),
    );
    match window::certify_window(edge, &near, window_delta(b), ledger) {
        Ok(window::Window::Tube(_)) | Err(Stop::Failed(_)) => Ok(()),
        Ok(window::Window::Empty { .. }) => Err(Stop::Failed(Fault {
            check: EmbedCheck::Edge as u32,
            finding: Finding::EmptyWindow,
        })),
        Err(Stop::Cap(c)) => Err(Stop::Cap(c)),
        Err(Stop::Exhausted(x)) => Err(Stop::Exhausted(x)),
    }
}

/// The faces of edge `e`'s coedges, read guarded.
fn edge_faces(b: &Brep, e: EdgeId) -> Vec<FaceId> {
    let cs = &b.coedges;
    let Some(&c0) = b.edges.coedge.get(e.index()) else {
        return Vec::new();
    };
    let c1 = cs.pair.get(c0.index()).copied();
    [Some(c0), c1]
        .into_iter()
        .flatten()
        .filter_map(|c| cs.face.get(c.index()).copied())
        .filter(|f| f.index() < b.face_count())
        .collect()
}

/// The other edges among `edges` near which edge `e`'s tube may not
/// reach (module docs, "The window"): each once, but `e` and the edges
/// sharing an end with it (by id or by position, bit for bit: the
/// vertex's ball takes those), their spans at the first level, in the
/// order of their end points (so the distance a search meets first
/// doesn't depend on the ids).
fn near(b: &Brep, e: EdgeId, edges: impl Iterator<Item = EdgeId>) -> Vec<window::Arc3> {
    let es = &b.edges;
    let ends = |g: EdgeId| -> Option<[VertId; 2]> {
        Some([*es.v_start.get(g.index())?, *es.v_end.get(g.index())?])
    };
    let pos = |v: VertId| b.verts.pos.get(v.index()).map(|p| p.map(f64::to_bits));
    let Some(mine) = ends(e) else {
        return Vec::new();
    };
    let mine_at = mine.map(pos);
    let mut ids: Vec<EdgeId> = edges.filter(|&g| g != e).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    for g in ids {
        let Some(theirs) = ends(g) else { continue };
        let touches = theirs
            .iter()
            .any(|&v| mine.contains(&v) || (pos(v).is_some() && mine_at.contains(&pos(v))));
        if touches {
            continue;
        }
        let Some(&c) = es.curve.get(g.index()) else {
            continue;
        };
        if c.index() >= b.curve_count() {
            continue;
        }
        if let Some(a) = window::arcs(b.curve(c), es.t_range[g.index()]) {
            out.extend(a);
        }
    }
    let ends = |a: &window::Arc3| -> [f64; 6] {
        let p = |k: usize| {
            let i = if k == 0 { 0 } else { a.degree() };
            let w = a.weights().coefs()[i].v;
            [0, 1, 2].map(|c| a.coords()[c].coefs()[i].v / w)
        };
        let (s, e) = (p(0), p(1));
        [s[0], s[1], s[2], e[0], e[1], e[2]]
    };
    out.sort_by(|x, y| {
        let (a, b) = (ends(x), ends(y));
        (0..6)
            .map(|k| a[k].total_cmp(&b[k]))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    });
    out
}

/// The window of edge `e` of `body` recomputed for its evidence on a
/// budget of `units`: its two faces, and a certified lower bound of
/// `ρ_min` and an upper bound of `ρ_graph` where it is certified empty
/// within them.
pub(crate) fn empty_window(body: &Brep, e: EdgeId, units: u64) -> Option<([FaceId; 2], [f64; 2])> {
    use crate::infra::budget::{Budget, Pass, Work};
    let [f, g] = <[FaceId; 2]>::try_from(edge_faces(body, e)).ok()?;
    let mut work = Work::new(&Budget::new(units));
    let pass = Pass {
        stage: "check 9 evidence",
        rung: 0,
    };
    let out = work
        .pass(pass, &[()], |_, ledger| window_of(body, e, ledger))
        .ok()?;
    match out.first()? {
        window::Window::Empty { rho_min, rho_graph } => Some(([f, g], [*rho_min, *rho_graph])),
        window::Window::Tube(_) => None,
    }
}

/// The window of edge `e` of `body` as the check certifies it, the
/// faces' other edges read off their loops (guarded).
pub(crate) fn window_of(
    body: &Brep,
    e: EdgeId,
    ledger: &mut Ledger<'_>,
) -> Result<window::Window, Stop<Skip>> {
    let edge = edge_of(body, e).map_err(Stop::Failed)?;
    let edges: Vec<EdgeId> = edge_faces(body, e)
        .iter()
        .flat_map(|&f| loop_edges(body, f))
        .collect();
    ledger.charge(Step::Work, edges.len() as u64)?;
    let near = near(body, e, edges.into_iter());
    window::certify_window(&edge, &near, window_delta(body), ledger)
}

/// The edges of face `f`'s loops, walked guarded (each loop at most as
/// many steps as the body has coedges).
fn loop_edges(b: &Brep, f: FaceId) -> Vec<EdgeId> {
    let (cs, ls) = (&b.coedges, &b.loops);
    let mut out = Vec::new();
    let Some(&first) = b.faces.first_loop.get(f.index()) else {
        return out;
    };
    let mut l = first;
    for _ in 0..b.loop_count() {
        let Some(&start) = ls.first_coedge.get(l.index()) else {
            break;
        };
        let mut c = start;
        for _ in 0..b.coedge_count() {
            let Some(&e) = cs.edge.get(c.index()) else {
                break;
            };
            out.push(e);
            match cs.next.get(c.index()) {
                Some(&n) if n != start => c = n,
                _ => break,
            }
        }
        match ls.next_loop.get(l.index()) {
            Some(&n) if n != first && !n.is_none() => l = n,
            _ => break,
        }
    }
    out
}

/// The pairs of faces at `v` that share no edge there, each once (the
/// lesser id first).
fn vertex_pairs<'a>(cx: &'a Cx<'_>, v: VertId) -> impl Iterator<Item = [FaceId; 2]> + 'a {
    let faces = cx.vert_faces(v);
    let b = cx.body();
    // The pairs joined by an edge at `v`: the faces of each corner's
    // coedge and of its pair.
    let mut joined: Vec<[FaceId; 2]> = cx
        .vert_corners(v)
        .iter()
        .filter_map(|&c| {
            let p = *b.coedges.pair.get(c.index())?;
            let f = *b.coedges.face.get(c.index())?;
            let g = *b.coedges.face.get(p.index())?;
            Some(if f <= g { [f, g] } else { [g, f] })
        })
        .collect();
    // The corners' next coedges leave `v` along the other edges there.
    joined.extend(cx.vert_corners(v).iter().filter_map(|&c| {
        let n = *b.coedges.next.get(c.index())?;
        let p = *b.coedges.pair.get(n.index())?;
        let f = *b.coedges.face.get(n.index())?;
        let g = *b.coedges.face.get(p.index())?;
        Some(if f <= g { [f, g] } else { [g, f] })
    }));
    joined.sort_unstable();
    joined.dedup();
    faces.iter().enumerate().flat_map(move |(i, &f)| {
        let joined = joined.clone();
        faces[i + 1..]
            .iter()
            .map(move |&g| [f, g])
            .filter(move |p| joined.binary_search(p).is_err())
    })
}

// ---------------------------------------------------------------------
// An edge's faces.

/// Why an edge or vertex has no certificate to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// An edge without two coedges (singular, or a sheet's open
    /// boundary): no pair of faces.
    NoPair,
    /// A zero-length edge, an edge ending at a pole, or a pole: certified
    /// (at its end) as at a vertex, a part this build doesn't have.
    Staged,
    /// An id or a curve the earlier invariants refuse: theirs to report.
    Unreadable,
}

/// One face of an edge, as the certificate reads it.
#[derive(Clone, Copy)]
pub(crate) struct Side<'a> {
    /// The face's surface.
    pub(crate) surface: &'a Surface,
    /// Its coedge's p-curve, over the edge's parameter.
    pub(crate) pcurve: &'a PCurve,
    /// Whether the face's outward normal is against its surface's.
    pub(crate) flipped: bool,
    /// Whether its coedge runs against the edge's curve.
    pub(crate) reversed: bool,
    /// Its coedge's seam bound: the width of its ribbon.
    pub(crate) seam: f64,
}

impl Side<'_> {
    /// Whether the face lies left of the edge's direction in its UV:
    /// left of its coedge's direction when not flipped, right when
    /// flipped, and the coedge runs the edge's way unless reversed.
    fn left(&self) -> bool {
        self.reversed == self.flipped
    }
}

/// An edge between two faces: its curve over `t_range`, and its faces,
/// the one whose coedge runs the edge's way first.
#[derive(Clone, Copy)]
pub(crate) struct Edge<'a> {
    /// The edge's curve.
    pub(crate) curve: &'a Curve3D,
    /// The edge's range of the curve's parameter.
    pub(crate) t_range: [f64; 2],
    /// The two faces: `[0]` with `G > 0`, `[1]` with `G < 0`.
    pub(crate) sides: [Side<'a>; 2],
    /// Whether the faces lie on one surface: one surface object, or
    /// surfaces of bit-identical form keys (a
    /// [`SameForm`](EdgeKind::SameForm) edge). Their window needs no
    /// certificate ([`window`]).
    pub(crate) one_surface: bool,
}

/// Edge `e` of `body` with its two faces, or why it has none to
/// certify.
pub(crate) fn edge_of(body: &Brep, e: EdgeId) -> Result<Edge<'_>, Skip> {
    let es = &body.edges;
    let cs = &body.coedges;
    let at = |v: &[CoedgeId], i: usize| v.get(i).copied();
    let c0 = at(&es.coedge, e.index()).ok_or(Skip::Unreadable)?;
    if body.edge_kind(e) == EdgeKind::Singular {
        return Err(Skip::NoPair);
    }
    if c0.index() >= body.coedge_count() {
        return Err(Skip::Unreadable);
    }
    let c1 = cs.pair[c0.index()];
    if c1.is_none() {
        return Err(Skip::NoPair);
    }
    if c1.index() >= body.coedge_count() {
        return Err(Skip::Unreadable);
    }
    let curve_id = es.curve[e.index()];
    if curve_id.index() >= body.curve_count() {
        return Err(Skip::Unreadable);
    }
    let curve = body.curve(curve_id);
    let t_range = es.t_range[e.index()];
    // Towards a pole the strips close up (`∂X/∂s → 0`): the pole's ball
    // takes the edge's end, a part this build doesn't have.
    let ends = [es.v_start[e.index()], es.v_end[e.index()]];
    if ends
        .iter()
        .any(|v| body.verts.kind.get(v.index()) == Some(&VertKind::Pole))
    {
        return Err(Skip::Staged);
    }
    let side = |c: CoedgeId| -> Result<(Side<'_>, SurfId), Skip> {
        let f = cs.face[c.index()];
        let p = cs.pcurve[c.index()];
        if f.index() >= body.face_count() || p.index() >= body.pcurve_count() {
            return Err(Skip::Unreadable);
        }
        let s = body.faces.surface[f.index()];
        if s.index() >= body.surface_count() {
            return Err(Skip::Unreadable);
        }
        let side = Side {
            surface: body.surface(s),
            pcurve: body.pcurve(p),
            flipped: body.faces.flipped[f.index()],
            reversed: cs.reversed[c.index()],
            seam: cs.seam_bound[c.index()],
        };
        Ok((side, s))
    };
    let ((a, sa), (b, sb)) = (side(c0)?, side(c1)?);
    if a.reversed == b.reversed {
        return Err(Skip::Unreadable);
    }
    let sides = if a.reversed { [b, a] } else { [a, b] };
    let same_form = matches!(body.edge_kind(e), EdgeKind::SameForm { .. });
    let edge = Edge {
        curve,
        t_range,
        sides,
        one_surface: sa == sb || same_form,
    };
    if edge.pieces().is_none() {
        return Err(Skip::Unreadable);
    }
    if edge.zero_length() {
        return Err(Skip::Staged);
    }
    Ok(edge)
}

/// A piece of an edge (module docs): `[a, b]` in span `sc` of the curve
/// and spans `sp` of the two p-curves.
#[derive(Clone, Copy, Debug)]
struct Piece {
    a: f64,
    b: f64,
    sc: usize,
    sp: [usize; 2],
}

impl Edge<'_> {
    /// The pieces in `t` order; `None` where the p-curves don't run over
    /// `t_range` exactly or the curve doesn't cover it.
    fn pieces(&self) -> Option<Vec<Piece>> {
        let [t0, t1] = self.t_range;
        let ck = self.curve.knots();
        if !(t0 < t1 && ck[0] <= t0 && t1 <= ck[ck.len() - 1]) {
            return None;
        }
        let mut cuts: Vec<f64> = vec![t0, t1];
        for s in &self.sides {
            let pk = s.pcurve.knots();
            if pk[0].to_bits() != t0.to_bits() || pk[pk.len() - 1].to_bits() != t1.to_bits() {
                return None;
            }
            cuts.extend_from_slice(pk);
        }
        cuts.extend(ck.iter().filter(|&&t| t0 < t && t < t1));
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let span = |k: &[f64], a: f64| k.partition_point(|&x| x <= a).checked_sub(1);
        cuts.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let sc = span(ck, a)?.min(self.curve.spans().len() - 1);
                let sp = [0, 1].map(|i| {
                    let p = self.sides[i].pcurve;
                    span(p.knots(), a).map(|k| k.min(p.spans().len() - 1))
                });
                Some(Piece {
                    a,
                    b,
                    sc,
                    sp: [sp[0]?, sp[1]?],
                })
            })
            .collect()
    }

    /// Whether the curve over `t_range` is one point: every control point
    /// of its spans there the same bits.
    fn zero_length(&self) -> bool {
        let [t0, t1] = self.t_range;
        let k = self.curve.knots();
        let mut pts = self
            .curve
            .spans()
            .iter()
            .enumerate()
            .filter(|&(i, _)| k[i] < t1 && k[i + 1] > t0)
            .flat_map(|(_, s)| s.iter().map(|p| [p[0], p[1], p[2]].map(f64::to_bits)));
        let Some(first) = pts.next() else {
            return true;
        };
        pts.all(|p| p == first)
    }
}

// ---------------------------------------------------------------------
// The edge certificate.

/// The UV width of each face's strip: how far into the face the sign is
/// certified, along `ν` of largest coordinate 1 (module docs). The tube
/// decides it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Strip {
    /// Per face, in its edge's order: the strip's width in global UV.
    pub(crate) width: [f64; 2],
}

/// What the certificate showed on its strips.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Held {
    /// Per face, the greatest `s₀` (in global UV along `ν`): beyond it
    /// the face is on its side of the ruled surface (or plane).
    pub(crate) s0: [f64; 2],
    /// Per face, the least strip width a cell kept (cut to its tile):
    /// the certificate covers the strip up to this width everywhere.
    pub(crate) reach: [f64; 2],
    /// Per face, a certified upper bound of the join zone's distance from
    /// the curve: of every point of the strip short of `s₀` from the
    /// curve's point at the same `t` (the ribbon included), `r₀`.
    pub(crate) r0: [f64; 2],
    /// A certified upper bound of `ρ_min` (module docs, "The window"),
    /// the largest of its cells'; `∞` where a cell's is unsettled.
    pub(crate) rho_min: f64,
    /// A certified lower bound of `ρ_min`: the largest of its creases'
    /// at their cells' middles; 0 where no cell is a certified crease.
    pub(crate) rho_min_lo: f64,
    /// Per face, the strips' far sides (`s` at their width), cell by
    /// cell in `t` order, at the first level: the tube's outer wall.
    pub(crate) far: [Vec<HomCurve<Bounded<f64>, 3>>; 2],
}

/// The edge certificate's outcome.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EdgeCert {
    /// It holds on the strips.
    Held(Held),
    /// Face `side` is certified on the wrong side at the edge's
    /// parameter `t`, `s` (global UV along `ν`) into the face: the
    /// certificate holds on no strip reaching that far there.
    Fails {
        /// The face, in its edge's order.
        side: usize,
        /// The edge's parameter.
        t: f64,
        /// How far into the face.
        s: f64,
        /// A certified upper bound of the distance from the curve of the
        /// point it fails at: no tube reaching that far holds.
        dist: f64,
        /// A certified lower bound of `ρ_min` from the cells settled
        /// before it, as [`Held::rho_min_lo`].
        rho_min_lo: f64,
    },
}

/// The ruled-surface certificate of `edge` on the strips `strip`
/// (module docs), each cell and box charged to `ledger`; with the
/// window's bounds on the strips that hold (module docs, "The window"):
/// the faces counted apart by more than `delta` beyond `ρ_min`.
pub(crate) fn certify_edge(
    edge: &Edge<'_>,
    strip: &Strip,
    delta: f64,
    ledger: &mut Ledger<'_>,
) -> Result<EdgeCert, Stop<Skip>> {
    let pieces = edge.pieces().ok_or(Stop::Failed(Skip::Unreadable))?;
    let mut held = Held {
        s0: [0.0; 2],
        reach: strip.width,
        r0: [0.0; 2],
        rho_min: 0.0,
        rho_min_lo: 0.0,
        far: [Vec::new(), Vec::new()],
    };
    for piece in &pieces {
        // A stack of field cells: the next last.
        let mut cells = vec![TCell {
            k: 0,
            depth: 0,
            level: 0,
        }];
        while let Some(cell) = cells.pop() {
            let row = if flat(edge) {
                &UNITS.adjacent.field_flat
            } else if quadratic(edge, piece) {
                &UNITS.adjacent.field
            } else {
                &UNITS.adjacent.field_low
            };
            ledger.charge(level_step(cell.level), row[usize::from(cell.level)])?;
            let mut at = FieldAt {
                edge,
                piece,
                cell,
                width: strip.width,
            };
            match run_at(&mut at, cell.level) {
                FieldOut::Ready(frame) => {
                    let at = Settle {
                        edge,
                        piece,
                        cell,
                        frame: &frame,
                        delta,
                    };
                    if let Some(fails) = settle(&at, &mut held, ledger)? {
                        return Ok(fails);
                    }
                }
                FieldOut::Precision => cells.push(up_level(cell)?),
                FieldOut::Space => {
                    let [l, r] = halves(cell)?;
                    cells.push(r);
                    cells.push(l);
                }
            }
        }
    }
    Ok(EdgeCert::Held(held))
}

/// Whether a face's p-curve span on the piece is quadratic in a
/// coordinate: the dearer row of the unit table.
fn quadratic(edge: &Edge<'_>, piece: &Piece) -> bool {
    (0..2).any(|i| {
        let span = &edge.sides[i].pcurve.spans()[piece.sp[i]];
        span.degrees().contains(&2)
    })
}

/// Whether both faces lie on planes, their normals their tiles' one
/// values ([`Tile::flat_normal`]): the cheapest row of the unit table.
fn flat(edge: &Edge<'_>) -> bool {
    edge.sides.iter().all(|s| {
        matches!(s.surface.lattice().key(), FormKey::Plane(_))
            && s.surface.tiles().iter().all(|t| t.flat_normal().is_some())
    })
}

/// A cell of a piece in `t`: part `k` of `2^depth`, at a precision level.
#[derive(Clone, Copy, Debug)]
struct TCell {
    k: u64,
    depth: u32,
    level: u8,
}

/// The cell one level up, or the cap.
fn up_level(c: TCell) -> Result<TCell, Stop<Skip>> {
    if c.level + 1 < LEVELS {
        Ok(TCell {
            level: c.level + 1,
            ..c
        })
    } else {
        Err(Stop::Cap(Cap::PrecisionBits))
    }
}

/// The cell's halves at its level, or the cap.
fn halves(c: TCell) -> Result<[TCell; 2], Stop<Skip>> {
    if c.depth >= MAX_SUBDIV_DEPTH {
        return Err(Stop::Cap(Cap::SubdivisionDepth));
    }
    Ok([0, 1].map(|b| TCell {
        k: 2 * c.k + b,
        depth: c.depth + 1,
        level: c.level,
    }))
}

/// Runs `f` at precision level `level` (an index of the schedule).
fn run_at<F: AtLevel<()>>(f: &mut F, level: u8) -> F::Out {
    let level = Level::from_index(usize::from(level)).unwrap_or(Level::TOP);
    at_level(f, level, &mut ())
}

/// What a field cell settled for its boxes: the field, and per face the
/// strip's direction, width and tile.
#[derive(Clone, Debug)]
struct Frame {
    /// The field `m` over the cell: its Bernstein coefficients, rounded
    /// once to `f64` from `N₁/λ₁ + N₂/λ₂` at the cell's level, and
    /// certified between the normals as they are (a rational field
    /// constructed once per cell).
    m: [Vec<f64>; 3],
    /// Per face `ν`, of largest coordinate 1 (a coordinate 0 where the
    /// strip may not move in it).
    nu: [[f64; 2]; 2],
    /// Per face the strip's width in global UV, a power of two.
    h: [f64; 2],
    /// Per face the tile's indices.
    tile: [(i64, i64); 2],
    /// The plane of the fast path, if the piece's curve span gives one:
    /// its normal, oriented as `m × C′` at the middle, and a point.
    plane: Option<([f64; 3], [f64; 3])>,
    /// `sin²` of the angle between the faces' normals: a certified lower
    /// bound over the cell (0 where none is positive: the faces may be
    /// tangent there), and a certified upper bound at its middle.
    sin2: [f64; 2],
}

/// A field cell's outcome.
enum FieldOut {
    Ready(Box<Frame>),
    Precision,
    Space,
}

/// The field cell `cell` of `piece` at one level.
struct FieldAt<'a, 'e> {
    edge: &'a Edge<'e>,
    piece: &'a Piece,
    cell: TCell,
    width: [f64; 2],
}

impl AtLevel<()> for FieldAt<'_, '_> {
    type Out = FieldOut;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> FieldOut {
        field_at(self.edge, self.piece, self.cell, self.width, like)
    }

    /// The certificate escalates cell by cell, not here.
    fn escalates(_: &FieldOut) -> bool {
        false
    }
}

/// The spans of the curve and both p-curves restricted to `[t_lo, t_hi]`
/// of the piece's local parameter's dyadic cell (`k`, `depth`), as spans
/// on `[0, 1]`.
struct Spans<S> {
    c: HomCurve<S, 3>,
    p: [[HomCurve<S, 1>; 2]; 2],
}

/// The local parameter of the piece's `[a, b]` in a span with knots
/// `[k0, k1]`, at `like`'s level.
fn piece_local<S: Scalar>(piece: &Piece, k: &[f64], i: usize, like: &S) -> (S, S) {
    (
        local(piece.a, k[i], k[i + 1], like),
        local(piece.b, k[i], k[i + 1], like),
    )
}

/// `c` halved down to part `k` of `2^depth`.
fn halve_to<S: Scalar, const N: usize>(
    mut c: HomCurve<S, N>,
    k: u64,
    depth: u32,
) -> HomCurve<S, N> {
    for bit in (0..depth).rev() {
        let [l, r] = c.split_half();
        c = if (k >> bit) & 1 == 1 { r } else { l };
    }
    c
}

fn spans_at<S: LevelScalar>(
    edge: &Edge<'_>,
    piece: &Piece,
    k: u64,
    depth: u32,
    like: &S,
) -> Spans<S> {
    let ck = edge.curve.knots();
    let (ca, cb) = piece_local(piece, ck, piece.sc, like);
    let c = restrict(&edge.curve.span_at(piece.sc, like).expect("a span"), ca, cb);
    let p = [0, 1].map(|i| {
        let pc = edge.sides[i].pcurve;
        let (pa, pb) = piece_local(piece, pc.knots(), piece.sp[i], like);
        pc.coords_at(piece.sp[i], like)
            .expect("a span")
            .map(|x| halve_to(restrict(&x, pa.clone(), pb.clone()), k, depth))
    });
    Spans {
        c: halve_to(c, k, depth),
        p,
    }
}

/// The certified range `[lo, hi]` of a coordinate span: its control
/// values' enclosures (positive weights keep it in their hull).
fn coord_range<S: Scalar>(x: &HomCurve<S, 1>) -> [f64; 2] {
    let mut r = [f64::INFINITY, f64::NEG_INFINITY];
    for (p, w) in x.coords()[0].coefs().iter().zip(x.weights().coefs()) {
        let [lo, hi] = p.div(w).enclose();
        if lo < r[0] || lo.is_nan() {
            r[0] = if lo.is_nan() { f64::NEG_INFINITY } else { lo };
        }
        if hi > r[1] || hi.is_nan() {
            r[1] = if hi.is_nan() { f64::INFINITY } else { hi };
        }
    }
    r
}

/// The largest power of two at most `x` (positive and normal), else 0.
fn dyadic_floor(x: f64) -> f64 {
    if x.is_normal() && x > 0.0 {
        f64::from_bits(x.to_bits() & 0xfff0_0000_0000_0000)
    } else {
        0.0
    }
}

/// `2^e` for `e` in the normal range.
fn pow2(e: i32) -> f64 {
    f64::from_bits(((e + 1023) as u64) << 52)
}

/// The tile a strip from a span with coordinate ranges `range` towards
/// `nu` lies in, and `ν` with each coordinate made 0 where the span
/// reaches the tile's side it points to (module docs); `None` where no
/// tile of the surface holds the span.
fn strip_tile(
    surface: &Surface,
    range: [[f64; 2]; 2],
    nu: [f64; 2],
) -> Option<((i64, i64), [f64; 2])> {
    let rect = surface.rect();
    let mut idx = [0i64; 2];
    let mut nu = nu;
    for k in 0..2 {
        let [lo, hi] = range[k];
        if !(lo.abs() < 4_503_599_627_370_496.0 && hi.abs() < 4_503_599_627_370_496.0) {
            return None;
        }
        let r = if k == 0 { &rect.u } else { &rect.v };
        let c = lo.floor() as i64;
        let fits = |i: i64| r.contains(&i) && (i as f64) <= lo && hi <= (i + 1) as f64;
        // On a tile line both tiles beside it hold the span: the strip's
        // side's.
        let order = if nu[k] < 0.0 { [c - 1, c] } else { [c, c - 1] };
        let i = order.into_iter().find(|&i| fits(i))?;
        if (nu[k] > 0.0 && hi >= (i + 1) as f64) || (nu[k] < 0.0 && lo <= i as f64) {
            nu[k] = 0.0;
        }
        idx[k] = i;
    }
    Some(((idx[0], idx[1]), nu))
}

/// The strip's width: the largest power of two at most `width` keeping
/// `range + [0, h]·ν` in tile `(i, j)`; 0 where none does.
fn strip_width(range: [[f64; 2]; 2], tile: (i64, i64), nu: [f64; 2], width: f64) -> f64 {
    let mut h = width;
    for k in 0..2 {
        let i = if k == 0 { tile.0 } else { tile.1 } as f64;
        let room = if nu[k] > 0.0 {
            down(down((i + 1.0) - range[k][1]) / up(nu[k]))
        } else if nu[k] < 0.0 {
            down(down(range[k][0] - i) / up(-nu[k]))
        } else {
            continue;
        };
        if room < h {
            h = room;
        }
    }
    dyadic_floor(h)
}

/// The tile's normal numerator the fold check reads at `like`'s level,
/// reduced to its true degrees where it is an elevation (module docs),
/// at the first level the tile's own, computed once
/// ([`Tile::fold_numerator_reduced_at`]).
fn numerator<S: Scalar + 'static>(tile: &Tile, like: &S) -> Option<[Tensor<S>; 3]> {
    tile.fold_numerator_reduced_at(like)
}

/// A coordinate span moved to tile index `i`'s local coordinate.
fn moved<S: Scalar>(x: &HomCurve<S, 1>, i: i64, like: &S) -> HomCurve<S, 1> {
    // Tile indices below 2⁵² in magnitude: exact in `f64`.
    x.translated(&[S::from_f64(i as f64, like)])
}

/// Face `side`'s outward normal numerator along its p-curve span `p` on
/// tile `(i, j)`; on a flat tile its one value ([`Tile::flat_normal`]),
/// a constant whose error holds every value over the tile.
fn normal_along<S: LevelScalar>(
    side: &Side<'_>,
    tile: &Tile,
    (i, j): (i64, i64),
    p: &[HomCurve<S, 1>; 2],
    like: &S,
) -> Option<[Poly<S>; 3]> {
    let a = match tile.flat_normal() {
        Some(n) => {
            n.map(|c| Poly::new([S::from_f64(c.v, like).widened(c.err())]).expect("a constant"))
        }
        None => {
            let n = numerator(tile, like)?;
            let (pu, pv) = (moved(&p[0], i, like), moved(&p[1], j, like));
            let u = [&pu.coords()[0], pu.weights()];
            let v = [&pv.coords()[0], pv.weights()];
            [
                n[0].compose_params(u, v)?,
                n[1].compose_params(u, v)?,
                n[2].compose_params(u, v)?,
            ]
        }
    };
    Some(if side.flipped { a.map(|x| x.neg()) } else { a })
}

/// The approximate value of `p` at ½.
fn mid_value<S: LevelScalar>(p: &Poly<S>, like: &S) -> f64 {
    let half = S::from_f64(0.5, like);
    p.eval(&half).approx()
}

/// Whether `x` is certainly positive (false for a NaN).
fn positive(x: f64) -> bool {
    x > 0.0
}

/// Whether every coefficient is positive in value (not certified).
fn values_positive<S: LevelScalar>(c: &[S]) -> bool {
    c.iter().all(|x| x.approx() > 0.0)
}

/// The tangent numerator of a span, reduced to its true degree where it
/// is an elevation.
fn tangent<S: Scalar, const N: usize>(c: &HomCurve<S, N>) -> Option<[Poly<S>; N]> {
    let t = c.tangent_numerator()?;
    let r: Vec<Option<Poly<S>>> = t.iter().map(Poly::reduced).collect();
    if r.iter().all(Option::is_some) && t[0].degree() > 0 {
        Some(std::array::from_fn(|k| r[k].clone().expect("checked")))
    } else {
        Some(t)
    }
}

/// The field `m = N₁/λ₁ + N₂/λ₂` from the normals and the `λ` exponents.
fn field<S: Scalar>(n: &[[Poly<S>; 3]; 2], lambda: [f64; 2], like: &S) -> [Poly<S>; 3] {
    let k = lambda.map(|x| S::from_f64(x, like));
    std::array::from_fn(|c| n[0][c].scale(&k[0]).add(&n[1][c].scale(&k[1])))
}

fn field_at<S: LevelScalar>(
    edge: &Edge<'_>,
    piece: &Piece,
    cell: TCell,
    width: [f64; 2],
    like: &S,
) -> FieldOut {
    let sp = spans_at(edge, piece, cell.k, cell.depth, like);
    let mut nu = [[0.0; 2]; 2];
    let mut h = [0.0; 2];
    let mut tile = [(0, 0); 2];
    let mut normals: Vec<[Poly<S>; 3]> = Vec::with_capacity(2);
    let mut lambda = [0.0; 2];
    let mut precision = false;
    for i in 0..2 {
        let side = &edge.sides[i];
        let p = &sp.p[i];
        // The UV tangent at the middle, approximately: `x′ = T / W²`.
        let (Some([tu]), Some([tv])) = (p[0].tangent_numerator(), p[1].tangent_numerator()) else {
            return FieldOut::Space;
        };
        let d = [(&tu, &p[0]), (&tv, &p[1])].map(|(t, x)| {
            let w = mid_value(x.weights(), like);
            mid_value(t, like) / (w * w)
        });
        let rot = if side.left() {
            [-d[1], d[0]]
        } else {
            [d[1], -d[0]]
        };
        let (au, av) = (rot[0].abs(), rot[1].abs());
        let big = if au > av { au } else { av };
        if !(big.is_normal()) {
            return FieldOut::Space;
        }
        // The cell's UV ranges, within the stored span's (exact: a span on
        // a tile line is on it, not about it).
        let hull = side.pcurve.spans()[piece.sp[i]].hull();
        let range = [0, 1].map(|k| {
            let r = coord_range(&p[k]);
            [
                if r[0] > hull[0][k] { r[0] } else { hull[0][k] },
                if r[1] < hull[1][k] { r[1] } else { hull[1][k] },
            ]
        });
        let Some((t, n)) = strip_tile(side.surface, range, [rot[0] / big, rot[1] / big]) else {
            return FieldOut::Space;
        };
        if n == [0.0, 0.0] {
            return FieldOut::Space;
        }
        let w = strip_width(range, t, n, width[i]);
        if !positive(w) {
            return FieldOut::Space;
        }
        // `det(p′, ν)·W_u²·W_v² = T_u·W_v²·ν_v − T_v·W_u²·ν_u`, of the
        // face's sign.
        let det = {
            let (wu, wv) = (p[0].weights(), p[1].weights());
            let a = tu.mul(&wv.mul(wv).expect("low")).expect("low");
            let b = tv.mul(&wu.mul(wu).expect("low")).expect("low");
            a.scale(&S::from_f64(n[1], like))
                .sub(&b.scale(&S::from_f64(n[0], like)))
        };
        let det = if side.left() { det } else { det.neg() };
        if !positive(det.hull().lo) {
            if values_positive(det.coefs()) {
                precision = true;
            } else {
                return FieldOut::Space;
            }
        }
        let Some(surf_tile) = side.surface.tile(t.0, t.1) else {
            return FieldOut::Space;
        };
        let Some(nn) = normal_along(side, surf_tile, t, p, like) else {
            return FieldOut::Space;
        };
        let len = {
            let v = nn.each_ref().map(|c| mid_value(c, like));
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };
        let k = 1.0 / len;
        if !(k.is_normal() && len.is_normal()) {
            return FieldOut::Space;
        }
        lambda[i] = k;
        normals.push(nn);
        nu[i] = n;
        h[i] = w;
        tile[i] = t;
    }
    let n = [normals[0].clone(), normals[1].clone()];
    let m = field(&n, lambda, like);
    let coefs = m.each_ref().map(|c| {
        c.coefs()
            .iter()
            .map(LevelScalar::approx)
            .collect::<Vec<_>>()
    });
    if !coefs.iter().flatten().all(|x| x.is_finite()) {
        return FieldOut::Space;
    }
    let m = coefs.each_ref().map(|c| poly_of(c, like));
    for nn in &n {
        let Some(d) = dot(&m, nn) else {
            return FieldOut::Space;
        };
        if !positive(d.hull().lo) {
            if values_positive(d.coefs()) {
                precision = true;
            } else {
                return FieldOut::Space;
            }
        }
    }
    if precision {
        return FieldOut::Precision;
    }
    let plane = plane_of(edge, piece, &sp.c, &m, like);
    FieldOut::Ready(Box::new(Frame {
        m: coefs,
        nu,
        h,
        tile,
        plane,
        sin2: sin2_bounds(&n, like),
    }))
}

/// `sin²` of the angle between the normals `n` over a cell (module docs,
/// "The window"): `|N₁ × N₂|² / (|N₁|²·|N₂|²)`, its lower bound over the
/// cell from the Bernstein hulls (0 where that isn't positive) and its
/// upper bound at the middle; `[0, ∞]` where the products pass the
/// largest degree.
fn sin2_bounds<S: LevelScalar>(n: &[[Poly<S>; 3]; 2], like: &S) -> [f64; 2] {
    let parts = || -> Option<(Poly<S>, Poly<S>)> {
        let c = cross(&n[0], &n[1])?;
        let num = dot(&c, &c)?;
        let den = dot(&n[0], &n[0])?.mul(&dot(&n[1], &n[1])?)?;
        Some((num, den))
    };
    let Some((num, den)) = parts() else {
        return [0.0, f64::INFINITY];
    };
    let (nh, dh) = (num.hull(), den.hull());
    let lo = if nh.lo > 0.0 && dh.hi.is_finite() && dh.hi > 0.0 {
        let q = down(down(nh.lo) / up(dh.hi));
        if q > 0.0 { q } else { 0.0 }
    } else {
        0.0
    };
    let half = S::from_f64(0.5, like);
    let [_, a] = num.eval(&half).enclose();
    let [b, _] = den.eval(&half).enclose();
    let hi = if a >= 0.0 { div_up(a, b) } else { 0.0 };
    [lo, if hi.is_nan() { f64::INFINITY } else { hi }]
}

/// The polynomial of the `f64` coefficients `c` at `like`'s level.
fn poly_of<S: Scalar>(c: &[f64], like: &S) -> Poly<S> {
    Poly::new(c.iter().map(|&x| S::from_f64(x, like))).expect("a field's degree")
}

/// `p` halved down to part `k` of `2^depth`.
fn halve_poly<S: Scalar>(mut p: Poly<S>, k: u64, depth: u32) -> Poly<S> {
    for bit in (0..depth).rev() {
        let [l, r] = p.split_half();
        p = if (k >> bit) & 1 == 1 { r } else { l };
    }
    p
}

/// The fast path's plane for the piece (module docs): the plane of its
/// curve span's control points, or for a line the plane through it
/// along `m` at the middle; oriented as `m × C′` there. `None` where
/// neither gives a plane in `f64`.
fn plane_of<S: LevelScalar>(
    edge: &Edge<'_>,
    piece: &Piece,
    c: &HomCurve<S, 3>,
    m: &[Poly<S>; 3],
    like: &S,
) -> Option<([f64; 3], [f64; 3])> {
    let span = edge.curve.spans().get(piece.sc)?;
    let p: [[f64; 3]; 3] = span.map(|x| [x[0], x[1], x[2]]);
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let crs = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dotf = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let tan = tangent(c)?;
    let tm = tan.each_ref().map(|x| mid_value(x, like));
    let mm = m.each_ref().map(|x| mid_value(x, like));
    let side = crs(mm, tm);
    let (u, v) = (sub(p[1], p[0]), sub(p[2], p[0]));
    let n = if collinear(&p) {
        crs(tm, mm)
    } else {
        crs(u, v)
    };
    let o = dotf(n, side);
    if !(o.is_normal()) {
        return None;
    }
    Some((if o > 0.0 { n } else { n.map(|x| -x) }, p[0]))
}

/// Whether three points are exactly collinear (their differences' cross
/// product exactly zero).
fn collinear(p: &[[f64; 3]; 3]) -> bool {
    let q = |x: f64| Rational::try_from(x).unwrap_or_default();
    let d = |a: [f64; 3], b: [f64; 3]| [0, 1, 2].map(|k| q(a[k]) - q(b[k]));
    let (u, v) = (d(p[1], p[0]), d(p[2], p[0]));
    let zero = Rational::default();
    [(1, 2), (2, 0), (0, 1)]
        .iter()
        .all(|&(i, j)| &u[i] * &v[j] - &u[j] * &v[i] == zero)
}

// ---------------------------------------------------------------------
// The strip's boxes.

/// A box of a strip: part `kt` of `2^dt` of its field cell in `t` (or of
/// a sector in `β`), part `ks` of `2^ds` of the strip in `s` (or `ρ`), at
/// a precision level.
#[derive(Clone, Copy, Debug)]
struct SBox {
    kt: u64,
    dt: u32,
    ks: u64,
    ds: u32,
    level: u8,
}

impl SBox {
    const WHOLE: SBox = SBox {
        kt: 0,
        dt: 0,
        ks: 0,
        ds: 0,
        level: 0,
    };

    /// `[s_a, s_b]`, exact.
    fn s_range(&self) -> [f64; 2] {
        let d = pow2(-(self.ds as i32));
        [self.ks as f64 * d, (self.ks + 1) as f64 * d]
    }

    /// `[t_a, t_b]` within its cell, exact.
    fn t_range(&self) -> [f64; 2] {
        let d = pow2(-(self.dt as i32));
        [self.kt as f64 * d, (self.kt + 1) as f64 * d]
    }
}

/// A box's outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BoxOut {
    /// It holds beyond `s₀` (relative to the strip).
    Holds { s0: f64 },
    /// A corner of certified wrong sign, at `(t, s)` relative to the
    /// cell and the strip, `s > 0`.
    Fails { t: f64, s: f64 },
    /// Rounding decides: one level up.
    Precision,
    /// Halve in `t`.
    SplitT,
    /// Halve in `s`.
    SplitS,
}

/// A strip's sign function, formed per box at a level: the form `F`
/// required positive (the sign applied) and the weight required positive
/// over the box, each a tensor in `(t, s)` on the box.
trait SignFn {
    /// The forms on box `b` at `like`'s level; `None` where they can't be
    /// formed (past the largest degree).
    fn forms<S: LevelScalar>(&self, b: &SBox, like: &S) -> Option<(Tensor<S>, Tensor<S>)>;

    /// The units of one box at level `level`.
    fn units(&self, level: u8) -> u64;

    /// A certified upper bound of the distance from the edge's curve of
    /// box `b`'s points, at the first level (a failing corner's); `∞`
    /// where it has none.
    fn reach(&self, _b: &SBox) -> f64 {
        f64::INFINITY
    }
}

/// One box of `f` at its level.
struct BoxAt<'a, F> {
    f: &'a F,
    b: SBox,
}

impl<F: SignFn> AtLevel<()> for BoxAt<'_, F> {
    type Out = BoxOut;

    fn at<S: LevelScalar>(&mut self, like: &S, _: &mut ()) -> BoxOut {
        match self.f.forms(&self.b, like) {
            Some((g, w)) => judge(&g, &w, &self.b),
            None => BoxOut::SplitT,
        }
    }

    fn escalates(_: &BoxOut) -> bool {
        false
    }
}

/// The verdict on a box (module docs, "The join zone"): `g` the form
/// required positive, `w` the weight required positive.
fn judge<S: LevelScalar>(g: &Tensor<S>, w: &Tensor<S>, b: &SBox) -> BoxOut {
    let [s_a, s_b] = b.s_range();
    let [t_a, t_b] = b.t_range();
    let (m, n) = g.degrees();
    // Corners of certified wrong sign away from `s = 0`.
    let lower = b.ks == 0;
    let corners: &[(usize, usize)] = if lower {
        &[(0, n), (m, n)]
    } else {
        &[(0, 0), (m, 0), (0, n), (m, n)]
    };
    for &(i, j) in corners {
        if n > 0 && g.get(i, j).sign() == Some(Ordering::Less) {
            return BoxOut::Fails {
                t: if i == 0 { t_a } else { t_b },
                s: if j == 0 { s_a } else { s_b },
            };
        }
    }
    if !positive(w.hull().lo) {
        return if values_positive(w.coefs()) {
            BoxOut::Precision
        } else {
            BoxOut::SplitT
        };
    }
    if !lower {
        if g.hull().lo > 0.0 {
            return BoxOut::Holds { s0: s_a };
        }
        if values_positive(g.coefs()) {
            return BoxOut::Precision;
        }
        return if b.dt <= b.ds {
            BoxOut::SplitT
        } else {
            BoxOut::SplitS
        };
    }
    // From `s = 0`: monotone in `s` beyond the join zone.
    let d = g.derivative(Dir::V);
    let g1 = d.hull().lo;
    if g1 > 0.0 {
        let a = (0..=m)
            .map(|i| -g.get(i, 0).enclose()[0])
            .fold(0.0, |acc: f64, x| if x > acc { x } else { acc });
        let sigma = if a > 0.0 { div_up(a, g1) } else { 0.0 };
        if sigma < 1.0 {
            return BoxOut::Holds {
                s0: s_a + (s_b - s_a) * sigma,
            };
        }
        return BoxOut::SplitS;
    }
    if values_positive(d.coefs()) {
        return BoxOut::Precision;
    }
    // The derivative fails near the edge: halve in `t`; farther out:
    // in `s`.
    let near = (0..=m).all(|i| d.get(i, 0).approx() > 0.0);
    if near { BoxOut::SplitS } else { BoxOut::SplitT }
}

/// The strip outcome: holds beyond `s₀` (relative), or fails at a point
/// `(t, s)` relative to the cell and the strip, at most the third value
/// from the curve.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StripOut {
    Holds(f64),
    Fails(f64, f64, f64),
}

/// Decides `f`'s strip box by box (module docs), from cell depth
/// `depth` (the halvings so far count towards the limit); `fast`: give up
/// (`None`) at the first box that doesn't hold at the first two levels
/// within one halving.
fn decide_strip<F: SignFn>(
    f: &F,
    depth: u32,
    fast: bool,
    ledger: &mut Ledger<'_>,
) -> Result<Option<StripOut>, Stop<Skip>> {
    let mut boxes = vec![SBox::WHOLE];
    let mut s0: f64 = 0.0;
    while let Some(b) = boxes.pop() {
        ledger.charge(level_step(b.level), f.units(b.level))?;
        let out = run_at(&mut BoxAt { f, b }, b.level);
        let deep = depth + b.dt + b.ds;
        match out {
            BoxOut::Holds { s0: s } => {
                if s > s0 {
                    s0 = s;
                }
            }
            BoxOut::Fails { t, s } => {
                return Ok((!fast).then(|| StripOut::Fails(t, s, f.reach(&b))));
            }
            BoxOut::Precision => {
                if fast && b.level >= 1 {
                    return Ok(None);
                }
                if b.level + 1 >= LEVELS {
                    return Err(Stop::Cap(Cap::PrecisionBits));
                }
                boxes.push(SBox {
                    level: b.level + 1,
                    ..b
                });
            }
            BoxOut::SplitT | BoxOut::SplitS => {
                if fast && b.dt + b.ds >= 1 {
                    return Ok(None);
                }
                if deep >= MAX_SUBDIV_DEPTH {
                    return Err(Stop::Cap(Cap::SubdivisionDepth));
                }
                let halves = if out == BoxOut::SplitT {
                    [1, 0].map(|x| SBox {
                        kt: 2 * b.kt + x,
                        dt: b.dt + 1,
                        ..b
                    })
                } else {
                    [1, 0].map(|x| SBox {
                        ks: 2 * b.ks + x,
                        ds: b.ds + 1,
                        ..b
                    })
                };
                boxes.extend(halves);
            }
        }
    }
    Ok(Some(StripOut::Holds(s0)))
}

/// An edge face's strip over a field cell.
struct EdgeStrip<'a, 'e> {
    edge: &'a Edge<'e>,
    piece: &'a Piece,
    cell: TCell,
    frame: &'a Frame,
    side: usize,
    /// The fast path's plane in place of the ruled surface.
    plane: bool,
}

/// The strip map's coordinate: `x(t) + (s_a + σ·(s_b − s_a))·h·ν`, over
/// its own weight: a tensor in `(t, σ)`, of degree 1 in `σ` where `ν` has
/// that coordinate.
fn strip_coord<S: Scalar>(
    x: &HomCurve<S, 1>,
    nu: f64,
    h: f64,
    s: [f64; 2],
    like: &S,
) -> [Tensor<S>; 2] {
    let (xs, ws) = (x.coords()[0].coefs(), x.weights().coefs());
    let k = xs.len() - 1;
    let w = Tensor::new(k, 0, ws.iter().cloned()).expect("a span's degree");
    if nu == 0.0 {
        return [
            Tensor::new(k, 0, xs.iter().cloned()).expect("a span's degree"),
            w,
        ];
    }
    // `s·h` is exact: dyadic.
    let c = s.map(|s| S::from_f64(s * h, like).mul(&S::from_f64(nu, like)));
    let coefs = xs
        .iter()
        .zip(ws)
        .flat_map(|(x, w)| [x.add(&c[0].mul(w)), x.add(&c[1].mul(w))]);
    [Tensor::new(k, 1, coefs).expect("a span's degree"), w]
}

/// A polynomial in `t` as a tensor of degree 0 in `s`.
fn lift<S: Scalar>(p: &Poly<S>) -> Tensor<S> {
    Tensor::new(p.degree(), 0, p.coefs().iter().cloned()).expect("its degree")
}

/// Face `side`'s strip over part `kt` of `2^dt` of field cell `cell` in
/// `t` and `s ∈ [s_a, s_b]` of the strip (`s·h` exact, `h` dyadic): the
/// tile's patch over the strip's map, a patch in `(t, σ)`, with the
/// cell's spans over that part, at `like`'s level; `None` where it can't
/// be formed.
#[allow(clippy::too_many_arguments, reason = "one strip's coordinates")]
fn strip_patch<S: LevelScalar>(
    edge: &Edge<'_>,
    piece: &Piece,
    cell: TCell,
    frame: &Frame,
    side: usize,
    (kt, dt): (u64, u32),
    s: [f64; 2],
    like: &S,
) -> Option<(HomPatch<S>, Spans<S>)> {
    let (k, depth) = ((cell.k << dt) + kt, cell.depth + dt);
    let sp = spans_at(edge, piece, k, depth, like);
    let (ti, tj) = frame.tile[side];
    let tile = edge.sides[side].surface.tile(ti, tj)?;
    let p = &sp.p[side];
    let (pu, pv) = (moved(&p[0], ti, like), moved(&p[1], tj, like));
    let (h, nu) = (frame.h[side], frame.nu[side]);
    let [uu, du] = strip_coord(&pu, nu[0], h, s, like);
    let [vv, dv] = strip_coord(&pv, nu[1], h, s, like);
    let x = tile.patch_at(like).compose_map([&uu, &du], [&vv, &dv])?;
    Some((x, sp))
}

/// A certified upper bound of `|X(t, σ) − C(t)|` over a strip's patch
/// `x` and the curve's span `c` over the same `t` (the numerator
/// `H·W_C − X_C·H₃` over `H₃·W_C`): how far from the curve the patch's
/// points lie; `None` where it can't be formed or bounded.
fn reach_bound<S: LevelScalar>(x: &HomPatch<S>, c: &HomCurve<S, 3>) -> Option<f64> {
    let [hx, hy, hz, hw] = x.tensors();
    let wc = lift(c.weights());
    let xc = c.coords().each_ref().map(lift);
    let mut sum = 0.0;
    for (h, xk) in [hx, hy, hz].into_iter().zip(&xc) {
        let m = h.mul(&wc)?.sub(&xk.mul(hw)?).hull().abs_max();
        sum = up(sum + up(m * m));
    }
    let r = div_up(up(sum.sqrt()), hw.mul(&wc)?.hull().lo);
    r.is_finite().then_some(r)
}

impl SignFn for EdgeStrip<'_, '_> {
    fn forms<S: LevelScalar>(&self, b: &SBox, like: &S) -> Option<(Tensor<S>, Tensor<S>)> {
        let i = self.side;
        let (x, sp) = strip_patch(
            self.edge,
            self.piece,
            self.cell,
            self.frame,
            i,
            (b.kt, b.dt),
            b.s_range(),
            like,
        )?;
        let [hx, hy, hz, hw] = x.tensors();
        let g = if self.plane {
            let (n, p0) = self.frame.plane?;
            let a = n.map(|c| S::from_f64(c, like));
            let c = a
                .iter()
                .zip(p0)
                .fold(S::int(0, like), |acc, (a, p)| {
                    acc.add(&a.mul(&S::from_f64(p, like)))
                })
                .neg();
            x.compose_linear(&a, &c)
        } else {
            let m = self
                .frame
                .m
                .each_ref()
                .map(|c| halve_poly(poly_of(c, like), b.kt, b.dt));
            let q = cross(&m, &tangent(&sp.c)?)?;
            let wc = lift(sp.c.weights());
            let xc = sp.c.coords().each_ref().map(lift);
            let dd = [
                hx.mul(&wc)?.sub(&xc[0].mul(hw)?),
                hy.mul(&wc)?.sub(&xc[1].mul(hw)?),
                hz.mul(&wc)?.sub(&xc[2].mul(hw)?),
            ];
            dot(&dd, &q.each_ref().map(lift))?
        };
        let g = if i == 0 { g } else { g.neg() };
        Some((g, hw.clone()))
    }

    fn reach(&self, b: &SBox) -> f64 {
        let like = Bounded::exact(0.0);
        let s = b.s_range();
        strip_patch(
            self.edge,
            self.piece,
            self.cell,
            self.frame,
            self.side,
            (b.kt, b.dt),
            s,
            &like,
        )
        .and_then(|(x, sp)| reach_bound(&x, &sp.c))
        .unwrap_or(f64::INFINITY)
    }

    fn units(&self, level: u8) -> u64 {
        let row = if self.plane {
            if quadratic(self.edge, self.piece) {
                &UNITS.adjacent.plane
            } else {
                &UNITS.adjacent.plane_low
            }
        } else if quadratic(self.edge, self.piece) {
            &UNITS.adjacent.sign
        } else {
            &UNITS.adjacent.sign_low
        };
        row[usize::from(level)]
    }
}

/// A ready field cell to settle, and the window's threshold `δ`.
struct Settle<'a, 'e> {
    edge: &'a Edge<'e>,
    piece: &'a Piece,
    cell: TCell,
    frame: &'a Frame,
    delta: f64,
}

/// Settles both faces' strips over a ready field cell: the plane first
/// where the frame has one, else (or where it doesn't settle at once) the
/// ruled surface; `Some` failure ends the certificate. Where both hold,
/// the cell's part of the window: `ρ_min` (a crease's from its angle,
/// else the faces' separation, [`window::separation`]), `r₀` and the far
/// sides.
fn settle(
    at: &Settle<'_, '_>,
    held: &mut Held,
    ledger: &mut Ledger<'_>,
) -> Result<Option<EdgeCert>, Stop<Skip>> {
    let &Settle {
        edge,
        piece,
        cell,
        frame,
        delta,
    } = at;
    let strip = |side, plane| EdgeStrip {
        edge,
        piece,
        cell,
        frame,
        side,
        plane,
    };
    let mut out = [StripOut::Holds(0.0); 2];
    let mut fast = frame.plane.is_some();
    if fast {
        for (side, o) in out.iter_mut().enumerate() {
            match decide_strip(&strip(side, true), cell.depth, true, ledger)? {
                Some(x) => *o = x,
                None => {
                    fast = false;
                    break;
                }
            }
        }
    }
    if !fast {
        for (side, o) in out.iter_mut().enumerate() {
            *o = decide_strip(&strip(side, false), cell.depth, false, ledger)?
                .expect("decided in full");
        }
    }
    let crease = frame.sin2[0] > 0.0;
    if crease {
        let lo = window::crease_lo(delta, frame.sin2[1]);
        if lo > held.rho_min_lo {
            held.rho_min_lo = lo;
        }
    }
    let width = pow2(-(cell.depth as i32));
    for side in 0..2 {
        let h = frame.h[side];
        match out[side] {
            StripOut::Holds(s0) => {
                let s0 = s0 * h;
                if s0 > held.s0[side] {
                    held.s0[side] = s0;
                }
                if h < held.reach[side] {
                    held.reach[side] = h;
                }
            }
            StripOut::Fails(t, s, dist) => {
                let at = (cell.k as f64 + t) * width;
                return Ok(Some(EdgeCert::Fails {
                    side,
                    t: piece.a + (piece.b - piece.a) * at,
                    s: s * h,
                    dist,
                    rho_min_lo: held.rho_min_lo,
                }));
            }
        }
    }
    // The cell's part of the window, at the first level.
    let rho = if crease {
        window::crease_hi(delta, frame.sin2[0])
    } else {
        let mut r: f64 = 0.0;
        for side in 0..2 {
            let x = window::separation(at, side, ledger)?;
            if x > r || x.is_nan() {
                r = x;
            }
        }
        r
    };
    if rho > held.rho_min || rho.is_nan() {
        held.rho_min = rho;
    }
    let like = Bounded::exact(0.0);
    for side in 0..2 {
        let StripOut::Holds(s0) = out[side] else {
            unreachable!("both strips hold here");
        };
        let whole = (0, 0);
        let r0 = strip_patch(edge, piece, cell, frame, side, whole, [0.0, s0], &like)
            .and_then(|(x, sp)| reach_bound(&x, &sp.c))
            .unwrap_or(f64::INFINITY);
        if r0 > held.r0[side] || r0.is_nan() {
            held.r0[side] = r0;
        }
        match strip_patch(edge, piece, cell, frame, side, whole, [1.0, 1.0], &like) {
            Some((x, _)) => held.far[side].push(x.iso_line(Dir::V, 0.0)),
            // No wall: the window can't be certified on this strip.
            None => held.r0[side] = f64::INFINITY,
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------
// The vertex certificate.

/// A face's germ at a vertex, as the certificate reads it: the face's
/// surface and orientation and its corners there, each the span leaving
/// the vertex along the next coedge and the span arriving along the
/// coedge ending there, both run from the vertex.
pub(crate) struct Germ<'a> {
    /// The face's surface.
    pub(crate) surface: &'a Surface,
    /// Whether the face's outward normal is against its surface's.
    pub(crate) flipped: bool,
    /// The corners: the leaving span and the arriving span, each run
    /// away from the vertex (their first control values the vertex's UV).
    pub(crate) corners: Vec<[PSpan; 2]>,
}

/// The vertex certificate's outcome.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the vertex's ball runs it as it lands")
)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum VertCert {
    /// The plane through the vertex with normal `normal` separates the
    /// germs beyond `rho0` (per face, in global UV along the sectors'
    /// sides of largest coordinate 1): the first face on its negative
    /// side, the second on its positive.
    Held {
        /// The plane's normal.
        normal: [f64; 3],
        /// Per face, the join zone's extent.
        rho0: [f64; 2],
    },
    /// No plane was certified (none proposed, or the one proposed fails
    /// on a sector).
    Open,
}

/// A sector of a germ: the triangle `uv + h·ρ·((1 − β)·a + β·b)` in tile
/// `(i, j)`, `a` to `b` under a half turn.
#[derive(Clone, Copy, Debug)]
struct Sector {
    uv: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    h: f64,
    tile: (i64, i64),
}

/// The sign of `a × b` (2D), exactly.
fn turn(a: [f64; 2], b: [f64; 2]) -> Ordering {
    let q = |x: f64| Rational::try_from(x).unwrap_or_default();
    let v = q(a[0]) * q(b[1]) - q(a[1]) * q(b[0]);
    v.partial_cmp(&Rational::default())
        .unwrap_or(Ordering::Equal)
}

/// The directions from a span's start to its other control values: the
/// span leaves its start inside their cone (positive weights). A G or iso
/// span's coordinates have their own weights, so its cone is that of the
/// coordinates' steps apart.
fn cone_of(span: &PSpan) -> Vec<[f64; 2]> {
    let s = span.start();
    match span {
        PSpan::Q(p) => p[1..].iter().map(|q| [q[0] - s[0], q[1] - s[1]]).collect(),
        PSpan::Linear(p) => vec![[p[1][0] - s[0], p[1][1] - s[1]]],
        _ => {
            let [u, v] = span.coords();
            let mut out = Vec::new();
            for x in &u.values()[1..] {
                out.push([x[0] - s[0], 0.0]);
            }
            for x in &v.values()[1..] {
                out.push([0.0, x[0] - s[1]]);
            }
            out.retain(|d| d != &[0.0, 0.0]);
            out
        }
    }
}

/// The cone's extreme directions `[clockwise-most, counter-clockwise-most]`,
/// exactly; `None` for an empty cone or one of a half turn or more.
fn extremes(dirs: &[[f64; 2]]) -> Option<[[f64; 2]; 2]> {
    let first = *dirs.first()?;
    let (mut cw, mut ccw) = (first, first);
    for &d in &dirs[1..] {
        if turn(d, cw) == Ordering::Greater {
            cw = d;
        }
        if turn(ccw, d) == Ordering::Greater {
            ccw = d;
        }
    }
    // Every direction within `[cw, ccw]`, which is under a half turn.
    let ok = turn(cw, ccw) != Ordering::Less
        && dirs
            .iter()
            .all(|&d| turn(cw, d) != Ordering::Less && turn(d, ccw) != Ordering::Less);
    ok.then_some([cw, ccw])
}

/// `d` turned a quarter counter-clockwise.
fn quarter(d: [f64; 2]) -> [f64; 2] {
    [-d[1], d[0]]
}

/// The sweep counter-clockwise from `a` to `b`, cut into parts under a
/// half turn (at quarter turns from `a`) and at the coordinate axes'
/// directions where `cut_axes` (the vertex on a tile line in that
/// coordinate); `None` for a full turn or more.
fn sweep(a: [f64; 2], b: [f64; 2], cut_axes: [bool; 2]) -> Option<Vec<[[f64; 2]; 2]>> {
    // Whether `x` points the way `y` does.
    let same = |x: [f64; 2], y: [f64; 2]| {
        turn(x, y) == Ordering::Equal && (x[0] * y[0] > 0.0 || x[1] * y[1] > 0.0)
    };
    if same(a, b) {
        return None;
    }
    let mut parts: Vec<[[f64; 2]; 2]> = Vec::new();
    let mut from = a;
    loop {
        if turn(from, b) == Ordering::Greater {
            parts.push([from, b]);
            break;
        }
        if parts.len() == 3 {
            return None;
        }
        let q = quarter(from);
        if same(q, b) {
            parts.push([from, b]);
            break;
        }
        parts.push([from, q]);
        from = q;
    }
    // Cut at the axes' directions inside a part.
    let axes: Vec<[f64; 2]> = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]]
        .into_iter()
        .filter(|d| {
            if d[0] != 0.0 {
                cut_axes[0]
            } else {
                cut_axes[1]
            }
        })
        .collect();
    let mut out = Vec::new();
    for [x, y] in parts {
        let mut from = x;
        let mut inside: Vec<[f64; 2]> = axes
            .iter()
            .copied()
            .filter(|&d| turn(x, d) == Ordering::Greater && turn(d, y) == Ordering::Greater)
            .collect();
        // In counter-clockwise order from `x`.
        inside.sort_by(|&p, &q| turn(q, p));
        for d in inside {
            out.push([from, d]);
            from = d;
        }
        out.push([from, y]);
    }
    Some(out)
}

/// The sectors of a germ's corners (module docs), each cut to its tile
/// and a width at most `width`; `None` where a corner has none (a cone of
/// a half turn or more, a full sweep, no tile).
fn sectors(germ: &Germ<'_>, width: f64) -> Option<Vec<Sector>> {
    let mut out = Vec::new();
    for [leave, arrive] in &germ.corners {
        let uv = leave.start();
        let [lcw, lccw] = extremes(&cone_of(leave))?;
        let [acw, accw] = extremes(&cone_of(arrive))?;
        // Counter-clockwise through the face from the leaving cone's
        // clockwise side to the arriving cone's counter-clockwise side,
        // on a face left of its coedges; the mirror on a flipped one.
        let (a, b) = if germ.flipped {
            (acw, lccw)
        } else {
            (lcw, accw)
        };
        let on_line = uv.map(|x| x.fract() == 0.0);
        for [x, y] in sweep(a, b, on_line)? {
            let tile = sector_tile(germ.surface, uv, x, y)?;
            let h = sector_width(uv, x, y, tile, width);
            if !positive(h) {
                return None;
            }
            out.push(Sector {
                uv,
                a: x,
                b: y,
                h,
                tile,
            });
        }
    }
    Some(out)
}

/// The tile holding a sector at `uv` between `a` and `b` (no coordinate
/// changing sign between them where `uv` is on a tile line).
fn sector_tile(surface: &Surface, uv: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Option<(i64, i64)> {
    let rect = surface.rect();
    let mut idx = [0i64; 2];
    for k in 0..2 {
        if uv[k].abs() >= 4_503_599_627_370_496.0 || uv[k].is_nan() {
            return None;
        }
        let c = uv[k].floor() as i64;
        let i = if uv[k].fract() == 0.0 && (a[k] < 0.0 || b[k] < 0.0) {
            c - 1
        } else {
            c
        };
        let r = if k == 0 { &rect.u } else { &rect.v };
        if !r.contains(&i) {
            return None;
        }
        idx[k] = i;
    }
    Some((idx[0], idx[1]))
}

/// The largest power of two at most `width` keeping the sector's triangle
/// in its tile; 0 where none does.
fn sector_width(uv: [f64; 2], a: [f64; 2], b: [f64; 2], tile: (i64, i64), width: f64) -> f64 {
    let mut h = width;
    for k in 0..2 {
        let i = if k == 0 { tile.0 } else { tile.1 } as f64;
        for d in [a[k], b[k]] {
            let room = if d > 0.0 {
                down(down((i + 1.0) - uv[k]) / up(d))
            } else if d < 0.0 {
                down(down(uv[k] - i) / up(-d))
            } else {
                continue;
            };
            if room < h {
                h = room;
            }
        }
    }
    dyadic_floor(h)
}

/// A sector's sign: `σ·ℓ·(X − v)` over the triangle, `β` first, `ρ`
/// second.
struct SectorSign<'a> {
    tile: &'a Tile,
    sector: Sector,
    normal: [f64; 3],
    v: [f64; 3],
    negate: bool,
}

impl SignFn for SectorSign<'_> {
    fn forms<S: LevelScalar>(&self, b: &SBox, like: &S) -> Option<(Tensor<S>, Tensor<S>)> {
        let sc = &self.sector;
        let [ba, bb] = b.t_range();
        let [ra, rb] = b.s_range();
        // The corners of the box's part of the triangle, `(β, ρ)`
        // row-major in `β`: `uv − tile + h·ρ·((1 − β)·a + β·b)`.
        let coord = |k: usize, i: f64| {
            let o = S::from_f64(sc.uv[k], like).sub(&S::from_f64(i, like));
            let at = |beta: f64, rho: f64| -> S {
                // `h·ρ` and `β` are dyadic; the direction's blend rounds
                // once at `like`'s level.
                let dir = S::from_f64(sc.a[k], like)
                    .mul(&S::from_f64(1.0 - beta, like))
                    .add(&S::from_f64(sc.b[k], like).mul(&S::from_f64(beta, like)));
                o.add(&S::from_f64(sc.h * rho, like).mul(&dir))
            };
            Tensor::new(1, 1, [at(ba, ra), at(ba, rb), at(bb, ra), at(bb, rb)]).expect("bilinear")
        };
        let (ti, tj) = sc.tile;
        let (u, v) = (coord(0, ti as f64), coord(1, tj as f64));
        let one = Tensor::new(0, 0, [S::int(1, like)]).expect("a constant");
        let x = self
            .tile
            .patch_at(like)
            .compose_map([&u, &one], [&v, &one])?;
        let a = self.normal.map(|c| S::from_f64(c, like));
        let c = a
            .iter()
            .zip(self.v)
            .fold(S::int(0, like), |acc, (a, p)| {
                acc.add(&a.mul(&S::from_f64(p, like)))
            })
            .neg();
        let g = x.compose_linear(&a, &c);
        let g = if self.negate { g.neg() } else { g };
        Some((g, x.weights().clone()))
    }

    fn units(&self, level: u8) -> u64 {
        UNITS.adjacent.sector[usize::from(level)]
    }
}

/// The vertex certificate of two germs at the vertex `v` (module docs),
/// each face's sectors cut to the width `radius[i]` in its global UV,
/// each box charged to `ledger`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the vertex's ball runs it as it lands")
)]
pub(crate) fn certify_vertex(
    v: [f64; 3],
    germs: [&Germ<'_>; 2],
    radius: [f64; 2],
    ledger: &mut Ledger<'_>,
) -> Result<VertCert, Stop<Skip>> {
    let mut secs = Vec::with_capacity(2);
    for (g, &r) in germs.iter().zip(&radius) {
        match sectors(g, r) {
            Some(s) => secs.push(s),
            None => return Ok(VertCert::Open),
        }
    }
    // The proposal: the sectors' far points, the first face's negated, in
    // one cone away from the origin.
    let mut pts = Vec::new();
    for (i, (g, ss)) in germs.iter().zip(&secs).enumerate() {
        for s in ss {
            for beta in [0.0, 0.5, 1.0] {
                let uv = [0, 1].map(|k| s.uv[k] + s.h * ((1.0 - beta) * s.a[k] + beta * s.b[k]));
                let Some(x) = g.surface.eval(uv) else {
                    return Ok(VertCert::Open);
                };
                let d = [0, 1, 2].map(|k| x[k].v - v[k]);
                pts.push(if i == 0 { d.map(|x| -x) } else { d });
            }
        }
    }
    let Some(normal) = propose_axis(&[[0.0; 3]], &pts) else {
        return Ok(VertCert::Open);
    };
    let mut rho0 = [0.0; 2];
    for (i, (g, ss)) in germs.iter().zip(&secs).enumerate() {
        for s in ss {
            let Some(tile) = g.surface.tile(s.tile.0, s.tile.1) else {
                return Ok(VertCert::Open);
            };
            let f = SectorSign {
                tile,
                sector: *s,
                normal,
                v,
                negate: i == 0,
            };
            match decide_strip(&f, 0, false, ledger)? {
                Some(StripOut::Holds(r)) => {
                    if r * s.h > rho0[i] {
                        rho0[i] = r * s.h;
                    }
                }
                _ => return Ok(VertCert::Open),
            }
        }
    }
    Ok(VertCert::Held { normal, rho0 })
}

/// Face `f`'s germ at vertex `v` of `body`: its corners there, read off
/// its loops; `None` where an id or a span can't be read.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the vertex's ball reads the germs as it lands")
)]
pub(crate) fn germ_of(body: &Brep, f: FaceId, v: VertId) -> Option<Germ<'_>> {
    let surface = body.face_surface(f);
    let mut corners = Vec::new();
    for l in body.loops(f) {
        for c in body.loop_coedges(l) {
            if body.coedge_vert(c, true) != v {
                continue;
            }
            let n = body.next(c);
            let leave = end_span(body.coedge_pcurve(n), body.reversed(n), false)?;
            let arrive = end_span(body.coedge_pcurve(c), body.reversed(c), true)?;
            corners.push([leave, arrive]);
        }
    }
    Some(Germ {
        surface,
        flipped: body.flipped(f),
        corners,
    })
}

/// The span of `p` at its coedge's start (`end` false) or end, run away
/// from that point.
fn end_span(p: &PCurve, reversed: bool, end: bool) -> Option<PSpan> {
    let spans = p.spans();
    // The coedge's start is the curve's start unless reversed.
    let at_curve_end = reversed != end;
    Some(if at_curve_end {
        spans.last()?.reversed()
    } else {
        *spans.first()?
    })
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times the certificates'
/// cells and boxes per precision level against the units charged for
/// them. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;
    use crate::geom::form::{CylinderKey, PlaneKey};
    use crate::geom::forms::cyl;
    use crate::geom::lattice::{CELL, Lattice};

    /// An edge along a quarter circle of radius 50 on the plane `z = 0`
    /// (quadratic spans on both faces, a conic curve), a straight edge on
    /// the plane, a ruling of a cylinder of radius 50 (iso spans on a
    /// curved surface), a bend across the cylinder (quadratic spans on a
    /// curved surface: sign functions of degrees (28, 4)), and a corner's
    /// germ on the plane.
    pub struct Fixture {
        surface: Surface,
        curve: Curve3D,
        pcurve: PCurve,
        line: Curve3D,
        on_line: PCurve,
        cylinder: Surface,
        ruling: Curve3D,
        on_ruling: PCurve,
        bend: Curve3D,
        on_bend: PCurve,
        corner: [PSpan; 2],
    }

    /// Which of the fixture's edges.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Which {
        /// The quarter arc on the plane.
        Arc,
        /// The ruling on the cylinder.
        Ruling,
        /// The straight edge on the plane.
        Line,
        /// The bend across the cylinder.
        Bend,
    }

    /// The fixture.
    pub fn fixture() -> Fixture {
        let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).expect("a plane");
        let lattice = Lattice::new(key.into()).expect("a lattice");
        let rect = lattice.half(0).expect("its tile");
        let surface = Surface::new(lattice, rect).expect("a surface");
        let lat = surface.lattice();
        let (o, [u, v, _]) = (lat.origin(), lat.axes());
        let uv = |x: [f64; 3]| {
            let d = [x[0] - o[0], x[1] - o[1], x[2] - o[2]];
            [u, v].map(|a| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5)
        };
        let (r, mid) = (50.0, std::f64::consts::FRAC_1_SQRT_2);
        let curve = Curve3D::new(
            vec![[[r, 0.0, 0.0, 1.0], [r, r, 0.0, mid], [0.0, r, 0.0, 1.0]]],
            vec![0.0, 1.0],
        )
        .expect("an arc");
        let span = curve.spans()[0].map(|p| {
            let q = uv([p[0], p[1], p[2]]);
            [q[0], q[1], p[3]]
        });
        let pcurve = PCurve::new(vec![span], vec![0.0, 1.0]).expect("a Q span");
        let (a, b) = ([r, 0.0, 0.0], [0.0, r, 0.0]);
        let line = Curve3D::new(
            vec![[
                [r, 0.0, 0.0, 1.0],
                [r * 0.5, r * 0.5, 0.0, 1.0],
                [0.0, r, 0.0, 1.0],
            ]],
            vec![0.0, 1.0],
        )
        .expect("a line");
        let (ua, ub) = (uv(a), uv(b));
        let on_line = PCurve::from_spans(
            vec![PSpan::Linear([[ua[0], ua[1], 1.0], [ub[0], ub[1], 1.0]])],
            vec![0.0, 1.0],
        )
        .expect("a UV line");
        let key = CylinderKey::new([0.0; 3], [0.0, 0.0, 1.0], r).expect("a cylinder");
        let cylinder = cyl::cylinder(key, 0).expect("a cylinder's half");
        let (u0, v0, v1) = (0.5, 0.5, 0.5 + 64.0 / CELL);
        let at = |v: f64| {
            let p = cylinder.eval([u0, v]).expect("on the cylinder");
            [p[0].v, p[1].v, p[2].v, 1.0]
        };
        let (p0, p1) = (at(v0), at(v1));
        let m = [0, 1, 2, 3].map(|k| (p0[k] + p1[k]) * 0.5);
        let ruling = Curve3D::new(vec![[p0, m, p1]], vec![0.0, 1.0]).expect("a ruling");
        let on_ruling = PCurve::from_spans(
            vec![PSpan::Iso {
                fixed: Dir::U,
                at: u0,
                m: [[v0, 1.0], [v1, 1.0]],
            }],
            vec![0.0, 1.0],
        )
        .expect("an iso span");
        // A parabola in the cylinder's UV, its curve through the points
        // of its control values (near the surface, as far as timing goes).
        let q = [
            [0.25, v0, 1.0],
            [0.5, v0 + 32.0 / CELL, 1.0],
            [0.75, v0, 1.0],
        ];
        let on_bend = PCurve::new(vec![q], vec![0.0, 1.0]).expect("a Q span");
        let p = |k: usize| {
            let x = cylinder.eval([q[k][0], q[k][1]]).expect("on the cylinder");
            [x[0].v, x[1].v, x[2].v, 1.0]
        };
        let bend = Curve3D::new(vec![[p(0), p(1), p(2)]], vec![0.0, 1.0]).expect("a bend");
        let at = uv([0.0; 3]);
        let ray = |d: [f64; 2]| {
            PSpan::Linear([
                [at[0], at[1], 1.0],
                [at[0] + d[0] * 1e-6, at[1] + d[1] * 1e-6, 1.0],
            ])
        };
        Fixture {
            surface,
            curve,
            pcurve,
            line,
            on_line,
            cylinder,
            ruling,
            on_ruling,
            bend,
            on_bend,
            corner: [ray([1.0, 0.0]), ray([0.0, 1.0])],
        }
    }

    impl Fixture {
        /// The edge `which`.
        fn edge(&self, which: Which) -> Edge<'_> {
            let (surface, curve, pcurve) = match which {
                Which::Arc => (&self.surface, &self.curve, &self.pcurve),
                Which::Line => (&self.surface, &self.line, &self.on_line),
                Which::Ruling => (&self.cylinder, &self.ruling, &self.on_ruling),
                Which::Bend => (&self.cylinder, &self.bend, &self.on_bend),
            };
            let side = |reversed| Side {
                surface,
                pcurve,
                flipped: false,
                reversed,
                seam: 0.0,
            };
            Edge {
                curve,
                t_range: [0.0, 1.0],
                sides: [side(false), side(true)],
                one_surface: false,
            }
        }
    }

    const WIDTH: f64 = 1.0 / 1_048_576.0;

    /// Evaluates every field cell `depth` halvings deep at precision
    /// level `level`, as the certificate does, on the edge `which`; the
    /// number of cells.
    pub fn field_cells(f: &Fixture, level: u8, depth: u32, which: Which) -> u64 {
        let edge = f.edge(which);
        let pieces = edge.pieces().expect("pieces");
        let mut n = 0;
        for k in 0..1u64 << depth {
            let mut at = FieldAt {
                edge: &edge,
                piece: &pieces[0],
                cell: TCell { k, depth, level },
                width: [WIDTH; 2],
            };
            std::hint::black_box(matches!(run_at(&mut at, level), FieldOut::Ready(_)));
            n += 1;
        }
        n
    }

    /// Evaluates every box `depth` halvings deep in `t` of both faces'
    /// strips at precision level `level`, against the ruled surface or
    /// (`plane`) the curve's plane, on the edge `which`; the number of
    /// boxes.
    pub fn sign_boxes(f: &Fixture, level: u8, depth: u32, plane: bool, which: Which) -> u64 {
        let edge = f.edge(which);
        let pieces = edge.pieces().expect("pieces");
        let cell = TCell {
            k: 0,
            depth: 0,
            level: 0,
        };
        let mut at = FieldAt {
            edge: &edge,
            piece: &pieces[0],
            cell,
            width: [WIDTH; 2],
        };
        let FieldOut::Ready(frame) = run_at(&mut at, 0) else {
            panic!("the fixture's field holds");
        };
        let mut n = 0;
        for side in 0..2 {
            let s = EdgeStrip {
                edge: &edge,
                piece: &pieces[0],
                cell,
                frame: &frame,
                side,
                plane,
            };
            for kt in 0..1u64 << depth {
                let b = SBox {
                    kt,
                    dt: depth,
                    ks: 0,
                    ds: 0,
                    level,
                };
                std::hint::black_box(run_at(&mut BoxAt { f: &s, b }, level));
                n += 1;
            }
        }
        n
    }

    /// Evaluates every box `depth` halvings deep in `β` of the corner's
    /// sector at precision level `level`; the number of boxes.
    pub fn sector_boxes(f: &Fixture, level: u8, depth: u32) -> u64 {
        let germ = Germ {
            surface: &f.surface,
            flipped: false,
            corners: vec![f.corner],
        };
        let sec = sectors(&germ, WIDTH).expect("a sector");
        let tile = f
            .surface
            .tile(sec[0].tile.0, sec[0].tile.1)
            .expect("its tile");
        let s = SectorSign {
            tile,
            sector: sec[0],
            normal: [-1.0, -1.0, 0.5],
            v: [0.0; 3],
            negate: false,
        };
        for kt in 0..1u64 << depth {
            let b = SBox {
                kt,
                dt: depth,
                ks: 0,
                ds: 0,
                level,
            };
            std::hint::black_box(run_at(&mut BoxAt { f: &s, b }, level));
        }
        1 << depth
    }
}

pub(crate) mod window;

#[cfg(test)]
mod tests;

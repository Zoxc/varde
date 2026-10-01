//! Union, difference and intersection of solids, built the way
//! [Manifold](https://github.com/elalish/manifold) builds them for flat
//! triangles (`boolean3.cpp`, `boolean_result.cpp`): every topological
//! fact comes from a few primitives, each worked out once and stored by
//! the pair it is about, through counting identities that hold whatever
//! the primitives' values. So the result is always a closed manifold, and
//! nothing is ever merged because two points are close.
//!
//! 1. **Broad phase**: pairs of triangles whose boxes meet, through the
//!    BVH, and from them the candidate edge–face pairs.
//! 2. **Primitives** ([`Primitives`]), along one fixed projection
//!    direction [`UP`], tilted off every axis:
//!    - `s02(v, f)`: the signed number of layers of face `f` above vertex
//!      `v` (+1 facing up, −1 facing down; 0 or ±1 for a flat triangle).
//!    - `s11(e, g)`: whether edges `e` of `A` and `g` of `B` cross seen
//!      along `UP`, which way, and which is above at the crossing.
//! 3. **Counting**: an edge `e` from `a` to `b` crosses a face `f`
//!    `x12(e, f) = s02(b, f) − s02(a, f) − Σ s11(e, h)` times, summed over
//!    `f`'s edges `h` that pass over `e`; a vertex's winding number in the
//!    other solid is the sum of its `s02`.
//! 4. **Assembly**: new vertices are records ("edge `e` through face
//!    `f`"); edges are cut at them in the order along the edge; each face
//!    pair's cut runs along a chain of shared edges between its ends; the
//!    parts are kept by winding number for the operation; each cut face's
//!    kept part is triangulated in its parameter domain (or its plane),
//!    and the halfedges pair up by vertex id.
//! 5. **Clean-up, merge, repair and check**: the degenerate triangles
//!    flush operands leave are removed, pieces refinement split and
//!    nothing cut are merged back, and the result is repaired and
//!    checked.
//!
//! The primitives come in two kinds. For **flat patches** (every edge
//! straight within the resolution) they are exact, with symbolic
//! perturbation breaking ties, near ties within a tie distance included
//! (see [`flat`]), and the new edges are straight. When an operand has a **curved patch** they are numerical
//! solves (see [`curved`]), each worked out once and shared, and exact
//! wherever the pieces they are about are straight or flat. Curved pairs
//! of faces then need their own decisions: whether a closed loop may
//! hide in a pair of patches that no edge crossing shows, and which of
//! several ends join up. Pairs that can't be decided from their ends and
//! a normal-cone certificate are refined, both operands split exactly
//! (red–green) and counted again, down to a size floor where fixed rules
//! decide (see [`pairs`]).
//!
//! Then each decided arc gets its geometry (see [`chain`]): two planes
//! meet in a line and a plane cuts a quadric in a conic, both exact (the
//! faces' tags say which, see [`surface`]); anything else is traced where
//! the patches meet and fitted with conics within the fit tolerance. Cut
//! curved faces keep their own surface wherever they can: their new inner
//! edges are the patch's own curves, and along a cut of a quadric they are
//! chosen so that the triangles there lie on it exactly too (see
//! [`assemble`]). See `agents/kernel.md`.

use std::cmp::Ordering;

use glam::DVec3;

use crate::budget::{Budget, Work};
use crate::mesh::{BuildError, Face, Mesh, MeshBuilder};
use crate::{KernelError, Solid, Tolerance};

mod assemble;
mod chain;
mod cleanup;
mod count;
mod curved;
pub(crate) mod exact;
mod flat;
mod input;
mod pairs;
mod surface;
mod triangulate;

use count::Crossing;
use input::{Input, Side};

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    /// Everything in either.
    Union,
    /// Everything in the first and not the second.
    Difference,
    /// Everything in both.
    Intersection,
}

/// Why a boolean gives no solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanError {
    /// The decisions don't fit together: near ties decided as ties that
    /// no one configuration has, rarely, with curved operands (flat ones
    /// are then decided again exactly).
    Inconsistent,
    /// A cut face's kept part couldn't be triangulated, or the triangles
    /// don't pair up into a closed surface.
    Degenerate,
}

impl std::fmt::Display for BooleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BooleanError::Inconsistent => "where the solids cross doesn't add up",
            BooleanError::Degenerate => "a cut face couldn't be triangulated",
        })
    }
}

impl std::error::Error for BooleanError {}

/// How near two things must be for the primitives to decide them as a
/// tie, by the perturbation: a 64th of the resolution. Exact ties (flush
/// faces, a vertex on a face) and those rounding leaves (the same faces
/// turned and moved, every coordinate rounded) then decide alike.
fn tie(tol: &Tolerance) -> f64 {
    tol.resolution() / 64.0
}

/// The clean-up's short length, an eighth of the resolution: shorter
/// straight edges collapse, and nearer ends make inner edges twins of
/// boundary ones (`assemble::face::cut_face`), in step with it.
fn short(tol: &Tolerance) -> f64 {
    tol.resolution() / 8.0
}

/// Units of work per patch that checking the result takes (about 2.7 µs
/// a patch on one thread, the units about half a microsecond), not
/// counting the patches it integrates to tell which way the shells face
/// ([`Solid::new_within`] charges those).
const CHECK_WORK: usize = 5;

/// The projection direction every primitive shares: nearly `+z`, tilted
/// off every axis so that walls along the axes, which CAD models are full
/// of, don't all project to lines. Its coordinates are small integers, so
/// the exact predicates take it as it is.
const UP: DVec3 = DVec3::new(2.0, 3.0, 32.0);

/// How the shadows of edge `e` of `A` and edge `g` of `B` cross, seen
/// along [`UP`], each edge in its own direction, as the counting reads
/// them: `a_under` sums, over the crossings where `e` passes under `g`,
/// +1 where `e` crosses `g` from its right to its left (the sign of
/// `det[g, e, UP]`) and −1 the other way; `b_under` sums the same over
/// the crossings where `g` passes under `e`, for `g` crossing `e`. Two
/// flat edges cross once at most, so one of them is ±1 or both are 0;
/// projected conics can cross several times, and their sums are what
/// counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cross11 {
    a_under: i8,
    b_under: i8,
}

/// The primitives the counting reads. Each is asked once per pair, and
/// its answer stored and shared by everything that needs it.
trait Primitives: Sync {
    /// `s02`: the signed number of layers of face `f` of the other operand
    /// above vertex `v` of `side`, along [`UP`].
    fn s02(&self, side: Side, v: u32, f: u32) -> i8;
    /// `s11`: how the shadows of edge `e` of `A` and edge `g` of `B`
    /// cross.
    fn s11(&self, e: u32, g: u32) -> Cross11;
    /// Whether edge `e` of `side` may pass through face `f` of the other
    /// and back again, which the count, `0`, doesn't show: then
    /// [`Self::crossings`] is asked for the pair with a count of `0` too.
    fn searches(&self, side: Side, e: u32, f: u32) -> bool;
    /// About how much work [`Self::crossings`] is at least, in units of
    /// the budget: what is spent before it is asked.
    fn search_work(&self) -> usize;
    /// How near two triangles' boxes must come for the broad phase to
    /// pair them: 0 for exact primitives; for numerical ones, as far as
    /// what they decide as a tie or take as flat reaches, so every pair a
    /// tie decides is counted too (a vertex a hair from a face, decided
    /// as on it, whose edges nothing then paired with that face, put a
    /// whole operand inside the other).
    fn margin(&self) -> f64;
    /// The crossings of edge `e` of `side` through face `f` of the other,
    /// whose signed number is `x` (+1 entering the other solid, −1
    /// leaving): each crossing's sign, where along the edge it is (0 at
    /// its start, 1 at its end), in order along the edge, and whether it
    /// was solved (a place where the two meet) or only placed for the
    /// count, which the result checks. Their signs add up to `x`,
    /// whatever the search for them finds: the count wins, the search
    /// only gives the positions. Also how much work finding
    /// them took, in units of the budget (at least
    /// [`Self::search_work`]). Fails with [`BooleanError::Inconsistent`]
    /// where no such crossings can be.
    fn crossings(&self, side: Side, e: u32, f: u32, x: i32) -> Result<Found, BooleanError>;
    /// The order along edge `e` of `side`, in its direction, of two of
    /// its crossings.
    fn order(&self, side: Side, e: u32, c1: &Crossing, c2: &Crossing) -> Ordering;
}

/// What [`Primitives::crossings`] gives: each crossing's sign, position
/// and whether it was solved, and the work it took.
type Found = (Vec<(i8, f64, bool)>, usize);

/// `a op b`, within `budget`: a solid that passes `check`, always.
///
/// The operands face out and nest properly, as every [`Solid`] does
/// (`check` makes sure of it), so the operation doesn't test them.
///
/// Fails with [`KernelError::Boolean`] for decisions that don't fit
/// together, or a cut face that can't be triangulated (see
/// [`BooleanError`]); with
/// [`KernelError::TooComplex`] past the budget; and with
/// [`KernelError::Invalid`] when the result can't pass `check` with `tol`,
/// such as two solids touching along an edge or at a point, where the
/// exact result isn't a manifold, or parts closer than the resolution.
pub fn boolean(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, KernelError> {
    let mut work = Work::new(budget);
    match (a.is_empty(), b.is_empty(), op) {
        (true, _, Op::Union) => return Ok(b.clone()),
        (_, true, Op::Union | Op::Difference) => return Ok(a.clone()),
        (true, _, _) | (_, true, _) => return Ok(Solid::empty()),
        _ => {}
    }
    let mesh = unchecked(a, b, op, tol, &mut work)?;
    let mesh = mesh.repair_within(tol, &mut work)?;
    // The check that makes it a solid, a few units a patch, and the
    // patches it integrated, charged once it has told how many.
    work.spend(mesh.tris().len().saturating_mul(CHECK_WORK))?;
    Solid::new_within(mesh, tol, &mut work)
}

/// The result's mesh, before repair and the check.
fn unchecked(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Mesh, KernelError> {
    let (ia, ib) = (Input::new(a.mesh(), tol), Input::new(b.mesh(), tol));
    let grow = op == Op::Union;
    let (mut soup, faces) = if ia.curved || ib.curved {
        // Counted and decided pair by pair, the operands refined where a
        // pair needs it.
        let refined = pairs::refined(a.mesh(), b.mesh(), grow, tol, work)?;
        let (ra, rb) = (Input::new(&refined.a, tol), Input::new(&refined.b, tol));
        let prims = curved::Curved::new(&ra, &rb, grow, tol);
        let refinement = assemble::Refinement {
            tree: [&refined.tree[0], &refined.tree[1]],
            leaf: [&refined.leaf[0], &refined.leaf[1]],
        };
        assemble::assemble(
            op,
            &ra,
            &rb,
            &refined.counts,
            &refined.arcs,
            &prims,
            tol,
            Some(&refinement),
            work,
        )?
    } else {
        flat_soup(op, &ia, &ib, tie(tol), tol, work)?
    };
    let mut faces = faces;
    cleanup::clean(
        &mut soup,
        &mut faces,
        short(tol),
        4.0 * tol.resolution(),
        tol,
        work,
    )?;
    build(soup, faces)
}

/// Flat operands' pieces, decided with near ties within `tie` taken as
/// ties ([`flat::Flat::tied`]), and where those decisions don't fit
/// together, again exactly.
///
/// Near ties taken as ties can (rarely) give decisions no one
/// configuration has, which the counting and the assembly catch as
/// [`BooleanError::Inconsistent`]. Exact decisions (`tie` 0) are those
/// of the perturbed operands, a real configuration, so they fit
/// together; at worst the result fails `check`. The second try spends
/// from the same `work`, and only on that failure. Only flat operands:
/// the `Flat` inside the curved primitives keeps their ties, which the
/// numerical primitives share.
fn flat_soup(
    op: Op,
    ia: &Input,
    ib: &Input,
    tie: f64,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(cleanup::Soup, Vec<Face>), KernelError> {
    match flat_decided(op, ia, ib, tie, tol, work) {
        Err(KernelError::Boolean(BooleanError::Inconsistent)) if tie > 0.0 => {
            flat_decided(op, ia, ib, 0.0, tol, work)
        }
        soup => soup,
    }
}

/// [`flat_soup`]'s one try, near ties within `tie` taken as ties (0:
/// exactly).
fn flat_decided(
    op: Op,
    ia: &Input,
    ib: &Input,
    tie: f64,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(cleanup::Soup, Vec<Face>), KernelError> {
    let prims = flat::Flat::tied(ia, ib, op == Op::Union, tie);
    let counts = count::count(ia, ib, &prims, tol, work)?;
    let arcs = pairs::flat(ia, ib, &counts)?;
    assemble::assemble(op, ia, ib, &counts, &arcs, &prims, tol, None, work)
}

/// Whether `a` and `b` touch or overlap: whether an edge of one crosses a
/// face of the other or a vertex of one is inside the other, with solids
/// that only touch (flush faces, an edge on a face) counted as touching.
/// Only the broad phase and the counting run (with curved patches, also
/// the refinement that finds loops no edge crossing shows), and not even
/// those for solids whose boxes (the control points', which hold them)
/// are more than the resolution apart.
pub fn touches(
    a: &Solid,
    b: &Solid,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<bool, KernelError> {
    let (Some(ba), Some(bb)) = (a.bounds3(), b.bounds3()) else {
        return Ok(false);
    };
    let gap = tol.resolution();
    if (ba.min - bb.max).max_element() > gap || (bb.min - ba.max).max_element() > gap {
        return Ok(false);
    }
    let mut work = Work::new(budget);
    let (ia, ib) = (Input::new(a.mesh(), tol), Input::new(b.mesh(), tol));
    if ia.curved || ib.curved {
        let refined = pairs::refined(a.mesh(), b.mesh(), true, tol, &mut work)?;
        return Ok(refined.counts.meet());
    }
    // Counted again exactly where the near ties don't fit together, as
    // the operation does ([`flat_soup`]).
    let counted = |tie: f64, work: &mut Work| {
        let prims = flat::Flat::tied(&ia, &ib, true, tie);
        count::count(&ia, &ib, &prims, tol, work)
    };
    let counts = match counted(tie(tol), &mut work) {
        Err(KernelError::Boolean(BooleanError::Inconsistent)) => counted(0.0, &mut work)?,
        counts => counts?,
    };
    Ok(counts.meet())
}

/// The mesh of the cleaned triangles: the vertices and faces no triangle
/// uses dropped (so chained booleans don't pile up faces long gone), the
/// rest numbered in order, halfedges paired by vertex id. No triangles
/// give the empty mesh.
fn build(soup: cleanup::Soup, faces: Vec<Face>) -> Result<Mesh, KernelError> {
    if soup.tris.is_empty() {
        return Ok(Mesh::default());
    }
    let mut used = vec![false; soup.pos.len()];
    let mut used_faces = vec![false; faces.len()];
    for (tri, &face) in soup.tris.iter().zip(&soup.faces) {
        for &v in tri {
            used[v as usize] = true;
        }
        used_faces[face as usize] = true;
    }
    let mut builder = MeshBuilder::new();
    let id: Vec<u32> = used
        .iter()
        .zip(&soup.pos)
        .map(|(&used, &p)| if used { builder.vert(p) } else { u32::MAX })
        .collect();
    let face_id: Vec<u32> = used_faces
        .iter()
        .zip(faces)
        .map(|(&used, f)| if used { builder.face(f) } else { u32::MAX })
        .collect();
    for (tri, face) in soup.tris.iter().zip(soup.faces) {
        builder.tri(tri.map(|v| id[v as usize]), face_id[face as usize]);
        // The curves of its sides that have one.
        for i in 0..3 {
            let (u, v) = (tri[i], tri[(i + 1) % 3]);
            if let Some(edge) = soup.curves.get(&(u.min(v), u.max(v))) {
                builder.edge(id[u as usize], id[v as usize], edge.ctrl, edge.weight);
            }
        }
    }
    builder.build().map_err(|e| match e {
        BuildError::TooManyPatches(_) => KernelError::TooComplex,
        _ => KernelError::Boolean(BooleanError::Degenerate),
    })
}

/// The straight segment from `p0` to `p1` as a curve: the control point
/// at the midpoint and weight 1.
fn segment(p0: DVec3, p1: DVec3) -> crate::patch::Conic3 {
    crate::patch::Conic3 {
        p0,
        c: (p0 + p1) * 0.5,
        w: 1.0,
        p1,
    }
}

/// Each vertex's connected part, as its lowest vertex id, of `n`
/// vertices joined by `links`.
fn parts(n: usize, links: impl IntoIterator<Item = [u32; 2]>) -> Vec<u32> {
    fn root(part: &mut [u32], mut v: u32) -> u32 {
        while part[v as usize] != v {
            part[v as usize] = part[part[v as usize] as usize];
            v = part[v as usize];
        }
        v
    }
    let mut part: Vec<u32> = (0..n as u32).collect();
    for [u, v] in links {
        let (x, y) = (root(&mut part, u), root(&mut part, v));
        part[x.max(y) as usize] = x.min(y);
    }
    for v in 0..n as u32 {
        part[v as usize] = root(&mut part, v);
    }
    part
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod curved_tests;

#[cfg(test)]
mod seeded_tests;

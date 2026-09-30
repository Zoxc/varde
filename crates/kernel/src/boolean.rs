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
//!    pair's cut runs between its two ends; the parts are kept by winding
//!    number for the operation; each cut face's kept part is triangulated
//!    in its parameter domain, and the halfedges pair up by vertex id.
//! 5. **Clean-up, repair and check**: the degenerate triangles flush
//!    operands leave are removed, and the result is repaired and checked.
//!
//! This is the counting skeleton for **flat patches**: patches whose
//! edges are straight within the resolution. The primitives are exact,
//! with symbolic perturbation breaking ties (see [`flat`]), and the new
//! edges are straight. Curved primitives (conic `s11`, rational `s02`,
//! pair refinement for hidden loops and arc pairing, tracing and fitting
//! the cuts) come in through [`Primitives`] and the assembly's cut edges;
//! until then an operand with a curved patch is refused with
//! [`BooleanError::Curved`]. See `agents/kernel.md`.

use std::cmp::Ordering;

use glam::DVec3;

use crate::budget::{Budget, Work};
use crate::mesh::{BuildError, Face, Mesh, MeshBuilder};
use crate::{KernelError, Solid, Tolerance};

mod assemble;
mod cleanup;
mod count;
mod exact;
mod flat;
mod input;
mod triangulate;

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
    /// An operand has a curved patch: curved booleans aren't built yet.
    Curved,
    /// An operand faces inwards, or its winding numbers aren't 0 and 1.
    InsideOut,
    /// The decisions don't fit together. Exact primitives never do this.
    Inconsistent,
    /// A cut face's part couldn't be triangulated.
    Degenerate,
}

impl std::fmt::Display for BooleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BooleanError::Curved => "booleans on curved faces aren't available yet",
            BooleanError::InsideOut => "a solid is inside out",
            BooleanError::Inconsistent => "the intersection's decisions don't fit together",
            BooleanError::Degenerate => "a cut face couldn't be triangulated",
        })
    }
}

impl std::error::Error for BooleanError {}

/// The projection direction every primitive shares: nearly `+z`, tilted
/// off every axis so that walls along the axes, which CAD models are full
/// of, don't all project to lines. Its coordinates are small integers, so
/// the exact predicates take it as it is.
const UP: DVec3 = DVec3::new(2.0, 3.0, 32.0);

/// How edges `e` of `A` and `g` of `B` cross, seen along [`UP`], in their
/// own directions: `sigma` is the sign of `det[g, e, UP]` (+1 where `e`
/// crosses `g` from its right to its left), and `a_above` says whether
/// `e` passes above `g` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cross11 {
    sigma: i8,
    a_above: bool,
}

/// The primitives the counting reads. Each is asked once per pair, and
/// its answer stored and shared by everything that needs it.
trait Primitives: Sync {
    /// `s02`: the signed number of layers of face `f` of the other operand
    /// above vertex `v` of `side`, along [`UP`].
    fn s02(&self, side: Side, v: u32, f: u32) -> i8;
    /// `s11`: how edge `e` of `A` and edge `g` of `B` cross, if they do.
    fn s11(&self, e: u32, g: u32) -> Option<Cross11>;
    /// The order along edge `e` of `side`, in its direction, of its
    /// crossings with faces `f1` and `f2` of the other operand. Both
    /// crossings exist.
    fn order(&self, side: Side, e: u32, f1: u32, f2: u32) -> Ordering;
    /// Where along edge `e` of `side` (0 at its start, 1 at its end) it
    /// crosses face `f` of the other: only the position, never a decision.
    fn crossing(&self, side: Side, e: u32, f: u32) -> f64;
}

/// `a op b`, within `budget`: a solid that passes `check`, always.
///
/// Fails with [`KernelError::Boolean`] for an operand with a curved patch
/// (not yet), or one that is inside out; with
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
    Solid::new(mesh, tol)
}

/// The result's mesh, before repair and the check.
fn unchecked(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Mesh, KernelError> {
    let (ia, ib) = inputs(a, b, tol)?;
    let prims = flat::Flat::new(&ia, &ib, op == Op::Union);
    let counts = count::count(&ia, &ib, &prims, tol, work)?;
    let (mut soup, faces) = assemble::assemble(op, &ia, &ib, &counts, &prims, work)?;
    cleanup::clean(
        &mut soup,
        tol.resolution() / 8.0,
        4.0 * tol.resolution(),
        work,
    )?;
    build(soup, faces)
}

/// Whether `a` and `b` touch or overlap: whether an edge of one crosses a
/// face of the other or a vertex of one is inside the other, with solids
/// that only touch (flush faces, an edge on a face) counted as touching.
/// Only the broad phase and the counting run.
pub fn touches(
    a: &Solid,
    b: &Solid,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<bool, KernelError> {
    if a.is_empty() || b.is_empty() {
        return Ok(false);
    }
    let (ia, ib) = inputs(a, b, tol)?;
    let prims = flat::Flat::new(&ia, &ib, true);
    let counts = count::count(&ia, &ib, &prims, tol, &mut Work::new(budget))?;
    Ok(counts.meet())
}

fn inputs<'a>(
    a: &'a Solid,
    b: &'a Solid,
    tol: &Tolerance,
) -> Result<(Input<'a>, Input<'a>), KernelError> {
    let ia = Input::new(a.mesh(), tol)?;
    let ib = Input::new(b.mesh(), tol)?;
    if ia.volume() <= 0.0 || ib.volume() <= 0.0 {
        return Err(KernelError::Boolean(BooleanError::InsideOut));
    }
    Ok((ia, ib))
}

/// The mesh of the cleaned triangles: vertices no triangle uses dropped,
/// the rest numbered in order, halfedges paired by vertex id. No
/// triangles give the empty mesh.
fn build(soup: cleanup::Soup, faces: Vec<Face>) -> Result<Mesh, KernelError> {
    if soup.tris.is_empty() {
        return Ok(Mesh::default());
    }
    let mut used = vec![false; soup.pos.len()];
    for tri in &soup.tris {
        for &v in tri {
            used[v as usize] = true;
        }
    }
    let mut builder = MeshBuilder::new();
    let id: Vec<u32> = used
        .iter()
        .zip(&soup.pos)
        .map(|(&used, &p)| if used { builder.vert(p) } else { u32::MAX })
        .collect();
    for f in faces {
        builder.face(f);
    }
    for (tri, face) in soup.tris.iter().zip(soup.faces) {
        builder.tri(tri.map(|v| id[v as usize]), face);
    }
    builder.build().map_err(|e| match e {
        BuildError::TooManyPatches(_) => KernelError::TooComplex,
        _ => KernelError::Boolean(BooleanError::Degenerate),
    })
}

#[cfg(test)]
mod tests;

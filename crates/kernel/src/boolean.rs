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
//!    nothing cut are merged back, and the result is repaired, its
//!    adjacent faces on one surface named alike (`Mesh::merge_faces`), and
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
//! faces' tags say which, see [`surface`]), and cylinders, cones and
//! spheres on one axis meet in parallels, exact too (their forms say so,
//! see [`coaxial`]); anything else is traced where
//! the patches meet and fitted with conics within the fit tolerance. Cut
//! curved faces keep their own surface wherever they can: their new inner
//! edges are the patch's own curves, and along a cut of a quadric they are
//! chosen so that the triangles there lie on it exactly too (see
//! [`assemble`]). See `agents/kernel.md`.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use glam::DVec3;

use crate::budget::{Budget, Work};
use crate::mesh::{BuildError, Bvh, CheckError, Face, FaceKey, Hint, Mesh, MeshBuilder, Surface};
use crate::patch::Bounds3;
use crate::solid::{CHECK_WORK, Kept, Unfinished};
use crate::topology::distance::{Allowance, to_patches};
use crate::{Evidence, Failure, KernelError, Solid, Tolerance};

mod assemble;
mod chain;
mod cleanup;
mod coaxial;
mod count;
mod curved;
mod evidence;
pub(crate) mod exact;
mod flat;
mod input;
pub(crate) mod near;
mod pairs;
mod split;
mod surface;
mod triangulate;

use count::Crossing;
use curved::SeamRules;
use input::{Input, Side};
use pairs::Shortcuts;
pub use split::{ToolError, chain_tool, half_space, split, surface_tool};

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
    /// The result would touch itself along an edge or at a point, so it
    /// isn't a manifold: solids touching only along an edge or at a corner,
    /// united, or one taking from another a part that touches its skin
    /// from inside along a line or at a point. Parts closer than the
    /// resolution come to the same thing at the kernel's resolution, and
    /// are named so too. Found where the result fails to repair or pass
    /// `check` and has two vertices within the clean-up's short length
    /// before repair, or two separate shells whose hulls come within the
    /// resolution (see `agents/kernel.md`, "Results that aren't
    /// manifolds"); the decisions may also give it before any mesh is
    /// built, where they show a pinch.
    NotManifold,
}

impl std::fmt::Display for BooleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BooleanError::Inconsistent => "where the solids cross doesn't add up",
            BooleanError::Degenerate => "a cut face couldn't be triangulated",
            BooleanError::NotManifold => {
                "the result would touch itself along an edge or at a point, or come closer \
                 to itself than the resolution"
            }
        })
    }
}

impl std::error::Error for BooleanError {}

/// How near two things must be for the primitives to decide them as a
/// tie, by the perturbation: a 64th ([`TIES`]) of the resolution. Exact
/// ties (flush faces, a vertex on a face) and those rounding leaves (the
/// same faces turned and moved, every coordinate rounded) then decide
/// alike.
fn tie(tol: &Tolerance) -> f64 {
    tol.resolution() / TIES
}

/// How many tie distances ([`tie`]) make the resolution.
const TIES: f64 = 64.0;

/// The clean-up's short length, an eighth of the resolution: shorter
/// straight edges collapse, and nearer ends make inner edges twins of
/// boundary ones (`assemble::face::cut_face`), in step with it.
fn short(tol: &Tolerance) -> f64 {
    tol.resolution() / 8.0
}

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
    /// pair them: 0 for exact primitives; for those deciding near ties
    /// as ties or numerical ones, as far as what they decide as a tie or
    /// take as flat reaches, so every pair a tie decides is counted too
    /// (a vertex a hair from a face, decided as on it, whose edges
    /// nothing then paired with that face, put a whole operand inside the
    /// other).
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
/// together, a cut face that can't be triangulated, or a result that
/// would touch itself along an edge or at a point (two solids touching
/// along an edge or at a corner, united: the exact result isn't a
/// manifold), or come closer to itself than the resolution
/// ([`BooleanError::NotManifold`]; see [`BooleanError`]); with
/// [`KernelError::TooComplex`] past the budget; and with
/// [`KernelError::Invalid`] when the result can't pass `check` with `tol`
/// for another reason, such as parts too thin for the resolution (a cusp
/// where faces are tangent, thin triangles), or faces within a tie of
/// each other that would leave a triangle facing against its face's
/// plane (its tag or form). Telling `NotManifold` from `Invalid` costs
/// work only once the operation has failed, so it never turns a result
/// into an error. An `Invalid` from repair or the check, and a
/// `NotManifold` named from one, come with the triangles of the result
/// the check's error names (or the pieces of them repair couldn't mend)
/// as [`Failure::evidence`], and a pinch with its two vertices; a
/// `NotManifold` from the decisions with the patches touching along a
/// line and the operands' faces they lie on; a `Degenerate` with the cut
/// face's loops and the face, or what the mesh's builder named; an
/// [`BooleanError::Inconsistent`] with what doesn't fit together (an edge
/// and its crossings, a vertex, a pair of patches and their ends, a
/// crossing, an arc and the curve refused for it, or a cut face's
/// boundary) and the operands' faces involved.
pub fn boolean(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, Failure> {
    boolean_within(a, b, op, tol, &mut Work::new(budget))
}

/// The least work [`boolean`] gives its last try, without joining ends
/// along walls' common direction, after one failed with them: a first
/// try that failed fast (folding on a sliver under the resolution in a
/// few thousand units) took up to 150 000 to refine as before.
const AGAIN: u64 = 150_000;

/// [`boolean`], charging `work`. The failure returned is a try's error
/// with that try's evidence.
///
/// The tries decide by the rules for seams meeting on the cut
/// ([`SeamRules`]); where every try fails the result's check
/// ([`KernelError::Invalid`], or a pinch named
/// [`BooleanError::NotManifold`]), or runs into a bound with budget
/// left (`TooComplex`), and the rules changed some decision,
/// the tries are made again without them, within three times the work
/// they took, or `AGAIN` if more (and what is left; those that worked
/// in the sweeps took up to 2.1 times as much): on frames turned by
/// a hair, the rules are one more way of deciding a near tie, and the
/// thin triangles a tangency's two crossings leave where seams nearly
/// meet pass the check or not by chance either way. The failure is the
/// first tries' then, or `TooComplex` where the budget ran out.
fn boolean_within(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Solid, Failure> {
    match (a.is_empty(), b.is_empty(), op) {
        (true, _, Op::Union) => return Ok(b.clone()),
        (_, true, Op::Union | Op::Difference) => return Ok(a.clone()),
        (true, _, _) | (_, true, _) => return Ok(Solid::empty()),
        _ => {}
    }
    let start = work.left();
    let rules = SeamRules::new(true);
    let e = match tries(a, b, op, &rules, tol, work) {
        Err(e)
            if rules.differed()
                && (matches!(
                    e.error,
                    KernelError::Invalid(_) | KernelError::Boolean(BooleanError::NotManifold)
                ) || e.error == KernelError::TooComplex && work.left() > 0) =>
        {
            e
        }
        result => return result,
    };
    let spent = start.saturating_sub(work.left());
    let cap = spent.saturating_mul(3).max(AGAIN).min(work.left());
    let mut again = Work::new(&Budget::new(cap));
    let next = tries(a, b, op, &SeamRules::new(false), tol, &mut again);
    work.spend(usize::try_from(cap - again.left()).unwrap_or(usize::MAX))?;
    match next {
        // Run out of the operation's budget, not the cap: as any try.
        Err(f) if f.error == KernelError::TooComplex && work.left() == 0 => {
            Err(KernelError::TooComplex.into())
        }
        Err(_) => Err(e),
        result => result,
    }
}

/// [`boolean_within`]'s tries, deciding by `rules`: with every shortcut,
/// and where that fails, without those taken.
fn tries(
    a: &Solid,
    b: &Solid,
    op: Op,
    rules: &SeamRules,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Solid, Failure> {
    let start = work.left();
    let mut used = Shortcuts::NONE;
    // What the mesh that failed to repair or pass `check` leaves, to
    // tell a result touching itself from others at the end.
    let mut failed = None;
    let first = checked_with(
        a,
        b,
        op,
        Shortcuts::ALL,
        rules,
        &mut used,
        &mut failed,
        tol,
        work,
    );
    // A coaxial cut's bands may stray past the fit with nothing left to
    // halve (`TooComplex` with budget to spare): tried again too.
    let retried = |e: &Failure, work: &Work| {
        used.any() && (e.error != KernelError::TooComplex || used.coaxial && work.left() > 0)
    };
    let e = match first {
        Err(e) if retried(&e, work) => e,
        result => return pinched_named(result, &failed, tol, work),
    };
    // Shortcuts taken in an early round (lines joined along walls' common
    // direction, coaxial walls certified) leave the pieces beside them as
    // large as they were, and some results that refinement gets right
    // fail the hull, neighbour or fold rules from them: then the result
    // is the one without them, else the first try's error. Without the
    // coaxial certificate, the joins along walls are tried first, as
    // they were before it, with what is left of the budget; then without
    // either, within as much work again as the tries so far took, or
    // `AGAIN` if more (and what is left). Unbounded, that last try ran
    // most refusals on to the budget, for a result in one of fifteen.
    // Each try decides with near ties and, where those don't fit
    // together, again exactly (`assembled`), both within the try's cap;
    // `used` holds what either took.
    let mut shortcuts = Shortcuts {
        along: used.coaxial,
        coaxial: false,
    };
    loop {
        let spent = start.saturating_sub(work.left());
        // Joining lines but not certifying coaxial walls, the decisions
        // are as they were before the certificate, which had the whole
        // budget.
        let cap = if shortcuts.along {
            work.left()
        } else {
            spent.max(AGAIN).min(work.left())
        };
        let mut again = Work::new(&Budget::new(cap));
        let mut taken = Shortcuts::NONE;
        let next = checked_with(
            a, b, op, shortcuts, rules, &mut taken, &mut None, tol, &mut again,
        );
        // What this try took, charged to the operation's budget.
        work.spend(usize::try_from(cap - again.left()).unwrap_or(usize::MAX))?;
        match next {
            Err(f) if f.error == KernelError::TooComplex && work.left() == 0 => {
                return Err(KernelError::TooComplex.into());
            }
            Err(f) if taken.any() && f.error != KernelError::TooComplex => {
                shortcuts = Shortcuts::NONE;
            }
            Err(_) => return pinched_named(Err(e), &failed, tol, work),
            result => return result,
        }
    }
}

/// What a result that failed repair or the check as
/// [`KernelError::Invalid`] leaves to tell whether it touches itself,
/// kept until the operation's last word (see [`pinched_named`]).
struct Failed {
    /// The mesh that failed, and the mesh as given to repair, the
    /// cleaned mesh, whose positions are the ones [`pinched`] measures.
    unfinished: Unfinished,
    /// The two triangles whose hulls came too close, if that was the
    /// failure: of the cleaned mesh where repair names them (by the
    /// triangles its pieces came from), else of the mesh the check
    /// refused.
    hull: Option<(u32, u32)>,
}

/// `result`, an operation's last word, with its [`KernelError::Invalid`]
/// named [`BooleanError::NotManifold`] where `failed` shows the result
/// touching itself:
/// - its positions before repair have two within the clean-up's short
///   length (see [`pinched`]): the clean-up collapses shorter straight
///   edges wherever every vertex keeps one fan, so two vertices that near
///   after it are mostly the zero-width neck the perturbation leaves where
///   the exact result touches itself (vertices along short curved edges,
///   never collapsed, can be too: then the name is a guess, which only
///   words the error);
/// - or repair or the check found the hulls of two triangles of separate
///   shells closer than the resolution (see [`apart`]): two parts touching or
///   nearly, as cylinders tangent along a line, united, whose shells the
///   operation leaves uncut.
///
/// Only an error is renamed, never `Ok` made one or one made `Ok`, and
/// not where telling runs out of what is left of the budget: then the
/// error stays as it was, the budget spent. A renamed error keeps the
/// evidence it came with, what the check or repair named, and where
/// named for two near vertices gets those as points too (one where they
/// are at one place): the pinch itself.
fn pinched_named(
    result: Result<Solid, Failure>,
    failed: &Option<Failed>,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Solid, Failure> {
    match (result, failed) {
        (
            Err(Failure {
                error: KernelError::Invalid(e),
                evidence,
            }),
            Some(failed),
        ) => {
            // Whether it touches itself, and where two vertices come
            // within the short length if that is how.
            let mut touches = || -> Result<Option<Option<[DVec3; 2]>>, KernelError> {
                let given = failed.unfinished.given();
                if let Some(pinch) = pinched(given.verts(), short(tol), work)? {
                    return Ok(Some(Some(pinch)));
                }
                let apart = match (failed.hull, &failed.unfinished) {
                    (Some((t, u)), Unfinished::Repair { given, .. }) => apart(given, t, u, work)?,
                    (Some((t, u)), Unfinished::Check { checked, .. }) => {
                        // A unit a triangle for listing its corners;
                        // the cleaned mesh's go uncharged, as the
                        // operation's work was set with them listed
                        // for free.
                        work.spend(checked.tris().len())?;
                        apart(checked, t, u, work)?
                    }
                    (None, _) => false,
                };
                Ok(apart.then_some(None))
            };
            let mut evidence = evidence;
            let error = match touches() {
                Ok(Some(pinch)) => {
                    // The pinch itself: both vertices, or one where they
                    // are at one place.
                    if let Some([p, q]) = pinch {
                        if p == q {
                            evidence.add_points([p]);
                        } else {
                            evidence.add_points([p, q]);
                        }
                    }
                    KernelError::Boolean(BooleanError::NotManifold)
                }
                _ => KernelError::Invalid(e),
            };
            Err(Failure { error, evidence })
        }
        (result, _) => result,
    }
}

/// Whether triangles `t` and `u` of `mesh` lie on separate shells, not
/// joined through edges. A unit of work a triangle.
fn apart(mesh: &Mesh, t: u32, u: u32, work: &mut Work) -> Result<bool, KernelError> {
    let n = mesh.tris().len();
    work.spend(n)?;
    if t as usize >= n || u as usize >= n {
        return Ok(false);
    }
    let tris: Vec<[u32; 3]> = (0..n as u32).map(|t| mesh.corners(t)).collect();
    let verts = mesh.verts().len();
    if tris.iter().flatten().any(|&v| v as usize >= verts) {
        return Ok(false);
    }
    let part = parts(
        verts,
        tris.iter().flat_map(|v| [[v[0], v[1]], [v[1], v[2]]]),
    );
    Ok(part[tris[t as usize][0] as usize] != part[tris[u as usize][0] as usize])
}

/// Two of `verts` within `d` of each other, the first found, by a grid of
/// cells `d` wide: each vertex is measured against those before it in
/// the 27 cells round its own. Cells are keyed by `floor(p / d)` as
/// `i64`: coordinates within [`MAX_COORD`](crate::MAX_COORD) and `d` at
/// least the finest tolerance's short length keep keys under about
/// `1e15`, and `as` saturates past that (NaN gives 0) and the
/// neighbours' offsets saturate too, which can only put more vertices in
/// a cell, never miss a near pair. A unit of work a vertex and one a
/// vertex it is measured against; points at least `d` apart fit about a
/// hundred to the 27 cells, so that is bounded too. Whether there is a
/// pair depends only on the positions, and the work and the pair found
/// (the earlier vertex first) on their order.
fn pinched(verts: &[DVec3], d: f64, work: &mut Work) -> Result<Option<[DVec3; 2]>, KernelError> {
    if !(d > 0.0 && d.is_finite()) {
        return Ok(None);
    }
    let key = |p: DVec3| {
        let c = (p / d).floor();
        [c.x as i64, c.y as i64, c.z as i64]
    };
    let near = d * d;
    let mut cells: std::collections::HashMap<[i64; 3], Vec<u32>> =
        std::collections::HashMap::with_capacity(verts.len());
    for (i, &p) in verts.iter().enumerate() {
        work.spend(1)?;
        let k = key(p);
        for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                for dz in -1..=1i64 {
                    let cell = [
                        k[0].saturating_add(dx),
                        k[1].saturating_add(dy),
                        k[2].saturating_add(dz),
                    ];
                    let Some(list) = cells.get(&cell) else {
                        continue;
                    };
                    work.spend(list.len())?;
                    if let Some(&j) = list
                        .iter()
                        .find(|&&j| verts[j as usize].distance_squared(p) <= near)
                    {
                        return Ok(Some([verts[j as usize], p]));
                    }
                }
            }
        }
        cells.entry(k).or_default().push(i as u32);
    }
    Ok(None)
}

#[cfg(test)]
thread_local! {
    /// What [`cleanup::thin_across`] found in the last operation's cleaned
    /// soup on this thread, `(0, 0)` if it didn't get that far: for tests
    /// measuring thin triangles across two faces.
    static THIN_ACROSS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    /// Whether operations on this thread keep the failure of their try
    /// with near ties, rather than deciding again without them
    /// ([`tied_or_exact`]): for tests of what such a failure shows.
    static ONE_TRY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The result's mesh, before repair and the check, joining ends along
/// walls' common direction, with the operand's triangle each of its
/// triangles may be ([`Kept::source`]).
#[cfg(test)]
fn unchecked(
    a: &Solid,
    b: &Solid,
    op: Op,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(Mesh, Vec<Hint>), KernelError> {
    let mut used = Shortcuts::NONE;
    let rules = SeamRules::new(true);
    let (soup, faces) =
        assembled(a, b, op, Shortcuts::ALL, &rules, &mut used, tol, work).map_err(|f| f.error)?;
    cleaned(a, b, op, soup, faces, true, &mut false, tol, work).map_err(|f| f.error)
}

/// The result, taking only the `shortcuts` given (see
/// [`pairs::refined_with`]) and deciding by `rules`, and setting `used`
/// to the shortcuts some pair took:
/// assembled, cleaned, repaired and checked. Where it fails as
/// [`KernelError::Invalid`] from repair or the check, `failed` is left
/// with what the mesh that failed shows (see [`checked`]), for
/// [`pinched_named`], and the failure with what it names.
#[allow(clippy::too_many_arguments)]
fn checked_with(
    a: &Solid,
    b: &Solid,
    op: Op,
    shortcuts: Shortcuts,
    rules: &SeamRules,
    used: &mut Shortcuts,
    failed: &mut Option<Failed>,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Solid, Failure> {
    let (soup, faces) = assembled(a, b, op, shortcuts, rules, used, tol, work)?;
    // The clean-up's last resort, unfolding sheets folded onto a flush
    // face, can leave a soup that fails where the clean-up without it
    // would have mended it by other means (Delaunay flips on a face a
    // hair off another): then the result is the one without it, if that
    // passes, so the rule never loses a result.
    let kept = (soup.clone(), faces.clone());
    let mut unfolded = false;
    match checked(
        a,
        b,
        op,
        soup,
        faces,
        true,
        &mut unfolded,
        failed,
        tol,
        work,
    ) {
        Err(e) if matches!(e.error, KernelError::Invalid(_)) && unfolded => {
            // The first try's error stands, so its positions and
            // evidence do.
            let first = failed.take();
            let (soup, faces) = kept;
            match checked(
                a,
                b,
                op,
                soup,
                faces,
                false,
                &mut unfolded,
                failed,
                tol,
                work,
            ) {
                Err(_) => {
                    *failed = first;
                    Err(e)
                }
                result => result,
            }
        }
        result => result,
    }
}

/// The result of the assembled `soup` and `faces`, cleaned (unfolding
/// folded sheets if `unfold`, and setting `unfolded` if it did),
/// repaired and checked. `failed` is left with the mesh that failed
/// where repair or the check fails as [`KernelError::Invalid`] (but for a
/// triangle facing against its face's plane, never renamed), and `None`
/// otherwise; such a failure comes with what it names, of that mesh
/// ([`Unfinished::failure`]).
#[allow(clippy::too_many_arguments)]
fn checked(
    a: &Solid,
    b: &Solid,
    op: Op,
    soup: cleanup::Soup,
    faces: Vec<Face>,
    unfold: bool,
    unfolded: &mut bool,
    failed: &mut Option<Failed>,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<Solid, Failure> {
    *failed = None;
    let (mesh, source) = cleaned(a, b, op, soup, faces, unfold, unfolded, tol, work)?;
    // Faces of one surface that meet merge, so a flush join leaves no
    // line between the two operands' pieces of a plane or cylinder.
    // Repair and the check test only what the operation changed, and
    // what comes near the other operand's kept triangles.
    let kept = Kept {
        operands: [a, b],
        source,
    };
    match Solid::finished_near(mesh, CHECK_WORK, Some(kept), tol, work) {
        Ok(solid) => Ok(solid),
        Err((error @ KernelError::Invalid(why), Some(unfinished))) => {
            let failure = unfinished.failure(error);
            let hull = match why {
                CheckError::FacesAgainst(_) => return Err(failure),
                CheckError::Hull(t, u) => Some((t, u)),
                _ => None,
            };
            *failed = Some(Failed { unfinished, hull });
            Err(failure)
        }
        Err((error, _)) => Err(error.into()),
    }
}

/// The result's triangles and faces, before the clean-up, taking only the
/// `shortcuts` given (see [`pairs::refined_with`]), deciding by `rules`
/// where an operand is curved, and adding to `used`
/// those some pair took. A union
/// of walls touching along a line comes with the pairs of faces showing
/// it, a cut face that can't be triangulated with its loops. Decisions
/// with near ties that don't fit together
/// ([`BooleanError::Inconsistent`]) are made again without them, flat
/// operands' or curved ones' ([`flat_soup`]), with the same `shortcuts`
/// and from the same `work`: the exact retry sits inside each of
/// [`boolean_within`]'s tries with fewer shortcuts, so it is bounded by
/// that try's cap.
#[allow(clippy::too_many_arguments)]
fn assembled(
    a: &Solid,
    b: &Solid,
    op: Op,
    shortcuts: Shortcuts,
    rules: &SeamRules,
    used: &mut Shortcuts,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(cleanup::Soup, Vec<Face>), Failure> {
    #[cfg(test)]
    THIN_ACROSS.set((0, 0));
    let (ia, ib) = (Input::new(a.mesh(), tol), Input::new(b.mesh(), tol));
    let (soup, faces) = if ia.curved || ib.curved {
        tied_or_exact(tie(tol), work, |tie, work| {
            curved_decided(a, b, op, shortcuts, rules, used, tie, tol, work)
        })?
    } else {
        flat_soup(op, &ia, &ib, tie(tol), tol, work)?
    };
    Ok((soup, faces))
}

/// [`assembled`]'s one try for operands with curved patches, near ties
/// within `tie` taken as ties (0: none), deciding by `rules`, taking
/// only the `shortcuts`
/// given: counted and decided pair by pair, the operands refined where a
/// pair needs it. Adds the shortcuts some pair took to `used`, so that
/// after the tied try and the exact one it holds what either took: a
/// tied try that took one and failed is reason enough to try the
/// operation again without it ([`boolean_within`]).
#[allow(clippy::too_many_arguments)]
fn curved_decided(
    a: &Solid,
    b: &Solid,
    op: Op,
    shortcuts: Shortcuts,
    rules: &SeamRules,
    used: &mut Shortcuts,
    tie: f64,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(cleanup::Soup, Vec<Face>), Failure> {
    let grow = op == Op::Union;
    let refined = pairs::refined_with(a.mesh(), b.mesh(), grow, shortcuts, tie, rules, tol, work)?;
    used.along |= refined.used.along;
    used.coaxial |= refined.used.coaxial;
    let (ra, rb) = (Input::new(&refined.a, tol), Input::new(&refined.b, tol));
    let prims = curved::Curved::new(&ra, &rb, grow, tie, rules, tol);
    let refinement = assemble::Refinement {
        tree: [&refined.tree[0], &refined.tree[1]],
        leaf: [&refined.leaf[0], &refined.leaf[1]],
    };
    let mut took = false;
    let assembled = assemble::assemble(
        op,
        &ra,
        &rb,
        &refined.counts,
        &refined.arcs,
        &prims,
        tol,
        Some(&refinement),
        shortcuts.coaxial,
        &mut took,
        rules,
        work,
    );
    // Taken even where the cut faces then fail.
    used.coaxial |= took;
    assembled
}

/// The mesh of the assembled `soup` and `faces`, cleaned (unfolding
/// folded sheets if `unfold`, and setting `unfolded` if it did), before
/// repair and the check, with the operand's triangle each of its
/// triangles may be ([`cleanup::Soup::source`]). Triangles that don't
/// pair up come with what the mesh's builder named (see [`build`]).
#[allow(clippy::too_many_arguments)]
fn cleaned(
    a: &Solid,
    b: &Solid,
    op: Op,
    mut soup: cleanup::Soup,
    mut faces: Vec<Face>,
    unfold: bool,
    unfolded: &mut bool,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(Mesh, Vec<Hint>), Failure> {
    *unfolded = cleanup::clean(
        &mut soup,
        &mut faces,
        short(tol),
        4.0 * tol.resolution(),
        unfold,
        tol,
        work,
    )?;
    #[cfg(test)]
    THIN_ACROSS.set(cleanup::thin_across(
        &soup,
        &faces,
        short(tol),
        4.0 * tol.resolution(),
    ));
    let aliases = aliases(a.mesh(), b.mesh(), &faces, &soup, work)?;
    // The mesh's triangles are the soup's, in order.
    let source = soup.source.clone();
    let mesh = build(soup, faces, &aliases)?;
    if op == Op::Difference {
        return Ok((mesh, source));
    }
    Ok((
        covered(mesh, [a.mesh(), b.mesh()], short(tol), work)?,
        source,
    ))
}

/// `mesh` with an alias for each operand's plane face whose key or
/// aliases no face or alias of it names any more because the other
/// operand's face, flush with it, covered it (the perturbation keeps one
/// of two flush caps whole and drops the other: a plate first, a boss
/// standing in it flush on top, and the boss's top is gone). Those of its
/// names become aliases of the lowest face of the same plane (unit
/// normals within `1e-12`, offsets within `small`) that it meets: the
/// middle of a triangle of one lies within `small` of the other (either
/// way round, so a small face over a large one's middle is found, and a
/// large one round a small one). Only names: the geometry is as it was.
/// A face nothing covers (cut away, or inside the other operand) gets
/// none: a point of it on the result's boundary, facing the same way, is
/// a point where it lay flush. Each middle looked up is charged a unit
/// and one a patch it is measured against, and each face's lookup table
/// a unit a triangle.
fn covered(
    mesh: Mesh,
    operands: [&Mesh; 2],
    small: f64,
    work: &mut Work,
) -> Result<Mesh, KernelError> {
    let unit = |surface: Surface| match surface {
        Surface::Plane { n, d } => {
            let len = n.length();
            (len > 0.0 && len.is_finite()).then(|| (n / len, d / len))
        }
        _ => None,
    };
    let named: std::collections::BTreeSet<FaceKey> = mesh
        .faces()
        .iter()
        .map(|f| f.name.key())
        .chain(mesh.aliases().iter().map(|&(_, key)| key))
        .collect();
    let on = tris_by_face(&mesh);
    // The result's plane faces by offset, to find those of a plane fast.
    let mut planes: Vec<(f64, u32, DVec3)> = (0..mesh.faces().len() as u32)
        .filter_map(|g| unit(mesh.faces()[g as usize].surface).map(|(n, d)| (d, g, n)))
        .collect();
    planes.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    // The result's faces' tables, made when first needed.
    let mut tables: BTreeMap<u32, Bvh> = BTreeMap::new();
    let mut added: Vec<(u32, FaceKey)> = Vec::new();
    for operand in operands {
        let tris = tris_by_face(operand);
        for (f, face) in operand.faces().iter().enumerate() {
            let Some((n, d)) = unit(face.surface) else {
                continue;
            };
            let lost: Vec<FaceKey> = std::iter::once(face.name.key())
                .chain(operand.face_aliases(f as u32))
                .filter(|key| !named.contains(key))
                .collect();
            if lost.is_empty() || tris[f].is_empty() {
                continue;
            }
            let from = planes.partition_point(|p| p.0 < d - small);
            let mut same: Vec<u32> = planes[from..]
                .iter()
                .take_while(|p| p.0 <= d + small)
                .filter(|p| p.2.dot(n) > 1.0 - 1e-12)
                .map(|p| p.1)
                .collect();
            if same.is_empty() {
                continue;
            }
            same.sort_unstable();
            let mine = table(operand, &tris[f], work)?;
            let mut left = Allowance::new();
            for g in same {
                let theirs = match tables.entry(g) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => e.insert(table(&mesh, &on[g as usize], work)?),
                };
                let meets = lies_on(
                    (operand, &tris[f]),
                    (&mesh, &on[g as usize], theirs),
                    small,
                    work,
                    &mut left,
                )? || lies_on(
                    (&mesh, &on[g as usize]),
                    (operand, &tris[f], &mine),
                    small,
                    work,
                    &mut left,
                )?;
                if meets {
                    added.extend(lost.iter().map(|&key| (g, key)));
                    break;
                }
            }
        }
    }
    if added.is_empty() {
        return Ok(mesh);
    }
    added.extend_from_slice(mesh.aliases());
    Ok(mesh.with_aliases(added))
}

/// The triangles of each face of `mesh`, ascending.
fn tris_by_face(mesh: &Mesh) -> Vec<Vec<u32>> {
    let mut on: Vec<Vec<u32>> = vec![Vec::new(); mesh.faces().len()];
    for (t, tri) in mesh.tris().iter().enumerate() {
        on[tri.face as usize].push(t as u32);
    }
    on
}

/// The boxes of `mesh`'s triangles `tris`, in that order, for looking
/// points up among them: charged a unit a triangle.
fn table(mesh: &Mesh, tris: &[u32], work: &mut Work) -> Result<Bvh, KernelError> {
    work.spend(tris.len())?;
    Ok(Bvh::new(
        tris.iter()
            .map(|&t| mesh.patch(t as usize).bounds())
            .collect(),
    ))
}

/// Whether the middle of one of the triangles `from.1` of mesh `from.0`
/// lies within `small` of one of the triangles `onto.1` of mesh `onto.0`
/// (their [`table`] `onto.2`), looking in order and stopping at the
/// first. Each middle is charged a unit and one a patch it is measured
/// against; the distance searches share `left`.
fn lies_on(
    from: (&Mesh, &[u32]),
    onto: (&Mesh, &[u32], &Bvh),
    small: f64,
    work: &mut Work,
    left: &mut Allowance,
) -> Result<bool, KernelError> {
    let mut near = Vec::new();
    for &t in from.1 {
        let x = from.0.patch(t as usize).eval(DVec3::splat(1.0 / 3.0));
        near.clear();
        onto.2.query(&Bounds3::point(x), small, &mut near);
        work.spend(near.len().saturating_add(1))?;
        let patches = near
            .iter()
            .map(|&i| onto.0.patch(onto.1[i as usize] as usize));
        if to_patches(x, patches, 2.0 * small, left) <= small {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The aliases of each source face of the soup (an operand's face, `A`'s
/// then `B`'s): the operand's own, and the keys of the faces merged into
/// it ([`cleanup::Soup::absorb`]) with their aliases, through any chain
/// of merges. Each merge in each pass is charged a unit and one a key of
/// the two faces' sets.
fn aliases(
    a: &Mesh,
    b: &Mesh,
    faces: &[Face],
    soup: &cleanup::Soup,
    work: &mut Work,
) -> Result<Vec<Vec<FaceKey>>, KernelError> {
    let na = a.faces().len() as u32;
    let mut sets: Vec<Vec<FaceKey>> = (0..na)
        .map(|f| a.face_aliases(f).collect())
        .chain((0..b.faces().len() as u32).map(|f| b.face_aliases(f).collect()))
        .collect();
    let mut merges = soup.absorbed.clone();
    merges.sort_unstable();
    merges.dedup();
    // Until nothing changes: at most a pass a merge, as each pass that
    // changes something takes one more step along the chains.
    for _ in 0..=merges.len() {
        let mut changed = false;
        for &(from, into) in &merges {
            let keys = sets[from as usize].len() + sets[into as usize].len();
            work.spend(keys.saturating_add(1))?;
            let mut add: Vec<FaceKey> = sets[from as usize].clone();
            add.push(faces[from as usize].name.key());
            let set = &mut sets[into as usize];
            let before = set.len();
            set.extend(add);
            set.sort_unstable();
            set.dedup();
            changed |= set.len() != before;
        }
        if !changed {
            break;
        }
    }
    Ok(sets)
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
/// from the same `work`, and only on that failure. The failure is the
/// last try's. Operands with curved patches are decided again the same
/// way ([`assembled`]), the numerical primitives and the `Flat` inside
/// them together, so they still share one tie: their decisions are then
/// the numbers' own, where rounding can still part them, but no tie
/// pulls the two kinds apart.
fn flat_soup(
    op: Op,
    ia: &Input,
    ib: &Input,
    tie: f64,
    tol: &Tolerance,
    work: &mut Work,
) -> Result<(cleanup::Soup, Vec<Face>), Failure> {
    tied_or_exact(tie, work, |tie, work| {
        flat_decided(op, ia, ib, tie, tol, work)
    })
}

/// `decided` with near ties within `tie` taken as ties, and where its
/// decisions don't fit together ([`BooleanError::Inconsistent`]) and
/// `tie` isn't 0 already, again exactly, from the same `work` (see
/// [`flat_soup`]): the result is the last try's, its failure whole.
fn tied_or_exact<T>(
    tie: f64,
    work: &mut Work,
    mut decided: impl FnMut(f64, &mut Work) -> Result<T, Failure>,
) -> Result<T, Failure> {
    #[cfg(test)]
    if ONE_TRY.get() {
        return decided(tie, work);
    }
    match decided(tie, work) {
        Err(f) if f.error == KernelError::Boolean(BooleanError::Inconsistent) && tie > 0.0 => {
            decided(0.0, work)
        }
        result => result,
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
) -> Result<(cleanup::Soup, Vec<Face>), Failure> {
    let prims = flat::Flat::tied(ia, ib, op == Op::Union, tie);
    let counts = count::count(ia, ib, &prims, tol, work)?;
    let arcs = pairs::flat(ia, ib, &counts)?;
    // Flat pairs' arcs are straight edges, never traced.
    let rules = SeamRules::new(true);
    assemble::assemble(
        op, ia, ib, &counts, &arcs, &prims, tol, None, false, &mut false, &rules, work,
    )
}

/// Whether `a` and `b` touch or overlap: whether an edge of one crosses a
/// face of the other or a vertex of one is inside the other, with solids
/// that only touch (flush faces, an edge on a face) counted as touching.
/// Only the broad phase and one counting run, and not even those for
/// solids whose boxes (the control points', which hold them) are more
/// than the resolution apart.
///
/// With curved patches, a counting that shows no crossing and no vertex
/// inside is followed by a search for surfaces within the resolution
/// (`near::touching`): that finds what no edge crossing shows, a
/// tangency along a line (cylinders side by side, a pin against a hole's
/// wall) or a loop cut inside one patch. So curved solids within the
/// resolution of each other touch, and those up to about 2.4 times it
/// apart may (the search stops on two flat pieces whose hulls come
/// within it), where flat ones touch only within the tie distance; the
/// difference is below anything a user can place. It only picks what an
/// operation works on: every [`boolean`] decides for itself. Decisions
/// that don't fit together fail as [`BooleanError::Inconsistent`] with
/// what they are about, as the boolean's counting does.
pub fn touches(a: &Solid, b: &Solid, tol: &Tolerance, budget: &Budget) -> Result<bool, Failure> {
    touches_within(a, b, tol, &mut Work::new(budget))
}

/// [`touches`] within `work`. A counting whose decisions don't fit
/// together fails with what it is about (the exact one's, after the
/// flat retry).
fn touches_within(a: &Solid, b: &Solid, tol: &Tolerance, work: &mut Work) -> Result<bool, Failure> {
    let (Some(ba), Some(bb)) = (a.bounds3(), b.bounds3()) else {
        return Ok(false);
    };
    let gap = tol.resolution();
    if (ba.min - bb.max).max_element() > gap || (bb.min - ba.max).max_element() > gap {
        return Ok(false);
    }
    let (ia, ib) = (Input::new(a.mesh(), tol), Input::new(b.mesh(), tol));
    if ia.curved || ib.curved {
        return near::touching(&ia, &ib, tol, work);
    }
    // Counted again exactly where the near ties don't fit together, as
    // the operation does ([`flat_soup`]).
    let counts = tied_or_exact(tie(tol), work, |tie, work| {
        let prims = flat::Flat::tied(&ia, &ib, true, tie);
        count::count(&ia, &ib, &prims, tol, work)
    })?;
    Ok(counts.meet())
}

/// The mesh of the cleaned triangles: the vertices and faces no triangle
/// uses dropped (so chained booleans don't pile up faces long gone), the
/// rest numbered in order, each with the `aliases` of its source,
/// halfedges paired by vertex id. No triangles give the empty mesh.
///
/// Triangles that don't make a mesh fail as
/// [`BooleanError::Degenerate`] (too many, as
/// [`KernelError::TooComplex`]), with what the builder named
/// ([`built_evidence`]).
fn build(soup: cleanup::Soup, faces: Vec<Face>, aliases: &[Vec<FaceKey>]) -> Result<Mesh, Failure> {
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
    // Each vertex's id in the mesh, and each id's vertex of the soup.
    let mut soup_of = Vec::new();
    let id: Vec<u32> = (used.iter().zip(&soup.pos).enumerate())
        .map(|(i, (&used, &p))| {
            if !used {
                return u32::MAX;
            }
            soup_of.push(i as u32);
            builder.vert(p)
        })
        .collect();
    let face_id: Vec<u32> = used_faces
        .iter()
        .zip(faces)
        .zip(&soup.sources)
        .map(|((&used, f), &source)| {
            if !used {
                return u32::MAX;
            }
            let id = builder.face(f);
            for &key in &aliases[source as usize] {
                builder.alias(id, key);
            }
            id
        })
        .collect();
    for (tri, &face) in soup.tris.iter().zip(&soup.faces) {
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
        BuildError::TooManyPatches(_) => KernelError::TooComplex.into(),
        e => Failure {
            error: KernelError::Boolean(BooleanError::Degenerate),
            evidence: Box::new(built_evidence(&soup, &soup_of, e)),
        },
    })
}

/// What the mesh's builder named refusing `soup`'s triangles (`soup_of`:
/// each of its vertex ids' vertex of the soup): a triangle's sides and
/// corners, or a halfedge (two running one way, one running neither way
/// back, or a curve no triangle has as a side) and its ends. Sides as
/// their curves, where they have one, else straight; corners at one
/// place given once. Nothing for an index out of range.
fn built_evidence(soup: &cleanup::Soup, soup_of: &[u32], error: BuildError) -> Evidence {
    let mut evidence = Evidence::default();
    let side = |u: u32, v: u32| {
        let (p, q) = (soup.pos[u as usize], soup.pos[v as usize]);
        match soup.curves.get(&(u.min(v), u.max(v))) {
            Some(edge) => crate::patch::Conic3 {
                p0: p,
                c: edge.ctrl,
                w: edge.weight,
                p1: q,
            },
            None => segment(p, q),
        }
    };
    let in_soup = |v: u32| soup_of.get(v as usize).copied();
    let corners: Vec<u32> = match error {
        BuildError::Tri(t) => soup.tris.get(t).map(|t| t.to_vec()).unwrap_or_default(),
        BuildError::Duplicate(a, b) | BuildError::Open(a, b) | BuildError::UnusedEdge(a, b) => {
            match (in_soup(a), in_soup(b)) {
                (Some(u), Some(v)) => vec![u, v],
                _ => Vec::new(),
            }
        }
        BuildError::TooManyPatches(_) => Vec::new(),
    };
    if corners.iter().any(|&v| v as usize >= soup.pos.len()) {
        return evidence;
    }
    let n = corners.len();
    // A halfedge is one side; a triangle's three close.
    let sides = if n == 3 { 3 } else { n.saturating_sub(1) };
    evidence.add_curves((0..sides).map(|i| side(corners[i], corners[(i + 1) % n])));
    let mut points: Vec<DVec3> = Vec::with_capacity(n);
    for p in corners.iter().map(|&v| soup.pos[v as usize]) {
        if !points.contains(&p) {
            points.push(p);
        }
    }
    evidence.add_points(points);
    evidence
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
pub(crate) fn parts(n: usize, links: impl IntoIterator<Item = [u32; 2]>) -> Vec<u32> {
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

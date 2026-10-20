//! The snap's target and map: which operand a tangency cluster is
//! rewritten onto, and how its other sheet is carried there.
//!
//! The snapped sheet must be a **certified graph over the target**: on
//! every leaf its normals (reversed on Contact) lie in the open
//! hemisphere of the target's, and the map from it to the target is
//! injective. The candidates, the first that certifies taken
//! ([`choose`]):
//! 1. targets without a singular point (a pole or an apex) inside the
//!    cluster before those with one (the caller says which have one);
//! 2. then the first operand before the second;
//! 3. for each target, the **closest-point** map first, then projection
//!    along a **common direction** `ℓ`.
//!
//! **Closest point** ([`Map::Closest`]), so far onto a cylinder only
//! ([`CylinderSheet`]), and only onto **one smooth sheet**: one face, or
//! faces joined smoothly, which the caller names. Every target face of
//! the cluster must be one of the sheet's: a creased target (a box edge
//! under a sheet) fails, since every point of the outer wedge of a convex
//! crease has the crease as its nearest point and the map folds; it
//! falls through to `ℓ`. On the sheet:
//! - the curvature bound `b_K·κ_max < ½`, with `κ_max = 1/r` exactly on
//!   a cylinder of radius `r`: `2·b_K < r`, an exact comparison. Every
//!   point of a leaf lies within `b_K` of the cylinder, so within half
//!   its radius, where the nearest point (the foot) is unique;
//! - per leaf, the snapped piece is a graph over the target along the
//!   feet: `g·n_T(x) > 0` for every graph vector `g` of the piece (its
//!   normal cone's generators, reversed on Contact) and every point `x`
//!   of it, `n_T(x) = σ_A·ρ(x)` the target's outward normal at `x`'s foot,
//!   `ρ = |a|²·r − (a·r)·a` the radial part of `r = x − c` (`c` the foot
//!   point of the axis, `a` the axis as stored);
//! - per leaf, every foot lies in the sheet's interior: its axial
//!   coordinate `a·r` strictly inside the sheet's range and its angle
//!   strictly inside the sheet's wedge from `s` to `e` (counter-clockwise
//!   about `a`, at most a half turn): `r·(a × s) > 0` and `r·(e × a) >
//!   0`, since `ρ` differs from `|a|²·r` by a multiple of `a`.
//!
//! Each condition is linear in the piece's homogeneous net `(R, W)`
//! (`R = X − c·W`, `W > 0`), so it is a Bernstein form of the piece's
//! degrees, certified by a strict sign of every coefficient at the
//! leaf's precision level. A flat leaf never takes it (so far no flat
//! target offers one): a flat cluster takes `ℓ`, which on parallel
//! flush planes is their normal, the same map.
//!
//! **Common `ℓ`** ([`Map::Along`]): the max–min direction
//! ([`max_min`], exact; the least of the optimal directions, so the
//! solver's path doesn't matter) of all leaf normals of both sheets (the
//! leaves' cone generators, reversed on Contact for the second operand,
//! each scaled to a largest component of 1, sorted, repeats dropped).
//! It is accepted if its minimum is positive and it lies in every leaf's
//! admissible cone (`Cone::contains`, which also bounds the vertical gap
//! along `ℓ` over the leaf's pieces: exact on parallel plane pieces, so on
//! a flush cluster of several plane normals, a box edge or an L's
//! corner, a tilted `ℓ` is admitted; the condition is not convex). Leaves read at a precision level take
//! directions in `f64`: then `ℓ` is rounded to the nearest `f64`s and
//! the minimum checked again, exactly, on the rounded one. The normals
//! are a set, so no leaf order is read; `ℓ` doesn't depend on the target,
//! so it is computed once.
//!
//! **Work.** Charged on the operation's budget in passes: the leaves (an
//! item per leaf, a unit per leaf and per normal), the linear program
//! (one item, a unit per normal), each check over the leaves (an item per
//! leaf, a unit each, and on a flat leaf's cone a unit per sub-piece pair
//! and per program).

use std::cmp::Ordering;

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::{Scalar, Tensor};
use varde_decide::ids::FaceId;
use varde_decide::leaves::flat;
use varde_decide::leaves::near_quadric::{self, Quadric, SheetPiece};
use varde_decide::perturb::dir::{Q3, max_min};
use varde_decide::records::keys::Operand;

use super::cluster::{ClusterLeaf, Tangency};
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;

/// A smooth target sheet on a cylinder: faces on one quadric, joined
/// smoothly, and the part of the cylinder they cover (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct CylinderSheet {
    quadric: Quadric,
    /// Sorted, without repeats; never empty.
    faces: Vec<FaceId>,
    /// The wedge's start and end directions.
    wedge: [[f64; 3]; 2],
    /// The range of `a·(x − c)`.
    axial: [f64; 2],
}

impl CylinderSheet {
    /// The sheet of `faces` (of the target operand) on `quadric`,
    /// covering the points whose axial coordinate `a·(x − c)` (`a` the
    /// axis as stored, `c` its foot) lies in `axial` and whose direction
    /// from the axis lies in the wedge from `wedge[0]` counter-clockwise
    /// about `a` to `wedge[1]`. `None` unless there is a face, every value
    /// is finite, `axial` is increasing, neither wedge direction is along
    /// the axis and the wedge spans more than nothing and at most a half
    /// turn (decided exactly).
    pub fn new(
        quadric: Quadric,
        faces: impl IntoIterator<Item = FaceId>,
        wedge: [[f64; 3]; 2],
        axial: [f64; 2],
    ) -> Option<CylinderSheet> {
        let mut faces: Vec<FaceId> = faces.into_iter().collect();
        faces.sort_unstable();
        faces.dedup();
        let increasing = axial[0].partial_cmp(&axial[1]) == Some(Ordering::Less);
        if faces.is_empty() || !increasing || !axial.iter().all(|x| x.is_finite()) {
            return None;
        }
        let q = |v: [f64; 3]| -> Option<Q3> {
            let c = |x: f64| Rational::try_from(x).ok();
            Some([c(v[0])?, c(v[1])?, c(v[2])?])
        };
        let (a, s, e) = (q(quadric.axis())?, q(wedge[0])?, q(wedge[1])?);
        let (as_, ae) = (cross(&a, &s), cross(&a, &e));
        if is_zero(&as_) || is_zero(&ae) {
            return None;
        }
        // `(s × e)·a`: positive for less than a half turn; zero with the
        // two apart (opposite about the axis) for a half turn.
        let turn = dot(&cross(&s, &e), &a);
        let ok = match turn.cmp(&Rational::from(0)) {
            Ordering::Greater => true,
            Ordering::Equal => dot(&as_, &ae) < 0u32,
            Ordering::Less => false,
        };
        ok.then_some(CylinderSheet {
            quadric,
            faces,
            wedge,
            axial,
        })
    }

    /// The cylinder.
    pub fn quadric(&self) -> &Quadric {
        &self.quadric
    }

    /// Its faces, sorted.
    pub fn faces(&self) -> &[FaceId] {
        &self.faces
    }
}

/// A leaf of a tangency cluster as [`choose`] reads it.
pub trait TangentLeaf: ClusterLeaf {
    /// Whether it reads directions exactly (else in `f64`).
    fn exact(&self) -> bool;
    /// Its leaf normals, the generators of its admissible cone (the
    /// second operand's reversed on Contact).
    fn normals(&self) -> Vec<Q3>;
    /// Whether `ell` lies in its admissible cone, and the units of work
    /// the check took (at least one).
    fn admits(&self, ell: &Q3) -> (bool, u64);
    /// Whether the closest-point map of its piece of the other operand
    /// onto `sheet`, a sheet of operand `target`, certifies on it (module
    /// docs; the face check is the caller's).
    fn closest(&self, target: Operand, sheet: &CylinderSheet) -> bool;
}

impl TangentLeaf for flat::Leaf {
    fn exact(&self) -> bool {
        true
    }

    fn normals(&self) -> Vec<Q3> {
        match &self.kind {
            flat::Kind::Near(t) | flat::Kind::Contact(t) => t.cone.gens().to_vec(),
            flat::Kind::Empty { .. } | flat::Kind::Simple { .. } => Vec::new(),
        }
    }

    /// A unit per sub-piece pair bounded and per program run.
    fn admits(&self, ell: &Q3) -> (bool, u64) {
        match &self.kind {
            flat::Kind::Near(t) | flat::Kind::Contact(t) => {
                let mut w = flat::LeafWork::default();
                let ok = t.cone.contains(ell, &mut w);
                (ok, 1 + u64::from(w.gaps) + u64::from(w.programs))
            }
            flat::Kind::Empty { .. } | flat::Kind::Simple { .. } => (false, 1),
        }
    }

    fn closest(&self, _: Operand, _: &CylinderSheet) -> bool {
        false
    }
}

fn curved_tangent<S>(kind: &near_quadric::Kind<S>) -> Option<&near_quadric::Tangent<S>> {
    match kind {
        near_quadric::Kind::Near(t) | near_quadric::Kind::Contact(t) => Some(t),
        near_quadric::Kind::Empty(_) | near_quadric::Kind::Simple { .. } => None,
    }
}

impl<S: Scalar + Sync> TangentLeaf for near_quadric::Leaf<S> {
    fn exact(&self) -> bool {
        false
    }

    fn normals(&self) -> Vec<Q3> {
        let Some(t) = curved_tangent(&self.kind) else {
            return Vec::new();
        };
        t.cone
            .gens()
            .iter()
            .filter_map(|v| {
                let c = |x: f64| Rational::try_from(x).ok();
                Some([c(v[0])?, c(v[1])?, c(v[2])?])
            })
            .collect()
    }

    fn admits(&self, ell: &Q3) -> (bool, u64) {
        let Some(t) = curved_tangent(&self.kind) else {
            return (false, 1);
        };
        (t.cone.contains(&to_f64(ell)), 1)
    }

    fn closest(&self, target: Operand, sheet: &CylinderSheet) -> bool {
        let Some(t) = curved_tangent(&self.kind) else {
            return false;
        };
        let piece = t.cone.sheet();
        // The quadric face is the target, the sheet piece is snapped.
        if self.a.operand() != target || piece.quadric() != sheet.quadric() {
            return false;
        }
        // `b_K·κ_max < ½`, `κ_max = 1/r`: doubling is exact (an overflow
        // gives `∞`, which fails), and a NaN fails.
        let b_k = t.cone.b_k();
        if !(b_k >= 0.0 && 2.0 * b_k < sheet.quadric().radius()) {
            return false;
        }
        closest_on_cylinder(piece, t.cone.opposed(), sheet)
    }
}

/// The piece's graph and interior conditions for the closest-point map
/// onto `sheet` (module docs), certified at the piece's level.
fn closest_on_cylinder<S: Scalar>(
    piece: &SheetPiece<S>,
    opposed: bool,
    sheet: &CylinderSheet,
) -> bool {
    let (r, w) = piece.net();
    let Some(like) = w.coefs().first().cloned() else {
        return false;
    };
    let c = |x: f64| S::from_f64(x, &like);
    let lift = |v: [f64; 3]| v.map(c);
    let a = lift(sheet.quadric.axis());
    let aa = dot_s(&a, &a);
    let ar = lin(r, &a);
    // `s·σ_A·g·ρ > 0`, `g·ρ = |a|²·g·R − (a·g)·(a·R)` (times `W > 0`).
    let flip = opposed == piece.outward();
    let minus = S::int(1, &like).neg();
    let graph = piece.graph().iter().all(|g| {
        let f = lin(r, g).scale(&aa).sub(&ar.scale(&dot_s(&a, g)));
        positive(&if flip { f.scale(&minus) } else { f })
    });
    if !graph {
        return false;
    }
    // Axial: `lo·W < a·R < hi·W`.
    let [lo, hi] = sheet.axial;
    if !positive(&ar.sub(&w.scale(&c(lo)))) || !positive(&w.scale(&c(hi)).sub(&ar)) {
        return false;
    }
    // The wedge: `R·(a × s) > 0` and `R·(e × a) > 0`.
    let [s, e] = sheet.wedge.map(lift);
    positive(&lin(r, &cross_s(&a, &s))) && positive(&lin(r, &cross_s(&e, &a)))
}

/// How the snapped sheet is carried onto the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Map {
    /// Each point to its nearest point on the target's sheet.
    Closest,
    /// Each point along `ℓ`, the common direction (any positive multiple).
    Along(Q3),
}

/// A cluster's target and map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The operand the cluster is rewritten onto.
    pub operand: Operand,
    /// How the other operand's sheet is carried there.
    pub map: Map,
}

/// What the caller knows of an operand as a target.
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidate<'a> {
    /// Whether its faces have a singular point (a pole or an apex) inside
    /// the cluster.
    pub singular: bool,
    /// The smooth sheet its faces in the cluster lie on, if a closest
    /// point can be certified onto it.
    pub sheet: Option<&'a CylinderSheet>,
}

/// Why a target can't be chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetError {
    /// No leaves.
    Empty,
    /// More leaves than `u32` positions.
    TooMany,
    /// The leaf at this position isn't of the cluster's tangency.
    Kind(u32),
    /// The leaf at this position doesn't have a box of each operand.
    Operands(u32),
    /// The budget ran out ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// What the leaves pass reads of a leaf.
struct Read {
    normals: Vec<Q3>,
    exact: bool,
}

/// The target and map of the cluster of `leaves` (any order), all of
/// tangency `kind`, with `candidates[0]` for the first operand and
/// `candidates[1]` for the second (module docs); `None` if no candidate
/// certifies, for the escalation. Charged to `work`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the boolean's snap calls it as it lands")
)]
pub(crate) fn choose<L: TangentLeaf>(
    kind: Tangency,
    leaves: &[&L],
    candidates: [Candidate<'_>; 2],
    work: &mut Work,
) -> Result<Option<Target>, TargetError> {
    if leaves.is_empty() {
        return Err(TargetError::Empty);
    }
    let Ok(n) = u32::try_from(leaves.len()) else {
        return Err(TargetError::TooMany);
    };
    let all: Vec<u32> = (0..n).collect();
    let read = run(work, "snap target: leaves", &all, |&i, ledger| {
        units(ledger, 1)?;
        let l = leaves[i as usize];
        if l.tangency() != Some(kind) {
            return Err(Stop::Failed(TargetError::Kind(i)));
        }
        let [a, b] = l.boxes();
        if a.operand() == b.operand() {
            return Err(Stop::Failed(TargetError::Operands(i)));
        }
        let normals = l.normals();
        units(ledger, normals.len() as u64)?;
        Ok(Read {
            normals,
            exact: l.exact(),
        })
    })?;

    let mut order = [(Operand::A, candidates[0]), (Operand::B, candidates[1])];
    // Stable: the first operand first among equals.
    order.sort_by_key(|(_, c)| c.singular);
    let mut common: Option<Option<Q3>> = None;
    for (operand, candidate) in order {
        if let Some(sheet) = candidate.sheet
            && closest(leaves, operand, sheet, work)?
        {
            return Ok(Some(Target {
                operand,
                map: Map::Closest,
            }));
        }
        let ell = match &common {
            Some(ell) => ell.clone(),
            None => {
                let ell = common_direction(leaves, &read, work)?;
                common = Some(ell.clone());
                ell
            }
        };
        if let Some(ell) = ell {
            return Ok(Some(Target {
                operand,
                map: Map::Along(ell),
            }));
        }
    }
    Ok(None)
}

/// Whether the closest-point map onto `sheet` of operand `target`
/// certifies on every leaf (module docs).
fn closest<L: TangentLeaf>(
    leaves: &[&L],
    target: Operand,
    sheet: &CylinderSheet,
    work: &mut Work,
) -> Result<bool, TargetError> {
    let ok = run(work, "snap target: closest point", leaves, |l, ledger| {
        units(ledger, 1)?;
        let on_sheet = l
            .boxes()
            .iter()
            .filter(|b| b.operand() == target)
            .all(|b| sheet.faces.binary_search(&b.face()).is_ok());
        Ok(on_sheet && l.closest(target, sheet))
    })?;
    Ok(ok.into_iter().all(|x| x))
}

/// The common direction of the leaves, if one certifies (module docs).
fn common_direction<L: TangentLeaf>(
    leaves: &[&L],
    read: &[Read],
    work: &mut Work,
) -> Result<Option<Q3>, TargetError> {
    let mut normals: Vec<Q3> = read
        .iter()
        .flat_map(|r| r.normals.iter().map(scaled))
        .collect();
    normals.sort_unstable();
    normals.dedup();
    if normals.is_empty() || normals.iter().any(is_zero) {
        return Ok(None);
    }
    let exact = read.iter().all(|r| r.exact);
    let found = run(work, "snap target: common direction", &[()], |_, ledger| {
        units(ledger, normals.len() as u64)?;
        let (d, min) = max_min(&normals);
        if min <= 0u32 {
            return Ok(None);
        }
        if exact {
            return Ok(Some(d));
        }
        let ell = to_f64(&d).map(|x| Rational::try_from(x).expect("a rounded rational is finite"));
        let agrees = normals.iter().all(|n| dot(n, &ell) > 0u32);
        Ok(agrees.then_some(ell))
    })?;
    let Some(ell) = found.into_iter().next().flatten() else {
        return Ok(None);
    };
    let admitted = run(work, "snap target: cones", leaves, |l, ledger| {
        units(ledger, 1)?;
        let (ok, used) = l.admits(&ell);
        units(ledger, used.saturating_sub(1))?;
        Ok(ok)
    })?;
    Ok(admitted.into_iter().all(|x| x).then_some(ell))
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, TargetError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Ledger<'_>) -> Result<R, Stop<TargetError>> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, f)
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => TargetError::OutOfBudget(e),
        })
}

/// Charges `units` of plain work.
fn units(ledger: &mut Ledger<'_>, units: u64) -> Result<(), Stop<TargetError>> {
    ledger.charge(Step::Work, units).map_err(Stop::from)
}

/// Every coefficient strictly positive, certified.
fn positive<S: Scalar>(t: &Tensor<S>) -> bool {
    t.coefs()
        .iter()
        .all(|c| c.sign() == Some(Ordering::Greater))
}

/// `Σ t_k·v_k` of three forms of one degree.
fn lin<S: Scalar>(t: &[Tensor<S>; 3], v: &[S; 3]) -> Tensor<S> {
    t[0].scale(&v[0])
        .add(&t[1].scale(&v[1]))
        .add(&t[2].scale(&v[2]))
}

fn dot_s<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn cross_s<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    let c = |i: usize, j: usize| a[i].mul(&b[j]).sub(&a[j].mul(&b[i]));
    [c(1, 2), c(2, 0), c(0, 1)]
}

fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn cross(a: &Q3, b: &Q3) -> Q3 {
    let c = |i: usize, j: usize| &a[i] * &b[j] - &a[j] * &b[i];
    [c(1, 2), c(2, 0), c(0, 1)]
}

fn is_zero(v: &Q3) -> bool {
    v.iter().all(|x| *x == 0u32)
}

/// `v` scaled to a largest component of magnitude 1 (zero kept).
fn scaled(v: &Q3) -> Q3 {
    let m = v
        .iter()
        .map(|x| if *x < 0u32 { -x } else { x.clone() })
        .max()
        .unwrap_or_default();
    if m == 0u32 {
        v.clone()
    } else {
        v.clone().map(|x| x / &m)
    }
}

/// Each component rounded to the nearest `f64`.
fn to_f64(v: &Q3) -> [f64; 3] {
    v.each_ref().map(|x| f64::rounding_from(x, Nearest).0)
}

#[cfg(test)]
mod tests;

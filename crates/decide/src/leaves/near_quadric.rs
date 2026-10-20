//! Curved leaves against a quadric: the kind of a pair of pieces, one box
//! of a face lying on a quadric (so far a circular cylinder) and one box
//! of a polynomial sheet, certified at a precision level (or exactly).
//!
//! The quadric face has an implicit equation `F` ([`Quadric`]); the sheet
//! is a free-form patch. Every certificate reads `F` along the sheet,
//! never the quadric face's parametrisation, but for its hull.
//!
//! **The implicit along the sheet.** For the sheet's homogeneous net
//! `(X, W)` over the box (`x = X/W`, `W > 0`) and the cylinder's foot `c`,
//! axis `a` (as stored, not unit) and radius `ρ`, with `R = X − c·W`:
//! - `G = W²·F(x) = |a|²·R·R − (a·R)² − ρ²|a|²·W²`, a Bernstein form of
//!   twice the sheet's degrees; `F = |a|²·|r|² − (r·a)² − ρ²|a|²` at
//!   `r = p − c` is the kernel's cylinder implicit (zero on it, negative
//!   inside), so `G`'s sign is the side of the cylinder;
//! - `D̂ = W·∇F(x)·ℓ = 2(|a|²·R·ℓ − (a·ℓ)·(a·R))`, of the sheet's degrees;
//! - `Q = ℓᵀMℓ = |a|²|ℓ|² − (a·ℓ)² ≥ 0`, the constant half second
//!   derivative of `F` along `ℓ` (`F`'s Hessian `2M`, `M = |a|²I − aaᵀ`
//!   positive semidefinite): `F(x + sℓ) = F(x) + s·∇F(x)·ℓ + s²·Q`.
//!
//! Each form's coefficients bound it (the basis is non-negative and sums
//! to 1), so a sign of every coefficient is a sign of the form.
//!
//! **Orientation.** `σ_A = +1` when the quadric face's outward normal is
//! `∇F` (for a lattice tile of the cylinder, the face not flipped against
//! its tile: the tile's `x_u × x_v` points out), else `−1`; `σ_B = −1` on
//! a sheet flipped against its patch.
//!
//! **Kinds**, tried in this order at each pair, the first that certifies
//! taken ([`certify`]); a pair that certifies Empty or Simple is never
//! snapped:
//! 1. **Empty** ([`empty`]): the pieces' hull boxes are disjoint, or `G`
//!    has every coefficient of one strict sign (the sheet piece misses
//!    the whole cylinder: the T1 coefficients of one sign).
//! 2. **Simple** ([`simple`], no tangency): `G_u` (else `G_v`) has every
//!    coefficient of one strict sign, so `G = 0` (the intersection in the
//!    sheet's UV, `W > 0`) has no point with `G_u = 0` there: a smooth
//!    curve, a graph over `v`.
//! 3. **Near** ([`near`], co-oriented) and **Contact** ([`contact`],
//!    opposed): for a direction `ℓ`,
//!    - the sheet piece is a certified injective graph over `ℓ⊥` with its
//!      normals about `ℓ` (Contact: about `−ℓ`): `ℓ·g > 0`, certified
//!      strictly, for every **graph vector** `g = σ_B·(a × b)` (Contact:
//!      `−g`), `a` a `u`-hodograph vector and `b` a `v`-hodograph vector
//!      of the piece's net (differences of neighbouring control points);
//!      `x_u`, `x_v` are positive combinations of them, so every normal is
//!      a positive combination of the graph vectors (the hull of the
//!      hodographs' products: the sheet's certified normal cone), and the
//!      projection along `ℓ` has a Jacobian of one sign on the convex box;
//!    - the **T1 gap**: `|F(x)| ≤ t·min ∇F̃·ℓ` over `x ± t·ℓ` for every
//!      point `x` of the piece, `F̃ = σ_A·F`, `t` a certified lower bound
//!      of `b_K/|ℓ|` (so the segment is at most `b_K` long each way). For
//!      a quadric the minimum over the segment is exact,
//!      `∇F̃(x)·ℓ − 2tQ` (the derivative is linear in `s`), so with
//!      `mono = σ_A·D̂ − 2tQ·W` the certificate is: every coefficient of
//!      `mono` strictly positive (the margin, settled by the level), and
//!      every coefficient of `t·W·mono ∓ G` non-negative (`|G| ≤
//!      t·W·mono`, the gap). Then `F̃` grows along the segment and changes
//!      sign on it: the line through `x` along `ℓ` meets the cylinder at
//!      one point `y` within `t·|ℓ| ≤ b_K` of `x`, where `n_A·ℓ > 0`;
//!    - the **far root**: `F` along the line is a convex quadratic
//!      (`Q ≥ 0`), so its other zero `z` (if any) lies beyond the vertex,
//!      on the side `−σ_A·ℓ` at parameter `|s_z| ≥ min ∇F̃·ℓ / Q − t`. If
//!      every such `z` lies past the quadric piece's hull along `ℓ` (for
//!      `σ_A = +1`: `max ℓ·x + (t − min ∇F̃·ℓ/Q)·|ℓ|² < min ℓ·p` over the
//!      pieces, `x` the sheet's and `p` the quadric's, checked on their
//!      hull boxes in `f64` with outward rounding), then a line through
//!      the sheet piece along `ℓ` meets the quadric piece only at `y`:
//!      over the pieces' common projection on `ℓ⊥` both are graphs, the
//!      quadric piece's normals lie about `ℓ`, and the two-sided gap
//!      along `ℓ` is at most `b_K` (Contact: the signed gap). This takes
//!      the place of the quadric tile's wedge constraints, which would
//!      ask that every root lie in the piece and so never certify a
//!      sheet box whose roots straddle a box side of the quadric face.
//!
//! **Directions.** The leaf direction `ℓ` is proposed by the max–min
//! direction ([`crate::perturb::dir::max_min`], exact) of the **leaf
//! normals**: the sheet's graph vectors (Contact: negated) and `σ_A·∇F` at
//! the sheet piece's control points, as `f64` vectors (enclosure
//! midpoints) scaled to a largest component of 1 and rounded to
//! multiples of `2⁻²⁰` (a proposal only: the certificate is checked on
//! the level's values). It is taken if that minimum is positive, rounded
//! to the nearest `f64`s, and certifies. A direction is a function of the
//! pieces alone.
//!
//! **Admissible cone** ([`Cone`]): the directions a Near or Contact leaf
//! accepts in place of its own `ℓ`, those for which the three conditions
//! above certify ([`Cone::contains`], at the leaf's level). Scaling `ℓ`
//! keeps every condition but for the rounding of `t`. Its generators
//! ([`Cone::gens`]) are the leaf normals above, for a common direction to
//! read.
//!
//! **Leaves** ([`leaves`]): as the flat leaves ([`super::flat::leaves`]),
//! the same box choices ([`BoxUse`]), split rule ([`halve`]), order, work
//! count and stops. A pair a level can't certify splits like one that
//! certifies nothing: a caller that reaches the depth limit may run the
//! leaves again at a finer level.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::flat::{BoxUse, LeafError, LeafWork, PieceError, halve};
use crate::bezier::{Dir, HomPatch, HullBox, Scalar, Tensor};
use crate::count::boxes::{BoxKey, sub_patch};
use crate::num::bounded::{add_up, down, mul_up, up};
use crate::perturb::dir::{Q3, max_min};

/// The grid the proposal vectors are rounded to (module docs): `2⁻²⁰`.
const PROPOSAL_GRID: f64 = (1u64 << 20) as f64;

/// A quadric's implicit equation, in its stored values. So far the
/// circular cylinder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quadric {
    foot: [f64; 3],
    axis: [f64; 3],
    radius: f64,
}

impl Quadric {
    /// The cylinder of radius `radius` about the line through `foot` along
    /// `axis` (any length, used as given); `None` unless every value is
    /// finite, the axis non-zero and the radius positive.
    pub fn cylinder(foot: [f64; 3], axis: [f64; 3], radius: f64) -> Option<Quadric> {
        let finite = foot.iter().chain(&axis).all(|x| x.is_finite()) && radius.is_finite();
        (finite && radius > 0.0 && axis.iter().any(|&x| x != 0.0)).then_some(Quadric {
            foot,
            axis,
            radius,
        })
    }

    /// The implicit `F(p) = |a|²·|r|² − (r·a)² − ρ²·|a|²`, `r = p − foot`,
    /// `a` the axis as given, at `p`'s level (exactly for exact `p`): zero
    /// on the cylinder, negative inside.
    pub fn implicit<S: Scalar>(&self, p: &[S; 3]) -> S {
        let like = &p[0];
        let c = |x: f64| S::from_f64(x, like);
        let a = self.axis.map(c);
        let r: [S; 3] = [0, 1, 2].map(|k| p[k].sub(&c(self.foot[k])));
        let aa = dot(&a, &a);
        let ra = dot(&r, &a);
        let rho = c(self.radius);
        dot(&r, &r)
            .mul(&aa)
            .sub(&ra.mul(&ra))
            .sub(&rho.mul(&rho).mul(&aa))
    }

    /// `∇F(p) = 2(|a|²·r − (r·a)·a)` in `f64`, for proposals only.
    fn gradient(&self, p: [f64; 3]) -> [f64; 3] {
        let a = self.axis;
        let r = [0, 1, 2].map(|k| p[k] - self.foot[k]);
        let aa = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
        let ra = r[0] * a[0] + r[1] * a[1] + r[2] * a[2];
        [0, 1, 2].map(|k| 2.0 * (aa * r[k] - ra * a[k]))
    }
}

/// A face on a quadric as [`leaves`] reads it.
#[derive(Debug)]
pub struct QuadricFace<'a, S> {
    /// The quadric the face lies on.
    pub quadric: &'a Quadric,
    /// The tile's patch at the level (positive weights).
    pub patch: &'a HomPatch<S>,
    /// Whether the face's outward side is against `∇F` (for a lattice
    /// tile, against the tile's `x_u × x_v`, which points out).
    pub flipped: bool,
}

/// A polynomial sheet as [`leaves`] reads it.
#[derive(Debug)]
pub struct SheetFace<'a, S> {
    /// The tile's patch at the level: polynomial, every weight one
    /// positive value.
    pub patch: &'a HomPatch<S>,
    /// Whether the face's outward side is against the tile's `x_u × x_v`.
    pub flipped: bool,
}

// By hand: a derive would ask `S: Copy`.
impl<S> Clone for QuadricFace<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for QuadricFace<'_, S> {}

impl<S> Clone for SheetFace<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for SheetFace<'_, S> {}

/// The part of a quadric face over one box: its hull box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadricPiece {
    world: HullBox,
}

impl QuadricPiece {
    /// The piece of `face` over `b`.
    pub fn of_box<S: Scalar>(face: &QuadricFace<'_, S>, b: &BoxKey) -> Result<Self, PieceError> {
        let world = sub_patch(face.patch, b)
            .hull_box()
            .ok_or(PieceError::Weights)?;
        Ok(QuadricPiece { world })
    }

    /// The certified box holding the piece, `[lo, hi]`.
    pub fn world_box(&self) -> [[f64; 3]; 2] {
        [self.world.lo, self.world.hi]
    }
}

/// The part of a sheet over one box, with the implicit of a quadric face
/// along it (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPiece<S> {
    quadric: Quadric,
    /// `σ_A = +1`.
    outward: bool,
    w: Tensor<S>,
    r: [Tensor<S>; 3],
    g: Tensor<S>,
    /// The graph vectors, `σ_B` applied.
    graph: Vec<[S; 3]>,
    /// `σ_A·∇F` at the control points, in `f64`.
    normals: Vec<[f64; 3]>,
    world: HullBox,
}

impl<S: Scalar> SheetPiece<S> {
    /// The piece of `sheet` over `b`, against the quadric face `target`.
    pub fn of_box(
        target: &QuadricFace<'_, S>,
        sheet: &SheetFace<'_, S>,
        b: &BoxKey,
    ) -> Result<Self, PieceError> {
        let ws = sheet.patch.weights().coefs();
        let w0 = ws.first().ok_or(PieceError::Net)?;
        let positive = w0.enclose()[0] > 0.0;
        if !positive || ws.iter().any(|w| w != w0) {
            return Err(PieceError::Weights);
        }
        let (m, n) = sheet.patch.degrees();
        if m == 0 || n == 0 {
            return Err(PieceError::Net);
        }
        let sub = sub_patch(sheet.patch, b);
        let world = sub.hull_box().ok_or(PieceError::Weights)?;
        let [x, y, z, w] = sub.tensors().clone();
        let q = target.quadric;
        let like = w.coefs()[0].clone();
        let c = |v: f64| S::from_f64(v, &like);
        let r = [x, y, z]
            .iter()
            .zip(q.foot)
            .map(|(t, f)| t.sub(&w.scale(&c(f))))
            .collect::<Vec<_>>();
        let r: [Tensor<S>; 3] = [r[0].clone(), r[1].clone(), r[2].clone()];
        let a = q.axis.map(c);
        let aa = dot(&a, &a);
        let ar = lin(&r, &a);
        let rr = crate::bezier::dot(&r, &r).ok_or(PieceError::Net)?;
        let ww = w.mul(&w).ok_or(PieceError::Net)?;
        let rho = c(q.radius);
        let g = rr
            .scale(&aa)
            .sub(&ar.mul(&ar).ok_or(PieceError::Net)?)
            .sub(&ww.scale(&rho.mul(&rho).mul(&aa)));
        // The graph vectors from the homogeneous net: `W` is one positive
        // value over the box (a polynomial sheet), so `X`'s hodographs are
        // the point's times that value.
        let [x, y, z, _] = sub.tensors();
        let at = |i: usize, j: usize| [x.get(i, j), y.get(i, j), z.get(i, j)].map(S::clone);
        let diff = |p: [S; 3], q: [S; 3]| [0, 1, 2].map(|k| p[k].sub(&q[k]));
        let mut du = Vec::with_capacity(m * (n + 1));
        for i in 0..m {
            for j in 0..=n {
                du.push(diff(at(i + 1, j), at(i, j)));
            }
        }
        let mut dv = Vec::with_capacity((m + 1) * n);
        for i in 0..=m {
            for j in 0..n {
                dv.push(diff(at(i, j + 1), at(i, j)));
            }
        }
        let mut graph = Vec::with_capacity(du.len() * dv.len());
        for p in &du {
            for q in &dv {
                let g = cross(p, q);
                graph.push(if sheet.flipped { g.map(|x| x.neg()) } else { g });
            }
        }
        let outward = !target.flipped;
        let sigma = if outward { 1.0 } else { -1.0 };
        let normals = (0..=m)
            .flat_map(|i| (0..=n).map(move |j| (i, j)))
            .map(|(i, j)| {
                let wij = mid(sub.weights().get(i, j));
                let p = at(i, j).map(|v| mid(&v) / wij);
                q.gradient(p).map(|v| sigma * v)
            })
            .collect();
        Ok(SheetPiece {
            quadric: *q,
            outward,
            w,
            r,
            g,
            graph,
            normals,
            world,
        })
    }

    /// The certified box holding the piece, `[lo, hi]`.
    pub fn world_box(&self) -> [[f64; 3]; 2] {
        [self.world.lo, self.world.hi]
    }

    /// `G = W²·F` along the piece (module docs).
    pub fn implicit(&self) -> &Tensor<S> {
        &self.g
    }

    /// The graph vectors (module docs), `σ_B` applied, unscaled.
    pub fn graph(&self) -> &[[S; 3]] {
        &self.graph
    }
}

/// Why a pair is Empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separation {
    /// The pieces' hull boxes are disjoint.
    Boxes,
    /// The quadric's implicit has one strict sign on the sheet piece.
    Implicit,
}

/// The directions a Near or Contact leaf admits (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Cone<S> {
    gens: Vec<[f64; 3]>,
    sheet: SheetPiece<S>,
    target: HullBox,
    opposed: bool,
    b_k: f64,
}

impl<S: Scalar> Cone<S> {
    /// The leaf normals (module docs): the sheet's graph vectors (negated
    /// for Contact) and `σ_A·∇F` at its control points, each scaled to a
    /// largest component of 1 and rounded to multiples of `2⁻²⁰`, sorted,
    /// repeats and zeros dropped. For proposing directions only.
    pub fn gens(&self) -> &[[f64; 3]] {
        &self.gens
    }

    /// The gap bound it was certified with.
    pub fn b_k(&self) -> f64 {
        self.b_k
    }

    /// Whether `ell` is admitted, certified at the leaf's level: the
    /// sheet's graph certificate, the T1 gap and the far-root bound for
    /// `ell` (module docs).
    pub fn contains(&self, ell: &[f64; 3]) -> bool {
        admits(&self.sheet, &self.target, self.opposed, self.b_k, ell)
    }
}

/// A Near or Contact certificate: the leaf's direction and its cone.
#[derive(Clone, Debug, PartialEq)]
pub struct Tangent<S> {
    /// The leaf direction `ℓ`, in its cone.
    pub ell: [f64; 3],
    /// The directions the leaf admits.
    pub cone: Cone<S>,
}

/// A pair's certified kind (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum Kind<S> {
    /// The pieces are disjoint.
    Empty(Separation),
    /// No tangency: `G`'s derivative in `dir` has one strict sign.
    Simple {
        /// The sheet's parameter direction.
        dir: Dir,
    },
    /// Co-oriented, within `b_K` along `ℓ`.
    Near(Tangent<S>),
    /// Opposed, within `b_K` along `ℓ`.
    Contact(Tangent<S>),
}

/// Empty: why the pieces are disjoint, if certified (module docs).
pub fn empty<S: Scalar>(a: &QuadricPiece, b: &SheetPiece<S>) -> Option<Separation> {
    let (p, q) = (&a.world, &b.world);
    if (0..3).any(|k| p.hi[k] < q.lo[k] || q.hi[k] < p.lo[k]) {
        return Some(Separation::Boxes);
    }
    one_sign(b.g.coefs()).map(|_| Separation::Implicit)
}

/// Simple: the sheet direction along which `G` is strictly monotone, if
/// certified (module docs), `u` first.
pub fn simple<S: Scalar>(b: &SheetPiece<S>) -> Option<Dir> {
    [Dir::U, Dir::V]
        .into_iter()
        .find(|&d| one_sign(b.g.derivative(d).coefs()).is_some())
}

/// Near: `ℓ` and the cone, if certified (module docs).
pub fn near<S: Scalar>(
    a: &QuadricPiece,
    b: &SheetPiece<S>,
    b_k: f64,
    work: &mut LeafWork,
) -> Option<Tangent<S>> {
    tangent(a, b, false, b_k, work)
}

/// Contact: as [`near`] with the sheet's normals reversed.
pub fn contact<S: Scalar>(
    a: &QuadricPiece,
    b: &SheetPiece<S>,
    b_k: f64,
    work: &mut LeafWork,
) -> Option<Tangent<S>> {
    tangent(a, b, true, b_k, work)
}

/// The first kind that certifies, in the order Empty, Simple, Near,
/// Contact (module docs); `None` if none does at this level.
pub fn certify<S: Scalar>(
    a: &QuadricPiece,
    b: &SheetPiece<S>,
    b_k: f64,
    work: &mut LeafWork,
) -> Option<Kind<S>> {
    if let Some(s) = empty(a, b) {
        return Some(Kind::Empty(s));
    }
    if let Some(dir) = simple(b) {
        return Some(Kind::Simple { dir });
    }
    if let Some(t) = near(a, b, b_k, work) {
        return Some(Kind::Near(t));
    }
    contact(a, b, b_k, work).map(Kind::Contact)
}

fn tangent<S: Scalar>(
    a: &QuadricPiece,
    b: &SheetPiece<S>,
    opposed: bool,
    b_k: f64,
    work: &mut LeafWork,
) -> Option<Tangent<S>> {
    debug_assert!(b_k >= 0.0, "a negative gap bound");
    let gens = proposal_vectors(b, opposed);
    if gens.is_empty() {
        return None;
    }
    work.programs = work.programs.saturating_add(1);
    let qs: Vec<Q3> = gens
        .iter()
        .map(|v| v.map(|x| Rational::try_from(x).expect("a finite proposal")))
        .collect();
    let (d, min) = max_min(&qs);
    if min <= 0u32 {
        return None;
    }
    let ell = d.map(|x| f64::rounding_from(&x, Nearest).0);
    admits(b, &a.world, opposed, b_k, &ell).then(|| Tangent {
        ell,
        cone: Cone {
            gens,
            sheet: b.clone(),
            target: a.world,
            opposed,
            b_k,
        },
    })
}

/// The leaf normals as proposal vectors (module docs).
fn proposal_vectors<S: Scalar>(b: &SheetPiece<S>, opposed: bool) -> Vec<[f64; 3]> {
    let s = if opposed { -1.0 } else { 1.0 };
    let mut out: Vec<[f64; 3]> = b
        .graph
        .iter()
        .map(|g| g.each_ref().map(|x| s * mid(x)))
        .chain(b.normals.iter().copied())
        .filter_map(|v| {
            let m = v
                .iter()
                .fold(0.0, |m: f64, x| if x.abs() > m { x.abs() } else { m });
            if !(m > 0.0 && m.is_finite()) {
                return None;
            }
            let q = v.map(|x| (x / m * PROPOSAL_GRID).round() / PROPOSAL_GRID);
            q.iter().any(|&x| x != 0.0).then_some(q)
        })
        .collect();
    out.sort_by(|p, q| {
        (0..3)
            .map(|k| p[k].total_cmp(&q[k]))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    });
    out.dedup();
    out
}

/// Whether `ell` certifies for the pair: the sheet's graph vectors, the
/// T1 gap and the far root (module docs).
fn admits<S: Scalar>(
    b: &SheetPiece<S>,
    target: &HullBox,
    opposed: bool,
    b_k: f64,
    ell: &[f64; 3],
) -> bool {
    if !ell.iter().all(|x| x.is_finite()) || ell.iter().all(|&x| x == 0.0) {
        return false;
    }
    let like = b.w.coefs()[0].clone();
    let l = ell.map(|x| S::from_f64(x, &like));
    let want = if opposed {
        Ordering::Less
    } else {
        Ordering::Greater
    };
    if !b.graph.iter().all(|g| dot(g, &l).sign() == Some(want)) {
        return false;
    }
    let Some(t1) = t1_gap(b, &l, b_k, ell) else {
        return false;
    };
    far_root_clear(b, target, ell, &t1)
}

/// What the far-root bound reads of a certified T1 gap.
struct T1 {
    /// A certified lower bound of the segment's parameter half-length.
    t: f64,
    /// A lower bound of `min ∇F̃·ℓ` over the piece (0 if none positive
    /// is certified).
    d_min: f64,
    /// An upper bound of `Q`.
    q_hi: f64,
}

/// The T1 gap for `ell` (module docs), certified at the level.
fn t1_gap<S: Scalar>(b: &SheetPiece<S>, l: &[S; 3], b_k: f64, ell: &[f64; 3]) -> Option<T1> {
    let like = &l[0];
    let two = S::int(2, like);
    let len2 = ell
        .iter()
        .fold(0.0, |s, &x| add_up(s, mul_up(x.abs(), x.abs())));
    let len = up(len2.sqrt());
    let t = if b_k > 0.0 { down(b_k / len) } else { 0.0 };
    let t = if t > 0.0 { t } else { 0.0 };
    let ts = S::from_f64(t, like);
    let a = b.quadric.axis.map(|x| S::from_f64(x, like));
    let aa = dot(&a, &a);
    let al = dot(&a, l);
    let q = aa.mul(&dot(l, l)).sub(&al.mul(&al));
    let d = lin(&b.r, l)
        .scale(&aa)
        .sub(&lin(&b.r, &a).scale(&al))
        .scale(&two);
    let d = if b.outward { d } else { d.neg() };
    let mono = d.sub(&b.w.scale(&two.mul(&ts).mul(&q)));
    if !mono
        .coefs()
        .iter()
        .all(|c| c.sign() == Some(Ordering::Greater))
    {
        return None;
    }
    let tw = b.w.mul(&mono)?.scale(&ts);
    let within = |p: &Tensor<S>| {
        p.coefs()
            .iter()
            .all(|c| matches!(c.sign(), Some(Ordering::Greater | Ordering::Equal)))
    };
    if !within(&tw.sub(&b.g)) || !within(&tw.add(&b.g)) {
        return None;
    }
    let lo = d
        .coefs()
        .iter()
        .fold(f64::INFINITY, |m, c| least(m, c.enclose()[0]));
    let w_hi =
        b.w.coefs()
            .iter()
            .fold(0.0, |m: f64, c| greatest(m, c.enclose()[1]));
    let d_min = if lo > 0.0 && w_hi > 0.0 && w_hi.is_finite() {
        down(lo / w_hi)
    } else {
        0.0
    };
    Some(T1 {
        t,
        d_min: if d_min > 0.0 { d_min } else { 0.0 },
        q_hi: q.enclose()[1],
    })
}

/// Whether every other zero of `F` on a line through the sheet piece
/// along `ell` lies past the quadric piece (module docs), on the hull
/// boxes in `f64` rounded outward.
fn far_root_clear<S>(b: &SheetPiece<S>, target: &HullBox, ell: &[f64; 3], t1: &T1) -> bool {
    // The other zero lies on the side `−σ_A·ℓ`: below the sheet along
    // `ℓ` for `σ_A = +1`, so the quadric piece's least height must be
    // above it.
    let gap = if b.outward {
        down(height_lo(target, ell) - height_hi(&b.world, ell))
    } else {
        down(height_lo(&b.world, ell) - height_hi(target, ell))
    };
    let len2_hi = ell
        .iter()
        .fold(0.0, |s, &x| add_up(s, mul_up(x.abs(), x.abs())));
    let len2_lo = ell.iter().fold(0.0, |s, &x| down(s + down(x * x)));
    let margin = down(gap - mul_up(t1.t, len2_hi));
    if margin >= 0.0 {
        return true;
    }
    // `margin·Q + min ∇F̃·ℓ·|ℓ|² > 0`, `Q ≥ 0`.
    let q_hi = greatest(t1.q_hi, 0.0);
    down(t1.d_min * len2_lo) > mul_up(q_hi, -margin)
}

/// A lower bound of `min ℓ·p` over the box.
fn height_lo(b: &HullBox, ell: &[f64; 3]) -> f64 {
    (0..3).fold(0.0, |s, k| {
        let x = if ell[k] >= 0.0 { b.lo[k] } else { b.hi[k] };
        let term = if ell[k] == 0.0 { 0.0 } else { down(ell[k] * x) };
        down(s + term)
    })
}

/// An upper bound of `max ℓ·p` over the box.
fn height_hi(b: &HullBox, ell: &[f64; 3]) -> f64 {
    let neg = ell.map(|x| -x);
    -height_lo(b, &neg)
}

/// The certified leaves under the pair of boxes `a0` of the quadric face
/// `fa` and `b0` of the sheet `fb` (module docs), with gap bound `b_k`,
/// visiting at most `max_pairs` pairs; `use_box` says per box what to do
/// with it and must depend on the box alone. The leaves are ordered by
/// their boxes, and come with the work done.
pub fn leaves<S: Scalar>(
    fa: QuadricFace<'_, S>,
    a0: BoxKey,
    fb: SheetFace<'_, S>,
    b0: BoxKey,
    b_k: f64,
    max_pairs: u32,
    mut use_box: impl FnMut(&BoxKey) -> BoxUse,
) -> Result<(Vec<Leaf<S>>, LeafWork), LeafError> {
    let mut work = LeafWork::default();
    let mut out = Vec::new();
    let mut quadric_pieces: BTreeMap<BoxKey, QuadricPiece> = BTreeMap::new();
    let mut sheet_pieces: BTreeMap<BoxKey, SheetPiece<S>> = BTreeMap::new();
    let mut stack = vec![(a0, b0)];
    while let Some((a, b)) = stack.pop() {
        if work.pairs >= max_pairs {
            return Err(LeafError::Pairs(work));
        }
        work.pairs += 1;
        let (ua, ub) = (use_box(&a), use_box(&b));
        if ua == BoxUse::Drop || ub == BoxUse::Drop {
            continue;
        }
        let first = match (ua, ub) {
            (BoxUse::Split, _) => true,
            (_, BoxUse::Split) => false,
            _ => {
                if let Entry::Vacant(e) = quadric_pieces.entry(a) {
                    e.insert(QuadricPiece::of_box(&fa, &a).map_err(|e| LeafError::Piece(a, e))?);
                }
                if let Entry::Vacant(e) = sheet_pieces.entry(b) {
                    e.insert(SheetPiece::of_box(&fa, &fb, &b).map_err(|e| LeafError::Piece(b, e))?);
                }
                let (pa, pb) = (&quadric_pieces[&a], &sheet_pieces[&b]);
                if let Some(kind) = certify(pa, pb, b_k, &mut work) {
                    out.push(Leaf {
                        a,
                        b,
                        kind,
                        world: [pa.world_box(), pb.world_box()],
                    });
                    continue;
                }
                a.depth() <= b.depth()
            }
        };
        let halves = if first {
            halve(&a)
                .map(|h| h.map(|x| (x, b)))
                .or_else(|| halve(&b).map(|h| h.map(|y| (a, y))))
        } else {
            halve(&b)
                .map(|h| h.map(|y| (a, y)))
                .or_else(|| halve(&a).map(|h| h.map(|x| (x, b))))
        };
        let [lo, hi] = halves.ok_or_else(|| LeafError::Depth(Box::new([a, b])))?;
        stack.push(hi);
        stack.push(lo);
    }
    out.sort_by_key(|l| (l.a, l.b));
    Ok((out, work))
}

/// A certified pair of boxes.
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf<S> {
    /// The quadric face's box.
    pub a: BoxKey,
    /// The sheet's box.
    pub b: BoxKey,
    /// Its kind.
    pub kind: Kind<S>,
    /// The pieces' certified boxes, the quadric face's first, each
    /// `[lo, hi]`.
    pub world: [[[f64; 3]; 2]; 2],
}

/// The strict sign every value has, if one does (certified).
fn one_sign<S: Scalar>(cs: &[S]) -> Option<Ordering> {
    let first = cs.first()?.sign()?;
    (first != Ordering::Equal && cs.iter().all(|c| c.sign() == Some(first))).then_some(first)
}

/// `Σ t_k·v_k` of three forms of one degree.
fn lin<S: Scalar>(t: &[Tensor<S>; 3], v: &[S; 3]) -> Tensor<S> {
    t[0].scale(&v[0])
        .add(&t[1].scale(&v[1]))
        .add(&t[2].scale(&v[2]))
}

fn dot<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn cross<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    let c = |i: usize, j: usize| a[i].mul(&b[j]).sub(&a[j].mul(&b[i]));
    [c(1, 2), c(2, 0), c(0, 1)]
}

/// The midpoint of `x`'s enclosure, for proposals.
fn mid<S: Scalar>(x: &S) -> f64 {
    let [lo, hi] = x.enclose();
    lo * 0.5 + hi * 0.5
}

/// The lesser of two floats by comparison (a NaN `b` keeps `a`).
fn least(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// The greater of two floats by comparison (a NaN `b` keeps `a`).
fn greatest(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

#[cfg(test)]
mod tests;

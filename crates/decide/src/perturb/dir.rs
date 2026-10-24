//! The perturbation directions: the direction `d` each element of `A`
//! moves along in motion 1 (before `σ_A`), and the moved-vertex blend
//! between them.
//!
//! **Sign agreement.** A direction is valid at an element when `n·d > 0`
//! for the outward normal `n` of every face of `A` at the element. The
//! boundary near the element is then a graph along `d` (every line
//! parallel to `d` crosses it only from inside to outside), so the star
//! translated by `−λd` lies in `A`'s interior for `λ > 0`, at any valence,
//! convex or not. Any positive multiple of a valid direction is valid, so
//! no direction needs a square root.
//!
//! **Flat elements** (every face at the element a plane) take exact
//! rational directions, from the stored normals `n_f` scaled to a largest
//! component of magnitude 1 (`n̄_f = n_f/‖n_f‖∞`, [`scaled_normal`]), in
//! this order ([`flat`]):
//!
//! 1. **equal offsets**: `d` with `n̄_f·d = 1` on every face, the solution
//!    of least length (in the normals' span), where the system is
//!    solvable: always for one face (`n̄/|n̄|²`), two that aren't opposite
//!    and three with independent normals; with four or more only when the
//!    normals' tips lie on one plane. Every face then moves by the same
//!    offset, so a face moving along any blend of such directions stays a
//!    plane;
//! 2. else the **max–min** direction: `d` maximising `min_f n̄_f·d` over
//!    `‖d‖∞ ≤ 1`, by an exact simplex ([`max_min`]), if that minimum is
//!    `> 0`; of the maximising `d`, the least in Euclidean length (unique,
//!    found exactly on the simplex's optimal face), so the direction is
//!    the normals' own and moves with them under 90° rotations and
//!    mirrors of the axes, whatever vertex the simplex reaches;
//! 3. else a **saddle**: no direction agrees (the normals lie in no open
//!    half-space), and `d = 0`: the generic translations decide.
//!
//! Equal offsets agree by construction and the max–min direction by its
//! minimum, so every flat direction's sign agreement is exact
//! ([`agrees`]), and so is every order it gives a flat row: the rows take
//! it as an exact [`Rate`], whatever its size (a knife edge's equal
//! offsets grow as the faces close).
//!
//! **Work.** The exact solves are counted ([`DirWork`]: the small
//! systems solved, the constraint rows checked, the simplex's tableau
//! rows pivoted) for the caller to charge at the unit table's row
//! ([`DirectionUnits`]); [`max_min`] and [`equal_offsets`] alone are
//! the uncounted forms, for callers that charge a program as a whole.
//!
//! **Curved elements** keep unit directions at a precision level,
//! certified ([`Curved`]): a face point `n̂` ([`face`]); an edge point
//! `unit(n̂₁ + n̂₂)` of its faces' normals at the same point ([`edge`]); a
//! vertex the unit sum of its faces' unit normals weighted by their
//! corner angles if it agrees, else the max–min direction of the
//! normals' `f64` values if it agrees, else a certified saddle
//! ([`vertex`]). Each is one vector whatever the level, which only
//! certifies it (or, for a vertex, that a candidate fails), so records
//! decided at different levels read one field. What the level doesn't
//! settle (a near knife edge) escalates.
//!
//! **The moved-vertex blend.** A point `x` of vertex `a`'s star moves along
//! `d̂(x) = (1 − λ)·d(a) + λ·d(x)`, `λ = clamp(‖x − a‖/r_a − 1, 0, 1)`:
//! `a`'s direction within `r_a` (there a translation of `a`'s star), the
//! point's own element's beyond `2r_a`, a blend between, with no jump.
//! Both terms agree with the faces at `x`, which are faces of `a`'s star,
//! so the blend does. Flat ([`blend_flat`]): exact, with the max-norm
//! distance and a dyadic `r_a`, not normalised. Curved ([`blend`]): the
//! Euclidean distance, normalised, with `λ` taken once in `f64` from the
//! `f64` coordinates of `a` and of the record's point
//! ([`blend_weight`]), not from a precision level's values: a record
//! escalating to a finer level reads the same blend as at the first. At a saddle `d(a) = 0`: `d̂(x)` is zero
//! within `r_a` and a positive multiple of `x`'s own direction beyond (the
//! flat blend `λ·d(x)`, with no jump at `r_a`; the curved one `d(x)`
//! itself), the same signs as taking `x`'s own direction there. Every row
//! reads the field at one point (its record's), so no record straddles
//! `r_a`.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;

use super::{Curved, Rate, dot3, lit3, unit3};
use crate::bezier::Scalar;
use crate::num::schedule::DirectionUnits;
use crate::num::trig;

/// An exact vector.
pub type Q3 = [Rational; 3];

/// A flat element's direction ([`flat`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlatDir {
    /// `n̄_f·d = 1` on every face, the least such `d`.
    EqualOffsets(Q3),
    /// The max–min direction: `‖d‖∞ ≤ 1` and `min`, `min_f n̄_f·d > 0`, the
    /// largest such minimum; the least such `d`.
    MaxMin {
        /// The direction.
        d: Q3,
        /// Its least scaled offset.
        min: Rational,
    },
    /// No direction agrees with every face: `d = 0`.
    Saddle,
}

impl FlatDir {
    /// The direction; zero at a saddle.
    pub fn d(&self) -> Q3 {
        match self {
            FlatDir::EqualOffsets(d) | FlatDir::MaxMin { d, .. } => d.clone(),
            FlatDir::Saddle => zero(),
        }
    }

    /// The direction as a rate: exact, its denominators cleared
    /// ([`Rate::cleared`]).
    pub fn rate(&self) -> Rate {
        Rate::cleared(self.d())
    }
}

/// What a direction's exact solves took, for the caller to charge at
/// [`DirectionUnits`] ([`DirWork::parts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirWork {
    /// Small exact linear systems solved: an equal-offset system, a
    /// candidate point of a max–min program's optimal face, at most three
    /// unknowns each.
    pub solves: u32,
    /// Constraint rows checked against a point exactly (a dot product
    /// each): a candidate against the program, an edge of a group, a
    /// face's corner against a partner's cone.
    pub checks: u32,
    /// Tableau rows a simplex pivot updated (a pivot updates every row).
    pub pivots: u32,
}

impl DirWork {
    /// This work and `o`'s together.
    pub fn and(self, o: DirWork) -> DirWork {
        DirWork {
            solves: self.solves.saturating_add(o.solves),
            checks: self.checks.saturating_add(o.checks),
            pivots: self.pivots.saturating_add(o.pivots),
        }
    }

    /// This work in parts of a unit
    /// ([`UNIT_PARTS`](crate::num::schedule::UNIT_PARTS)) at `u`'s prices.
    pub fn parts(&self, u: &DirectionUnits) -> u64 {
        [
            (self.solves, u.solve),
            (self.checks, u.check),
            (self.pivots, u.pivot),
        ]
        .into_iter()
        .fold(0u64, |s, (n, p)| {
            s.saturating_add(u64::from(n).saturating_mul(p))
        })
    }

    fn solve(&mut self, rows: usize) {
        self.solves = self.solves.saturating_add(1);
        self.check(rows);
    }

    pub(crate) fn check(&mut self, rows: usize) {
        self.checks = self
            .checks
            .saturating_add(u32::try_from(rows).unwrap_or(u32::MAX));
    }
}

/// `v` exactly; `None` unless every component is finite.
pub fn exact(v: DVec3) -> Option<Q3> {
    let c = |x: f64| Rational::try_from(x).ok();
    Some([c(v.x)?, c(v.y)?, c(v.z)?])
}

/// `n/‖n‖∞`, exactly: the normal scaled to a largest component of
/// magnitude 1. `None` for a zero normal or one that isn't finite.
pub fn scaled_normal(n: DVec3) -> Option<Q3> {
    let n = exact(n)?;
    let m = n.iter().map(abs).max()?;
    if m == 0u32 {
        return None;
    }
    Some(n.map(|x| x / &m))
}

/// The direction of a flat element from its faces' stored outward
/// normals (module docs): equal offsets, else the max–min direction,
/// else a saddle. The order of the normals and repeats (parallel ones
/// alike) don't matter. `None` without a face or for a zero normal, or
/// one that isn't finite. The solves are counted in `work`.
pub fn flat(normals: &[DVec3], work: &mut DirWork) -> Option<FlatDir> {
    let mut ns = normals
        .iter()
        .map(|&n| scaled_normal(n))
        .collect::<Option<Vec<_>>>()?;
    if ns.is_empty() {
        return None;
    }
    ns.sort();
    ns.dedup();
    if let Some(d) = equal_offsets_counted(&ns, work) {
        return Some(FlatDir::EqualOffsets(d));
    }
    Some(max_min_dir(&ns, work).unwrap_or(FlatDir::Saddle))
}

/// The max–min direction alone of the faces' stored outward normals,
/// scaled to max-norm 1, equal ones once: [`FlatDir::MaxMin`] where its
/// minimum is `> 0`, else [`FlatDir::Saddle`] (no direction agrees with
/// every face). The direction of a group or a flush patch, which takes
/// the program whether or not equal offsets solve. `None` without a face
/// or for a zero normal, or one that isn't finite. Counted in `work`.
pub fn max_min_flat(normals: &[DVec3], work: &mut DirWork) -> Option<FlatDir> {
    let mut ns = normals
        .iter()
        .map(|&n| scaled_normal(n))
        .collect::<Option<Vec<_>>>()?;
    if ns.is_empty() {
        return None;
    }
    ns.sort();
    ns.dedup();
    Some(max_min_dir(&ns, work).unwrap_or(FlatDir::Saddle))
}

/// [`max_min_counted`] as a direction, if its minimum is `> 0`.
fn max_min_dir(ns: &[Q3], work: &mut DirWork) -> Option<FlatDir> {
    let (d, min) = max_min_counted(ns, work);
    (min > 0u32).then_some(FlatDir::MaxMin { d, min })
}

/// The least `d` with `n·d = 1` for every `n` of `ns` (non-zero), if
/// there is one (`least_on`); uncounted ([`equal_offsets_counted`]).
pub fn equal_offsets(ns: &[Q3]) -> Option<Q3> {
    equal_offsets_counted(ns, &mut DirWork::default())
}

/// [`equal_offsets`], its solve counted in `work`.
pub fn equal_offsets_counted(ns: &[Q3], work: &mut DirWork) -> Option<Q3> {
    let rows: Vec<Half> = ns.iter().map(|n| (n.clone(), Rational::from(1))).collect();
    least_on(&rows.iter().collect::<Vec<_>>(), work)
}

/// A constraint `a·d ≥ b` (or, in [`least_on`], `a·d = b`).
type Half = (Q3, Rational);

/// The first rows whose `a` are independent of those before: a basis of
/// their span.
fn basis<'a>(rows: &[&'a Half]) -> Vec<&'a Half> {
    let mut b: Vec<&Half> = Vec::new();
    for &row in rows {
        let n = &row.0;
        let independent = match b.as_slice() {
            [] => n.iter().any(|x| *x != 0u32),
            [b0] => cross(&b0.0, n).iter().any(|x| *x != 0u32),
            [b0, b1] => dot(&cross(&b0.0, &b1.0), n) != 0u32,
            _ => false,
        };
        if independent {
            b.push(row);
        }
    }
    b
}

/// The least `d` with `a·d = b` on every row, if there is one: the
/// solution in the span of a basis of the `a` (the Gram system
/// `G·c = b`, `d = Σ cⱼ·aⱼ`), checked against all the rows. No rows give
/// `d = 0`. One solve, its rows checked, counted in `work`.
fn least_on(rows: &[&Half], work: &mut DirWork) -> Option<Q3> {
    work.solve(rows.len());
    let b = basis(rows);
    let k = b.len();
    // `[G | b]`, eliminated without pivoting: `G` is positive definite.
    let mut m: Vec<Vec<Rational>> = b
        .iter()
        .map(|ri| {
            let mut row: Vec<Rational> = b.iter().map(|rj| dot(&ri.0, &rj.0)).collect();
            row.push(ri.1.clone());
            row
        })
        .collect();
    for c in 0..k {
        for r in c + 1..k {
            let f = &m[r][c] / &m[c][c];
            for j in c..=k {
                let t = &f * &m[c][j];
                m[r][j] -= t;
            }
        }
    }
    let mut coef = vec![Rational::default(); k];
    for c in (0..k).rev() {
        let mut v = m[c][k].clone();
        for j in c + 1..k {
            v -= &m[c][j] * &coef[j];
        }
        coef[c] = v / &m[c][c];
    }
    let mut d = zero();
    for (c, row) in coef.iter().zip(&b) {
        d = add(&d, &scale(&row.0, c));
    }
    rows.iter().all(|(a, v)| dot(a, &d) == *v).then_some(d)
}

/// The max–min direction of the normals `ns` (non-empty): `d` maximising
/// `min_n n·d` over `‖d‖∞ ≤ 1`, the least such `d`, and that minimum,
/// exactly ([`max_min_in`] on that box). The minimum is never below zero
/// (`d = 0`), and is zero exactly when no direction agrees. Uncounted
/// ([`max_min_counted`]).
pub fn max_min(ns: &[Q3]) -> (Q3, Rational) {
    max_min_counted(ns, &mut DirWork::default())
}

/// [`max_min`], its pivots and solves counted in `work`.
pub fn max_min_counted(ns: &[Q3], work: &mut DirWork) -> (Q3, Rational) {
    let one = || [0, 1, 2].map(|_| Rational::from(1));
    max_min_in(ns, &one().map(|x| -x), &one(), work)
}

/// `d` maximising `min_n n·d` over the box `lo ≤ d ≤ hi` (`lo ≤ hi`, a
/// coordinate fixed where they are equal), and that minimum `t*`,
/// exactly. Of all the maximising `d` (the optimal face `F`: `n·d ≥ t*`
/// for every `n`, in the box) the least in Euclidean length, which is
/// unique: so the result is that of the normals as a set, and moves with
/// them under the box's symmetries (signed permutations of the axes:
/// 90° rotations, mirrors), not a vertex the simplex happens to reach.
///
/// `t*` and the constraints tight on all of `F` (the equalities `E`)
/// come from `simplex`. The least point `x*` of `F` is the least point
/// of the affine hull of the face of `F` it lies inside, which is cut out
/// by `E` and at most `3 − rank E` more constraints tight there; so `x*`
/// is the least of the points `least_on` gives for `E` and each such
/// set that lie in `F` (each candidate lies in `F`, and `x*` is one).
/// Counted in `work`: the simplex's pivots, each candidate's solve and
/// check.
pub fn max_min_in(ns: &[Q3], lo: &Q3, hi: &Q3, work: &mut DirWork) -> (Q3, Rational) {
    let (t, tight) = simplex(ns, lo, hi, work);
    // `n·d ≥ t*`, then `−dᵢ ≥ −hiᵢ`, then `dᵢ ≥ loᵢ`.
    let unit = |i: usize, x: i32| {
        let mut a = zero();
        a[i] = Rational::from(x);
        a
    };
    let rows: Vec<Half> = ns
        .iter()
        .map(|n| (n.clone(), t.clone()))
        .chain((0..3).map(|i| (unit(i, -1), -&hi[i])))
        .chain((0..3).map(|i| (unit(i, 1), lo[i].clone())))
        .collect();
    let (eq, free): (Vec<_>, Vec<_>) = rows.iter().zip(&tight).partition(|(_, e)| **e);
    let eq: Vec<&Half> = eq.into_iter().map(|(r, _)| r).collect();
    let free: Vec<&Half> = free.into_iter().map(|(r, _)| r).collect();
    let dim = 3 - basis(&eq).len();
    let inside = |d: &Q3| rows.iter().all(|(a, b)| dot(a, d) >= *b);
    let mut best: Option<(Rational, Q3)> = None;
    let mut consider = |extra: &[&Half]| {
        let mut set = eq.clone();
        set.extend_from_slice(extra);
        let d = least_on(&set, work);
        if d.is_some() {
            work.check(rows.len());
        }
        if let Some(d) = d.filter(|d| inside(d)) {
            let len = dot(&d, &d);
            if best.as_ref().is_none_or(|(l, _)| len < *l) {
                best = Some((len, d));
            }
        }
    };
    consider(&[]);
    for i in 0..free.len() {
        if dim >= 1 {
            consider(&[free[i]]);
        }
        for j in i + 1..free.len() {
            if dim >= 2 {
                consider(&[free[i], free[j]]);
            }
            for k in j + 1..free.len() {
                if dim >= 3 {
                    consider(&[free[i], free[j], free[k]]);
                }
            }
        }
    }
    let (_, d) = best.expect("the optimal face has a least point");
    (d, t)
}

/// The linear program of [`max_min_in`]: maximise `t` with `n·d ≥ t` for
/// every `n` and `d` in the box, by the simplex method with Bland's rule
/// (so it ends, degenerate or not), on `y = d − lo ∈ [0, hi − lo]` and
/// `s = t + T₀ ≥ 0`, `T₀` the largest of `0` and the `−n·lo`, where the
/// origin is feasible. Returns `t*` and, for each constraint (`n·d ≥ t`
/// for each `n`, then `dᵢ ≤ hiᵢ`, then `dᵢ ≥ loᵢ`), whether it is tight on
/// every optimal point: at the end `s = s* − Σ c̄ⱼ·xⱼ` over the non-basic
/// variables with reduced costs `c̄ⱼ ≥ 0`, so a feasible point is optimal
/// exactly when every `xⱼ` with `c̄ⱼ > 0` is zero (a slack: its row tight;
/// a `yᵢ`: `dᵢ = loᵢ`). Each pivot counts its tableau's rows in `work`.
fn simplex(ns: &[Q3], lo: &Q3, hi: &Q3, work: &mut DirWork) -> (Rational, Vec<bool>) {
    let t0 = ns
        .iter()
        .map(|n| -dot(n, lo))
        .fold(Rational::default(), Rational::max);
    // Rows `−n·y + s ≤ T₀ + n·lo`, then `yᵢ ≤ hiᵢ − loᵢ`; columns
    // `y₀ y₁ y₂ s`, the slacks, the right-hand side.
    let rows = ns.len() + 3;
    let cols = 4 + rows + 1;
    let mut tab = vec![vec![Rational::default(); cols]; rows];
    for (r, n) in ns.iter().enumerate() {
        for i in 0..3 {
            tab[r][i] = -&n[i];
        }
        tab[r][3] = Rational::from(1);
        tab[r][cols - 1] = &t0 + dot(n, lo);
    }
    for i in 0..3 {
        let r = ns.len() + i;
        tab[r][i] = Rational::from(1);
        tab[r][cols - 1] = &hi[i] - &lo[i];
    }
    for (r, row) in tab.iter_mut().enumerate() {
        row[4 + r] = Rational::from(1);
    }
    // The objective row of `z − s = 0`.
    let mut obj = vec![Rational::default(); cols];
    obj[3] = Rational::from(-1);
    let mut basis: Vec<usize> = (4..4 + rows).collect();
    // Bland: the first column that improves, the leaving row by the least
    // ratio, ties to the least basic variable.
    while let Some(j) = (0..cols - 1).find(|&j| obj[j] < 0u32) {
        let mut leave: Option<(usize, Rational)> = None;
        for r in 0..rows {
            if tab[r][j] > 0u32 {
                let ratio = &tab[r][cols - 1] / &tab[r][j];
                let better = match &leave {
                    None => true,
                    Some((l, best)) => match ratio.cmp(best) {
                        Ordering::Less => true,
                        Ordering::Equal => basis[r] < basis[*l],
                        Ordering::Greater => false,
                    },
                };
                if better {
                    leave = Some((r, ratio));
                }
            }
        }
        // Bounded (`y` in the box, `s` by any face's row): some row always
        // limits the column.
        let Some((r, _)) = leave else { break };
        work.pivots = work
            .pivots
            .saturating_add(u32::try_from(rows + 1).unwrap_or(u32::MAX));
        let p = tab[r][j].clone();
        for x in &mut tab[r] {
            *x /= &p;
        }
        let pivot = tab[r].clone();
        for (k, row) in tab.iter_mut().enumerate() {
            if k != r && row[j] != 0u32 {
                let f = row[j].clone();
                for (x, y) in row.iter_mut().zip(&pivot) {
                    *x -= &f * y;
                }
            }
        }
        let f = obj[j].clone();
        for (x, y) in obj.iter_mut().zip(&pivot) {
            *x -= &f * y;
        }
        basis[r] = j;
    }
    // `z = s*` at the end; `t* = s* − T₀`.
    let t = &obj[cols - 1] - &t0;
    let tight = (0..rows)
        .map(|r| obj[4 + r] > 0u32)
        .chain((0..3).map(|i| obj[i] > 0u32))
        .collect();
    (t, tight)
}

/// Whether `d` agrees with every face, `n·d > 0` exactly for each stored
/// normal `n` (finite).
pub fn agrees(normals: &[DVec3], d: &Q3) -> bool {
    normals
        .iter()
        .all(|&n| exact(n).is_some_and(|n| dot(&n, d) > 0u32))
}

/// The blend weight `λ = clamp(‖x − a‖∞/r_a − 1, 0, 1)`, exactly, for a
/// finite blend radius `r_a > 0` (otherwise `1`: no blend, the element's
/// own direction).
pub fn weight_flat(a: &Q3, x: &Q3, r_a: f64) -> Rational {
    let dist = (0..3)
        .map(|i| abs(&(&x[i] - &a[i])))
        .max()
        .unwrap_or_default();
    let r = Rational::try_from(r_a).unwrap_or_default();
    if r <= 0u32 {
        return Rational::from(1);
    }
    let l = dist / r - Rational::from(1);
    l.clamp(Rational::from(0), Rational::from(1))
}

/// The moved-vertex blend on flat elements (module docs):
/// `(1 − λ)·d(a) + λ·d(x)`, `λ` from [`weight_flat`], exact and not
/// normalised. `d_a` is vertex `a`'s direction, `d_x` that of the element
/// `x` lies on (an edge or face of `a`'s star).
pub fn blend_flat(d_a: &Q3, d_x: &Q3, a: &Q3, x: &Q3, r_a: f64) -> Q3 {
    mix_flat(d_a, d_x, &weight_flat(a, x, r_a))
}

/// `(1 − λ)·d_a + λ·d_x` for a weight `λ` in `[0, 1]`, exactly: `d_a`
/// itself at `0`, `d_x` at `1`.
pub fn mix_flat(d_a: &Q3, d_x: &Q3, l: &Rational) -> Q3 {
    if *l == 0u32 {
        return d_a.clone();
    }
    if *l == 1u32 {
        return d_x.clone();
    }
    let k = Rational::from(1) - l;
    add(&scale(d_a, &k), &scale(d_x, l))
}

/// A curved element's direction at a precision level.
#[derive(Clone, Debug, PartialEq)]
pub enum CurvedDir<S> {
    /// A unit direction, its agreement certified.
    Dir([S; 3]),
    /// Certainly no direction agrees: `d = 0`.
    Saddle,
    /// Neither certified at this level (a near knife edge): refine in
    /// space or precision.
    Escalate,
}

/// Whether `n·d > 0` is certified for every `n`.
pub fn agrees_at<S: Curved>(d: &[S; 3], normals: &[[S; 3]]) -> bool {
    normals
        .iter()
        .all(|n| dot3(n, d).sign() == Some(Ordering::Greater))
}

/// `d` if it is certified to agree with `normals`, else to escalate.
fn certified<S: Curved>(d: [S; 3], normals: &[[S; 3]]) -> CurvedDir<S> {
    if agrees_at(&d, normals) {
        CurvedDir::Dir(d)
    } else {
        CurvedDir::Escalate
    }
}

/// A face point's direction, `n̂` from its normal `n` (any positive
/// multiple).
pub fn face<S: Curved>(n: &[S; 3]) -> CurvedDir<S> {
    let n = rescale(n);
    certified(unit3(&n), std::slice::from_ref(&n))
}

/// An edge point's direction from its two faces' normals at the point
/// (any positive multiples): `unit(n̂₁ + n̂₂)`, at which `d·n̂ᵢ =
/// √((1 + n̂₁·n̂₂)/2)`. A knife edge too sharp to certify escalates.
pub fn edge<S: Curved>(n1: &[S; 3], n2: &[S; 3]) -> CurvedDir<S> {
    let (n1, n2) = (rescale(n1), rescale(n2));
    let (u1, u2) = (unit3(&n1), unit3(&n2));
    let sum = [0, 1, 2].map(|k| u1[k].add(&u2[k]));
    certified(unit3(&rescale(&sum)), &[n1, n2])
}

/// A face at a curved vertex, for [`vertex`].
#[derive(Clone, Debug, PartialEq)]
pub struct Corner<S> {
    /// The face's normal at the vertex at the level (any positive
    /// multiple).
    pub normal: [S; 3],
    /// That normal in `f64`, the same at every level (any positive
    /// multiple, near the normal): the max–min candidate is taken from
    /// these.
    pub approx: DVec3,
    /// The corner angle (`> 0`; any positive weight gives a direction to
    /// certify, [`corner_angle`]).
    pub angle: f64,
}

/// Whether `n·d > 0` for every `n`: `Some(true)` certified, `Some(false)`
/// when some `n·d ≤ 0` certainly, `None` when the level settles neither.
pub(crate) fn verdict<S: Curved>(d: &[S; 3], normals: &[[S; 3]]) -> Option<bool> {
    let mut all = true;
    for n in normals {
        let v = dot3(n, d);
        if v.enclose()[1] <= 0.0 {
            return Some(false);
        }
        all &= v.sign() == Some(Ordering::Greater);
    }
    all.then_some(true)
}

/// The least `f64` margin `min_f n̂_f·ŝ` of the faces' `f64` unit normals
/// over their angle-weighted sum `ŝ` at which a vertex tries the sum
/// before the max–min direction ([`vertex`]): a sum nearer a face's plane
/// than that (or on it) is a poor direction, and one the levels may
/// never certify either way.
pub const SUM_MARGIN: f64 = 1.0 / (1u64 << 30) as f64;

/// A vertex's direction from its faces at the vertex: the first of two
/// candidates that agrees, each a fixed vector whatever the level, in an
/// order fixed by the `f64` normals ([`Corner::approx`]), so records
/// decided at different levels read one field; the level only certifies
/// whether a candidate agrees. The candidates: the unit angle-weighted
/// sum of the unit normals, and the max–min direction of the `f64`
/// normals, exact as [`max_min`] gives it and rounded to `f64`, if its
/// minimum is `> 0`; the sum first if its `f64` margin is at least
/// [`SUM_MARGIN`] and every angle is `> 0`. Neither agreeing: a saddle
/// when on every face of the box `‖d‖∞ ≤ 1` the max–min value of the
/// normals' enclosure midpoints is below zero by more than their
/// distance from the normals can account for. Where the level settles
/// neither whether a candidate agrees nor the saddle, escalate. The exact
/// programs are counted in `work`.
pub fn vertex<S: Curved>(star: &[Corner<S>], work: &mut DirWork) -> CurvedDir<S> {
    let Some(like) = star.first().map(|c| c.normal[0].clone()) else {
        return CurvedDir::Escalate;
    };
    let normals: Vec<[S; 3]> = star.iter().map(|c| rescale(&c.normal)).collect();
    let weighted = star.iter().all(|c| c.angle.is_finite() && c.angle > 0.0);
    // The two candidates' order, from the `f64` normals alone.
    let units: Vec<DVec3> = star
        .iter()
        .map(|c| {
            let v = c.approx / max_abs([c.approx.x, c.approx.y, c.approx.z]);
            v / v.length()
        })
        .collect();
    let sum_f = units
        .iter()
        .zip(star)
        .fold(DVec3::ZERO, |s, (u, c)| s + *u * c.angle);
    // Comparisons, so a NaN (a zero `f64` normal) puts the sum second.
    let sum_first = weighted
        && units
            .iter()
            .all(|u| u.dot(sum_f) / sum_f.length() >= SUM_MARGIN);
    for take_sum in [sum_first, !sum_first] {
        let d = if take_sum {
            if !weighted {
                continue;
            }
            let zero = S::int(0, &like);
            let mut sum = [zero.clone(), zero.clone(), zero];
            for (n, c) in normals.iter().zip(star) {
                let u = unit3(n);
                let w = S::from_f64(c.angle, &like);
                sum = [0, 1, 2].map(|k| sum[k].add(&u[k].mul(&w)));
            }
            unit3(&rescale(&sum))
        } else {
            let approx: Option<Vec<Q3>> = star.iter().map(|c| scaled_normal(c.approx)).collect();
            let Some((d, min)) = approx.map(|ns| max_min_counted(&ns, work)) else {
                continue;
            };
            if min <= 0u32 {
                continue;
            }
            let rounded = d.each_ref().map(|x| f64::rounding_from(x, Nearest).0);
            unit3(&lit3(DVec3::from_array(rounded), &like))
        };
        match verdict(&d, &normals) {
            Some(true) => return CurvedDir::Dir(d),
            Some(false) => {}
            None => return CurvedDir::Escalate,
        }
    }
    if certified_saddle(&normals, work) {
        CurvedDir::Saddle
    } else {
        CurvedDir::Escalate
    }
}

/// Whether no direction agrees with `normals` (curved, at a level),
/// certified: on every face of the box `‖d‖∞ ≤ 1` the max–min value of
/// the normals' enclosure midpoints is below zero by more than their
/// distance from the normals can account for. `false` where the level
/// doesn't settle it. The programs are counted in `work`.
pub(crate) fn certified_saddle<S: Curved>(normals: &[[S; 3]], work: &mut DirWork) -> bool {
    // The midpoints `m` and their distances `e` from the unit normals,
    // as exact rationals.
    let mut mids = Vec::with_capacity(normals.len());
    let mut spread = Rational::default();
    for n in normals {
        let u = unit3(n);
        let mut m = zero();
        let mut e = Rational::default();
        for k in 0..3 {
            let [lo, hi] = u[k].enclose();
            let (Ok(lo), Ok(hi)) = (Rational::try_from(lo), Rational::try_from(hi)) else {
                return false;
            };
            m[k] = (&lo + &hi) / Rational::from(2);
            e += (hi - lo) / Rational::from(2);
        }
        let c = m.iter().map(abs).max().unwrap_or_default();
        if c == 0u32 {
            return false;
        }
        // Scaled to max-norm 1, as is its distance.
        mids.push(m.map(|x| x / &c));
        spread = spread.max(e / c);
    }
    // On each face of the box, `n·d ≤ m·d + ‖n − m‖₁` (each scaled): its
    // max–min value below `−spread` leaves no `d` there with every
    // `n·d > 0`, and every direction is a positive multiple of one there.
    let faces = (0..3).flat_map(|i| [-1, 1].map(|s| (i, Rational::from(s))));
    let mut saddle = true;
    for (i, side) in faces {
        let (mut lo, mut hi) = (
            zero().map(|_| Rational::from(-1)),
            zero().map(|_| Rational::from(1)),
        );
        lo[i] = side.clone();
        hi[i] = side;
        let (min, _) = simplex(&mids, &lo, &hi, work);
        saddle &= min + &spread < 0u32;
    }
    saddle
}

/// The corner angle between a face's two edge tangents `a` and `b` at a
/// vertex, in `f64` (through `libm`, the same bits everywhere): a weight
/// for [`vertex`], which certifies the direction it gives, not the angle.
pub fn corner_angle(a: DVec3, b: DVec3) -> f64 {
    trig::atan2(a.cross(b).length(), a.dot(b))
}

/// The curved blend weight `λ = clamp(‖x − a‖/r_a − 1, 0, 1)` in `f64`,
/// from the `f64` coordinates of vertex `a` and of the record's point
/// `x` (the same at every precision level, so records decided at
/// different levels read one field; any `λ` in `[0, 1]` keeps agreement,
/// so its rounding doesn't matter): `0` at `‖x − a‖ ≤ r_a` and where the
/// distance isn't a number, `1` from `2r_a` on, and for a blend radius
/// that isn't finite and `> 0` (no blend, the element's own direction,
/// as [`weight_flat`]).
pub fn blend_weight(a: DVec3, x: DVec3, r_a: f64) -> f64 {
    if !(r_a > 0.0 && r_a.is_finite()) {
        return 1.0;
    }
    let v = x - a;
    let dist = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    let l = dist / r_a - 1.0;
    // Compare and select: a NaN gives `0`.
    if l >= 1.0 {
        1.0
    } else if l > 0.0 {
        l
    } else {
        0.0
    }
}

/// The moved-vertex blend on curved elements (module docs):
/// `unit((1 − λ)·d(a) + λ·d(x))` at a level for the weight `λ` in
/// `[0, 1]` ([`blend_weight`]): `d(a)` alone where `λ` is 0 (also a
/// saddle's zero), `d(x)` where it is 1.
pub fn blend<S: Curved>(d_a: &[S; 3], d_x: &[S; 3], l: f64) -> [S; 3] {
    if l <= 0.0 {
        return d_a.clone();
    }
    if l >= 1.0 {
        return d_x.clone();
    }
    let like = &d_a[0];
    let (k, l) = (S::from_f64(1.0 - l, like), S::from_f64(l, like));
    let sum = [0, 1, 2].map(|i| d_a[i].mul(&k).add(&d_x[i].mul(&l)));
    unit3(&sum)
}

/// The direction of a flat ribbon point at `s` across the ribbon
/// (module docs): `(1 − s)·n̄_f + s·d_e`, exact and not normalised, with
/// `n̄_f` the face's stored normal scaled to max-norm 1 ([`scaled_normal`])
/// and `d_e` the edge's direction there; `s` is clamped to `[0, 1]`.
/// `None` for a zero normal or one that isn't finite.
pub fn ribbon_flat(normal: DVec3, d_e: &Q3, s: &Rational) -> Option<Q3> {
    let n = scaled_normal(normal)?;
    let s = s.clone().clamp(Rational::from(0), Rational::from(1));
    let k = Rational::from(1) - &s;
    Some(add(&scale(&n, &k), &scale(d_e, &s)))
}

/// The direction of a curved ribbon point at `s` across the ribbon
/// (module docs): `unit((1 − s)·n̂_f + s·d_e)` at a level, `n̂_f` the unit
/// normal from the face's normal `n` there (any positive multiple) and
/// `d_e` the edge's unit direction there, `s` in `f64` (clamped to `[0,
/// 1]`, a NaN taken as 0), the same at every level. Uncertified where the
/// blend can't be normalised (`d_e` nearly `−n̂_f`, which no agreeing
/// edge direction is).
pub fn ribbon<S: Curved>(n: &[S; 3], d_e: &[S; 3], s: f64) -> [S; 3] {
    let s = if s >= 1.0 {
        1.0
    } else if s > 0.0 {
        s
    } else {
        0.0
    };
    blend(&unit3(&rescale(n)), d_e, s)
}

/// `v` times a power of two bringing its largest component near 1
/// (exactly where the level allows): a positive multiple, so no direction
/// or sign changes, without the overflow or underflow of squaring a huge
/// or tiny vector. `v` itself where the level doesn't give its size as a
/// positive `f64`.
pub(crate) fn rescale<S: Scalar>(v: &[S; 3]) -> [S; 3] {
    let m = max_abs(v.iter().flat_map(|x| x.enclose()));
    if !(m > 0.0 && m.is_finite()) {
        return v.clone();
    }
    // `m` in `[2^e, 2^(e+1))`; `2^−e` in two factors, each a float.
    let bits = m.to_bits();
    let raw = (bits >> 52) as i32;
    let e = if raw == 0 {
        63 - bits.leading_zeros() as i32 - 1074
    } else {
        raw - 1023
    };
    let pow2 = |k: i32| f64::from_bits(((k + 1023) as u64) << 52);
    let (a, b) = (-e / 2, -e - -e / 2);
    let (a, b) = (S::from_f64(pow2(a), &v[0]), S::from_f64(pow2(b), &v[0]));
    [0, 1, 2].map(|k| v[k].mul(&a).mul(&b))
}

/// The largest magnitude, by compare and select: NaN if any is.
fn max_abs(xs: impl IntoIterator<Item = f64>) -> f64 {
    let mut m = 0.0;
    for x in xs {
        let a = x.abs();
        if a.is_nan() {
            return a;
        }
        if a > m {
            m = a;
        }
    }
    m
}

pub(crate) fn zero() -> Q3 {
    [0, 1, 2].map(|_| Rational::default())
}

fn abs(x: &Rational) -> Rational {
    if *x < 0u32 { -x } else { x.clone() }
}

pub(crate) fn dot(a: &Q3, b: &Q3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

pub(crate) fn cross(a: &Q3, b: &Q3) -> Q3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

pub(crate) fn add(a: &Q3, b: &Q3) -> Q3 {
    [0, 1, 2].map(|i| &a[i] + &b[i])
}

pub(crate) fn scale(a: &Q3, s: &Rational) -> Q3 {
    [0, 1, 2].map(|i| &a[i] * s)
}

#[cfg(test)]
mod tests;

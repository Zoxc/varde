//! What a solved sketch's equations say about it: the degrees of freedom
//! left, what they fix, and which are redundant.

use std::collections::BTreeSet;

use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::householder;
use faer::{Conj, Mat, Par};
use serde::{Deserialize, Serialize};

use crate::{Id, Sketch};

use super::equation::Slot;
use super::system::{Fixing, System};
use super::{Jacobian, TOLERANCE, scale, within};

/// A diagonal entry of the factored Jacobian this small, relative to the
/// largest (or to one, if that's smaller), counts as zero: the equation it
/// stands for depends on the others.
const RANK_TOLERANCE: f64 = 1e-9;

/// A variable whose direction has a part at most this long in the
/// Jacobian's null space is fixed: no motion the equations allow moves it.
const FIXED_TOLERANCE: f64 = 1e-6;

/// An equation whose weight in a dependency is at most this is not part
/// of it.
const DEPENDENCE_TOLERANCE: f64 = 1e-8;

/// What the constraints do to a sketch, from [`analyse`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    /// Degrees of freedom left: the variables (two per point, one per
    /// circle's radius) less the independent equations on them. Zero is
    /// fully constrained.
    pub freedom: usize,
    /// The points and curves the constraints fix: every variable of theirs
    /// fixed, a curve's points included.
    pub fixed: BTreeSet<Id>,
    /// The constraints and driving dimensions (and arcs, for the equation
    /// each implies) in a dependency among the equations: each could be told from the others,
    /// so together they're redundant, or, where they don't hold, in
    /// conflict. Empty for a sketch whose every constraint counts.
    pub redundant: BTreeSet<Id>,
    /// Whether every equation holds, within the solver's tolerance.
    pub solved: bool,
}

/// Analyses `sketch`, which is to have passed [`Sketch::check`] and is
/// usually solved, at its points and radii. Unlike solving, what a
/// [`Constraint::Fix`](crate::Constraint::Fix) pins is variables held by
/// equations of its own, so a `Fix` can be named as redundant too.
///
/// Each component's Jacobian is factored (QR with column pivoting of its
/// transpose): its rank takes from the degrees of freedom, a variable is
/// fixed when the null space leaves it out, and the dependent equations
/// with those they depend on, the support of the left null space, are
/// redundant.
pub fn analyse(sketch: &Sketch) -> Analysis {
    let system = System::new(sketch, Fixing::Equations);
    let tolerance = TOLERANCE * scale(sketch);
    let solved = system
        .equations
        .iter()
        .all(|equation| within(equation.residual.value(&system.values), tolerance));
    let (components, constant) = system.components();
    // An equation of constants alone adds nothing. There are none while
    // what's fixed is variables, but origin and axes may be constants.
    let mut redundant: BTreeSet<Id> = constant
        .iter()
        .map(|&index| &system.equations[index])
        .filter(|equation| !equation.implied)
        .map(|equation| equation.source)
        .collect();
    let mut freedom = system.values.len();
    let mut fixed = vec![false; system.values.len()];
    for component in &components {
        // Where a derivative isn't a number, it's taken as zero rather
        // than analysing nothing.
        let jacobian = Jacobian::new(&system, component, None).dense(component.vars.len());
        let implied: Vec<bool> = component
            .equations
            .iter()
            .map(|&index| system.equations[index].implied)
            .collect();
        let dependencies = counted_dependencies(&jacobian, &implied);
        freedom -= dependencies.rank;
        for (&var, &is_fixed) in component.vars.iter().zip(&dependencies.fixed) {
            fixed[var] = is_fixed;
        }
        for (&index, &involved) in component.equations.iter().zip(&dependencies.involved) {
            if involved {
                redundant.insert(system.equations[index].source);
            }
        }
    }

    let slot_fixed = |slot: Slot| match slot {
        Slot::Var(var) => fixed[var],
        Slot::Const(_) => true,
    };
    let point_fixed = |slots: [Slot; 2]| slots.into_iter().all(slot_fixed);
    let mut fixed_items = BTreeSet::new();
    for (point, &slots) in sketch.points.iter().zip(&system.points) {
        if point_fixed(slots) {
            fixed_items.insert(point.id);
        }
    }
    for (entry, radius) in sketch.curves.iter().zip(&system.radii) {
        let points = entry
            .curve
            .points()
            .all(|id| system.point(sketch, id).is_some_and(point_fixed));
        if points && radius.is_none_or(slot_fixed) {
            fixed_items.insert(entry.id);
        }
    }
    Analysis {
        freedom,
        fixed: fixed_items,
        redundant,
        solved,
    }
}

/// What a Jacobian's factorization says, by row and column.
pub(crate) struct Dependencies {
    /// The number of independent rows.
    pub rank: usize,
    /// Per column: no motion keeping the rows' linearized equations moves
    /// its variable.
    pub fixed: Vec<bool>,
    /// Per row: part of a linear dependency among the rows.
    pub involved: Vec<bool>,
}

/// The [`dependencies`] of the rows of `jacobian` but those `implied`
/// (see [`Equation::implied`](super::equation::Equation::implied)) that
/// depend on the rest, each taken, in order, only where it adds to the
/// rank of those taken before it. Those left out are in no dependency.
pub(crate) fn counted_dependencies(jacobian: &Mat<f64>, implied: &[bool]) -> Dependencies {
    if !implied.contains(&true) {
        return dependencies(jacobian);
    }
    let (rows, columns) = jacobian.shape();
    let rank_of = |taken: &[bool]| {
        let chosen: Vec<usize> = (0..rows).filter(|&row| taken[row]).collect();
        let rows = Mat::from_fn(chosen.len(), columns, |i, j| jacobian[(chosen[i], j)]);
        dependencies(&rows).rank
    };
    let mut taken: Vec<bool> = implied.iter().map(|&implied| !implied).collect();
    let mut rank = rank_of(&taken);
    for row in (0..rows).filter(|&row| implied[row]) {
        taken[row] = true;
        match rank_of(&taken) {
            more if more > rank => rank = more,
            _ => taken[row] = false,
        }
    }
    let chosen: Vec<usize> = (0..rows).filter(|&row| taken[row]).collect();
    let kept = Mat::from_fn(chosen.len(), columns, |i, j| jacobian[(chosen[i], j)]);
    let found = dependencies(&kept);
    let mut involved = vec![false; rows];
    for (&row, &is_involved) in chosen.iter().zip(&found.involved) {
        involved[row] = is_involved;
    }
    Dependencies { involved, ..found }
}

/// Factors `jacobian`'s transpose, `Jᵀ P = Q R`, with column pivoting,
/// which puts the rows of `J` that depend on those before them last. The
/// rank is the number of diagonal entries of `R` above zero; the
/// columns of `Q` past the rank span `J`'s null space, which a fixed
/// variable has no part in; and each dependent row is a combination of the independent
/// ones, `R₁₁⁻¹ R₁₂`, whose rows with a weight in it are involved too.
pub(crate) fn dependencies(jacobian: &Mat<f64>) -> Dependencies {
    let (rows, columns) = jacobian.shape();
    let mut involved = vec![false; rows];
    if rows == 0 {
        return Dependencies {
            rank: 0,
            fixed: vec![false; columns],
            involved,
        };
    }
    let qr = jacobian.transpose().col_piv_qr();
    let r = qr.thin_R();
    let size = r.nrows();
    let largest = if size == 0 { 0.0 } else { r[(0, 0)].abs() };
    let threshold = RANK_TOLERANCE * largest.max(1.0);
    let rank = (0..size)
        .take_while(|&i| r[(i, i)].abs() > threshold)
        .count();

    // Only the null space's columns of `Q`: few, unless much is free.
    let free = columns - rank;
    let mut null = Mat::zeros(columns, free);
    for k in 0..free {
        null[(rank + k, k)] = 1.0;
    }
    let scratch = householder::apply_block_householder_sequence_on_the_left_in_place_scratch::<f64>(
        columns,
        qr.Q_coeff().nrows(),
        free,
    );
    householder::apply_block_householder_sequence_on_the_left_in_place_with_conj(
        qr.Q_basis(),
        qr.Q_coeff(),
        Conj::No,
        null.as_mut(),
        Par::Seq,
        MemStack::new(&mut MemBuffer::new(scratch)),
    );
    let fixed = (0..columns)
        .map(|var| {
            let part: f64 = (0..free).map(|k| null[(var, k)] * null[(var, k)]).sum();
            part.sqrt() <= FIXED_TOLERANCE
        })
        .collect();

    let (order, _) = qr.P().arrays();
    for &row in &order[rank..] {
        involved[row] = true;
    }
    if rank < rows && rank > 0 {
        let mut weights = r.get(..rank, rank..).to_owned();
        faer::linalg::triangular_solve::solve_upper_triangular_in_place(
            r.get(..rank, ..rank),
            weights.as_mut(),
            Par::Seq,
        );
        for (i, &row) in order[..rank].iter().enumerate() {
            if (0..rows - rank).any(|k| weights[(i, k)].abs() > DEPENDENCE_TOLERANCE) {
                involved[row] = true;
            }
        }
    }
    Dependencies {
        rank,
        fixed,
        involved,
    }
}

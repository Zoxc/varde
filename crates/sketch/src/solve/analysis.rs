//! What a solved sketch's equations say about it: the degrees of freedom
//! left, what they fix, the sizes they set, and which are redundant.

use std::collections::BTreeSet;

use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::householder;
use faer::{Conj, Mat, MatRef, Par};
use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{Constraint, Curve, Id, Kind, Measure, OffsetPair, Sketch};

use super::equation::{PointSlots, Slot};
use super::jacobian::Jacobian;
use super::system::{Component, Fixing, System};
use super::{TOLERANCE, scale, within};

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
    /// The lines, circles and arcs whose size (a line's length, a
    /// circle's or arc's radius) no motion the equations allow changes,
    /// wherever it leaves them: every curve in `fixed` but a spline,
    /// and those the constraints size but leave free to move.
    pub sized: BTreeSet<Id>,
    /// The constraints and driving dimensions (and arcs, for the equation
    /// each implies) in a dependency among the equations: each could be told from the others,
    /// so together they're redundant, or, where they don't hold, in
    /// conflict. Empty for a sketch whose every constraint counts.
    pub redundant: BTreeSet<Id>,
    /// Whether every equation holds, within the solver's tolerance.
    pub solved: bool,
}

impl Analysis {
    /// Whether this could be the analysis of `sketch`: at most as many
    /// degrees of freedom as it has variables
    /// ([`Sketch::most_variables`]), what's fixed its points and curves,
    /// what's sized its lines, circles and arcs, and what's redundant
    /// items equations come from ([`Sketch::equation_sources`]). A check
    /// on an analysis made elsewhere, such as a web worker's.
    pub fn fits(&self, sketch: &Sketch) -> bool {
        let fixed = |id| matches!(sketch.kind(id), Some(kind) if kind != Kind::Constraint);
        let sized = |id| matches!(sketch.kind(id), Some(Kind::Line | Kind::Circle | Kind::Arc));
        sketch
            .most_variables()
            .is_some_and(|variables| self.freedom <= variables)
            && self.fixed.iter().all(|&id| fixed(id))
            && self.sized.iter().all(|&id| sized(id))
            && sketch.equation_sources(&self.redundant)
    }
}

impl Sketch {
    /// How many variables the solver has for the sketch at most: its
    /// points' coordinates, circles' radii, and a parameter for each point
    /// on a spline or offset from one. Fewer where links hold some as
    /// constants. `None` if that overflows.
    pub fn most_variables(&self) -> Option<usize> {
        let circles = (self.curves.iter())
            .filter(|entry| matches!(entry.curve, Curve::Circle { .. }))
            .count();
        let from_spline = |pair: [Id; 2]| self.offset_pair(pair) == Some(OffsetPair::Spline);
        let offsets = self.dimensions.iter().filter(
            |entry| matches!(entry.dimension.measure, Measure::Offset(a, b) if from_spline([a, b])),
        );
        let on_splines = (self.constraints.iter())
            .map(|entry| match entry.constraint {
                Constraint::PointOnCurve { curve, .. } => {
                    usize::from(self.kind(curve) == Some(Kind::Spline))
                }
                Constraint::EqualOffset { a, b } => {
                    [a, b].into_iter().filter(|&p| from_spline(p)).count()
                }
                _ => 0,
            })
            .try_fold(offsets.count(), usize::checked_add)?;
        self.points
            .len()
            .checked_mul(2)?
            .checked_add(circles)?
            .checked_add(on_splines)
    }

    /// Whether `ids` are all items of the sketch equations come from:
    /// constraints, arcs, fillets and chamfers, and dimensions.
    pub fn equation_sources<'a>(&self, ids: impl IntoIterator<Item = &'a Id>) -> bool {
        ids.into_iter().all(|&id| {
            matches!(
                self.kind(id),
                Some(Kind::Constraint | Kind::Arc | Kind::Dimension)
            ) || self.curve(id).is_some_and(|entry| entry.corner.is_some())
        })
    }
}

/// Analyses `sketch`, which is to have passed [`Sketch::check`] and is
/// usually solved, at its points and radii. Unlike solving, what a
/// [`Constraint::Fix`] pins is variables held by equations of its own,
/// so a `Fix` can be named as redundant too.
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
    let mut motions = Motions {
        place: vec![None; system.values.len()],
        nulls: Vec::with_capacity(components.len()),
        fixed: vec![false; system.values.len()],
    };
    for (number, component) in components.iter().enumerate() {
        // Where a derivative isn't a number, it's taken as zero rather
        // than analysing nothing.
        let dependencies =
            component.dependencies(&system, &Jacobian::new(&system, component, None));
        freedom -= dependencies.rank;
        for (column, (&var, &is_fixed)) in
            component.vars.iter().zip(&dependencies.fixed).enumerate()
        {
            motions.fixed[var] = is_fixed;
            motions.place[var] = Some((number, column));
        }
        for (&index, &involved) in component.equations.iter().zip(&dependencies.involved) {
            if involved {
                redundant.insert(system.equations[index].source);
            }
        }
        motions.nulls.push(dependencies.null);
    }

    let mut fixed = BTreeSet::new();
    let mut sized = BTreeSet::new();
    for (point, &slots) in sketch.points.iter().zip(&system.points) {
        if motions.point_fixed(slots) {
            fixed.insert(point.id);
        }
    }
    for (entry, &radius) in sketch.curves.iter().zip(&system.radii) {
        let points = entry.curve.points().all(|id| {
            system
                .point(sketch, id)
                .is_some_and(|p| motions.point_fixed(p))
        });
        if points && radius.is_none_or(|slot| motions.slot_fixed(slot)) {
            fixed.insert(entry.id);
        }
        if size_gradient(&system, sketch, &entry.curve, radius)
            .is_some_and(|gradient| motions.keeps(&gradient))
        {
            sized.insert(entry.id);
        }
    }
    Analysis {
        freedom,
        fixed,
        sized,
        redundant,
        solved,
    }
}

/// The motions a sketch's equations allow, by component.
struct Motions {
    /// Each variable's component and column in it, if an equation reads
    /// it.
    place: Vec<Option<(usize, usize)>>,
    /// The components' null spaces, the motions their equations allow, a
    /// row per column.
    nulls: Vec<Mat<f64>>,
    /// Per variable: no motion allowed moves it.
    fixed: Vec<bool>,
}

impl Motions {
    /// Whether no motion allowed changes a quantity of this gradient, as a
    /// size set: none of it on a variable no equation reads, and in each
    /// component, its projection on the null space next to nothing.
    fn keeps(&self, gradient: &[(Slot, f64)]) -> bool {
        let mut parts: Vec<Vec<(usize, f64)>> = vec![Vec::new(); self.nulls.len()];
        let mut length = 0.0;
        for &(slot, weight) in gradient {
            let Slot::Var(var) = slot else { continue };
            length += weight * weight;
            match self.place[var] {
                Some((number, column)) => parts[number].push((column, weight)),
                None => return false,
            }
        }
        let mut part = 0.0;
        for (null, entries) in self.nulls.iter().zip(&parts) {
            for k in 0..null.ncols() {
                let along: f64 = entries
                    .iter()
                    .map(|&(column, weight)| null[(column, k)] * weight)
                    .sum();
                part += along * along;
            }
        }
        part.sqrt() <= FIXED_TOLERANCE * length.sqrt().max(1.0)
    }

    /// Whether no motion allowed moves `slot`: a constant, or a fixed
    /// variable.
    fn slot_fixed(&self, slot: Slot) -> bool {
        match slot {
            Slot::Var(var) => self.fixed[var],
            Slot::Const(_) => true,
        }
    }

    /// Whether no motion allowed moves the point of `slots`.
    fn point_fixed(&self, slots: PointSlots) -> bool {
        slots.into_iter().all(|slot| self.slot_fixed(slot))
    }
}

/// The gradient of the size of `curve` (with `radius`, a circle's): a
/// line's length, a circle's or an arc's radius. `None` for a spline, or
/// a line or an arc whose points meet.
fn size_gradient(
    system: &System,
    sketch: &Sketch,
    curve: &Curve,
    radius: Option<Slot>,
) -> Option<Vec<(Slot, f64)>> {
    // A distance's gradient: along the unit from `b` to `a` on `a`,
    // against it on `b`; none where they meet.
    let distance = |a: Id, b: Id| {
        let (a, b) = (system.point(sketch, a)?, system.point(sketch, b)?);
        let value = |slot: Slot| system.value(slot);
        let d = DVec2::new(value(a[0]) - value(b[0]), value(a[1]) - value(b[1]));
        d.try_normalize()
            .map(|u| vec![(a[0], u.x), (a[1], u.y), (b[0], -u.x), (b[1], -u.y)])
    };
    match *curve {
        Curve::Line { start, end } => distance(start, end),
        Curve::Circle { .. } => radius.map(|slot| vec![(slot, 1.0)]),
        Curve::Arc { center, start, .. } => distance(start, center),
        Curve::Spline(_) => None,
    }
}

/// What a Jacobian's factorization says, by row and column.
pub(super) struct Dependencies {
    /// The number of independent rows.
    pub rank: usize,
    /// Per column: no motion keeping the rows' linearized equations moves
    /// its variable.
    pub fixed: Vec<bool>,
    /// Per row: part of a linear dependency among the rows.
    pub involved: Vec<bool>,
    /// The null space: the motions keeping the rows' linearized
    /// equations, an orthonormal basis of them as columns, a row per
    /// column of the Jacobian.
    pub null: Mat<f64>,
}

impl Component {
    /// The [`counted_dependencies`] of the component's
    /// equations, from their `jacobian` at the system's values.
    pub(super) fn dependencies(&self, system: &System, jacobian: &Jacobian) -> Dependencies {
        let implied: Vec<bool> = self
            .equations
            .iter()
            .map(|&index| system.equations[index].implied)
            .collect();
        counted_dependencies(&jacobian.dense(self.vars.len()), &implied)
    }
}

/// The [`dependencies`] of the rows of `jacobian` but those `implied`
/// (see [`Equation::implied`](super::equation::Equation::implied)) that
/// depend on the rest, each taken, in order, only where it adds to the
/// rank of those taken before it. Those left out are in no dependency.
pub(super) fn counted_dependencies(jacobian: &Mat<f64>, implied: &[bool]) -> Dependencies {
    if !implied.contains(&true) {
        return dependencies(jacobian);
    }
    let rows = jacobian.nrows();
    let rank_of = |taken: &[bool]| rank(&rows_of(jacobian, taken).1);
    let mut taken: Vec<bool> = implied.iter().map(|&implied| !implied).collect();
    let mut rank = rank_of(&taken);
    for row in (0..rows).filter(|&row| implied[row]) {
        taken[row] = true;
        match rank_of(&taken) {
            more if more > rank => rank = more,
            _ => taken[row] = false,
        }
    }
    let (chosen, kept) = rows_of(jacobian, &taken);
    let found = dependencies(&kept);
    let mut involved = vec![false; rows];
    for (&row, &is_involved) in chosen.iter().zip(&found.involved) {
        involved[row] = is_involved;
    }
    Dependencies { involved, ..found }
}

/// The rows of `jacobian` `taken`, by index and as a matrix of them.
fn rows_of(jacobian: &Mat<f64>, taken: &[bool]) -> (Vec<usize>, Mat<f64>) {
    let (rows, columns) = jacobian.shape();
    let chosen: Vec<usize> = (0..rows).filter(|&row| taken[row]).collect();
    let matrix = Mat::from_fn(chosen.len(), columns, |i, j| jacobian[(chosen[i], j)]);
    (chosen, matrix)
}

/// The number of independent rows of `jacobian`, as [`dependencies`]
/// counts them, without the rest of its work.
fn rank(jacobian: &Mat<f64>) -> usize {
    if jacobian.nrows() == 0 {
        return 0;
    }
    rank_of_r(jacobian.transpose().col_piv_qr().thin_R())
}

/// The rank a pivoted QR factorization's `R` shows: its diagonal entries
/// above zero, relative to the largest.
fn rank_of_r(r: MatRef<'_, f64>) -> usize {
    let size = r.nrows();
    let largest = if size == 0 { 0.0 } else { r[(0, 0)].abs() };
    let threshold = RANK_TOLERANCE * largest.max(1.0);
    (0..size)
        .take_while(|&i| r[(i, i)].abs() > threshold)
        .count()
}

/// Factors `jacobian`'s transpose, `Jᵀ P = Q R`, with column pivoting,
/// which puts the rows of `J` that depend on those before them last. The
/// rank is the number of diagonal entries of `R` above zero; the
/// columns of `Q` past the rank span `J`'s null space, which a fixed
/// variable has no part in; and each dependent row is a combination of the independent
/// ones, `R₁₁⁻¹ R₁₂`, whose rows with a weight in it are involved too.
fn dependencies(jacobian: &Mat<f64>) -> Dependencies {
    let (rows, columns) = jacobian.shape();
    let mut involved = vec![false; rows];
    if rows == 0 {
        return Dependencies {
            rank: 0,
            fixed: vec![false; columns],
            involved,
            null: Mat::identity(columns, columns),
        };
    }
    let qr = jacobian.transpose().col_piv_qr();
    let r = qr.thin_R();
    let rank = rank_of_r(r);

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
        null,
    }
}

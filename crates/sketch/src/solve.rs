//! The solver: moves a sketch's points and radii until every constraint
//! holds, moving them as little as it can, and the analysis of a solved
//! sketch (degrees of freedom, what's fixed, redundant constraints).
//!
//! Pure functions of the sketch, so they run wherever they're called: in
//! `update`, a thread or a Web Worker.
//!
//! The variables are the points' coordinates and the circles' radii,
//! anything a [`Constraint::Fix`](crate::Constraint::Fix) pins being a
//! constant instead. Each constraint gives one or two equations
//! (`equation`), each driving dimension one, and each arc one of its own,
//! its ends at the same distance from its centre. Variables and equations split into connected
//! components (`system`), solved one by one with a damped Gauss-Newton
//! (Levenberg-Marquardt) step of least norm, so a sketch that's partly
//! free moves as little as it can; one already solved takes no step.

mod analysis;
mod equation;
mod normal;
mod real;
mod system;

pub use analysis::{Analysis, analyse};

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use faer::Mat;
use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{Curve, Id, Sketch};

use equation::{PointSlots, Slot, SplineSlots};
use normal::Normal;
use system::{Component, Fixing, System};

/// The largest residual a solved sketch keeps, relative to its size
/// ([`scale`]): a nanometre on a sketch ten metres across.
const TOLERANCE: f64 = 1e-10;

/// How much more a dragged variable weighs than the rest in the step of
/// least norm: moving it costs as much as moving another a thousand times
/// as far, so the rest follows the drag and it stays near its target.
const DRAG_WEIGHT: f64 = 1e6;

/// The Levenberg-Marquardt damping: the factor it starts at, multiplied
/// by ten after a step that fails and divided by ten after one that
/// succeeds, within its bounds. Past `DAMPING_MAX` no step helps.
const DAMPING_START: f64 = 1e-9;
const DAMPING_MIN: f64 = 1e-12;
const DAMPING_MAX: f64 = 1e12;

/// The least damping, relative to the largest diagonal entry of the
/// normal matrix, which keeps it positive definite when equations are
/// redundant.
const DAMPING_FLOOR: f64 = 1e-12;

/// A step lowering the squared residuals by less than this fraction is
/// stuck: the equations can't all hold from here.
const STALL: f64 = 1e-9;

/// The iterations [`Budget::default`] allows.
pub const DEFAULT_ITERATIONS: u32 = 100;

/// What solving may spend before giving up.
#[derive(Clone, Copy)]
pub struct Budget<'a> {
    /// The most steps, across all components.
    pub iterations: u32,
    /// Asked before each step; `true` stops the solve with
    /// [`Failure::OutOfTime`]. The caller keeps the clock, since the web
    /// has no `std::time::Instant`.
    pub expired: &'a dyn Fn() -> bool,
}

impl Default for Budget<'_> {
    /// [`DEFAULT_ITERATIONS`] and no time limit.
    fn default() -> Self {
        fn never() -> bool {
            false
        }
        Budget {
            iterations: DEFAULT_ITERATIONS,
            expired: &never,
        }
    }
}

/// What to solve for.
#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    /// Every constraint held, from where the sketch is.
    Settle,
    /// Points (and circles' radii) dragged towards targets, every
    /// constraint still held: each dragged one is put at its target and
    /// weighs heavily, so the rest moves to accommodate it and it moves
    /// only as far as the constraints force it, ending at about the
    /// nearest place they allow. Only the components the drag touches are
    /// solved. Something fixed doesn't move; ids naming no point (or
    /// circle) are ignored.
    Drag {
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
    },
}

/// A solved sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// The sketch with its points and radii moved, everything else as it
    /// was.
    pub sketch: Sketch,
    /// The steps taken, across all components: zero for a sketch that
    /// was already solved.
    pub iterations: u32,
}

/// Why [`solve`] gave up. It never returns a sketch solved only in part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    /// The equations can't all hold from where the sketch is, within the
    /// iterations allowed: constraints that conflict, or a place the
    /// constraints can't reach, such as a tangency's mirror image.
    /// `involved` names the constraints and driving dimensions (and arcs,
    /// for the equation each implies) in the conflict, as far as it can be told: those in a
    /// dependency among the equations where the solve stopped, or else
    /// those left unsatisfied. A [`Constraint::Fix`](crate::Constraint::Fix)
    /// is never among them: solving, what's fixed is a constant.
    NotConverged { involved: BTreeSet<Id> },
    /// [`Budget::expired`] said so.
    OutOfTime,
    /// A number stopped being one (a line of no length being made
    /// parallel, a target that isn't finite), or a circle's radius fell to
    /// zero or below. `involved` as for `NotConverged`.
    Degenerate { involved: BTreeSet<Id> },
}

/// The size tolerances are relative to: the largest coordinate or radius,
/// at least a micrometre.
fn scale(sketch: &Sketch) -> f64 {
    let coordinates = sketch
        .points
        .iter()
        .flat_map(|point| point.at.abs().to_array());
    let radii = sketch.curves.iter().filter_map(|entry| match entry.curve {
        Curve::Circle { radius, .. } => Some(radius.abs()),
        _ => None,
    });
    coordinates.chain(radii).fold(1e-3, f64::max)
}

/// Whether a residual is within `tolerance` of zero: not if it isn't a
/// number.
fn within(residual: f64, tolerance: f64) -> bool {
    residual.abs() <= tolerance
}

/// Solves `sketch`, which is to have passed [`Sketch::check`], for `goal`
/// within `budget`: Gauss-Newton steps of least (weighted) norm, damped,
/// warm started from where the sketch is, until every residual is within
/// a ten-billionth of the sketch's size. The solved sketch can still be
/// past the coordinate limit [`Sketch::check`] takes, which is the
/// caller's to check.
pub fn solve(sketch: &Sketch, goal: &Goal, budget: &Budget) -> Result<Solution, Failure> {
    // A point dragged along a spline starts where it's going.
    let starts = match goal {
        Goal::Drag { points, .. } => points.iter().copied().collect(),
        Goal::Settle => HashMap::new(),
    };
    let mut system = System::toward(sketch, Fixing::Constants, &starts);
    let mut weights = vec![1.0; system.values.len()];
    let dragging = matches!(goal, Goal::Drag { .. });
    let mut scale = scale(sketch);
    if let Goal::Drag { points, radii } = goal {
        let targets = points
            .iter()
            .filter_map(|&(id, target)| {
                let slots = system.point(sketch, id)?;
                Some(
                    slots
                        .into_iter()
                        .zip(target.to_array())
                        .map(move |t| (id, t)),
                )
            })
            .flatten();
        let radii = radii.iter().filter_map(|&(id, target)| {
            let slot = (*system.radii.get(sketch.curve_index(id)?)?)?;
            Some((id, (slot, target)))
        });
        for (id, (slot, target)) in targets.chain(radii).collect::<Vec<_>>() {
            if !target.is_finite() {
                return Err(Failure::Degenerate {
                    involved: BTreeSet::from([id]),
                });
            }
            if let Slot::Var(var) = slot {
                system.values[var] = target;
                weights[var] = DRAG_WEIGHT;
                scale = scale.max(target.abs());
            }
        }
    }

    let tolerance = TOLERANCE * scale;
    let (components, constant) = system.components();
    if !dragging {
        // Equations between fixed things alone: no step changes them.
        let broken: BTreeSet<Id> = constant
            .iter()
            .map(|&index| &system.equations[index])
            .filter(|equation| !within(equation.residual.value(&system.values), tolerance))
            .map(|equation| equation.source)
            .collect();
        if !broken.is_empty() {
            return Err(Failure::NotConverged { involved: broken });
        }
    }
    let mut spent = 0;
    for component in &components {
        if dragging && component.vars.iter().all(|&var| weights[var] == 1.0) {
            continue;
        }
        let mut solver = Solver {
            system: &mut system,
            component,
            weights: &weights,
            scale,
            budget,
            spent: &mut spent,
        };
        solver.settle()?;
    }

    let mut solved = sketch.clone();
    system.write(&mut solved);
    let finite = solved.points.iter().all(|point| point.at.is_finite());
    let bad_radius = solved.curves.iter().find(|entry| match entry.curve {
        Curve::Circle { radius, .. } => !(radius > 0.0 && radius.is_finite()),
        _ => false,
    });
    if !finite || bad_radius.is_some() {
        return Err(Failure::Degenerate {
            involved: bad_radius.map(|entry| entry.id).into_iter().collect(),
        });
    }
    Ok(Solution {
        sketch: solved,
        iterations: spent,
    })
}

/// A component's Jacobian, a row per equation, each row's entries by
/// the variable's position in the component.
struct Jacobian {
    starts: Vec<usize>,
    entries: Vec<(usize, f64)>,
}

impl Jacobian {
    /// At the system's values, each variable's column multiplied by its
    /// entry of `scaling`, if given.
    fn new(system: &System, component: &Component, scaling: Option<&[f64]>) -> Self {
        let mut starts = Vec::with_capacity(component.equations.len() + 1);
        let mut entries = Vec::new();
        for &index in &component.equations {
            starts.push(entries.len());
            system.equations[index]
                .residual
                .gradient(&system.values, |var, derivative| {
                    let column = component.column(var);
                    let scale = scaling.map_or(1.0, |scaling| scaling[column]);
                    entries.push((column, derivative * scale));
                });
        }
        starts.push(entries.len());
        Jacobian { starts, entries }
    }

    /// Whether every entry is a number.
    fn is_finite(&self) -> bool {
        self.entries.iter().all(|(_, value)| value.is_finite())
    }

    fn row(&self, row: usize) -> &[(usize, f64)] {
        &self.entries[self.starts[row]..self.starts[row + 1]]
    }

    fn rows(&self) -> usize {
        self.starts.len() - 1
    }

    /// As a dense matrix of `columns` columns, entries that aren't
    /// numbers taken as zero.
    fn dense(&self, columns: usize) -> Mat<f64> {
        let mut matrix = Mat::zeros(self.rows(), columns);
        for row in 0..self.rows() {
            for &(column, value) in self.row(row) {
                if value.is_finite() {
                    matrix[(row, column)] += value;
                }
            }
        }
        matrix
    }
}

impl Component {
    /// The position of `var`, one of the component's variables.
    fn column(&self, var: usize) -> usize {
        self.vars.binary_search(&var).unwrap_or(0)
    }
}

/// Solving one component, in place in the system's values.
struct Solver<'a, 'b> {
    system: &'a mut System,
    component: &'a Component,
    weights: &'a [f64],
    scale: f64,
    budget: &'a Budget<'b>,
    spent: &'a mut u32,
}

impl Solver<'_, '_> {
    fn residuals(&self) -> Vec<f64> {
        self.component
            .equations
            .iter()
            .map(|&index| {
                self.system.equations[index]
                    .residual
                    .value(&self.system.values)
            })
            .collect()
    }

    /// Levenberg-Marquardt from the system's values. A step solves
    /// `(A Aᵀ + λ I) u = -f` and moves by `W^-½ Aᵀ u`, where `A` is the
    /// Jacobian with each column divided by the square root of its
    /// variable's weight `W`: the least-norm solution of the linearized
    /// equations as `λ` goes to zero, and a shorter step towards less
    /// residual as it grows. Redundant equations only make `A Aᵀ`
    /// singular, which the damping's floor keeps solvable.
    ///
    /// A spline through fit points is read linearly in them, for the
    /// parameters their places give ([`SplineSlots`]), held while it
    /// solves; once solved, they're found anew from where the points went,
    /// and if the equations no longer hold, it solves on from there, until
    /// they do with the parameters their points give. Each round leaves
    /// about a tenth of the error before, the steps leaving out how the
    /// parameters move: a drag step takes two or three.
    ///
    /// A variable with bounds (a point's parameter on an open spline) is
    /// put back within them after each step, and one at a bound that a
    /// step would push past it is held there, its column left out, and the
    /// step found again without it, until a step is taken: a point dragged
    /// past a spline's end stops there rather than the step shrinking to
    /// nothing.
    fn settle(&mut self) -> Result<(), Failure> {
        let tolerance = TOLERANCE * self.scale;
        let converged = |f: &[f64]| f.iter().all(|&r| within(r, tolerance));
        let squared = |f: &[f64]| f.iter().map(|r| r * r).sum::<f64>();
        let mut f = self.residuals();
        if converged(&f) {
            return Ok(());
        }
        let mut cost = squared(&f);
        let n = self.component.vars.len();
        let scaling: Vec<f64> = self
            .component
            .vars
            .iter()
            .map(|&var| 1.0 / self.weights[var].sqrt())
            .collect();
        // The bounded variables' columns, with their bounds.
        let bounded: Vec<(usize, f64, f64)> = self
            .system
            .bounds
            .iter()
            .filter_map(|&(var, least, most)| {
                let column = self.component.vars.binary_search(&var).ok()?;
                Some((column, least, most))
            })
            .collect();
        // The splines through fit points its equations read, each once,
        // and the points on them.
        let mut refits: Vec<Rc<SplineSlots>> = Vec::new();
        let mut on: Vec<(PointSlots, usize, Rc<SplineSlots>)> = Vec::new();
        for &index in &self.component.equations {
            let residual = &self.system.equations[index].residual;
            for spline in residual.splines() {
                if spline.moves() && !refits.iter().any(|seen| Rc::ptr_eq(seen, spline)) {
                    refits.push(spline.clone());
                }
            }
            if let Some((point, var, spline)) = residual.on_spline()
                && spline.moves()
            {
                on.push((point, var, spline.clone()));
            }
        }
        let mut held = vec![false; n];
        let mut damping = DAMPING_START;
        let mut before = vec![0.0; n];
        let mut normal: Option<Normal> = None;
        'steps: loop {
            let scaling: Vec<f64> = scaling
                .iter()
                .zip(&held)
                .map(|(&scaling, &held)| if held { 0.0 } else { scaling })
                .collect();
            let a = Jacobian::new(self.system, self.component, Some(&scaling));
            if !a.is_finite() {
                return Err(Failure::Degenerate {
                    involved: self.sources(),
                });
            }
            match &mut normal {
                Some(normal) if normal.fits(&a) => normal.fill(&a),
                _ => normal = Normal::new(&a, n),
            }
            let Some(normal) = &normal else {
                return Err(self.not_converged(&f));
            };
            let diagonal = normal.largest_diagonal();
            loop {
                if *self.spent >= self.budget.iterations {
                    return Err(self.not_converged(&f));
                }
                if (self.budget.expired)() {
                    return Err(Failure::OutOfTime);
                }
                *self.spent += 1;
                let lambda =
                    diagonal.max(1.0) * (damping * cost / self.scale.powi(2)).max(DAMPING_FLOOR);
                let rhs: Vec<f64> = f.iter().map(|r| -r).collect();
                let step = normal.solve(lambda, &rhs).map(|u| {
                    let mut step = vec![0.0; n];
                    for (row, u) in u.iter().enumerate() {
                        for &(column, value) in a.row(row) {
                            step[column] += value * u;
                        }
                    }
                    step
                });
                if let Some(step) = step {
                    let value = |column: usize| self.system.values[self.component.vars[column]];
                    let past: Vec<usize> = bounded
                        .iter()
                        .filter(|&&(column, least, most)| {
                            let (at, by) = (value(column), step[column]);
                            !held[column] && ((at <= least && by < 0.0) || (at >= most && by > 0.0))
                        })
                        .map(|&(column, ..)| column)
                        .collect();
                    if !past.is_empty() {
                        for column in past {
                            held[column] = true;
                        }
                        continue 'steps;
                    }
                    for (column, &var) in self.component.vars.iter().enumerate() {
                        before[column] = self.system.values[var];
                        self.system.values[var] += scaling[column] * step[column];
                    }
                    for &(column, least, most) in &bounded {
                        let var = self.component.vars[column];
                        self.system.values[var] = self.system.values[var].clamp(least, most);
                    }
                    let trial = self.residuals();
                    let trial_cost = squared(&trial);
                    if trial_cost < cost {
                        let stalled = cost - trial_cost <= STALL * cost;
                        (f, cost) = (trial, trial_cost);
                        damping = (damping / 10.0).max(DAMPING_MIN);
                        if converged(&f) {
                            if refits.is_empty() {
                                return Ok(());
                            }
                            // Solved as its splines were; as they are
                            // where their points went, perhaps not. The
                            // points on them first slide to where they're
                            // nearest now, as their parameters mean other
                            // places.
                            for spline in &refits {
                                spline.refit(&self.system.values);
                            }
                            for (point, var, spline) in &on {
                                let t = self.system.values[*var];
                                self.system.values[*var] =
                                    spline.project(&self.system.values, *point, t);
                            }
                            f = self.residuals();
                            cost = squared(&f);
                            if converged(&f) {
                                return Ok(());
                            }
                            held.fill(false);
                            break;
                        }
                        if stalled {
                            return Err(self.not_converged(&f));
                        }
                        held.fill(false);
                        break;
                    }
                    for (column, &var) in self.component.vars.iter().enumerate() {
                        self.system.values[var] = before[column];
                    }
                }
                damping *= 10.0;
                if damping > DAMPING_MAX {
                    return Err(self.not_converged(&f));
                }
            }
        }
    }

    /// Every constraint and arc with an equation in the component.
    fn sources(&self) -> BTreeSet<Id> {
        self.component
            .equations
            .iter()
            .map(|&index| self.system.equations[index].source)
            .collect()
    }

    /// The failure where the solve stopped, with residuals `f`: the
    /// equations in a dependency among them, and those unsatisfied.
    fn not_converged(&self, f: &[f64]) -> Failure {
        let tolerance = TOLERANCE * self.scale;
        let mut involved = BTreeSet::new();
        let jacobian = Jacobian::new(self.system, self.component, None);
        let implied: Vec<bool> = self
            .component
            .equations
            .iter()
            .map(|&index| self.system.equations[index].implied)
            .collect();
        let dependent = jacobian.is_finite().then(|| {
            let dense = jacobian.dense(self.component.vars.len());
            analysis::counted_dependencies(&dense, &implied)
        });
        for (position, &index) in self.component.equations.iter().enumerate() {
            let in_dependency = dependent
                .as_ref()
                .is_some_and(|dependent| dependent.involved[position]);
            if in_dependency || !within(f[position], tolerance) {
                involved.insert(self.system.equations[index].source);
            }
        }
        Failure::NotConverged { involved }
    }
}

#[cfg(test)]
mod tests;

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
mod jacobian;
mod normal;
mod real;
mod system;

pub use analysis::{Analysis, analyse};

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{Curve, Id, Sketch};

use equation::{PointSlots, Slot, SplineSlots};
use jacobian::Jacobian;
use normal::Normal;
use system::{Bound, Component, Fixing, System};

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
    let drag = Drag::new(&mut system, sketch, goal, scale(sketch))?;
    let tolerance = TOLERANCE * drag.scale;
    let (components, constant) = system.components();
    if drag.dragged.is_none() {
        constants_hold(&system, &constant, tolerance)?;
    }
    let mut spent = 0;
    for component in &components {
        if let Some(dragged) = &drag.dragged
            && !component.vars.iter().any(|&var| dragged[var])
        {
            continue;
        }
        let mut solver = Solver {
            system: &mut system,
            component,
            weights: &drag.weights,
            scale: drag.scale,
            budget,
            spent: &mut spent,
        };
        solver.settle()?;
    }
    Ok(Solution {
        sketch: written(&system, sketch)?,
        iterations: spent,
    })
}

/// What a goal drags, its targets set in the system's values.
struct Drag {
    /// Each variable's weight: [`DRAG_WEIGHT`] for one dragged.
    weights: Vec<f64>,
    /// Per variable, whether the drag sets it, which picks the components
    /// solved. `None` settling.
    dragged: Option<Vec<bool>>,
    /// The size tolerances are relative to, the targets' too.
    scale: f64,
}

impl Drag {
    /// Sets the targets of `goal` on `sketch` in `system`'s values, `scale`
    /// the sketch's size. `Degenerate` for a target that isn't finite.
    fn new(system: &mut System, sketch: &Sketch, goal: &Goal, scale: f64) -> Result<Self, Failure> {
        let mut drag = Drag {
            weights: vec![1.0; system.values.len()],
            dragged: None,
            scale,
        };
        let Goal::Drag { points, radii } = goal else {
            return Ok(drag);
        };
        let mut dragged = vec![false; system.values.len()];
        let mut targets = Vec::new();
        for &(id, target) in points {
            if let Some(slots) = system.point(sketch, id) {
                targets.extend(slots.into_iter().zip(target.to_array()).map(|t| (id, t)));
            }
        }
        for &(id, target) in radii {
            let slot = sketch
                .curve_index(id)
                .and_then(|index| *system.radii.get(index)?);
            if let Some(slot) = slot {
                targets.push((id, (slot, target)));
            }
        }
        for (id, (slot, target)) in targets {
            if !target.is_finite() {
                return Err(Failure::Degenerate {
                    involved: BTreeSet::from([id]),
                });
            }
            if let Slot::Var(var) = slot {
                system.values[var] = target;
                drag.weights[var] = DRAG_WEIGHT;
                dragged[var] = true;
                drag.scale = drag.scale.max(target.abs());
            }
        }
        drag.dragged = Some(dragged);
        Ok(drag)
    }
}

/// Fails unless the equations between fixed things alone, `constant` by
/// index, hold within `tolerance`: no step changes them.
fn constants_hold(system: &System, constant: &[usize], tolerance: f64) -> Result<(), Failure> {
    let broken: BTreeSet<Id> = constant
        .iter()
        .map(|&index| &system.equations[index])
        .filter(|equation| !within(equation.residual.value(&system.values), tolerance))
        .map(|equation| equation.source)
        .collect();
    if broken.is_empty() {
        Ok(())
    } else {
        Err(Failure::NotConverged { involved: broken })
    }
}

/// `sketch` with `system`'s values written in. `Degenerate` for a point
/// that isn't finite or a circle's radius not above zero.
fn written(system: &System, sketch: &Sketch) -> Result<Sketch, Failure> {
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
    Ok(solved)
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
        let bounded = self.bounded_columns();
        let moving = self.moving_splines();
        let mut held = vec![false; n];
        let mut damping = DAMPING_START;
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
            // Damped tries from this Jacobian, until one improves or
            // meets a bound.
            loop {
                if *self.spent >= self.budget.iterations {
                    return Err(self.not_converged(&f));
                }
                if (self.budget.expired)() {
                    return Err(Failure::OutOfTime);
                }
                *self.spent += 1;
                let lambda = diagonal.max(1.0)
                    * (damping * cost / (self.scale * self.scale)).max(DAMPING_FLOOR);
                if let Some(step) = step(&a, normal, lambda, &f, n) {
                    let past = self.past_bounds(&step, &bounded, &held);
                    if !past.is_empty() {
                        // Found again without them.
                        for column in past {
                            held[column] = true;
                        }
                        continue 'steps;
                    }
                    let before = self.take(&step, &scaling, &bounded);
                    let trial = self.residuals();
                    let trial_cost = squared(&trial);
                    if trial_cost < cost {
                        let stalled = cost - trial_cost <= STALL * cost;
                        (f, cost) = (trial, trial_cost);
                        damping = (damping / 10.0).max(DAMPING_MIN);
                        if converged(&f) {
                            if moving.refits.is_empty() {
                                return Ok(());
                            }
                            f = self.refit(&moving);
                            cost = squared(&f);
                            if converged(&f) {
                                return Ok(());
                            }
                        } else if stalled {
                            return Err(self.not_converged(&f));
                        }
                        held.fill(false);
                        continue 'steps;
                    }
                    self.restore(&before);
                }
                damping *= 10.0;
                if damping > DAMPING_MAX {
                    return Err(self.not_converged(&f));
                }
            }
        }
    }

    /// The columns of the component's bounded variables, with their
    /// bounds.
    fn bounded_columns(&self) -> Vec<ColumnBound> {
        self.system
            .bounds
            .iter()
            .filter_map(|&Bound { var, least, most }| {
                let column = self.component.vars.binary_search(&var).ok()?;
                Some(ColumnBound {
                    column,
                    least,
                    most,
                })
            })
            .collect()
    }

    /// The splines through fit points the component's equations read,
    /// each once, and the points on them with their parameters'
    /// variables.
    fn moving_splines(&self) -> MovingSplines {
        let mut refits: Vec<Rc<SplineSlots>> = Vec::new();
        let mut on: Vec<OnSpline> = Vec::new();
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
                on.push(OnSpline {
                    point,
                    var,
                    spline: spline.clone(),
                });
            }
        }
        MovingSplines { refits, on }
    }

    /// The bounded columns not `held` that `step` would push past a
    /// bound they're at.
    fn past_bounds(&self, step: &[f64], bounded: &[ColumnBound], held: &[bool]) -> Vec<usize> {
        let value = |column: usize| self.system.values[self.component.vars[column]];
        bounded
            .iter()
            .filter(|bound| {
                let column = bound.column;
                !held[column] && bound.pushes_past(value(column), step[column])
            })
            .map(|bound| bound.column)
            .collect()
    }

    /// Moves by `step`, scaled by `scaling`, the bounded variables put
    /// back within their bounds: the values before, for
    /// [`Solver::restore`].
    fn take(&mut self, step: &[f64], scaling: &[f64], bounded: &[ColumnBound]) -> Vec<f64> {
        let mut before = Vec::with_capacity(step.len());
        for (column, &var) in self.component.vars.iter().enumerate() {
            before.push(self.system.values[var]);
            self.system.values[var] += scaling[column] * step[column];
        }
        for bound in bounded {
            let var = self.component.vars[bound.column];
            self.system.values[var] = self.system.values[var].clamp(bound.least, bound.most);
        }
        before
    }

    /// Puts back the component's values from before a step.
    fn restore(&mut self, before: &[f64]) {
        for (column, &var) in self.component.vars.iter().enumerate() {
            self.system.values[var] = before[column];
        }
    }

    /// Solved as its splines were; as they are where their points went,
    /// perhaps not: finds them anew, the points on them first sliding to
    /// where they're nearest now, as their parameters mean other places.
    /// The residuals after.
    fn refit(&mut self, moving: &MovingSplines) -> Vec<f64> {
        for spline in &moving.refits {
            spline.refit(&self.system.values);
        }
        for &OnSpline {
            point,
            var,
            ref spline,
        } in &moving.on
        {
            let t = self.system.values[var];
            self.system.values[var] = spline.project(&self.system.values, point, t);
        }
        self.residuals()
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
        let dependent = jacobian
            .is_finite()
            .then(|| self.component.dependencies(self.system, &jacobian));
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

/// The splines a component's equations read that move with their fit
/// points, and the points on them with their parameters' variables.
struct MovingSplines {
    refits: Vec<Rc<SplineSlots>>,
    on: Vec<OnSpline>,
}

/// A point on a moving spline at a variable parameter.
struct OnSpline {
    point: PointSlots,
    /// The parameter's variable.
    var: usize,
    spline: Rc<SplineSlots>,
}

/// A bounded variable of a component by its column: its least and most.
#[derive(Debug, Clone, Copy)]
struct ColumnBound {
    column: usize,
    least: f64,
    most: f64,
}

impl ColumnBound {
    /// Whether a step `by` from `at` pushes past a bound it's at.
    fn pushes_past(&self, at: f64, by: f64) -> bool {
        (at <= self.least && by < 0.0) || (at >= self.most && by > 0.0)
    }
}

/// The sum of the squares of residuals `f`.
fn squared(f: &[f64]) -> f64 {
    f.iter().map(|r| r * r).sum()
}

/// The step of a damped try at `lambda`: `Aᵀ u` for `u` solving
/// `(A Aᵀ + λ I) u = -f`, over `n` columns. `None` where it can't be
/// solved.
fn step(a: &Jacobian, normal: &Normal, lambda: f64, f: &[f64], n: usize) -> Option<Vec<f64>> {
    let rhs: Vec<f64> = f.iter().map(|r| -r).collect();
    let u = normal.solve(lambda, &rhs)?;
    let mut step = vec![0.0; n];
    for (row, u) in u.iter().enumerate() {
        for &(column, value) in a.row(row) {
            step[column] += value * u;
        }
    }
    Some(step)
}

#[cfg(test)]
mod tests;

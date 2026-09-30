//! Proposals and drags: an edit applied, solved and analysed into a sketch
//! that can be committed or a refusal saying why ([`propose`]), and a drag
//! stepping from one solution to the next ([`DragSession`]). Pure, like
//! the solver they run.

use std::collections::BTreeSet;
use std::f64::consts::PI;
use std::fmt;

use glam::DVec2;

use crate::angle;
use serde::{Deserialize, Serialize};

use crate::{
    Analysis, Budget, Design, EditError, Failure, Goal, Id, Measure, Sketch, SketchEdit, analyse,
    solve,
};

/// The most steps a dimension's value is changed in
/// ([`SketchEdit::SetDimension`]).
const CONTINUATION_STEPS: u32 = 16;

/// The most a length changes by in one step of a dimension's value: this
/// factor, either way.
const LENGTH_STEP: f64 = 1.25;

/// The most an angle changes by in one step of a dimension's value: 15°.
const ANGLE_STEP: f64 = PI / 12.0;

/// An edit that can be committed: the sketch it makes, solved, and its
/// analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub sketch: Sketch,
    pub analysis: Analysis,
}

/// Why an edit was refused. The ids it names are those of the sketch the
/// edit made, so new constraints have the ids applying it gives them
/// ([`Add::resolve`](crate::Add::resolve)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rejected {
    /// The edit can't be applied, or its solution fails [`Sketch::check`],
    /// such as a point pulled past the coordinate limit.
    Edit(EditError),
    /// Solved, but these constraints (and arcs, for the equation each
    /// implies, and driving dimensions) are in a dependency: one of them
    /// follows from the others.
    Redundant { involved: BTreeSet<Id> },
    /// Driving `dimensions` the edit adds or makes driving over-constrain
    /// the sketch: they're `involved` in a dependency, as for `Redundant`,
    /// solved or not, or in what doesn't solve. As reference dimensions,
    /// which the UI can suggest, they'd add no equation.
    Driving {
        dimensions: BTreeSet<Id>,
        involved: BTreeSet<Id>,
    },
    /// It can't be solved: constraints in conflict, a place they can't
    /// reach, or out of time.
    Unsolved(Failure),
}

/// Nothing involved.
static NONE: BTreeSet<Id> = BTreeSet::new();

impl Rejected {
    /// The constraints and dimensions (and arcs) involved, to highlight;
    /// empty where they can't be told.
    pub fn involved(&self) -> &BTreeSet<Id> {
        match self {
            Rejected::Redundant { involved }
            | Rejected::Driving { involved, .. }
            | Rejected::Unsolved(
                Failure::NotConverged { involved } | Failure::Degenerate { involved },
            ) => involved,
            Rejected::Edit(_) | Rejected::Unsolved(Failure::OutOfTime) => &NONE,
        }
    }
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::Edit(why) => why.fmt(f),
            Rejected::Redundant { .. } => f.write_str("it would over-constrain the sketch"),
            Rejected::Driving { .. } => f.write_str(
                "the dimension would over-constrain the sketch, but could be a reference",
            ),
            Rejected::Unsolved(Failure::NotConverged { .. }) => {
                f.write_str("the sketch can't be solved with it")
            }
            Rejected::Unsolved(Failure::Degenerate { .. }) => {
                f.write_str("it would collapse the sketch")
            }
            Rejected::Unsolved(Failure::OutOfTime) => f.write_str("solving it took too long"),
        }
    }
}

impl std::error::Error for Rejected {}

/// Applies `edit` to `sketch`, which is to have passed [`Sketch::check`]
/// against `max`, solves the result warm started from where it is, and
/// analyses it: [`Accepted`] if it solves with no constraint redundant and
/// within `max`, else [`Rejected`] naming why, never a sketch solved in
/// part. `budget` bounds each solve.
///
/// An [`Add`](crate::Add)'s `auto` constraints are tried together first;
/// if that fails, the edit is solved without them (failing that, it's
/// refused) and each is tried in turn on the last success, kept if the
/// sketch still solves with nothing redundant, dropped if not, whether it
/// restates the rest or contradicts it. Running out of time refuses the
/// edit instead.
///
/// A [`SketchEdit::Move`] is a drag from `sketch` to where it puts its
/// points and radii, then settled: what's fixed stays, the rest follows,
/// and a moved point ends where it was put, or as near as the constraints
/// allow.
///
/// A [`SketchEdit::SetDimension`] of a driving dimension moves its value
/// from the old to the new in steps (continuation), each solved from the
/// last, so the geometry follows it on the branch it's on rather than
/// jumping to another solution: a length by at most a quarter either way
/// a step, an angle by at most 15°, in at most 16 steps. A driving
/// dimension an [`Add`](crate::Add) places goes the same way, from what
/// it measures where it's placed to its value.
///
/// An edit adding driving dimensions, or making one driving, that's
/// refused as redundant or unsolved with them involved is
/// [`Rejected::Driving`].
pub fn propose(
    sketch: &Sketch,
    edit: &SketchEdit,
    design: &Design,
    budget: &Budget,
) -> Result<Accepted, Rejected> {
    let (applied, auto) = edit.apply_marked(sketch, design).map_err(Rejected::Edit)?;
    let driving = edit.driving(sketch, &applied);
    proposed(sketch, edit, applied, auto, design, budget).map_err(|why| {
        let involved = match &why {
            Rejected::Redundant { involved }
            | Rejected::Unsolved(Failure::NotConverged { involved }) => involved,
            _ => return why,
        };
        let dimensions: BTreeSet<Id> = driving
            .into_iter()
            .filter(|id| involved.contains(id))
            .collect();
        if dimensions.is_empty() {
            return why;
        }
        Rejected::Driving {
            dimensions,
            involved: involved.clone(),
        }
    })
}

/// [`propose`] once `edit` is `applied`, `auto` the ids its `auto`
/// constraints got.
fn proposed(
    sketch: &Sketch,
    edit: &SketchEdit,
    applied: Sketch,
    auto: Vec<Id>,
    design: &Design,
    budget: &Budget,
) -> Result<Accepted, Rejected> {
    match edit {
        SketchEdit::Move { points, radii } => {
            let goal = Goal::Drag {
                points: points.clone(),
                radii: radii.clone(),
            };
            let dragged = solve(sketch, &goal, budget).map_err(Rejected::Unsolved)?;
            return settle(&dragged.sketch, design, budget);
        }
        SketchEdit::SetDimension { id, value } => {
            // A reference's value moves nothing.
            let old = sketch
                .dimension(*id)
                .filter(|entry| entry.dimension.driving)
                .map(|entry| entry.dimension.value.value);
            let targets: Vec<_> = old.map(|old| (*id, old, value.value)).into_iter().collect();
            let continued = continuation(applied, &targets, budget)?;
            return settle(&continued, design, budget);
        }
        _ => {}
    }
    // New driving dimensions go from what they measure where they're
    // placed to the values they're given, as a value set does.
    let applied = match edit {
        SketchEdit::Add(_) => {
            let placed: Vec<_> = edit
                .driving(sketch, &applied)
                .into_iter()
                .filter_map(|id| {
                    let dimension = &applied.dimension(id)?.dimension;
                    let old = applied.measure(&dimension.measure, dimension.side)?;
                    (old > 0.0).then_some((id, old, dimension.value.value))
                })
                .collect();
            continuation(applied, &placed, budget)?
        }
        _ => applied,
    };
    let all = settle(&applied, design, budget);
    if auto.is_empty() || matches!(all, Ok(_) | Err(Rejected::Unsolved(Failure::OutOfTime))) {
        return all;
    }

    // Tell the `auto` constraints apart: the analysis names a whole
    // dependency, which could be the edit's own.
    let mut without = applied;
    let (auto, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut without.constraints)
        .into_iter()
        .partition(|entry| auto.binary_search(&entry.id).is_ok());
    without.constraints = rest;
    let mut accepted = settle(&without, design, budget)?;
    for entry in auto {
        // Its id is the highest yet, so it goes last.
        let mut trial = accepted.sketch.clone();
        trial.constraints.push(entry);
        match settle(&trial, design, budget) {
            Ok(with) => accepted = with,
            Err(out @ Rejected::Unsolved(Failure::OutOfTime)) => return Err(out),
            Err(_) => {}
        }
    }
    Ok(accepted)
}

/// `applied`, with its points and radii where changing each dimension of
/// `targets`, `(id, old, new)`, from its `old` value to its `new` one in
/// steps takes them, each solved from the last, all but the last: that's
/// left to [`settle`], which finds each at its value in `applied`. The
/// steps are as many as the dimension changing most takes. `applied` as
/// it is if a step is all it takes.
fn continuation(
    mut applied: Sketch,
    targets: &[(Id, f64, f64)],
    budget: &Budget,
) -> Result<Sketch, Rejected> {
    let mut measured = Vec::with_capacity(targets.len());
    for &(id, old, new) in targets {
        let entry = applied
            .dimension(id)
            .ok_or(Rejected::Edit(EditError::Target(id)))?;
        measured.push((id, entry.dimension.measure.clone(), old, new));
    }
    let steps = measured
        .iter()
        .map(|(_, measure, old, new)| steps(measure, *old, *new))
        .max()
        .unwrap_or(1);
    for step in 1..steps {
        let t = f64::from(step) / f64::from(steps);
        let mut stepped = applied.clone();
        for (id, measure, old, new) in &measured {
            let between = match measure {
                Measure::Angle(..) => old + (new - old) * t,
                // Evenly in proportion, as a length only changes by a
                // factor at each step.
                _ => angle::exp(angle::ln(*old) + (angle::ln(*new) - angle::ln(*old)) * t),
            };
            let dimension = stepped
                .dimension_mut(*id)
                .ok_or(Rejected::Edit(EditError::Target(*id)))?;
            dimension.dimension.value.value = between;
        }
        let solved = solve(&stepped, &Goal::Settle, budget)
            .map_err(Rejected::Unsolved)?
            .sketch;
        // Back to the values asked for, the geometry where this step left
        // it.
        applied.points = solved.points;
        applied.curves = solved.curves;
    }
    Ok(applied)
}

/// How many steps a dimension of `measure` goes from `old` to `new` in,
/// see [`propose`]: one where the values aren't both above zero and
/// finite, as a checked sketch's and edit's are.
fn steps(measure: &Measure, old: f64, new: f64) -> u32 {
    let steps = match measure {
        Measure::Angle(..) => (new - old).abs() / ANGLE_STEP,
        _ => (angle::ln(new) - angle::ln(old)).abs() / angle::ln(LENGTH_STEP),
    };
    if steps.is_finite() {
        // Within a u32 once bounded; the cast of a number to an integer
        // saturates.
        (steps.ceil() as u32).clamp(1, CONTINUATION_STEPS)
    } else {
        1
    }
}

/// `sketch` solved, within the design's limit and with nothing redundant.
fn settle(sketch: &Sketch, design: &Design, budget: &Budget) -> Result<Accepted, Rejected> {
    let solved = solve(sketch, &Goal::Settle, budget)
        .map_err(Rejected::Unsolved)?
        .sketch;
    solved
        .check(design)
        .map_err(|why| Rejected::Edit(why.into()))?;
    let analysis = analyse(&solved);
    if !analysis.redundant.is_empty() {
        return Err(Rejected::Redundant {
            involved: analysis.redundant,
        });
    }
    Ok(Accepted {
        sketch: solved,
        analysis,
    })
}

/// A drag in progress: the last solution that converged, each step solved
/// from it (a warm start), so what it shows only ever holds every
/// constraint. The lane keeps one per drag, so later steps send only the
/// targets.
#[derive(Debug, Clone, PartialEq)]
pub struct DragSession {
    sketch: Sketch,
    design: Design,
}

impl DragSession {
    /// Starts dragging `sketch`, which is to have passed [`Sketch::check`]
    /// against `design`, whose coordinate limit each step keeps within.
    pub fn new(sketch: Sketch, design: Design) -> DragSession {
        DragSession { sketch, design }
    }

    /// The last solution that converged, or the sketch it started from.
    pub fn sketch(&self) -> &Sketch {
        &self.sketch
    }

    /// Drags `points` and circles' `radii` towards their targets, see
    /// [`Goal::Drag`]: the new solution, or, if it doesn't converge within
    /// `budget` or would go past the limit, why not, keeping the last.
    pub fn step(
        &mut self,
        points: Vec<(Id, DVec2)>,
        radii: Vec<(Id, f64)>,
        budget: &Budget,
    ) -> Result<&Sketch, Rejected> {
        let goal = Goal::Drag { points, radii };
        let solved = solve(&self.sketch, &goal, budget)
            .map_err(Rejected::Unsolved)?
            .sketch;
        solved
            .check(&self.design)
            .map_err(|why| Rejected::Edit(why.into()))?;
        self.sketch = solved;
        Ok(&self.sketch)
    }
}

#[cfg(test)]
mod tests;

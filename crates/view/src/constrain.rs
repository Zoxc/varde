//! Constraints as the user applies them: the kinds the toolbar, the keys
//! and the Constrain tool offer, which of them fit what's selected and in
//! what order, and the constraints each makes of it. Pure functions of the
//! sketch and the selection, tested headless.

use std::collections::BTreeSet;

use glam::DVec2;
use varde_sketch::{Constraint, Id, Kind, Sketch};

use crate::icons::Icon;

/// A constraint as the user picks it, whatever it's made of: Coincident is
/// two points at one place or a point on a curve, Horizontal a line or two
/// points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintKind {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    /// A spline's end joined smoothly: tangent, and curving alike.
    Smooth,
    Equal,
    Concentric,
    Midpoint,
    Symmetric,
    Fix,
    /// Equal offsets, which only the Offset tool makes: never applied to
    /// a selection, so not among [`ConstraintKind::ALL`].
    Offset,
}

impl ConstraintKind {
    /// Every kind the user applies, in the order the lists show them.
    pub const ALL: [ConstraintKind; 12] = [
        ConstraintKind::Coincident,
        ConstraintKind::Horizontal,
        ConstraintKind::Vertical,
        ConstraintKind::Parallel,
        ConstraintKind::Perpendicular,
        ConstraintKind::Tangent,
        ConstraintKind::Smooth,
        ConstraintKind::Equal,
        ConstraintKind::Concentric,
        ConstraintKind::Midpoint,
        ConstraintKind::Symmetric,
        ConstraintKind::Fix,
    ];

    /// The kind as the user sees it.
    pub fn label(self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "Coincident",
            ConstraintKind::Horizontal => "Horizontal",
            ConstraintKind::Vertical => "Vertical",
            ConstraintKind::Parallel => "Parallel",
            ConstraintKind::Perpendicular => "Perpendicular",
            ConstraintKind::Tangent => "Tangent",
            ConstraintKind::Smooth => "Smooth",
            ConstraintKind::Equal => "Equal",
            ConstraintKind::Concentric => "Concentric",
            ConstraintKind::Midpoint => "Midpoint",
            ConstraintKind::Symmetric => "Symmetric",
            ConstraintKind::Fix => "Fix",
            ConstraintKind::Offset => "Offset",
        }
    }

    /// Its glyph, in the lists, the toolbar and the viewport.
    pub(crate) fn icon(self) -> Icon {
        match self {
            ConstraintKind::Coincident => Icon::Coincident,
            ConstraintKind::Horizontal => Icon::Horizontal,
            ConstraintKind::Vertical => Icon::Vertical,
            ConstraintKind::Parallel => Icon::Parallel,
            ConstraintKind::Perpendicular => Icon::Perpendicular,
            ConstraintKind::Tangent => Icon::Tangent,
            ConstraintKind::Smooth => Icon::Smooth,
            ConstraintKind::Equal => Icon::Equal,
            ConstraintKind::Concentric => Icon::Concentric,
            ConstraintKind::Midpoint => Icon::Midpoint,
            ConstraintKind::Symmetric => Icon::Symmetric,
            ConstraintKind::Fix => Icon::Fix,
            ConstraintKind::Offset => Icon::OffsetConstraint,
        }
    }

    /// The kind `constraint` is of.
    pub fn of(constraint: &Constraint) -> ConstraintKind {
        match constraint {
            Constraint::Coincident(..) | Constraint::PointOnCurve { .. } => {
                ConstraintKind::Coincident
            }
            Constraint::Horizontal(_) | Constraint::HorizontalPoints(..) => {
                ConstraintKind::Horizontal
            }
            Constraint::Vertical(_) | Constraint::VerticalPoints(..) => ConstraintKind::Vertical,
            Constraint::Parallel(..) => ConstraintKind::Parallel,
            Constraint::Perpendicular(..) => ConstraintKind::Perpendicular,
            Constraint::Tangent { .. } => ConstraintKind::Tangent,
            Constraint::Smooth { .. } => ConstraintKind::Smooth,
            Constraint::Equal(..) => ConstraintKind::Equal,
            Constraint::Concentric(..) => ConstraintKind::Concentric,
            Constraint::Midpoint { .. } => ConstraintKind::Midpoint,
            Constraint::Symmetric { .. } => ConstraintKind::Symmetric,
            Constraint::Fix(_) => ConstraintKind::Fix,
            Constraint::EqualOffset { .. } => ConstraintKind::Offset,
        }
    }

    /// The constraints of this kind the geometry `selected` in `sketch`
    /// makes, if it fits: constraints selected are left out. Several
    /// items make a constraint between the first and each of the rest,
    /// where the kind takes two or more (coincident points, parallel or
    /// equal lines, equal or concentric circles), or one each (a line's
    /// horizontal, a fix). Splines take a point on them, a tangent or a
    /// smooth join at an end ([`Sketch::joint`]), and a fix. Nothing that
    /// names a curve and one of its own points, nor the origin and axes as
    /// they can't be ([`Constraint::fits_builtins`]), which
    /// [`Sketch::check`] refuses.
    pub fn make(self, sketch: &Sketch, selected: &BTreeSet<Id>) -> Option<Vec<Constraint>> {
        let picked = Picked::of(sketch, selected);
        let Picked {
            points,
            lines,
            rounds,
            splines,
        } = &picked;
        let curves = picked.curves();
        let takes_splines = matches!(
            self,
            ConstraintKind::Coincident
                | ConstraintKind::Tangent
                | ConstraintKind::Smooth
                | ConstraintKind::Fix
        );
        if !splines.is_empty() && !takes_splines {
            return None;
        }
        let first_with_each = |ids: &[Id], make: fn(Id, Id) -> Constraint| {
            (ids.len() >= 2).then(|| ids[1..].iter().map(|&id| make(ids[0], id)).collect())
        };
        let made: Option<Vec<Constraint>> = match self {
            ConstraintKind::Coincident => match (&points[..], &curves[..]) {
                (points, []) => first_with_each(points, Constraint::Coincident),
                (&[point], &[curve]) => Some(vec![Constraint::PointOnCurve { point, curve }]),
                _ => None,
            },
            ConstraintKind::Horizontal | ConstraintKind::Vertical => {
                let horizontal = self == ConstraintKind::Horizontal;
                match (&points[..], &lines[..], &rounds[..]) {
                    (&[a, b], [], []) => Some(vec![if horizontal {
                        Constraint::HorizontalPoints(a, b)
                    } else {
                        Constraint::VerticalPoints(a, b)
                    }]),
                    ([], lines, []) if !lines.is_empty() => Some(
                        lines
                            .iter()
                            .map(|&line| {
                                if horizontal {
                                    Constraint::Horizontal(line)
                                } else {
                                    Constraint::Vertical(line)
                                }
                            })
                            .collect(),
                    ),
                    _ => None,
                }
            }
            ConstraintKind::Parallel => match picked.only_lines() {
                Some(lines) => first_with_each(lines, Constraint::Parallel),
                None => None,
            },
            ConstraintKind::Perpendicular => match picked.only_lines() {
                Some(&[a, b]) => Some(vec![Constraint::Perpendicular(a, b)]),
                _ => None,
            },
            ConstraintKind::Tangent => match (&points[..], &curves[..]) {
                ([], &[a, b]) => sketch.tangent(a, b).map(|tangent| vec![tangent]),
                _ => None,
            },
            ConstraintKind::Smooth => match (&points[..], &curves[..]) {
                ([], &[a, b]) => sketch.smooth(a, b).map(|smooth| vec![smooth]),
                _ => None,
            },
            ConstraintKind::Equal => match (&points[..], &lines[..], &rounds[..]) {
                ([], lines, []) => first_with_each(lines, Constraint::Equal),
                ([], [], rounds) => first_with_each(rounds, Constraint::Equal),
                _ => None,
            },
            ConstraintKind::Concentric => match (lines.is_empty(), rounds.first()) {
                (true, Some(&round)) => {
                    let others = points.iter().chain(&rounds[1..]);
                    let made: Vec<_> = others
                        .map(|&other| Constraint::Concentric(round, other))
                        .collect();
                    (!made.is_empty()).then_some(made)
                }
                _ => None,
            },
            ConstraintKind::Midpoint => match (&points[..], &lines[..], &rounds[..]) {
                (&[point], &[line], []) => Some(vec![Constraint::Midpoint { point, line }]),
                _ => None,
            },
            ConstraintKind::Symmetric => match (&points[..], &lines[..], &rounds[..]) {
                (&[a, b], &[about], []) => Some(vec![Constraint::Symmetric { a, b, about }]),
                _ => None,
            },
            ConstraintKind::Fix => {
                let all: Vec<_> = points.iter().chain(&curves).copied().collect();
                (!all.is_empty()).then(|| all.into_iter().map(Constraint::Fix).collect())
            }
            ConstraintKind::Offset => None,
        };
        made.filter(|made| {
            made.iter().all(|constraint| {
                constraint.own_point(sketch).is_none() && constraint.fits_builtins()
            })
        })
    }

    /// The kinds that fit the geometry `selected` in `sketch` (see
    /// [`make`](ConstraintKind::make)), the most likely first: what ties
    /// several items together before what holds one alone, and of two
    /// that could, the one the geometry is nearer to already (horizontal
    /// or vertical, parallel or perpendicular, concentric or not).
    pub fn fitting(sketch: &Sketch, selected: &BTreeSet<Id>) -> Vec<ConstraintKind> {
        use ConstraintKind::*;
        let picked = Picked::of(sketch, selected);
        let mut order = vec![
            Coincident,
            Tangent,
            Smooth,
            Symmetric,
            Midpoint,
            Parallel,
            Perpendicular,
            Horizontal,
            Vertical,
            Equal,
            Concentric,
            Fix,
        ];
        let swap = |order: &mut Vec<ConstraintKind>, a, b| {
            let (i, j) = (position(order, a), position(order, b));
            order.swap(i, j);
        };
        if let Some(direction) = picked.direction(sketch)
            && direction.y.abs() > direction.x.abs()
        {
            swap(&mut order, Horizontal, Vertical);
        }
        if let Some(&[a, b]) = picked.only_lines()
            && let (Some(a), Some(b)) = (line_direction(sketch, a), line_direction(sketch, b))
            && a.perp_dot(b).abs() > a.dot(b).abs()
        {
            swap(&mut order, Parallel, Perpendicular);
        }
        if let ([], [], &[a, b]) = (&picked.points[..], &picked.lines[..], &picked.rounds[..])
            && let (Some((ca, ra)), Some((cb, rb))) = (sketch.round(a), sketch.round(b))
            && ca.distance(cb) < ra.min(rb) / 4.0
        {
            // Nearly about one centre: concentric first.
            let at = position(&order, Concentric);
            let concentric = order.remove(at);
            order.insert(0, concentric);
        }
        order.retain(|kind| kind.make(sketch, selected).is_some());
        order
    }
}

/// Where `kind` is in `order`, which holds every kind.
fn position(order: &[ConstraintKind], kind: ConstraintKind) -> usize {
    order
        .iter()
        .position(|&k| k == kind)
        .expect("every kind is in the order")
}

/// A line's direction, start to end.
fn line_direction(sketch: &Sketch, line: Id) -> Option<DVec2> {
    let (start, end) = sketch.line(line)?;
    Some(end - start)
}

/// The geometry selected, by kind, each in increasing order of id.
#[derive(Debug, Default)]
struct Picked {
    points: Vec<Id>,
    lines: Vec<Id>,
    /// Circles and arcs.
    rounds: Vec<Id>,
    splines: Vec<Id>,
}

impl Picked {
    fn of(sketch: &Sketch, selected: &BTreeSet<Id>) -> Self {
        let mut picked = Picked::default();
        for &id in selected {
            match sketch.kind(id) {
                Some(Kind::Point) => picked.points.push(id),
                Some(Kind::Line) => picked.lines.push(id),
                Some(Kind::Circle | Kind::Arc) => picked.rounds.push(id),
                Some(Kind::Spline) => picked.splines.push(id),
                Some(Kind::Constraint | Kind::Dimension) | None => {}
            }
        }
        picked
    }

    /// The curves, lines first, splines last.
    fn curves(&self) -> Vec<Id> {
        let curves = self.lines.iter().chain(&self.rounds).chain(&self.splines);
        curves.copied().collect()
    }

    /// The lines, if nothing else is picked.
    fn only_lines(&self) -> Option<&[Id]> {
        let others = self.points.is_empty() && self.rounds.is_empty() && self.splines.is_empty();
        (others && !self.lines.is_empty()).then_some(&self.lines[..])
    }

    /// The way the selection runs, for telling horizontal from vertical:
    /// the first line's, or from one of two points to the other.
    fn direction(&self, sketch: &Sketch) -> Option<DVec2> {
        match (&self.points[..], &self.lines[..]) {
            (&[a, b], []) => Some(sketch.point(b)?.at - sketch.point(a)?.at),
            ([], &[line, ..]) => line_direction(sketch, line),
            _ => None,
        }
    }
}

/// Some constraint kinds, as a small set: which fit the selection, for
/// the keys, which are only bound while they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConstraintSet(u16);

impl ConstraintSet {
    pub fn contains(self, kind: ConstraintKind) -> bool {
        self.0 & Self::bit(kind) != 0
    }

    fn bit(kind: ConstraintKind) -> u16 {
        1 << position(&ConstraintKind::ALL, kind)
    }
}

impl FromIterator<ConstraintKind> for ConstraintSet {
    fn from_iter<I: IntoIterator<Item = ConstraintKind>>(kinds: I) -> Self {
        ConstraintSet(kinds.into_iter().fold(0, |set, kind| set | Self::bit(kind)))
    }
}

#[cfg(test)]
mod tests;

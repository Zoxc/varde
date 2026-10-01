//! The shape tools: trimming a curve back to the curves cutting it
//! ([`SketchEdit::Trim`](crate::SketchEdit::Trim)), extending a line, an
//! arc or a spline to the next curve it meets
//! ([`SketchEdit::Extend`](crate::SketchEdit::Extend)) and mirroring
//! geometry about a line ([`SketchEdit::Mirror`](crate::SketchEdit::Mirror)),
//! with what the view shows of them before they're made
//! ([`Sketch::trim_piece`], [`Sketch::extension`]).
//!
//! Every curve cuts, construction curves too, as in other CAD, the origin's
//! axes not. Where curves meet is [`meet`], with the tolerance profiles
//! use, of the sketch's size. What the tools tie their new ends to is
//! `auto` (see [`Add::auto`](crate::Add::auto)): kept if it holds with the
//! rest, dropped if it restates or contradicts it.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use glam::DVec2;

use crate::intersect::{Geom, meet};
use crate::{
    BSpline, Constraint, Corner, Curve, EditError, Id, Kind, Measure, OutOfIds, Sketch, SplineKind,
    control_knots, foot,
};

/// Where other curves cut a curve, trimming or extending it.
#[derive(Debug, Clone, PartialEq)]
struct Cut {
    /// The parameter along the curve cut (see [`Geom`]).
    u: f64,
    /// The curves cutting it there.
    by: Vec<Id>,
    /// An end of one of those curves that's there, if one is: a curve cut
    /// there ends at that point itself.
    end: Option<Id>,
}

/// What trimming a curve near a place takes away.
#[derive(Debug, Clone, PartialEq)]
enum Trimmed {
    /// All of it: nothing cuts it around there.
    Whole,
    /// It from the cut `from` to the cut `to`, the curve's start or end
    /// where either is `None`. A circle has both, and loses what's
    /// counter-clockwise from `from` to `to`.
    Span { from: Option<Cut>, to: Option<Cut> },
}

/// Where extending a line or an arc takes an end: the curve it runs on
/// from the end on (see [`Sketch::ahead`]) and the cut there.
#[derive(Debug, Clone, PartialEq)]
struct Reached {
    ahead: Geom,
    /// The parameter of the end on `ahead`.
    from: f64,
    cut: Cut,
}

impl Sketch {
    /// Where the curves but `skip` meet `geom`, within `tolerance`, in
    /// order along it: each place where one or more do. `None` where a
    /// spline lies along another so closely that not every place is
    /// found ([`meet`] runs out of steps).
    fn cuts(&self, geom: &Geom, skip: Id, tolerance: f64) -> Option<Vec<Cut>> {
        let mut found = Vec::new();
        let mut places = Vec::new();
        for entry in self.curves.iter().filter(|entry| entry.id != skip) {
            let Some(other) = Geom::of(self, &entry.curve) else {
                continue;
            };
            found.clear();
            if meet(geom, &other, tolerance, &mut found) == usize::MAX {
                return None;
            }
            let ends = entry.curve.ends();
            for &(u, v) in &found {
                let end = ends.and_then(|[start, end]| {
                    if other.length(0.0, v) <= tolerance {
                        Some(start)
                    } else if other.length(v, other.last()) <= tolerance {
                        Some(end)
                    } else {
                        None
                    }
                });
                places.push((u, entry.id, end));
            }
        }
        places.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut cuts: Vec<Cut> = Vec::new();
        for (u, by, end) in places {
            match cuts.last_mut() {
                Some(cut) if geom.length(cut.u, u) <= tolerance => {
                    if !cut.by.contains(&by) {
                        cut.by.push(by);
                    }
                    cut.end = cut.end.or(end);
                }
                _ => cuts.push(Cut {
                    u,
                    by: vec![by],
                    end,
                }),
            }
        }
        // Round a circle, the last place and the first may be one.
        if !geom.has_ends()
            && cuts.len() > 1
            && let (Some(first), Some(last)) = (cuts.first(), cuts.last())
            && geom.length(last.u, first.u + geom.last()) <= tolerance
            && let Some(last) = cuts.pop()
        {
            let first = &mut cuts[0];
            for by in last.by {
                if !first.by.contains(&by) {
                    first.by.push(by);
                }
            }
            first.end = first.end.or(last.end);
        }
        Some(cuts)
    }

    /// What trimming `curve` near `near` takes away, of its shape: the
    /// span between the cuts either side of the place on it nearest
    /// `near`, or all of it where there are none (a circle needs two).
    /// `None` if `curve` is no curve of the sketch's, or has no size, or
    /// where it's cut can't all be found (see [`Sketch::cuts`]).
    fn trimmed(&self, curve: Id, near: DVec2) -> Option<(Geom, Trimmed)> {
        let geom = Geom::of(self, &self.curve(curve)?.curve)?;
        let tolerance = self.curve_tolerance();
        let mut cuts = self.cuts(&geom, curve, tolerance)?;
        // Where it meets what it's joined to, at its ends, cuts nothing.
        if geom.has_ends() {
            cuts.retain(|cut| {
                geom.length(0.0, cut.u) > tolerance && geom.length(cut.u, geom.last()) > tolerance
            });
        } else if cuts.len() < 2 {
            return Some((geom, Trimmed::Whole));
        }
        let u = geom.closest(near);
        let after = cuts.iter().position(|cut| cut.u > u);
        let before = after.unwrap_or(cuts.len()).checked_sub(1);
        let trimmed = if geom.has_ends() {
            let from = before.map(|i| cuts[i].clone());
            let to = after.map(|i| cuts[i].clone());
            if from.is_none() && to.is_none() {
                Trimmed::Whole
            } else {
                Trimmed::Span { from, to }
            }
        } else {
            // Round a circle from the last cut to the first.
            let from = before.unwrap_or(cuts.len() - 1);
            let to = after.unwrap_or(0);
            Trimmed::Span {
                from: Some(cuts[from].clone()),
                to: Some(cuts[to].clone()),
            }
        };
        Some((geom, trimmed))
    }

    /// The piece of `curve` that trimming it near `near` takes away (see
    /// [`SketchEdit::Trim`](crate::SketchEdit::Trim)), as a polyline: the
    /// whole curve as [`Sketch::flatten`] draws it where nothing cuts it.
    /// `None` if `curve` is no curve of the sketch's, or has no size.
    pub fn trim_piece(&self, curve: Id, near: DVec2) -> Option<Vec<DVec2>> {
        match self.trimmed(curve, near)? {
            (_, Trimmed::Whole) => self.flatten(&self.curve(curve)?.curve),
            (geom, Trimmed::Span { from, to }) => {
                let from = from.map_or(0.0, |cut| cut.u);
                let mut to = to.map_or(geom.last(), |cut| cut.u);
                if to < from {
                    to += geom.last();
                }
                Some(geom.polyline(from, to))
            }
        }
    }

    /// The end of the line, arc or open spline `curve` nearer `at`, its
    /// end where they're as near. `None` for anything else.
    pub fn nearer_end(&self, curve: Id, at: DVec2) -> Option<Id> {
        let [start, end] = self.curve(curve)?.curve.ends()?;
        let distance = |id| self.point(id).map(|point| point.at.distance(at));
        Some(if distance(start)? < distance(end)? {
            start
        } else {
            end
        })
    }

    /// The curve the line, arc or open spline `curve` runs on past its
    /// end `end`, and the parameter of `end` on it: a line on for `reach`,
    /// an arc round the rest of its circle, which runs counter-clockwise
    /// from its end to its start, a spline along its tangent there for
    /// `reach`. `Target` if `curve` is none of those, has no size, or `end`
    /// is no end of it.
    fn ahead(&self, curve: Id, end: Id, reach: f64) -> Result<(Geom, f64), EditError> {
        let entry = self.curve(curve).ok_or(EditError::Target(curve))?;
        let geom = Geom::of(self, &entry.curve).ok_or(EditError::Target(curve))?;
        let [first, last] = entry.curve.ends().ok_or(EditError::Target(curve))?;
        if end != first && end != last {
            return Err(EditError::Target(end));
        }
        Ok(match geom {
            Geom::Segment { start, end: finish } => {
                let (from, to) = if end == last {
                    (start, finish)
                } else {
                    (finish, start)
                };
                let way = (to - from).normalize();
                let ahead = Geom::Segment {
                    start: to,
                    end: to + way * reach,
                };
                (ahead, 0.0)
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                let ahead = Geom::Round {
                    center,
                    radius,
                    begin: begin + sweep,
                    sweep: TAU - sweep,
                };
                (ahead, if end == last { 0.0 } else { TAU - sweep })
            }
            // On along its tangent at the end, which the spline then
            // curves into (see `Sketch::extend_spline`).
            Geom::Spline(path) => {
                let (u, forward) = if end == last {
                    (path.last(), true)
                } else {
                    (0.0, false)
                };
                let from = path.at(u);
                let ahead = Geom::Segment {
                    start: from,
                    end: from + path.heading(u, forward).0 * reach,
                };
                (ahead, 0.0)
            }
        })
    }

    /// Where extending `end` of `curve` takes it, see [`Sketch::ahead`]:
    /// the first place past it where another curve meets it.
    /// `NothingAhead` if none does.
    fn reached(&self, curve: Id, end: Id, reach: f64) -> Result<Reached, EditError> {
        let (ahead, from) = self.ahead(curve, end, reach)?;
        let tolerance = self.curve_tolerance();
        let cuts = (self.cuts(&ahead, curve, tolerance)).ok_or(EditError::TooComplex)?;
        // Not where it is, nor, round an arc, back at its other end.
        let far = if from == 0.0 { ahead.last() } else { 0.0 };
        let cut = cuts
            .into_iter()
            .filter(|cut| {
                ahead.length(from, cut.u) > tolerance && ahead.length(cut.u, far) > tolerance
            })
            .min_by(|a, b| {
                let (a, b) = (ahead.length(from, a.u), ahead.length(from, b.u));
                a.total_cmp(&b)
            })
            .ok_or(EditError::NothingAhead)?;
        Ok(Reached { ahead, from, cut })
    }

    /// What extending `end` of the line, arc or spline `curve` adds to it
    /// (see [`SketchEdit::Extend`](crate::SketchEdit::Extend)), as a
    /// polyline from the end to where it would reach, a line or a spline
    /// looking at most `reach` ahead: the spline as it would be past its
    /// old end. `None` if it can't be extended.
    pub fn extension(&self, curve: Id, end: Id, reach: f64) -> Option<Vec<DVec2>> {
        if self.spline(curve).is_some() {
            let [_, last] = self.curve(curve)?.curve.ends()?;
            let old = self.point(end)?.at;
            let mut extended = self.clone();
            extended.extend(curve, end, reach).ok()?;
            let geom = Geom::of(&extended, &extended.curve(curve)?.curve)?;
            // Past its end, or before its start.
            let far = if end == last { geom.last() } else { 0.0 };
            return Some(geom.polyline(geom.closest(old), far));
        }
        let reached = self.reached(curve, end, reach).ok()?;
        Some(reached.ahead.polyline(reached.from, reached.cut.u))
    }

    /// The point a curve cut at `cut` of `geom` ends at: the end of a
    /// curve cutting it that's there, unless it's one of `own`, or else a
    /// new one, tied to the curves cutting it there by `ties`.
    fn cut_point(
        &mut self,
        geom: &Geom,
        cut: &Cut,
        own: &BTreeSet<Id>,
        ties: &mut Vec<Constraint>,
    ) -> Result<Id, OutOfIds> {
        if let Some(end) = cut.end.filter(|end| !own.contains(end)) {
            return Ok(end);
        }
        let point = self.add_point(geom.at(cut.u))?;
        ties.extend(
            cut.by
                .iter()
                .map(|&curve| Constraint::PointOnCurve { point, curve }),
        );
        Ok(point)
    }

    /// Trims `curve` near `near`, see
    /// [`SketchEdit::Trim`](crate::SketchEdit::Trim), giving the ids of
    /// the constraints tying its new ends, which are `auto`. The sketch
    /// may be left part way on failure.
    pub(crate) fn trim(&mut self, curve: Id, near: DVec2) -> Result<Vec<Id>, EditError> {
        // A fillet or a chamfer goes whole, giving its corner back.
        if self
            .curve(curve)
            .is_some_and(|entry| entry.corner.is_some())
        {
            self.delete(&[curve]);
            return Ok(Vec::new());
        }
        let (geom, trimmed) = self.trimmed(curve, near).ok_or(EditError::Target(curve))?;
        let Trimmed::Span { from, to } = trimmed else {
            self.delete(&[curve]);
            return Ok(Vec::new());
        };
        let entry = self.curve(curve).ok_or(EditError::Target(curve))?.clone();
        // A line ends at no point of a fillet or chamfer on it, which
        // would stop being one.
        let own = self.corner_points(curve);
        let mut ties = Vec::new();
        let mut gone = Vec::new();
        match (entry.curve.clone(), from, to) {
            (Curve::Circle { center, .. }, Some(from), Some(to)) => {
                // What's left runs counter-clockwise from `to` round to
                // `from`: an arc, keeping the circle's id.
                let start = self.cut_point(&geom, &to, &own, &mut ties)?;
                let end = self.cut_point(&geom, &from, &own, &mut ties)?;
                let number = self.next_number(Kind::Arc.name());
                let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
                entry.curve = Curve::Arc { center, start, end };
                entry.number = number;
            }
            (Curve::Line { start, end } | Curve::Arc { start, end, .. }, from, to) => {
                // What's before `from` keeps the curve's id, and what's
                // after `to` is a new curve of the same kind.
                let before = from
                    .map(|cut| self.cut_point(&geom, &cut, &own, &mut ties))
                    .transpose()?;
                let after = to
                    .map(|cut| self.cut_point(&geom, &cut, &own, &mut ties))
                    .transpose()?;
                match (before, after) {
                    (Some(new), None) => {
                        self.set_end(curve, end, new)?;
                        gone.push(end);
                    }
                    (None, Some(new)) => {
                        self.set_end(curve, start, new)?;
                        gone.push(start);
                    }
                    (Some(new_end), Some(new_start)) => {
                        self.set_end(curve, end, new_end)?;
                        let rest = match entry.curve {
                            Curve::Arc { center, .. } => Curve::Arc {
                                center,
                                start: new_start,
                                end,
                            },
                            _ => Curve::Line {
                                start: new_start,
                                end,
                            },
                        };
                        let rest = self.add_curve(rest, entry.construction)?;
                        // The two stay one line, or on one circle.
                        match entry.curve {
                            Curve::Arc { .. } => ties.push(Constraint::Equal(curve, rest)),
                            _ => ties.extend(
                                [new_start, end]
                                    .map(|point| Constraint::PointOnCurve { point, curve }),
                            ),
                        }
                        self.hand_over(curve, rest, end);
                    }
                    (None, None) => return Err(EditError::Target(curve)),
                }
            }
            (Curve::Spline(spline), from, to) => {
                gone = spline.all_points().collect();
                self.trim_spline(curve, &geom, [from, to], &own, &mut ties)?;
            }
            _ => return Err(EditError::Target(curve)),
        }
        self.finish_shape(curve, &gone, ties)
    }

    /// Trims the spline `curve`, of shape `geom`, taking away what's from
    /// the cut `from` to the cut `to` (see [`Trimmed::Span`]): what's
    /// before `from` keeps its id, and what's after `to`, if both are
    /// there, is a new spline, each a piece of it keeping its shape
    /// ([`Sketch::spline_piece`]); a closed one keeps what's from `to`
    /// round to `from`, open. Points held on it stay on the piece they're
    /// on, and go where they're on what's taken away. The ties of its new
    /// ends go onto `ties`.
    fn trim_spline(
        &mut self,
        curve: Id,
        geom: &Geom,
        [from, to]: [Option<Cut>; 2],
        own: &BTreeSet<Id>,
        ties: &mut Vec<Constraint>,
    ) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let tolerance = self.curve_tolerance();
        let entry = self.curve(curve).ok_or(target)?.clone();
        let Curve::Spline(spline) = &entry.curve else {
            return Err(target);
        };
        // Where each point held on it is along it.
        let on: Vec<(Id, f64)> = self
            .constraints
            .iter()
            .filter_map(|held| match held.constraint {
                Constraint::PointOnCurve { point, curve: on } if on == curve => {
                    Some((held.id, geom.closest(self.point(point)?.at)))
                }
                _ => None,
            })
            .collect();
        let mut cut = |sketch: &mut Sketch, cut: Option<Cut>| {
            cut.map(|cut| Ok::<_, OutOfIds>((cut.u, sketch.cut_point(geom, &cut, own, ties)?)))
                .transpose()
        };
        let (before, after) = (cut(self, from)?, cut(self, to)?);
        let last = geom.last();
        // The parts kept, each from and to where along it, and whether
        // the second is a spline of its own.
        let (kept, rest) = match (spline.ends(), before, after) {
            (Some([first, _]), Some((a, new)), after) => {
                let piece = self.spline_piece(curve, [0.0, a], [first, new], tolerance)?;
                ((piece, [0.0, a]), after)
            }
            (Some([_, end]), None, Some((b, new))) => {
                let piece = self.spline_piece(curve, [b, last], [new, end], tolerance)?;
                ((piece, [b, last]), None)
            }
            // Round from `to` to `from`.
            (None, Some((a, end)), Some((b, start))) => {
                let a = if a > b { a } else { a + last };
                let piece = self.spline_piece(curve, [b, a], [start, end], tolerance)?;
                ((piece, [b, a]), None)
            }
            _ => return Err(target),
        };
        let rest = match (rest, spline.ends()) {
            (Some((b, new)), Some([_, end])) => {
                let piece = self.spline_piece(curve, [b, last], [new, end], tolerance)?;
                let rest = self.add_curve(Curve::Spline(piece), entry.construction)?;
                self.hand_over(curve, rest, end);
                Some((rest, b))
            }
            _ => None,
        };
        let (piece, [low, high]) = kept;
        self.curve_mut(curve).ok_or(target)?.curve = Curve::Spline(piece);
        // Points on what's taken away go; those on the rest are on it.
        let within = |u: f64| {
            let u = if u < low { u + last } else { u };
            u >= low && u <= high
        };
        for (id, u) in on {
            match rest {
                Some((rest, b)) if u >= b => {
                    let held = self.constraints.iter_mut().find(|held| held.id == id);
                    if let Some(held) = held
                        && let Constraint::PointOnCurve { curve: on, .. } = &mut held.constraint
                    {
                        *on = rest;
                    }
                }
                _ if within(u) => {}
                _ => self.constraints.retain(|held| held.id != id),
            }
        }
        Ok(())
    }

    /// Extends `end` of the line, arc or spline `curve` to the next curve
    /// it meets, a line or a spline looking at most `reach` ahead, see
    /// [`SketchEdit::Extend`](crate::SketchEdit::Extend), giving the ids
    /// of the constraints tying its new end, which are `auto`. The sketch
    /// may be left part way on failure.
    pub(crate) fn extend(&mut self, curve: Id, end: Id, reach: f64) -> Result<Vec<Id>, EditError> {
        if self
            .curve(curve)
            .is_some_and(|entry| entry.corner.is_some())
        {
            return Err(EditError::Target(curve));
        }
        let reached = self.reached(curve, end, reach)?;
        let own = self.corner_points(curve);
        let mut ties = Vec::new();
        let new = self.cut_point(&reached.ahead, &reached.cut, &own, &mut ties)?;
        if self.spline(curve).is_some() {
            self.extend_spline(curve, end, new)?;
            return self.finish_shape(curve, &[], ties);
        }
        self.set_end(curve, end, new)?;
        self.finish_shape(curve, &[end], ties)
    }

    /// Makes the spline `curve` end at `new` past its end `end`, which it
    /// then runs on through, curving into the way to `new` smoothly:
    /// through fit points `new` a fit point more, by control points a
    /// control point more, its knots squeezed into the part of its
    /// parameter the old polygon's length takes and one more where the
    /// extension starts. What held it tangent or smooth at `end` goes, as
    /// it no longer ends there.
    fn extend_spline(&mut self, curve: Id, end: Id, new: Id) -> Result<(), EditError> {
        let target = EditError::Target(curve);
        let mut spline = self.spline(curve).ok_or(target)?.clone();
        let [_, last] = spline.ends().ok_or(target)?;
        let at_end = end == last;
        if at_end {
            spline.points.push(new);
        } else {
            spline.points.insert(0, new);
        }
        if spline.kind == SplineKind::Control {
            let places = self.places(&spline.points).ok_or(target)?;
            let (old, reach) = if at_end {
                let old = &places[..places.len() - 1];
                (old, places[places.len() - 1].distance(old[old.len() - 1]))
            } else {
                (&places[1..], places[0].distance(places[1]))
            };
            let length: f64 = old.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
            let share = length / (length + reach);
            let knots: Vec<f64> = if at_end {
                let old = spline.knots.iter().map(|&k| k * share);
                old.chain([share]).collect()
            } else {
                let start = 1.0 - share;
                let old = spline.knots.iter().map(|&k| start + k * share);
                std::iter::once(start).chain(old).collect()
            };
            let sound = BSpline::clamped(&knots, places.clone()).is_some();
            spline.knots = if sound {
                knots
            } else {
                control_knots(&places, false)
            };
        }
        self.curve_mut(curve).ok_or(target)?.curve = Curve::Spline(spline);
        self.constraints.retain(|entry| match entry.constraint {
            Constraint::Tangent {
                a, b, at: Some(at), ..
            }
            | Constraint::Smooth { a, b, at, .. } => at != end || (a != curve && b != curve),
            _ => true,
        });
        Ok(())
    }

    /// The points of the fillets and chamfers on the line `line`.
    fn corner_points(&self, line: Id) -> BTreeSet<Id> {
        let on = |corner: &Corner| corner.a == line || corner.b == line;
        self.curves
            .iter()
            .filter(|entry| entry.corner.as_ref().is_some_and(on))
            .flat_map(|entry| entry.curve.points())
            .collect()
    }

    /// Makes the line or arc `curve` end at `new` where it ended at `old`.
    fn set_end(&mut self, curve: Id, old: Id, new: Id) -> Result<(), EditError> {
        let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
        match &mut entry.curve {
            Curve::Line { start, end } | Curve::Arc { start, end, .. } => {
                for point in [start, end] {
                    if *point == old {
                        *point = new;
                        return Ok(());
                    }
                }
                Err(EditError::Target(old))
            }
            Curve::Circle { .. } | Curve::Spline(_) => Err(EditError::Target(curve)),
        }
    }

    /// Hands what's on `curve` at its end `end`, now `rest`'s, to `rest`:
    /// the tangents with a curve `end` is an end of too, which touch there
    /// (a spline's, and smooth joins, where they're at `end`), and the
    /// fillets and chamfers on the corner there.
    fn hand_over(&mut self, curve: Id, rest: Id, end: Id) {
        for entry in &mut self.curves {
            if let Some(corner) = &mut entry.corner
                && corner.at == end
            {
                for line in [&mut corner.a, &mut corner.b] {
                    if *line == curve {
                        *line = rest;
                    }
                }
            }
        }
        let ends_at = |sketch: &Sketch, other: Id| {
            let ends = sketch.curve(other).and_then(|entry| entry.curve.ends());
            ends.is_some_and(|ends| ends.contains(&end))
        };
        let moved: Vec<(usize, Constraint)> = self
            .constraints
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let mut constraint = entry.constraint.clone();
                // With a spline, where it touches is named.
                let (a, b, at) = match &mut constraint {
                    Constraint::Tangent { a, b, at, .. } => (a, b, *at),
                    Constraint::Smooth { a, b, at, .. } => (a, b, Some(*at)),
                    _ => return None,
                };
                let touches = |other: Id| at.map_or_else(|| ends_at(self, other), |at| at == end);
                if *a == curve && touches(*b) {
                    *a = rest;
                } else if *b == curve && touches(*a) {
                    *b = rest;
                } else {
                    return None;
                }
                Some((index, constraint))
            })
            .collect();
        for (index, constraint) in moved {
            self.constraints[index].constraint = constraint;
        }
    }

    /// Drops what held the length of `line`, which trimming or extending
    /// changes: an equal length, a midpoint, a driving length, or
    /// distance from its midpoint to another line. What's on the endless
    /// line through it (a point on it, parallel, an angle) still holds.
    fn reshaped(&mut self, line: Id) {
        self.constraints.retain(|entry| match entry.constraint {
            Constraint::Equal(a, b) => a != line && b != line,
            Constraint::Midpoint { line: on, .. } => on != line,
            _ => true,
        });
        let lines: BTreeSet<Id> = self
            .dimensions
            .iter()
            .filter_map(|entry| match entry.dimension.measure {
                Measure::Distance(a, b) if a == line => Some(b),
                _ => None,
            })
            .filter(|&b| self.line(b).is_some())
            .collect();
        self.dimensions.retain(|entry| {
            !entry.dimension.driving
                || match entry.dimension.measure {
                    Measure::Length(of) => of != line,
                    Measure::Distance(a, b) => a != line || !lines.contains(&b),
                    _ => true,
                }
        });
    }

    /// Ends a trim or an extension of `curve`: what held its length goes if
    /// it's a line ([`Sketch::reshaped`]), and the fillets and chamfers on
    /// corners it no longer makes; the points of `gone` no curve is made
    /// from any more are deleted, with what's on them; constraints and
    /// dimensions naming a curve and one of its own points, which the
    /// shape's new ends can make of a point on it, are dropped; and `ties`
    /// are added, giving their ids.
    fn finish_shape(
        &mut self,
        curve: Id,
        gone: &[Id],
        ties: Vec<Constraint>,
    ) -> Result<Vec<Id>, EditError> {
        if self.kind(curve) == Some(Kind::Line) {
            self.reshaped(curve);
        }
        self.drop_broken_corners();
        let orphans: Vec<Id> = gone
            .iter()
            .copied()
            .filter(|&point| {
                !self
                    .curves
                    .iter()
                    .any(|entry| entry.curve.points().any(|id| id == point))
            })
            .collect();
        self.delete(&orphans);
        let own: BTreeSet<Id> = self
            .constraints
            .iter()
            .filter(|entry| entry.constraint.own_point(self).is_some())
            .map(|entry| entry.id)
            .chain(
                self.dimensions
                    .iter()
                    .filter(|entry| entry.dimension.measure.own_point(self).is_some())
                    .map(|entry| entry.id),
            )
            .collect();
        self.constraints.retain(|entry| !own.contains(&entry.id));
        self.dimensions.retain(|entry| !own.contains(&entry.id));
        let mut added = Vec::with_capacity(ties.len());
        for tie in ties {
            // A tie with a curve's own point, joining its end to itself,
            // or with a fillet or chamfer gone with its corner.
            let there = tie.items().all(|(id, _)| self.kind(id).is_some());
            if there && tie.own_point(self).is_none() {
                added.push(self.add_constraint(tie)?);
            }
        }
        Ok(added)
    }

    /// Mirrors the points and curves among `ids` about the line `about`,
    /// see [`SketchEdit::Mirror`](crate::SketchEdit::Mirror), giving the
    /// ids of the constraints holding points on the line there, which are
    /// `auto`. The sketch may be left part way on failure.
    pub(crate) fn mirror(&mut self, ids: &[Id], about: Id) -> Result<Vec<Id>, EditError> {
        let (a, b) = self.line(about).ok_or(EditError::Target(about))?;
        let along = (b - a).try_normalize().ok_or(EditError::Target(about))?;
        let reflect = |p: DVec2| 2.0 * foot(p, a, b) - p;
        let tolerance = self.curve_tolerance();
        let on_line = |p: DVec2| (p - a).perp_dot(along).abs() <= tolerance;
        let own: BTreeSet<Id> = self
            .curve(about)
            .map(|entry| entry.curve.points().collect())
            .unwrap_or_default();

        let ids: BTreeSet<Id> = ids.iter().copied().filter(|&id| id != about).collect();
        let at = |id: Id| self.point(id).map(|point| point.at);
        let arc_points: BTreeSet<Id> = self
            .curves
            .iter()
            .filter(|entry| ids.contains(&entry.id) && matches!(entry.curve, Curve::Arc { .. }))
            .flat_map(|entry| entry.curve.points())
            .collect();
        // A point on the line is its own mirror image, and a curve's there
        // is shared with its copy, but for an arc's, whose copy's radius
        // follows from the symmetry of all three of its points (see the
        // solver's system). A curve all of whose points are so is its own
        // mirror image, and isn't copied.
        let shared = |id: Id| !arc_points.contains(&id) && at(id).is_some_and(on_line);
        let (curves, own_images): (Vec<_>, Vec<_>) = self
            .curves
            .iter()
            .filter(|entry| ids.contains(&entry.id))
            .cloned()
            .partition(|entry| !entry.curve.points().all(shared));
        let own_images: BTreeSet<Id> = own_images.iter().map(|entry| entry.id).collect();
        let used: BTreeSet<Id> = curves
            .iter()
            .flat_map(|entry| entry.curve.points())
            .collect();
        // Lone points selected off the line, and the curves' points, each
        // once.
        let lone = ids
            .iter()
            .copied()
            .filter(|&id| !id.is_builtin() && self.point(id).is_some() && !shared(id));
        let points: BTreeSet<Id> = lone.chain(used.iter().copied()).collect();

        let placed = points
            .iter()
            .map(|&point| {
                Ok((
                    point,
                    at(point).ok_or(EditError::Target(point))?,
                    shared(point),
                ))
            })
            .collect::<Result<Vec<_>, EditError>>()?;

        let mut images = BTreeMap::new();
        let mut symmetric = Vec::new();
        let mut ties = Vec::new();
        for (point, place, shared) in placed {
            if used.contains(&point) && on_line(place) && !own.contains(&point) {
                ties.push(Constraint::PointOnCurve {
                    point,
                    curve: about,
                });
            }
            let image = if shared {
                point
            } else {
                let image = self.add_point(reflect(place))?;
                symmetric.push(Constraint::Symmetric {
                    a: point,
                    b: image,
                    about,
                });
                image
            };
            images.insert(point, image);
        }
        let image = |id: Id| images.get(&id).copied().ok_or(EditError::Target(id));
        let mut equal = Vec::new();
        let mut copies = BTreeMap::new();
        for entry in &curves {
            let curve = match entry.curve {
                // Mirrored, counter-clockwise runs the other way.
                Curve::Arc { center, start, end } => Curve::Arc {
                    center: image(center)?,
                    start: image(end)?,
                    end: image(start)?,
                },
                // A spline's mirror image is the spline of its points'.
                ref curve => curve.map_points(image)?,
            };
            let copy = self.add_curve(curve, entry.construction)?;
            if let Curve::Circle { .. } = entry.curve {
                equal.push(Constraint::Equal(entry.id, copy));
            }
            copies.insert(entry.id, copy);
        }
        // A fillet or chamfer copied with its lines is a corner of theirs,
        // its equations restating the symmetry, which the solver leaves
        // out (see its system). A line that's its own image, as the line
        // mirrored about is, is its own copy.
        for entry in &curves {
            let Some(corner) = entry.corner else {
                continue;
            };
            let line = |id: Id| {
                copies
                    .get(&id)
                    .copied()
                    .or_else(|| (own_images.contains(&id) || id == about).then_some(id))
            };
            let (Some(a), Some(b), Some(&at), Some(&copy)) = (
                line(corner.a),
                line(corner.b),
                images.get(&corner.at),
                copies.get(&entry.id),
            ) else {
                continue;
            };
            // A mirrored arc runs the other way, from the image of its end,
            // on `b`'s image.
            let (a, b) = match entry.curve {
                Curve::Arc { .. } => (b, a),
                _ => (a, b),
            };
            self.set_corner(copy, Corner { a, b, at, ..corner });
        }
        if curves.is_empty() && symmetric.is_empty() {
            return Err(EditError::NothingToMirror);
        }
        for constraint in symmetric.into_iter().chain(equal) {
            self.add_constraint(constraint)?;
        }
        let mut added = Vec::with_capacity(ties.len());
        for tie in ties {
            if tie.fits_builtins() {
                added.push(self.add_constraint(tie)?);
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests;

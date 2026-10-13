//! Trimming a curve back to the curves cutting it
//! ([`SketchEdit::Trim`](crate::SketchEdit::Trim)).

use std::collections::BTreeSet;

use glam::DVec2;

use super::*;
use crate::intersect::Geom;
use crate::{Constraint, Curve, EditError, Id, Kind, Noun, OutOfIds, Sketch};

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

impl Sketch {
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

    /// Trims `curve` near `near`, see
    /// [`SketchEdit::Trim`](crate::SketchEdit::Trim), giving the ids of
    /// the constraints tying its new ends, which are `auto`. The sketch
    /// may be left part way on failure.
    pub(crate) fn trim(&mut self, curve: Id, near: DVec2) -> Result<Vec<Id>, EditError> {
        // A fillet or a chamfer goes whole, giving its corner back.
        if self.is_corner(curve) {
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
                let number = self.next_number(Noun::Item(Kind::Arc));
                let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
                entry.curve = Curve::Arc { center, start, end };
                entry.number = number;
            }
            // A closed arc is cut as a circle is, its point going unless
            // a cut is there or another curve has it.
            (Curve::Arc { center, start, end }, Some(from), Some(to)) if start == end => {
                let start_at = self.cut_point(&geom, &to, &own, &mut ties)?;
                let end_at = self.cut_point(&geom, &from, &own, &mut ties)?;
                let entry = self.curve_mut(curve).ok_or(EditError::Target(curve))?;
                entry.curve = Curve::Arc {
                    center,
                    start: start_at,
                    end: end_at,
                };
                gone.push(start);
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
        let mut gone = BTreeSet::new();
        for (id, u) in on {
            match rest {
                Some((rest, b)) if u >= b => {
                    if let Some(held) = self.constraint_mut(id)
                        && let Constraint::PointOnCurve { curve: on, .. } = &mut held.constraint
                    {
                        *on = rest;
                    }
                }
                _ if within(u) => {}
                _ => {
                    gone.insert(id);
                }
            }
        }
        self.constraints.retain(|held| !gone.contains(&held.id));
        Ok(())
    }
}

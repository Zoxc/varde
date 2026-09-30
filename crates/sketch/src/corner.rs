//! Fillets and chamfers: the corner where two lines end rounded by an arc
//! tangent to both, or cut by a line across it
//! ([`SketchEdit::Fillet`](crate::SketchEdit::Fillet),
//! [`SketchEdit::Chamfer`](crate::SketchEdit::Chamfer)). Each is added to
//! the corner rather than replacing it: the lines stay whole, still there
//! to constrain and dimension to, and their ends from the corner to where
//! the fillet or chamfer meets them are cut off ([`Sketch::cut_back`]):
//! drawn dashed, and not part of a profile. Deleting the fillet or chamfer
//! gives the sharp corner back.

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use varde_expr::Value;

use crate::angle;
use crate::{
    ArcPoints, Curve, CurveEntry, Dimension, EditError, Id, Measure, OutOfIds, Side, Sketch,
};

/// Legs nearer parallel than this, the sine of the angle between them,
/// make no corner: where a fillet or a chamfer would go is far off, or
/// nowhere.
const PARALLEL: f64 = 1e-9;

/// How far a new fillet's or chamfer's labels are from their anchors,
/// in its sizes.
const LABEL_REACH: f64 = 1.5;

/// The corner a fillet or a chamfer is on: where the lines `a` and `b` end
/// at the point `at`. The curve that has it runs from its start, on `a`,
/// to its end, on `b`: a fillet is an arc, counter-clockwise, so that it
/// takes the short way round inside the corner, and a chamfer a line.
///
/// It implies equations, held by the solver as an arc's radius is: the
/// start on the endless line through `a` and the end on `b`'s, and for a
/// fillet its radius at each at right angles to the line, towards the
/// inside of the corner (sided, as an angle is: never its mirror image).
/// A chamfer with `equal` has its ends as far from the corner as each
/// other. Its size is the dimensions' to hold: a fillet's radius, a
/// chamfer's distances along the lines, or a distance and an angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Corner {
    pub a: Id,
    pub b: Id,
    pub at: Id,
    /// A chamfer's ends as far from the corner as each other. Never so for
    /// a fillet, whose tangency makes them.
    pub equal: bool,
}

/// How far back a chamfer cuts its corner: `V` is a [`Value`] as typed in
/// an edit, or a number, in millimetres and radians, for a preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Setback<V = Value> {
    /// Both ends this far from the corner along their lines, held equal.
    Equal(V),
    /// The end on the first line the first distance along it from the
    /// corner, the end on the second the second.
    Two(V, V),
    /// The end on the first line this far along it from the corner, the
    /// chamfer at this angle to it, inside the corner.
    Angle(V, V),
}

impl Corner {
    /// What the curve with a corner is called: a fillet if it's an arc, a
    /// chamfer if it's a line.
    pub(crate) fn noun(curve: &Curve) -> &'static str {
        match curve {
            Curve::Arc { .. } => "Fillet",
            _ => "Chamfer",
        }
    }
}

impl<V> Setback<V> {
    /// The same setback with `f` of each value.
    pub fn map<W>(&self, f: impl Fn(&V) -> W) -> Setback<W> {
        match self {
            Setback::Equal(d) => Setback::Equal(f(d)),
            Setback::Two(a, b) => Setback::Two(f(a), f(b)),
            Setback::Angle(d, angle) => Setback::Angle(f(d), f(angle)),
        }
    }
}

/// A corner's two lines seen from it: where it is, the way along each line
/// from it, as a unit vector, and how long each is.
#[derive(Debug, Clone, Copy)]
struct Legs {
    corner: DVec2,
    a: DVec2,
    b: DVec2,
    a_length: f64,
    b_length: f64,
}

impl Legs {
    /// Half the angle between the legs, above zero and below a right
    /// angle.
    fn half(&self) -> f64 {
        angle::atan2(self.a.perp_dot(self.b).abs(), self.a.dot(self.b)) / 2.0
    }

    /// The way from the corner into it, halfway between the legs.
    fn bisector(&self) -> DVec2 {
        (self.a + self.b).normalize()
    }

    /// The legs the other way round.
    fn swapped(self) -> Legs {
        Legs {
            a: self.b,
            b: self.a,
            a_length: self.b_length,
            b_length: self.a_length,
            ..self
        }
    }
}

impl Sketch {
    /// The way along `line` from its end `at`, as a unit vector, and its
    /// length, if it's a line (not a chamfer) with a length that ends
    /// there.
    fn leg(&self, at: Id, line: Id) -> Option<(DVec2, f64)> {
        let entry = self.curve(line).filter(|entry| entry.corner.is_none())?;
        let Curve::Line { start, end } = entry.curve else {
            return None;
        };
        let other = match at {
            _ if start == at => end,
            _ if end == at => start,
            _ => return None,
        };
        let corner = self.point(at)?.at;
        let along = self.point(other)?.at - corner;
        let length = along.length();
        (length > 0.0 && length.is_finite()).then(|| (along / length, length))
    }

    /// The corner at `at` of the lines `a` and `b`, if they make one: two
    /// lines, not chamfers, ending there, with lengths, not parallel.
    fn legs(&self, at: Id, [a, b]: [Id; 2]) -> Option<Legs> {
        if a == b {
            return None;
        }
        let ((a, a_length), (b, b_length)) = (self.leg(at, a)?, self.leg(at, b)?);
        (a.perp_dot(b).abs() > PARALLEL).then_some(Legs {
            corner: self.point(at)?.at,
            a,
            b,
            a_length,
            b_length,
        })
    }

    /// The lines a fillet or a chamfer on the corner at the point `at`
    /// would be on, the one running nearest the way towards `toward` first,
    /// and of the others the nearest that makes a corner with it: lines
    /// ending there, not chamfers, not parallel. `None` without two such.
    pub fn corner_lines(&self, at: Id, toward: DVec2) -> Option<[Id; 2]> {
        let corner = self.point(at)?.at;
        let way = toward - corner;
        let mut legs: Vec<(f64, Id, DVec2)> = self
            .curves
            .iter()
            .filter_map(|entry| {
                let (along, _) = self.leg(at, entry.id)?;
                let off = angle::atan2(along.perp_dot(way).abs(), along.dot(way));
                Some((off, entry.id, along))
            })
            .collect();
        legs.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        let (_, first, along) = *legs.first()?;
        let (_, second, _) = legs[1..]
            .iter()
            .find(|(_, _, other)| along.perp_dot(*other).abs() > PARALLEL)?;
        Some([first, *second])
    }

    /// The point the lines `a` and `b` make a corner at, if they do: an end
    /// of both, as [`Sketch::corner_lines`] would pick them.
    pub fn corner_of(&self, a: Id, b: Id) -> Option<Id> {
        let Curve::Line { start, end } = self.curve(a)?.curve else {
            return None;
        };
        [start, end]
            .into_iter()
            .find(|&at| self.legs(at, [a, b]).is_some())
    }

    /// The fillet of `radius` on the corner at `at` of `lines`, as an arc
    /// counter-clockwise, and the lines in the order its start and its end
    /// are on. `NoCorner` if they make none (see [`Sketch::corner_lines`]),
    /// `NoRoom` if it doesn't fit along both lines, or the radius is no
    /// length.
    fn fillet_at(
        &self,
        at: Id,
        lines: [Id; 2],
        radius: f64,
    ) -> Result<(ArcPoints, [Id; 2]), EditError> {
        let legs = self.legs(at, lines).ok_or(EditError::NoCorner)?;
        // Counter-clockwise from the end on the first line to that on the
        // second, the short way, turns from the first leg to the second
        // clockwise.
        let (legs, lines) = if legs.a.perp_dot(legs.b) > 0.0 {
            (legs.swapped(), [lines[1], lines[0]])
        } else {
            (legs, lines)
        };
        let half = legs.half();
        let back = radius / angle::tan(half);
        if !(radius > 0.0 && back <= legs.a_length && back <= legs.b_length) {
            return Err(EditError::NoRoom);
        }
        let arc = ArcPoints {
            center: legs.corner + legs.bisector() * (radius / angle::sin(half)),
            start: legs.corner + legs.a * back,
            end: legs.corner + legs.b * back,
        };
        Ok((arc, lines))
    }

    /// Where the chamfer `setback` makes on the corner at `at` of `lines`
    /// meets them, on the first then the second. `NoCorner` if they make
    /// none, `NoRoom` if it doesn't fit along both lines, or reach across
    /// at that angle, or a distance is no length.
    fn chamfer_at(
        &self,
        at: Id,
        lines: [Id; 2],
        setback: &Setback<f64>,
    ) -> Result<[DVec2; 2], EditError> {
        let legs = self.legs(at, lines).ok_or(EditError::NoCorner)?;
        let (along_a, along_b) = match *setback {
            Setback::Equal(d) => (d, d),
            Setback::Two(a, b) => (a, b),
            // The triangle cut off has the corner's angle at the corner
            // and `angle` at the first end, so by the sines, the second
            // end is this far along.
            Setback::Angle(d, angle) => {
                let far = 2.0 * legs.half() + angle;
                if !(angle > 0.0 && far < PI) {
                    return Err(EditError::NoRoom);
                }
                (d, d * angle::sin(angle) / angle::sin(far))
            }
        };
        let fits = |along: f64, length: f64| along > 0.0 && along <= length;
        if !(fits(along_a, legs.a_length) && fits(along_b, legs.b_length)) {
            return Err(EditError::NoRoom);
        }
        Ok([
            legs.corner + legs.a * along_a,
            legs.corner + legs.b * along_b,
        ])
    }

    /// The fillet of `radius` a [`SketchEdit::Fillet`](crate::SketchEdit::Fillet)
    /// on the corner at `at` of `lines` makes, as an arc counter-clockwise.
    /// `None` if they make no corner or it doesn't fit.
    pub fn fillet_preview(&self, at: Id, lines: [Id; 2], radius: f64) -> Option<ArcPoints> {
        self.fillet_at(at, lines, radius).ok().map(|(arc, _)| arc)
    }

    /// The chamfer a [`SketchEdit::Chamfer`](crate::SketchEdit::Chamfer)
    /// cutting the corner at `at` of `lines` back by `setback` makes: its
    /// ends, on the first line then the second. `None` if they make no
    /// corner or it doesn't fit.
    pub fn chamfer_preview(
        &self,
        at: Id,
        lines: [Id; 2],
        setback: &Setback<f64>,
    ) -> Option<[DVec2; 2]> {
        self.chamfer_at(at, lines, setback).ok()
    }

    /// How far `place` is into the corner at `at` of `lines`, along the
    /// way halfway between them, if it's in it at all.
    fn depth_into(&self, at: Id, lines: [Id; 2], place: DVec2) -> Option<(Legs, f64)> {
        let legs = self.legs(at, lines)?;
        let into = (place - legs.corner).dot(legs.bisector());
        (into > 0.0 && into.is_finite()).then_some((legs, into))
    }

    /// The radius of the fillet on the corner at `at` of `lines` whose
    /// middle is as far into the corner as `place`, for the Fillet tool's
    /// preview through the cursor. `None` if `place` isn't into it.
    pub fn fillet_through(&self, at: Id, lines: [Id; 2], place: DVec2) -> Option<f64> {
        let (legs, into) = self.depth_into(at, lines, place)?;
        // The centre is `radius / sin(half)` in, the middle a radius less.
        let sin = angle::sin(legs.half());
        Some(into * sin / (1.0 - sin))
    }

    /// How far along each line the chamfer on the corner at `at` of
    /// `lines`, cutting both back as far, is when it crosses the way into
    /// the corner as far in as `place`, for the Chamfer tool's preview
    /// through the cursor. `None` if `place` isn't into it.
    pub fn chamfer_through(&self, at: Id, lines: [Id; 2], place: DVec2) -> Option<f64> {
        let (legs, into) = self.depth_into(at, lines, place)?;
        Some(into / angle::cos(legs.half()))
    }

    /// The lines fillets and chamfers cut back, each with what's left of
    /// it: the parameters, from 0 at its start to 1 at its end, where the
    /// fillet or chamfer at each of its ends meets it (0 and 1 at an end
    /// without one, a meeting past the line's other end at it). The first
    /// is past the second where they overlap, leaving nothing. For drawing
    /// the lines' ends cut off dashed, and leaving them out of profiles.
    pub fn cut_back(&self) -> HashMap<Id, [f64; 2]> {
        let mut cut: HashMap<Id, [f64; 2]> = HashMap::new();
        for entry in &self.curves {
            let Some(corner) = entry.corner else {
                continue;
            };
            let Some(ends) = entry.curve.ends() else {
                continue;
            };
            for (line, meets) in [(corner.a, ends[0]), (corner.b, ends[1])] {
                let (Some(Curve::Line { start, end }), Some(meets)) = (
                    self.curve(line).map(|entry| &entry.curve),
                    self.point(meets),
                ) else {
                    continue;
                };
                let (Some(from), Some(to)) = (self.point(*start), self.point(*end)) else {
                    continue;
                };
                let along = to.at - from.at;
                let u = along.dot(meets.at - from.at) / along.length_squared();
                if !u.is_finite() {
                    continue;
                }
                let u = u.clamp(0.0, 1.0);
                let kept = cut.entry(line).or_insert([0.0, 1.0]);
                if *start == corner.at {
                    kept[0] = kept[0].max(u);
                } else if *end == corner.at {
                    kept[1] = kept[1].min(u);
                }
            }
        }
        cut
    }

    /// Adds `curve` as the fillet or chamfer on `corner`, giving its id.
    fn add_corner(
        &mut self,
        curve: Curve,
        corner: Corner,
        construction: bool,
    ) -> Result<Id, OutOfIds> {
        let id = self.add_curve(curve, construction)?;
        self.set_corner(id, corner);
        Ok(id)
    }

    /// Makes the arc or line `id` the fillet or chamfer on `corner`,
    /// numbered as one.
    pub(crate) fn set_corner(&mut self, id: Id, corner: Corner) {
        let Some(entry) = self.curve(id) else {
            return;
        };
        let number = self.next_number(Corner::noun(&entry.curve));
        if let Some(entry) = self.curve_mut(id) {
            entry.corner = Some(corner);
            entry.number = number;
        }
    }

    /// Whether the fillet or chamfer `id` would be on the corner at `at`
    /// of `lines`, either way round.
    fn on_corner(&self, id: Id, at: Id, [a, b]: [Id; 2]) -> bool {
        self.curve(id)
            .and_then(|entry| entry.corner)
            .is_some_and(|corner| {
                corner.at == at
                    && ((corner.a, corner.b) == (a, b) || (corner.a, corner.b) == (b, a))
            })
    }

    /// Deletes the fillet or chamfer on the corner at `at` of `lines`, if
    /// there is one: a new one replaces it.
    fn uncorner(&mut self, at: Id, lines: [Id; 2]) {
        let old: Vec<Id> = self
            .curves
            .iter()
            .filter(|entry| self.on_corner(entry.id, at, lines))
            .map(|entry| entry.id)
            .collect();
        self.delete(&old);
    }

    /// Whether a fillet or a chamfer on `lines` is construction geometry:
    /// where both are.
    fn construction_of(&self, [a, b]: [Id; 2]) -> bool {
        let construction = |id| self.curve(id).is_some_and(|entry| entry.construction);
        construction(a) && construction(b)
    }

    /// Rounds the corner at `at` of `lines` with a fillet of `radius`, see
    /// [`SketchEdit::Fillet`](crate::SketchEdit::Fillet). The sketch may be
    /// left part way on failure.
    pub(crate) fn fillet(
        &mut self,
        at: Id,
        lines: [Id; 2],
        radius: &Value,
    ) -> Result<(), EditError> {
        let (arc, [a, b]) = self.fillet_at(at, lines, radius.value)?;
        self.uncorner(at, lines);
        let construction = self.construction_of(lines);
        let center = self.add_point(arc.center)?;
        let start = self.add_point(arc.start)?;
        let end = self.add_point(arc.end)?;
        let corner = Corner {
            a,
            b,
            at,
            equal: false,
        };
        let fillet = self.add_corner(Curve::Arc { center, start, end }, corner, construction)?;
        // Out past the arc's middle, from its centre.
        let middle = (arc.start + arc.end) / 2.0 - arc.center;
        let label = middle.normalize_or_zero() * radius.value * LABEL_REACH;
        self.add_dimension(Dimension {
            measure: Measure::Radius(fillet),
            value: radius.clone(),
            driving: true,
            label,
            side: Side::Positive,
        })?;
        Ok(())
    }

    /// Cuts the corner at `at` of `lines` with a chamfer as far back as
    /// `setback` says, see [`SketchEdit::Chamfer`](crate::SketchEdit::Chamfer).
    /// The sketch may be left part way on failure.
    pub(crate) fn chamfer(
        &mut self,
        at: Id,
        lines: [Id; 2],
        setback: &Setback,
    ) -> Result<(), EditError> {
        let [start_at, end_at] = self.chamfer_at(at, lines, &setback.map(|value| value.value))?;
        self.uncorner(at, lines);
        let construction = self.construction_of(lines);
        let corner_at = self.point(at).ok_or(EditError::NoCorner)?.at;
        let start = self.add_point(start_at)?;
        let end = self.add_point(end_at)?;
        let corner = Corner {
            a: lines[0],
            b: lines[1],
            at,
            equal: matches!(setback, Setback::Equal(_)),
        };
        let chamfer = self.add_corner(Curve::Line { start, end }, corner, construction)?;
        // Beside each distance, outside the corner.
        let inside = ((start_at - corner_at).normalize_or_zero()
            + (end_at - corner_at).normalize_or_zero())
        .normalize_or_zero();
        let distance = |sketch: &mut Sketch, to: Id, value: &Value| {
            sketch.add_dimension(Dimension {
                measure: Measure::Distance(at, to),
                value: value.clone(),
                driving: true,
                label: -inside * value.value * LABEL_REACH / 2.0,
                side: Side::Positive,
            })
        };
        match setback {
            Setback::Equal(d) => {
                distance(self, start, d)?;
            }
            Setback::Two(a, b) => {
                distance(self, start, a)?;
                distance(self, end, b)?;
            }
            Setback::Angle(d, angle) => {
                distance(self, start, d)?;
                // Of the angles between the line and the chamfer, the one
                // at the chamfer's start inside the corner, whichever way
                // the line runs.
                let off = |(measure, side): &(Measure, Side)| {
                    let measured = self.measure(measure, *side);
                    measured.map_or(f64::INFINITY, |value| (value - angle.value).abs())
                };
                let (measure, side) = [
                    Measure::Angle(lines[0], chamfer),
                    Measure::Angle(chamfer, lines[0]),
                ]
                .map(|measure| {
                    let side = self.side(&measure);
                    (measure, side)
                })
                .into_iter()
                .min_by(|p, q| off(p).total_cmp(&off(q)))
                .ok_or(EditError::NoCorner)?;
                let toward = (corner_at - start_at).normalize_or_zero()
                    + (end_at - start_at).normalize_or_zero();
                self.add_dimension(Dimension {
                    measure,
                    value: angle.clone(),
                    driving: true,
                    label: toward.normalize_or_zero() * d.value * LABEL_REACH,
                    side,
                })?;
            }
        }
        Ok(())
    }

    /// Deletes the fillets and chamfers whose corner is one no longer: its
    /// point no longer an end of both its lines, as trimming or extending
    /// one can leave it.
    pub(crate) fn drop_broken_corners(&mut self) {
        let broken: Vec<Id> = self
            .curves
            .iter()
            .filter(|entry| {
                entry.corner.is_some_and(|corner| {
                    let ends = |line| match self.curve(line).map(|entry| &entry.curve) {
                        Some(&Curve::Line { start, end }) => start == corner.at || end == corner.at,
                        _ => false,
                    };
                    !(ends(corner.a) && ends(corner.b))
                })
            })
            .map(|entry| entry.id)
            .collect();
        self.delete(&broken);
    }

    /// Whether the fillet or chamfer `entry` is on a corner: an arc (not
    /// `equal`) or a line, on two lines that aren't chamfers ending at its
    /// corner's point, made from none of their points.
    pub(crate) fn fits_corner(&self, entry: &CurveEntry) -> bool {
        let Some(corner) = entry.corner else {
            return true;
        };
        let shape = match entry.curve {
            Curve::Arc { .. } => !corner.equal,
            Curve::Line { .. } => true,
            Curve::Circle { .. } | Curve::Spline(_) => false,
        };
        let line = |id| {
            self.curve(id)
                .filter(|line| line.corner.is_none())
                .and_then(|line| match line.curve {
                    Curve::Line { start, end } => Some([start, end]),
                    _ => None,
                })
        };
        let (Some(a), Some(b)) = (line(corner.a), line(corner.b)) else {
            return false;
        };
        shape
            && a.contains(&corner.at)
            && b.contains(&corner.at)
            && !entry
                .curve
                .points()
                .any(|point| a.contains(&point) || b.contains(&point))
    }

    /// A fillet or chamfer on the same corner of the same two lines as
    /// one before it, if there is one, which [`Sketch::check`] refuses.
    pub(crate) fn repeated_corner(&self) -> Option<Id> {
        let mut seen = HashSet::new();
        self.curves.iter().find_map(|entry| {
            let corner = entry.corner?;
            let lines = (corner.a.min(corner.b), corner.a.max(corner.b));
            (!seen.insert((corner.at, lines))).then_some(entry.id)
        })
    }
}

#[cfg(test)]
mod tests;

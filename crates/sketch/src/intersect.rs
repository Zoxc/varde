//! Where curves meet: exact crossings of lines, circles and arcs, and
//! splines' by subdividing them to the tolerance, with a tolerance for
//! touching and overlapping, for profiles and the shape tools (trim,
//! extend).

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use glam::DVec2;

use crate::angle;
use crate::flatten::arc_segments;
use crate::spline::bezier::{self, Path};
use crate::{Curve, Id, Sketch, arc_sweep, crossing, foot};

/// Places within this much of the sketch's size of each other are one.
/// Ten times what the solver leaves (10⁻¹⁰), so what's constrained to
/// meet does.
const RELATIVE_TOLERANCE: f64 = 1e-9;

/// The least size tolerances are relative to, as the solver's.
const MIN_SIZE: f64 = 1e-3;

/// How near places are to be one, for curves of the shapes `geoms`:
/// [`RELATIVE_TOLERANCE`] of their size, their largest coordinate or a
/// circle's reach, at least [`MIN_SIZE`].
pub(crate) fn tolerance<'g>(geoms: impl IntoIterator<Item = &'g Geom>) -> f64 {
    let size = geoms
        .into_iter()
        .map(|geom| {
            let (min, max) = geom.bounds();
            min.abs().max(max.abs()).max_element()
        })
        .fold(MIN_SIZE, f64::max);
    size * RELATIVE_TOLERANCE
}

impl Sketch {
    /// The shapes of its curves, but those with no size.
    pub(crate) fn geoms(&self) -> impl Iterator<Item = Geom> + '_ {
        let curves = self.curves.iter();
        curves.filter_map(|entry| Geom::of(self, &entry.curve))
    }

    /// How near places on its curves are to be one, construction curves
    /// too: see [`tolerance`].
    pub(crate) fn curve_tolerance(&self) -> f64 {
        tolerance(&self.geoms().collect::<Vec<_>>())
    }

    /// The place on the curve `curve` nearest `at`: on a line or an arc
    /// between its ends, on a spline anywhere along it. `None` if it's no
    /// curve of the sketch's, or has no size.
    pub fn nearest_on(&self, curve: Id, at: DVec2) -> Option<DVec2> {
        let geom = Geom::of(self, &self.curve(curve)?.curve)?;
        let place = geom.at(geom.closest(at));
        place.is_finite().then_some(place)
    }
}

/// A curve's shape as intersecting sees it, with a parameter `u` running
/// along it.
///
/// Splines are Bézier segments ([`Path`]), meeting the rest by
/// subdivision to the tolerance in [`meet`]; the rest (pieces, faces,
/// nesting) takes any curve with a parameter, a place and a direction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Geom {
    /// A line from `start` to `end`, `u` from 0 at `start` to 1 at `end`.
    Segment { start: DVec2, end: DVec2 },
    /// A circle, or an arc of one: from the direction `begin` (radians)
    /// counter-clockwise by `sweep`, a whole turn for a circle, `u` the
    /// angle turned from `begin`, from 0 to `sweep`.
    Round {
        center: DVec2,
        radius: f64,
        begin: f64,
        sweep: f64,
    },
    /// A spline, `u` from 0 to 1 along it, see [`Path`]. Shared, as it
    /// takes solving to make.
    Spline(Arc<Path>),
}

impl Geom {
    /// The shape of `curve` in `sketch`: an arc's radius is its start's
    /// distance from its centre, and it ends in its end's direction. `None`
    /// if it names a point the sketch doesn't have, isn't finite, or has no
    /// size (a line from a point to another at the same place, an arc
    /// starting at its centre, a spline all at one place).
    pub(crate) fn of(sketch: &Sketch, curve: &Curve) -> Option<Geom> {
        let at = |id| sketch.point(id).map(|point| point.at);
        let geom = match curve {
            &Curve::Line { start, end } => Geom::Segment {
                start: at(start)?,
                end: at(end)?,
            },
            &Curve::Circle { center, radius } => Geom::Round {
                center: at(center)?,
                radius,
                begin: 0.0,
                sweep: TAU,
            },
            &Curve::Arc { center, start, end } => {
                let center = at(center)?;
                let (from, to) = (at(start)? - center, at(end)? - center);
                Geom::Round {
                    center,
                    radius: from.length(),
                    begin: angle::to_angle(from),
                    sweep: arc_sweep(from, to),
                }
            }
            Curve::Spline(spline) => {
                let path = sketch.spline_shape(spline)?.path();
                return path.is_sound().then(|| Geom::Spline(Arc::new(path)));
            }
        };
        let finite = match geom {
            Geom::Segment { start, end } => start.is_finite() && end.is_finite() && start != end,
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                center.is_finite()
                    && radius.is_finite()
                    && radius > 0.0
                    && (begin + sweep).is_finite()
            }
            Geom::Spline(_) => true,
        };
        finite.then_some(geom)
    }

    /// Where the parameter ends: 1 for a line, the sweep for a circle or
    /// an arc.
    pub(crate) fn last(&self) -> f64 {
        match *self {
            Geom::Segment { .. } => 1.0,
            Geom::Round { sweep, .. } => sweep,
            Geom::Spline(ref path) => path.last(),
        }
    }

    /// Whether it has ends: a line, an arc or an open spline, not a
    /// circle or a closed spline.
    pub(crate) fn has_ends(&self) -> bool {
        match *self {
            Geom::Segment { .. } => true,
            Geom::Round { sweep, .. } => sweep < TAU,
            Geom::Spline(ref path) => !path.closed(),
        }
    }

    /// Whether the piece from `from` to `to` (increasing), whose ends are
    /// one place, goes round rather than being a sliver: a circle's more
    /// than half of it, a closed spline's, or a loop of a spline crossing
    /// itself, with its middle further than `tolerance` from its ends.
    pub(crate) fn goes_round(&self, from: f64, to: f64, tolerance: f64) -> bool {
        match self {
            Geom::Segment { .. } => false,
            Geom::Round { .. } => to - from > PI,
            Geom::Spline(path) => path.goes_round(from, to, tolerance),
        }
    }

    /// Where it's always cut: its ends, or a circle's start, so that it
    /// has a vertex.
    pub(crate) fn cuts(&self) -> impl Iterator<Item = f64> + use<> {
        let last = self.has_ends().then_some(self.last());
        std::iter::once(0.0).chain(last)
    }

    /// The place at `u`.
    pub(crate) fn at(&self, u: f64) -> DVec2 {
        match *self {
            // Exactly the ends at 0 and 1.
            Geom::Segment { start, end } => start * (1.0 - u) + end * u,
            Geom::Round {
                center,
                radius,
                begin,
                ..
            } => center + radius * angle::from_angle(begin + u),
            Geom::Spline(ref path) => path.at(u),
        }
    }

    /// The unit direction at `u` going towards increasing `u` if
    /// `forward`, else back, and the curvature that way: positive turning
    /// left, one over the radius.
    pub(crate) fn heading(&self, u: f64, forward: bool) -> (DVec2, f64) {
        let sign = if forward { 1.0 } else { -1.0 };
        match *self {
            Geom::Segment { start, end } => ((end - start).normalize() * sign, 0.0),
            Geom::Round { radius, begin, .. } => {
                (angle::from_angle(begin + u).perp() * sign, sign / radius)
            }
            Geom::Spline(ref path) => path.heading(u, forward),
        }
    }

    /// The length from `u0` to `u1`, either way.
    pub(crate) fn length(&self, u0: f64, u1: f64) -> f64 {
        let scale = match *self {
            Geom::Segment { start, end } => start.distance(end),
            Geom::Round { radius, .. } => radius,
            Geom::Spline(ref path) => return path.length(u0, u1),
        };
        (u1 - u0).abs() * scale
    }

    /// The box it lies in, a circle's whole for an arc, a spline's
    /// control points'.
    pub(crate) fn bounds(&self) -> (DVec2, DVec2) {
        match *self {
            Geom::Segment { start, end } => (start.min(end), start.max(end)),
            Geom::Round { center, radius, .. } => (center - radius, center + radius),
            Geom::Spline(ref path) => path.bounds(),
        }
    }

    /// The box the curve from `u0` to `u1` (either way) lies in, exactly:
    /// its ends', and an arc's furthest places along x and y it passes.
    pub(crate) fn span_bounds(&self, u0: f64, u1: f64) -> (DVec2, DVec2) {
        if let Geom::Spline(path) = self {
            return path.span_bounds(u0, u1);
        }
        let (a, b) = (self.at(u0), self.at(u1));
        let (mut min, mut max) = (a.min(b), a.max(b));
        if let Geom::Round {
            center,
            radius,
            begin,
            ..
        } = *self
        {
            let (low, high) = (u0.min(u1), u0.max(u1));
            for quarter in 0..4 {
                let angle = f64::from(quarter) * FRAC_PI_2;
                let u = (angle - begin).rem_euclid(TAU);
                if (low..=high).contains(&u) || (low..=high).contains(&(u + TAU)) {
                    let at = center + radius * angle::from_angle(angle);
                    (min, max) = (min.min(at), max.max(at));
                }
            }
        }
        (min, max)
    }

    /// The parameter of `point`, taken to be on the endless line or the
    /// whole circle, if it's on the curve: within `tolerance` of its ends,
    /// which it's then put at. On a spline, of the place nearest it, if
    /// that's within `tolerance`.
    pub(crate) fn param(&self, point: DVec2, tolerance: f64) -> Option<f64> {
        match *self {
            Geom::Segment { start, end } => {
                let along = end - start;
                let slack = tolerance / along.length();
                let u = along.dot(point - start) / along.length_squared();
                (-slack..=1.0 + slack)
                    .contains(&u)
                    .then(|| u.clamp(0.0, 1.0))
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                let u = (angle::to_angle(point - center) - begin).rem_euclid(TAU);
                if sweep >= TAU || u <= sweep {
                    return Some(u);
                }
                let slack = tolerance / radius;
                if u <= sweep + slack {
                    Some(sweep)
                } else if u >= TAU - slack {
                    Some(0.0)
                } else {
                    None
                }
            }
            Geom::Spline(ref path) => path.param_of(point, tolerance),
        }
    }

    /// The parameter of the place on the curve nearest `point`: on a line
    /// or an arc, an end's where the place nearest on the endless line or
    /// the whole circle is past it; on a spline, found numerically (see
    /// [`Path::closest`]).
    pub(crate) fn closest(&self, point: DVec2) -> f64 {
        match *self {
            Geom::Segment { start, end } => {
                let along = end - start;
                let u = along.dot(point - start) / along.length_squared();
                if u.is_finite() {
                    u.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
            Geom::Round {
                center,
                begin,
                sweep,
                ..
            } => {
                let u = (angle::to_angle(point - center) - begin).rem_euclid(TAU);
                // Past an arc's end, whichever end is nearer round.
                if u <= sweep || sweep >= TAU {
                    u
                } else if u - sweep <= TAU - u {
                    sweep
                } else {
                    0.0
                }
            }
            Geom::Spline(ref path) => path.closest(point),
        }
    }

    /// The curve from `u0` to `u1` (either way) as a polyline through
    /// both ends: a line's two, a circle's or an arc's its share of
    /// [`CIRCLE_SEGMENTS`](crate::CIRCLE_SEGMENTS), a spline's as
    /// [`Sketch::flatten`] draws it.
    pub(crate) fn polyline(&self, u0: f64, u1: f64) -> Vec<DVec2> {
        match self {
            Geom::Spline(path) => path.polyline(u0, u1),
            Geom::Segment { .. } => vec![self.at(u0), self.at(u1)],
            Geom::Round { .. } => {
                let segments = arc_segments((u1 - u0).abs());
                (0..=segments)
                    .map(|i| self.at(u0 + (u1 - u0) * i as f64 / segments as f64))
                    .collect()
            }
        }
    }

    /// The place nearest `point` on the endless line or the whole circle,
    /// or on a spline.
    fn nearest(&self, point: DVec2) -> DVec2 {
        match *self {
            Geom::Segment { start, end } => foot(point, start, end),
            Geom::Round { center, radius, .. } => {
                let out = (point - center).try_normalize().unwrap_or(DVec2::X);
                center + out * radius
            }
            Geom::Spline(ref path) => path.at(path.closest(point)),
        }
    }

    /// How far round `point` the curve from `u0` to `u1` (either way)
    /// winds, in radians, counter-clockwise positive: summed over a closed
    /// loop not through `point`, a whole number of turns.
    pub(crate) fn winding(&self, u0: f64, u1: f64, point: DVec2) -> f64 {
        let chord = |a: DVec2, b: DVec2| {
            let (a, b) = (a - point, b - point);
            angle::atan2(a.perp_dot(b), a.dot(b))
        };
        match *self {
            Geom::Segment { .. } => chord(self.at(u0), self.at(u1)),
            Geom::Spline(ref path) => path.winding(u0, u1, point),
            Geom::Round { center, radius, .. } => {
                // In parts of at most a quarter turn, each its chord's
                // angle and a whole turn more if `point` is between the
                // chord and the arc, which is on the chord's right going
                // counter-clockwise: the loop of the arc and its chord
                // back winds once round what's between.
                let (low, high) = (u0.min(u1), u0.max(u1));
                let parts = ((high - low) / FRAC_PI_2).ceil().max(1.0);
                // A whole turn at most, so a few parts.
                let parts = parts.min(8.0) as usize;
                let inside = point.distance_squared(center) < radius * radius;
                let mut total = 0.0;
                for i in 0..parts {
                    let a = self.at(low + (high - low) * i as f64 / parts as f64);
                    let b = self.at(low + (high - low) * (i + 1) as f64 / parts as f64);
                    total += chord(a, b);
                    if inside && (b - a).perp_dot(point - a) < 0.0 {
                        total += TAU;
                    }
                }
                if u1 < u0 { -total } else { total }
            }
        }
    }

    /// The area between the curve from `u0` to `u1` (either way) and the
    /// chord joining its ends, positive left of travel for an arc bulging
    /// left: what an arc adds to a loop's area over its chord.
    pub(crate) fn bulge(&self, u0: f64, u1: f64) -> f64 {
        match *self {
            Geom::Segment { .. } => 0.0,
            Geom::Round { radius, .. } => {
                let sweep = u1 - u0;
                radius * radius / 2.0 * (sweep - angle::sin(sweep))
            }
            Geom::Spline(ref path) => path.bulge(u0, u1),
        }
    }
}

/// Where `a` and `b` meet, as pairs of their parameters, pushed onto
/// `out`: where they cross, one place where they touch (within
/// `tolerance`), and where an end of either (or a circle's start, see
/// [`Geom::cuts`]) is within `tolerance` of the other, which is also where
/// lines on one line, or arcs on one circle, start and stop overlapping,
/// so that both are cut alike where they overlap. Places may repeat; the
/// caller merges them. A spline meets the rest by subdivision, bounded
/// ([`bezier::MAX_MEET_STEPS`]): the steps taken are returned, one for
/// lines, circles and arcs, for a caller that counts its work.
pub(crate) fn meet(a: &Geom, b: &Geom, tolerance: f64, out: &mut Vec<(f64, f64)>) -> usize {
    let steps = if let (Geom::Spline(_), _) | (_, Geom::Spline(_)) = (a, b) {
        bezier::crossings(a, b, tolerance, out)
    } else {
        for point in crossings(a, b, tolerance).into_iter().flatten() {
            if let (Some(ua), Some(ub)) = (a.param(point, tolerance), b.param(point, tolerance)) {
                out.push((ua, ub));
            }
        }
        1
    };
    for (from, to, swap) in [(a, b, false), (b, a, true)] {
        for u in from.cuts() {
            let end = from.at(u);
            let near = to.nearest(end);
            if near.distance(end) > tolerance {
                continue;
            }
            if let Some(v) = to.param(near, tolerance) {
                out.push(if swap { (v, u) } else { (u, v) });
            }
        }
    }
    steps
}

/// Where the endless lines and whole circles of `a` and `b` cross, one
/// place where they touch within `tolerance`, none where they're on one
/// line or one circle (their ends tell where those overlap). Crossing
/// barely, into each other by less than the tolerance, they still cross
/// twice where those places are further apart than it: made one, a third
/// curve crossing between them would cross the two in an order that
/// can't be drawn. None for a spline, which [`meet`] meets otherwise.
fn crossings(a: &Geom, b: &Geom, tolerance: f64) -> [Option<DVec2>; 2] {
    match (a, b) {
        (&Geom::Segment { start: p, end: e }, &Geom::Segment { start: q, end: f }) => {
            let (r, s) = (e - p, f - q);
            [crossing(p, r, q, s).map(|(t, _)| p + r * t), None]
        }
        (&Geom::Segment { start, end }, &Geom::Round { center, radius, .. })
        | (&Geom::Round { center, radius, .. }, &Geom::Segment { start, end }) => {
            let near = foot(center, start, end);
            let off = near.distance(center);
            if off > radius + tolerance {
                return [None, None];
            }
            let half = (radius * radius - off * off).max(0.0).sqrt();
            if 2.0 * half <= tolerance {
                return [Some(near), None];
            }
            let along = (end - start).normalize();
            [Some(near - along * half), Some(near + along * half)]
        }
        (
            &Geom::Round {
                center: c1,
                radius: r1,
                ..
            },
            &Geom::Round {
                center: c2,
                radius: r2,
                ..
            },
        ) => {
            let apart = c1.distance(c2);
            if apart <= tolerance
                || apart > r1 + r2 + tolerance
                || apart < (r1 - r2).abs() - tolerance
            {
                return [None, None];
            }
            let toward = (c2 - c1) / apart;
            let along = (apart * apart + r1 * r1 - r2 * r2) / (2.0 * apart);
            let half = (r1 * r1 - along * along).max(0.0).sqrt();
            if 2.0 * half > tolerance {
                let middle = c1 + toward * along;
                return [
                    Some(middle - toward.perp() * half),
                    Some(middle + toward.perp() * half),
                ];
            }
            if (apart - (r1 + r2)).abs() <= (apart - (r1 - r2).abs()).abs() {
                // Touching from outside: halfway between each's nearest.
                [Some((c1 + toward * r1 + c2 - toward * r2) / 2.0), None]
            } else {
                // Touching inside: both on the side away from the larger's
                // centre.
                let out = if r1 >= r2 { toward } else { -toward };
                [Some((c1 + out * r1 + c2 + out * r2) / 2.0), None]
            }
        }
        (Geom::Spline(_), _) | (_, Geom::Spline(_)) => [None, None],
    }
}

#[cfg(test)]
mod tests;

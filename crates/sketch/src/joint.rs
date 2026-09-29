//! Where a tangent or a smooth join with a spline holds, a spline's end
//! ([`Joint`]), and each curve's way along there and how it curves: for
//! the solver's equations, and for making one on the side the geometry
//! is on ([`Sketch::joint`]).

use glam::DVec2;

use crate::{Constraint, Curve, Id, Kind, Side, Sketch, SplineKind};

/// Where a [`Constraint::Tangent`] or [`Constraint::Smooth`] with a
/// spline holds: its point `at`, an end of `spline` (the first of the
/// two, where both are splines), and of `other` too where it's `shared`.
/// Shared, the two touch there, and the constraint only has them run
/// along each other (and curve alike); else `at` is held on `other` as
/// well: on a line's endless line, or a circle's or an arc's circle. Two
/// splines always share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Joint {
    pub spline: Id,
    pub other: Id,
    pub shared: bool,
}

impl Joint {
    /// The joint of the curves `a` and `b` of `sketch` at `at`: `None`
    /// unless one is an open spline ending at `at` and the other a line
    /// (or an axis), a circle, an arc, or a spline ending at `at` too, not
    /// made from `at` otherwise (an arc's centre).
    pub fn of(sketch: &Sketch, a: Id, b: Id, at: Id) -> Option<Joint> {
        let curve = |id| sketch.curve(id).map(|entry| &entry.curve);
        let ends_at = |id| {
            curve(id)
                .and_then(Curve::ends)
                .is_some_and(|ends| ends.contains(&at))
        };
        let [spline, other] = spline_first(sketch, a, b)?;
        let shared = ends_at(other);
        let made_from = curve(other).is_some_and(|curve| curve.points().any(|id| id == at));
        let fits = match sketch.kind(other)? {
            Kind::Spline => shared,
            Kind::Line | Kind::Circle | Kind::Arc => shared || !made_from,
            _ => false,
        };
        (ends_at(spline) && fits).then_some(Joint {
            spline,
            other,
            shared,
        })
    }

    /// Whether the two can curve alike at `at`, for a smooth join: a
    /// spline through fit points has a handle there, its end being
    /// straight otherwise (see [`Interpolation`](crate::Interpolation)),
    /// whatever moves.
    pub fn curves(&self, sketch: &Sketch, at: Id) -> bool {
        [self.spline, self.other].into_iter().all(|id| {
            sketch
                .spline(id)
                .is_none_or(|spline| spline.kind == SplineKind::Control || spline.has_handle(at))
        })
    }
}

/// `a` and `b` with a spline first, `a` if both are: `None` if neither
/// is.
fn spline_first(sketch: &Sketch, a: Id, b: Id) -> Option<[Id; 2]> {
    let is_spline = |id| sketch.kind(id) == Some(Kind::Spline);
    match (is_spline(a), is_spline(b)) {
        (true, _) => Some([a, b]),
        (false, true) => Some([b, a]),
        (false, false) => None,
    }
}

impl Sketch {
    /// The way the curve `curve` runs at its point or end `at` (see
    /// [`Constraint::Tangent`]), as a vector, and how it curves there, one
    /// over its radius, positive turning left going that way: a line from
    /// its start to its end, straight; a circle or an arc
    /// counter-clockwise, across `at`'s direction from its centre; a
    /// spline as its parameter runs, at its end `at`. `None` for a spline
    /// `at` isn't an end of, or a point missing.
    pub(crate) fn heading(&self, curve: Id, at: Id) -> Option<(DVec2, f64)> {
        if let Some((start, end)) = self.line(curve) {
            return Some((end - start, 0.0));
        }
        if let Some((center, radius)) = self.round(curve) {
            return Some(((self.point(at)?.at - center).perp(), 1.0 / radius));
        }
        let spline = self.spline(curve)?;
        let t = spline.end_param(at)?;
        let shape = self.spline_shape(spline)?;
        Some((shape.eval(t)[1], shape.curvature(t)))
    }

    /// The end at which a tangent or a smooth join between the curves `a`
    /// and `b` is made ([`Joint`]), and the side they run on there (see
    /// [`Constraint::Tangent`]): the spline's end the other also ends at,
    /// if there's one, else the spline's end nearer the other. `None`
    /// unless one is an open spline, and for two splines, unless they
    /// share an end.
    pub fn joint(&self, a: Id, b: Id) -> Option<(Id, Side)> {
        let [spline, other] = spline_first(self, a, b)?;
        let ends = self.curve(spline)?.curve.ends()?;
        let joint = |end| Joint::of(self, a, b, end);
        let at = match ends
            .into_iter()
            .find(|&end| joint(end).is_some_and(|j| j.shared))
        {
            Some(at) => at,
            None => {
                let off = |end: Id| self.off(other, end).unwrap_or(f64::INFINITY);
                let [first, last] = ends;
                if off(last) < off(first) { last } else { first }
            }
        };
        joint(at)?;
        let ((u, _), (w, _)) = (self.heading(a, at)?, self.heading(b, at)?);
        let side = Side::of(u.dot(w));
        (u.is_finite() && w.is_finite()).then_some((at, side))
    }

    /// A [`Constraint::Smooth`] between the curves `a` and `b` at the end
    /// [`Sketch::joint`] picks, on the side the geometry is on. `None`
    /// where there's no joint, or they can't curve alike there
    /// ([`Joint::curves`]).
    pub fn smooth(&self, a: Id, b: Id) -> Option<Constraint> {
        let (at, side) = self.joint(a, b)?;
        Joint::of(self, a, b, at)?
            .curves(self, at)
            .then_some(Constraint::Smooth { a, b, at, side })
    }

    /// How far the point `at` is from the line (endless) or the circle
    /// `curve` is on. `None` for anything else.
    fn off(&self, curve: Id, at: Id) -> Option<f64> {
        let at = self.point(at)?.at;
        if let Some((start, end)) = self.line(curve) {
            return Some((end - start).normalize().perp_dot(at - start).abs());
        }
        let (center, radius) = self.round(curve)?;
        Some((at.distance(center) - radius).abs())
    }
}

#[cfg(test)]
mod tests;

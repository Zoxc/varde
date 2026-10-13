//! Offset pairs: what [`Measure::Offset`](crate::Measure::Offset)(crate::Measure::Offset)
//! measures and [`Constraint::EqualOffset`](crate::Constraint::EqualOffset)
//! ties, two items one a copy of the other some way off.

use glam::DVec2;

use crate::angle;
use crate::intersect::Geom;
use crate::{Curve, Id, Kind, Sketch};

/// What an offset pair is, see [`Sketch::offset_pair`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetPair {
    /// Two lines: the second's midpoint from the line through the first.
    Lines,
    /// Two circles or arcs: the second's radius less the first's.
    Rounds,
    /// A point and a circle or an arc about it (a round join): its radius.
    Join,
    /// A spline and a point: the point's distance from the place on the
    /// spline nearest it, left of the way the spline runs positive. What
    /// holds each fit point of a spline's copy.
    Spline,
}

impl Sketch {
    /// What the pair `[a, b]` is as an offset: `b` a copy of `a` some way
    /// off, which [`Measure::Offset`](crate::Measure::Offset) measures
    /// and [`Constraint::EqualOffset`](crate::Constraint::EqualOffset)
    /// ties. Two lines (the first may be an axis), two circles or arcs, a
    /// point and a circle or an arc about it, or a spline and a point, not
    /// one of its own. `None` for anything else, or an item twice.
    pub fn offset_pair(&self, [a, b]: [Id; 2]) -> Option<OffsetPair> {
        if a == b {
            return None;
        }
        let round = |id| matches!(self.kind(id), Some(Kind::Circle | Kind::Arc));
        let line = |id| self.kind(id) == Some(Kind::Line);
        let center = |id| self.curve(id)?.curve.center();
        if line(a) && line(b) && !b.is_builtin() {
            Some(OffsetPair::Lines)
        } else if round(a) && round(b) {
            Some(OffsetPair::Rounds)
        } else if self.kind(a) == Some(Kind::Point) && center(b) == Some(a) {
            Some(OffsetPair::Join)
        } else if self.kind(b) == Some(Kind::Point)
            && self
                .spline(a)
                .is_some_and(|spline| !spline.all_points().any(|id| id == b))
        {
            Some(OffsetPair::Spline)
        } else {
            None
        }
    }

    /// How far the second of the offset pair `pair` lies from the first,
    /// signed (see [`Measure::Offset`](crate::Measure::Offset)). `None` if
    /// it's no offset pair, or a line has no length.
    pub(crate) fn offset_of(&self, pair: [Id; 2]) -> Option<f64> {
        let [a, b] = pair;
        let offset = match self.offset_pair(pair)? {
            OffsetPair::Lines => {
                let ((start, end), (from, to)) = (self.line(a)?, self.line(b)?);
                crate::signed_distance(from.midpoint(to), start, end)
            }
            OffsetPair::Rounds => self.round(b)?.1 - self.round(a)?.1,
            OffsetPair::Join => self.round(b)?.1,
            OffsetPair::Spline => {
                let (distance, side) = self.spline_side(a, self.point(b)?.at)?;
                distance * side.sign()
            }
        };
        offset.is_finite().then_some(offset)
    }

    /// Where a dimension of the offset pair `pair` is anchored: halfway
    /// between the second line's midpoint and its foot on the first, or
    /// halfway across from one circle or arc to the other (or from a round
    /// join's corner out to it) through the second's middle, a circle's
    /// upper right, or halfway from a point to the place on a spline
    /// nearest it. `None` if it's no offset pair.
    pub(crate) fn offset_anchor(&self, pair: [Id; 2]) -> Option<DVec2> {
        let [a, b] = pair;
        match self.offset_pair(pair)? {
            OffsetPair::Spline => {
                let point = self.point(b)?.at;
                Some(point.midpoint(self.nearest_on(a, point)?))
            }
            OffsetPair::Lines => {
                let ((start, end), (from, to)) = (self.line(a)?, self.line(b)?);
                let middle = from.midpoint(to);
                Some(middle.midpoint(crate::foot(middle, start, end)))
            }
            kind => {
                let (center, radius) = self.round(b)?;
                let inner = match kind {
                    OffsetPair::Rounds => self.round(a)?.1,
                    _ => 0.0,
                };
                let toward = self.round_middle(b)?;
                Some(center + toward * (radius + inner) / 2.0)
            }
        }
    }

    /// The way from its centre to the middle of the arc `id`, or to a
    /// circle's upper right.
    fn round_middle(&self, id: Id) -> Option<DVec2> {
        let curve = &self.curve(id)?.curve;
        match (curve, Geom::of(self, curve)?) {
            (Curve::Arc { .. }, Geom::Round { begin, sweep, .. }) => {
                Some(angle::from_angle(begin + sweep / 2.0))
            }
            (Curve::Circle { .. }, _) => Some(DVec2::splat(std::f64::consts::FRAC_1_SQRT_2)),
            _ => None,
        }
    }
}

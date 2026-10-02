use glam::Vec3;

use crate::render_mesh::{split, splits, within};
use crate::{Aabb, MAX_COORD};

/// Curves flattened for drawing: polylines in world space, such as the
/// finished sketches drawn with the model.
///
/// Like a [`RenderMesh`](crate::RenderMesh), always drawable: every
/// polyline has at least two points, there are at most
/// [`RenderLines::MAX_POINTS`] points and every one is within
/// [`RenderLines::MAX_POSITION`], so the renderer's bounds and depth range
/// stay finite. The fields are private so that holds; lines built
/// elsewhere come in through [`RenderLines::from_parts`], which checks
/// them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderLines {
    points: Vec<[f32; 3]>,
    /// One past each polyline's last point, in increasing order, the last
    /// one the number of points.
    ends: Vec<u32>,
}

impl RenderLines {
    /// The most points lines may have, about 8 million: well within `u32`
    /// ends, and the renderer asserts that as many segments fit its GPU
    /// buffers. A sketch of every curve a sketch may hold, all circles,
    /// flattens to under a million.
    pub const MAX_POINTS: usize = 1 << 23;
    /// The most polylines lines may have: each takes at least two points.
    pub const MAX_POLYLINES: usize = Self::MAX_POINTS / 2;
    /// The largest coordinate a point may have. A sketch point is within
    /// [`MAX_COORD`] of zero, so an arc's radius, the distance between two
    /// of them, is under three times that, and the arc's points on an
    /// origin plane are within four times it.
    pub const MAX_POSITION: f32 = 4.0 * MAX_COORD;

    /// Lines of these parts, if they make them; see [`RenderLines`] for
    /// what is checked.
    pub fn from_parts(points: Vec<[f32; 3]>, ends: Vec<u32>) -> Result<RenderLines, LinesError> {
        if points.len() > Self::MAX_POINTS || ends.len() > Self::MAX_POLYLINES {
            return Err(LinesError::TooLarge);
        }
        if !splits(&ends, points.len(), |len| len >= 2) {
            return Err(LinesError::Ends);
        }
        if !within(&points, Self::MAX_POSITION) {
            return Err(LinesError::Values);
        }
        Ok(RenderLines { points, ends })
    }

    /// Every polyline's points, one after the other.
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }

    /// One past each polyline's last point in [`points`](Self::points).
    pub fn ends(&self) -> &[u32] {
        &self.ends
    }

    /// Each polyline's points.
    pub fn polylines(&self) -> impl Iterator<Item = &[[f32; 3]]> {
        split(&self.points, &self.ends)
    }

    /// How many segments the polylines have together: each has one fewer
    /// than its points.
    pub fn segment_count(&self) -> usize {
        self.points.len() - self.ends.len()
    }

    /// Appends a polyline through `points`.
    ///
    /// Fails, leaving `self` as it was, with [`LinesError::Ends`] if it has
    /// fewer than two points, [`LinesError::TooLarge`] if the lines would
    /// have more than [`RenderLines::MAX_POINTS`] points, and
    /// [`LinesError::Values`] if a point isn't within
    /// [`RenderLines::MAX_POSITION`]. Documents are flattened by pushing
    /// every curve, and a file can hold any number of sketches.
    pub fn push(&mut self, points: impl IntoIterator<Item = Vec3>) -> Result<(), LinesError> {
        let start = self.points.len();
        let result = self.extend(points);
        if result.is_err() {
            self.points.truncate(start);
        }
        result
    }

    /// [`push`](Self::push), but leaving what it pushed of a polyline it
    /// refuses.
    fn extend(&mut self, points: impl IntoIterator<Item = Vec3>) -> Result<(), LinesError> {
        let start = self.points.len();
        for point in points {
            if self.points.len() >= Self::MAX_POINTS {
                return Err(LinesError::TooLarge);
            }
            let point = point.to_array();
            if !within(&[point], Self::MAX_POSITION) {
                return Err(LinesError::Values);
            }
            self.points.push(point);
        }
        if self.points.len() - start < 2 {
            return Err(LinesError::Ends);
        }
        // Within `MAX_POINTS`, checked above.
        let end = u32::try_from(self.points.len()).map_err(|_| LinesError::TooLarge)?;
        self.ends.push(end);
        Ok(())
    }

    /// Axis-aligned bounds, or `None` if there are no lines.
    pub fn bounds(&self) -> Option<Aabb> {
        Aabb::around(&self.points)
    }
}

/// One of the parts [`RenderLines`] are made of, as
/// [`RenderLines::from_parts`] takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinesPart {
    Points,
    Ends,
}

impl std::fmt::Display for LinesPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LinesPart::Points => "line points",
            LinesPart::Ends => "line ends",
        })
    }
}

/// Why parts don't make [`RenderLines`], or a polyline can't be pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinesError {
    /// More points than [`RenderLines::MAX_POINTS`], or polylines than
    /// [`RenderLines::MAX_POLYLINES`].
    TooLarge,
    /// The ends don't split the points into polylines of two points or
    /// more.
    Ends,
    /// A point isn't within [`RenderLines::MAX_POSITION`].
    Values,
}

impl std::fmt::Display for LinesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LinesError::TooLarge => "the lines have more points than the kernel allows",
            LinesError::Ends => "the lines aren't polylines of two points or more",
            LinesError::Values => "the lines hold points out of range",
        })
    }
}

impl std::error::Error for LinesError {}

#[cfg(test)]
mod tests;

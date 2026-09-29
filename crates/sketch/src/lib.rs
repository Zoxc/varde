//! 2D sketches: points, lines and arcs on a plane, plus the geometric
//! constraints between them (coincident, parallel, distance, ...).
//!
//! Only the data model exists so far; the solver is still to be written.

use glam::DVec2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PointId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Entity {
    Line {
        start: PointId,
        end: PointId,
    },
    Arc {
        center: PointId,
        start: PointId,
        end: PointId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Coincident(PointId, PointId),
    Horizontal(PointId, PointId),
    Vertical(PointId, PointId),
    Distance(PointId, PointId, f64),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<DVec2>,
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
}

impl Sketch {
    /// Every point id the entities and constraints refer to.
    pub fn point_ids(&self) -> impl Iterator<Item = PointId> + '_ {
        let entities = self.entities.iter().flat_map(|e| match *e {
            Entity::Line { start, end } => [Some(start), Some(end), None],
            Entity::Arc { center, start, end } => [Some(center), Some(start), Some(end)],
        });
        let constraints = self.constraints.iter().flat_map(|c| match *c {
            Constraint::Coincident(a, b)
            | Constraint::Horizontal(a, b)
            | Constraint::Vertical(a, b)
            | Constraint::Distance(a, b, _) => [a, b],
        });
        entities.flatten().chain(constraints)
    }

    /// Checks what a file could get wrong: every coordinate is within
    /// `max` of zero, every distance is above zero and at most `max` (a
    /// zero one would be a [`Constraint::Coincident`]), and every point id
    /// names one of the points.
    pub fn check(&self, max: f64) -> Result<(), SketchError> {
        if let Some(value) = self
            .points
            .iter()
            .flat_map(|p| p.to_array())
            .find(|v| !(-max..=max).contains(v))
        {
            return Err(SketchError::Coordinate { value, max });
        }
        if let Some(distance) = self.constraints.iter().find_map(|c| match *c {
            Constraint::Distance(_, _, distance) if !(distance > 0.0 && distance <= max) => {
                Some(distance)
            }
            Constraint::Coincident(..)
            | Constraint::Horizontal(..)
            | Constraint::Vertical(..)
            | Constraint::Distance(..) => None,
        }) {
            return Err(SketchError::Distance { distance, max });
        }
        if let Some(id) = self
            .point_ids()
            .find(|id| self.points.get(id.0 as usize).is_none())
        {
            return Err(SketchError::UnknownPoint {
                id,
                points: self.points.len(),
            });
        }
        Ok(())
    }
}

/// Why a [`Sketch`] fails [`Sketch::check`], against the limit `max` it
/// was checked with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SketchError {
    /// A point coordinate farther than `max` from zero, or not a number.
    Coordinate { value: f64, max: f64 },
    /// A distance constraint not above zero, or past `max`.
    Distance { distance: f64, max: f64 },
    /// A point id past the sketch's `points` points.
    UnknownPoint { id: PointId, points: usize },
}

impl std::fmt::Display for SketchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            SketchError::Coordinate { value, max } => write!(
                f,
                "the sketch has a coordinate of {value}, outside the limit of {max}"
            ),
            SketchError::Distance { distance, max } => write!(
                f,
                "the sketch has a distance of {distance}, not above zero and within {max}"
            ),
            SketchError::UnknownPoint { id, points } => write!(
                f,
                "the sketch refers to point {}, but has {points} points",
                id.0
            ),
        }
    }
}

impl std::error::Error for SketchError {}

#[cfg(test)]
mod tests;

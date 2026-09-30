//! Naming a region so it's found again after its sketch changes:
//! [`RegionRef`].

use std::fmt;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use super::{Piece, Profiles, Region};
use crate::{Id, MAX_CURVES};

/// The most curve ids a [`RegionRef`] may hold, its outer loop's and its
/// holes' together: every curve of the largest sketch, twice (a curve can
/// bound the outer loop and a hole, or several holes).
pub const MAX_REGION_CURVES: usize = 2 * MAX_CURVES;

/// The horizontal lines across a region tried for a point inside it
/// ([`Profiles::reference`]).
const SCANLINES: usize = 16;

/// A region of a sketch as a feature names it, to be found again once the
/// sketch has changed ([`Profiles::resolve`]): the curves its outer loop
/// is made from and those each hole is made from, each list sorted with no
/// repeats and the holes' lists sorted, and a point strictly inside it
/// when it was named.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionRef {
    pub curves: Vec<Id>,
    pub holes: Vec<Vec<Id>>,
    pub inside: DVec2,
}

/// What's wrong with a [`RegionRef`] read from a file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionRefError {
    /// More than [`MAX_REGION_CURVES`] ids.
    TooManyCurves(usize),
    /// A list of ids is empty, or not sorted without repeats, or the
    /// holes' lists aren't sorted.
    Unsorted,
    /// The point inside isn't finite, or is further from zero than the
    /// limit.
    Inside(DVec2),
}

impl fmt::Display for RegionRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegionRefError::TooManyCurves(count) => write!(
                f,
                "a region names {count} curves, more than {MAX_REGION_CURVES}"
            ),
            RegionRefError::Unsorted => {
                f.write_str("a region's curves are missing, repeated or out of order")
            }
            RegionRefError::Inside(at) => write!(f, "a region's point {at} is out of bounds"),
        }
    }
}

impl std::error::Error for RegionRefError {}

impl RegionRef {
    /// Checks what a file could get wrong: at most [`MAX_REGION_CURVES`]
    /// ids in all, each list not empty and sorted without repeats, the
    /// holes' lists sorted, and `inside` finite and within `max` of zero.
    pub fn check(&self, max: f64) -> Result<(), RegionRefError> {
        let count = self.holes.iter().fold(self.curves.len(), |count, hole| {
            count.saturating_add(hole.len())
        });
        if count > MAX_REGION_CURVES {
            return Err(RegionRefError::TooManyCurves(count));
        }
        let sorted = |ids: &[Id]| !ids.is_empty() && ids.windows(2).all(|pair| pair[0] < pair[1]);
        if !sorted(&self.curves)
            || !self.holes.iter().all(|hole| sorted(hole))
            || !self.holes.windows(2).all(|pair| pair[0] <= pair[1])
        {
            return Err(RegionRefError::Unsorted);
        }
        if !(self.inside.is_finite() && self.inside.abs().max_element() <= max) {
            return Err(RegionRefError::Inside(self.inside));
        }
        Ok(())
    }
}

impl Region {
    /// The curves of the outer loop, and of each hole, each sorted with no
    /// repeats, and the holes' sorted, as a [`RegionRef`] holds them.
    fn curve_ids(&self) -> (Vec<Id>, Vec<Vec<Id>>) {
        let ids = |pieces: &[Piece]| {
            let mut ids: Vec<Id> = pieces.iter().map(|piece| piece.curve).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let mut holes: Vec<Vec<Id>> = self.holes.iter().map(|hole| ids(hole)).collect();
        holes.sort_unstable();
        (ids(&self.outer), holes)
    }

    /// A point well inside the region, as it's drawn: along each of
    /// [`SCANLINES`] horizontal lines across its box, the middle of the
    /// widest span inside it by the even-odd rule on its outline that
    /// [`Region::contains`] agrees is inside, and of those the one
    /// furthest from the outline. A span's middle can lie on the outline,
    /// where a line runs along an edge or through a corner, and a point
    /// there is as much in the region beside it once the sketch moves a
    /// hair: none if every one is on it.
    fn inside(&self) -> Option<DVec2> {
        let (min, max) = self.bounds;
        let mut best: Option<(f64, DVec2)> = None;
        let mut crossings = Vec::new();
        for k in 1..=SCANLINES {
            let y = min.y + (max.y - min.y) * k as f64 / (SCANLINES + 1) as f64;
            crossings.clear();
            for polyline in &self.outline {
                let Some(&last) = polyline.last() else {
                    continue;
                };
                let mut before = last;
                // As `crosses_odd` counts them.
                for &at in polyline {
                    if (at.y > y) != (before.y > y) {
                        crossings.push(
                            before.x + (y - before.y) / (at.y - before.y) * (at.x - before.x),
                        );
                    }
                    before = at;
                }
            }
            crossings.sort_by(f64::total_cmp);
            let widest = crossings
                .as_chunks::<2>()
                .0
                .iter()
                .map(|span| (span[1] - span[0], DVec2::new((span[0] + span[1]) / 2.0, y)))
                .filter(|&(width, point)| width > 0.0 && point.is_finite() && self.contains(point))
                .reduce(|widest, span| if span.0 > widest.0 { span } else { widest });
            if let Some((_, point)) = widest {
                let clearance = self.clearance(point);
                if clearance > 0.0 && best.is_none_or(|(most, _)| clearance > most) {
                    best = Some((clearance, point));
                }
            }
        }
        best.map(|(_, point)| point)
    }

    /// How far `point` is from the nearest edge of the outline.
    fn clearance(&self, point: DVec2) -> f64 {
        let mut nearest = f64::INFINITY;
        for polyline in &self.outline {
            let Some(&last) = polyline.last() else {
                continue;
            };
            let mut before = last;
            for &at in polyline {
                let edge = at - before;
                let along = (point - before).dot(edge) / edge.length_squared();
                let foot = if along.is_finite() {
                    before + edge * along.clamp(0.0, 1.0)
                } else {
                    before
                };
                nearest = nearest.min(point.distance(foot));
                before = at;
            }
        }
        nearest
    }
}

impl Profiles {
    /// A reference to region `index`, to find it again with
    /// [`Profiles::resolve`]: its curves and a point inside it. None if
    /// there's no such region, it's too thin for a point inside to be
    /// found, or it has more than [`MAX_REGION_CURVES`] curves.
    pub fn reference(&self, index: usize) -> Option<RegionRef> {
        let region = self.regions.get(index)?;
        let (curves, holes) = region.curve_ids();
        let reference = RegionRef {
            curves,
            holes,
            inside: region.inside()?,
        };
        reference.check(f64::MAX).ok()?;
        Some(reference)
    }

    /// The region each of `references` names, in the same order: the one
    /// region bounded by the same curves, outer loop and holes; or, with
    /// several, the one of them its point inside is in; or, with none, the
    /// region its point is in ([`Profiles::region_at`]); or else none, the
    /// region's gone. With several, a point in another region names none:
    /// it's a region of those curves that was meant, and one of other
    /// curves would be extruded without a word.
    pub fn resolve(&self, references: &[RegionRef]) -> Vec<Option<usize>> {
        let ids: Vec<(Vec<Id>, Vec<Vec<Id>>)> =
            self.regions.iter().map(Region::curve_ids).collect();
        references
            .iter()
            .map(|reference| {
                let same = |(curves, holes): &(Vec<Id>, Vec<Vec<Id>>)| {
                    *curves == reference.curves && *holes == reference.holes
                };
                let mut found = ids.iter().enumerate().filter(|(_, ids)| same(ids));
                match (found.next(), found.next()) {
                    (Some((index, _)), None) => Some(index),
                    (None, _) => self.region_at(reference.inside),
                    (Some(_), Some(_)) => self
                        .region_at(reference.inside)
                        .filter(|&index| same(&ids[index])),
                }
            })
            .collect()
    }
}

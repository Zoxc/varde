//! Offsetting a chain of curves
//! ([`SketchEdit::Offset`](crate::SketchEdit::Offset)): a copy of each
//! line and arc of the chain, or of a circle, a distance to one side
//! (a spline alone has a copy of its own, see `offset/spline.rs`),
//! joined where they meet, less what falls within that distance of the
//! chain, tied to the original so one dimension drives it all. With what
//! the Offset tool needs before it's made: the chain a curve is in
//! ([`Sketch::chain_of`]), how far and on which side of it a place is
//! ([`Sketch::offset_side`]) and the copy as it would be
//! ([`Sketch::offset_preview`]).
//!
//! The stages: the offset pair vocabulary measures and constraints use
//! (`pair`), finding the chain (`chain`), the raw offset and what of it
//! survives, pure geometry (`raw`), and which ties the copies keep
//! (`ties`); this module commits the copy to the sketch.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use glam::DVec2;
use varde_expr::Value;

use crate::intersect::Geom;
use crate::{Constraint, Curve, Design, Dimension, EditError, Id, Measure, Side, Sketch};

mod chain;
mod pair;
mod raw;
mod spline;
#[cfg(test)]
mod tests;
mod ties;

use chain::*;
pub use pair::OffsetPair;
use raw::*;
use ties::*;

/// The most two lines of a chain may turn by at a corner for their copies
/// to run on to meet where a gap opens: 150°, the corner then reaching
/// under four times the distance out. Past it, a round join closes the
/// gap, as it would a needle's worth of corner.
pub const MITER_TURN: f64 = 5.0 * PI / 6.0;

/// How many times the tolerance (see
/// [`tolerance`](crate::intersect::tolerance)) places along the copy are
/// one within: where two curves touch, rounding puts the two places
/// they cross at up to about the square root of the tolerance times their
/// size apart, which would leave a sliver between.
const NEAR: f64 = 100.0;

/// How many times the tolerance two loose ends of the copy are one within,
/// each the end of one part only: where a copy touches the edge of the
/// band round another piece, it's within the distance of it by as little
/// as the square of how far from where it touches, so a part up to about
/// the root of the tolerance times the sketch's size past it can be kept,
/// at the tolerance's size, the one to 10⁻⁹. Such a sliver leaves two ends
/// that should be one that far apart: made one, the sliver's gone.
const SLIVER: f64 = 5e4;

/// How near copies meeting at a point have to run to the same way there
/// to be tangent: the sine of the angle between them.
const TANGENT: f64 = 1e-6;

/// The most steps an offset takes working out what's left: pairs of curves
/// met while cutting, parts tested against pieces, parts compared, their
/// ends compared to make those near each other one, past
/// which it's refused as too complex ([`EditError::TooComplex`]), in a
/// few tenths of a second at most: a chain of a thousand pieces or so.
pub const MAX_OFFSET_WORK: usize = 10_000_000;

/// How far to the left of a curve (to the right, below zero) its offset
/// `distance` to `side` is. `NothingLeft` for no distance.
pub(super) fn offset_shift(distance: f64, side: Side) -> Result<f64, EditError> {
    if !(distance > 0.0 && distance.is_finite()) {
        return Err(EditError::NothingLeft);
    }
    Ok(distance * side.sign())
}

/// Where the kept parts of an offset end: the places, each kept part's
/// two of them (none for a whole circle), and the point made at each.
struct KeptEnds {
    places: Vec<DVec2>,
    ends: Vec<Option<(usize, usize)>>,
    points: Vec<Id>,
}

impl Sketch {
    /// How far `at` is from the chain of `chain` (see
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset)) and on which side
    /// of it, as an offset through `at` would be: from the piece or corner
    /// nearest it, the left of the way the chain runs [`Side::Positive`].
    /// `None` if `chain` is no chain.
    pub fn offset_side(&self, chain: &[Id], at: DVec2) -> Option<(f64, Side)> {
        if let Some(spline) = self.lone_spline(chain) {
            return self.spline_side(spline, at);
        }
        let (distance, side) = self.chained(chain).ok()?.nearest(at, 0.0)?;
        // Past a corner turning back on itself, either: the left.
        let side = side.unwrap_or(Side::Positive);
        distance.is_finite().then_some((distance, side))
    }

    /// The copy offsetting the chain `chain` by `distance` to `side` would
    /// make (see [`SketchEdit::Offset`](crate::SketchEdit::Offset)), as
    /// polylines, each curve its own: a spline's exact offset, which its
    /// copy follows. Refused as the edit is: no chain, nothing left, too
    /// complex, a spline's copy folding.
    pub fn offset_preview(
        &self,
        chain: &[Id],
        distance: f64,
        side: Side,
    ) -> Result<Vec<Vec<DVec2>>, EditError> {
        if let Some(spline) = self.lone_spline(chain) {
            return Ok(vec![self.spline_offset_preview(spline, distance, side)?]);
        }
        let chain = self.chained(chain)?;
        let offset = self.raw_offset(&chain, distance, side)?;
        Ok(offset
            .kept
            .iter()
            .map(|kept| {
                let geom = &offset.raws[kept.raw].geom;
                geom.polyline(kept.from, kept.to)
            })
            .collect())
    }

    /// Offsets the chain `chain` by `distance` to `side`, see
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset). The sketch may be
    /// left part way on failure.
    pub(crate) fn offset(
        &mut self,
        chain: &[Id],
        distance: &Value,
        side: Side,
        design: &Design,
    ) -> Result<(), EditError> {
        let measure = Measure::Offset(Id::ORIGIN, Id::ORIGIN);
        let ask = measure.ask(design);
        if distance.check(&ask).is_err() {
            return Err(EditError::OutOfRange {
                measured: distance.value,
                min: ask.min.unwrap_or(0.0),
                max: ask.max,
                angle: false,
            });
        }
        if let Some(spline) = self.lone_spline(chain) {
            return self.offset_spline(spline, distance, side);
        }
        let chain = self.chained(chain)?;
        let mut offset = self.raw_offset(&chain, distance.value, side)?;
        let ends = self.offset_points(&mut offset)?;
        let copies = self.offset_copies(&chain, &offset, &ends)?;
        // Kept parts' copies, as they're numbered among the copies made.
        let mut numbered = Vec::with_capacity(copies.len());
        let mut made = Vec::new();
        for copy in copies {
            numbered.push(copy.map(|_| made.len()));
            made.extend(copy);
        }
        let mut copies = made;
        let tangents = self.offset_tangents(&offset, &ends, &numbered, &copies);
        // A part too short to be a curve leaves its ends unused.
        let used: BTreeSet<Id> = copies
            .iter()
            .filter_map(|copy| self.curve(copy.id))
            .flat_map(|entry| entry.curve.points())
            .collect();
        self.points
            .retain(|point| !ends.points.contains(&point.id) || used.contains(&point.id));
        let pairs: Vec<[usize; 2]> = tangents.iter().map(|(pair, _)| *pair).collect();
        let untied = untie(&copies, &pairs).ok_or(EditError::NothingLeft)?;
        let tangents = self.apply_untie(&mut copies, &untied, tangents)?;
        self.add_offset_ties(&copies, copies[untied.lead], tangents, distance)
    }

    /// The points the kept parts of `offset` end at, those near one place
    /// one point.
    fn offset_points(&mut self, offset: &mut Offset) -> Result<KeptEnds, EditError> {
        let near = NEAR * offset.tol;
        let mut places: Vec<DVec2> = Vec::new();
        let mut point_at = |at: DVec2, work: &mut Work| -> Result<usize, EditError> {
            work.spend(places.len())?;
            Ok(match places.iter().position(|p| p.distance(at) <= near) {
                Some(index) => index,
                None => {
                    places.push(at);
                    places.len() - 1
                }
            })
        };
        let mut ends: Vec<Option<(usize, usize)>> = Vec::with_capacity(offset.kept.len());
        for kept in &offset.kept {
            let geom = &offset.raws[kept.raw].geom;
            ends.push(if kept.whole {
                None
            } else {
                let from = point_at(geom.at(kept.from), &mut offset.work)?;
                Some((from, point_at(geom.at(kept.to), &mut offset.work)?))
            });
        }
        loose_ends(&mut ends, &places, SLIVER * offset.tol, &mut offset.work)?;
        let mut points = Vec::with_capacity(places.len());
        for &at in &places {
            points.push(self.add_point(at)?);
        }
        Ok(KeptEnds {
            places,
            ends,
            points,
        })
    }

    /// The copy of each kept part of `offset`, of the piece or join of
    /// `chain` it's from, or none where it's too short to be a curve.
    fn offset_copies(
        &mut self,
        chain: &Chain,
        offset: &Offset,
        ends: &KeptEnds,
    ) -> Result<Vec<Option<OffsetCopy>>, EditError> {
        let points = &ends.points;
        let mut copies: Vec<Option<OffsetCopy>> = Vec::with_capacity(offset.kept.len());
        for (kept, ends) in offset.kept.iter().zip(&ends.ends) {
            let raw = &offset.raws[kept.raw];
            let (index, base, pair) = match raw.source {
                Source::Piece(index) => {
                    let link = &chain.links[index];
                    let pair = match link.geom {
                        Geom::Segment { .. } => OffsetPair::Lines,
                        Geom::Round { .. } => OffsetPair::Rounds,
                        Geom::Spline(_) => return Err(EditError::NotAChain),
                    };
                    (index, link.id, pair)
                }
                Source::Join { corner, after } => (after, corner, OffsetPair::Join),
            };
            let entry = self
                .curve(chain.links[index].id)
                .ok_or(EditError::Target(chain.links[index].id))?;
            let construction = entry.construction;
            let center = entry.curve.center();
            let curve = match (pair, *ends) {
                (OffsetPair::Lines, Some((from, to))) => {
                    let (from, to) = (points[from], points[to]);
                    // The copy runs the way its original does.
                    let forward = chain.links[index].forward;
                    let (start, end) = if forward { (from, to) } else { (to, from) };
                    (start != end).then_some(Curve::Line { start, end })
                }
                (OffsetPair::Rounds, None) => {
                    let Geom::Round { radius, .. } = raw.geom else {
                        return Err(EditError::Target(base));
                    };
                    let center = center.ok_or(EditError::Target(base))?;
                    Some(Curve::Circle { center, radius })
                }
                (OffsetPair::Rounds | OffsetPair::Join, Some((from, to))) => {
                    let center = if pair == OffsetPair::Join {
                        base
                    } else {
                        center.ok_or(EditError::Target(base))?
                    };
                    let (start, end) = (points[from], points[to]);
                    (start != end).then_some(Curve::Arc { center, start, end })
                }
                _ => None,
            };
            let copy = match curve {
                Some(curve) => Some(OffsetCopy {
                    id: self.add_curve(curve, construction)?,
                    base,
                    pair,
                    distance: true,
                    parallel: pair == OffsetPair::Lines,
                }),
                None => None,
            };
            copies.push(copy);
        }
        Ok(copies)
    }

    /// The tangents between the copies made, `copies`, `numbered` among
    /// them by the kept parts of `offset`, ending at `ends`, each with the
    /// two it's between.
    ///
    /// Where two copies meet tangent at a point they share, a tangent
    /// stands for one of their ties, never the last of their offsets: on
    /// both their offsets, the point would be where two tangent curves
    /// meet, which the solver can't tell along them.
    fn offset_tangents(
        &self,
        offset: &Offset,
        ends: &KeptEnds,
        numbered: &[Option<usize>],
        copies: &[OffsetCopy],
    ) -> Vec<([usize; 2], Constraint)> {
        let places = &ends.places;
        let mut meeting: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, ends) in ends.ends.iter().enumerate() {
            if let Some((from, to)) = *ends {
                meeting.entry(from).or_default().push(index);
                meeting.entry(to).or_default().push(index);
            }
        }
        let mut tangents = Vec::new();
        for (&at, parts) in &meeting {
            let &[a, b] = parts.as_slice() else {
                continue;
            };
            // Which way each runs where the point is, which may have been
            // made one with another near.
            let way = |index: usize| {
                let geom = &offset.raws[offset.kept[index].raw].geom;
                geom.heading(geom.closest(places[at]), true).0
            };
            if way(a).perp_dot(way(b)).abs() > TANGENT {
                continue;
            }
            let (Some(a), Some(b)) = (numbered[a], numbered[b]) else {
                continue;
            };
            // Two parts of one circle say nothing to each other.
            let about = |copy: &OffsetCopy| self.curve(copy.id)?.curve.center();
            if about(&copies[a]).is_some() && about(&copies[a]) == about(&copies[b]) {
                continue;
            }
            if let Some(tangent) = self.tangent(copies[a].id, copies[b].id) {
                tangents.push(([a, b], tangent));
            }
        }
        tangents
    }

    /// Applies `untied` to `copies`: drops the ties its tangents (of
    /// `tangents`) stand for and gives the freed arcs centres of their
    /// own. The tangents kept.
    fn apply_untie(
        &mut self,
        copies: &mut [OffsetCopy],
        untied: &Untied,
        tangents: Vec<([usize; 2], Constraint)>,
    ) -> Result<Vec<Constraint>, EditError> {
        let lead = untied.lead;
        let mut dropped = vec![0usize; copies.len()];
        let tangents: Vec<Constraint> = tangents
            .into_iter()
            .zip(&untied.assigned)
            .filter_map(|((_, tangent), &copy)| {
                dropped[copy?] += 1;
                Some(tangent)
            })
            .collect();
        for (index, copy) in copies.iter_mut().enumerate() {
            if untied.freed[index] {
                // About a centre of its own, which its first two tangents
                // hold.
                let entry = self.curve(copy.id).ok_or(EditError::Target(copy.id))?;
                let Curve::Arc { center, start, end } = entry.curve else {
                    return Err(EditError::Target(copy.id));
                };
                let at = self.point(center).ok_or(EditError::Target(center))?.at;
                let center = self.add_point(at)?;
                let entry = self.curve_mut(copy.id).ok_or(EditError::Target(copy.id))?;
                entry.curve = Curve::Arc { center, start, end };
                dropped[index] = dropped[index].saturating_sub(2);
            }
            // A line's offset goes first, then its parallel; the lead's
            // stays.
            match (dropped[index], index == lead) {
                (0, _) => {}
                (1, true) => copy.parallel = false,
                (1, false) => copy.distance = false,
                _ => (copy.distance, copy.parallel) = (false, false),
            }
        }
        Ok(tangents)
    }

    /// Adds the ties of `copies`, `lead`'s offset the dimension of
    /// `distance`, and `tangents`.
    fn add_offset_ties(
        &mut self,
        copies: &[OffsetCopy],
        lead: OffsetCopy,
        tangents: Vec<Constraint>,
        distance: &Value,
    ) -> Result<(), EditError> {
        for copy in copies {
            if copy.parallel {
                self.add_constraint(Constraint::Parallel(copy.base, copy.id))?;
            }
        }
        let others = copies
            .iter()
            .filter(|copy| copy.distance && copy.id != lead.id)
            .map(|copy| [copy.base, copy.id]);
        self.tie_offsets([lead.base, lead.id], others, distance)?;
        for tangent in tangents {
            self.add_constraint(tangent)?;
        }
        Ok(())
    }

    /// Holds the offset pair `lead` by a driving [`Measure::Offset`] of
    /// `distance`, and each of `others` by a [`Constraint::EqualOffset`]
    /// to it: the one dimension drives the whole copy.
    pub(super) fn tie_offsets(
        &mut self,
        lead: [Id; 2],
        others: impl IntoIterator<Item = [Id; 2]>,
        distance: &Value,
    ) -> Result<(), EditError> {
        for other in others {
            self.add_constraint(Constraint::EqualOffset { a: lead, b: other })?;
        }
        let measure = Measure::Offset(lead[0], lead[1]);
        let side = self.side(&measure);
        self.add_dimension(Dimension {
            measure,
            value: distance.clone(),
            driving: true,
            label: DVec2::ZERO,
            side,
        })?;
        Ok(())
    }
}

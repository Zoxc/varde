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
//! **The raw offset.** Each piece is offset exactly, to the left of the
//! way the chain runs for [`Side::Positive`]: a line along itself, an arc
//! or a circle about its centre. Where two pieces meet, their copies meet
//! too if the pieces are tangent there; where the corner turns towards
//! the copy, the copies cross, and what's past the crossing falls within
//! the distance and goes (below); where it turns away, a gap opens, which
//! two lines close by running on to meet (a sharp corner, as the corner
//! of the original is), unless they turn by more than [`MITER_TURN`], and
//! anything else by an arc about the corner (a round join).
//!
//! An arc offset towards its centre by more than its radius has no copy:
//! its ends are rounded instead, by circles about them that what's kept
//! (below) cuts down to what's beyond the distance.
//!
//! **What survives.** Where the distance is larger than a feature of the
//! chain, the raw offset crosses itself, and pieces of it fall within the
//! distance of some other part of the chain: those go. Each raw curve is
//! cut wherever its distance from the chain can reach the distance: where
//! it meets another raw curve or the edge of the band the distance makes
//! round a piece (its copies either side, and circles round its ends), and
//! each part kept if its middle is no nearer the chain than the distance,
//! exactly, by the pieces themselves, on the side offset to (by the piece
//! it's nearest, or the corner, which only the side it points to is
//! nearest), and not inside a corner run on to meet. Copies lying on each
//! other (a loop's sides offset inwards by half its width) leave nothing
//! between them, and both go. That's what clipper2's offset of the
//! flattened chain would say, but only to its flattening's tolerance,
//! which the parts barely inside can't be told by (see "Built so far (step
//! 6b)" in `notes/SketchImpl.md`).
//!
//! **Ties.** Each copy of a line is [`Constraint::Parallel`] to it, and a
//! copy of an arc or a circle made about the same centre point; a round
//! join is made about the corner. How far each copy is from its original
//! is an offset pair ([`Sketch::offset_pair`]): the first copy's is held
//! by a driving [`Measure::Offset`] of the distance, and every other's
//! [`Constraint::EqualOffset`] to it, so the whole copy follows the one
//! dimension. Copies meeting at a point share it; where they're tangent
//! there they're held [`Constraint::Tangent`], which takes the place of one
//! of their own ties (a line's offset, then its parallel, an arc's
//! offset), as the point where two tangent curves meet is no place the
//! solver can tell along them: which tie each tangent takes is a matching
//! ([`untie`]). Round a loop of arcs alone, each tangent to the next, one
//! copy is made about a centre of its own, where its original's is.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::angle;
use varde_expr::Value;

use crate::intersect::{Geom, meet, tolerance};
use crate::{
    Constraint, Curve, Design, Dimension, EditError, Id, Kind, Measure, Side, Sketch, arc_sweep,
    crossing,
};

/// The most two lines of a chain may turn by at a corner for their copies
/// to run on to meet where a gap opens: 150°, the corner then reaching
/// under four times the distance out. Past it, a round join closes the
/// gap, as it would a needle's worth of corner.
pub const MITER_TURN: f64 = 5.0 * PI / 6.0;

/// How many times the tolerance (see [`tolerance`]) places along the copy
/// are one within: where two curves touch, rounding puts the two places
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
    /// off, which [`Measure::Offset`] measures and
    /// [`Constraint::EqualOffset`] ties. Two lines (the first may be an
    /// axis), two circles or arcs, a point and a circle or an arc about
    /// it, or a spline and a point, not one of its own. `None` for
    /// anything else, or an item twice.
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
    /// signed (see [`Measure::Offset`]). `None` if it's no offset pair, or
    /// a line has no length.
    pub(crate) fn offset_of(&self, pair: [Id; 2]) -> Option<f64> {
        let [a, b] = pair;
        let offset = match self.offset_pair(pair)? {
            OffsetPair::Lines => {
                let ((start, end), (from, to)) = (self.line(a)?, self.line(b)?);
                let along = end - start;
                along.perp_dot(from.midpoint(to) - start) / along.length()
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
        match self.curve(id)?.curve {
            Curve::Arc { center, start, end } => {
                let at = |id| self.point(id).map(|point| point.at);
                let center = at(center)?;
                let from = at(start)? - center;
                let sweep = arc_sweep(from, at(end)? - center);
                angle::from_angle(sweep / 2.0).rotate(from).try_normalize()
            }
            Curve::Circle { .. } => Some(DVec2::splat(std::f64::consts::FRAC_1_SQRT_2)),
            Curve::Line { .. } | Curve::Spline(_) => None,
        }
    }

    /// The chain the curve `curve` is in: the curves joined to it end to
    /// end, on through each point where two of them end and nothing else
    /// does, `curve` first and then the rest in no order that matters. A
    /// circle or a closed spline alone. Empty if `curve` is none of the
    /// sketch's.
    pub fn chain_of(&self, curve: Id) -> Vec<Id> {
        let Some(entry) = self.curve(curve) else {
            return Vec::new();
        };
        if let Curve::Circle { .. } = entry.curve {
            return vec![curve];
        }
        let ending = ending(
            self.curves
                .iter()
                .map(|entry| (entry.id, entry.curve.ends())),
        );
        let mut chain = vec![curve];
        let mut seen = BTreeSet::from([curve]);
        let mut next = 0;
        while let Some(&id) = chain.get(next) {
            next += 1;
            let Some(ends) = self.curve(id).and_then(|entry| entry.curve.ends()) else {
                continue;
            };
            for point in ends {
                if let Some(&[a, b]) = ending.get(&point).map(Vec::as_slice) {
                    let other = if a == id { b } else { a };
                    if seen.insert(other) {
                        chain.push(other);
                    }
                }
            }
        }
        chain
    }

    /// Whether the curves `ids` are a chain an offset can copy: lines and
    /// arcs joined end to end, no point the end of more than two, or a
    /// circle or a spline alone (see
    /// [`SketchEdit::Offset`](crate::SketchEdit::Offset)).
    pub fn is_chain(&self, ids: &[Id]) -> bool {
        self.lone_spline(ids).is_some() || self.chained(ids).is_ok()
    }

    /// The curves `ids` as a chain, run so that the first goes forward
    /// (from its start, an arc counter-clockwise): each line or arc joined
    /// to the next end to end, a loop if the last ends where the first
    /// starts, or a circle alone. `Target` for an id that's no line, arc
    /// or circle of the sketch's, or one with no size; `NotAChain` if they
    /// don't join up so, a point is the end of more than two, or one is a
    /// spline, which is offset alone (see `offset/spline.rs`).
    fn chained(&self, ids: &[Id]) -> Result<Chain, EditError> {
        let mut seen = BTreeSet::new();
        let ids: Vec<Id> = ids.iter().copied().filter(|&id| seen.insert(id)).collect();
        let &first = ids.first().ok_or(EditError::NotAChain)?;
        let mut shapes = BTreeMap::new();
        for &id in &ids {
            let entry = self.curve(id).ok_or(EditError::Target(id))?;
            let geom = Geom::of(self, &entry.curve).ok_or(EditError::Target(id))?;
            if let Geom::Spline(_) = geom {
                return Err(EditError::NotAChain);
            }
            shapes.insert(id, (entry.curve.ends(), geom));
        }
        let link = |id: Id, forward: bool| Link {
            id,
            forward,
            geom: shapes[&id].1.clone(),
        };
        let ends_of = |id: Id| shapes[&id].0;
        if ends_of(first).is_none() {
            return if ids.len() == 1 {
                Ok(Chain {
                    links: vec![link(first, true)],
                    closed: true,
                })
            } else {
                Err(EditError::NotAChain)
            };
        }
        if ids.iter().any(|&id| ends_of(id).is_none()) {
            return Err(EditError::NotAChain);
        }
        let ending = ending(ids.iter().map(|&id| (id, ends_of(id))));
        if ending.values().any(|curves| curves.len() > 2) {
            return Err(EditError::NotAChain);
        }
        // The curve other than `from` ending at `point`, and whether it
        // runs away from there.
        let beyond = |from: Id, point: Id| {
            let curves = ending.get(&point)?;
            let &next = curves.iter().find(|&&id| id != from)?;
            let [start, _] = ends_of(next)?;
            Some((next, start == point))
        };
        let [first_start, first_end] = ends_of(first).ok_or(EditError::NotAChain)?;
        let (mut ahead, mut closed) = (Vec::new(), false);
        let (mut at, mut from) = (first_end, first);
        while let Some((next, away)) = beyond(from, at) {
            if next == first {
                closed = true;
                break;
            }
            // Joined up otherwise, it's been round already: can't be, as
            // no point ends more than two.
            if ahead.len() > ids.len() {
                return Err(EditError::NotAChain);
            }
            ahead.push(link(next, away));
            let [start, end] = ends_of(next).ok_or(EditError::NotAChain)?;
            at = if away { end } else { start };
            from = next;
        }
        let mut behind = Vec::new();
        if !closed {
            let (mut at, mut from) = (first_start, first);
            while let Some((next, away)) = beyond(from, at) {
                if behind.len() > ids.len() {
                    return Err(EditError::NotAChain);
                }
                // Run towards `at`: forward if it ends there.
                behind.push(link(next, !away));
                let [start, end] = ends_of(next).ok_or(EditError::NotAChain)?;
                at = if away { end } else { start };
                from = next;
            }
        }
        let links: Vec<Link> = behind
            .into_iter()
            .rev()
            .chain([link(first, true)])
            .chain(ahead)
            .collect();
        if links.len() != ids.len() {
            return Err(EditError::NotAChain);
        }
        Ok(Chain { links, closed })
    }

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

    /// The offset of `chain` by `distance` to `side`: its raw curves and
    /// the parts of them kept. `NothingLeft` if none are.
    fn raw_offset(&self, chain: &Chain, distance: f64, side: Side) -> Result<Offset, EditError> {
        if !(distance > 0.0 && distance.is_finite()) {
            return Err(EditError::NothingLeft);
        }
        let shift = distance * side.sign();
        let mut raws: Vec<Raw> = Vec::new();
        // Each piece's raw curve, by the piece.
        let mut of_piece = Vec::with_capacity(chain.links.len());
        for (index, link) in chain.links.iter().enumerate() {
            of_piece.push(link.raw(shift).map(|geom| {
                raws.push(Raw {
                    forward: link.forward || matches!(geom, Geom::Segment { .. }),
                    geom,
                    source: Source::Piece(index),
                });
                raws.len() - 1
            }));
        }
        // Places are one within the tolerance of the sketch and the copy
        // together.
        let geoms: Vec<Geom> = self.geoms().collect();
        let tol = tolerance(geoms.iter().chain(raws.iter().map(|raw| &raw.geom)));
        // The corners run on to meet: each the corner of the chain, where
        // the one copy ended, where they meet and where the other started.
        let mut corners = Vec::new();
        let count = chain.links.len();
        let joins = if chain.closed && count > 1 {
            count
        } else {
            count - 1
        };
        for before in 0..joins {
            let after = (before + 1) % count;
            let (Some(i), Some(j)) = (of_piece[before], of_piece[after]) else {
                continue;
            };
            let (links, corner) = (&chain.links, chain.corner(self, before));
            let Some(corner) = corner else {
                continue;
            };
            let (end, start) = (raws[i].end(), raws[j].start());
            if end.distance(start) <= tol {
                continue;
            }
            let way_in = links[before].heading_out();
            let way_out = links[after].heading_in();
            let turn = way_in.perp_dot(way_out);
            let opens = turn * shift < 0.0 || (turn == 0.0 && way_in.dot(way_out) < 0.0);
            if !opens {
                // The copies cross.
                cross(
                    &mut raws,
                    [i, j],
                    [&links[after..=after], &links[before..=before]],
                    distance,
                    tol,
                );
                continue;
            }
            let lines = matches!(
                (&raws[i].geom, &raws[j].geom),
                (Geom::Segment { .. }, Geom::Segment { .. })
            );
            let angle = angle::between(way_in, way_out);
            let met = crossing(end, way_in, start, way_out)
                .filter(|&(t, u)| lines && angle <= MITER_TURN && t >= 0.0 && u <= 0.0)
                .map(|(t, _)| end + way_in * t);
            if let Some(met) = met {
                raws[i].set_end(met);
                raws[j].set_start(met);
                corners.push([corner.1, end, met, start]);
                continue;
            }
            let (center, at) = corner;
            let (from, to) = (end - at, start - at);
            // Round the corner from the one copy's end to the other's
            // start, clockwise turning right, counter-clockwise left.
            let (begin, sweep, forward) = if shift > 0.0 {
                (angle::to_angle(to), arc_sweep(to, from), false)
            } else {
                (angle::to_angle(from), arc_sweep(from, to), true)
            };
            raws.push(Raw {
                geom: Geom::Round {
                    center: at,
                    radius: distance,
                    begin,
                    sweep,
                },
                forward,
                source: Source::Join {
                    corner: center,
                    after: before,
                },
            });
        }
        // Pieces with no copy between two that have: theirs may cross past
        // them, as copies at a corner do.
        let kept: Vec<usize> = (0..count).filter(|&k| of_piece[k].is_some()).collect();
        let runs = kept.windows(2).map(|pair| (pair[0], pair[1]));
        let round = (chain.closed && kept.len() > 1).then(|| (kept[kept.len() - 1], kept[0]));
        for (before, after) in runs.chain(round) {
            let between = (after + count - before) % count;
            if between < 2 {
                continue;
            }
            let (Some(i), Some(j)) = (of_piece[before], of_piece[after]) else {
                continue;
            };
            // The pieces after `before` up to `after`, and from `before`
            // up to the one before `after`, round the loop.
            let from: Vec<Link> = (1..=between)
                .map(|k| chain.links[(before + k) % count].clone())
                .collect();
            let to: Vec<Link> = (0..between)
                .map(|k| chain.links[(before + k) % count].clone())
                .collect();
            cross(&mut raws, [i, j], [&from, &to], distance, tol);
        }
        // An arc offset inwards by more than its radius has no copy: round
        // each of its ends that another piece meets, as round joins would
        // do, whose parts beyond the distance are what's left there.
        let mut rounded = BTreeSet::new();
        let vanished = of_piece.iter().enumerate().filter(|(_, raw)| raw.is_none());
        for (index, _) in vanished {
            let joined = [
                (index > 0 || chain.closed).then(|| (index + count - 1) % count),
                (index + 1 < count || chain.closed).then_some(index),
            ];
            for before in joined.into_iter().flatten() {
                let Some((corner, at)) = chain.corner(self, before) else {
                    continue;
                };
                if count > 1 && rounded.insert(corner) {
                    raws.push(Raw {
                        geom: Geom::Round {
                            center: at,
                            radius: distance,
                            begin: 0.0,
                            sweep: TAU,
                        },
                        forward: true,
                        source: Source::Join {
                            corner,
                            after: index,
                        },
                    });
                }
            }
        }
        let mut work = Work::default();
        let kept = self.kept(chain, &raws, &corners, distance, side, tol, &mut work)?;
        if kept.is_empty() {
            return Err(EditError::NothingLeft);
        }
        Ok(Offset {
            raws,
            kept,
            tol,
            work,
        })
    }

    /// The parts of `raws` no nearer the pieces of `chain` than
    /// `distance`, each as long as it can be, see the module's docs.
    #[allow(clippy::too_many_arguments)]
    fn kept(
        &self,
        chain: &Chain,
        raws: &[Raw],
        corners: &[[DVec2; 4]],
        distance: f64,
        side: Side,
        tol: f64,
        work: &mut Work,
    ) -> Result<Vec<Kept>, EditError> {
        // The band's edges round each piece.
        let mut edges = Vec::new();
        for link in &chain.links {
            edges.extend(link.band(distance, tol));
        }
        let bounds: Vec<(DVec2, DVec2)> = raws.iter().map(|raw| raw.geom.bounds()).collect();
        let edge_bounds: Vec<(DVec2, DVec2)> = edges.iter().map(Geom::bounds).collect();
        let overlap = |(a_min, a_max): (DVec2, DVec2), (b_min, b_max): (DVec2, DVec2)| {
            a_min.cmple(b_max + tol).all() && b_min.cmple(a_max + tol).all()
        };
        let mut kept = Vec::new();
        let mut found = Vec::new();
        for (index, raw) in raws.iter().enumerate() {
            let geom = &raw.geom;
            let last = geom.last();
            let mut cuts = Vec::new();
            let others = raws
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .map(|(other, raw)| (&raw.geom, bounds[other]))
                .chain(edges.iter().zip(edge_bounds.iter().copied()));
            for (other, other_bounds) in others {
                work.spend(1)?;
                if !overlap(bounds[index], other_bounds) {
                    continue;
                }
                found.clear();
                meet(geom, other, tol, &mut found);
                // Where a circle starts is no end of it, and changes
                // nothing, but `meet` cuts there as at an end.
                let starts = |geom: &Geom, u: f64| !geom.has_ends() && u == 0.0;
                let cut = found
                    .iter()
                    .filter(|&&(u, v)| !starts(geom, u) && !starts(other, v));
                cuts.extend(cut.map(|&(u, _)| u));
            }
            // Cuts at its ends, or near them, cut nothing.
            let near = NEAR * tol;
            cuts.retain(|&u| {
                u.is_finite()
                    && (!geom.has_ends()
                        || (geom.length(0.0, u) > near && geom.length(u, last) > near))
            });
            cuts.sort_by(f64::total_cmp);
            let mut places: Vec<f64> = Vec::with_capacity(cuts.len() + 2);
            if geom.has_ends() {
                places.push(0.0);
            }
            for u in cuts {
                if places
                    .last()
                    .is_none_or(|&before| geom.length(before, u) > near)
                {
                    places.push(u);
                }
            }
            if geom.has_ends() {
                places.push(last);
            } else if places.len() > 1
                && let (Some(&first), Some(&end)) = (places.first(), places.last())
                && geom.length(end, first + TAU) <= near
            {
                // Round a circle, the last is near the first.
                places.pop();
            }
            // The parts between, round a circle from the last cut back to
            // the first.
            let spans: Vec<(f64, f64)> = if geom.has_ends() {
                places.windows(2).map(|pair| (pair[0], pair[1])).collect()
            } else if places.is_empty() {
                vec![(0.0, TAU)]
            } else {
                let mut spans: Vec<(f64, f64)> =
                    places.windows(2).map(|pair| (pair[0], pair[1])).collect();
                let (first, end) = (places[0], places[places.len() - 1]);
                spans.push((end, first + TAU));
                spans
            };
            // Nearer a piece than the distance, it's within the band, and
            // inside a corner run on to meet it's within the copy too.
            // And on the other side of the chain, it's no part of this
            // side's copy.
            let keeps = |(from, to): (f64, f64)| {
                let middle = geom.at((from + to) / 2.0);
                geom.length(from, to) > near
                    && chain.beyond(middle, distance, side, tol)
                    && !corners.iter().any(|corner| within(corner, middle, tol))
            };
            work.spend(spans.len().saturating_mul(chain.links.len()))?;
            let mut parts: Vec<(f64, f64)> = Vec::new();
            for span in spans {
                if !keeps(span) {
                    continue;
                }
                match parts.last_mut() {
                    // Joined to the part before, with nothing cut out.
                    Some(part) if part.1 == span.0 => part.1 = span.1,
                    _ => parts.push(span),
                }
            }
            // Round a circle, the last part may go on into the first.
            if !geom.has_ends() && parts.len() > 1 {
                let (first, end) = (parts[0], parts[parts.len() - 1]);
                if end.1 == first.0 + TAU {
                    parts.pop();
                    parts[0] = (end.0, first.1 + TAU);
                }
            }
            let whole = !geom.has_ends()
                && parts.len() == 1
                && geom.length(parts[0].0, parts[0].1) >= geom.length(0.0, TAU) - NEAR * tol;
            for (from, to) in parts {
                kept.push(Kept {
                    raw: index,
                    from,
                    to,
                    whole,
                });
            }
        }
        // Copies on each other, as those of a loop's sides offset inwards
        // by half its width are, leave no width between them: both go.
        let on_another = |index: usize, kept_one: &Kept| {
            let geom = &raws[kept_one.raw].geom;
            let middle = geom.at((kept_one.from + kept_one.to) / 2.0);
            kept.iter().enumerate().any(|(other, part)| {
                other != index
                    && part.raw != kept_one.raw
                    && span_distance(&raws[part.raw].geom, part.from, part.to, middle) <= tol
            })
        };
        let lying = kept
            .iter()
            .enumerate()
            .map(|(index, part)| {
                work.spend(kept.len())?;
                Ok(on_another(index, part))
            })
            .collect::<Result<Vec<bool>, EditError>>()?;
        Ok(kept
            .into_iter()
            .zip(lying)
            .filter_map(|(part, lying)| (!lying).then_some(part))
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
        let near = NEAR * offset.tol;

        // The ends of the parts kept, those near one place one point.
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

        let mut copies: Vec<Option<Copy>> = Vec::with_capacity(offset.kept.len());
        for (kept, ends) in offset.kept.iter().zip(&ends) {
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
                Some(curve) => Some(Copy {
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

        // Where two copies meet tangent at a point they share, a tangent
        // stands for one of their ties, never the last of their offsets: on
        // both their offsets, the point would be where two tangent curves
        // meet, which the solver can't tell along them.
        let mut meeting: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, ends) in ends.iter().enumerate() {
            if let Some((from, to)) = *ends {
                meeting.entry(from).or_default().push(index);
                meeting.entry(to).or_default().push(index);
            }
        }
        // Kept parts' copies, as they're numbered among the copies made.
        let mut numbered = Vec::with_capacity(copies.len());
        let mut made = Vec::new();
        for copy in copies {
            numbered.push(copy.map(|_| made.len()));
            made.extend(copy);
        }
        let mut copies = made;
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
            let about = |copy: &Copy| self.curve(copy.id)?.curve.center();
            if about(&copies[a]).is_some() && about(&copies[a]) == about(&copies[b]) {
                continue;
            }
            if let Some(tangent) = self.tangent(copies[a].id, copies[b].id) {
                tangents.push(([a, b], tangent));
            }
        }
        // A part too short to be a curve leaves its ends unused.
        let used: BTreeSet<Id> = copies
            .iter()
            .filter_map(|copy| self.curve(copy.id))
            .flat_map(|entry| entry.curve.points())
            .collect();
        self.points
            .retain(|point| !points.contains(&point.id) || used.contains(&point.id));
        let pairs: Vec<[usize; 2]> = tangents.iter().map(|(pair, _)| *pair).collect();
        let untied = untie(&copies, &pairs).ok_or(EditError::NothingLeft)?;
        let lead = untied.lead;
        let mut dropped = vec![0usize; copies.len()];
        let tangents: Vec<Constraint> = tangents
            .into_iter()
            .zip(untied.assigned)
            .filter_map(|((_, tangent), copy)| {
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
        let lead = copies[lead];
        let lead_pair = [lead.base, lead.id];
        let measure = Measure::Offset(lead.base, lead.id);
        let side = self.side(&measure);
        for copy in &copies {
            if copy.parallel {
                self.add_constraint(Constraint::Parallel(copy.base, copy.id))?;
            }
        }
        for copy in &copies {
            if copy.distance && copy.id != lead.id {
                self.add_constraint(Constraint::EqualOffset {
                    a: lead_pair,
                    b: [copy.base, copy.id],
                })?;
            }
        }
        for tangent in tangents {
            self.add_constraint(tangent)?;
        }
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

/// The curves ending at each point, of `curves`, each an id and its ends
/// if it has them.
fn ending(curves: impl IntoIterator<Item = (Id, Option<[Id; 2]>)>) -> BTreeMap<Id, Vec<Id>> {
    let mut ending: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for (id, ends) in curves {
        for point in ends.into_iter().flatten() {
            ending.entry(point).or_default().push(id);
        }
    }
    ending
}

/// A chain of curves joined end to end, as offsetting takes it, see
/// [`Sketch::chained`].
#[derive(Debug, Clone, PartialEq)]
struct Chain {
    links: Vec<Link>,
    /// Whether the last ends where the first starts, or it's a circle.
    closed: bool,
}

impl Chain {
    /// The point where the piece `before` meets the one after, and where
    /// it is.
    fn corner(&self, sketch: &Sketch, before: usize) -> Option<(Id, DVec2)> {
        let link = &self.links[before];
        let [start, end] = sketch.curve(link.id)?.curve.ends()?;
        let point = if link.forward { end } else { start };
        Some((point, sketch.point(point)?.at))
    }

    /// Whether `point` is at least `distance` from every piece (less
    /// `tol`), and on `side` of the chain where it's nearest (see
    /// [`Chain::nearest`]).
    fn beyond(&self, point: DVec2, distance: f64, side: Side, tol: f64) -> bool {
        self.nearest(point, tol)
            .is_some_and(|(found, on)| found >= distance - tol && on.is_none_or(|on| on == side))
    }

    /// How far `point` is from the nearest of its pieces, and which side of
    /// the chain it's on there, the left of the way it runs
    /// [`Side::Positive`]: of the piece, or where it's nearest a corner
    /// (within `tol`), where two pieces meet, the side the corner points
    /// to (away from the way it turns), the only side a place can be
    /// nearest a corner from; past a corner turning back on itself, either
    /// (`None`). `None` without pieces.
    fn nearest(&self, point: DVec2, tol: f64) -> Option<(f64, Option<Side>)> {
        let (index, u, found) = self
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let (u, found) = nearest(&link.geom, point);
                (index, u, found)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        let count = self.links.len();
        let link = &self.links[index];
        let (into, out) = link.params();
        // The corner it's nearest, if it's nearest an end joined to
        // another piece: the pieces into it and out of it.
        let joined = |at: f64| link.geom.length(u, at) <= tol;
        let corner = if joined(out) && (index + 1 < count || self.closed) && count > 1 {
            Some((index, (index + 1) % count))
        } else if joined(into) && (index > 0 || self.closed) && count > 1 {
            Some(((index + count - 1) % count, index))
        } else {
            None
        };
        if let Some((before, after)) = corner {
            let way_in = self.links[before].heading_out();
            let way_out = self.links[after].heading_in();
            let turn = way_in.perp_dot(way_out);
            if turn.abs() > TANGENT {
                return Some((found, Some(Side::of(-turn))));
            }
            if way_in.dot(way_out) < 0.0 {
                return Some((found, None));
            }
        }
        let (heading, _) = link.geom.heading(u, link.forward);
        let side = Side::of(heading.perp_dot(point - link.geom.at(u)));
        Some((found, Some(side)))
    }
}

/// A curve of a chain, as the chain runs it: a line, a circle or an arc.
#[derive(Debug, Clone, PartialEq)]
struct Link {
    id: Id,
    /// Whether the chain runs it from its start to its end (an arc
    /// counter-clockwise, as a circle always is).
    forward: bool,
    geom: Geom,
}

impl Link {
    /// Its parameters where the chain comes in and goes out.
    fn params(&self) -> (f64, f64) {
        if self.forward {
            (0.0, self.geom.last())
        } else {
            (self.geom.last(), 0.0)
        }
    }

    /// Which way it runs where the chain comes in.
    fn heading_in(&self) -> DVec2 {
        self.geom.heading(self.params().0, self.forward).0
    }

    /// Which way it runs where the chain goes out.
    fn heading_out(&self) -> DVec2 {
        self.geom.heading(self.params().1, self.forward).0
    }

    /// Its copy `shift` to the left of the way it runs (right, below
    /// zero): a line along itself, run the same way; an arc or a circle
    /// about its centre. `None` where an arc's or a circle's copy would
    /// have no radius left, and for a spline, which isn't copied so.
    fn raw(&self, shift: f64) -> Option<Geom> {
        match self.geom {
            Geom::Spline(_) => None,
            Geom::Segment { .. } => {
                let (from, to) = self.params();
                let (from, to) = (self.geom.at(from), self.geom.at(to));
                let left = (to - from).normalize().perp();
                Some(Geom::Segment {
                    start: from + left * shift,
                    end: to + left * shift,
                })
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                // Left of counter-clockwise is inwards.
                let radius = if self.forward {
                    radius - shift
                } else {
                    radius + shift
                };
                (radius > 0.0 && radius.is_finite()).then_some(Geom::Round {
                    center,
                    radius,
                    begin,
                    sweep,
                })
            }
        }
    }

    /// The edges of the band `distance` round it: its copies either side,
    /// and circles round its ends. A copy with no radius left, within
    /// `tol` of its centre, is left out.
    fn band(&self, distance: f64, tol: f64) -> Vec<Geom> {
        let cap = |center| Geom::Round {
            center,
            radius: distance,
            begin: 0.0,
            sweep: TAU,
        };
        match self.geom {
            Geom::Spline(_) => Vec::new(),
            Geom::Segment { start, end } => {
                let left = (end - start).normalize().perp() * distance;
                vec![
                    Geom::Segment {
                        start: start + left,
                        end: end + left,
                    },
                    Geom::Segment {
                        start: start - left,
                        end: end - left,
                    },
                    cap(start),
                    cap(end),
                ]
            }
            Geom::Round {
                center,
                radius,
                begin,
                sweep,
            } => {
                let round = |radius| Geom::Round {
                    center,
                    radius,
                    begin,
                    sweep,
                };
                let mut edges = vec![round(radius + distance)];
                if radius - distance > tol {
                    edges.push(round(radius - distance));
                }
                if self.geom.has_ends() {
                    edges.extend([cap(self.geom.at(0.0)), cap(self.geom.at(sweep))]);
                }
                edges
            }
        }
    }
}

/// Makes the ends of parts (`ends`, by index into `places`) that are the
/// end of one part only, loose, one where two are within `slack` of each
/// other, the nearer pairs first, at the first's place, see [`SLIVER`].
/// `TooComplex` if comparing them takes `work` past its limit.
fn loose_ends(
    ends: &mut [Option<(usize, usize)>],
    places: &[DVec2],
    slack: f64,
    work: &mut Work,
) -> Result<(), EditError> {
    let mut uses = vec![0usize; places.len()];
    for &(from, to) in ends.iter().flatten() {
        uses[from] += 1;
        uses[to] += 1;
    }
    let loose: Vec<usize> = (0..places.len()).filter(|&at| uses[at] == 1).collect();
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (index, &a) in loose.iter().enumerate() {
        work.spend(loose.len() - index)?;
        for &b in &loose[index + 1..] {
            let apart = places[a].distance(places[b]);
            if apart <= slack {
                pairs.push((apart, a, b));
            }
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut into: Vec<usize> = (0..places.len()).collect();
    let mut joined = vec![false; places.len()];
    for (_, a, b) in pairs {
        if !joined[a] && !joined[b] {
            (joined[a], joined[b]) = (true, true);
            into[b] = a;
        }
    }
    for (from, to) in ends.iter_mut().flatten() {
        (*from, *to) = (into[*from], into[*to]);
    }
    Ok(())
}

/// The steps an offset has taken, refused past [`MAX_OFFSET_WORK`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Work(usize);

impl Work {
    /// Takes `steps` more: `TooComplex` past the limit.
    fn spend(&mut self, steps: usize) -> Result<(), EditError> {
        self.0 = self.0.saturating_add(steps);
        if self.0 > MAX_OFFSET_WORK {
            Err(EditError::TooComplex)
        } else {
            Ok(())
        }
    }
}

/// Ends `raws[i]` and starts `raws[j]`, copies of pieces a corner (or
/// pieces with no copy) apart, where they cross nearest where they end and
/// start, so that what's past it goes even where it's within the distance
/// by less than the tolerance, the corner barely turning. Only where what's
/// past on each is within `distance` of the pieces `past` (the one's past
/// the other's end, the other's before its start), or as good as: pieces
/// too short to cross by the corner are left to what's kept.
fn cross(raws: &mut [Raw], [i, j]: [usize; 2], past: [&[Link]; 2], distance: f64, tol: f64) {
    let (end, start) = (raws[i].end(), raws[j].start());
    let mut found = Vec::new();
    meet(&raws[i].geom, &raws[j].geom, tol, &mut found);
    let off =
        |(u, v): (f64, f64)| raws[i].geom.at(u).distance(end) + raws[j].geom.at(v).distance(start);
    let Some((u, v)) = found.into_iter().min_by(|&a, &b| off(a).total_cmp(&off(b))) else {
        return;
    };
    let within = |raw: &Raw, u: f64, at: f64, links: &[Link]| {
        let middle = raw.geom.at((u + at) / 2.0);
        let nearest = links.iter().map(|link| self::distance(&link.geom, middle));
        nearest.fold(f64::INFINITY, f64::min) <= distance + tol
    };
    if within(&raws[i], u, raws[i].end_param(), past[0])
        && within(&raws[j], v, raws[j].start_param(), past[1])
    {
        raws[i].trim_end(u);
        raws[j].trim_start(v);
    }
}

/// How far `point` is from `geom`, see [`nearest`].
fn distance(geom: &Geom, point: DVec2) -> f64 {
    nearest(geom, point).1
}

/// Where on `geom` is nearest `point` ([`Geom::closest`]), and how far.
fn nearest(geom: &Geom, point: DVec2) -> (f64, f64) {
    let u = geom.closest(point);
    (u, geom.at(u).distance(point))
}

/// How far `point` is from `geom` from `from` to `to` (increasing): from
/// the nearest place of it, an end's where that's past it.
fn span_distance(geom: &Geom, from: f64, to: f64, point: DVec2) -> f64 {
    let ends = geom
        .at(from)
        .distance(point)
        .min(geom.at(to).distance(point));
    let u = match *geom {
        Geom::Segment { start, end } => {
            let along = end - start;
            along.dot(point - start) / along.length_squared()
        }
        Geom::Round { center, begin, .. } => {
            let u = (angle::to_angle(point - center) - begin).rem_euclid(TAU);
            // A part round a circle may go on past a turn.
            if u < from { u + TAU } else { u }
        }
        Geom::Spline(_) => geom.closest(point),
    };
    if (from..=to).contains(&u) {
        geom.at(u).distance(point).min(ends)
    } else {
        ends
    }
}

/// Whether `point` is inside the corner `corner` (see
/// [`Sketch::raw_offset`]), a convex quadrilateral, by more than `tol`.
fn within(corner: &[DVec2; 4], point: DVec2, tol: f64) -> bool {
    let area: f64 = (0..4)
        .map(|i| corner[i].perp_dot(corner[(i + 1) % 4]))
        .sum();
    let sign = if area < 0.0 { -1.0 } else { 1.0 };
    (0..4).all(|i| {
        let (a, b) = (corner[i], corner[(i + 1) % 4]);
        let along = b - a;
        let length = along.length();
        length > 0.0 && sign * along.perp_dot(point - a) / length > tol
    })
}

/// A curve of the raw offset, before what falls within the distance goes:
/// a line, a circle or an arc.
#[derive(Debug, Clone, PartialEq)]
struct Raw {
    geom: Geom,
    /// Whether the chain runs it with its parameter: a line's always, an
    /// arc's counter-clockwise.
    forward: bool,
    source: Source,
}

impl Raw {
    /// The parameter where the chain comes in, and goes out.
    fn start_param(&self) -> f64 {
        if self.forward { 0.0 } else { self.geom.last() }
    }

    fn end_param(&self) -> f64 {
        if self.forward { self.geom.last() } else { 0.0 }
    }

    fn start(&self) -> DVec2 {
        self.geom.at(self.start_param())
    }

    fn end(&self) -> DVec2 {
        self.geom.at(self.end_param())
    }

    /// Ends it at its parameter `u`, where the chain runs out of it.
    fn trim_end(&mut self, u: f64) {
        match (&mut self.geom, self.forward) {
            (Geom::Segment { start, end }, _) => *end = start.lerp(*end, u),
            (Geom::Round { sweep, .. }, true) => *sweep = u,
            (Geom::Round { begin, sweep, .. }, false) => {
                *begin += u;
                *sweep -= u;
            }
            (Geom::Spline(_), _) => {}
        }
    }

    /// Starts it at its parameter `u`, where the chain runs into it.
    fn trim_start(&mut self, u: f64) {
        match (&mut self.geom, self.forward) {
            (Geom::Segment { start, end }, _) => *start = start.lerp(*end, u),
            (Geom::Round { begin, sweep, .. }, true) => {
                *begin += u;
                *sweep -= u;
            }
            (Geom::Round { sweep, .. }, false) => *sweep = u,
            (Geom::Spline(_), _) => {}
        }
    }

    /// Runs a line's copy on to end at `at`.
    fn set_end(&mut self, at: DVec2) {
        if let Geom::Segment { end, .. } = &mut self.geom {
            *end = at;
        }
    }

    /// Runs a line's copy on to start at `at`.
    fn set_start(&mut self, at: DVec2) {
        if let Geom::Segment { start, .. } = &mut self.geom {
            *start = at;
        }
    }
}

/// What a raw curve is the copy of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Source {
    /// The chain's piece of this index.
    Piece(usize),
    /// A round join about the point `corner`, taking whether it's
    /// construction from the piece `after`: the one before the corner, or
    /// for the round ends of an arc with no copy, the arc.
    Join { corner: Id, after: usize },
}

/// A part of a raw curve kept: from `from` to `to` along it, increasing,
/// or all of a circle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Kept {
    raw: usize,
    from: f64,
    to: f64,
    whole: bool,
}

/// The raw offset of a chain and what's kept of it.
#[derive(Debug, Clone, PartialEq)]
struct Offset {
    raws: Vec<Raw>,
    kept: Vec<Kept>,
    /// How near places are to be one.
    tol: f64,
    /// The steps taken working it out.
    work: Work,
}

/// A copy made, what it's a copy of, and which of its ties it keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Copy {
    id: Id,
    /// Its original: a line, arc or circle, or a round join's corner.
    base: Id,
    pair: OffsetPair,
    /// Whether it's tied by its offset: the lead's dimension, or equal to
    /// it.
    distance: bool,
    /// Whether a line's copy is held parallel to it.
    parallel: bool,
}

/// Which copies' ties the tangents between them stand for, see [`untie`].
struct Untied {
    /// The copy whose offset the dimension holds.
    lead: usize,
    /// The copy each tangent stands for a tie of, if any.
    assigned: Vec<Option<usize>>,
    /// The copies of arcs made about centres of their own.
    freed: Vec<bool>,
}

/// Which copy's ties each tangent between `copies` (`tangents`, by their
/// indices) stands for, and which copy leads, its offset held by the
/// dimension: a line's copy gives up its offset, then its parallel (the
/// lead its parallel alone), an arc's or a round join's its offset. The
/// first lead, lines' copies before arcs' before round joins', of the
/// first few, that lets every tangent stand for a tie, or failing any, the
/// one letting the most.
///
/// Round a loop of arcs alone, each tangent to the next, about centres
/// that stay where they are, the radii that make each tangent to the next
/// make the last tangent to the first too, so one of those tangents is
/// always left over, and without it the two arcs there meet where their
/// circles touch, which the solver can't place along them. So an arc's
/// copy tangent at both ends is made about a centre of its own there,
/// which the two tangents hold, standing for two more ties. A tangent
/// standing for none still is left out. `None` without copies.
fn untie(copies: &[Copy], tangents: &[[usize; 2]]) -> Option<Untied> {
    /// How many leads are tried at most, each a matching.
    const LEADS: usize = 8;
    let order = [OffsetPair::Lines, OffsetPair::Rounds, OffsetPair::Join];
    let leads = order.iter().flat_map(|&pair| {
        let of = copies.iter().enumerate();
        of.filter(move |(_, copy)| copy.pair == pair)
            .map(|(index, _)| index)
    });
    let mut best: Option<(usize, Vec<Option<usize>>, usize)> = None;
    for lead in leads.take(LEADS) {
        let capacity = |copy: usize| {
            let ties = if copies[copy].pair == OffsetPair::Lines {
                2
            } else {
                1
            };
            ties - usize::from(copy == lead)
        };
        let assigned = matching(copies.len(), tangents, capacity);
        let matched = assigned.iter().flatten().count();
        let full = matched == tangents.len();
        if best.as_ref().is_none_or(|(_, _, most)| matched > *most) {
            best = Some((lead, assigned, matched));
        }
        if full {
            break;
        }
    }
    let (lead, mut assigned, _) = best?;
    let mut freed = vec![false; copies.len()];
    let tangent_ends = |copy: usize| tangents.iter().filter(|ends| ends.contains(&copy)).count();
    loop {
        let unmatched = assigned.iter().position(Option::is_none);
        let free = unmatched.and_then(|tangent| {
            tangents[tangent].into_iter().find(|&copy| {
                copy != lead
                    && !freed[copy]
                    && copies[copy].pair == OffsetPair::Rounds
                    && tangent_ends(copy) == 2
            })
        });
        let Some(free) = free else {
            break;
        };
        freed[free] = true;
        let capacity = |copy: usize| {
            let ties = match copies[copy].pair {
                OffsetPair::Lines => 2,
                _ if freed[copy] => 3,
                _ => 1,
            };
            ties - usize::from(copy == lead)
        };
        assigned = matching(copies.len(), tangents, capacity);
    }
    Some(Untied {
        lead,
        assigned,
        freed,
    })
}

/// A matching of `tangents`, each between two of `count` copies, to the
/// copies, each taking at most `capacity` of them: as many as can be, each
/// tangent's copy or none. Augmenting paths found breadth first, so no
/// recursion however long the chain.
fn matching(
    count: usize,
    tangents: &[[usize; 2]],
    capacity: impl Fn(usize) -> usize,
) -> Vec<Option<usize>> {
    let mut assigned: Vec<Option<usize>> = vec![None; tangents.len()];
    let mut load = vec![0; count];
    let mut held: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (tangent, copies) in tangents.iter().enumerate() {
        // The tangent each copy was reached through, which would move to it.
        let mut came: Vec<Option<usize>> = vec![None; count];
        let mut queue = std::collections::VecDeque::new();
        for &copy in copies {
            if came[copy].is_none() {
                came[copy] = Some(tangent);
                queue.push_back(copy);
            }
        }
        let mut free = None;
        while let Some(copy) = queue.pop_front() {
            if load[copy] < capacity(copy) {
                free = Some(copy);
                break;
            }
            for &other in &held[copy] {
                for next in tangents[other] {
                    if next != copy && came[next].is_none() {
                        came[next] = Some(other);
                        queue.push_back(next);
                    }
                }
            }
        }
        // Moves each tangent along the path back to the new one.
        let mut at = free;
        while let Some(copy) = at {
            let Some(moved) = came[copy] else {
                break;
            };
            let from = assigned[moved].replace(copy);
            load[copy] += 1;
            held[copy].push(moved);
            if let Some(from) = from {
                load[from] -= 1;
                held[from].retain(|&t| t != moved);
            }
            at = from;
        }
    }
    assigned
}

mod spline;

#[cfg(test)]
mod tests;

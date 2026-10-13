//! The raw offset of a chain and what of it survives, pure geometry:
//! nothing here changes the sketch.
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

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use glam::DVec2;

use super::*;
use crate::angle;
use crate::bounds;
use crate::intersect::{Geom, meet, tolerance};
use crate::{EditError, Id, Side, Sketch, arc_sweep, crossing};

/// The band about a chain an offset of it by `distance` to `side` keeps
/// out of, places within `tol` one.
#[derive(Debug, Clone, Copy)]
pub(super) struct Band<'a> {
    pub(super) chain: &'a Chain,
    pub(super) distance: f64,
    pub(super) side: Side,
    pub(super) tol: f64,
}

impl Band<'_> {
    /// Whether `point` is at least the distance from every piece of the
    /// chain (less the tolerance), and on the side of it where it's
    /// nearest (see [`Chain::nearest`]).
    pub(super) fn beyond(&self, point: DVec2) -> bool {
        self.chain
            .nearest(point, self.tol)
            .is_some_and(|(found, on)| {
                found >= self.distance - self.tol && on.is_none_or(|on| on == self.side)
            })
    }
}

/// Makes the ends of parts (`ends`, by index into `places`) that are the
/// end of one part only, loose, one where two are within `slack` of each
/// other, the nearer pairs first, at the first's place, see [`SLIVER`].
/// `TooComplex` if comparing them takes `work` past its limit.
pub(super) fn loose_ends(
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
pub(super) struct Work(pub(super) usize);

impl Work {
    /// Takes `steps` more: `TooComplex` past the limit.
    pub(super) fn spend(&mut self, steps: usize) -> Result<(), EditError> {
        self.0 = self.0.saturating_add(steps);
        if self.0 > MAX_OFFSET_WORK {
            Err(EditError::TooComplex)
        } else {
            Ok(())
        }
    }
}

/// Where `geom`, in the box `bounds`, is cut by the curves `others`, each
/// with its box: where it meets them within `tol`, but where a circle
/// starts. `found` is room for the places met. Spends a step of `work`
/// per curve; `TooComplex` past the limit, or where splines lying along
/// each other run out of steps.
fn cuts_of<'g>(
    geom: &Geom,
    bounds: (DVec2, DVec2),
    others: impl Iterator<Item = (&'g Geom, (DVec2, DVec2))>,
    tol: f64,
    found: &mut Vec<(f64, f64)>,
    work: &mut Work,
) -> Result<Vec<f64>, EditError> {
    let mut cuts = Vec::new();
    for (other, other_bounds) in others {
        work.spend(1)?;
        if !bounds::overlap(bounds, other_bounds, tol) {
            continue;
        }
        found.clear();
        // Splines lying along each other run out of steps before
        // every place is found: too complex, not cut short.
        if meet(geom, other, tol, found) == usize::MAX {
            return Err(EditError::TooComplex);
        }
        // Where a circle starts is no end of it, and changes
        // nothing, but `meet` cuts there as at an end.
        let starts = |geom: &Geom, u: f64| !geom.has_ends() && u == 0.0;
        let cut = found
            .iter()
            .filter(|&&(u, v)| !starts(geom, u) && !starts(other, v));
        cuts.extend(cut.map(|&(u, _)| u));
    }
    Ok(cuts)
}

/// The spans of `geom` between its `cuts` (and its ends), but cuts at or
/// within `near` of its ends or of the cut before; round a circle, from
/// the last cut back to the first.
fn spans_between(geom: &Geom, mut cuts: Vec<f64>, near: f64) -> Vec<(f64, f64)> {
    let last = geom.last();
    // Cuts at its ends, or near them, cut nothing.
    cuts.retain(|&u| {
        u.is_finite()
            && (!geom.has_ends() || (geom.length(0.0, u) > near && geom.length(u, last) > near))
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
    if geom.has_ends() {
        places.windows(2).map(|pair| (pair[0], pair[1])).collect()
    } else if places.is_empty() {
        vec![(0.0, TAU)]
    } else {
        let mut spans: Vec<(f64, f64)> = places.windows(2).map(|pair| (pair[0], pair[1])).collect();
        let (first, end) = (places[0], places[places.len() - 1]);
        spans.push((end, first + TAU));
        spans
    }
}

/// The spans of `geom` that `keeps`, those that follow on from each
/// other with nothing cut out joined into one, and round a circle the
/// last into the first.
fn merge_kept(
    geom: &Geom,
    spans: Vec<(f64, f64)>,
    keeps: impl Fn((f64, f64)) -> bool,
) -> Vec<(f64, f64)> {
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
    parts
}

/// `kept` less the parts lying on another raw's part, within `tol`: both
/// go. Spends `kept.len()` steps of `work` a part.
fn drop_lying(
    raws: &[Raw],
    kept: Vec<Kept>,
    tol: f64,
    work: &mut Work,
) -> Result<Vec<Kept>, EditError> {
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

/// Each piece of `chain`'s raw copy shifted by `shift`, and the index
/// among them of each piece's, `None` where a piece has none.
fn piece_copies(chain: &Chain, shift: f64) -> (Vec<Raw>, Vec<Option<usize>>) {
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
    (raws, of_piece)
}

/// The copies in `raws` of pieces of `chain` either side of pieces with
/// no copy (`of_piece`), crossed where they cross past them, as copies at
/// a corner do.
fn cross_gaps(
    chain: &Chain,
    raws: &mut [Raw],
    of_piece: &[Option<usize>],
    distance: f64,
    tol: f64,
) {
    let count = chain.links.len();
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
        cross(raws, [i, j], [&from, &to], distance, tol);
    }
}

/// Ends `raws[i]` and starts `raws[j]`, copies of pieces a corner (or
/// pieces with no copy) apart, where they cross nearest where they end and
/// start, so that what's past it goes even where it's within the distance
/// by less than the tolerance, the corner barely turning. Only where what's
/// past on each is within `distance` of the pieces `past` (the one's past
/// the other's end, the other's before its start), or as good as: pieces
/// too short to cross by the corner are left to what's kept.
pub(super) fn cross(
    raws: &mut [Raw],
    [i, j]: [usize; 2],
    past: [&[Link]; 2],
    distance: f64,
    tol: f64,
) {
    let (end, start) = (raws[i].end(), raws[j].start());
    let mut found = Vec::new();
    // Where not every place is found, crossing by the nearest found may
    // be wrong: left as too short to cross, the two meet again when cut
    // where copies cross, and are refused there.
    if meet(&raws[i].geom, &raws[j].geom, tol, &mut found) == usize::MAX {
        return;
    }
    let off =
        |(u, v): (f64, f64)| raws[i].geom.at(u).distance(end) + raws[j].geom.at(v).distance(start);
    let Some((u, v)) = found.into_iter().min_by(|&a, &b| off(a).total_cmp(&off(b))) else {
        return;
    };
    let within = |raw: &Raw, u: f64, at: f64, links: &[Link]| {
        let middle = raw.geom.at((u + at) / 2.0);
        let nearest = links.iter().map(|link| link.geom.distance_to(middle));
        nearest.fold(f64::INFINITY, f64::min) <= distance + tol
    };
    if within(&raws[i], u, raws[i].params().1, past[0])
        && within(&raws[j], v, raws[j].params().0, past[1])
    {
        raws[i].trim_end(u);
        raws[j].trim_start(v);
    }
}

/// How far `point` is from `geom` from `from` to `to` (increasing): from
/// the nearest place of it, an end's where that's past it.
fn span_distance(geom: &Geom, from: f64, to: f64, point: DVec2) -> f64 {
    let ends = geom
        .at(from)
        .distance(point)
        .min(geom.at(to).distance(point));
    let u = geom.carrier_param(point);
    // A part round a circle may go on past a turn.
    let u = match geom {
        Geom::Round { .. } if u < from => u + TAU,
        _ => u,
    };
    if (from..=to).contains(&u) {
        geom.at(u).distance(point).min(ends)
    } else {
        ends
    }
}

/// Whether `point` is inside the corner `corner` (see
/// [`Sketch::raw_offset`]), a convex quadrilateral, by more than `tol`.
pub(super) fn within(corner: &[DVec2; 4], point: DVec2, tol: f64) -> bool {
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
pub(super) struct Raw {
    pub(super) geom: Geom,
    /// Whether the chain runs it with its parameter: a line's always, an
    /// arc's counter-clockwise.
    pub(super) forward: bool,
    pub(super) source: Source,
}

impl Run for Raw {
    fn geom(&self) -> &Geom {
        &self.geom
    }

    fn forward(&self) -> bool {
        self.forward
    }
}

impl Raw {
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
pub(super) enum Source {
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
pub(super) struct Kept {
    pub(super) raw: usize,
    pub(super) from: f64,
    pub(super) to: f64,
    pub(super) whole: bool,
}

/// The raw offset of a chain and what's kept of it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Offset {
    pub(super) raws: Vec<Raw>,
    pub(super) kept: Vec<Kept>,
    /// How near places are to be one.
    pub(super) tol: f64,
    /// The steps taken working it out.
    pub(super) work: Work,
}

impl Sketch {
    /// The offset of `chain` by `distance` to `side`: its raw curves and
    /// the parts of them kept. `NothingLeft` if none are.
    pub(super) fn raw_offset(
        &self,
        chain: &Chain,
        distance: f64,
        side: Side,
    ) -> Result<Offset, EditError> {
        let shift = offset_shift(distance, side)?;
        let (mut raws, of_piece) = piece_copies(chain, shift);
        // Places are one within the tolerance of the sketch and the copy
        // together.
        let geoms: Vec<Geom> = self.geoms().collect();
        let tol = tolerance(geoms.iter().chain(raws.iter().map(|raw| &raw.geom)));
        let corners = self.join_corners(chain, &mut raws, &of_piece, shift, distance, tol);
        cross_gaps(chain, &mut raws, &of_piece, distance, tol);
        self.round_vanished(chain, &mut raws, &of_piece, distance);
        let mut work = Work::default();
        let band = Band {
            chain,
            distance,
            side,
            tol,
        };
        let kept = self.kept(&band, &raws, &corners, &mut work)?;
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

    /// At each corner of `chain` between two pieces' copies in `raws`
    /// (`of_piece`) that don't meet, the copies crossed, mitred or joined
    /// by a round of `distance` pushed to `raws`. The corners run on to
    /// meet: each the corner of the chain, where the one copy ended, where
    /// they meet and where the other started.
    fn join_corners(
        &self,
        chain: &Chain,
        raws: &mut Vec<Raw>,
        of_piece: &[Option<usize>],
        shift: f64,
        distance: f64,
        tol: f64,
    ) -> Vec<[DVec2; 4]> {
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
                    raws,
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
        corners
    }

    /// Full circles of `distance` pushed to `raws` round the ends of the
    /// pieces of `chain` with no copy (`of_piece`) that another piece
    /// meets.
    fn round_vanished(
        &self,
        chain: &Chain,
        raws: &mut Vec<Raw>,
        of_piece: &[Option<usize>],
        distance: f64,
    ) {
        let count = chain.links.len();
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
                        geom: Geom::circle(at, distance),
                        forward: true,
                        source: Source::Join {
                            corner,
                            after: index,
                        },
                    });
                }
            }
        }
    }

    /// The parts of `raws` beyond `band`, each as long as it can be, see
    /// the module's docs.
    pub(super) fn kept(
        &self,
        band: &Band,
        raws: &[Raw],
        corners: &[[DVec2; 4]],
        work: &mut Work,
    ) -> Result<Vec<Kept>, EditError> {
        let Band {
            chain,
            distance,
            tol,
            ..
        } = *band;
        // The band's edges round each piece.
        let mut edges = Vec::new();
        for link in &chain.links {
            edges.extend(link.band(distance, tol));
        }
        let bounds: Vec<(DVec2, DVec2)> = raws.iter().map(|raw| raw.geom.bounds()).collect();
        let edge_bounds: Vec<(DVec2, DVec2)> = edges.iter().map(Geom::bounds).collect();
        let mut kept = Vec::new();
        let mut found = Vec::new();
        for (index, raw) in raws.iter().enumerate() {
            let geom = &raw.geom;
            let others = raws
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .map(|(other, raw)| (&raw.geom, bounds[other]))
                .chain(edges.iter().zip(edge_bounds.iter().copied()));
            let cuts = cuts_of(geom, bounds[index], others, tol, &mut found, work)?;
            let near = NEAR * tol;
            let spans = spans_between(geom, cuts, near);
            // Nearer a piece than the distance, it's within the band, and
            // inside a corner run on to meet it's within the copy too.
            // And on the other side of the chain, it's no part of this
            // side's copy.
            let keeps = |(from, to): (f64, f64)| {
                let middle = geom.at((from + to) / 2.0);
                geom.length(from, to) > near
                    && band.beyond(middle)
                    && !corners.iter().any(|corner| within(corner, middle, tol))
            };
            work.spend(spans.len().saturating_mul(chain.links.len()))?;
            let parts = merge_kept(geom, spans, keeps);
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
        drop_lying(raws, kept, tol, work)
    }
}

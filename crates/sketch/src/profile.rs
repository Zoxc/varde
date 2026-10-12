//! Profiles: the regions a sketch's curves enclose, each an outer loop and
//! its holes, which extrude and its kind will take, and the near misses
//! where open ends almost meet.
//!
//! The curves that aren't construction are cut where they meet
//! ([`meet`]) into pieces, whose ends within a tolerance of the sketch's
//! size are one vertex. Pieces with an end nothing else reaches are
//! dangling, and taken away until none are. What's left is a planar
//! graph: the faces are traced round it with each face on the left, so a
//! face's outer loop runs counter-clockwise and the boundary of a
//! connected part, seen from outside, clockwise. A clockwise boundary is a
//! hole in the smallest face of another part it's inside, or, inside none,
//! bounds nothing. Every face is a region: a plate with bolt holes is the
//! plate with its holes, and each hole's inside is a region of its own,
//! as is an island in a hole.
//!
//! Every count is bounded ([`MAX_SPLITS`], [`MAX_WORK`]): a sketch past
//! them is [`TooComplex`] rather than a long wait.
//!
//! Profiles don't depend on the view, so they're worked out once per
//! sketch; near misses, which do (the gap is a few pixels), are paired
//! from the open ends as asked ([`Profiles::near_misses`]).

use std::fmt;

use glam::DVec2;

use crate::intersect::{Geom, meet, tolerance};
use crate::sets::Sets;
use crate::spline::bezier;
use crate::{Curve, Id, Sketch};

mod graph;
mod merge;
mod reference;

pub use merge::MergeError;
pub use reference::{MAX_REGION_CURVES, RegionRef, RegionRefError};

use graph::{Graph, pinches};

/// The most places where curves are cut: their ends and where they meet.
/// A sketch people draw has a few per curve; a file could have every line
/// cross every other, millions.
pub const MAX_SPLITS: usize = 100_000;

/// The most steps of work finding profiles may take: pairs of curves
/// whose boxes are compared, places compared to merge them, steps round
/// faces, and faces a point is tested against, each one; what costs
/// more by its share, about the time of one such step (a nanosecond or
/// two): where two curves meet, a spline's segments' boxes compared and
/// pieces subdivided, a winding number's chords, and each cut what
/// follows from it (pieces, faces). About a tenth of a second, whatever
/// the sketch: a few times what dense sketches people draw take
/// (hundreds of letters' outlines, a dozen or two splines crossing each
/// other), as refusing one costs more than a short wait. Besides it,
/// what's linear in the sketch's size (its splines' shapes, bounded by
/// [`MAX_POINTS`](crate::MAX_POINTS)).
pub const MAX_WORK: usize = 60_000_000;

/// What each cut adds to the work, in its unit (see [`MAX_WORK`]): the
/// pieces and vertices it makes, sorting them round, the walks round
/// faces, their areas, boxes and polylines, and the regions' pieces,
/// about a microsecond or two a cut, besides the few steps counted as
/// they're taken. Every cut [`MAX_SPLITS`] allows fits in the work.
pub(crate) const CUT_COST: usize = 512;

const _: () = assert!(MAX_SPLITS * CUT_COST < MAX_WORK);

/// The most near misses reported: enough to find them, however large the
/// gap asked for is.
pub const MAX_NEAR_MISSES: usize = 1_000;

/// The most pairs of open ends [`Profiles::near_misses`] compares, for a
/// gap so large that most are within it: a few milliseconds.
pub const MAX_NEAR_PAIRS: usize = 1_000_000;

/// Part of a curve, exact: the curve, and the parameters it runs from and
/// to, backwards along the curve when `to` is below `from`. A line's
/// parameter runs from 0 at its start to 1 at its end; a circle's or an
/// arc's is the angle in radians turned counter-clockwise from its start's
/// direction (a circle's start is to the right of its centre), from 0 to
/// its sweep, 2π for a circle; a spline's from 0 at its start (a closed
/// one's first point) to 1 at its end (see
/// [`BSpline`](crate::BSpline)).
///
/// `start` and `end` are the vertices it runs between, indices into
/// [`Profiles::vertices`]: pieces meeting there share them, where their
/// curves' own places meet only within the tolerance. Loops join up by
/// them exactly, and whoever builds exact geometry from pieces puts the
/// ends there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub curve: Id,
    pub from: f64,
    pub to: f64,
    pub start: usize,
    pub end: usize,
}

/// A region of the sketch: what an outer loop encloses, less its holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Counter-clockwise, each piece starting where the one before ends
    /// and the last ending where the first starts.
    pub outer: Vec<Piece>,
    /// Each clockwise, joined up the same way.
    pub holes: Vec<Vec<Piece>>,
    /// The outer loop then the holes as polylines, for drawing and
    /// picking, each closing from its last point back to its first.
    pub outline: Vec<Vec<DVec2>>,
    /// The area enclosed, less the holes'.
    pub area: f64,
    /// The box the outer loop lies in, and so its polyline.
    pub bounds: (DVec2, DVec2),
}

/// An end of a piece no other piece reaches: where a dangling line or an
/// open chain ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenEnd {
    pub at: DVec2,
    /// Where the piece it ends starts from, the other way.
    pub from: DVec2,
}

/// Two open ends, `a` and `b`, closer than the gap asked for: a loop that
/// almost closes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearMiss {
    pub a: DVec2,
    pub b: DVec2,
}

/// A sketch's profiles, see [`Sketch::profiles`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profiles {
    pub regions: Vec<Region>,
    /// Where the pieces' ends are, by [`Piece::start`] and [`Piece::end`]:
    /// the places curves were cut, merged within the tolerance, each a
    /// curve's end's place if one is among them.
    pub vertices: Vec<DVec2>,
    /// Sorted by `x`, for [`Profiles::near_misses`].
    pub open_ends: Vec<OpenEnd>,
}

/// The sketch's curves cross too often, or too many are close together,
/// for profiles to be found in bounded time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooComplex;

impl fmt::Display for TooComplex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("too complex to find profiles")
    }
}

impl std::error::Error for TooComplex {}

impl Profiles {
    /// Pairs of open ends within `gap` of each other (in sketch units: a
    /// few pixels' worth, which is the caller's to say), but not the two
    /// ends of one piece: at most [`MAX_NEAR_MISSES`], from at most
    /// [`MAX_NEAR_PAIRS`] pairs compared.
    pub fn near_misses(&self, gap: f64) -> Vec<NearMiss> {
        let mut near = Vec::new();
        if gap.is_nan() || gap < 0.0 {
            return near;
        }
        let mut pairs = 0;
        for (k, a) in self.open_ends.iter().enumerate() {
            for b in &self.open_ends[k + 1..] {
                if b.at.x - a.at.x > gap {
                    break;
                }
                pairs += 1;
                if pairs > MAX_NEAR_PAIRS {
                    return near;
                }
                let one_piece = a.from == b.at && b.from == a.at;
                if a.at.distance(b.at) <= gap && !one_piece {
                    near.push(NearMiss { a: a.at, b: b.at });
                    if near.len() >= MAX_NEAR_MISSES {
                        return near;
                    }
                }
            }
        }
        near
    }

    /// The region `point` is in, by the even-odd rule on its outline, as
    /// it's drawn: inside its outer loop and none of its holes.
    pub fn region_at(&self, point: DVec2) -> Option<usize> {
        self.regions
            .iter()
            .position(|region| region.contains(point))
    }
}

impl Region {
    /// Whether `point` is in the region by the even-odd rule on its
    /// outline, as it's drawn.
    fn contains(&self, point: DVec2) -> bool {
        let (min, max) = self.bounds;
        point.cmpge(min).all()
            && point.cmple(max).all()
            && self
                .outline
                .iter()
                .filter(|polyline| crosses_odd(polyline, point))
                .count()
                % 2
                == 1
    }
}

/// Whether a ray from `point` to the right crosses the closed `polyline`
/// an odd number of times.
fn crosses_odd(polyline: &[DVec2], point: DVec2) -> bool {
    closed_edges(polyline)
        .filter_map(|(before, at)| crossing_x(before, at, point.y))
        .filter(|&x| point.x < x)
        .count()
        % 2
        == 1
}

/// The edges of the closed `polyline`, each from a point to the next,
/// the first from its last point.
fn closed_edges(polyline: &[DVec2]) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
    let before = polyline.last().into_iter().chain(polyline);
    before.copied().zip(polyline.iter().copied())
}

/// Where the edge from `before` to `at` crosses the horizontal line at
/// `y`, if it does: one end above it, the other not.
fn crossing_x(before: DVec2, at: DVec2, y: f64) -> Option<f64> {
    ((at.y > y) != (before.y > y))
        .then(|| before.x + (y - before.y) / (at.y - before.y) * (at.x - before.x))
}

impl Sketch {
    /// The regions the curves that aren't construction enclose, and the
    /// open ends, see the module's docs. Regions are in no particular
    /// order, but always the same one for the same sketch.
    pub fn profiles(&self) -> Result<Profiles, TooComplex> {
        self.profiles_counting(MAX_SPLITS, &mut Work(MAX_WORK))
    }

    /// [`Sketch::profiles`] within what's `left` of a budget shared with
    /// others, as well as [`MAX_WORK`], the work done taken from `left`
    /// (all it was allowed, if too complex): for a caller finding the
    /// profiles of many sketches at once.
    pub fn profiles_spending(&self, left: &mut usize) -> Result<Profiles, TooComplex> {
        let allowed = MAX_WORK.min(*left);
        let mut work = Work(allowed);
        let found = self.profiles_counting(MAX_SPLITS, &mut work);
        // Too complex, it took all it may have.
        let spent = if found.is_ok() {
            allowed - work.0
        } else {
            allowed
        };
        *left = left.saturating_sub(spent);
        found
    }

    /// [`Sketch::profiles`] within `most_splits` cuts ([`MAX_SPLITS`] but in
    /// tests), the work spent from `work`.
    fn profiles_counting(
        &self,
        most_splits: usize,
        work: &mut Work,
    ) -> Result<Profiles, TooComplex> {
        let mut curves: Vec<(Id, Geom)> = self
            .curves
            .iter()
            .filter(|entry| !entry.construction)
            .filter_map(|entry| Some((entry.id, Geom::of(self, &entry.curve)?)))
            .collect();
        let tolerance = tolerance(curves.iter().map(|(_, geom)| geom));
        // An arc whose end is off the circle its start is on, as it is
        // until the solver holds its radii equal, is drawn with the radius
        // changing along it: it bounds nothing rather than a region that
        // isn't the one drawn. A circle or an arc within the tolerance of
        // its centre is a point, as a line within it of its start is.
        curves.retain(|(id, geom)| {
            let id = *id;
            if let Geom::Round { radius, .. } = *geom
                && radius <= tolerance
            {
                return false;
            }
            match self.curve(id).map(|entry| &entry.curve) {
                Some(&Curve::Arc { end, .. }) => self
                    .point(end)
                    .is_some_and(|end| end.at.distance(geom.at(geom.last())) <= tolerance),
                _ => true,
            }
        });
        // What fillets and chamfers leave of the lines they cut back: the
        // ends cut off are no part of a profile, the fillet or chamfer
        // taking their place.
        let cut_back = self.cut_back();
        curves.retain(|(id, _)| cut_back.get(id).is_none_or(|[from, to]| from < to));
        let kept: Vec<[f64; 2]> = curves
            .iter()
            .map(|(id, geom)| cut_back.get(id).copied().unwrap_or([0.0, geom.last()]))
            .collect();
        let splits = splits(&curves, &kept, tolerance, most_splits, work)?;
        work.spend(splits.len().saturating_mul(CUT_COST))?;
        let (vertex_of, vertices) = merge(&splits, tolerance, work)?;
        let edges = edges(&curves, &kept, &splits, &vertex_of, tolerance, work)?;
        let graph = Graph::new(&curves, vertices, edges, tolerance);
        let open_ends = graph.open_ends();
        let vertices = graph.vertices.clone();
        let regions = graph.regions(work)?;
        Ok(Profiles {
            regions,
            vertices,
            open_ends,
        })
    }
}

/// Steps of work left, see [`MAX_WORK`].
struct Work(usize);

impl Work {
    fn spend(&mut self, steps: usize) -> Result<(), TooComplex> {
        self.0 = self.0.checked_sub(steps).ok_or(TooComplex)?;
        Ok(())
    }
}

/// A place a curve is cut: the curve (an index into the curves), the
/// parameter there, the place, and whether it's the curve's end, whose
/// place a merged vertex takes before others'.
struct Split {
    curve: usize,
    u: f64,
    at: DVec2,
    end: bool,
}

/// Every place a curve is cut, at `most`: [`Geom::cuts`], where what's
/// `kept` of it starts and ends, where a spline crosses itself, and where
/// it meets another, found by sweeping the curves' boxes along x.
fn splits(
    curves: &[(Id, Geom)],
    kept: &[[f64; 2]],
    tolerance: f64,
    most: usize,
    work: &mut Work,
) -> Result<Vec<Split>, TooComplex> {
    let mut splits = Vec::new();
    let push = |splits: &mut Vec<Split>, curve: usize, u: f64, end: bool| {
        if splits.len() >= most {
            return Err(TooComplex);
        }
        let at = curves[curve].1.at(u);
        splits.push(Split { curve, u, at, end });
        Ok(())
    };
    for (index, (_, geom)) in curves.iter().enumerate() {
        for u in geom.cuts() {
            push(&mut splits, index, u, geom.has_ends())?;
        }
        for u in kept[index] {
            if !geom.cuts().any(|cut| cut == u) {
                push(&mut splits, index, u, false)?;
            }
        }
        if let Geom::Spline(path) = geom {
            let mut found = Vec::new();
            work.spend(bezier::self_crossings(path, tolerance, &mut found))?;
            for (u, v) in found {
                for u in [u, v] {
                    push(&mut splits, index, u, false)?;
                }
            }
        }
    }
    let boxes: Vec<_> = curves
        .iter()
        .map(|(_, geom)| {
            let (min, max) = geom.bounds();
            (min - tolerance, max + tolerance)
        })
        .collect();
    let mut order: Vec<usize> = (0..curves.len()).collect();
    order.sort_by(|&a, &b| boxes[a].0.x.total_cmp(&boxes[b].0.x));
    let mut found = Vec::new();
    for (k, &a) in order.iter().enumerate() {
        let (min, max) = boxes[a];
        for &b in &order[k + 1..] {
            let (other_min, other_max) = boxes[b];
            if other_min.x > max.x {
                break;
            }
            work.spend(1)?;
            if other_min.y > max.y || other_max.y < min.y {
                continue;
            }
            found.clear();
            work.spend(meet(&curves[a].1, &curves[b].1, tolerance, &mut found))?;
            found.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
            found.dedup();
            for &(ua, ub) in &found {
                // A cut's place is already there, as the curve's own; lines
                // meeting at a corner add nothing.
                for (curve, u) in [(a, ua), (b, ub)] {
                    if !curves[curve].1.cuts().any(|cut| cut == u) {
                        push(&mut splits, curve, u, false)?;
                    }
                }
            }
        }
    }
    Ok(splits)
}

/// Merges splits within `tolerance` of each other (and so on, from one to
/// the next) into vertices: each split's vertex, and where each vertex is,
/// a curve's end's place if one is in it. Splits are found near each
/// other by the cells `tolerance` wide they're in, sorted.
fn merge(
    splits: &[Split],
    tolerance: f64,
    work: &mut Work,
) -> Result<(Vec<usize>, Vec<DVec2>), TooComplex> {
    let mut sets = Sets::new(splits.len());
    // Coordinates are within a size of zero, the size a billion
    // tolerances, so cells are numbered well within `i64`.
    let cell = |at: DVec2| {
        let cell = (at / tolerance).floor();
        (cell.x as i64, cell.y as i64)
    };
    let mut sorted: Vec<((i64, i64), usize)> = splits
        .iter()
        .enumerate()
        .map(|(i, split)| (cell(split.at), i))
        .collect();
    sorted.sort_unstable();
    // Each pair once: along the row from each split on to the cell past
    // it, and in the next row the three cells beside, whose run moves on
    // through the order as the split does.
    let (mut low, mut high) = (0, 0);
    for (k, &((x, y), i)) in sorted.iter().enumerate() {
        let next = x.saturating_add(1);
        while low < sorted.len() && sorted[low].0 < (next, y.saturating_sub(1)) {
            low += 1;
        }
        high = high.max(low);
        while high < sorted.len() && sorted[high].0 <= (next, y.saturating_add(1)) {
            high += 1;
        }
        let along = sorted[k + 1..]
            .iter()
            .take_while(|&&(cell, _)| cell <= (x, y.saturating_add(1)));
        for &(_, j) in along.chain(&sorted[low..high]) {
            work.spend(1)?;
            if splits[i].at.distance(splits[j].at) <= tolerance {
                sets.join(i, j);
            }
        }
    }
    let mut vertex_of = vec![usize::MAX; splits.len()];
    let mut vertices: Vec<DVec2> = Vec::new();
    let mut taken_end = Vec::new();
    for i in 0..splits.len() {
        // A set's root is its lowest index, so it's had its vertex.
        let r = sets.root(i);
        if vertex_of[r] == usize::MAX {
            vertex_of[r] = vertices.len();
            vertices.push(splits[r].at);
            taken_end.push(splits[r].end);
        }
        let vertex = vertex_of[r];
        vertex_of[i] = vertex;
        if splits[i].end && !taken_end[vertex] {
            vertices[vertex] = splits[i].at;
            taken_end[vertex] = true;
        }
    }
    Ok((vertex_of, vertices))
}

/// A piece between two vertices: the curve (an index), its parameters
/// from `start` to `end`, increasing.
#[derive(Debug, Clone, Copy)]
struct Edge {
    curve: usize,
    from: f64,
    to: f64,
    start: usize,
    end: usize,
}

/// The pieces the splits cut the curves into, each once: pieces with
/// both ends at one vertex are left out unless they go round
/// ([`Geom::goes_round`]: a circle cut once, a closed spline, a spline's
/// loop), as are those outside what's `kept` of
/// their curve, and of pieces joining the same vertices through the same
/// place (lines on one line, arcs on one circle, overlapping) only the
/// first is kept.
fn edges(
    curves: &[(Id, Geom)],
    kept: &[[f64; 2]],
    splits: &[Split],
    vertex_of: &[usize],
    tolerance: f64,
    work: &mut Work,
) -> Result<Vec<Edge>, TooComplex> {
    let mut order: Vec<usize> = (0..splits.len()).collect();
    order.sort_by(|&a, &b| {
        (splits[a].curve.cmp(&splits[b].curve)).then(splits[a].u.total_cmp(&splits[b].u))
    });
    let mut edges = Vec::new();
    let add = |edge: Edge, edges: &mut Vec<Edge>| {
        let geom = &curves[edge.curve].1;
        let round = geom.goes_round(edge.from, edge.to, tolerance);
        let [from, to] = kept[edge.curve];
        let middle = (edge.from + edge.to) / 2.0;
        if (edge.start != edge.end || round) && from <= middle && middle <= to {
            edges.push(edge);
        }
    };
    for group in order.chunk_by(|&a, &b| splits[a].curve == splits[b].curve) {
        let curve = splits[group[0]].curve;
        for pair in group.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let edge = Edge {
                curve,
                from: splits[a].u,
                to: splits[b].u,
                start: vertex_of[a],
                end: vertex_of[b],
            };
            add(edge, &mut edges);
        }
        let geom = &curves[curve].1;
        if !geom.has_ends() {
            // Round a circle from its last cut to its start, which is
            // always cut.
            let last = group[group.len() - 1];
            let edge = Edge {
                curve,
                from: splits[last].u,
                to: geom.last(),
                start: vertex_of[last],
                end: vertex_of[group[0]],
            };
            add(edge, &mut edges);
        }
    }
    // Of those joining the same vertices through the same middle, the
    // first.
    let joins = |edge: &Edge| (edge.start.min(edge.end), edge.start.max(edge.end));
    let middle = |edge: &Edge| curves[edge.curve].1.at((edge.from + edge.to) / 2.0);
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by_key(|&e| (joins(&edges[e]), e));
    let mut kept = vec![true; edges.len()];
    for same in order.chunk_by(|&a, &b| joins(&edges[a]) == joins(&edges[b])) {
        for (k, &a) in same.iter().enumerate() {
            if !kept[a] {
                continue;
            }
            work.spend(same.len() - k)?;
            let at = middle(&edges[a]);
            for &b in &same[k + 1..] {
                if middle(&edges[b]).distance(at) <= tolerance {
                    kept[b] = false;
                }
            }
        }
    }
    let mut kept = kept.into_iter();
    edges.retain(|_| kept.next().unwrap_or(false));
    Ok(edges)
}

#[cfg(test)]
mod tests;

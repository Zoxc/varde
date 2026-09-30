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

use std::f64::consts::{PI, TAU};
use std::fmt;

use glam::DVec2;

use crate::angle;
use crate::intersect::{Geom, meet, tolerance};
use crate::sets::Sets;
use crate::spline::bezier;
use crate::{Curve, Id, Sketch};

mod merge;
mod reference;

pub use merge::MergeError;
pub use reference::{MAX_REGION_CURVES, RegionRef, RegionRefError};

/// The most places where curves are cut: their ends and where they meet.
/// A sketch people draw has a few per curve; a file could have every line
/// cross every other, millions.
pub const MAX_SPLITS: usize = 100_000;

/// The most steps of work finding profiles may take: pairs of curves
/// whose boxes are compared, places compared to merge them, steps round
/// faces, and pieces a point is tested against. Tens of milliseconds.
pub const MAX_WORK: usize = 20_000_000;

/// The most near misses reported: enough to find them, however large the
/// gap asked for is.
pub const MAX_NEAR_MISSES: usize = 1_000;

/// The most pairs of open ends [`Profiles::near_misses`] compares, for a
/// gap so large that most are within it: a few milliseconds.
pub const MAX_NEAR_PAIRS: usize = 1_000_000;

/// Directions leaving a vertex within this many radians of each other are
/// told apart by where they are a little further on ([`sort_around`]).
const SAME_DIRECTION: f64 = 1e-6;

/// Places within this many tolerances of a vertex sideways are on it
/// ([`sort_around`]): a unit or two in the last place of the sketch's
/// size, of which the tolerance is 10⁻⁹, as far apart as rounding puts
/// two curves' places where they cross. Found by trying: much more and
/// curves merged into a vertex a hair to one side are taken to pass
/// through it, much less and rounding sorts barely crossing ones.
const ROUNDING: f64 = 2e-7;

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
    let mut odd = false;
    let mut before = match polyline.last() {
        Some(&last) => last,
        None => return false,
    };
    for &at in polyline {
        if (at.y > point.y) != (before.y > point.y) {
            let x = before.x + (point.y - before.y) / (at.y - before.y) * (at.x - before.x);
            if point.x < x {
                odd = !odd;
            }
        }
        before = at;
    }
    odd
}

impl Sketch {
    /// The regions the curves that aren't construction enclose, and the
    /// open ends, see the module's docs. Regions are in no particular
    /// order, but always the same one for the same sketch.
    pub fn profiles(&self) -> Result<Profiles, TooComplex> {
        self.profiles_within(&LIMITS)
    }

    /// [`Sketch::profiles`] within `limits`.
    fn profiles_within(&self, limits: &Limits) -> Result<Profiles, TooComplex> {
        let mut work = Work(limits.work);
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
        let splits = splits(&curves, &kept, tolerance, limits.splits, &mut work)?;
        let (vertex_of, vertices) = merge(&splits, tolerance, &mut work)?;
        let edges = edges(&curves, &kept, &splits, &vertex_of, tolerance, &mut work)?;
        let graph = Graph::new(&curves, vertices, edges, tolerance);
        let open_ends = graph.open_ends();
        let vertices = graph.vertices.clone();
        let regions = graph.regions(&mut work)?;
        Ok(Profiles {
            regions,
            vertices,
            open_ends,
        })
    }
}

/// How much finding profiles may do: [`MAX_SPLITS`] and [`MAX_WORK`],
/// less in tests.
struct Limits {
    splits: usize,
    work: usize,
}

const LIMITS: Limits = Limits {
    splits: MAX_SPLITS,
    work: MAX_WORK,
};

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

/// The pieces as a graph: half-edge `2 e` runs along edge `e` from its
/// start to its end, `2 e + 1` back.
struct Graph<'c> {
    curves: &'c [(Id, Geom)],
    vertices: Vec<DVec2>,
    edges: Vec<Edge>,
    /// Each vertex's edges' count of ends there, a piece going round
    /// counting twice.
    degree: Vec<usize>,
    /// What places were merged within.
    tolerance: f64,
}

impl<'c> Graph<'c> {
    fn new(
        curves: &'c [(Id, Geom)],
        vertices: Vec<DVec2>,
        edges: Vec<Edge>,
        tolerance: f64,
    ) -> Graph<'c> {
        let mut degree = vec![0; vertices.len()];
        for edge in &edges {
            degree[edge.start] += 1;
            degree[edge.end] += 1;
        }
        Graph {
            curves,
            vertices,
            edges,
            degree,
            tolerance,
        }
    }

    fn geom(&self, edge: usize) -> &Geom {
        &self.curves[self.edges[edge].curve].1
    }

    /// Where the half-edge `h` starts and ends, as vertices.
    fn ends(&self, h: usize) -> (usize, usize) {
        let edge = &self.edges[h / 2];
        if h.is_multiple_of(2) {
            (edge.start, edge.end)
        } else {
            (edge.end, edge.start)
        }
    }

    /// The parameters the half-edge `h` runs from and to.
    fn params(&self, h: usize) -> (f64, f64) {
        let edge = &self.edges[h / 2];
        if h.is_multiple_of(2) {
            (edge.from, edge.to)
        } else {
            (edge.to, edge.from)
        }
    }

    /// The ends only one piece reaches, sorted by `x`.
    fn open_ends(&self) -> Vec<OpenEnd> {
        let mut open: Vec<OpenEnd> = self
            .edges
            .iter()
            .flat_map(|edge| [(edge.start, edge.end), (edge.end, edge.start)])
            .filter(|&(end, _)| self.degree[end] == 1)
            .map(|(end, from)| OpenEnd {
                at: self.vertices[end],
                from: self.vertices[from],
            })
            .collect();
        open.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
        open
    }

    /// The regions, see the module's docs.
    fn regions(mut self, work: &mut Work) -> Result<Vec<Region>, TooComplex> {
        self.prune();
        let sorted = self.around(work)?;
        let walks = self.faces(&sorted, work)?;
        self.nest(walks, work)
    }

    /// Takes away dangling pieces, those with an end nothing else reaches,
    /// until there are none.
    fn prune(&mut self) {
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); self.vertices.len()];
        for (e, edge) in self.edges.iter().enumerate() {
            at[edge.start].push(e);
            at[edge.end].push(e);
        }
        let mut alive = vec![true; self.edges.len()];
        let mut open: Vec<usize> = (0..self.vertices.len())
            .filter(|&v| self.degree[v] == 1)
            .collect();
        while let Some(v) = open.pop() {
            let Some(&e) = at[v].iter().find(|&&e| alive[e]) else {
                continue;
            };
            alive[e] = false;
            let edge = self.edges[e];
            for end in [edge.start, edge.end] {
                self.degree[end] = self.degree[end].saturating_sub(1);
                if self.degree[end] == 1 {
                    open.push(end);
                }
            }
        }
        let mut kept = alive.iter();
        self.edges.retain(|_| *kept.next().unwrap_or(&false));
    }

    /// Each vertex's outgoing half-edges counter-clockwise, and each
    /// half-edge's place among its vertex's.
    fn around(&self, work: &mut Work) -> Result<(Vec<Vec<usize>>, Vec<usize>), TooComplex> {
        let mut out: Vec<Vec<Leaving>> = vec![Vec::new(); self.vertices.len()];
        for h in 0..2 * self.edges.len() {
            let (from, to) = self.params(h);
            let geom = self.geom(h / 2);
            let (direction, curvature) = geom.heading(from, to > from);
            let start = self.ends(h).0;
            out[start].push(Leaving {
                h,
                angle: angle::to_angle(direction),
                curvature,
                off: geom.at(from) - self.vertices[start],
            });
        }
        let length = |h: usize| {
            let (from, to) = self.params(h);
            self.geom(h / 2).length(from, to)
        };
        let mut sorted = Vec::with_capacity(out.len());
        let mut place = vec![0; 2 * self.edges.len()];
        for mut leaving in out {
            work.spend(leaving.len())?;
            sort_around(&mut leaving, &length, self.tolerance * ROUNDING);
            for (i, leaving) in leaving.iter().enumerate() {
                place[leaving.h] = i;
            }
            sorted.push(leaving.into_iter().map(|leaving| leaving.h).collect());
        }
        Ok((sorted, place))
    }

    /// Traces every face with it on the left, and cuts each walk round
    /// one into simple loops: the bridges (pieces with the face on both
    /// sides) cut out, and cut again where it passes a vertex twice. Those
    /// going counter-clockwise are faces' outer loops, the clockwise ones
    /// the holes in the face they're traced with, or else, traced without
    /// one, a part's boundary seen from outside.
    fn faces(
        &self,
        (sorted, place): &(Vec<Vec<usize>>, Vec<usize>),
        work: &mut Work,
    ) -> Result<Vec<Walk>, TooComplex> {
        let count = 2 * self.edges.len();
        let mut walk_of = vec![usize::MAX; count];
        let mut traced = Vec::new();
        for first in 0..count {
            if walk_of[first] != usize::MAX {
                continue;
            }
            let mut walk = Vec::new();
            let mut h = first;
            // Each half-edge is in one walk, so they take `count` steps
            // in all.
            while walk_of[h] == usize::MAX {
                work.spend(1)?;
                walk_of[h] = traced.len();
                walk.push(h);
                // The next half-edge clockwise from this one's twin,
                // where it ends.
                let twin = h ^ 1;
                let leaving = &sorted[self.ends(twin).0];
                h = leaving[(place[twin] + leaving.len() - 1) % leaving.len()];
            }
            traced.push(walk);
        }
        let mut walks = Vec::with_capacity(traced.len());
        let mut seen = vec![false; self.edges.len()];
        let mut passed = vec![usize::MAX; self.vertices.len()];
        for (index, walk) in traced.iter().enumerate() {
            // A bridge's first crossing opens a loop and its second
            // closes it.
            let mut open = vec![Vec::new()];
            let mut bridged = Vec::new();
            for &h in walk {
                if walk_of[h ^ 1] != index {
                    if let Some(current) = open.last_mut() {
                        current.push(h);
                    }
                } else if !seen[h / 2] {
                    seen[h / 2] = true;
                    open.push(Vec::new());
                } else if let Some(closed) = open.pop() {
                    bridged.push(closed);
                }
            }
            bridged.extend(open);
            let mut loops = Vec::new();
            for found in bridged {
                work.spend(found.len())?;
                pinches(&found, |h| self.ends(h).0, &mut passed, &mut loops);
            }
            loops.retain(|found| !found.is_empty());
            let areas: Vec<f64> = loops.iter().map(|found| self.area(found)).collect();
            // Only a face's outer loop runs counter-clockwise; the largest,
            // should rounding find two.
            let outer = (0..loops.len())
                .filter(|&l| areas[l] > 0.0)
                .max_by(|&a, &b| areas[a].total_cmp(&areas[b]));
            walks.push(Walk {
                loops,
                areas,
                outer,
            });
        }
        Ok(walks)
    }

    /// The area `found` encloses, positive counter-clockwise.
    fn area(&self, found: &[usize]) -> f64 {
        let origin = self.vertices[self.ends(found[0]).0];
        found
            .iter()
            .map(|&h| {
                let geom = self.geom(h / 2);
                let (from, to) = self.params(h);
                let (a, b) = (geom.at(from) - origin, geom.at(to) - origin);
                a.perp_dot(b) / 2.0 + geom.bulge(from, to)
            })
            .sum()
    }

    /// The box `found` lies in.
    fn bounds(&self, found: &[usize]) -> Bounds {
        found
            .iter()
            .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(min, max), &h| {
                let (from, to) = self.params(h);
                let (low, high) = self.geom(h / 2).span_bounds(from, to);
                (min.min(low), max.max(high))
            })
    }

    /// The length round `found`.
    fn perimeter(&self, found: &[usize]) -> f64 {
        found
            .iter()
            .map(|&h| {
                let (from, to) = self.params(h);
                self.geom(h / 2).length(from, to)
            })
            .sum()
    }

    /// How many times `found` winds round `point`, counter-clockwise
    /// positive.
    fn winding(&self, found: &[usize], point: DVec2) -> i64 {
        let total: f64 = found
            .iter()
            .map(|&h| {
                let (from, to) = self.params(h);
                self.geom(h / 2).winding(from, to, point)
            })
            .sum();
        (total / TAU).round() as i64
    }

    /// Each part's boundary seen from outside is inside the smallest face
    /// of another part around it, if any. Every face is a region, holes
    /// and all: a plate with bolt holes is the plate, with a hole per bolt,
    /// and each hole's inside a region of its own, as is an island in one.
    /// Slivers (no wider than the tolerance) are left out.
    fn nest(&self, walks: Vec<Walk>, work: &mut Work) -> Result<Vec<Region>, TooComplex> {
        // Which connected part each vertex is in.
        let mut part = Sets::new(self.vertices.len());
        for edge in &self.edges {
            part.join(edge.start, edge.end);
        }
        // Faces that aren't slivers, as polylines and the box they're in.
        let solid: Vec<Option<(Vec<DVec2>, Bounds)>> = walks
            .iter()
            .map(|walk| {
                let outer = &walk.loops[walk.outer?];
                let wide = walk.areas[walk.outer?] > self.tolerance * self.perimeter(outer);
                // The loop's box, not its polyline's, which an arc's
                // segments cut inside, so that what's between them and
                // the arc is inside it too.
                wide.then(|| (self.flatten(outer), self.bounds(outer)))
            })
            .collect();
        let grid = Grid::new(
            solid
                .iter()
                .enumerate()
                .filter_map(|(f, solid)| Some((f, solid.as_ref()?.1))),
            work,
        )?;
        // The boundaries seen from outside, by the face each is a hole in.
        let mut holes_in: Vec<Vec<&[usize]>> = vec![Vec::new(); walks.len()];
        for boundary in walks
            .iter()
            .filter(|walk| walk.outer.is_none())
            .flat_map(|walk| &walk.loops)
        {
            let start = self.ends(boundary[0]).0;
            let point = self.vertices[start];
            let own = part.root(start);
            // The face and its area.
            let mut smallest: Option<(usize, f64)> = None;
            let near = grid.at(point);
            work.spend(near.len())?;
            for &f in near {
                let (Some((_, (min, max))), Some(outer)) = (&solid[f], walks[f].outer) else {
                    continue;
                };
                if point.cmplt(*min).any() || point.cmpgt(*max).any() {
                    continue;
                }
                let area = walks[f].areas[outer];
                let outer = &walks[f].loops[outer];
                if part.root(self.ends(outer[0]).0) == own
                    || smallest.is_some_and(|(_, smallest)| smallest <= area)
                {
                    continue;
                }
                work.spend(outer.len())?;
                if self.winding(outer, point) != 0 {
                    smallest = Some((f, area));
                }
            }
            if let Some((face, _)) = smallest {
                holes_in[face].push(boundary);
            }
        }
        let mut regions = Vec::new();
        for (w, (walk, solid)) in walks.iter().zip(solid).enumerate() {
            let (Some((polyline, bounds)), Some(outer)) = (solid, walk.outer) else {
                continue;
            };
            let area = walk.areas[outer];
            let mut holes: Vec<&[usize]> = (0..walk.loops.len())
                .filter(|&l| l != outer)
                .map(|l| &walk.loops[l][..])
                .collect();
            holes.extend(&holes_in[w]);
            // As faces are, holes no wider than the tolerance are slivers.
            holes.retain(|hole| self.area(hole).abs() > self.tolerance * self.perimeter(hole));
            let holes_area: f64 = holes.iter().map(|hole| self.area(hole).abs()).sum();
            let outer = &walk.loops[outer];
            let mut outline = vec![polyline];
            outline.extend(holes.iter().map(|hole| self.flatten(hole)));
            regions.push(Region {
                outer: self.pieces(outer),
                holes: holes.iter().map(|hole| self.pieces(hole)).collect(),
                outline,
                area: area - holes_area,
                bounds,
            });
        }
        Ok(regions)
    }

    /// The half-edges `found` as exact pieces.
    fn pieces(&self, found: &[usize]) -> Vec<Piece> {
        found
            .iter()
            .map(|&h| {
                let (from, to) = self.params(h);
                let (start, end) = self.ends(h);
                Piece {
                    curve: self.curves[self.edges[h / 2].curve].0,
                    from,
                    to,
                    start,
                    end,
                }
            })
            .collect()
    }

    /// `found` as a polyline, from each piece's first vertex, a circle's
    /// or an arc's pieces in their share of
    /// [`CIRCLE_SEGMENTS`](crate::CIRCLE_SEGMENTS), a spline's as it's
    /// drawn ([`Geom::polyline`]).
    fn flatten(&self, found: &[usize]) -> Vec<DVec2> {
        let mut polyline = Vec::new();
        for &h in found {
            polyline.push(self.vertices[self.ends(h).0]);
            let (from, to) = self.params(h);
            let piece = self.geom(h / 2).polyline(from, to);
            // Its ends are the vertices.
            polyline.extend(piece.iter().skip(1).take(piece.len().saturating_sub(2)));
        }
        polyline
    }
}

/// A box, its least and greatest corners.
type Bounds = (DVec2, DVec2);

/// Boxes by the cells of a grid over them all they cover, to find those a
/// point may be in without trying every one.
struct Grid {
    min: DVec2,
    cell: DVec2,
    side: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    /// The most cells along a side.
    const MAX_SIDE: usize = 256;

    /// A grid of about as many cells as `boxes`, each an index and a box.
    fn new(
        boxes: impl Iterator<Item = (usize, Bounds)> + Clone,
        work: &mut Work,
    ) -> Result<Grid, TooComplex> {
        let (min, max, count) = boxes.clone().fold(
            (DVec2::INFINITY, DVec2::NEG_INFINITY, 0usize),
            |(min, max, count), (_, (low, high))| {
                (min.min(low), max.max(high), count.saturating_add(1))
            },
        );
        let side = ((count as f64).sqrt().ceil() as usize).clamp(1, Grid::MAX_SIDE);
        let mut grid = Grid {
            min,
            cell: ((max - min) / side as f64).max(DVec2::splat(f64::MIN_POSITIVE)),
            side,
            cells: vec![Vec::new(); side * side],
        };
        for (index, (low, high)) in boxes {
            let (Some((x0, y0)), Some((x1, y1))) = (grid.cell(low), grid.cell(high)) else {
                continue;
            };
            work.spend((x1 - x0 + 1) * (y1 - y0 + 1))?;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.cells[y * side + x].push(index);
                }
            }
        }
        Ok(grid)
    }

    /// The cell `point` is in, if it's in the grid.
    fn cell(&self, point: DVec2) -> Option<(usize, usize)> {
        let at = ((point - self.min) / self.cell).floor();
        let within = |value: f64| (0.0..=self.side as f64).contains(&value);
        let last = self.side - 1;
        (within(at.x) && within(at.y))
            .then(|| ((at.x as usize).min(last), (at.y as usize).min(last)))
    }

    /// The indices of the boxes that may hold `point`.
    fn at(&self, point: DVec2) -> &[usize] {
        match self.cell(point) {
            Some((x, y)) => &self.cells[y * self.side + x],
            None => &[],
        }
    }
}

/// A walk round a face, cut into simple loops, with their areas and
/// which is the face's outer loop, if it's bounded.
struct Walk {
    loops: Vec<Vec<usize>>,
    areas: Vec<f64>,
    outer: Option<usize>,
}

/// A half-edge leaving a vertex, for [`sort_around`].
#[derive(Debug, Clone, Copy)]
struct Leaving {
    h: usize,
    /// The direction it leaves in.
    angle: f64,
    /// How it turns there, left positive.
    curvature: f64,
    /// Where it starts, from the vertex, which it was merged into.
    off: DVec2,
}

/// Sorts half-edges leaving a vertex counter-clockwise from the widest
/// gap between them: by angle, and those within [`SAME_DIRECTION`] of the
/// one before by where each is, sideways, halfway along the shortest of
/// them (`length` of a half-edge), which none crosses another before: as
/// it leaves the vertex, turns from the first's direction and curves
/// (tangent, the one curving more to the left is further round; crossing,
/// however barely, the one leaving further left), from where it was
/// merged into the vertex, unless that's within `rounding` of it. Stable,
/// so those alike stay in order of direction.
///
/// Sorted by direction alone, curves crossing barely, meeting again at
/// another vertex further on, and those merged into a vertex a hair to
/// one side, as a circle's start is into where it touches a line, would
/// be on one side of each other at one vertex and the other at the next,
/// and faces wouldn't join up.
fn sort_around(leaving: &mut [Leaving], length: &dyn Fn(usize) -> f64, rounding: f64) {
    leaving.sort_by(|a, b| a.angle.total_cmp(&b.angle).then(a.h.cmp(&b.h)));
    let n = leaving.len();
    if n < 2 {
        return;
    }
    let gap = |i: usize| {
        let next = leaving[(i + 1) % n].angle;
        let gap = next - leaving[i].angle;
        if i + 1 == n { gap + TAU } else { gap }
    };
    let widest = (0..n)
        .max_by(|&a, &b| gap(a).total_cmp(&gap(b)))
        .unwrap_or(0);
    leaving.rotate_left((widest + 1) % n);
    let mut start = 0;
    for i in 1..=n {
        let apart = |i: usize| (leaving[i].angle - leaving[i - 1].angle).rem_euclid(TAU);
        if i == n || apart(i) > SAME_DIRECTION {
            let group = &mut leaving[start..i];
            start = i;
            if group.len() < 2 {
                continue;
            }
            let base = group[0].angle;
            let along = group
                .iter()
                .map(|leaving| length(leaving.h))
                .fold(f64::INFINITY, f64::min)
                / 2.0;
            let side = |leaving: &Leaving| {
                let off = angle::from_angle(base).perp_dot(leaving.off);
                let off = if off.abs() <= rounding { 0.0 } else { off };
                // Across the angles' wrap, the long way round.
                let turned = match leaving.angle - base {
                    turned if turned > PI => turned - TAU,
                    turned if turned < -PI => turned + TAU,
                    turned => turned,
                };
                off + angle::sin(turned) * along + leaving.curvature * along * along / 2.0
            };
            group.sort_by(|a, b| side(a).total_cmp(&side(b)));
        }
    }
}

/// Cuts the closed walk `found` where it passes a vertex twice into loops
/// that don't, onto `loops`, each step leaving the vertex `from` gives.
/// `passed` is where in the path so far each vertex is left from,
/// `usize::MAX` for none, as it's given and left: indexed by vertex, so
/// every vertex must be within it.
fn pinches<T: Copy>(
    found: &[T],
    from: impl Fn(T) -> usize,
    passed: &mut [usize],
    loops: &mut Vec<Vec<T>>,
) {
    let mut path: Vec<T> = Vec::with_capacity(found.len());
    for &step in found {
        let vertex = from(step);
        if passed[vertex] != usize::MAX {
            let closed = path.split_off(passed[vertex]);
            for &passing in &closed {
                passed[from(passing)] = usize::MAX;
            }
            loops.push(closed);
        }
        passed[vertex] = path.len();
        path.push(step);
    }
    for &step in &path {
        passed[from(step)] = usize::MAX;
    }
    loops.push(path);
}

#[cfg(test)]
mod tests;

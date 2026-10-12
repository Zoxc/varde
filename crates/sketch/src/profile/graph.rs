//! The planar graph of the cut pieces, and tracing its faces into
//! regions.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use super::{Edge, OpenEnd, Piece, Region, TooComplex, Work};
use crate::Id;
use crate::angle;
use crate::intersect::Geom;
use crate::sets::Sets;

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

/// The pieces as a graph: half-edge `2 e` runs along edge `e` from its
/// start to its end, `2 e + 1` back.
pub(super) struct Graph<'c> {
    curves: &'c [(Id, Geom)],
    pub(super) vertices: Vec<DVec2>,
    edges: Vec<Edge>,
    /// Each vertex's edges' count of ends there, a piece going round
    /// counting twice.
    degree: Vec<usize>,
    /// What places were merged within.
    tolerance: f64,
}

/// The half-edges round each vertex, from [`Graph::around`].
struct Around {
    /// Each vertex's outgoing half-edges, counter-clockwise.
    leaving: Vec<Vec<usize>>,
    /// Each half-edge's index in its start vertex's `leaving`.
    place: Vec<usize>,
}

impl<'c> Graph<'c> {
    pub(super) fn new(
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
    pub(super) fn open_ends(&self) -> Vec<OpenEnd> {
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
    pub(super) fn regions(mut self, work: &mut Work) -> Result<Vec<Region>, TooComplex> {
        self.prune();
        let around = self.around(work)?;
        let walks = self.faces(&around, work)?;
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

    /// Each vertex's outgoing half-edges counter-clockwise, see [`Around`].
    fn around(&self, work: &mut Work) -> Result<Around, TooComplex> {
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
        Ok(Around {
            leaving: sorted,
            place,
        })
    }

    /// Traces every face with it on the left, and cuts each walk round
    /// one into simple loops: the bridges (pieces with the face on both
    /// sides) cut out, and cut again where it passes a vertex twice. Those
    /// going counter-clockwise are faces' outer loops, the clockwise ones
    /// the holes in the face they're traced with, or else, traced without
    /// one, a part's boundary seen from outside.
    fn faces(&self, around: &Around, work: &mut Work) -> Result<Vec<Walk>, TooComplex> {
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
                let leaving = &around.leaving[self.ends(twin).0];
                h = leaving[(around.place[twin] + leaving.len() - 1) % leaving.len()];
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
            // should rounding find two. One no wider than the tolerance is
            // a sliver, whether a face that's nothing or a walk round a
            // part's outside that's passed a vertex twice as rounding has
            // it: either way no face, its loops seen from outside, so
            // that a part inside a face is a hole in it.
            let outer = (0..loops.len())
                .filter(|&l| areas[l] > 0.0)
                .max_by(|&a, &b| areas[a].total_cmp(&areas[b]))
                .filter(|&l| areas[l] > self.tolerance * self.perimeter(&loops[l]));
            walks.push(Walk {
                loops,
                areas,
                outer,
            });
        }
        Ok(walks)
    }

    /// Whether each piece of `found` starts where the one before ends,
    /// and the first where the last does. A walk is cut into loops that
    /// join up where the pieces are sorted round each vertex alike from
    /// either end, as they are but for curves lying along each other a
    /// hair apart, merged into vertices at some places and not others:
    /// then a walk can pass a piece both ways without it being a bridge,
    /// and its loops not join.
    fn joined(&self, found: &[usize]) -> bool {
        let next = found.iter().cycle().skip(1);
        found
            .iter()
            .zip(next)
            .all(|(&h, &n)| self.ends(h).1 == self.ends(n).0)
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
    /// positive, its work spent from `work`.
    fn winding(&self, found: &[usize], point: DVec2, work: &mut Work) -> Result<i64, TooComplex> {
        let mut spent = 0;
        let total: f64 = found
            .iter()
            .map(|&h| {
                let (from, to) = self.params(h);
                self.geom(h / 2).winding(from, to, point, &mut spent)
            })
            .sum();
        work.spend(spent)?;
        Ok((total / TAU).round() as i64)
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
        // Faces' loops, slivers too, and below the boundaries that are
        // holes in them, all join up (see [`Graph::joined`]), or regions
        // made of them would be wrong, a sliver among them left out
        // wrongly: curves too close together to find regions in. The
        // boundaries round the outside, in no face, may not.
        let faces = walks.iter().filter(|walk| walk.outer.is_some());
        if !faces
            .flat_map(|walk| &walk.loops)
            .all(|found| self.joined(found))
        {
            return Err(TooComplex);
        }
        // Faces (no slivers, which `faces` left without an outer loop),
        // by their outer loop, as a polyline and the box it's in.
        let solid: Vec<Option<(usize, Vec<DVec2>, Bounds)>> = walks
            .iter()
            .map(|walk| {
                let index = walk.outer?;
                let outer = &walk.loops[index];
                // The loop's box, not its polyline's, which an arc's
                // segments cut inside, so that what's between them and
                // the arc is inside it too.
                Some((index, self.flatten(outer), self.bounds(outer)))
            })
            .collect();
        let grid = Grid::new(
            solid
                .iter()
                .enumerate()
                .filter_map(|(f, solid)| Some((f, solid.as_ref()?.2))),
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
            // The faces whose box holds it, smallest first (the first
            // made of those alike), so that the first it's inside is the
            // one: nested a thousand deep, it's tested against one or two
            // rather than all of them. Taken from a heap rather than
            // sorted, as only those first are wanted.
            let cell = grid.at(point);
            work.spend(cell.len())?;
            let near: Vec<Reverse<Smallest>> = (cell.iter())
                .filter_map(|&f| {
                    let &(outer, _, (min, max)) = solid[f].as_ref()?;
                    let inside = point.cmpge(min).all() && point.cmple(max).all();
                    inside.then_some(Reverse(Smallest(walks[f].areas[outer], f, outer)))
                })
                .collect();
            work.spend(near.len())?;
            let mut near = BinaryHeap::from(near);
            let taken = sort_cost(near.len());
            while let Some(Reverse(Smallest(_, f, outer))) = near.pop() {
                work.spend(taken)?;
                let outer = &walks[f].loops[outer];
                if part.root(self.ends(outer[0]).0) == own {
                    continue;
                }
                if self.winding(outer, point, work)? != 0 {
                    holes_in[f].push(boundary);
                    break;
                }
            }
        }
        let mut regions = Vec::new();
        for (w, (walk, solid)) in walks.iter().zip(solid).enumerate() {
            let Some((outer, polyline, bounds)) = solid else {
                continue;
            };
            let area = walk.areas[outer];
            let mut holes: Vec<&[usize]> = (0..walk.loops.len())
                .filter(|&l| l != outer)
                .map(|l| &walk.loops[l][..])
                .collect();
            if !holes_in[w].iter().all(|found| self.joined(found)) {
                return Err(TooComplex);
            }
            holes.extend(&holes_in[w]);
            // As faces are, holes no wider than the tolerance are slivers.
            holes.retain(|hole| self.area(hole).abs() > self.tolerance * self.perimeter(hole));
            let holes_area: f64 = holes.iter().map(|hole| self.area(hole).abs()).sum();
            // Holes run clockwise, inside the outer loop. Where curves
            // touch at a place found as several a hair over the tolerance
            // apart, or lie along each other, the pieces at those vertices
            // can be sorted in ways that don't agree from one to the
            // next, and walks take wrong turns there: refused, as a region
            // with a hole the wrong way round or outside it is wrong.
            let backwards = holes.iter().any(|hole| self.area(hole) >= 0.0);
            if backwards || holes_area >= area {
                return Err(TooComplex);
            }
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

/// The work of taking an item from a heap of `count`, or of sorting
/// them, per item: about their logarithm, at least one.
fn sort_cost(count: usize) -> usize {
    (usize::BITS - count.leading_zeros()).max(1) as usize
}

/// A face by its area, then its index, and its outer loop: ordered
/// smallest first (in a [`Reverse`]), the first made of those alike.
#[derive(Debug, Clone, Copy)]
struct Smallest(f64, usize, usize);

impl PartialEq for Smallest {
    fn eq(&self, other: &Smallest) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Smallest {}

impl PartialOrd for Smallest {
    fn partial_cmp(&self, other: &Smallest) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Smallest {
    fn cmp(&self, other: &Smallest) -> Ordering {
        (self.0.total_cmp(&other.0)).then(self.1.cmp(&other.1))
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
pub(super) fn pinches<T: Copy>(
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

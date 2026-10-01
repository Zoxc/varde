#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::collections::BTreeSet;
use std::f64::consts::PI;

use super::*;
use crate::Curve;
use crate::testing::{circle, line, point};

/// Lines joining `corners` in turn, the last back to the first, sharing
/// their points.
fn polygon(sketch: &mut Sketch, corners: &[(f64, f64)]) -> Vec<Id> {
    let points: Vec<Id> = corners.iter().map(|&(x, y)| point(sketch, x, y)).collect();
    (0..points.len())
        .map(|i| line(sketch, points[i], points[(i + 1) % points.len()]))
        .collect()
}

fn rectangle(sketch: &mut Sketch, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Id> {
    polygon(sketch, &[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn round(sketch: &mut Sketch, x: f64, y: f64, radius: f64) -> Id {
    let center = point(sketch, x, y);
    circle(sketch, center, radius)
}

/// The arc around `(x, y)` of `radius` counter-clockwise from the angle
/// `from` to `to`.
fn arc(sketch: &mut Sketch, x: f64, y: f64, radius: f64, from: f64, to: f64) -> Id {
    let center = DVec2::new(x, y);
    let [center, start, end] = [
        center,
        center + DVec2::from_angle(from) * radius,
        center + DVec2::from_angle(to) * radius,
    ]
    .map(|at| sketch.add_point(at).unwrap());
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap()
}

fn profiles(sketch: &Sketch) -> Profiles {
    let profiles = sketch.profiles().unwrap();
    for region in &profiles.regions {
        closed(sketch, region);
    }
    profiles
}

/// The shape of the curve `piece` is part of.
fn geom(sketch: &Sketch, piece: &Piece) -> Geom {
    Geom::of(sketch, &sketch.curve(piece.curve).unwrap().curve).unwrap()
}

/// The area `pieces` enclose, positive counter-clockwise, worked out
/// from the curves, about where they start, as their ends meet only
/// within the tolerance.
fn area(sketch: &Sketch, pieces: &[Piece]) -> f64 {
    let origin = geom(sketch, &pieces[0]).at(pieces[0].from);
    pieces
        .iter()
        .map(|piece| {
            let geom = geom(sketch, piece);
            let (a, b) = (geom.at(piece.from) - origin, geom.at(piece.to) - origin);
            a.perp_dot(b) / 2.0 + geom.bulge(piece.from, piece.to)
        })
        .sum()
}

/// Checks each of `region`'s loops joins up, the outer one runs
/// counter-clockwise and the holes clockwise, and the area adds up.
fn closed(sketch: &Sketch, region: &Region) {
    let size = sketch
        .points
        .iter()
        .map(|point| point.at.abs().max_element())
        .fold(1.0, f64::max);
    let place = |piece: &Piece, u: f64| geom(sketch, piece).at(u);
    for pieces in std::iter::once(&region.outer).chain(&region.holes) {
        assert!(!pieces.is_empty());
        for (i, piece) in pieces.iter().enumerate() {
            let next = &pieces[(i + 1) % pieces.len()];
            let (end, start) = (place(piece, piece.to), place(next, next.from));
            assert!(end.distance(start) <= 1e-6 * size, "{end} {start}");
            assert_eq!(piece.end, next.start);
        }
    }
    let outer = area(sketch, &region.outer);
    assert!(outer > 0.0, "{outer}");
    let holes: f64 = region.holes.iter().map(|hole| area(sketch, hole)).sum();
    for hole in &region.holes {
        assert!(area(sketch, hole) < 0.0);
    }
    assert!(
        (outer + holes - region.area).abs() <= 1e-6 * size * size,
        "{outer} {holes} {}",
        region.area
    );
    assert_eq!(region.outline.len(), 1 + region.holes.len());
    assert!(region.outline.iter().flatten().all(|at| at.is_finite()));
}

/// The regions' areas, sorted.
fn areas(profiles: &Profiles) -> Vec<f64> {
    let mut areas: Vec<f64> = profiles.regions.iter().map(|region| region.area).collect();
    areas.sort_by(f64::total_cmp);
    areas
}

fn about(found: &[f64], expected: &[f64]) {
    assert_eq!(found.len(), expected.len(), "{found:?} {expected:?}");
    for (found, expected) in found.iter().zip(expected) {
        assert!((found - expected).abs() < 1e-6, "{found:?} {expected:?}");
    }
}

/// The curves `pieces` are parts of.
fn curves(pieces: &[Piece]) -> BTreeSet<Id> {
    pieces.iter().map(|piece| piece.curve).collect()
}

#[test]
fn a_plate_with_holes_is_the_plate_and_each_hole() {
    let mut sketch = Sketch::default();
    let sides = rectangle(&mut sketch, 0.0, 0.0, 100.0, 60.0);
    let holes: BTreeSet<Id> = [(10.0, 10.0), (90.0, 10.0), (90.0, 50.0), (10.0, 50.0)]
        .iter()
        .map(|&(x, y)| round(&mut sketch, x, y, 5.0))
        .collect();
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 1 + holes.len());
    let plate = found
        .regions
        .iter()
        .find(|region| !region.holes.is_empty())
        .unwrap();
    assert_eq!(curves(&plate.outer), sides.into_iter().collect());
    assert_eq!(plate.holes.len(), 4);
    let in_holes: BTreeSet<Id> = plate.holes.iter().flat_map(|hole| curves(hole)).collect();
    assert_eq!(in_holes, holes);
    // Each hole's inside, a disc with nothing in it.
    let discs: Vec<&Region> = found
        .regions
        .iter()
        .filter(|region| region.holes.is_empty())
        .collect();
    assert_eq!(discs.len(), 4);
    let disc_curves: BTreeSet<Id> = discs.iter().flat_map(|disc| curves(&disc.outer)).collect();
    assert_eq!(disc_curves, holes);
    let disc = 25.0 * PI;
    about(
        &areas(&found),
        &[disc, disc, disc, disc, 6000.0 - 4.0 * disc],
    );
    assert!(found.open_ends.is_empty());
}

#[test]
fn crossing_circles_make_three_regions() {
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    round(&mut sketch, 8.0, 0.0, 5.0);
    let found = profiles(&sketch);
    // The lens is two segments of height 1 of a circle of radius 5,
    // meeting at x = 4, half-angle acos(4 / 5).
    let half = (4.0f64 / 5.0).acos();
    let lens = 2.0 * (25.0 * half - 4.0 * 3.0);
    let crescent = 25.0 * PI - lens;
    about(&areas(&found), &[lens, crescent, crescent]);
    assert!(found.regions.iter().all(|region| region.holes.is_empty()));
}

#[test]
fn a_line_across_a_circle_splits_it() {
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    let (a, b) = (
        point(&mut sketch, -10.0, 1.0),
        point(&mut sketch, 10.0, -1.0),
    );
    line(&mut sketch, a, b);
    let found = profiles(&sketch);
    let half = 25.0 * PI / 2.0;
    about(&areas(&found), &[half, half]);
    // Through a quadrant point, where the circle starts.
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    let (a, b) = (
        point(&mut sketch, -10.0, 0.0),
        point(&mut sketch, 10.0, 0.0),
    );
    line(&mut sketch, a, b);
    about(&areas(&profiles(&sketch)), &[half, half]);
}

#[test]
fn a_rectangle_with_a_diagonal_is_two() {
    let mut sketch = Sketch::default();
    let mut corners = Vec::new();
    for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)] {
        corners.push(point(&mut sketch, x, y));
    }
    for i in 0..4 {
        line(&mut sketch, corners[i], corners[(i + 1) % 4]);
    }
    line(&mut sketch, corners[0], corners[2]);
    about(&areas(&profiles(&sketch)), &[30.0, 30.0]);
}

#[test]
fn rings_in_rings_are_each_a_region() {
    let mut sketch = Sketch::default();
    for radius in [10.0, 8.0, 6.0, 4.0, 2.0] {
        round(&mut sketch, 0.0, 0.0, radius);
    }
    // 10 less 8, 8 less 6, 6 less 4, 4 less 2, and the disc of 2.
    let found = profiles(&sketch);
    about(
        &areas(&found),
        &[4.0 * PI, 12.0 * PI, 20.0 * PI, 28.0 * PI, 36.0 * PI],
    );
    let holes: Vec<usize> = found
        .regions
        .iter()
        .map(|region| region.holes.len())
        .collect();
    assert_eq!(holes.iter().sum::<usize>(), 4);
    assert!(holes.iter().all(|&holes| holes <= 1));
}

#[test]
fn an_island_in_a_hole_is_a_region() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 100.0, 100.0);
    rectangle(&mut sketch, 20.0, 20.0, 80.0, 80.0);
    // Crossing circles in the hole, and a hole in their lens.
    round(&mut sketch, 45.0, 50.0, 8.0);
    round(&mut sketch, 55.0, 50.0, 8.0);
    rectangle(&mut sketch, 48.0, 48.0, 52.0, 52.0);
    let found = profiles(&sketch);
    let half = (5.0f64 / 8.0).acos();
    let lens = 2.0 * (64.0 * half - 5.0 * 39.0f64.sqrt());
    let crescent = 64.0 * PI - lens;
    // The hole's inside less the circles, the circles' faces, the square
    // in the lens and the frame.
    let circles = 128.0 * PI - lens;
    about(
        &areas(&found),
        &[
            16.0,
            lens - 16.0,
            crescent,
            crescent,
            3600.0 - circles,
            10_000.0 - 3600.0,
        ],
    );
    // The frame has one hole, the hole's inside a hole of the two circles
    // (crossing, one boundary), the lens one of the square.
    let mut holes: Vec<usize> = found
        .regions
        .iter()
        .map(|region| region.holes.len())
        .collect();
    holes.sort();
    assert_eq!(holes, [0, 0, 0, 1, 1, 1]);
}

#[test]
fn loops_touching_at_a_point() {
    // From outside: two regions.
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    round(&mut sketch, 10.0, 0.0, 5.0);
    let disc = 25.0 * PI;
    about(&areas(&profiles(&sketch)), &[disc, disc]);
    // Squares at a corner.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 1.0, 1.0);
    rectangle(&mut sketch, 1.0, 1.0, 2.0, 2.0);
    about(&areas(&profiles(&sketch)), &[1.0, 1.0]);
    // Inside, touching: still a hole.
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 10.0);
    let small = round(&mut sketch, 6.0, 0.0, 4.0);
    let found = profiles(&sketch);
    about(&areas(&found), &[16.0 * PI, 84.0 * PI]);
    let ring = found
        .regions
        .iter()
        .find(|region| !region.holes.is_empty())
        .unwrap();
    assert_eq!(ring.holes.len(), 1);
    assert_eq!(curves(&ring.holes[0]), BTreeSet::from([small]));
    // A square in a square sharing a corner.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    rectangle(&mut sketch, 0.0, 0.0, 2.0, 2.0);
    about(&areas(&profiles(&sketch)), &[4.0, 96.0]);
}

#[test]
fn overlapping_curves_count_once() {
    // Rectangles side by side, sharing part of a side.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    rectangle(&mut sketch, 10.0, 2.0, 20.0, 8.0);
    about(&areas(&profiles(&sketch)), &[60.0, 100.0]);
    // A rectangle drawn twice, and one over half of it.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    rectangle(&mut sketch, 0.0, 0.0, 5.0, 10.0);
    about(&areas(&profiles(&sketch)), &[50.0, 50.0]);
    // A circle, an arc on it (its own centre point) and a diameter.
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    arc(&mut sketch, 0.0, 0.0, 5.0, 0.3, 2.0);
    let (a, b) = (point(&mut sketch, -5.0, 0.0), point(&mut sketch, 5.0, 0.0));
    line(&mut sketch, a, b);
    let half = 25.0 * PI / 2.0;
    about(&areas(&profiles(&sketch)), &[half, half]);
    // Two arcs on one circle overlapping, closed by a chord: the segment.
    let mut sketch = Sketch::default();
    arc(&mut sketch, 0.0, 0.0, 5.0, 0.0, 2.0);
    arc(&mut sketch, 0.0, 0.0, 5.0, 1.0, PI);
    let (a, b) = (point(&mut sketch, 5.0, 0.0), point(&mut sketch, -5.0, 0.0));
    line(&mut sketch, a, b);
    about(&areas(&profiles(&sketch)), &[half]);
}

#[test]
fn a_hole_joined_by_a_line_is_still_a_hole() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 20.0, 20.0);
    round(&mut sketch, 10.0, 10.0, 5.0);
    let (a, b) = (point(&mut sketch, 0.0, 10.0), point(&mut sketch, 5.0, 10.0));
    line(&mut sketch, a, b);
    let found = profiles(&sketch);
    about(&areas(&found), &[25.0 * PI, 400.0 - 25.0 * PI]);
    let plate = found
        .regions
        .iter()
        .find(|region| region.area > 100.0)
        .unwrap();
    assert_eq!(plate.holes.len(), 1);
}

#[test]
fn arcs_and_lines_close_a_slot() {
    // Two half circles joined by lines, as a slot is drawn.
    let mut sketch = Sketch::default();
    let at = |sketch: &mut Sketch, x, y| point(sketch, x, y);
    let [a, b, c, d] =
        [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)].map(|(x, y)| at(&mut sketch, x, y));
    let (right, left) = (at(&mut sketch, 10.0, 2.0), at(&mut sketch, 0.0, 2.0));
    line(&mut sketch, a, b);
    line(&mut sketch, c, d);
    for (center, start, end) in [(right, b, c), (left, d, a)] {
        sketch
            .add_curve(Curve::Arc { center, start, end }, false)
            .unwrap();
    }
    about(&areas(&profiles(&sketch)), &[40.0 + 4.0 * PI]);
}

#[test]
fn dangling_lines_and_construction_are_left_out() {
    let mut sketch = Sketch::default();
    let sides: BTreeSet<Id> = rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0)
        .into_iter()
        .collect();
    let corner = sketch.points[2].id;
    // Out from a corner, in from a side, a branching spur, and alone.
    let out = point(&mut sketch, 15.0, 15.0);
    line(&mut sketch, corner, out);
    let (a, b, c) = (
        point(&mut sketch, 0.0, 5.0),
        point(&mut sketch, 5.0, 5.0),
        point(&mut sketch, 7.0, 7.0),
    );
    line(&mut sketch, a, b);
    line(&mut sketch, b, c);
    let d = point(&mut sketch, 7.0, 3.0);
    line(&mut sketch, b, d);
    let (e, f) = (point(&mut sketch, 20.0, 0.0), point(&mut sketch, 30.0, 0.0));
    line(&mut sketch, e, f);
    // Construction across it all.
    let center = point(&mut sketch, 5.0, 5.0);
    sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 4.0,
            },
            true,
        )
        .unwrap();
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 1);
    assert_eq!(curves(&found.regions[0].outer), sides);
    assert!(found.regions[0].holes.is_empty());
    about(&areas(&found), &[100.0]);
}

#[test]
fn open_ends_a_gap_apart_are_near_misses() {
    let mut sketch = Sketch::default();
    let corners = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.01),
    ]
    .map(|(x, y)| point(&mut sketch, x, y));
    for pair in corners.windows(2) {
        line(&mut sketch, pair[0], pair[1]);
    }
    let found = profiles(&sketch);
    assert!(found.regions.is_empty());
    assert_eq!(found.open_ends.len(), 2);
    assert_eq!(
        found.near_misses(0.1),
        [NearMiss {
            a: DVec2::ZERO,
            b: DVec2::new(0.0, 0.01)
        }]
    );
    for gap in [0.001, -1.0, f64::NAN] {
        assert!(found.near_misses(gap).is_empty(), "{gap}");
    }
    // Within the tolerance it's closed.
    sketch.points[4].at = DVec2::new(0.0, 1e-10);
    let found = profiles(&sketch);
    about(&areas(&found), &[100.0]);
    assert!(found.open_ends.is_empty());
    // A line shorter than the gap has ends close together, but that's no
    // near miss; nor are ends that meet something.
    let mut sketch = Sketch::default();
    let (a, b) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 0.05, 0.0));
    line(&mut sketch, a, b);
    rectangle(&mut sketch, 1.0, 1.0, 2.0, 2.0);
    assert!(sketch.profiles().unwrap().near_misses(0.1).is_empty());
}

#[test]
fn near_misses_are_bounded() {
    // Short lines in a row, each end close to the next's.
    let mut sketch = Sketch::default();
    for i in 0..2 * MAX_NEAR_MISSES {
        let x = i as f64 * 2.0;
        let (a, b) = (point(&mut sketch, x, 0.0), point(&mut sketch, x + 1.5, 0.0));
        line(&mut sketch, a, b);
    }
    let found = profiles(&sketch);
    assert_eq!(found.near_misses(1.0).len(), MAX_NEAR_MISSES);
    // Ends all within the gap along x but far apart along y: more pairs
    // than are compared, none near.
    let mut sketch = Sketch::default();
    for i in 0..1500 {
        let (x, y) = (i as f64 * 1e-3, i as f64 * 10.0);
        let (a, b) = (point(&mut sketch, x, y), point(&mut sketch, x, y + 5.0));
        line(&mut sketch, a, b);
    }
    let found = profiles(&sketch);
    let ends = found.open_ends.len();
    assert!(ends * (ends - 1) / 2 > MAX_NEAR_PAIRS);
    assert!(found.near_misses(3.0).is_empty());
}

#[test]
fn regions_are_picked_by_a_point() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 100.0, 60.0);
    round(&mut sketch, 50.0, 30.0, 10.0);
    let found = profiles(&sketch);
    let plate = found.region_at(DVec2::new(5.0, 5.0)).unwrap();
    assert_eq!(found.regions[plate].holes.len(), 1);
    // The hole's inside is a region of its own.
    let hole = found.region_at(DVec2::new(50.0, 30.0)).unwrap();
    assert_ne!(hole, plate);
    assert!(found.regions[hole].holes.is_empty());
    assert_eq!(found.region_at(DVec2::new(-5.0, 30.0)), None);

    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 5.0);
    round(&mut sketch, 8.0, 0.0, 5.0);
    let found = profiles(&sketch);
    let smallest = |found: &Profiles| {
        (0..found.regions.len())
            .min_by(|&a, &b| found.regions[a].area.total_cmp(&found.regions[b].area))
    };
    assert_eq!(found.region_at(DVec2::new(4.0, 0.0)), smallest(&found));
    let left = found.region_at(DVec2::new(-3.0, 0.0)).unwrap();
    let right = found.region_at(DVec2::new(11.0, 0.0)).unwrap();
    assert_ne!(left, right);
    assert_ne!(Some(left), smallest(&found));
}

#[test]
fn too_many_crossings_or_too_much_work_is_too_complex() {
    // A grid of lines crossing each other.
    let mut sketch = Sketch::default();
    for i in 0..20 {
        let at = i as f64;
        let (a, b) = (point(&mut sketch, at, -1.0), point(&mut sketch, at, 20.0));
        line(&mut sketch, a, b);
        let (a, b) = (point(&mut sketch, -1.0, at), point(&mut sketch, 20.0, at));
        line(&mut sketch, a, b);
    }
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 19 * 19);
    // 80 ends and 400 crossings, each cutting two lines.
    let limits = |splits, work| Limits { splits, work };
    assert!(sketch.profiles_within(&limits(880, MAX_WORK)).is_ok());
    assert_eq!(
        sketch.profiles_within(&limits(879, MAX_WORK)),
        Err(TooComplex)
    );
    assert_eq!(
        sketch.profiles_within(&limits(MAX_SPLITS, 1000)),
        Err(TooComplex)
    );
    // Circles in a column, whose boxes all overlap along x, and never
    // meet: work, not crossings.
    let mut sketch = Sketch::default();
    for i in 0..100 {
        round(&mut sketch, 0.0, i as f64 * 3.0, 1.0);
    }
    assert_eq!(profiles(&sketch).regions.len(), 100);
    assert_eq!(
        sketch.profiles_within(&limits(MAX_SPLITS, 99 * 50)),
        Err(TooComplex)
    );
}

/// A few pseudo-random numbers, the same each run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n - 1)
    }
}

/// A sketch of random lines, circles and arcs, often sharing points, on a
/// coarse grid with `snap` so many are on one line or circle, touch or
/// overlap.
fn random_sketch(random: &mut Random, curves: usize, snap: bool) -> Sketch {
    let mut sketch = Sketch::default();
    let coordinate = |random: &mut Random| {
        let value = random.next() * 20.0;
        if snap { value.round() } else { value }
    };
    for _ in 0..curves {
        let reuse = |sketch: &Sketch, random: &mut Random| {
            (!sketch.points.is_empty() && random.next() < 0.5)
                .then(|| sketch.points[random.below(sketch.points.len())].id)
        };
        let new_point = |sketch: &mut Sketch, random: &mut Random| {
            let at = DVec2::new(coordinate(random), coordinate(random));
            sketch.add_point(at).unwrap()
        };
        let kind = random.below(3);
        let a = match reuse(&sketch, random) {
            Some(id) => id,
            None => new_point(&mut sketch, random),
        };
        let b = match reuse(&sketch, random) {
            Some(id) if id != a => id,
            _ => new_point(&mut sketch, random),
        };
        let curve = match kind {
            0 => Curve::Line { start: a, end: b },
            1 => {
                let radius = if snap {
                    1.0 + random.below(6) as f64
                } else {
                    0.5 + random.next() * 6.0
                };
                Curve::Circle { center: a, radius }
            }
            _ => {
                // An arc from `b` round `a`, ending on its circle.
                let center = sketch.point(a).unwrap().at;
                let from = sketch.point(b).unwrap().at - center;
                let turn = random.next() * 6.0;
                let end = center + DVec2::from_angle(from.to_angle() + turn) * from.length();
                let end = sketch.add_point(end).unwrap();
                Curve::Arc {
                    center: a,
                    start: b,
                    end,
                }
            }
        };
        sketch.add_curve(curve, random.next() < 0.1).unwrap();
    }
    sketch
}

#[test]
fn random_sketches_make_closed_regions() {
    let mut random = Random(7);
    let mut regions = 0;
    for round in 0..300 {
        let snap = round % 2 == 0;
        let sketch = random_sketch(&mut random, 1 + round % 40, snap);
        let found = sketch.profiles().unwrap();
        for region in &found.regions {
            closed(&sketch, region);
            assert!(region.area > 0.0, "{round}: {}", region.area);
            let (min, max) = region.bounds;
            assert!(min.cmple(max).all());
        }
        regions += found.regions.len();
        for _ in 0..10 {
            let at = DVec2::new(random.next() * 24.0 - 2.0, random.next() * 24.0 - 2.0);
            found.region_at(at);
        }
        // The regions don't overlap: a point is in one at most, by the
        // outlines as drawn, away from their edges.
        for _ in 0..20 {
            let at = DVec2::new(random.next() * 20.0, random.next() * 20.0);
            let near_edge = found.regions.iter().any(|region| {
                region.outline.iter().any(|polyline| {
                    (0..polyline.len()).any(|i| {
                        let (a, b) = (polyline[i], polyline[(i + 1) % polyline.len()]);
                        let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                        (a + (b - a) * t).distance(at) < 0.1
                    })
                })
            });
            if near_edge {
                continue;
            }
            let inside = found
                .regions
                .iter()
                .filter(|region| region.contains(at))
                .count();
            assert!(inside <= 1, "{round}: {at} in {inside}");
        }
    }
    // They found some.
    assert!(regions > 100, "{regions}");
}

#[test]
fn an_arc_off_its_circle_bounds_nothing() {
    // A half disc: the arc from (0, -5) round the right to (0, 5), and a
    // line back. Its end drawn out to (0, 6), the radius changing along
    // it, the arc isn't the half circle profiles would take it to be.
    let mut sketch = Sketch::default();
    let [center, start, end] =
        [(0.0, 0.0), (0.0, -5.0), (0.0, 6.0)].map(|(x, y)| point(&mut sketch, x, y));
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    line(&mut sketch, end, start);
    let found = profiles(&sketch);
    assert!(found.regions.is_empty(), "{:?}", areas(&found));
    // Off by what the solver leaves, it's on it.
    let mut sketch = Sketch::default();
    let [center, start, end] =
        [(0.0, 0.0), (0.0, -5.0), (0.0, 5.0 + 1e-11)].map(|(x, y)| point(&mut sketch, x, y));
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    line(&mut sketch, end, start);
    about(&areas(&profiles(&sketch)), &[12.5 * PI]);
}

#[test]
fn lines_leaving_a_corner_almost_together_are_told_apart() {
    // Two thin triangles meeting at the origin, one to the right, its
    // sides 10⁻⁷ radians apart, one to the left; the right one's upper
    // side drawn first, so it comes first among the pieces.
    for upper_first in [true, false] {
        let mut sketch = Sketch::default();
        let origin = point(&mut sketch, 0.0, 0.0);
        let low = point(&mut sketch, 10.0, 0.0);
        let high = point(&mut sketch, 10.0, 1e-6);
        if upper_first {
            line(&mut sketch, origin, high);
            line(&mut sketch, origin, low);
        } else {
            line(&mut sketch, origin, low);
            line(&mut sketch, origin, high);
        }
        line(&mut sketch, low, high);
        let (left_low, left_high) = (
            point(&mut sketch, -10.0, -1.0),
            point(&mut sketch, -10.0, 1.0),
        );
        line(&mut sketch, origin, left_low);
        line(&mut sketch, left_low, left_high);
        line(&mut sketch, left_high, origin);
        about(&areas(&profiles(&sketch)), &[5e-6, 10.0]);
    }
}

#[test]
fn an_island_by_an_arc_s_rim_is_a_hole_in_it() {
    // A disc of radius 100 cut off by a chord on the left: the arc
    // flattens into segments that don't reach x = 100.
    let mut sketch = Sketch::default();
    let from = 170f64.to_radians();
    let arc = arc(&mut sketch, 0.0, 0.0, 100.0, -from, from);
    let Curve::Arc { start, end, .. } = sketch.curve(arc).unwrap().curve else {
        unreachable!()
    };
    line(&mut sketch, end, start);
    // A small triangle inside, by the rim, right of where the polyline is.
    polygon(&mut sketch, &[(99.9, -0.3), (99.95, 0.0), (99.9, 0.3)]);
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 2);
    let disc = found
        .regions
        .iter()
        .find(|region| curves(&region.outer).contains(&arc))
        .unwrap();
    assert_eq!(disc.holes.len(), 1);
}

#[test]
fn a_hole_too_small_to_see_is_left_out() {
    // Circles well within the tolerance (10⁻⁸) of a point, inside the
    // square and on its corner, where they'd cross its sides.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    round(&mut sketch, 5.0, 5.0, 1e-10);
    round(&mut sketch, 10.0 - 1e-10, 0.0, 1e-10);
    // A little over it: a hole, but too thin to be a region.
    round(&mut sketch, 3.0, 3.0, 1.5e-8);
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 1);
    assert!(
        found.regions[0].holes.is_empty(),
        "{:?}",
        found.regions[0].holes
    );
    about(&areas(&found), &[100.0]);
}

#[test]
fn circles_barely_crossing_with_a_curve_between_make_closed_regions() {
    // A kilometre off, where the tolerance is a micrometre: a circle and an
    // arc inside it, crossing it barely (into it by 0.96 µm, at places
    // 0.26 apart), and a circle crossing both between those places.
    let off = DVec2::splat(1e6);
    let mut sketch = Sketch::default();
    let mut at = |x, y| sketch.add_point(off + DVec2::new(x, y)).unwrap();
    let (big, center, start, end, other) = (
        at(5.262692302116193, 7.141976652666926),
        at(6.8506810531252995, 5.309805041411892),
        at(8.054608615930192, 1.9195978688076138),
        at(3.7096138009801507, 3.55576890532393),
        at(13.790142732090317, 2.036044267239049),
    );
    circle(&mut sketch, big, 6.021249833842113);
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    circle(&mut sketch, other, 4.57753203335046);
    // Each loop joins up and winds the way it should.
    let found = profiles(&sketch);
    assert!(found.regions.iter().all(|region| region.holes.is_empty()));
}

#[test]
fn a_slot_barely_into_its_arcs_is_four_pieces() {
    // Lines along y = ±1 into the arcs' circles by 10⁻¹², tangent as a
    // solver leaves them: their crossings further on are off the arcs.
    let mut sketch = Sketch::default();
    let y = 1.0 - 1e-12;
    let [a, b, c, d] =
        [(0.0, -y), (10.0, -y), (10.0, y), (0.0, y)].map(|(x, y)| point(&mut sketch, x, y));
    let (left, right) = (point(&mut sketch, 0.0, 0.0), point(&mut sketch, 10.0, 0.0));
    line(&mut sketch, a, b);
    sketch
        .add_curve(
            Curve::Arc {
                center: right,
                start: b,
                end: c,
            },
            false,
        )
        .unwrap();
    line(&mut sketch, c, d);
    sketch
        .add_curve(
            Curve::Arc {
                center: left,
                start: d,
                end: a,
            },
            false,
        )
        .unwrap();
    let found = profiles(&sketch);
    assert_eq!(found.regions.len(), 1);
    assert_eq!(found.regions[0].outer.len(), 4);
    about(&areas(&found), &[20.0 + PI]);
}

#[test]
fn circles_in_squares_as_a_solver_leaves_them_make_five_regions_each() {
    // Two squares side by side, a circle in each touching all four sides,
    // off by what a solver leaves: the left one into the middle side by
    // 10⁻¹³, crossing it at places 2·10⁻⁶ apart, 2·10⁻⁷ radians
    // apart, the right one short of it, the circles into each other.
    // Each square is the circle and its four corners, whatever the
    // slivers between.
    for (left, right) in [
        (
            (5.0000000000000036, 5.000000000000045, 5.000000000000084),
            (15.000000000000052, 4.999999999999943, 5.000000000000012),
        ),
        (
            (4.999999999999699, 4.999999999999026, 4.999999999999963),
            (14.999999999999414, 4.99999999999965, 5.000000000000611),
        ),
        (
            (5.000000000000588, 4.999999999999783, 5.000000000000631),
            (14.999999999999376, 5.00000000000098, 5.000000000000966),
        ),
    ] {
        let mut sketch = Sketch::default();
        rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
        rectangle(&mut sketch, 10.0, 0.0, 20.0, 10.0);
        for (x, y, radius) in [left, right] {
            round(&mut sketch, x, y, radius);
        }
        let found = profiles(&sketch);
        let corner = 25.0 - 25.0 * PI / 4.0;
        about(
            &areas(&found),
            &[
                corner,
                corner,
                corner,
                corner,
                corner,
                corner,
                corner,
                corner,
                25.0 * PI,
                25.0 * PI,
            ],
        );
    }
}

#[test]
fn four_circles_in_squares_as_a_solver_leaves_them_make_five_regions_each() {
    // Two by two squares, a circle in each touching its sides, corners
    // and circles off by what a solver leaves: where the circles cross
    // the middle lines barely, next to each other, the places of a
    // crossing are apart by rounding, as far as the curves are sideways
    // halfway to the next.
    let corners = [
        [
            (-2.007146350802873e-15, 7.0346801386735416e-15),
            (10.0, 9.3284781004137e-15),
            (20.0, -1.7384405003944226e-15),
        ],
        [
            (-1.3135151610597885e-15, 10.000000000000004),
            (9.999999999999993, 9.999999999999998),
            (20.0, 10.000000000000004),
        ],
        [
            (6.71956610770142e-15, 20.0),
            (10.000000000000005, 20.0),
            (20.0, 20.00000000000001),
        ],
    ];
    let mut sketch = Sketch::default();
    let corners = corners.map(|row| row.map(|(x, y)| point(&mut sketch, x, y)));
    for i in 0..3 {
        for j in 0..2 {
            line(&mut sketch, corners[i][j], corners[i][j + 1]);
            line(&mut sketch, corners[j][i], corners[j + 1][i]);
        }
    }
    for (x, y, radius) in [
        (5.0000000000000036, 5.000000000000007, 5.00000000000001),
        (14.999999999999904, 5.000000000000024, 4.999999999999992),
        (4.999999999999927, 14.999999999999963, 4.999999999999977),
        (15.000000000000007, 14.999999999999982, 5.000000000000004),
    ] {
        round(&mut sketch, x, y, radius);
    }
    let found = profiles(&sketch);
    let corner = 25.0 - 25.0 * PI / 4.0;
    let mut expected = vec![corner; 16];
    expected.extend([25.0 * PI; 4]);
    about(&areas(&found), &expected);
}

/// Checks `loops`, merged, join up by their vertices, piece to piece, and
/// pass no vertex twice, and gives their areas, sorted.
fn merged_areas(sketch: &Sketch, profiles: &Profiles, loops: &[Vec<Piece>]) -> Vec<f64> {
    for found in loops {
        assert!(!found.is_empty());
        let mut starts = BTreeSet::new();
        for (i, piece) in found.iter().enumerate() {
            assert_eq!(piece.end, found[(i + 1) % found.len()].start);
            assert!(starts.insert(piece.start), "{found:?}");
            assert!(piece.start < profiles.vertices.len());
        }
    }
    let mut areas: Vec<f64> = loops.iter().map(|found| area(sketch, found)).collect();
    areas.sort_by(f64::total_cmp);
    areas
}

/// The index of the region `point` is in.
fn at_point(profiles: &Profiles, x: f64, y: f64) -> usize {
    profiles.region_at(DVec2::new(x, y)).unwrap()
}

#[test]
fn regions_side_by_side_merge_into_one_loop() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 2.0, 1.0);
    let a = point(&mut sketch, 1.0, 0.0);
    let b = point(&mut sketch, 1.0, 1.0);
    let middle = line(&mut sketch, a, b);
    let profiles = profiles(&sketch);
    assert_eq!(profiles.regions.len(), 2);
    let loops = profiles.merge(&[0, 1]).unwrap();
    assert_eq!(loops.len(), 1);
    about(&merged_areas(&sketch, &profiles, &loops), &[2.0]);
    assert!(loops[0].iter().all(|piece| piece.curve != middle));
    // Either way round, and repeats counting once, the same.
    assert_eq!(profiles.merge(&[1, 0, 1]).unwrap(), loops);
    // One region is its own loops.
    assert_eq!(
        profiles.merge(&[1]).unwrap(),
        vec![profiles.regions[1].outer.clone()]
    );
}

#[test]
fn a_hole_and_what_fills_it_merge_into_the_plate() {
    let mut sketch = Sketch::default();
    let sides = rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    round(&mut sketch, 5.0, 5.0, 2.0);
    round(&mut sketch, 2.0, 2.0, 1.0);
    let profiles = profiles(&sketch);
    let plate = at_point(&profiles, 8.0, 8.0);
    let disc = at_point(&profiles, 5.0, 5.0);
    let small = at_point(&profiles, 2.0, 2.0);
    // The plate alone keeps both holes.
    let loops = profiles.merge(&[plate]).unwrap();
    let hole = PI * 4.0;
    let little = PI;
    about(
        &merged_areas(&sketch, &profiles, &loops),
        &[-hole, -little, 100.0],
    );
    // With the disc, one hole's gone.
    let loops = profiles.merge(&[disc, plate]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[-little, 100.0]);
    // With both, the square.
    let loops = profiles.merge(&[disc, plate, small]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[100.0]);
    assert_eq!(curves(&loops[0]), sides.into_iter().collect());
    // The discs alone are two loops.
    let loops = profiles.merge(&[small, disc]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[little, hole]);
}

#[test]
fn halves_of_a_circle_merge_into_the_circle() {
    let mut sketch = Sketch::default();
    let circle = round(&mut sketch, 0.0, 0.0, 3.0);
    let a = point(&mut sketch, -1.0, -5.0);
    let b = point(&mut sketch, 2.0, 5.0);
    line(&mut sketch, a, b);
    let profiles = profiles(&sketch);
    assert_eq!(profiles.regions.len(), 2);
    let loops = profiles.merge(&[0, 1]).unwrap();
    assert_eq!(loops.len(), 1);
    assert!(loops[0].iter().all(|piece| piece.curve == circle));
    about(&merged_areas(&sketch, &profiles, &loops), &[PI * 9.0]);
}

#[test]
fn regions_apart_or_at_a_corner_stay_loops_of_their_own() {
    let mut sketch = Sketch::default();
    // Three in a row, the middle one sharing a side with each.
    rectangle(&mut sketch, 0.0, 0.0, 3.0, 1.0);
    for x in [1.0, 2.0] {
        let a = point(&mut sketch, x, 0.0);
        let b = point(&mut sketch, x, 1.0);
        line(&mut sketch, a, b);
    }
    // And one touching the last at its corner.
    rectangle(&mut sketch, 3.0, 1.0, 4.0, 2.0);
    let profiles = profiles(&sketch);
    let [first, middle, last, corner] =
        [(0.5, 0.5), (1.5, 0.5), (2.5, 0.5), (3.5, 1.5)].map(|(x, y)| at_point(&profiles, x, y));
    let loops = profiles.merge(&[first, last]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[1.0, 1.0]);
    let loops = profiles.merge(&[first, middle, last]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[3.0]);
    // Cut where they meet, as two loops, whichever way they were traced.
    let loops = profiles.merge(&[first, middle, last, corner]).unwrap();
    about(&merged_areas(&sketch, &profiles, &loops), &[1.0, 3.0]);
}

#[test]
fn merging_nothing_or_a_missing_region_fails() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 1.0, 1.0);
    let profiles = profiles(&sketch);
    assert_eq!(profiles.merge(&[]), Err(MergeError::Empty));
    assert_eq!(profiles.merge(&[0, 1]), Err(MergeError::NoRegion(1)));
    assert_eq!(
        Profiles::default().merge(&[usize::MAX]),
        Err(MergeError::NoRegion(usize::MAX))
    );
    // Hand-made profiles whose pieces don't join up, or name vertices
    // they don't have.
    let mut broken = profiles.clone();
    broken.regions[0].outer.pop();
    assert_eq!(broken.merge(&[0]), Err(MergeError::Open));
    let mut broken = profiles.clone();
    broken.regions[0].outer[0].start = profiles.vertices.len();
    assert_eq!(broken.merge(&[0]), Err(MergeError::Open));
}

#[test]
fn every_region_is_found_again_by_its_reference() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    round(&mut sketch, 5.0, 5.0, 2.0);
    // A line across the disc splits it into two regions of the same
    // curves, told apart by the point inside.
    let a = point(&mut sketch, 3.0, 3.0);
    let b = point(&mut sketch, 7.0, 7.5);
    line(&mut sketch, a, b);
    // A thin one.
    rectangle(&mut sketch, 1.0, 8.0, 9.0, 8.01);
    let profiles = profiles(&sketch);
    let references: Vec<RegionRef> = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    let found = profiles.resolve(&references);
    let all: Vec<Option<usize>> = (0..profiles.regions.len()).map(Some).collect();
    assert_eq!(found, all);
    for (index, reference) in references.iter().enumerate() {
        assert_eq!(reference.check(1e6), Ok(()));
        assert_eq!(profiles.region_at(reference.inside), Some(index));
    }
    assert_eq!(profiles.reference(profiles.regions.len()), None);
}

#[test]
fn a_region_is_found_again_after_its_sketch_changes() {
    let mut sketch = Sketch::default();
    let sides = rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    let hole = round(&mut sketch, 5.0, 5.0, 2.0);
    let before = profiles(&sketch);
    let plate = at_point(&before, 9.0, 9.0);
    let reference = before.reference(plate).unwrap();
    let mut expected: Vec<Id> = sides.clone();
    expected.sort_unstable();
    assert_eq!(reference.curves, expected);
    assert_eq!(reference.holes, vec![vec![hole]]);

    // Curves added elsewhere, and the plate made smaller so the point
    // inside is outside it: found by its curves.
    rectangle(&mut sketch, 20.0, 0.0, 30.0, 10.0);
    for point in &mut sketch.points {
        if point.at.x == 10.0 {
            point.at.x = 8.0;
        }
    }
    sketch.curves.iter_mut().for_each(|entry| {
        if let Curve::Circle { radius, .. } = &mut entry.curve {
            *radius = 1.0;
        }
    });
    let mut moved = reference.clone();
    moved.inside = DVec2::new(9.0, 9.0);
    let after = profiles(&sketch);
    let plate = at_point(&after, 7.0, 7.0);
    assert_eq!(after.resolve(&[moved.clone()]), vec![Some(plate)]);
    assert_eq!(after.regions[plate].holes.len(), 1);

    // Split by a line across it: no region has its curves, so the one
    // its point is in.
    let a = point(&mut sketch, 0.0, 8.5);
    let b = point(&mut sketch, 8.0, 8.5);
    line(&mut sketch, a, b);
    let split = profiles(&sketch);
    let top = at_point(&split, 5.0, 9.0);
    let mut inside = reference.clone();
    inside.inside = DVec2::new(5.0, 9.0);
    assert_eq!(split.resolve(&[inside]), vec![Some(top)]);
    // With its point outside every region, it's gone.
    assert_eq!(split.resolve(&[moved]), vec![None]);
}

#[test]
fn references_from_files_are_checked() {
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 10.0, 10.0);
    round(&mut sketch, 5.0, 5.0, 2.0);
    round(&mut sketch, 2.0, 2.0, 1.0);
    let profiles = profiles(&sketch);
    let good = profiles.reference(at_point(&profiles, 9.0, 9.0)).unwrap();
    assert_eq!(good.holes.len(), 2);
    assert_eq!(good.check(1e6), Ok(()));
    let bytes = postcard::to_allocvec(&good).unwrap();
    assert_eq!(postcard::from_bytes::<RegionRef>(&bytes).unwrap(), good);

    let wrong = |change: &dyn Fn(&mut RegionRef)| {
        let mut reference = good.clone();
        change(&mut reference);
        reference.check(1e6)
    };
    assert_eq!(wrong(&|r| r.curves.clear()), Err(RegionRefError::Unsorted));
    assert_eq!(
        wrong(&|r| r.curves.reverse()),
        Err(RegionRefError::Unsorted)
    );
    assert_eq!(
        wrong(&|r| r.curves.push(r.curves[3])),
        Err(RegionRefError::Unsorted)
    );
    assert_eq!(wrong(&|r| r.holes.reverse()), Err(RegionRefError::Unsorted));
    assert_eq!(
        wrong(&|r| r.holes[0].clear()),
        Err(RegionRefError::Unsorted)
    );
    for inside in [
        DVec2::new(f64::NAN, 0.0),
        DVec2::new(0.0, f64::INFINITY),
        DVec2::new(0.0, -2e6),
    ] {
        let found = wrong(&|r| r.inside = inside);
        assert!(
            matches!(found, Err(RegionRefError::Inside(at)) if at.x.to_bits() == inside.x.to_bits()),
            "{found:?}"
        );
    }
    let many = RegionRef {
        curves: good.curves.clone(),
        holes: vec![good.holes[0].clone(); MAX_REGION_CURVES],
        inside: good.inside,
    };
    assert_eq!(
        many.check(1e6),
        Err(RegionRefError::TooManyCurves(4 + MAX_REGION_CURVES))
    );
}

#[test]
fn a_region_s_point_inside_keeps_clear_of_its_outline() {
    // An L 17 high with a notch 1 high cut from its bottom right, the
    // notch a region of its own: the first line across the L, at a
    // height of 1, runs along the notch's top, and the middle of the
    // widest span there is the notch's corner.
    let mut sketch = Sketch::default();
    let points: Vec<Id> = [(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 1.0), (5.0, 1.0)]
        .iter()
        .chain(&[(10.0, 17.0), (0.0, 17.0)])
        .map(|&(x, y)| point(&mut sketch, x, y))
        .collect();
    for (a, b) in [
        (0, 1),
        (1, 4),
        (4, 3),
        (3, 5),
        (5, 6),
        (6, 0),
        (1, 2),
        (2, 3),
    ] {
        line(&mut sketch, points[a], points[b]);
    }
    let found = profiles(&sketch);
    let l = at_point(&found, 2.0, 10.0);
    let reference = found.reference(l).unwrap();
    let outline = &found.regions[l].outline;
    let clearance = outline
        .iter()
        .flat_map(|polyline| {
            (0..polyline.len()).map(|i| (polyline[i], polyline[(i + 1) % polyline.len()]))
        })
        .map(|(a, b)| {
            let t = ((reference.inside - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            reference.inside.distance(a + (b - a) * t)
        })
        .fold(f64::INFINITY, f64::min);
    assert!(
        clearance >= 1.0,
        "{} is {clearance} from the outline",
        reference.inside
    );

    // The notch a hair higher and a hole in the L: the L's curves differ,
    // so it's found by its point, which must still be in the L.
    for entry in &mut sketch.points {
        if entry.at.y == 1.0 {
            entry.at.y = 1.0 + 1e-6;
        }
    }
    round(&mut sketch, 3.0, 12.0, 1.0);
    let after = profiles(&sketch);
    assert_eq!(
        after.resolve(&[reference]),
        vec![Some(at_point(&after, 2.0, 10.0))]
    );
}

#[test]
fn a_region_of_curves_several_share_is_only_found_among_them() {
    // A disc in a plate, cut in two by a line across it: both halves
    // are bounded by the circle and the line.
    let mut sketch = Sketch::default();
    rectangle(&mut sketch, 0.0, 0.0, 20.0, 10.0);
    let center = point(&mut sketch, 5.0, 5.0);
    circle(&mut sketch, center, 3.0);
    let a = point(&mut sketch, 5.0, 1.0);
    let b = point(&mut sketch, 5.0, 9.0);
    line(&mut sketch, a, b);
    let before = profiles(&sketch);
    let left = at_point(&before, 4.0, 5.0);
    let reference = before.reference(left).unwrap();
    assert_eq!(
        before.resolve(std::slice::from_ref(&reference)),
        vec![Some(left)]
    );

    // The disc and its line moved right: the point inside the left half
    // is in the plate now, which isn't bounded by those curves.
    for entry in &mut sketch.points {
        if entry.at.x == 5.0 {
            entry.at.x = 14.0;
        }
    }
    let after = profiles(&sketch);
    assert_eq!(
        after.region_at(reference.inside),
        Some(at_point(&after, 1.0, 1.0))
    );
    assert_eq!(after.resolve(std::slice::from_ref(&reference)), vec![None]);
    // Its point in one of them, it's that one.
    let mut moved = reference;
    moved.inside.x += 9.0;
    assert_eq!(
        after.resolve(&[moved]),
        vec![Some(at_point(&after, 13.0, 5.0))]
    );
}

#[test]
fn arc_ends_take_their_bits_from_libm() {
    // Arcs and circles at random places, angles and radii, and lines
    // crossing them. Each arc's ends are vertices placed by `libm`'s
    // trigonometry, never the platform's maths library, so a sketch gives
    // the same profiles on every platform.
    let mut state = 0x5eed_u64;
    let mut unit = || {
        // SplitMix64.
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let on = |center: DVec2, radius: f64, angle: f64| {
        center + DVec2::new(libm::cos(angle), libm::sin(angle)) * radius
    };
    let mut sketch = Sketch::default();
    let mut expected = Vec::new();
    for _ in 0..50 {
        let center = DVec2::new(unit() * 100.0, unit() * 100.0);
        let radius = 1.0 + unit() * 30.0;
        if unit() < 0.2 {
            let id = sketch.add_point(center).unwrap();
            circle(&mut sketch, id, radius);
            continue;
        }
        let (from, to) = (unit() * 20.0 - 10.0, unit() * 20.0 - 10.0);
        let [c, start, end] = [center, on(center, radius, from), on(center, radius, to)]
            .map(|at| sketch.add_point(at).unwrap());
        sketch
            .add_curve(
                Curve::Arc {
                    center: c,
                    start,
                    end,
                },
                false,
            )
            .unwrap();
        // As the sketch places them: the start at its angle, the end that
        // plus the sweep.
        let (a, b) = (
            sketch.point(start).unwrap().at - center,
            sketch.point(end).unwrap().at - center,
        );
        let (begin, finish) = (libm::atan2(a.y, a.x), libm::atan2(b.y, b.x));
        let sweep = TAU - (begin - finish).rem_euclid(TAU);
        expected.push(on(center, a.length(), begin));
        expected.push(on(center, a.length(), begin + sweep));
    }
    for _ in 0..20 {
        let a = point(&mut sketch, unit() * 140.0 - 20.0, unit() * 140.0 - 20.0);
        let b = point(&mut sketch, unit() * 140.0 - 20.0, unit() * 140.0 - 20.0);
        line(&mut sketch, a, b);
    }
    let profiles = sketch.profiles().unwrap();
    assert!(!profiles.regions.is_empty());
    let bits = |p: DVec2| (p.x.to_bits(), p.y.to_bits());
    let vertices: BTreeSet<_> = profiles.vertices.iter().map(|&p| bits(p)).collect();
    for p in expected {
        assert!(vertices.contains(&bits(p)), "{p}");
    }
}

#[test]
fn angles_on_the_wrap_and_the_axes_give_whole_regions() {
    // Where `atan2` jumps from π to −π, and at multiples of π/2 and π/4,
    // with `+0` and `−0` where a coordinate is zero and at radii far
    // apart: a circle cut through its centre by a line at each of those
    // angles is two half discs, and an arc from angle 0 to π (or back)
    // closed by its diameter is one.
    let half_turn = |r: f64| PI * r * r / 2.0;
    let areas = |sketch: &Sketch| {
        sketch
            .profiles()
            .unwrap()
            .regions
            .iter()
            .map(|region| region.area)
            .collect::<Vec<_>>()
    };
    let near = |got: f64, want: f64| (got - want).abs() <= 1e-9 * want;
    for r in [1e-3, 1.0, 1e4, 3e5] {
        let q = r * std::f64::consts::FRAC_1_SQRT_2;
        for zero in [0.0, -0.0] {
            let across = [
                DVec2::new(r, zero),
                DVec2::new(q, q),
                DVec2::new(zero, r),
                DVec2::new(-q, q),
            ];
            for way in across {
                let mut sketch = Sketch::default();
                let center = sketch.add_point(DVec2::ZERO).unwrap();
                circle(&mut sketch, center, r);
                let a = sketch.add_point(-2.0 * way).unwrap();
                let b = sketch.add_point(2.0 * way).unwrap();
                line(&mut sketch, a, b);
                let got = areas(&sketch);
                assert_eq!(got.len(), 2, "{r} {zero} {way}");
                assert!(got.iter().all(|&a| near(a, half_turn(r))), "{got:?}");
            }
            for upper in [true, false] {
                let mut sketch = Sketch::default();
                let center = sketch.add_point(DVec2::ZERO).unwrap();
                let east = sketch.add_point(DVec2::new(r, 0.0)).unwrap();
                let west = sketch.add_point(DVec2::new(-r, zero)).unwrap();
                let (start, end) = if upper { (east, west) } else { (west, east) };
                sketch
                    .add_curve(Curve::Arc { center, start, end }, false)
                    .unwrap();
                line(&mut sketch, east, west);
                let got = areas(&sketch);
                assert_eq!(got.len(), 1, "{r} {zero} {upper}");
                assert!(near(got[0], half_turn(r)), "{got:?}");
            }
        }
    }
}

/// `Profiles::resolve` as it was, scanning every region for each
/// reference: the oracle the sorted lookup must agree with.
fn resolve_by_scan(profiles: &Profiles, references: &[RegionRef]) -> Vec<Option<usize>> {
    let lists = |pieces: &[Piece]| {
        let ids: BTreeSet<Id> = pieces.iter().map(|piece| piece.curve).collect();
        ids.into_iter().collect::<Vec<Id>>()
    };
    let ids: Vec<(Vec<Id>, Vec<Vec<Id>>)> = profiles
        .regions
        .iter()
        .map(|region| {
            let mut holes: Vec<Vec<Id>> = region.holes.iter().map(|hole| lists(hole)).collect();
            holes.sort();
            (lists(&region.outer), holes)
        })
        .collect();
    references
        .iter()
        .map(|reference| {
            let same = |(curves, holes): &(Vec<Id>, Vec<Vec<Id>>)| {
                *curves == reference.curves && *holes == reference.holes
            };
            let found: Vec<usize> = (0..ids.len()).filter(|&i| same(&ids[i])).collect();
            match found[..] {
                [index] => Some(index),
                [] => profiles.region_at(reference.inside),
                _ => profiles
                    .region_at(reference.inside)
                    .filter(|&index| same(&ids[index])),
            }
        })
        .collect()
}

/// References for `profiles`, and their resolution checked against the
/// scan: each region's own, and each with its point moved (to random
/// places, other regions' points, the outline), its curves another
/// region's, cut short, or naming nothing, its holes dropped.
fn resolves_as_the_scan(
    random: &mut Random,
    named: &Profiles,
    profiles: &Profiles,
    bogus: Id,
) -> usize {
    let own: Vec<RegionRef> = (0..named.regions.len())
        .filter_map(|index| named.reference(index))
        .collect();
    let mut references = own.clone();
    let (min, max) = profiles.regions.iter().fold(
        (DVec2::splat(-1.0), DVec2::splat(1.0)),
        |(min, max), region| (min.min(region.bounds.0), max.max(region.bounds.1)),
    );
    let outline: Vec<DVec2> = profiles
        .regions
        .iter()
        .flat_map(|region| region.outline.iter().flatten().copied())
        .collect();
    for (k, reference) in own.iter().enumerate() {
        let point = |random: &mut Random| {
            let mut moved = reference.clone();
            moved.inside = min + (max - min) * DVec2::new(random.next(), random.next());
            moved
        };
        references.push(point(random));
        references.push(point(random));
        let other = &own[random.below(own.len())];
        let mut moved = reference.clone();
        moved.inside = other.inside;
        references.push(moved);
        let mut theirs = other.clone();
        theirs.inside = reference.inside;
        references.push(theirs);
        // On the outline: a vertex, and an edge's middle.
        if !outline.is_empty() {
            let at = random.below(outline.len());
            let mut on = reference.clone();
            on.inside = outline[at];
            references.push(on.clone());
            on.inside = (outline[at] + outline[(at + 1) % outline.len()]) / 2.0;
            references.push(on);
        }
        let mut short = reference.clone();
        if short.curves.len() > 1 {
            short.curves.remove(random.below(short.curves.len()));
            references.push(short.clone());
        }
        short.holes.clear();
        references.push(short);
        let mut nothing = reference.clone();
        nothing.curves = vec![bogus];
        references.push(nothing.clone());
        nothing.holes = vec![vec![bogus]; 1 + k % 3];
        references.push(nothing);
    }
    let found = profiles.resolve(&references);
    assert_eq!(found, resolve_by_scan(profiles, &references));
    assert_eq!(found.len(), references.len());
    references.len()
}

#[test]
fn resolve_agrees_with_a_scan() {
    let mut random = Random(28);
    let bogus = Id(u32::MAX);
    let mut references = 0;
    let mut check = |random: &mut Random, sketch: &Sketch| {
        let profiles = profiles(sketch);
        references += resolves_as_the_scan(random, &profiles, &profiles, bogus);
    };

    // Regions of one key: a circle cut by a line, and the lens and two
    // crescents of crossing circles.
    let mut sketch = Sketch::default();
    round(&mut sketch, 0.0, 0.0, 2.0);
    let a = point(&mut sketch, -3.0, 0.5);
    let b = point(&mut sketch, 3.0, -0.5);
    line(&mut sketch, a, b);
    round(&mut sketch, 10.0, 0.0, 2.0);
    round(&mut sketch, 11.0, 0.0, 2.0);
    assert_eq!(profiles(&sketch).regions.len(), 5);
    check(&mut random, &sketch);

    // A 20 by 20 grid of lines.
    let mut sketch = Sketch::default();
    for k in 0..21 {
        let x = k as f64;
        let (a, b) = (point(&mut sketch, x, -0.5), point(&mut sketch, x, 20.5));
        line(&mut sketch, a, b);
        let (a, b) = (point(&mut sketch, -0.5, x), point(&mut sketch, 20.5, x));
        line(&mut sketch, a, b);
    }
    assert_eq!(profiles(&sketch).regions.len(), 400);
    check(&mut random, &sketch);

    // Nested circles, and nested circles each cut by a line through the
    // middle: rings of one key in pairs.
    for cut in [false, true] {
        let mut sketch = Sketch::default();
        let center = point(&mut sketch, 0.0, 0.0);
        for k in 1..=12 {
            circle(&mut sketch, center, k as f64);
        }
        if cut {
            let (a, b) = (
                point(&mut sketch, -13.0, 0.1),
                point(&mut sketch, 13.0, -0.1),
            );
            line(&mut sketch, a, b);
        }
        check(&mut random, &sketch);
    }

    // Random sketches, each resolving its own references, and those of
    // the sketch it grew from (the same curves, fewer of them) and of
    // the sketch it grows into.
    for round in 0..200 {
        let snap = round % 2 == 0;
        let seed = random.0;
        let count = 1 + round % 30;
        let sketch = random_sketch(&mut Random(seed), count, snap);
        let fewer = random_sketch(&mut Random(seed), count.div_ceil(2), snap);
        let more = random_sketch(&mut Random(seed), count + 5, snap);
        let [sketch, fewer, more] =
            [&sketch, &fewer, &more].map(|sketch| sketch.profiles().unwrap());
        references += resolves_as_the_scan(&mut random, &sketch, &sketch, bogus);
        references += resolves_as_the_scan(&mut random, &fewer, &sketch, bogus);
        references += resolves_as_the_scan(&mut random, &more, &sketch, bogus);
        random.next();
    }
    assert!(references > 10_000, "{references}");
}

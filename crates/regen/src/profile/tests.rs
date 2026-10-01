use std::f64::consts::{FRAC_1_SQRT_2, PI};

use glam::DVec2;
use varde_kernel::{Loop, Profile};
use varde_sketch::{Curve, Profiles, Sketch, Spline};

use super::*;

const FIT: f64 = 1e-3;

/// The profile of the regions of `sketch` that `pick` picks, and the
/// profiles.
fn picked(sketch: &Sketch, pick: impl Fn(&varde_sketch::Region) -> bool) -> (Profile, Profiles) {
    picked_within(sketch, pick, FIT)
}

fn picked_within(
    sketch: &Sketch,
    pick: impl Fn(&varde_sketch::Region) -> bool,
    fit: f64,
) -> (Profile, Profiles) {
    let profiles = sketch.profiles().unwrap();
    let indices: Vec<usize> = (0..profiles.regions.len())
        .filter(|&index| pick(&profiles.regions[index]))
        .collect();
    assert!(!indices.is_empty(), "no region picked");
    let loops = profiles.merge(&indices).unwrap();
    let profile = profile(sketch, &profiles, &loops, fit).unwrap();
    profile.check().unwrap();
    (profile, profiles)
}

fn line(sketch: &mut Sketch, a: DVec2, b: DVec2) {
    let start = sketch.add_point(a).unwrap();
    let end = sketch.add_point(b).unwrap();
    sketch.add_curve(Curve::Line { start, end }, false).unwrap();
}

/// Checks each segment of `profile` is an exact arc of the circle about
/// `center` of `radius` or straight, sampled.
fn assert_on_circle_or_straight(profile: &Profile, center: DVec2, radius: f64) {
    for segment in profile.loops.iter().flat_map(|l| &l.segments) {
        let conic = &segment.conic;
        if conic.w == 1.0 && conic.c == conic.p0.midpoint(conic.p1) {
            continue;
        }
        for i in 0..=16 {
            let at = conic.eval(f64::from(i) / 16.0);
            let off = (at.distance(center) - radius).abs();
            assert!(off <= 1e-12 * radius, "{at} is {off} off");
        }
    }
}

#[test]
fn a_circle_is_four_exact_quarters() {
    let mut sketch = Sketch::default();
    let center = DVec2::new(1.0, 2.0);
    let id = sketch.add_point(center).unwrap();
    let curve = sketch
        .add_curve(
            Curve::Circle {
                center: id,
                radius: 5.0,
            },
            false,
        )
        .unwrap();
    let (profile, profiles) = picked(&sketch, |_| true);
    let [l] = &profile.loops[..] else {
        panic!("one loop");
    };
    assert_eq!(l.segments.len(), 4);
    for segment in &l.segments {
        assert_eq!(segment.curve, u64::from(curve.get()));
        assert!((segment.conic.w - FRAC_1_SQRT_2).abs() < 1e-15);
    }
    // It starts and ends at its vertex, its pieces meeting to the bit.
    assert_eq!(l.segments[0].conic.p0, profiles.vertices[0]);
    assert_eq!(l.segments[3].conic.p1, profiles.vertices[0]);
    assert_on_circle_or_straight(&profile, center, 5.0);
    assert!((profile.area() - PI * 25.0).abs() < 1e-12 * PI * 25.0);
}

/// An arc about the origin of radius 5 from angle `from` to `to`
/// (degrees, counter-clockwise), closed by lines through the centre if
/// `through_center`, else by its chord.
fn arc_sketch(from: f64, to: f64, through_center: bool) -> Sketch {
    let mut sketch = Sketch::default();
    let at = |degrees: f64| DVec2::from_angle(degrees.to_radians()) * 5.0;
    let center = sketch.add_point(DVec2::ZERO).unwrap();
    let start = sketch.add_point(at(from)).unwrap();
    let end = sketch.add_point(at(to)).unwrap();
    sketch
        .add_curve(Curve::Arc { center, start, end }, false)
        .unwrap();
    if through_center {
        sketch
            .add_curve(
                Curve::Line {
                    start: center,
                    end: start,
                },
                false,
            )
            .unwrap();
        sketch
            .add_curve(
                Curve::Line {
                    start: end,
                    end: center,
                },
                false,
            )
            .unwrap();
    } else {
        sketch
            .add_curve(
                Curve::Line {
                    start: end,
                    end: start,
                },
                false,
            )
            .unwrap();
    }
    sketch
}

#[test]
fn arcs_are_halved_to_at_most_90_degrees() {
    // Sweep, whether through the centre, how many arc parts, the area.
    let r2 = 25.0;
    for (from, to, through, parts, area) in [
        (0.0, 60.0, true, 1, PI * r2 / 6.0),
        (10.0, 100.0, true, 1, PI * r2 / 4.0),
        (0.0, 180.0, false, 2, PI * r2 / 2.0),
        (30.0, 300.0, true, 4, PI * r2 * 0.75),
        (-100.0, 150.0, false, 4, {
            // A disc less the segment cut off by the chord of 110°.
            let theta = 110f64.to_radians();
            PI * r2 - r2 / 2.0 * (theta - theta.sin())
        }),
    ] {
        let sketch = arc_sketch(from, to, through);
        let (profile, _) = picked(&sketch, |_| true);
        let curved = profile.loops[0]
            .segments
            .iter()
            .filter(|segment| segment.conic.w != 1.0)
            .count();
        assert_eq!(curved, parts, "{from}..{to}");
        assert_on_circle_or_straight(&profile, DVec2::ZERO, 5.0);
        assert!(
            (profile.area() - area).abs() < 1e-11 * area,
            "{from}..{to}: {} vs {area}",
            profile.area()
        );
    }
}

#[test]
fn a_hole_runs_clockwise_and_lines_keep_their_ends() {
    let mut sketch = Sketch::default();
    let corners = [(-3.0, -2.0), (3.0, -2.0), (3.0, 2.0), (-3.0, 2.0)].map(DVec2::from);
    for k in 0..4 {
        line(&mut sketch, corners[k], corners[(k + 1) % 4]);
    }
    let center = sketch.add_point(DVec2::new(0.5, 0.0)).unwrap();
    sketch
        .add_curve(
            Curve::Circle {
                center,
                radius: 1.0,
            },
            false,
        )
        .unwrap();
    let (profile, _) = picked(&sketch, |region| region.holes.len() == 1);
    let [outer, hole] = &profile.loops[..] else {
        panic!("two loops");
    };
    assert!(outer.area() > 0.0 && hole.area() < 0.0);
    assert!((profile.area() - (24.0 - PI)).abs() < 1e-12 * 24.0);
    for segment in &outer.segments {
        assert!(corners.contains(&segment.conic.p0));
    }
    assert_on_circle_or_straight(&profile, DVec2::new(0.5, 0.0), 1.0);
}

/// A closed spline through five points round the origin, about 10 across,
/// and its curve id.
fn blob(sketch: &mut Sketch) -> varde_sketch::Id {
    let points = [
        (5.0, 0.0),
        (2.0, 4.0),
        (-4.0, 3.0),
        (-3.0, -3.0),
        (2.0, -5.0),
    ]
    .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    sketch
        .add_curve(Curve::Spline(Spline::through(points.to_vec(), true)), false)
        .unwrap()
}

/// The shoelace area of `sketch`'s spline `curve`, sampled finely.
fn spline_area(sketch: &Sketch, curve: varde_sketch::Id) -> f64 {
    let shape = sketch.spline_shape(sketch.spline(curve).unwrap()).unwrap();
    let n = 200_000;
    let points: Vec<DVec2> = (0..n).map(|i| shape.point(i as f64 / n as f64)).collect();
    (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]) / 2.0)
        .sum()
}

/// The largest distance from sampled points of `l`'s segments of `curve` to the
/// spline `curve`, sampled finely.
fn largest_error(sketch: &Sketch, curve: varde_sketch::Id, l: &Loop) -> f64 {
    let shape = sketch.spline_shape(sketch.spline(curve).unwrap()).unwrap();
    let n = 20_000;
    let dense: Vec<DVec2> = (0..=n).map(|i| shape.point(i as f64 / n as f64)).collect();
    let to_polyline = |p: DVec2| {
        dense
            .windows(2)
            .map(|w| {
                let d = w[1] - w[0];
                let t = ((p - w[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                p.distance(w[0] + d * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    l.segments
        .iter()
        .filter(|segment| segment.curve == u64::from(curve.get()))
        .flat_map(|segment| (1..8).map(move |i| segment.conic.eval(f64::from(i) / 8.0)))
        .map(to_polyline)
        .fold(0.0, f64::max)
}

/// Whether `conic` is a straight segment as `Conic2::line` makes it.
fn straight(conic: &Conic2) -> bool {
    conic.w == 1.0 && conic.c == (conic.p0 + conic.p1) * 0.5
}

#[test]
fn a_spline_is_fitted_within_the_tolerance_turning_smoothly() {
    let mut sketch = Sketch::default();
    let curve = blob(&mut sketch);
    let mut counts = Vec::new();
    for fit in [1e-1, 1e-3, 1e-5] {
        let (profile, _) = picked_within(&sketch, |_| true, fit);
        let [l] = &profile.loops[..] else {
            panic!("one loop");
        };
        counts.push(l.segments.len());
        assert!(l.segments.iter().all(|s| s.curve == u64::from(curve.get())));
        // Within the tolerance, and the fine polyline's own error.
        let error = largest_error(&sketch, curve, l);
        assert!(error <= fit / 2.0 + 1e-6, "{fit}: {error}");
        // Where curved conics meet, they leave along the same line; where
        // a line meets a conic, it turns by about the tolerance over the
        // line's length at most.
        let n = l.segments.len();
        for k in 0..n {
            let (a, b) = (&l.segments[k].conic, &l.segments[(k + 1) % n].conic);
            assert_eq!(a.p1, b.p0);
            let (out, on) = ((a.p1 - a.c).normalize(), (b.c - b.p0).normalize());
            let kink = out.perp_dot(on).abs();
            match (straight(a), straight(b)) {
                (false, false) => assert!(kink < 1e-3, "{fit}: {k} turns"),
                (true, true) => {}
                (true, false) | (false, true) => {
                    let length = if straight(a) {
                        a.p0.distance(a.p1)
                    } else {
                        b.p0.distance(b.p1)
                    };
                    assert!(kink <= 2.0 * fit / length, "{fit}: {k} turns by {kink}");
                }
            }
            assert!(out.dot(on) > 0.0);
        }
        let area = spline_area(&sketch, curve);
        let perimeter = 40.0;
        assert!(
            (profile.area() - area).abs() <= fit * perimeter,
            "{fit}: {} vs {area}",
            profile.area()
        );
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2], "{counts:?}");
    assert!(counts[2] < 1000, "{counts:?}");
}

#[test]
fn a_spline_hole_runs_backwards() {
    let mut sketch = Sketch::default();
    let corners = [(-8.0, -8.0), (8.0, -8.0), (8.0, 8.0), (-8.0, 8.0)].map(DVec2::from);
    for k in 0..4 {
        line(&mut sketch, corners[k], corners[(k + 1) % 4]);
    }
    let curve = blob(&mut sketch);
    let (profile, _) = picked(&sketch, |region| region.holes.len() == 1);
    let area = 256.0 - spline_area(&sketch, curve);
    assert!((profile.area() - area).abs() <= FIT * 40.0);
    let hole = &profile.loops[1];
    assert!(hole.area() < 0.0);
    assert!(largest_error(&sketch, curve, hole) <= FIT / 2.0 + 1e-6);
}

#[test]
fn an_open_spline_cut_by_a_line_keeps_its_vertices() {
    // An arch: an open spline from (-5, 0) over to (5, 0), and the line
    // back along the bottom, which the spline's ends are on.
    let mut sketch = Sketch::default();
    let points = [(-5.0, 0.0), (-3.0, 4.0), (0.0, 5.0), (3.0, 4.0), (5.0, 0.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let spline = sketch
        .add_curve(
            Curve::Spline(Spline::through(points.to_vec(), false)),
            false,
        )
        .unwrap();
    sketch
        .add_curve(
            Curve::Line {
                start: points[4],
                end: points[0],
            },
            false,
        )
        .unwrap();
    let (profile, profiles) = picked(&sketch, |_| true);
    let l = &profile.loops[0];
    let fitted: Vec<_> = l
        .segments
        .iter()
        .filter(|segment| segment.curve == u64::from(spline.get()))
        .collect();
    assert!(fitted.len() > 1);
    let ends = [fitted[0].conic.p0, fitted[fitted.len() - 1].conic.p1];
    for end in ends {
        assert!(profiles.vertices.contains(&end));
    }
    assert!(largest_error(&sketch, spline, l) <= FIT / 2.0 + 1e-6);
}

/// The SplitMix64 generator, for corpora fixed by their seeds.
struct SplitMix(u64);

impl SplitMix {
    /// The next number in `[0, 1)`.
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A plate `width` wide and half as high whose top edge is an open spline
/// through `n` fit points evenly spaced along it, each but the ends moved
/// up or down by up to `jitter / 2`; the plate is closed by three lines.
/// The spline's id.
fn wavy_plate(
    sketch: &mut Sketch,
    n: usize,
    width: f64,
    jitter: f64,
    seed: u64,
) -> varde_sketch::Id {
    let mut rng = SplitMix(seed);
    let last = n - 1;
    let points: Vec<_> = (0..n)
        .map(|i| {
            let x = width * i as f64 / last as f64;
            let off = if i == 0 || i == last {
                0.0
            } else {
                (rng.next() - 0.5) * jitter
            };
            sketch.add_point(DVec2::new(x, width / 2.0 + off)).unwrap()
        })
        .collect();
    let (first, end) = (points[0], points[last]);
    let spline = sketch
        .add_curve(Curve::Spline(Spline::through(points, false)), false)
        .unwrap();
    let corner = |sketch: &mut Sketch, x| sketch.add_point(DVec2::new(x, 0.0)).unwrap();
    let (b0, b1) = (corner(sketch, 0.0), corner(sketch, width));
    for (start, end) in [(end, b1), (b1, b0), (b0, first)] {
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    spline
}

/// Whether `profile` extrudes, at the tolerance `fit`.
fn extrudes(profile: &Profile, fit: f64) -> Result<(), varde_kernel::KernelError> {
    let tolerance = varde_kernel::Tolerance::new(fit).unwrap();
    let frame = varde_kernel::Frame::XY;
    let budget = varde_kernel::Budget::DEFAULT;
    varde_kernel::extrude(profile, &frame, 0.0, 10.0, 1, &tolerance, &budget).map(|_| ())
}

#[test]
fn a_spline_straight_within_the_tolerance_is_one_segment() {
    // 100 fit points over 1 mm, all within 0.0005 of a straight line.
    let mut sketch = Sketch::default();
    let curve = wavy_plate(&mut sketch, 100, 1.0, 0.001, 7);
    let id = u64::from(curve.get());
    let (profile, _) = picked_within(&sketch, |_| true, 0.1);
    let fitted = profile.loops[0]
        .segments
        .iter()
        .filter(|s| s.curve == id)
        .count();
    assert!(fitted <= 2, "{fitted} segments");
    extrudes(&profile, 0.1).unwrap();
    // Fine, it still follows every wiggle.
    let fit = 1e-5;
    let (profile, _) = picked_within(&sketch, |_| true, fit);
    let error = largest_error(&sketch, curve, &profile.loops[0]);
    assert!(error <= fit / 2.0, "{error}");
}

#[test]
fn dense_wavy_edges_extrude_at_coarse_tolerances() {
    let mut refused = Vec::new();
    for width in [1.0, 5.0] {
        for slope in [0.1, 0.3] {
            for seed in 0..3 {
                let n = 100;
                let spacing = width / (n - 1) as f64;
                let mut sketch = Sketch::default();
                wavy_plate(&mut sketch, n, width, spacing * slope, seed);
                let (profile, _) = picked_within(&sketch, |_| true, 0.1);
                if let Err(e) = extrudes(&profile, 0.1) {
                    refused.push(format!("{width} {slope} {seed}: {e:?}"));
                }
            }
        }
    }
    assert!(refused.is_empty(), "{refused:#?}");
}

#[test]
#[ignore = "refused until the kernel refines its caps for quality"]
fn steep_dense_wavy_edges_extrude_at_coarse_tolerances() {
    // Wiggles as steep as they are long, 100 fit points, at fits of 1e-2
    // and 0.1: real detail near the tolerance, which the fitter keeps as
    // fine chains of nearly straight conics and lines. The kernel's plain
    // caps of such chains hold triangles thinner than the resolution
    // (`Invalid`) or ask to halve a concave piece at a narrow corner
    // until it is too small (`TooFine`). Also 30 points over 1 mm at
    // 1e-2. Today 9 of the 10 at width 5 and fit 1e-2 are refused, and
    // the 30 points; a debug build runs the first three seeds.
    let mut cases = Vec::new();
    let seeds = if cfg!(debug_assertions) { 3 } else { 10 };
    for width in [1.0, 5.0, 20.0] {
        for seed in 0..seeds {
            for fit in [1e-2, 0.1] {
                cases.push((100, width, seed, fit));
            }
        }
    }
    cases.push((30, 1.0, 0, 1e-2));
    let mut refused = Vec::new();
    for (n, width, seed, fit) in cases {
        let spacing = width / (n - 1) as f64;
        let mut sketch = Sketch::default();
        wavy_plate(&mut sketch, n, width, spacing, seed);
        let (profile, _) = picked_within(&sketch, |_| true, fit);
        let tolerance = varde_kernel::Tolerance::new(fit).unwrap();
        let (frame, budget) = (varde_kernel::Frame::XY, varde_kernel::Budget::DEFAULT);
        match varde_kernel::extrude(&profile, &frame, 0.0, 10.0, 1, &tolerance, &budget) {
            Ok(solid) => {
                let exact = profile.area() * 10.0;
                let volume = solid.volume();
                assert!(
                    (volume - exact).abs() < 1e-9 * exact,
                    "{volume}, not {exact}"
                );
            }
            Err(e) => refused.push(format!("n {n} width {width} seed {seed} fit {fit}: {e:?}")),
        }
    }
    assert!(
        refused.is_empty(),
        "{} refused: {refused:#?}",
        refused.len()
    );
}

/// A closed spline through `n` points on a sliver `2·height` across and
/// 2 long, bent by `bend` (as a fraction of the height) into an S;
/// self-crossing, two lobes, if `bend` is 1.
fn sliver(sketch: &mut Sketch, n: usize, height: f64, bend: f64) {
    let points: Vec<_> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let y = height * (a.sin() + bend * (2.0 * a).sin());
            sketch.add_point(DVec2::new(a.cos(), y)).unwrap()
        })
        .collect();
    sketch
        .add_curve(Curve::Spline(Spline::through(points, true)), false)
        .unwrap();
}

#[test]
fn a_sliver_thinner_than_the_tolerance_keeps_its_shape() {
    // A region narrower than the tolerance is still a region: no run of
    // it may be a line doubling back on the one before, collapsing the
    // loop to a cusp or no area at all. Every one of these extrudes
    // with each span fitted alone. The last has a tip a single span
    // fits only as a line, sharp but not doubling back.
    let cases = [
        (4, 0.001, 0.0),
        (6, 0.001, 0.0),
        (6, 0.01, 0.5),
        (8, 0.01, 1.0),
        (8, 0.005, 0.0),
        (12, 0.03, 1.0),
        (20, 0.01, 0.0),
        (40, 0.005, 1.0),
        (20, 0.02, 0.5),
    ];
    let fit = 0.1;
    let mut refused = Vec::new();
    for (n, height, bend) in cases {
        let mut sketch = Sketch::default();
        sliver(&mut sketch, n, height, bend);
        let profiles = sketch.profiles().unwrap();
        for region in 0..profiles.regions.len() {
            let loops = profiles.merge(&[region]).unwrap();
            let fitted = profile(&sketch, &profiles, &loops, fit).unwrap();
            if let Err(e) = extrudes(&fitted, fit) {
                refused.push(format!("{n} {height} {bend} r{region}: {e:?}"));
            }
        }
    }
    assert!(refused.is_empty(), "{refused:#?}");
}

#[test]
fn a_reversed_piece_fits_to_the_same_conics() {
    // A rectangle split by a spline from its left side to its right: the
    // region below runs along the spline backwards, the one above
    // forwards. A closed spline in the upper region is the outer loop of
    // its own region and a hole of the one round it.
    let mut sketch = Sketch::default();
    let points = [
        (-6.0, 0.0),
        (-3.0, 1.3),
        (0.0, -0.7),
        (2.0, 0.4),
        (6.0, -0.2),
    ]
    .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let spline = sketch
        .add_curve(
            Curve::Spline(Spline::through(points.to_vec(), false)),
            false,
        )
        .unwrap();
    let corners = [(6.0, 8.0), (-6.0, 8.0), (-6.0, -8.0), (6.0, -8.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let ring = [
        points[4], corners[0], corners[1], points[0], corners[2], corners[3],
    ];
    for k in 0..ring.len() {
        let (start, end) = (ring[k], ring[(k + 1) % ring.len()]);
        sketch.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let blob_points = [(3.0, 4.0), (1.0, 6.0), (-2.0, 5.0), (-3.0, 3.0), (0.5, 2.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let blob = sketch
        .add_curve(
            Curve::Spline(Spline::through(blob_points.to_vec(), true)),
            false,
        )
        .unwrap();
    for fit in [1e-1, 1e-3, 1e-5] {
        let of = |profile: &Profile, curve: varde_sketch::Id| -> Vec<Conic2> {
            let id = u64::from(curve.get());
            let segments = profile.loops.iter().flat_map(|l| &l.segments);
            segments
                .filter(|s| s.curve == id)
                .map(|s| s.conic)
                .collect()
        };
        assert_eq!(sketch.profiles().unwrap().regions.len(), 3);
        let (below, _) = picked_within(&sketch, |r| r.bounds.1.y < 4.0, fit);
        let (above, _) = picked_within(&sketch, |r| r.holes.len() == 1, fit);
        let (inside, _) = picked_within(&sketch, |r| r.holes.is_empty() && r.bounds.0.y > 1.0, fit);
        // The spline piece, forwards above, backwards below.
        let forwards = of(&above, spline);
        let backwards: Vec<Conic2> = of(&below, spline)
            .iter()
            .rev()
            .map(Conic2::reversed)
            .collect();
        assert!(!forwards.is_empty());
        assert_eq!(forwards, backwards, "{fit}");
        // The closed spline, counter-clockwise inside, clockwise as the hole.
        let outer = of(&inside, blob);
        let hole: Vec<Conic2> = of(&above, blob)
            .iter()
            .rev()
            .map(Conic2::reversed)
            .collect();
        assert!(!outer.is_empty());
        assert_eq!(outer, hole, "{fit}");
    }
}

#[test]
fn pieces_naming_what_the_sketch_lacks_are_refused() {
    let mut sketch = Sketch::default();
    blob(&mut sketch);
    let profiles = sketch.profiles().unwrap();
    let loops = profiles.merge(&[0]).unwrap();
    let mut gone = profiles.clone();
    gone.vertices.clear();
    assert_eq!(
        profile(&sketch, &gone, &loops, FIT),
        Err(ProfileError::Missing)
    );
    assert_eq!(
        profile(&Sketch::default(), &profiles, &loops, FIT),
        Err(ProfileError::Missing)
    );
}

#[test]
fn errors_display() {
    for error in [
        ProfileError::Missing,
        ProfileError::TooManySegments,
        ProfileError::Fit,
        ProfileError::Patch(varde_kernel::patch::PatchError::Degenerate),
    ] {
        assert!(!error.to_string().is_empty());
    }
}

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
        // Where conics meet, they leave along the same line.
        let n = l.segments.len();
        for k in 0..n {
            let (a, b) = (&l.segments[k].conic, &l.segments[(k + 1) % n].conic);
            assert_eq!(a.p1, b.p0);
            let (out, on) = ((a.p1 - a.c).normalize(), (b.c - b.p0).normalize());
            assert!(out.perp_dot(on).abs() < 1e-3, "{fit}: {k} turns");
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

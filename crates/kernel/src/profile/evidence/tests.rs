#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::TAU;

use glam::{DVec2, DVec3};

use super::*;
use crate::mesh::tests::TOL;
use crate::profile::tests::{arc, circle, polygon, rect};
use crate::profile::{Loop, MAX_PROFILE_SEGMENTS};
use crate::{Budget, Failure, MAX_EVIDENCE, Sweep, extrude, revolve};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn profile(loops: Vec<Loop>) -> Profile {
    Profile { loops }
}

/// A frame off the origin and turned: the profile's `x` along world `y`,
/// its `y` along world `z`, the normal along `x`.
const TURNED: Frame = Frame {
    origin: DVec3::new(1.0, 2.0, 3.0),
    x: DVec3::Y,
    y: DVec3::Z,
};

fn extruding(p: &Profile) -> Failure {
    extrude(p, &TURNED, 0.0, 1.0, 9, &TOL, &Budget::DEFAULT).unwrap_err()
}

fn revolving(p: &Profile, sweep: Sweep) -> Failure {
    revolve(p, &TURNED, sweep, 9, &TOL, &Budget::DEFAULT).unwrap_err()
}

/// Segment `s` of loop `l` placed on `frame`.
fn placed_on(frame: &Frame, p: &Profile, l: usize, s: usize) -> Conic3 {
    let c = p.loops[l].segments[s].conic;
    Conic3::new(
        frame.point(c.p0, 0.0),
        frame.point(c.c, 0.0),
        c.w,
        frame.point(c.p1, 0.0),
    )
    .unwrap()
}

fn placed(p: &Profile, l: usize, s: usize) -> Conic3 {
    placed_on(&TURNED, p, l, s)
}

/// The sketch curves of the segments `at`, each once, in order.
fn curves_of(p: &Profile, at: &[(usize, usize)]) -> Vec<u64> {
    let mut out = Vec::new();
    for &(l, s) in at {
        let c = p.loops[l].segments[s].curve;
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Checks the failure is `error` and its evidence the segments `at` (in
/// order), placed on [`TURNED`], with their sketch curves.
fn segments_are(f: &Failure, error: ProfileError, p: &Profile, at: &[(usize, usize)]) {
    assert_eq!(f.error, KernelError::Profile(error));
    let curves: Vec<Conic3> = at.iter().map(|&(l, s)| placed(p, l, s)).collect();
    assert_eq!(f.evidence.curves, curves, "{error:?}");
    assert_eq!(f.evidence.sketch_curves, curves_of(p, at), "{error:?}");
    assert!(f.evidence.within_caps() && !f.evidence.truncated);
}

/// The distance from `q` to the placed segment, by its parameter:
/// sampled, then narrowed.
fn distance_to(c: &Conic3, q: DVec3) -> f64 {
    let n = 4096;
    let (mut best, mut at) = (f64::INFINITY, 0.0);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let d = c.eval(t).distance(q);
        if d < best {
            (best, at) = (d, t);
        }
    }
    let mut h = 1.0 / n as f64;
    for _ in 0..60 {
        for t in [at - h, at + h] {
            let t = t.clamp(0.0, 1.0);
            let d = c.eval(t).distance(q);
            if d < best {
                (best, at) = (d, t);
            }
        }
        h *= 0.5;
    }
    best
}

#[test]
fn an_empty_profile_gives_nothing() {
    let f = extruding(&Profile::default());
    assert_eq!(f.error, KernelError::Profile(ProfileError::Empty));
    assert!(f.evidence.is_empty());
}

#[test]
fn too_many_segments_give_the_first_up_to_the_caps() {
    let n = MAX_PROFILE_SEGMENTS + 1;
    let points: Vec<DVec2> = (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            v(a.cos(), a.sin()) * 1000.0
        })
        .collect();
    let p = profile(vec![polygon(&points, 0)]);
    let f = extruding(&p);
    assert_eq!(
        f.error,
        KernelError::Profile(ProfileError::TooManySegments(n))
    );
    let e = &f.evidence;
    assert!(e.truncated && e.within_caps());
    assert_eq!(e.curves.len(), MAX_EVIDENCE.curves);
    assert_eq!(e.sketch_curves.len(), MAX_EVIDENCE.sketch_curves);
    assert_eq!(e.curves[7], placed(&p, 0, 7));
    assert_eq!(e.sketch_curves[7], 7);
}

#[test]
fn a_short_loop_and_one_of_no_area_give_the_loop() {
    let line = |a, b, c| Segment::line(a, b, c).unwrap();
    let short = profile(vec![
        rect(v(0.0, 0.0), v(1.0, 1.0), 0),
        Loop {
            segments: vec![line(v(5.0, 0.0), v(6.0, 0.0), 7)],
        },
    ]);
    segments_are(
        &extruding(&short),
        ProfileError::Short(1),
        &short,
        &[(1, 0)],
    );
    let flat = profile(vec![Loop {
        segments: vec![
            line(v(0.0, 0.0), v(2.0, 0.0), 3),
            line(v(2.0, 0.0), v(0.0, 0.0), 4),
        ],
    }]);
    segments_are(
        &extruding(&flat),
        ProfileError::Area(0),
        &flat,
        &[(0, 0), (0, 1)],
    );
}

#[test]
fn a_bad_segment_gives_its_sketch_curve_and_its_curve_where_it_can() {
    // Past the coordinate limit: nothing to draw, but the sketch curve.
    let far = f64::from(crate::MAX_COORD) * 2.0;
    let p = profile(vec![polygon(&[v(0.0, 0.0), v(far, 0.0), v(0.0, 1.0)], 5)]);
    let f = extruding(&p);
    assert!(matches!(
        f.error,
        KernelError::Profile(ProfileError::Segment(0, 0, _))
    ));
    assert!(f.evidence.curves.is_empty() && f.evidence.points.is_empty());
    assert_eq!(f.evidence.sketch_curves, [5]);

    // A straight segment running back on itself, found by the chain.
    let back = profile(vec![Loop {
        segments: vec![
            Segment {
                conic: Conic2::new(DVec2::ZERO, v(2.0, 0.0), 1.0, DVec2::X).unwrap(),
                curve: 0,
            },
            Segment::line(DVec2::X, DVec2::ONE, 1).unwrap(),
            Segment::line(DVec2::ONE, DVec2::ZERO, 2).unwrap(),
        ],
    }]);
    segments_are(
        &extruding(&back),
        ProfileError::Degenerate(0, 0),
        &back,
        &[(0, 0)],
    );
}

#[test]
fn an_open_loop_gives_the_gap() {
    let line = |a, b, c| Segment::line(a, b, c).unwrap();
    let p = profile(vec![Loop {
        segments: vec![
            line(v(0.0, 0.0), v(2.0, 0.0), 1),
            line(v(2.0, 0.0), v(2.0, 2.0), 2),
            line(v(2.1, 2.0), v(0.0, 0.0), 3),
        ],
    }]);
    let f = extruding(&p);
    segments_are(&f, ProfileError::Open(0, 1), &p, &[(0, 1), (0, 2)]);
    assert_eq!(
        f.evidence.points,
        [
            TURNED.point(v(2.0, 2.0), 0.0),
            TURNED.point(v(2.1, 2.0), 0.0)
        ]
    );
    // The last segment's gap is to the first.
    let p = profile(vec![Loop {
        segments: vec![
            line(v(0.0, 0.0), v(2.0, 0.0), 1),
            line(v(2.0, 0.0), v(2.0, 2.0), 2),
            line(v(2.0, 2.0), v(0.0, 0.1), 3),
        ],
    }]);
    let f = extruding(&p);
    segments_are(&f, ProfileError::Open(0, 2), &p, &[(0, 2), (0, 0)]);
}

#[test]
fn a_cusp_gives_its_vertex_and_the_segments_round_it() {
    let cusp = profile(vec![Loop {
        segments: vec![
            arc(v(0.0, 3.0), v(0.0, 0.0), v(3.0, 3.0), 0),
            Segment::line(v(3.0, 3.0), v(1.0, 1.0), 1).unwrap(),
            arc(v(0.0, 1.0), v(1.0, 1.0), v(0.0, 0.0), 2),
        ],
    }]);
    let f = extruding(&cusp);
    segments_are(&f, ProfileError::Cusp(0, 0), &cusp, &[(0, 2), (0, 0)]);
    assert_eq!(f.evidence.points, [TURNED.point(DVec2::ZERO, 0.0)]);
}

#[test]
fn crossing_loops_give_where_they_cross() {
    // Two circles crossing at (0.75, ±√(1 − 0.75²)).
    let p = profile(vec![
        circle(v(0.0, 0.0), 1.0, 1, false),
        circle(v(1.5, 0.0), 1.0, 2, false),
    ]);
    let f = extruding(&p);
    let KernelError::Profile(ProfileError::Touching([a, b])) = f.error else {
        panic!("{:?}", f.error);
    };
    assert_eq!((a.0, b.0), (0, 1));
    segments_are(&f, ProfileError::Touching([a, b]), &p, &[a, b]);
    let [q] = f.evidence.points[..] else {
        panic!("{:?}", f.evidence.points);
    };
    // On both circles and both segments.
    let local = q - TURNED.origin;
    let (x, y) = (local.y, local.z);
    assert!((v(x, y).length() - 1.0).abs() < 1e-9, "{q}");
    assert!((v(x - 1.5, y).length() - 1.0).abs() < 1e-9, "{q}");
    assert!(distance_to(&placed(&p, a.0, a.1), q) < 1e-9);
    assert!(distance_to(&placed(&p, b.0, b.1), q) < 1e-9);
}

#[test]
fn loops_too_close_give_both_nearest_points() {
    // Two squares half the resolution apart, side by side.
    let gap = 0.5 * TOL.resolution();
    let p = profile(vec![
        rect(v(0.0, 0.0), v(1.0, 1.0), 1),
        rect(v(1.0 + gap, 0.25), v(2.0, 0.75), 5),
    ]);
    let f = extruding(&p);
    let KernelError::Profile(ProfileError::Touching([a, b])) = f.error else {
        panic!("{:?}", f.error);
    };
    // The first square's right side and a side of the second meeting it.
    assert_eq!(a, (0, 1));
    assert_eq!(b.0, 1);
    segments_are(&f, ProfileError::Touching([a, b]), &p, &[a, b]);
    let [qa, qb] = f.evidence.points[..] else {
        panic!("{:?}", f.evidence.points);
    };
    assert!(distance_to(&placed(&p, a.0, a.1), qa) < 1e-12);
    assert!(distance_to(&placed(&p, b.0, b.1), qb) < 1e-12);
    assert!((qa.distance(qb) - gap).abs() < 1e-3 * gap, "{qa} {qb}");
}

#[test]
fn loops_that_dont_nest_give_the_loop_named() {
    let inside = profile(vec![
        rect(v(0.0, 0.0), v(10.0, 10.0), 0),
        rect(v(4.0, 4.0), v(6.0, 6.0), 4),
    ]);
    segments_are(
        &extruding(&inside),
        ProfileError::Nesting(1),
        &inside,
        &[(1, 0), (1, 1), (1, 2), (1, 3)],
    );
    let wrong_hole = profile(vec![
        rect(v(0.0, 0.0), v(10.0, 10.0), 0),
        circle(v(5.0, 5.0), 1.0, 4, false),
    ]);
    segments_are(
        &extruding(&wrong_hole),
        ProfileError::Nesting(1),
        &wrong_hole,
        &[(1, 0), (1, 1), (1, 2), (1, 3)],
    );
}

#[test]
fn errors_naming_the_whole_profile_or_a_segment_give_them() {
    // Triangulation and TooFine aren't easily made to happen: the
    // evidence from the error alone.
    let p = profile(vec![
        rect(v(0.0, 0.0), v(10.0, 10.0), 0),
        circle(v(5.0, 5.0), 1.0, 4, true),
    ]);
    let all: Vec<(usize, usize)> = (0..2).flat_map(|l| (0..4).map(move |s| (l, s))).collect();
    let error = KernelError::Profile(ProfileError::Triangulation);
    segments_are(
        &profile_failure(error, &p, &TURNED),
        ProfileError::Triangulation,
        &p,
        &all,
    );
    let error = KernelError::Profile(ProfileError::TooFine(1, 2));
    segments_are(
        &profile_failure(error, &p, &TURNED),
        ProfileError::TooFine(1, 2),
        &p,
        &[(1, 2)],
    );
    // Indices past the profile give nothing, and other errors none.
    let error = KernelError::Profile(ProfileError::TooFine(2, 0));
    assert!(profile_failure(error, &p, &TURNED).evidence.is_empty());
    let error = KernelError::TooComplex;
    assert!(profile_failure(error, &p, &TURNED).evidence.is_empty());
}

#[test]
fn a_frame_unfit_to_place_by_gives_only_sketch_curves() {
    let p = profile(vec![
        rect(v(0.0, 0.0), v(10.0, 10.0), 0),
        rect(v(4.0, 4.0), v(6.0, 6.0), 4),
    ]);
    let skewed = Frame {
        x: DVec3::new(1.0, 1.0, 0.0),
        ..Frame::XY
    };
    let f = profile_failure(KernelError::Profile(ProfileError::Nesting(1)), &p, &skewed);
    assert!(f.evidence.curves.is_empty());
    assert_eq!(f.evidence.sketch_curves, [4, 5, 6, 7]);
}

#[test]
fn a_profile_crossing_the_axis_gives_the_axis_across_it() {
    let across = profile(vec![rect(v(-1.0, -2.0), v(3.0, 1.0), 1)]);
    for sweep in [Sweep::Full, Sweep::Part { from: 0.0, to: 1.0 }] {
        let f = revolving(&across, sweep);
        assert_eq!(
            f.error,
            KernelError::Profile(ProfileError::CrossesAxis(0, 0))
        );
        let axis = Conic3::line(
            TURNED.point(v(0.0, -2.0), 0.0),
            TURNED.point(v(0.0, 1.0), 0.0),
        );
        assert_eq!(f.evidence.curves, [placed(&across, 0, 0), axis.unwrap()]);
        assert_eq!(f.evidence.sketch_curves, [1]);
    }
}

#[test]
fn a_profile_touching_the_axis_gives_where() {
    // A vertex alone on the axis, in a full turn.
    let diamond = profile(vec![polygon(
        &[v(0.0, 0.0), v(2.0, -1.0), v(4.0, 0.0), v(2.0, 1.0)],
        1,
    )]);
    let f = revolving(&diamond, Sweep::Full);
    segments_are(
        &f,
        ProfileError::TouchesAxis(0, 0),
        &diamond,
        &[(0, 3), (0, 0)],
    );
    assert_eq!(f.evidence.points, [TURNED.point(DVec2::ZERO, 0.0)]);
    // A parabola touching it half way along, `x = (1 − 2t)²`.
    let line = |a, b, curve| Segment::line(a, b, curve).unwrap();
    let touching = profile(vec![Loop {
        segments: vec![
            line(v(1.0, 0.0), v(3.0, 0.0), 1),
            line(v(3.0, 0.0), v(3.0, 2.0), 2),
            line(v(3.0, 2.0), v(1.0, 2.0), 3),
            Segment {
                conic: Conic2::new(v(1.0, 2.0), v(-1.0, 1.0), 1.0, v(1.0, 0.0)).unwrap(),
                curve: 4,
            },
        ],
    }]);
    let f = revolving(&touching, Sweep::Full);
    segments_are(&f, ProfileError::TouchesAxis(0, 3), &touching, &[(0, 3)]);
    let [q] = f.evidence.points[..] else {
        panic!("{:?}", f.evidence.points);
    };
    assert!(q.distance(TURNED.point(v(0.0, 1.0), 0.0)) < 1e-6, "{q}");
    assert!(distance_to(&placed(&touching, 0, 3), q) < 1e-12);
}

#[test]
fn a_nearly_full_turn_gives_the_profile_at_both_ends() {
    let washer = profile(vec![rect(v(1.0, 0.0), v(3.0, 1.0), 1)]);
    let (from, to) = (0.5, 0.5 + TAU - 1e-10);
    let f = revolving(&washer, Sweep::Part { from, to });
    assert_eq!(f.error, KernelError::Profile(ProfileError::NearlyFullTurn));
    let e = &f.evidence;
    assert_eq!(e.curves.len(), 8);
    assert_eq!(e.sketch_curves, [1, 2, 3, 4]);
    // The profile turned to `from` and to `to`, about the axis (world
    // `z` through the origin): world `x` here is the frame's `x × y`.
    for (i, angle) in [(0, from), (4, to)] {
        let frame = Frame {
            origin: TURNED.origin,
            x: DVec3::Y * angle.cos() + DVec3::X * angle.sin(),
            y: DVec3::Z,
        };
        for s in 0..4 {
            let want = placed_on(&frame, &washer, 0, s);
            let got = e.curves[i + s];
            for (a, b) in [(got.p0, want.p0), (got.c, want.c), (got.p1, want.p1)] {
                assert!(a.distance(b) < 1e-12, "{a} {b}");
            }
        }
    }
}

#[test]
fn the_error_and_its_evidence_are_the_same_every_time() {
    let p = profile(vec![
        circle(v(0.0, 0.0), 1.0, 1, false),
        circle(v(1.5, 0.0), 1.0, 2, false),
    ]);
    let f = crate::par::assert_deterministic(|| extruding(&p));
    assert_eq!(f, extruding(&p));
    let q = profile(vec![rect(v(1.0, 0.0), v(3.0, 1.0), 1)]);
    let sweep = Sweep::Part {
        from: 0.0,
        to: TAU - 1e-10,
    };
    crate::par::assert_deterministic(|| revolving(&q, sweep));
}

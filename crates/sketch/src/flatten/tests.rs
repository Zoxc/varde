#![allow(
    clippy::disallowed_methods,
    reason = "std maths as an independent reference, or to build inputs"
)]

use std::f64::consts::{FRAC_PI_2, PI};

use super::*;
use crate::Id;

/// A sketch with points at `at`, and their ids.
fn points<const N: usize>(at: [DVec2; N]) -> (Sketch, [Id; N]) {
    let mut sketch = Sketch::default();
    let ids = at.map(|at| sketch.add_point(at).unwrap());
    (sketch, ids)
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.abs_diff_eq(b, 1e-9)
}

#[test]
fn line_is_its_two_points() {
    let (a, b) = (DVec2::new(1.0, 2.0), DVec2::new(-3.0, 4.0));
    let (sketch, [start, end]) = points([a, b]);
    assert_eq!(
        sketch.flatten(&Curve::Line { start, end }),
        Some(vec![a, b])
    );
}

#[test]
fn circle_is_closed_and_on_the_circle() {
    let center = DVec2::new(3.0, -2.0);
    let (sketch, [id]) = points([center]);
    let polyline = sketch
        .flatten(&Curve::Circle {
            center: id,
            radius: 5.0,
        })
        .unwrap();
    assert_eq!(polyline.len(), CIRCLE_SEGMENTS + 1);
    assert_eq!(polyline.first(), polyline.last());
    assert!(close(polyline[0], center + DVec2::new(5.0, 0.0)));
    for point in &polyline {
        assert!((point.distance(center) - 5.0).abs() < 1e-9, "{point}");
    }
    // Counter-clockwise.
    assert!((polyline[1] - center).perp_dot(polyline[2] - center) > 0.0);
}

#[test]
fn arc_goes_counter_clockwise_through_its_ends() {
    let center = DVec2::new(1.0, 1.0);
    let (a, b) = (center + DVec2::X * 2.0, center + DVec2::Y * 2.0);
    let (sketch, [c, s, e]) = points([center, a, b]);

    // A quarter turn from +x to +y.
    let quarter = sketch
        .flatten(&Curve::Arc {
            center: c,
            start: s,
            end: e,
        })
        .unwrap();
    assert_eq!(quarter.len(), CIRCLE_SEGMENTS / 4 + 1);
    assert_eq!((quarter[0], *quarter.last().unwrap()), (a, b));
    for point in &quarter {
        let offset = *point - center;
        assert!((offset.length() - 2.0).abs() < 1e-9, "{point}");
        assert!(offset.x >= -1e-9 && offset.y >= -1e-9, "{point}");
    }

    // The other way round is the other three quarters.
    let rest = sketch
        .flatten(&Curve::Arc {
            center: c,
            start: e,
            end: s,
        })
        .unwrap();
    assert_eq!(rest.len(), CIRCLE_SEGMENTS * 3 / 4 + 1);
    assert_eq!((rest[0], *rest.last().unwrap()), (b, a));
    let middle = rest[rest.len() / 2] - center;
    assert!(close(middle, DVec2::from_angle(PI + FRAC_PI_2 / 2.0) * 2.0));
}

#[test]
fn arc_ending_where_it_starts_is_a_whole_turn() {
    let center = DVec2::ZERO;
    let (a, b) = (DVec2::new(1.0, 0.0), DVec2::new(3.0, 0.0));
    let (sketch, [c, s, e]) = points([center, a, b]);
    let polyline = sketch
        .flatten(&Curve::Arc {
            center: c,
            start: s,
            end: e,
        })
        .unwrap();
    assert_eq!(polyline.len(), CIRCLE_SEGMENTS + 1);
    // The radius goes from the start's to the end's.
    assert_eq!((polyline[0], *polyline.last().unwrap()), (a, b));
    let halfway = polyline[CIRCLE_SEGMENTS / 2];
    assert!(close(halfway, DVec2::new(-2.0, 0.0)), "{halfway}");
}

#[test]
fn tiny_arc_is_one_segment() {
    let center = DVec2::ZERO;
    let (a, b) = (DVec2::from_angle(0.0), DVec2::from_angle(1e-6));
    let (sketch, [c, s, e]) = points([center, a, b]);
    let polyline = sketch.flatten(&Curve::Arc {
        center: c,
        start: s,
        end: e,
    });
    assert_eq!(polyline, Some(vec![a, b]));
}

#[test]
fn degenerate_arcs_stay_bounded_and_finite() {
    let max = 1e6;
    let corners = [DVec2::splat(-max), DVec2::splat(max), DVec2::new(max, -max)];
    // A zero radius, and the largest a checked sketch can have.
    for (center, start, end) in [
        (DVec2::ZERO, DVec2::ZERO, DVec2::X),
        (DVec2::ZERO, DVec2::ZERO, DVec2::ZERO),
        (corners[0], corners[1], corners[2]),
    ] {
        let (sketch, [c, s, e]) = points([center, start, end]);
        let polyline = sketch
            .flatten(&Curve::Arc {
                center: c,
                start: s,
                end: e,
            })
            .unwrap();
        assert!((2..=CIRCLE_SEGMENTS + 1).contains(&polyline.len()));
        for point in polyline {
            assert!(point.is_finite(), "{point}");
            // Within a point's bound plus the largest radius.
            assert!(point.abs().max_element() < 4.0 * max, "{point}");
        }
    }
}

#[test]
fn missing_points_flatten_to_nothing() {
    let (sketch, [a]) = points([DVec2::ZERO]);
    let mut other = sketch.clone();
    let missing = other.add_point(DVec2::X).unwrap();
    assert_eq!(
        sketch.flatten(&Curve::Line {
            start: a,
            end: missing
        }),
        None
    );
    assert_eq!(
        sketch.flatten(&Curve::Circle {
            center: missing,
            radius: 1.0
        }),
        None
    );
    assert_eq!(
        sketch.flatten(&Curve::Arc {
            center: a,
            start: a,
            end: missing
        }),
        None
    );
}

#[test]
fn arcs_never_have_more_than_a_circle_of_segments() {
    for sweep in [0.0, 1e-300, 0.5, PI, TAU - 1e-12, TAU, f64::NAN] {
        let segments = arc_segments(sweep);
        assert!((1..=CIRCLE_SEGMENTS).contains(&segments), "{sweep}");
    }
    assert_eq!(arc_segments(TAU), CIRCLE_SEGMENTS);
}

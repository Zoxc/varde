use super::*;

const STYLE: LineStyle = LineStyle {
    color: Srgba([1.0, 0.0, 0.0, 1.0]),
    width: 2.0,
    dash: None,
};

fn at(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// The segments of the polyline through `points`, drawn in [`STYLE`].
fn polyline(points: &[DVec2]) -> Vec<LineInstance> {
    let mut layer = SketchLayer::default();
    layer.polyline(Space::Sketch, points, STYLE);
    layer.lines
}

#[test]
fn a_polyline_is_a_segment_each_knowing_its_neighbours() {
    let points = [at(0.0, 0.0), at(3.0, 0.0), at(3.0, 4.0), at(0.0, 4.0)];
    let segments = polyline(&points);
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].flags, HAS_NEXT);
    assert_eq!(segments[1].flags, HAS_PREV | HAS_NEXT);
    assert_eq!(segments[2].flags, HAS_PREV);
    assert_eq!(segments[1].ends, [3.0, 0.0, 3.0, 4.0]);
    assert_eq!(segments[1].neighbours, [0.0, 0.0, 0.0, 4.0]);
    // Its length runs on from one segment to the next, for the dashes.
    let along: Vec<_> = segments.iter().map(|s| s.along).collect();
    assert_eq!(along, [[0.0, 3.0], [3.0, 7.0], [7.0, 10.0]]);
}

#[test]
fn a_polyline_ending_where_it_starts_is_closed() {
    let points = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 0.0)];
    let segments = polyline(&points);
    assert!(segments.iter().all(|s| s.flags == HAS_PREV | HAS_NEXT));
    // The first segment comes after the last, and the other way round.
    assert_eq!(segments[0].neighbours[..2], [1.0, 1.0]);
    assert_eq!(segments[2].neighbours[2..], [1.0, 0.0]);
    // Back and forth on one segment isn't a loop.
    let there_and_back = polyline(&[at(0.0, 0.0), at(1.0, 0.0), at(0.0, 0.0)]);
    assert_eq!(there_and_back[0].flags, HAS_NEXT);
}

#[test]
fn repeated_and_broken_points_are_left_out() {
    let repeated = polyline(&[at(0.0, 0.0), at(0.0, 0.0), at(1.0, 0.0), at(1.0, 0.0)]);
    assert_eq!(repeated.len(), 1);
    assert_eq!(repeated[0].flags, 0);
    assert!(polyline(&[at(1.0, 1.0), at(1.0, 1.0)]).is_empty());
    assert!(polyline(&[at(1.0, 1.0)]).is_empty());
    assert!(polyline(&[at(0.0, 0.0), at(f64::NAN, 1.0), at(1.0, 0.0)]).is_empty());
    assert!(polyline(&[at(0.0, 0.0), at(f64::INFINITY, 1.0)]).is_empty());
}

#[test]
fn styles_carry_to_the_gpu() {
    let mut layer = SketchLayer::default();
    let dashed = LineStyle {
        dash: Some([6.0, 4.0]),
        ..STYLE
    };
    let no_gap = LineStyle {
        dash: Some([6.0, 0.0]),
        ..STYLE
    };
    for style in [dashed, no_gap] {
        layer.polyline(Space::Screen, &[at(0.0, 0.0), at(10.0, 0.0)], style);
    }
    assert_eq!(layer.lines[0].style, [2.0, 6.0, 4.0, 0.0]);
    assert_eq!(layer.lines[0].flags, SCREEN);
    // A dash without a gap is a solid line.
    assert_eq!(layer.lines[1].style, [2.0, 0.0, 0.0, 0.0]);
    // In linear colour, alpha as it is.
    let half = Srgba([0.5, 1.0, 0.0, 0.25]).linear();
    assert!((half[0] - 0.214).abs() < 1e-3, "{half:?}");
    assert_eq!(half[1..], [1.0, 0.0, 0.25]);

    let rim = Srgba([0.0, 0.0, 1.0, 1.0]);
    let style = PointStyle {
        radius: 3.0,
        rim_width: 1.0,
        rim,
        fill: Srgba([1.0; 4]),
        fixed: false,
    };
    layer.point(at(1.0, 2.0), style);
    layer.point(
        at(1.0, 2.0),
        PointStyle {
            fixed: true,
            ..style
        },
    );
    layer.point(at(f64::NAN, 2.0), style);
    assert_eq!(layer.points.len(), 2);
    assert_eq!(layer.points[0].fill, [1.0; 4]);
    // A fixed point is filled with its rim's colour.
    assert_eq!(layer.points[1].fill, rim.linear());
    assert_eq!(layer.points[1].size, [3.0, 1.0]);
}

/// The area of `fills`' triangles.
fn area(fills: &[FillVertex]) -> f64 {
    fills
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|corner| DVec2::from(corner.at.map(f64::from)));
            (b - a).perp_dot(c - a).abs() / 2.0
        })
        .sum()
}

#[test]
fn a_fill_leaves_out_its_holes() {
    let square = |min: f64, max: f64| [at(min, min), at(max, min), at(max, max), at(min, max)];
    let (outer, hole) = (square(0.0, 10.0), square(2.0, 6.0));
    let color = Srgba([0.0, 0.0, 1.0, 0.5]);
    let mut layer = SketchLayer::default();
    layer.fill(Space::Sketch, [&outer[..], &hole[..]], color);
    assert!(!layer.fills.is_empty() && layer.fills.len() % 3 == 0);
    assert!(
        (area(&layer.fills) - 84.0).abs() < 1e-6,
        "{}",
        area(&layer.fills)
    );
    assert!(
        layer
            .fills
            .iter()
            .all(|v| v.color == color.linear() && v.flags == 0)
    );

    // Too short a contour is left out, and a broken one leaves out all.
    let mut layer = SketchLayer::default();
    layer.fill(Space::Screen, [&outer[..], &outer[..2]], color);
    assert!((area(&layer.fills) - 100.0).abs() < 1e-6);
    assert!(layer.fills.iter().all(|v| v.flags == SCREEN));
    let broken = [at(0.0, 0.0), at(1.0, f64::NAN), at(1.0, 1.0)];
    let mut layer = SketchLayer::default();
    layer.fill(Space::Sketch, [&outer[..], &broken[..]], color);
    assert!(layer.is_empty());
    // Nothing to fill.
    layer.fill(Space::Sketch, [&outer[..2]], color);
    layer.fill(Space::Sketch, [], color);
    assert!(layer.is_empty());
}

#[test]
fn a_triangle_is_filled_as_it_is() {
    let color = Srgba([1.0, 0.0, 0.0, 1.0]);
    let mut layer = SketchLayer::default();
    layer.triangle(
        Space::Screen,
        [at(0.0, 0.0), at(4.0, 0.0), at(0.0, 3.0)],
        color,
    );
    assert_eq!(layer.fills.len(), 3);
    assert!((area(&layer.fills) - 6.0).abs() < 1e-9);
    assert!(
        layer
            .fills
            .iter()
            .all(|v| v.flags == SCREEN && v.color == color.linear())
    );
    layer.triangle(
        Space::Sketch,
        [at(0.0, 0.0), at(f64::NAN, 0.0), at(0.0, 3.0)],
        color,
    );
    assert_eq!(layer.fills.len(), 3);
}

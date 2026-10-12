use super::*;

/// A triangle, closed, and a single segment.
fn two() -> RenderLines {
    let mut lines = RenderLines::default();
    lines
        .push([Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::ZERO])
        .unwrap();
    lines.push([Vec3::Z, Vec3::ONE]).unwrap();
    lines
}

#[test]
fn push_appends_polylines() {
    let lines = two();
    assert_eq!(lines.points().len(), 6);
    assert_eq!(lines.ends(), [4, 6]);
    assert_eq!(lines.segment_count(), 4);
    let polylines: Vec<_> = lines.polylines().collect();
    assert_eq!(polylines.len(), 2);
    assert_eq!(
        polylines[0],
        [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0; 3]]
    );
    assert_eq!(polylines[1], [[0.0, 0.0, 1.0], [1.0; 3]]);
    assert_eq!(
        lines.bounds(),
        Some(Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE
        })
    );
}

#[test]
fn empty_lines_have_no_bounds() {
    let lines = RenderLines::default();
    assert_eq!(lines.bounds(), None);
    assert_eq!(lines.segment_count(), 0);
    assert_eq!(lines.polylines().count(), 0);
}

#[test]
fn push_refuses_a_polyline_of_one_point_and_leaves_the_lines() {
    let mut lines = two();
    for points in [&[][..], &[Vec3::X]] {
        assert_eq!(lines.push(points.iter().copied()), Err(LinesError::Ends));
        assert_eq!(lines, two());
    }
}

#[test]
fn push_refuses_points_past_the_limit_and_leaves_the_lines() {
    let limit = RenderLines::MAX_POSITION;
    let mut lines = two();
    // Reaching the limit is fine.
    lines
        .push([Vec3::splat(-limit), Vec3::splat(limit)])
        .unwrap();
    for bad in [limit * 1.001, f32::INFINITY, f32::NAN] {
        let mut lines = two();
        assert_eq!(
            lines.push([Vec3::ZERO, Vec3::X, Vec3::new(0.0, bad, 0.0)]),
            Err(LinesError::Values),
            "{bad}"
        );
        assert_eq!(lines, two());
    }
}

#[test]
fn push_refuses_more_than_the_limit() {
    // Zeroed, so the pages aren't touched.
    let len = RenderLines::MAX_POINTS - 2;
    let full = || RenderLines {
        points: vec![[0.0; 3]; len],
        ends: vec![len as u32],
    };
    let mut lines = full();
    lines.push([Vec3::ZERO, Vec3::X]).unwrap();
    assert_eq!(lines.points().len(), RenderLines::MAX_POINTS);

    let mut lines = full();
    assert_eq!(
        lines.push([Vec3::ZERO, Vec3::X, Vec3::Y]),
        Err(LinesError::TooLarge)
    );
    assert_eq!(lines.points().len(), len);
    assert_eq!(lines.ends(), [len as u32]);
}

#[test]
fn from_parts_takes_lines() {
    let lines = two();
    assert_eq!(
        RenderLines::from_parts(lines.points().to_vec(), lines.ends().to_vec()),
        Ok(lines)
    );
    assert_eq!(
        RenderLines::from_parts(Vec::new(), Vec::new()),
        Ok(RenderLines::default())
    );
}

#[test]
fn from_parts_refuses_ends_not_making_polylines() {
    let points = two().points().to_vec();
    for ends in [
        // Points after the last polyline.
        &[4][..],
        &[],
        // Past the points.
        &[4, 7],
        &[4, u32::MAX],
        // A polyline of one point, or none.
        &[1, 6],
        &[4, 5, 6],
        &[4, 4, 6],
        // Going back.
        &[4, 2, 6],
    ] {
        assert_eq!(
            RenderLines::from_parts(points.clone(), ends.to_vec()),
            Err(LinesError::Ends),
            "{ends:?}"
        );
    }
}

#[test]
fn from_parts_refuses_points_out_of_range() {
    for bad in [f32::NAN, f32::NEG_INFINITY, RenderLines::MAX_POSITION * 2.0] {
        let mut points = two().points().to_vec();
        points[5][2] = bad;
        assert_eq!(
            RenderLines::from_parts(points, vec![4, 6]),
            Err(LinesError::Values),
            "{bad}"
        );
    }
}

#[test]
fn from_parts_refuses_more_than_the_limits() {
    assert_eq!(
        RenderLines::from_parts(vec![[0.0; 3]; RenderLines::MAX_POINTS + 1], Vec::new()),
        Err(LinesError::TooLarge)
    );
    assert_eq!(
        RenderLines::from_parts(Vec::new(), vec![0; RenderLines::MAX_POLYLINES + 1]),
        Err(LinesError::TooLarge)
    );
}

#[test]
fn errors_display() {
    assert_eq!(
        LinesError::Ends.to_string(),
        "the lines aren't polylines of two points or more"
    );
    assert_eq!(LinesPart::Ends.to_string(), "line ends");
}

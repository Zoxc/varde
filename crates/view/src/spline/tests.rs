use glam::DVec2;

use super::*;
use crate::testing::{self, at, line, point};

/// A sketch with a spline through three fit points and one by control
/// points, and a line: the sketch, the two splines and the line.
fn splines() -> (Sketch, [Id; 2], Id) {
    let mut sketch = Sketch::default();
    let (through, _) = testing::spline(&mut sketch, &[(0.0, 0.0), (5.0, 3.0), (10.0, 0.0)]);
    let control: Vec<Id> = [(0.0, 9.0), (4.0, 12.0), (8.0, 12.0), (12.0, 9.0)]
        .map(|(x, y)| point(&mut sketch, x, y))
        .to_vec();
    let places: Vec<DVec2> = control
        .iter()
        .map(|&id| sketch.point(id).unwrap().at)
        .collect();
    let by = Spline {
        kind: SplineKind::Control,
        knots: varde_sketch::control_knots(&places, false),
        ..Spline::through(control, false)
    };
    let by = sketch.add_curve(Curve::Spline(by), false).unwrap();
    let (a, b) = (
        point(&mut sketch, 0.0, -5.0),
        point(&mut sketch, 10.0, -5.0),
    );
    let other = line(&mut sketch, a, b);
    assert_eq!(sketch.check(&testing::DESIGN), Ok(()));
    (sketch, [through, by], other)
}

#[test]
fn the_splines_selected_convert_to_the_other_kind() {
    let (sketch, [through, by], other) = splines();
    let selection = crate::testing::items([through, by, other]);
    assert_eq!(
        conversions(&sketch, &selection),
        [
            SketchEdit::Convert {
                spline: through,
                to: SplineKind::Control
            },
            SketchEdit::Convert {
                spline: by,
                to: SplineKind::Through
            },
        ]
    );
    assert!(conversions(&sketch, &crate::testing::items([other])).is_empty());
}

#[test]
fn handles_go_on_fit_points_selected_or_all_a_spline_s_and_come_off() {
    let (mut sketch, [through, by], other) = splines();
    let fit = sketch.spline(through).unwrap().points.clone();
    // The fit point selected.
    let middle = crate::testing::items([fit[1]]);
    assert_eq!(
        handles(&sketch, &middle),
        Some(SketchEdit::AddHandles(vec![fit[1]]))
    );
    // The spline selected: all its fit points.
    let whole = crate::testing::items([through]);
    assert_eq!(
        handles(&sketch, &whole),
        Some(SketchEdit::AddHandles(fit.clone()))
    );
    // Once they all have, they come off.
    let tips: Vec<Id> = [(1.0, 1.0), (11.0, 1.0)]
        .map(|(x, y)| point(&mut sketch, x, y))
        .to_vec();
    let first = crate::testing::handle(&mut sketch, fit[0], tips[0]);
    let last = crate::testing::handle(&mut sketch, fit[2], tips[1]);
    if let Some(Curve::Spline(spline)) = sketch.curve_mut(through).map(|e| &mut e.curve) {
        spline.handles = vec![first, last];
    }
    let ends = crate::testing::items([fit[0], fit[2]]);
    assert_eq!(
        handles(&sketch, &ends),
        Some(SketchEdit::Delete(tips.clone()))
    );
    // The spline selected, its middle without: that one gets one.
    assert_eq!(
        handles(&sketch, &whole),
        Some(SketchEdit::AddHandles(vec![fit[1]]))
    );
    // One with and one without: the one without gets one.
    let both = crate::testing::items([fit[0], fit[1]]);
    assert_eq!(
        handles(&sketch, &both),
        Some(SketchEdit::AddHandles(vec![fit[1]]))
    );
    // Nothing for control points, lines or nothing.
    let control = sketch.spline(by).unwrap().points[1];
    for selection in [
        crate::testing::items([by]),
        crate::testing::items([control]),
        crate::testing::items([other]),
        crate::testing::items([]),
    ] {
        assert_eq!(handles(&sketch, &selection), None, "{selection:?}");
    }
}

#[test]
fn the_spline_tool_ends_with_enough_points_and_closes_on_its_first() {
    let placed = [at(0.0, 0.0), at(5.0, 5.0), at(10.0, 0.0)];
    let mut tool = testing::tool(Tool::Spline, &placed[..1], &[]);
    assert!(!tool.spline_ends());
    tool.placed = &placed[..2];
    assert!(tool.spline_ends());
    // Too few to close.
    assert!(!tool.closes(at(0.0, 0.0), 0.1));
    tool.placed = &placed;
    // Within the snap's reach of the first point, eight pixels.
    assert!(tool.closes(at(0.7, 0.3), 0.1));
    assert!(!tool.closes(at(1.0, 0.0), 0.1));
    // By control points it takes four to end.
    tool.control = true;
    assert_eq!(tool.spline_kind(), SplineKind::Control);
    assert!(!tool.spline_ends());
    assert!(tool.closes(at(0.0, 0.0), 0.1));
    // Other tools neither end nor close so.
    let line = testing::tool(Tool::Line, &placed, &[]);
    assert!(!line.spline_ends() && !line.closes(at(0.0, 0.0), 0.1));
}

/// An open spline's end dropped on its other end previews it closed, its
/// last point merged into its first; with three points it can't close,
/// and on any other point it closes nothing.
#[test]
fn an_end_dropped_on_the_other_previews_it_closed() {
    let mut sketch = Sketch::default();
    let places = [(0.0, 0.0), (5.0, 3.0), (10.0, 0.0), (5.0, -3.0)];
    let (_, fit) = testing::spline(&mut sketch, &places);
    let target = Some(Target::Point(fit[0]));
    let (id, dropped) = closed_by_drop(&sketch, fit[3], target).unwrap();
    let closed = dropped.spline(id).unwrap();
    assert!(closed.closed);
    assert_eq!(closed.points, fit[..3]);
    assert!(dropped.point(fit[3]).is_none());
    assert_eq!(dropped.profiles().unwrap().regions.len(), 1);
    assert!(closed_by_drop(&sketch, fit[3], Some(Target::Point(fit[1]))).is_none());
    let (three, _, _) = splines();
    let through = three.splines().next().unwrap().1.points.clone();
    let target = Some(Target::Point(through[0]));
    assert!(closed_by_drop(&three, through[2], target).is_none());
}

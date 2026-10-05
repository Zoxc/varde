use glam::{DVec2, DVec3};
use iced::{Point, Size};
use varde_document::{FeatureId, Placement};
use varde_render::{Projection, View};
use varde_sketch::{Curve, Sketch};

use super::*;
use crate::pick::tests::{SIZE, camera, plate, shown};

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(SIZE[0], SIZE[1]))
}

/// A sketch holding a line from (-25, 10) to (25, 10) and one from
/// (-6, 0) to (6, 0), and its line along y 10.
fn lines() -> (Sketch, Id) {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(DVec2::new(-25.0, 10.0)).unwrap();
    let b = sketch.add_point(DVec2::new(25.0, 10.0)).unwrap();
    let line = sketch
        .add_curve(Curve::Line { start: a, end: b }, false)
        .unwrap();
    let c = sketch.add_point(DVec2::new(-6.0, 0.0)).unwrap();
    let d = sketch.add_point(DVec2::new(6.0, 0.0)).unwrap();
    sketch
        .add_curve(Curve::Line { start: c, end: d }, false)
        .unwrap();
    (sketch, line)
}

/// Two features' ids, of the example document's.
fn ids() -> (FeatureId, FeatureId) {
    let document = varde_document::Document::example();
    let features = document.features();
    (features[0].id, features[1].id)
}

/// On the plane `z` up, facing up.
fn at_height(z: f64) -> Placement {
    Placement::on_plane(DVec3::Z, z).unwrap()
}

/// Two sketches of the same lines, one 10 mm over the example's plate
/// (z 0 to 10, a hole of radius 8 through its middle) and one 5 mm
/// under it, seen from the top: the curve under the cursor is the upper
/// sketch's, the nearer; the lower one's alone is hidden by the plate
/// when the model hides what's behind it, but shows through the hole,
/// and is picked anywhere when it doesn't (the faded model behind the
/// sketch being edited).
#[test]
fn the_nearest_curve_is_picked_and_what_the_model_hides_only_through_it() {
    let (sketch, line) = lines();
    let (over, under) = ids();
    let upper = SketchLines {
        feature: over,
        placement: at_height(20.0),
        sketch: &sketch,
    };
    let lower = SketchLines {
        feature: under,
        placement: at_height(-5.0),
        sketch: &sketch,
    };
    let index = plate();
    let camera = camera(View::Top, Projection::Orthographic);
    let on_line = shown(&camera, DVec3::new(20.0, 10.0, 0.0));
    let both = [lower, upper];
    for hidden_by in [None, Some(&index)] {
        let hit = curve_under(&both, on_line, &camera, bounds(), hidden_by).unwrap();
        assert_eq!(hit.of(), (over, line));
        assert!(
            (hit.at - DVec3::new(20.0, 10.0, 20.0)).length() < 1e-9,
            "{hit:?}"
        );
    }
    // The lower sketch alone: behind the plate.
    let alone = [lower];
    assert_eq!(
        curve_under(&alone, on_line, &camera, bounds(), Some(&index)),
        None
    );
    let hit = curve_under(&alone, on_line, &camera, bounds(), None).unwrap();
    assert_eq!(hit.of(), (under, line));
    // Through the hole, the short line shows.
    let in_hole = shown(&camera, DVec3::new(3.0, 0.0, 0.0));
    let hit = curve_under(&alone, in_hole, &camera, bounds(), Some(&index)).unwrap();
    assert_eq!(hit.sketch, under);
    assert_ne!(hit.item, line);
    // Its ends likewise, as points: points first.
    let at_end = shown(&camera, DVec3::new(6.0, 0.0, 0.0));
    let hit = item_under(&alone, at_end, &camera, bounds(), Some(&index)).unwrap();
    assert!(sketch.point(hit.item).is_some(), "{hit:?}");
    let at_far_end = shown(&camera, DVec3::new(25.0, 10.0, 0.0));
    assert_eq!(
        item_under(&alone, at_far_end, &camera, bounds(), Some(&index)),
        None
    );
    let hit = item_under(&alone, at_far_end, &camera, bounds(), None).unwrap();
    assert!(sketch.point(hit.item).is_some(), "{hit:?}");
    // Points on their own only: those ends are a curve's.
    assert_eq!(
        point_under(&alone, at_far_end, &camera, bounds(), Points::Loose, None),
        None
    );
}

/// A sketch on the plate's top face isn't hidden by it.
#[test]
fn a_curve_on_a_face_is_not_hidden_by_it() {
    let (sketch, line) = lines();
    let (feature, _) = ids();
    let on_top = [SketchLines {
        feature,
        placement: at_height(10.0),
        sketch: &sketch,
    }];
    let index = plate();
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let camera = camera(View::Top, projection);
        let on_line = shown(&camera, DVec3::new(20.0, 10.0, 10.0));
        let hit = curve_under(&on_top, on_line, &camera, bounds(), Some(&index));
        assert_eq!(
            hit.map(SketchHit::of),
            Some((feature, line)),
            "{projection:?}"
        );
    }
}

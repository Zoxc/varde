//! Profile errors with their geometry: a failing extrude's and revolve's
//! `FeatureFailure`, and a failing draft's `Drafted`, carry the profile's
//! segments placed by the sketch's placement, the points the error is
//! about and the sketch curves.

use glam::{DVec3, Vec3};
use varde_document::{AxisLine, Revolve, Turn};

use super::*;
use crate::tests::{answered, regenerate_with};
use crate::{Draft, ErrorGeometry};

/// Draws two squares meeting at the corner (10, 10) only: their regions
/// together are refused, touching there. The ids of the four lines
/// meeting at the corner.
fn corner_squares(sketch: &mut Sketch) -> Vec<u64> {
    let mut at_corner = Vec::new();
    let corner = sketch.add_point(DVec2::new(10.0, 10.0)).unwrap();
    for (a, b, c) in [
        ((0.0, 0.0), (10.0, 0.0), (0.0, 10.0)),
        ((20.0, 10.0), (20.0, 20.0), (10.0, 20.0)),
    ] {
        let [a, b, c] = [a, b, c].map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
        // Each counter-clockwise through the corner.
        let loop_ = if at_corner.is_empty() {
            [a, b, corner, c]
        } else {
            [corner, a, b, c]
        };
        for k in 0..4 {
            let (start, end) = (loop_[k], loop_[(k + 1) % 4]);
            let id = sketch.add_curve(Curve::Line { start, end }, false).unwrap();
            if start == corner || end == corner {
                at_corner.push(u64::from(id.get()));
            }
        }
    }
    at_corner
}

/// The geometry of the only failure of `evaluation`, `feature`'s.
fn failed_geometry(evaluation: &Evaluation, feature: FeatureId) -> Arc<ErrorGeometry> {
    let [failure] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(failure.feature, feature);
    failure.geometry.clone().expect("geometry")
}

fn near(a: [f32; 3], b: DVec3) -> bool {
    (Vec3::from(a) - b.as_vec3()).abs().max_element() < 1e-4
}

/// Checks `geometry` is the touching corner's on the XZ plane: lines
/// on the plane, the corner (10, 10) of the sketch at world (10, 0, 10)
/// among its points, its sketch curves lines meeting there.
fn is_the_corner(geometry: &ErrorGeometry, at_corner: &[u64]) {
    assert!(!geometry.lines().points().is_empty());
    assert!(geometry.lines().points().iter().all(|p| p[1] == 0.0));
    let corner = DVec3::new(10.0, 0.0, 10.0);
    assert!(
        geometry.points().iter().any(|&p| near(p, corner)),
        "{:?}",
        geometry.points()
    );
    let curves = geometry.sketch_curves();
    assert!(!curves.is_empty() && curves.len() <= 2, "{curves:?}");
    assert!(curves.iter().all(|c| at_corner.contains(c)), "{curves:?}");
    assert!(geometry.mesh().triangle_count() == 0 && !geometry.truncated());
}

#[test]
fn a_touching_extrude_fails_with_where_it_touches() {
    let mut editor = Editor::new(Document::default());
    let mut at_corner = Vec::new();
    let extent = Extent::OneSide(length(editor.document(), "5"));
    let extrude = add_extrude_on(
        &mut editor,
        OriginPlane::XZ,
        |sketch| at_corner = corner_squares(sketch),
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    assert_eq!(
        evaluation.failed,
        [(
            extrude,
            "its outline touches or crosses itself, or comes too close to itself".to_owned()
        )]
    );
    is_the_corner(&failed_geometry(&evaluation, extrude), &at_corner);

    // As a draft, on a design of the sketch alone, it fails the same
    // way, with the same geometry.
    let FeatureKind::Extrude(made) = editor.document().feature(extrude).unwrap().kind.clone()
    else {
        panic!("an extrude");
    };
    let mut sketched = Editor::new(Document::default());
    sketched
        .apply(
            sketched
                .document()
                .add_sketch(Plane::Origin(OriginPlane::XZ)),
        )
        .unwrap();
    let sketch = sketched.document().features()[0].id;
    let FeatureKind::Sketch { sketch: drawn, .. } = &editor.document().features()[0].kind else {
        panic!("a sketch");
    };
    sketched
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn.clone()),
        })
        .unwrap();
    let draft = Draft {
        revision: 3,
        feature: None,
        kind: Extrude { sketch, ..made }.into(),
    };
    let answer = answered(crate::handle(regenerate_with(&sketched, Some(draft))));
    let drafted = answer.draft.expect("the draft's answer");
    assert!(drafted.error.is_some());
    is_the_corner(&drafted.geometry.expect("geometry"), &at_corner);
}

#[test]
fn a_revolve_across_its_axis_fails_with_the_segment_and_the_axis() {
    // The example's plate, centred on the origin of XY, about the
    // sketch's y.
    let mut editor = Editor::new(Document::example());
    let extrude = example_extrude(editor.document());
    let revolve = Revolve {
        sketch: extrude.sketch,
        regions: extrude.regions.clone(),
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let revolve = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    let geometry = failed_geometry(&evaluation, revolve);
    // On the sketch's plane, z = 0.
    let points = geometry.lines().points();
    assert!(points.iter().all(|p| p[2].abs() < 1e-6), "{points:?}");
    // The axis, x = 0, across the plate from y = −20 to 20, among them.
    let on_axis: Vec<f32> = (geometry.lines().polylines())
        .filter(|line| line.iter().all(|p| p[0] == 0.0))
        .flat_map(|line| line.iter().map(|p| p[1]))
        .collect();
    let lo = on_axis.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = on_axis.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert_eq!((lo, hi), (-20.0, 20.0), "{on_axis:?}");
    // The segment crossing it: one of the outline's curves, reaching
    // either side.
    let FeatureKind::Sketch { sketch, .. } = &editor.document().features()[0].kind else {
        panic!("the example's first feature is its sketch");
    };
    let [curve] = geometry.sketch_curves()[..] else {
        panic!("{:?}", geometry.sketch_curves());
    };
    assert!(
        (sketch.curves.iter()).any(|entry| u64::from(entry.id.get()) == curve),
        "{curve}"
    );
    let xs = points.iter().map(|p| p[0]);
    assert!(xs.clone().any(|x| x < 0.0) && xs.clone().any(|x| x > 0.0));
    assert!(geometry.points().is_empty());
}

#[test]
fn a_touching_extrude_on_a_face_is_placed_by_the_face() {
    // The squares on the example plate's top face (z = 10): the
    // geometry lies on that face, the corner where the sketch's
    // placement puts it.
    let mut editor = Editor::new(Document::example());
    let plane = Plane::Face(super::faces::top(editor.document()));
    editor.apply(editor.document().add_sketch(plane)).unwrap();
    let sketch = editor.document().features().last().unwrap().id;
    let mut drawn = Sketch::default();
    let at_corner = corner_squares(&mut drawn);
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let extrude = Extrude {
        taper: None,
        sketch,
        regions,
        extent: Extent::OneSide(length(editor.document(), "5")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let extrude = editor.document().features().last().unwrap().id;
    let evaluation = evaluated(editor.document());
    let (_, placement) = *(evaluation.placements.iter())
        .find(|(id, _)| *id == sketch)
        .expect("the sketch is placed");
    assert!((placement.origin.z - 10.0).abs() < 1e-9, "{placement:?}");
    let geometry = failed_geometry(&evaluation, extrude);
    let lines = geometry.lines().points();
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|p| (p[2] - 10.0).abs() < 1e-4),
        "{lines:?}"
    );
    let corner = placement.origin + placement.x * 10.0 + placement.y * 10.0;
    assert!(
        geometry.points().iter().any(|&p| near(p, corner)),
        "{:?} {corner}",
        geometry.points()
    );
    let curves = geometry.sketch_curves();
    assert!(curves.iter().all(|c| at_corner.contains(c)), "{curves:?}");
}

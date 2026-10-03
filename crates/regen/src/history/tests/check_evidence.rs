//! Failures of the solid an extrude builds, or a join makes, carry the
//! triangles the kernel's check (or repair) names: a failing feature's
//! geometry has a mesh, where it fails.

use super::*;
use crate::ErrorGeometry;

/// The geometry of the only failure of `evaluation`, `feature`'s.
fn failed_geometry(evaluation: &Evaluation, feature: FeatureId) -> Arc<ErrorGeometry> {
    let [failure] = &evaluation.failed[..] else {
        panic!("{:?}", evaluation.failed);
    };
    assert_eq!(failure.feature, feature);
    failure.geometry.clone().expect("geometry")
}

/// Checks `geometry` is triangles alone, within `min..=max` (and a
/// little).
fn triangles_within(geometry: &ErrorGeometry, min: [f32; 3], max: [f32; 3]) {
    assert!(geometry.mesh().triangle_count() > 0);
    assert!(geometry.points().is_empty() && geometry.sketch_curves().is_empty());
    assert!(!geometry.truncated());
    let bounds = geometry.bounds().expect("a box");
    for i in 0..3 {
        assert!(bounds.min[i] >= min[i] - 1e-3, "{bounds:?}");
        assert!(bounds.max[i] <= max[i] + 1e-3, "{bounds:?}");
    }
}

#[test]
fn an_extrude_too_thin_fails_where_it_is_thin() {
    // A 10 mm square with a slot four resolutions wide cut into it from
    // the top, down to 2 mm: far enough apart for the profile, too near
    // for the solid. The triangles the kernel's check names come with
    // the error, by the slot.
    let mut editor = Editor::new(Document::default());
    let half = 2.0 * editor.document().tolerance().resolution();
    let extent = Extent::OneSide(length(editor.document(), "10"));
    let extrude = add_extrude_on(
        &mut editor,
        OriginPlane::XY,
        |sketch| {
            let corners = [
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (5.0 + half, 10.0),
                (5.0 + half, 2.0),
                (5.0 - half, 2.0),
                (5.0 - half, 10.0),
                (0.0, 10.0),
            ]
            .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
            for (k, &start) in corners.iter().enumerate() {
                let end = corners[(k + 1) % corners.len()];
                sketch.add_curve(Curve::Line { start, end }, false).unwrap();
            }
        },
        extent,
        Operation::NewBody(BodyId::NEW),
    );
    let evaluation = evaluated(editor.document());
    let geometry = failed_geometry(&evaluation, extrude);
    triangles_within(&geometry, [0.0; 3], [10.0; 3]);
}

#[test]
fn a_join_touching_along_an_edge_fails_where_it_touches() {
    // Two 10 mm cubes along an edge, the second joined to the first:
    // the union would touch itself there, and the triangles that failed
    // come with the error, by the edge.
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((0.0, 0.0), (10.0, 10.0)),
        extent.clone(),
        Operation::NewBody(BodyId::NEW),
    );
    let join = add_extrude_on(
        &mut editor,
        OriginPlane::XY,
        rectangle((10.0, 10.0), (20.0, 20.0)),
        extent,
        Operation::Join(Targets::default()),
    );
    let evaluation = evaluated(editor.document());
    let geometry = failed_geometry(&evaluation, join);
    triangles_within(&geometry, [0.0; 3], [20.0, 20.0, 10.0]);
}

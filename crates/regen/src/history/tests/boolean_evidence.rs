//! A boolean's failure carries where it fails: the two vertices of a
//! pinch, and the operand faces the decisions found touching along a
//! line, resolved to the faces of the bodies drawn and drawn with the
//! rest.

use glam::Vec3;

use super::*;
use crate::tests::regenerate_with;
use crate::{ErrorGeometry, Response, handle};

/// The geometry of the one failure of `editor`'s document as
/// regenerated, `feature`'s.
fn failed(editor: &Editor, feature: FeatureId) -> Arc<ErrorGeometry> {
    let Response::Regenerated { failed, .. } = handle(regenerate_with(editor, None)) else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, feature);
    failure.geometry.clone().expect("geometry")
}

#[test]
fn a_join_along_an_edge_shows_the_pinch() {
    // Two 10 mm cubes along an edge, the second joined to the first: the
    // pinch's two vertices (or one, where they are at one place) come
    // with the triangles that failed, on the edge.
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
    let geometry = failed(&editor, join);
    assert!(geometry.mesh().triangle_count() > 0);
    let points = geometry.points();
    assert!(matches!(points.len(), 1 | 2), "{points:?}");
    for &[x, y, z] in points {
        assert!(
            (x - 10.0).abs() < 1e-3 && (y - 10.0).abs() < 1e-3,
            "{x} {y}"
        );
        assert!((-1e-3..=10.001).contains(&z), "{z}");
    }
}

#[test]
fn a_join_touching_along_a_line_shows_the_faces_that_touch() {
    // Two discs of radius 5 side by side, the second joined to the first:
    // the walls touch along a line, refused from the decisions with the
    // pieces of the walls along it and the operands' walls by name. The
    // first is the body joined to, so its wall resolves to its faces in
    // the model, drawn with the pieces; the second is the tool, drawn as
    // no body, so names none.
    let mut editor = Editor::new(Document::default());
    let extent = Extent::OneSide(length(editor.document(), "10"));
    add_extrude_on(
        &mut editor,
        OriginPlane::XY,
        disc((0.0, 0.0), 5.0),
        extent.clone(),
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let join = add_extrude_on(
        &mut editor,
        OriginPlane::XY,
        disc((10.0, 0.0), 5.0),
        extent,
        Operation::Join(Targets::default()),
    );
    let Response::Regenerated {
        mesh,
        picking,
        failed,
        ..
    } = handle(regenerate_with(&editor, None))
    else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, join);
    assert!(
        failure.message.contains("would touch itself"),
        "{}",
        failure.message
    );
    let geometry = failure.geometry.clone().expect("geometry");
    // The first body's wall, every face of it in the model.
    let faces = geometry.faces();
    assert!(!faces.is_empty());
    let wall = picking.faces()[faces[0].1 as usize].key;
    let walls: Vec<(BodyId, u32)> = (0..mesh.face_count() as u32)
        .filter(|&f| {
            picking.face_body(&mesh, f) == Some(body) && picking.faces()[f as usize].key == wall
        })
        .map(|f| (body, f))
        .collect();
    assert_eq!(faces, walls);
    // Drawn: the pieces along the line and those faces' triangles, all
    // within the box, which takes in the line where they touch.
    let face_triangles: usize = (faces.iter())
        .map(|&(_, f)| mesh.face_indices(f as usize).unwrap().len() / 3)
        .sum();
    assert!(geometry.mesh().triangle_count() > face_triangles);
    let bounds = geometry.bounds().expect("a box");
    let line = Vec3::new(5.0, 0.0, 5.0);
    assert!(bounds.min.cmple(line).all() && line.cmple(bounds.max).all());
    assert!(bounds.max.x < 15.01 && bounds.min.x > -5.01, "{bounds:?}");
}

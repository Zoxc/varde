//! A boolean's failure carries where it fails: the two vertices of a
//! pinch, the operand faces the decisions found touching along a line,
//! and where decisions don't fit together, the faces resolved to those
//! of the bodies drawn and drawn with the rest.

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

#[test]
fn an_intersect_that_cant_be_worked_out_shows_where() {
    // A rod of radius 0.25 along y, its axis at x 1.75 in the plane of
    // the end of a cylinder of radius 1 along x, its foot tangent to that
    // one's wall, intersected with it: a face of the rod is left with
    // pieces of boundary that don't close into loops, refused as
    // decisions that don't fit together. The failure shows those pieces
    // as lines, the two vertices where they stop as points, and the
    // rod's face, resolved to the body's faces in the model and drawn
    // with the rest.
    let mut editor = Editor::new(Document::default());
    let rod = two_sides(editor.document(), "1.75", "0.5");
    add_extrude_on(
        &mut editor,
        OriginPlane::XZ,
        disc((1.75, 0.0), 0.25),
        rod,
        Operation::NewBody(BodyId::NEW),
    );
    let body = editor.document().bodies()[0].id;
    let extent = Extent::OneSide(length(editor.document(), "1.75"));
    let intersect = add_extrude_on(
        &mut editor,
        OriginPlane::YZ,
        disc((-0.5, 0.75), 1.0),
        extent,
        Operation::Intersect(Targets::default()),
    );
    let Response::Regenerated { failed, .. } = handle(regenerate_with(&editor, None)) else {
        panic!("regeneration failed");
    };
    let [failure] = &failed[..] else {
        panic!("{failed:?}");
    };
    assert_eq!(failure.feature, intersect);
    assert!(
        failure.message.contains("can't be worked out"),
        "{}",
        failure.message
    );
    let geometry = failure.geometry.clone().expect("geometry");
    // Where the pieces stop, on the rod's wall at the cylinder's end.
    let points = geometry.points();
    assert_eq!(points.len(), 2, "{points:?}");
    for &[x, y, z] in points {
        assert!((x - 1.75).abs() < 1e-3, "{x}");
        assert!(((x - 1.75).hypot(z) - 0.25).abs() < 1e-3, "{y} {z}");
    }
    assert!(!geometry.lines().points().is_empty());
    // The rod's face, every one of its faces of that name in the model.
    let faces = geometry.faces();
    assert!(!faces.is_empty());
    assert!(faces.iter().all(|&(b, _)| b == body), "{faces:?}");
    assert!(geometry.mesh().triangle_count() > 0);
}

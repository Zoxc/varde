use glam::{DVec2, DVec3};
use varde_document::{
    Command, Document, EdgeRef, Editor, FaceRef, FeatureKind, LinkSource, OriginPlane, OutsideRef,
    Plane,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::mesh::Form;
use varde_sketch::{Curve, LinkKind, LinkShape, SketchEdit};

use super::*;
use crate::cache::Cache;
use crate::history::evaluate;
use crate::picking::region_form;

/// The example plate (60 × 40 × 10 about the origin, a hole of radius 8)
/// with a new sketch on `plane` linking `source` by `kind`: the editor
/// and the sketch's feature.
fn linked(plane: OriginPlane, kind: LinkKind, source: OutsideRef) -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::example());
    editor
        .apply(editor.document().add_sketch(Plane::Origin(plane)))
        .unwrap();
    let feature = editor.document().features()[2].id;
    let design = editor.document().design();
    let sketch = sketch_of(&editor, feature).clone();
    let added = SketchEdit::AddLink { kind }
        .apply(&sketch, &design)
        .unwrap();
    let link = added.links[0].id;
    editor
        .apply(Command::AddLink {
            feature,
            sketch: Box::new(added),
            source: LinkSource { link, source },
        })
        .unwrap();
    (editor, feature)
}

fn sketch_of(editor: &Editor, feature: FeatureId) -> &Sketch {
    match &editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

/// The plate's body and solid.
fn plate() -> (BodyId, Arc<varde_kernel::Solid>) {
    let document = Document::example();
    let evaluation = evaluate(&document, &mut Cache::default());
    let made = &evaluation.bodies[0];
    (made.body, made.solid.clone())
}

/// The plate's edge whose shape `wanted` takes, named.
fn edge(wanted: impl Fn(&EdgeShape) -> bool) -> EdgeRef {
    let (body, solid) = plate();
    let topology = solid.topology();
    let chain = (topology.chains().iter())
        .find(|chain| wanted(&edge_shape(&solid, chain)))
        .expect("the edge");
    let mut faces = chain.regions.map(|r| topology.regions()[r as usize].key);
    faces.sort();
    let mesh = solid.mesh();
    EdgeRef {
        body,
        faces,
        near: mesh.curve(chain.halfedges[0]).eval(0.5),
    }
}

/// The plate's face whose form `wanted` takes, named.
fn face(wanted: impl Fn(&Form) -> bool) -> FaceRef {
    let (body, solid) = plate();
    let topology = solid.topology();
    let region = (topology.regions().iter())
        .find(|region| wanted(region_form(&solid, region)))
        .expect("the face");
    let patch = solid.mesh().patch(region.tris[0] as usize);
    FaceRef {
        body,
        key: region.key,
        near: patch.eval(DVec3::splat(1.0 / 3.0)),
    }
}

/// What the sketch `feature` of `editor`'s document is relinked to, and
/// its broken links.
fn found(editor: &Editor, feature: FeatureId) -> (Option<Arc<Sketch>>, Vec<String>) {
    let evaluation = evaluate(editor.document(), &mut Cache::default());
    let relinked = (evaluation.relinked.iter())
        .find(|(id, _)| *id == feature)
        .map(|(_, sketch)| sketch.clone());
    let broken = (evaluation.broken.into_iter())
        .filter(|(id, _, _)| *id == feature)
        .map(|(_, _, why)| why)
        .collect();
    (relinked, broken)
}

/// The shape the sketch's only link holds.
fn held(sketch: &Sketch) -> LinkShape {
    sketch.link_shape(&sketch.links[0]).unwrap()
}

fn near(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-9
}

/// The hole's top rim: the circle at z = 10.
fn top_rim(shape: &EdgeShape) -> bool {
    matches!(*shape, EdgeShape::Circle { centre, .. } if (centre.z - 10.0).abs() < 1e-9)
}

#[test]
fn a_round_edge_projects_to_a_circle_or_seen_edge_on_a_line() {
    let rim = OutsideRef::Edge(edge(top_rim));
    let (editor, feature) = linked(OriginPlane::XY, LinkKind::Project, rim);
    let (relinked, broken) = found(&editor, feature);
    assert!(broken.is_empty(), "{broken:?}");
    let shape = held(&relinked.unwrap());
    let [Curve::Circle { radius, .. }] = shape.curves[..] else {
        panic!("{shape:?}");
    };
    assert!((radius - 8.0).abs() < 1e-9);
    assert!(near(shape.points[0], DVec2::ZERO));

    let (editor, feature) = linked(OriginPlane::XZ, LinkKind::Project, rim);
    let shape = held(&found(&editor, feature).0.unwrap());
    assert!(
        matches!(shape.curves[..], [Curve::Line { .. }]),
        "{shape:?}"
    );
    let mut ends = [shape.points[0], shape.points[1]];
    ends.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!(near(ends[0], DVec2::new(-8.0, 10.0)), "{ends:?}");
    assert!(near(ends[1], DVec2::new(8.0, 10.0)), "{ends:?}");
}

#[test]
fn a_straight_edge_square_to_the_plane_projects_to_a_point() {
    let upright = |shape: &EdgeShape| matches!(*shape, EdgeShape::Line { from, to } if (from - to).cross(DVec3::Z).length() < 1e-9);
    let (editor, feature) = linked(
        OriginPlane::XY,
        LinkKind::Project,
        OutsideRef::Edge(edge(upright)),
    );
    let shape = held(&found(&editor, feature).0.unwrap());
    assert_eq!(shape.points.len(), 1);
    assert!(shape.curves.is_empty());
    assert!((shape.points[0].x.abs() - 30.0).abs() < 1e-9);
    assert!((shape.points[0].y.abs() - 20.0).abs() < 1e-9);
}

#[test]
fn the_hole_cut_through_its_axis_is_two_lines() {
    let wall = face(|form| matches!(form, Form::Cylinder { .. }));
    let (editor, feature) = linked(OriginPlane::XZ, LinkKind::Intersect, OutsideRef::Face(wall));
    let (relinked, broken) = found(&editor, feature);
    assert!(broken.is_empty(), "{broken:?}");
    let shape = held(&relinked.unwrap());
    assert_eq!(shape.curves.len(), 2, "{shape:?}");
    for curve in &shape.curves {
        let Curve::Line { start, end } = *curve else {
            panic!("{shape:?}");
        };
        let (a, b) = (
            shape.points[start.get() as usize],
            shape.points[end.get() as usize],
        );
        assert!(
            (a.x.abs() - 8.0).abs() < 1e-9 && (a.x - b.x).abs() < 1e-9,
            "{a} {b}"
        );
        assert!(((a.y - b.y).abs() - 10.0).abs() < 1e-9, "{a} {b}");
    }
    // The plate's top lies in no origin plane, but its bottom is in XY:
    // refused with why.
    let bottom = face(|form| matches!(*form, Form::Plane { n, d } if n.z < -0.5 && d.abs() < 1e-9));
    let (editor, feature) = linked(
        OriginPlane::XY,
        LinkKind::Intersect,
        OutsideRef::Face(bottom),
    );
    let (relinked, broken) = found(&editor, feature);
    assert!(relinked.is_none());
    assert_eq!(broken, [FACE_IN_PLANE]);
}

#[test]
fn an_edge_crossing_the_plane_intersects_at_a_point() {
    let front_top = |shape: &EdgeShape| {
        matches!(*shape, EdgeShape::Line { from, to }
            if (from.z - 10.0).abs() < 1e-9 && (to.z - 10.0).abs() < 1e-9
                && (from.y + 20.0).abs() < 1e-9 && (to.y + 20.0).abs() < 1e-9)
    };
    let (editor, feature) = linked(
        OriginPlane::YZ,
        LinkKind::Intersect,
        OutsideRef::Edge(edge(front_top)),
    );
    let shape = held(&found(&editor, feature).0.unwrap());
    assert_eq!(shape.points.len(), 1);
    assert!(near(shape.points[0], DVec2::new(-20.0, 10.0)), "{shape:?}");
    // Along the plane it lies in nothing: it misses XZ.
    let (editor, feature) = linked(
        OriginPlane::XZ,
        LinkKind::Intersect,
        OutsideRef::Edge(edge(front_top)),
    );
    assert_eq!(found(&editor, feature).1, [EDGE_MISSES]);
}

#[test]
fn another_sketchs_line_projects_to_a_line_or_a_point() {
    let document = Document::example();
    let first = document.features()[0].id;
    let FeatureKind::Sketch { sketch, .. } = &document.features()[0].kind else {
        panic!("the plate's sketch");
    };
    // The rectangle's bottom (along x) and right (along y) sides.
    let line = |along: DVec2| {
        (sketch.curves.iter())
            .find(|entry| {
                let Curve::Line { start, end } = entry.curve else {
                    return false;
                };
                let d = sketch.point(end).unwrap().at - sketch.point(start).unwrap().at;
                d.normalize().dot(along).abs() > 0.99
                    && sketch.point(start).unwrap().at.dot(along.perp()) < 0.0
            })
            .unwrap()
            .id
    };
    let (bottom, right) = (line(DVec2::X), line(DVec2::Y));
    let source = |item| OutsideRef::Sketch {
        sketch: first,
        item,
    };
    let (editor, feature) = linked(OriginPlane::XZ, LinkKind::Project, source(bottom));
    let shape = held(&found(&editor, feature).0.unwrap());
    assert!(
        matches!(shape.curves[..], [Curve::Line { .. }]),
        "{shape:?}"
    );
    let xs: Vec<f64> = shape.points.iter().map(|p| p.x).collect();
    assert!(xs.contains(&-30.0) && xs.contains(&30.0), "{xs:?}");
    assert!(shape.points.iter().all(|p| p.y == 0.0));

    let (editor, feature) = linked(OriginPlane::XZ, LinkKind::Project, source(right));
    let shape = held(&found(&editor, feature).0.unwrap());
    assert!(shape.curves.is_empty());
    assert_eq!(shape.points.len(), 1);
    assert!((shape.points[0].x.abs() - 30.0).abs() < 1e-9);
}

#[test]
fn relinking_keeps_ids_and_a_missing_source_is_broken() {
    let document = Document::example();
    let first = document.features()[0].id;
    let FeatureKind::Sketch { sketch, .. } = &document.features()[0].kind else {
        panic!("the plate's sketch");
    };
    let corner = sketch.points[0].id;
    let (mut editor, feature) = linked(
        OriginPlane::XY,
        LinkKind::Project,
        OutsideRef::Sketch {
            sketch: first,
            item: corner,
        },
    );
    let relinked = found(&editor, feature).0.unwrap();
    editor
        .amend(Command::SetSketch {
            feature,
            sketch: Box::new(Sketch::clone(&relinked)),
        })
        .unwrap();
    let held = sketch_of(&editor, feature).links[0].clone();
    // In step: nothing to relink.
    assert!(found(&editor, feature).0.is_none());

    // The corner moved: the same point, moved.
    let mut moved = sketch.clone();
    moved.points[0].at += DVec2::new(-5.0, 0.0);
    editor
        .apply(Command::SetSketch {
            feature: first,
            sketch: Box::new(moved),
        })
        .unwrap();
    let relinked = found(&editor, feature).0.unwrap();
    assert_eq!(relinked.links[0], held);
    let at = relinked.point(held.points[0]).unwrap().at;
    assert!(near(at, sketch.points[0].at + DVec2::new(-5.0, 0.0)));

    // The corner gone: broken, keeping what it held.
    let mut gone = sketch.clone();
    gone.delete(&[corner]);
    editor
        .apply(Command::SetSketch {
            feature: first,
            sketch: Box::new(gone),
        })
        .unwrap();
    let (relinked, broken) = found(&editor, feature);
    assert!(relinked.is_none());
    assert_eq!(broken, [SOURCE_ITEM_GONE]);
}

#[test]
fn an_edge_not_on_its_body_is_broken() {
    let mut missing = edge(top_rim);
    missing.faces[0].instance = 7;
    missing.faces.sort();
    let (editor, feature) = linked(
        OriginPlane::XY,
        LinkKind::Project,
        OutsideRef::Edge(missing),
    );
    assert_eq!(found(&editor, feature).1, [EDGE_NOT_FOUND]);
}

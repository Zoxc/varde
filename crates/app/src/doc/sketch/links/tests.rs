//! The Sketch tab's link rows: their names, broken ones, selecting,
//! lighting, removing and counting for profiles.

use std::cell::RefCell;
use varde_sketch::Selectable;

use glam::{DVec2, DVec3};
use varde_document::{Command, FaceRef, FeatureKind, OriginPlane, OutsideRef, Plane, Sketch};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::mesh::Form;
use varde_regen::Request;
use varde_sketch::LinkKind;
use varde_view::{Edit, Look, RowMenu};

use crate::doc::Doc;
use crate::doc::sketch::tests::Answered;
use crate::tests::{SolveLane, answer, example};

/// Answers the models asked for and the solver until neither is asked
/// anything more.
fn settle(doc: &mut Answered, requests: &RefCell<Vec<Request>>) {
    for _ in 0..8 {
        if requests.borrow().is_empty() {
            break;
        }
        answer(doc, requests);
        let Answered { doc, lane } = doc;
        lane.answer(doc);
    }
}

fn edited(doc: &Doc) -> &Sketch {
    let feature = doc.sketch.as_ref().unwrap().feature;
    match &doc.editor.document().feature(feature).unwrap().kind {
        FeatureKind::Sketch { sketch, .. } => sketch,
        _ => panic!("not a sketch"),
    }
}

/// The example with a sketch on XZ being edited, projecting the plate's
/// front top edge and cutting the hole's wall.
fn linked() -> (Answered, std::rc::Rc<RefCell<Vec<Request>>>) {
    let (mut doc, requests) = example();
    doc.apply(
        doc.editor
            .document()
            .add_sketch(Plane::Origin(OriginPlane::XZ)),
    );
    let feature = doc.editor.document().features().last().unwrap().id;
    doc.sync();
    answer(&mut doc, &requests);
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    doc.look(Look::EditFeature(feature));
    answer(&mut doc, &requests);

    let evaluation = varde_regen::evaluate(
        &varde_document::Document::example(),
        &mut Default::default(),
    );
    let made = &evaluation.bodies[0];
    let solid = &made.solid;
    let topology = solid.topology();
    let front_top = (topology.chains().iter())
        .find(|chain| {
            matches!(edge_shape(solid, chain), EdgeShape::Line { from, to }
                if from.z == 10.0 && to.z == 10.0 && from.y == -20.0 && to.y == -20.0)
        })
        .unwrap();
    let mut faces = front_top
        .regions
        .map(|r| topology.regions()[r as usize].key);
    faces.sort();
    let edge = varde_document::EdgeRef {
        body: made.body,
        faces,
        near: DVec3::new(0.0, -20.0, 10.0),
    };
    let wall = (0..topology.regions().len() as u32)
        .find(|&r| matches!(topology.form(solid, r), Form::Cylinder { .. }))
        .unwrap();
    let region = &topology.regions()[wall as usize];
    let face = FaceRef {
        body: made.body,
        key: region.key,
        near: solid
            .mesh()
            .patch(region.tris[0] as usize)
            .eval(DVec3::splat(1.0 / 3.0)),
    };
    assert!(doc.propose_link(LinkKind::Project, OutsideRef::Edge(edge)));
    assert!(doc.propose_link(LinkKind::Intersect, OutsideRef::Face(face)));
    doc.lane.answer(&mut doc.doc);
    doc.sync();
    settle(&mut doc, &requests);
    (doc, requests)
}

#[test]
fn links_are_listed_by_source_and_select_and_light_from_their_rows() {
    let (mut doc, _requests) = linked();
    let rows = doc.sketch.as_ref().unwrap().links.clone();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].kind, rows[0].source.as_str()),
        (LinkKind::Project, "Edge of Body 1")
    );
    assert_eq!(
        (rows[1].kind, rows[1].source.as_str()),
        (LinkKind::Intersect, "Face of Body 1")
    );
    assert!(rows.iter().all(|row| row.broken.is_none() && row.profiles));
    let state = doc.sketch_state().unwrap();
    assert_eq!(state.links.len(), 2);

    // Its row selects what it made.
    doc.look(Look::ClickLink(rows[1].link));
    let made = edited(&doc).link(rows[1].link).unwrap().clone();
    let selection = &doc.sketch.as_ref().unwrap().selection;
    assert_eq!(selection.len(), made.points.len() + made.curves.len());
    assert!(
        made.curves
            .iter()
            .all(|curve| selection.contains(&Selectable::Item(*curve)))
    );

    // Hovered, what it comes from is lit in the model.
    doc.look(Look::HoverLink(Some(rows[0].link)));
    let lit = doc.highlight().unwrap();
    assert_eq!(lit.highlights.selected_edges.len(), 1);
    doc.look(Look::HoverLink(None));
    assert!(doc.highlight().is_none());

    // Its menu makes its curves construction, then removes it.
    doc.look(Look::OpenMenu(RowMenu::Link(rows[0].link)));
    assert_eq!(doc.sketch_state().unwrap().link_menu, Some(rows[0].link));
    doc.update(Edit::SetLinkProfiles(rows[0].link, false));
    let link = edited(&doc).link(rows[0].link).unwrap().clone();
    assert!(!link.profiles);
    assert!(edited(&doc).curve(link.curves[0]).unwrap().construction);
    doc.update(Edit::RemoveLink(rows[0].link));
    assert!(edited(&doc).link(rows[0].link).is_none());
    assert!(edited(&doc).curve(link.curves[0]).is_none());
    assert_eq!(doc.sketch.as_ref().unwrap().links.len(), 1);
}

#[test]
fn a_broken_link_s_row_says_why() {
    let (mut doc, requests) = linked();
    let plate = doc.editor.document().features()[1].id;
    // The extrude making the plate removed: its edge and face are gone,
    // the links keep what they held.
    let held = edited(&doc).clone();
    doc.apply(Command::RemoveFeature(plate));
    doc.sync();
    settle(&mut doc, &requests);
    assert_eq!(edited(&doc), &held);
    let rows = &doc.sketch.as_ref().unwrap().links;
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.broken.as_deref(), Some("Its body is gone"), "{row:?}");
        assert!(row.source.contains("a removed body"));
    }
}

#[test]
fn a_shape_snapped_to_a_link_s_point_gets_its_own() {
    let (doc, _requests) = linked();
    let sketch = edited(&doc);
    let point = sketch.links[0].points[0];
    let at = sketch.point(point).unwrap().at;
    let mut add = varde_sketch::Add::new(sketch);
    let target = Some(varde_view::Target::Point(point));
    let placed = super::super::edit::place(sketch, &mut add, at, target).unwrap();
    assert_ne!(placed, point);
    assert_eq!(add.points, [(placed, at)]);
    assert_eq!(
        add.auto,
        [varde_sketch::Constraint::Coincident(placed, point)]
    );
}

#[test]
fn delete_on_any_of_a_link_s_items_deletes_the_link_with_the_rest_selected() {
    let (mut doc, _requests) = linked();
    let before = edited(&doc).clone();
    let [projected, intersected] = [0, 1].map(|i| before.links[i].clone());
    // One curve of the projected edge's, by the Delete key.
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(projected.curves[0])),
        add: false,
    });
    doc.key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Delete,
    ));
    assert!(doc.edit_error.is_none(), "{:?}", doc.edit_error);
    assert!(edited(&doc).link(projected.id).is_none());
    assert!(
        projected
            .items()
            .all(|item| edited(&doc).kind(item).is_none())
    );
    assert!(edited(&doc).link(intersected.id).is_some());
    // One undo step brings it back.
    doc.update(Edit::Undo);
    assert_eq!(edited(&doc), &before);

    // The user's own point with one point of the intersected face's and
    // all of the projected edge's: the point, and both links, go.
    let mut own = before.clone();
    let point = own.add_point(DVec2::new(3.0, 3.0)).unwrap();
    let feature = doc.sketch.as_ref().unwrap().feature;
    doc.apply(Command::SetSketch {
        feature,
        sketch: Box::new(own),
    });
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(point)),
        add: false,
    });
    doc.look(Look::ClickGeometry {
        hit: Some(Selectable::Item(intersected.points[0])),
        add: true,
    });
    for &item in &projected.points {
        doc.look(Look::ClickGeometry {
            hit: Some(Selectable::Item(item)),
            add: true,
        });
    }
    doc.update(Edit::DeleteSelection);
    assert!(doc.edit_error.is_none(), "{:?}", doc.edit_error);
    let after = edited(&doc);
    assert!(after.links.is_empty());
    assert!(after.point(point).is_none());
    assert!(after.points.is_empty() && after.curves.is_empty());
    doc.update(Edit::Undo);
    assert!(edited(&doc).point(point).is_some());
    assert_eq!(edited(&doc).links, before.links);
}

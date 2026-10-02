//! Changing a sketch's plane: from its Timeline row or the Sketch tab,
//! to an origin plane or a flat face of a body made before it, one undo
//! step, the drawing kept; `Esc` leaves no trace.

use varde_view::RowMenu;

use super::*;

/// The example with a circle in a sketch on its plate's top, left and
/// answered, the Timeline showing: its id.
fn circle_on_the_top() -> (Doc, FeatureId, Requests) {
    let (mut doc, id, requests) = on_the_top();
    doc.look(Look::FinishSketch);
    circle_in(&mut doc, &requests, id, DVec2::new(20.0, 10.0), 4.0);
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    (doc, id, requests)
}

/// Opens the Timeline row menu of `id` and clicks its Change plane.
fn change_from_the_timeline(doc: &mut Doc, id: FeatureId) {
    doc.look(Look::OpenMenu(RowMenu::Feature(id)));
    let at = text_at(doc, "Change plane");
    let sent = click_screen(doc, at);
    assert!(
        (sent.iter()).any(|m| matches!(m, Ui::Look(Look::ChangePlane(f)) if *f == id)),
        "{sent:?}"
    );
    take(doc, sent);
}

/// `doc`'s screen, laid out.
fn shown_ui<'a>(doc: &'a Doc, renderer: &mut iced::Renderer) -> crate::tests::Headless<'a> {
    shown(doc.view_in(Mode::Light), SIZE, renderer)
}

/// The points of the sketches drawn in the model.
fn drawn_points(doc: &Doc) -> Vec<[f32; 3]> {
    doc.feed.sketches().points().to_vec()
}

#[test]
fn the_timeline_notes_the_face_a_sketch_is_on() {
    let (doc, _, _requests) = circle_on_the_top();
    let shown = all_texts(&doc);
    // The example's sketch on XY, and this one on the extrude's far cap.
    assert!(shown.iter().any(|t| t == "XY"), "{shown:?}");
    assert!(shown.iter().any(|t| t == "on Extrude 1's end"), "{shown:?}");
}

#[test]
fn changing_the_plane_from_the_timeline_moves_the_drawing() {
    let (mut doc, id, requests) = circle_on_the_top();
    let drawn = sketch_of(&doc, id);
    let revision = doc.editor.revision();
    assert!(drawn_points(&doc).iter().all(|p| p[2] == 10.0));

    change_from_the_timeline(&mut doc, id);
    let picking = doc.picking_plane.as_ref().expect("picking a plane");
    assert!(!picking.enter);
    let bar = status_bar(&doc);
    assert!(
        bar.iter()
            .any(|t| t == "Pick a plane or a flat face for Sketch 2"),
        "{bar:?}"
    );
    assert!(all_texts(&doc).iter().any(|t| t == "Sketch 2's plane"));

    // Onto the plate's bottom: facing down, its x along X and y along −Y.
    let bottom = face_on(&doc, -DVec3::Z, 0.0);
    doc.update(Edit::FacePicked(bottom));
    assert!(doc.picking_plane.is_none());
    assert_eq!(edited(&doc), None, "the Timeline's change doesn't enter it");
    assert_eq!(plane(&doc, id), Plane::Face(bottom));
    assert_eq!(sketch_of(&doc, id), drawn, "the drawing is kept");
    let placement = doc.placement(id).unwrap();
    assert_eq!(placement.origin, DVec3::ZERO);
    assert_eq!(placement.x, DVec3::X);
    assert_eq!(placement.y, -DVec3::Y);
    answer(&mut doc, &requests);
    assert_eq!(doc.feed.placement(id), Some(placement), "the same bits");
    let points = drawn_points(&doc);
    assert!(!points.is_empty());
    for p in &points {
        assert_eq!(p[2], 0.0, "{p:?}");
        // The circle about (20, 10) in the sketch is about (20, −10).
        let off = glam::Vec2::new(p[0] - 20.0, p[1] + 10.0).length();
        assert!(off <= 4.0 + 1e-3, "{p:?}");
    }
    let shown = all_texts(&doc);
    assert!(
        shown.iter().any(|t| t == "on Extrude 1's start"),
        "{shown:?}"
    );

    // Onto YZ, then back with two undos: one step each.
    change_from_the_timeline(&mut doc, id);
    doc.update(Edit::PlanePicked(OriginPlane::YZ));
    assert_eq!(plane(&doc, id), Plane::Origin(OriginPlane::YZ));
    assert_eq!(doc.placement(id), Some(OriginPlane::YZ.placement()));
    answer(&mut doc, &requests);
    assert!(drawn_points(&doc).iter().all(|p| p[0] == 0.0));
    doc.update(Edit::Undo);
    assert_eq!(plane(&doc, id), Plane::Face(bottom));
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.revision(), revision);
    assert!(matches!(plane(&doc, id), Plane::Face(f) if f.near.z == 10.0));
    answer(&mut doc, &requests);
    assert_eq!(doc.placement(id).unwrap().origin.z, 10.0);
}

#[test]
fn esc_while_changing_the_plane_leaves_no_trace() {
    let (mut doc, id, _requests) = circle_on_the_top();
    let revision = doc.editor.revision();
    let before = plane(&doc, id);
    change_from_the_timeline(&mut doc, id);
    doc.look(Look::Escape);
    assert!(doc.picking_plane.is_none());
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(plane(&doc, id), before);
    assert_eq!(edited(&doc), None);
    // A face click on its way when picking ended does nothing.
    let bottom = face_on(&doc, -DVec3::Z, 0.0);
    doc.update(Edit::FacePicked(bottom));
    assert_eq!(doc.editor.revision(), revision);

    // The sketch undone while its plane is picked: picking goes with it.
    change_from_the_timeline(&mut doc, id);
    while doc.editor.document().feature(id).is_some() {
        doc.update(Edit::Undo);
    }
    assert!(doc.picking_plane.is_none());
}

#[test]
fn only_faces_made_before_the_sketch_take_it() {
    // The example's first sketch, on XY, is before the plate it makes.
    let (mut doc, requests) = example();
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let first = doc.editor.document().features()[0].id;
    let revision = doc.editor.revision();
    change_from_the_timeline(&mut doc, first);

    let [(top, near)] = faces(
        &doc,
        |s| matches!(*s, Summary::Plane { n, d } if n[2] == 1.0 && d == 10.0),
    )[..] else {
        panic!("one top");
    };
    let body = doc.editor.document().bodies()[0].id;
    let pick = varde_view::Pick {
        model: doc.feed.model(),
        target: Picked::Face(top),
        body,
        at: near,
        snap: None,
    };
    doc.look(Look::Hover(Some(pick)));
    assert_eq!(doc.highlight(), None, "a later face isn't highlighted");
    assert!(!doc.model_picking().unwrap().takes(pick.target));
    let bar = status_bar(&doc);
    let why = "Sketch 1 can only go on a face made before it";
    assert!(bar.iter().any(|t| t == why), "{bar:?}");
    // A click there picks nothing.
    let sent = click_screen(&doc, OVER_TOP);
    assert!(
        !(sent.iter()).any(|m| matches!(m, Ui::Edit(Edit::FacePicked(_)))),
        "{sent:?}"
    );

    // Asked for anyway, it's refused, and picking goes on.
    let face = doc.feed.pick_index().face_ref(top, near).unwrap();
    doc.update(Edit::FacePicked(face));
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(doc.notice.as_deref(), Some(why));
    assert!(doc.picking_plane.is_some());

    // An origin plane takes it, and the plate follows: drawn on XZ, the
    // plate stands up from the XZ plane, 10 mm towards −Y.
    doc.update(Edit::PlanePicked(OriginPlane::XZ));
    assert_eq!(plane(&doc, first), Plane::Origin(OriginPlane::XZ));
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    let positions = doc.feed.mesh().positions();
    let low = positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let high = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    assert_eq!((low, high), (-10.0, 0.0));
}

#[test]
fn changing_the_plane_from_the_sketch_tab_comes_back_to_the_sketch() {
    let (mut doc, id, requests) = circle_on_the_top();
    let revision = doc.editor.revision();
    doc.look(Look::EditFeature(id));
    assert_eq!(edited(&doc), Some(id));
    let shown = all_texts(&doc);
    assert!(shown.iter().any(|t| t == "on Extrude 1's end"), "{shown:?}");
    // The button's label on one line beside it.
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown_ui(&doc, &mut renderer);
    let found = texts(&mut ui, &renderer);
    let button = found.iter().find(|t| t.text == "Change plane").unwrap();
    let note = found
        .iter()
        .find(|t| t.text == "on Extrude 1's end")
        .unwrap();
    assert!(button.bounds.height < 20.0, "{button:?}");
    assert!(
        note.bounds.x + note.bounds.width <= button.bounds.x,
        "{note:?} {button:?}"
    );
    drop(ui);

    // Backed out of: the sketch is edited again, nothing changed.
    let sent = click_screen(&doc, text_at(&doc, "Change plane"));
    take(&mut doc, sent);
    assert_eq!(edited(&doc), None, "left while the plane is picked");
    assert!(doc.picking_plane.as_ref().is_some_and(|p| p.enter));
    assert!(doc.picks(), "the model is picked");
    doc.look(Look::Escape);
    assert!(doc.picking_plane.is_none());
    assert_eq!(edited(&doc), Some(id));
    assert_eq!(doc.editor.revision(), revision);

    // A plane picked: back in the sketch, there.
    let sent = click_screen(&doc, text_at(&doc, "Change plane"));
    take(&mut doc, sent);
    doc.update(Edit::PlanePicked(OriginPlane::XZ));
    assert_eq!(edited(&doc), Some(id));
    assert_eq!(
        doc.sketch_state().unwrap().placement,
        OriginPlane::XZ.placement()
    );
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(-Vec3::Y, 1e-5));
    let shown = all_texts(&doc);
    assert!(shown.iter().any(|t| t == "on XZ"), "{shown:?}");
    doc.look(Look::FinishSketch);
    answer(&mut doc, &requests);
    doc.update(Edit::Undo);
    assert_eq!(doc.editor.revision(), revision);
}

#[test]
fn change_plane_waits_for_a_document_that_can_be_edited() {
    let (mut doc, id, _requests) = circle_on_the_top();
    doc.read_only = Some("read only".to_owned());
    doc.look(Look::ChangePlane(id));
    assert!(doc.picking_plane.is_none());
    // Nor for a feature that isn't a sketch.
    doc.read_only = None;
    let extrude = doc.editor.document().features()[1].id;
    doc.look(Look::ChangePlane(extrude));
    assert!(doc.picking_plane.is_none());
}

#[test]
fn picking_a_plane_and_measuring_leave_each_other() {
    let (mut doc, id, _requests) = circle_on_the_top();

    // Measuring picks faces and edges and snaps, but never a plane.
    doc.look(Look::StartMeasure);
    let picking = doc.model_picking().unwrap();
    assert!(picking.snaps && picking.planes.is_none());
    doc.look(Look::StartMeasure);
    assert!(doc.measure.is_none());

    // Picking a plane, from S or Change plane, leaves the measure tool:
    // only faces, no snapping.
    for look in [Look::PickPlane, Look::ChangePlane(id)] {
        doc.look(Look::StartMeasure);
        assert!(doc.measure.is_some());
        doc.look(look);
        assert!(doc.measure.is_none() && doc.picking_plane.is_some());
        let picking = doc.model_picking().unwrap();
        assert!(picking.planes.is_some() && !picking.snaps);
        assert_eq!(picking.picks, varde_view::Picks::Faces);

        // And the measure tool leaves picking the plane.
        doc.look(Look::StartMeasure);
        assert!(doc.measure.is_some() && doc.picking_plane.is_none());
        let picking = doc.model_picking().unwrap();
        assert!(picking.snaps && picking.planes.is_none());
        doc.look(Look::StartMeasure);
        assert!(doc.measure.is_none() && doc.picking_plane.is_none());
    }
}

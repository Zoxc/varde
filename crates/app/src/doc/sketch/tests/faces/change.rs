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
    assert_eq!(
        doc.feed.placement(id, &plane(&doc, id)),
        Some(placement),
        "the same bits"
    );
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

#[test]
fn an_undo_in_the_sketch_puts_it_back_on_its_face_and_turns_to_it() {
    let (mut doc, id, requests) = circle_on_the_top();
    doc.look(Look::EditFeature(id));
    doc.look(Look::ChangePlane(id));
    // Looked at from below, as the bottom face would be picked.
    settle_camera(&mut doc);
    doc.look(Look::Orbit {
        yaw: 0.0,
        pitch: -2.0,
    });
    let bottom = face_on(&doc, -DVec3::Z, 0.0);
    doc.update(Edit::FacePicked(bottom));
    assert_eq!(edited(&doc), Some(id));
    answer(&mut doc, &requests);
    let on_bottom = doc.placement(id).unwrap();
    assert_eq!(on_bottom.normal, -DVec3::Z);
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(-Vec3::Z, 1e-5));

    doc.update(Edit::Undo);
    assert_eq!(edited(&doc), Some(id));
    assert!(matches!(plane(&doc, id), Plane::Face(f) if f.near.z == 10.0));
    // The model shown has it on the bottom, where it no longer is: no
    // placement until a model of it on the top shows, the session kept
    // as it was meanwhile.
    assert_eq!(doc.placement(id), None);
    assert_eq!(doc.sketch_state().unwrap().placement, on_bottom);
    answer(&mut doc, &requests);
    let on_top = doc.placement(id).unwrap();
    assert_eq!(on_top.origin, DVec3::new(0.0, 0.0, 10.0));
    assert_eq!(doc.sketch_state().unwrap().placement, on_top);
    // Still from below, the side the view was on, at the top face now.
    settle_camera(&mut doc);
    assert!(doc.camera.backward().abs_diff_eq(-Vec3::Z, 1e-5));
    assert!((doc.camera.target().z - 10.0).abs() < 1e-4);
}

#[test]
fn a_replaced_document_forgets_the_placement_worked_out_at_a_pick() {
    let (mut doc, _requests) = example();
    doc.look(Look::PickPlane);
    let top = face_on(&doc, DVec3::Z, 10.0);
    doc.update(Edit::FacePicked(top));
    let id = edited(&doc).expect("entered");
    assert!(doc.placement(id).is_some());
    doc.look(Look::FinishSketch);
    // The document again in other units: its sketch has the same id and
    // plane, but nothing places it until a model of it shows.
    let mut other = Editor::new(doc.editor.document().clone());
    other
        .apply(Command::SetUnits(varde_document::LengthUnit::In))
        .unwrap();
    doc.apply(Command::Replace(Box::new(other.document().clone())));
    doc.sync();
    assert_eq!(doc.placement(id), None);
}

#[test]
fn a_face_of_a_body_merged_after_the_sketch_is_named_by_its_own_body() {
    // Two plates, a sketch, then a join merging Body 2 into Body 1: Body
    // 2's bottom shows as Body 1's, but at the sketch Body 1 hasn't it.
    let (mut editor, [top, below]) = crate::tests::two_plates();
    let xy = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    crate::tests::add_join(&mut editor);
    let (mut doc, requests) = crate::tests::holding(editor.document().clone());
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    doc.look(Look::ChangePlane(id));
    let bottom = face_on(&doc, -DVec3::Z, 3.0);
    assert_eq!(bottom.body, top, "shown on the holder");
    doc.update(Edit::FacePicked(bottom));
    assert!(doc.picking_plane.is_none());
    let Plane::Face(face) = plane(&doc, id) else {
        panic!("on a face");
    };
    assert_eq!(face.body, below, "named by the body made with it");
    let picked = doc.placement(id).unwrap();
    assert_eq!(picked.origin, DVec3::new(0.0, 0.0, -3.0));
    answer(&mut doc, &requests);
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
    assert_eq!(doc.feed.placement(id, &plane(&doc, id)), Some(picked));
}

#[test]
fn a_cut_face_of_a_body_merged_after_the_sketch_is_named_by_the_body_it_was_cut_in() {
    // Two plates, a pocket cut into Body 2's top alone, a sketch, then a
    // join merging Body 2 into Body 1: the pocket's floor shows as Body
    // 1's, and was made by the cut, not by Body 2's maker, but at the
    // sketch it's Body 2's.
    let (mut editor, [top, below]) = crate::tests::two_plates();
    let xy = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let pocket = editor.document().features().last().unwrap().id;
    let mut square = varde_sketch::Sketch::default();
    let corners = [(-25.0, 5.0), (-15.0, 5.0), (-15.0, 15.0), (-25.0, 15.0)]
        .map(|(x, y)| square.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        square.add_curve(line, false).unwrap();
    }
    let region = square.profiles().unwrap().reference(0).unwrap();
    editor
        .apply(editor.document().set_sketch_whole(pocket, square))
        .unwrap();
    let ask = Extent::ask(&editor.document().design());
    let cut = Extrude {
        taper: None,
        sketch: pocket,
        regions: vec![region],
        extent: Extent::OneSide(Value::new("2", &ask).unwrap()),
        flip: true,
        operation: Operation::Cut(varde_document::Targets {
            excluded: vec![top],
            held: None,
        }),
    };
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    crate::tests::add_join(&mut editor);
    let (mut doc, requests) = crate::tests::holding(editor.document().clone());
    assert!(doc.feed.failed_features().is_empty());
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    doc.look(Look::ChangePlane(id));
    let floor = face_on(&doc, DVec3::Z, -2.0);
    assert_eq!(floor.body, top, "shown on the holder");
    doc.update(Edit::FacePicked(floor));
    let Plane::Face(face) = plane(&doc, id) else {
        panic!("on a face: {:?}", doc.notice);
    };
    assert_eq!(face.body, below, "named by the body it was cut in");
    let picked = doc.placement(id).unwrap();
    answer(&mut doc, &requests);
    assert!(
        doc.feed.failed_features().is_empty(),
        "{:?}",
        doc.feed.failed_features()
    );
    assert_eq!(doc.feed.placement(id, &plane(&doc, id)), Some(picked));
}

#[test]
fn a_face_cut_through_two_bodies_merged_after_the_sketch_is_refused() {
    // Two plates, a square hole cut through both, a sketch, then a join
    // merging Body 2 into Body 1: a wall of the hole shows on Body 1, but
    // at the sketch it may be on either plate, which can't be told.
    let (mut editor, [top, below]) = crate::tests::two_plates();
    let xy = Plane::Origin(OriginPlane::XY);
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let hole = editor.document().features().last().unwrap().id;
    let mut square = varde_sketch::Sketch::default();
    let corners = [(-25.0, 5.0), (-15.0, 5.0), (-15.0, 15.0), (-25.0, 15.0)]
        .map(|(x, y)| square.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let line = Curve::Line {
            start: corners[k],
            end: corners[(k + 1) % 4],
        };
        square.add_curve(line, false).unwrap();
    }
    let region = square.profiles().unwrap().reference(0).unwrap();
    editor
        .apply(editor.document().set_sketch_whole(hole, square))
        .unwrap();
    let cut = Extrude {
        taper: None,
        sketch: hole,
        regions: vec![region],
        extent: crate::tests::two_sides(editor.document(), "15", "5"),
        flip: false,
        operation: Operation::Cut(varde_document::Targets::default()),
    };
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    editor.apply(editor.document().add_sketch(xy)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    crate::tests::add_join(&mut editor);
    let (mut doc, _requests) = crate::tests::holding(editor.document().clone());
    assert!(doc.feed.failed_features().is_empty());
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    let revision = doc.editor.revision();
    doc.look(Look::ChangePlane(id));
    let walls = faces(
        &doc,
        |s| matches!(*s, Summary::Plane { n, d } if n == [1.0, 0.0, 0.0] && d == -25.0),
    );
    assert!(!walls.is_empty());
    let index = doc.feed.pick_index();
    let picking = doc.picking_plane.as_ref().unwrap();
    for &(wall, near) in &walls {
        assert!(!picking.pick.takes(index, wall));
        assert_eq!(picking.pick.face_ref(index, wall, near), None);
        let why = picking.pick.refusal(index, wall).unwrap();
        assert!(why.contains("can't be told"), "{why}");
    }
    // Asked for anyway, named by the body shown, it's refused.
    let (wall, near) = walls[0];
    let face = index.face_ref(wall, near).unwrap();
    doc.update(Edit::FacePicked(face));
    assert_eq!(doc.editor.revision(), revision);
    assert!(doc.notice.as_deref().unwrap().contains("can't be told"));
    // A new sketch at the end of the history takes it.
    doc.look(Look::Escape);
    doc.look(Look::PickPlane);
    let index = doc.feed.pick_index();
    let picking = doc.picking_plane.as_ref().unwrap();
    assert!(picking.pick.takes(index, wall));
}

#[test]
fn picking_a_plane_ends_when_the_document_turns_read_only() {
    // Saved somewhere it can't be written while a plane is picked from
    // the Sketch tab: backed out of, the sketch edited again.
    let (mut doc, id, _requests) = circle_on_the_top();
    doc.look(Look::EditFeature(id));
    doc.look(Look::ChangePlane(id));
    assert!(doc.picking_plane.is_some());
    doc.read_only = Some("read only".to_owned());
    doc.sync();
    assert!(doc.picking_plane.is_none());
    assert_eq!(edited(&doc), Some(id));
    // And for a new sketch.
    doc.read_only = None;
    doc.look(Look::FinishSketch);
    doc.look(Look::PickPlane);
    assert!(doc.picking_plane.is_some());
    doc.read_only = Some("read only".to_owned());
    doc.sync();
    assert!(doc.picking_plane.is_none());
    let bar = status_bar(&doc);
    assert!(!bar.iter().any(|t| t.contains("Pick a plane")), "{bar:?}");
}

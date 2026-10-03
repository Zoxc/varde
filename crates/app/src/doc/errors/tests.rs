use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use glam::DVec2;
use varde_document::{
    AxisLine, BodyId, Command, Document, Editor, FeatureId, FeatureKind, Operation, Revolve, Turn,
};
use varde_regen::{ErrorGeometry, Regenerator, Request};
use varde_sketch::Curve;
use varde_view::{Edit, Framing, Look, RevolveLook};

use super::*;
use crate::doc::revolve::tests::lathe;
use crate::tests::{answer, clicked, deferred, shown, texts};

/// The example, and a revolve of its plate about the sketch's y axis,
/// which crosses it, so it fails with the axis and the crossing segment
/// as its geometry: answered, the revolve's and the sketch's ids.
fn failing_revolve() -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId, FeatureId) {
    let mut editor = Editor::new(Document::example());
    let FeatureKind::Extrude(plate) = &editor.document().features()[1].kind else {
        panic!("the example's second feature is its extrude");
    };
    let revolve = Revolve {
        sketch: plate.sketch,
        regions: plate.regions.clone(),
        axis: AxisLine::SketchY,
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    let sketch = plate.sketch;
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let revolve = editor.document().features().last().unwrap().id;
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    (doc, requests, revolve, sketch)
}

/// The geometry of `feature`'s failure as the model shown found it.
fn failure(doc: &Doc, feature: FeatureId) -> Arc<ErrorGeometry> {
    doc.failure_geometry(feature).expect("geometry").clone()
}

/// Whether `doc` shows the geometry of exactly `geometry`, the very
/// `Arc`s, whole.
fn shows(doc: &Doc, geometry: &[&Arc<ErrorGeometry>]) -> bool {
    let shown: Vec<_> = doc.shown_errors().shown().collect();
    shown.len() == geometry.len()
        && (shown.iter().zip(geometry)).all(|(a, b)| a.lines && Arc::ptr_eq(&a.geometry, b))
}

#[test]
fn a_failed_feature_shows_while_its_row_is_hovered_or_selected() {
    let (mut doc, _requests, revolve, sketch) = failing_revolve();
    let geometry = failure(&doc, revolve);
    // An old failure isn't drawn of itself.
    assert!(doc.shown_errors().is_empty());

    doc.look(Look::HoverFeature(Some(revolve)));
    assert!(shows(&doc, &[&geometry]));
    doc.look(Look::HoverFeature(None));
    assert!(doc.shown_errors().is_empty());

    // A row that didn't fail shows nothing.
    doc.look(Look::HoverFeature(Some(sketch)));
    assert!(doc.shown_errors().is_empty());

    doc.look(Look::SelectFeature(revolve));
    assert!(shows(&doc, &[&geometry]));
    // Hovered and selected, it's drawn once.
    doc.look(Look::HoverFeature(Some(revolve)));
    assert!(shows(&doc, &[&geometry]));
    doc.look(Look::HoverFeature(None));
    doc.look(Look::SelectFeature(sketch));
    assert!(doc.shown_errors().is_empty());

    // Not while a sketch is edited, where the Timeline doesn't show: the
    // hover is let go of with the panel.
    doc.look(Look::HoverFeature(Some(revolve)));
    doc.look(Look::SelectPanel(varde_view::Panel::Objects));
    assert!(doc.shown_errors().is_empty());
}

/// A failed feature's Timeline row has no Show: only a failing draft's
/// panel does.
#[test]
fn a_failed_feature_s_timeline_row_has_no_show_button() {
    let (mut doc, _requests, _, _) = failing_revolve();
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(varde_view::Mode::Light), size, &mut renderer);
    let found = texts(&mut ui, &renderer);
    assert!(!found.iter().any(|shown| shown.text == "Show"), "{found:?}");
}

/// Show frames the camera on the box of where the draft fails, keeping
/// its direction, and turns into Go back, which turns it back to where
/// it was; fixed, the draft forgets that view.
#[test]
fn show_frames_the_draft_s_failure_and_go_back_returns() {
    let mut lathe = lathe();
    let across = crossing_line(&mut lathe);
    lathe.set_up(AxisLine::Curve(across));
    lathe.answer();
    let bounds = (lathe.doc.feed.draft_geometry())
        .and_then(|geometry| geometry.bounds())
        .expect("a box");
    let before = lathe.doc.camera;
    lathe.doc.look(Look::ShowFailure);
    let to = lathe.doc.animation.as_ref().expect("the camera turns").to;
    assert_eq!(to.target(), bounds.center());
    let diagonal = (bounds.max - bounds.min).length();
    assert!((to.view_height() - diagonal * FRAME_MARGIN).abs() < 1e-3 * diagonal);
    assert_eq!(to.backward(), before.backward());
    let framed = |doc: &Doc| doc.revolve_state().unwrap().show_error;
    assert_eq!(framed(&lathe.doc), Some(Framing::GoBack));

    // Shown again, it still goes back to the view before the first.
    lathe.doc.look(Look::ShowFailure);
    lathe.doc.look(Look::BackFromFailure);
    let to = lathe.doc.animation.as_ref().expect("the camera turns").to;
    assert_eq!(to.target(), before.target());
    assert_eq!(to.view_height(), before.view_height());
    assert_eq!(framed(&lathe.doc), Some(Framing::Show));

    // Shown, then fixed: nothing to go back from.
    lathe.doc.look(Look::ShowFailure);
    let axis = AxisLine::Curve(lathe.construction);
    let sketch = lathe.sketch;
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    lathe.answer();
    assert_eq!(framed(&lathe.doc), None);
    assert!(lathe.doc.before_show.is_none());
}

#[test]
fn an_unchanged_failure_keeps_its_arc_and_what_is_drawn_of_it() {
    let (mut doc, requests, revolve, _) = failing_revolve();
    // One regenerator throughout, as a lane keeps its cache.
    let mut regen = Regenerator::default();
    let mut answer = |doc: &mut Doc| {
        for request in requests.take() {
            doc.computed(regen.handle(request));
        }
    };
    let body = doc.editor.document().bodies()[0].id;
    doc.update(Edit::ToggleVisible(body));
    answer(&mut doc);
    doc.look(Look::SelectFeature(revolve));
    let geometry = failure(&doc, revolve);
    let drawn = doc.shown_errors().clone();
    assert!(shows(&doc, &[&geometry]));

    // Another edit, which the revolve doesn't depend on, answered anew.
    doc.update(Edit::ToggleVisible(body));
    answer(&mut doc);
    assert!(Arc::ptr_eq(&failure(&doc, revolve), &geometry));
    assert!(Arc::ptr_eq(doc.shown_errors(), &drawn));
}

/// Adds a construction line from (15, −5) to (15, 35) to the lathe's
/// sketch, across its rectangle: its id.
pub(crate) fn crossing_line(lathe: &mut crate::doc::revolve::tests::Lathe) -> varde_document::Id {
    let document = lathe.doc.editor.document();
    let Some(FeatureKind::Sketch { sketch, .. }) =
        document.feature(lathe.sketch).map(|feature| &feature.kind)
    else {
        panic!("the lathe's sketch");
    };
    let mut sketch = sketch.clone();
    let [start, end] =
        [(15.0, -5.0), (15.0, 35.0)].map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let line = sketch.add_curve(Curve::Line { start, end }, true).unwrap();
    lathe.doc.apply(Command::SetSketch {
        feature: lathe.sketch,
        sketch: Box::new(sketch),
    });
    lathe.doc.sync();
    lathe.answer();
    line
}

#[test]
fn a_failing_draft_shows_its_geometry_until_it_is_fixed() {
    let mut lathe = lathe();
    let across = crossing_line(&mut lathe);
    lathe.set_up(AxisLine::Curve(across));
    // Asked, not answered: the panel has no error yet, nor the viewport.
    assert!(lathe.doc.shown_errors().is_empty());
    lathe.answer();
    assert!(lathe.doc.feed.draft_error().is_some());
    let geometry = lathe.doc.feed.draft_geometry().expect("geometry").clone();
    assert!(shows(&lathe.doc, &[&geometry]));
    assert_eq!(
        lathe.doc.revolve_state().unwrap().show_error,
        Some(Framing::Show)
    );

    // Show beside the panel's Add anyway frames the draft's.
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let view = lathe.doc.view_in(varde_view::Mode::Light);
    let mut ui = shown(view, size, &mut renderer);
    let found = texts(&mut ui, &renderer);
    let show = (found.iter())
        .find(|shown| shown.text == "Show" && !shown.hidden())
        .expect("a Show button beside the panel's Add anyway");
    let sent = clicked(&mut ui, &mut renderer, show.bounds.center());
    assert!(
        matches!(sent[..], [varde_view::Message::Look(Look::ShowFailure)]),
        "{sent:?}"
    );
    drop(ui);
    lathe.doc.look(Look::ShowFailure);
    let to = lathe.doc.animation.as_ref().expect("the camera turns").to;
    assert_eq!(to.target(), geometry.bounds().unwrap().center());

    // Picking an axis that works: while it's on its way the error, and
    // the geometry, of the draft before don't show; answered, none.
    let sketch = lathe.sketch;
    let axis = AxisLine::Curve(lathe.construction);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    assert!(lathe.doc.shown_errors().is_empty());
    lathe.answer();
    assert_eq!(lathe.doc.feed.draft_error(), None);
    assert!(lathe.doc.shown_errors().is_empty());
    assert_eq!(lathe.doc.revolve_state().unwrap().show_error, None);

    // Failing again, then cancelled: nothing shows.
    lathe.revolve(RevolveLook::PickAxis {
        sketch,
        axis: AxisLine::Curve(across),
    });
    lathe.answer();
    assert!(!lathe.doc.shown_errors().is_empty());
    lathe.doc.look(Look::Escape);
    assert!(lathe.doc.shown_errors().is_empty());
}

#[test]
fn editing_a_failed_feature_shows_its_failure_and_then_its_draft_s() {
    let (mut doc, requests, revolve, _) = failing_revolve();
    let geometry = failure(&doc, revolve);
    doc.look(Look::EditFeature(revolve));
    assert!(doc.revolve.is_some());
    // Selected as it's edited, or not: its panel is open.
    doc.selected_feature = None;
    doc.look(Look::HoverFeature(None));
    assert!(shows(&doc, &[&geometry]));
    // The draft's answer: the same revolve, failing the same way; its
    // geometry wins, drawn once.
    answer(&mut doc, &requests);
    let draft = doc.feed.draft_geometry().expect("the draft fails").clone();
    assert!(shows(&doc, &[&draft]));
}

/// What [`touching_extrude`] makes: the sketch's and the extrude's ids,
/// the sketch, its corner, and the four lines at it, the second square's
/// last.
struct Touching {
    doc: Doc,
    requests: Rc<RefCell<Vec<Request>>>,
    sketch: FeatureId,
    extrude: FeatureId,
    drawn: varde_sketch::Sketch,
    corner: varde_document::Id,
    at_corner: [varde_document::Id; 4],
}

/// A sketch on XZ of two squares meeting at the corner (10, 10) only,
/// and an extrude of both, which fails touching there, answered.
fn touching_extrude() -> Touching {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(
            (editor.document()).add_sketch(varde_document::Plane::Origin(
                varde_document::OriginPlane::XZ,
            )),
        )
        .unwrap();
    let sketch = editor.document().features()[0].id;
    let mut drawn = varde_sketch::Sketch::default();
    let mut at_corner = Vec::new();
    let corner = drawn.add_point(DVec2::new(10.0, 10.0)).unwrap();
    for (a, b, c) in [
        ((0.0, 0.0), (10.0, 0.0), (0.0, 10.0)),
        ((20.0, 10.0), (20.0, 20.0), (10.0, 20.0)),
    ] {
        let [a, b, c] = [a, b, c].map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
        // Each counter-clockwise through the corner.
        let loop_ = if at_corner.is_empty() {
            [a, b, corner, c]
        } else {
            [corner, a, b, c]
        };
        for k in 0..4 {
            let (start, end) = (loop_[k], loop_[(k + 1) % 4]);
            let id = drawn.add_curve(Curve::Line { start, end }, false).unwrap();
            if start == corner || end == corner {
                at_corner.push(id);
            }
        }
    }
    let profiles = drawn.profiles().unwrap();
    let regions = (0..profiles.regions.len())
        .map(|index| profiles.reference(index).unwrap())
        .collect();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn.clone()),
        })
        .unwrap();
    let extrude = varde_document::Extrude {
        sketch,
        regions,
        extent: varde_document::Extent::OneSide(crate::tests::length(editor.document(), "5")),
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(extrude.into()))
        .unwrap();
    let extrude = editor.document().features().last().unwrap().id;
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    let at_corner = at_corner.try_into().expect("four lines at the corner");
    Touching {
        doc,
        requests,
        sketch,
        extrude,
        drawn,
        corner,
        at_corner,
    }
}

/// The curves the sketch being edited marks as failing.
fn failing(doc: &Doc) -> BTreeSet<varde_document::Id> {
    doc.sketch_state()
        .expect("a sketch is edited")
        .failing
        .clone()
}

#[test]
fn editing_the_sketch_of_a_failing_extrude_marks_the_curves_it_names() {
    let Touching {
        mut doc,
        requests,
        sketch,
        extrude,
        drawn,
        corner,
        at_corner,
    } = touching_extrude();
    let geometry = failure(&doc, extrude);
    let named = geometry.sketch_curves();
    assert!(!named.is_empty());
    // Not outside the sketch.
    assert!(doc.sketch.is_none());

    doc.look(Look::EditFeature(sketch));
    answer(&mut doc, &requests);
    let marked = failing(&doc);
    assert!(!marked.is_empty());
    assert!(marked.iter().all(|id| at_corner.contains(id)), "{marked:?}");
    assert_eq!(
        (marked.iter())
            .map(|id| u64::from(id.get()))
            .collect::<Vec<_>>(),
        named
    );
    // The 3D copy shows its points, where the squares touch, but not the
    // curves the sketch marks: selected or not.
    assert!(!geometry.points().is_empty());
    let points_only = |doc: &Doc| {
        let shown: Vec<_> = doc.shown_errors().shown().collect();
        let geometry = failure(doc, extrude);
        matches!(shown[..], [only] if !only.lines && Arc::ptr_eq(&only.geometry, &geometry))
    };
    assert!(points_only(&doc));
    doc.selected_feature = Some(extrude);
    doc.refresh_errors();
    assert!(points_only(&doc));
    // Left, it's drawn whole while selected; not selected, it's drawn
    // while its sketch is edited still.
    doc.look(Look::FinishSketch);
    assert!(doc.sketch.is_none());
    assert!(shows(&doc, &[&failure(&doc, extrude)]));
    doc.look(Look::EditFeature(sketch));
    answer(&mut doc, &requests);
    doc.selected_feature = None;
    doc.refresh_errors();
    assert!(points_only(&doc));

    // A curve it names deleted: not found, so not marked, while the
    // model shown still has the failure.
    let mut edited = drawn.clone();
    let deleted = *marked.first().unwrap();
    edited.delete(&[deleted]);
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(edited),
    });
    doc.sync();
    assert!(doc.failure_geometry(extrude).is_some());
    let left = failing(&doc);
    assert!(!left.contains(&deleted));
    assert_eq!(left.len(), marked.len() - 1);

    // Fixed: the second square's corner moved off the first's, the
    // extrude regenerates, and nothing is marked.
    let mut fixed = drawn;
    let moved = fixed.add_point(DVec2::new(12.0, 12.0)).unwrap();
    for &id in &at_corner[2..] {
        let entry = fixed.curve_mut(id).unwrap();
        let Curve::Line { start, end } = &mut entry.curve else {
            panic!("a line");
        };
        for point in [start, end] {
            if *point == corner {
                *point = moved;
            }
        }
    }
    doc.apply(Command::SetSketch {
        feature: sketch,
        sketch: Box::new(fixed),
    });
    doc.sync();
    answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    assert!(failing(&doc).is_empty());
}

/// What moving the cursor to `at` on `ui` sends.
fn moved(
    ui: &mut crate::tests::Headless<'_>,
    renderer: &mut iced::Renderer,
    at: iced::Point,
) -> Vec<varde_view::Message> {
    use iced::mouse::{Cursor, Event};
    let mut sent = Vec::new();
    let _ = ui.update(
        &[iced::Event::Mouse(Event::CursorMoved { position: at })],
        Cursor::Available(at),
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut sent,
    );
    sent
}

#[test]
fn moving_up_from_one_row_to_the_next_hovers_it() {
    let (mut doc, _requests, revolve, _) = failing_revolve();
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let extrude = doc.editor.document().features()[1].id;
    let name = |id: FeatureId| doc.editor.document().feature(id).unwrap().name.clone();
    let (extrude_name, revolve_name) = (name(extrude), name(revolve));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(varde_view::Mode::Light), size, &mut renderer);
    let found = texts(&mut ui, &renderer);
    let at = |name: &str| {
        (found.iter())
            .find(|shown| shown.text == name && !shown.hidden())
            .expect("the row")
            .bounds
            .center()
    };
    let (upper, lower) = (at(&extrude_name), at(&revolve_name));
    assert!(upper.y < lower.y);
    let mut sent = moved(&mut ui, &mut renderer, lower);
    // Straight from the lower row to the one above it, as one move.
    sent.extend(moved(&mut ui, &mut renderer, upper));
    drop(ui);
    for message in sent {
        if let varde_view::Message::Look(look) = message {
            doc.look(look);
        }
    }
    assert_eq!(doc.hovered_feature, Some(extrude));
}

#[test]
fn a_hovered_row_gone_without_an_exit_is_let_go_of() {
    // Into a sketch, where the Timeline doesn't show, and out again.
    let (mut doc, _requests, revolve, sketch) = failing_revolve();
    doc.look(Look::HoverFeature(Some(revolve)));
    assert!(!doc.shown_errors().is_empty());
    doc.look(Look::EditFeature(sketch));
    assert!(doc.sketch.is_some());
    doc.look(Look::FinishSketch);
    assert!(doc.sketch.is_none());
    assert_eq!(doc.hovered_feature, None);
    assert!(doc.shown_errors().is_empty());

    // Its feature removed, then the removal undone.
    let (mut doc, requests, revolve, _) = failing_revolve();
    doc.look(Look::HoverFeature(Some(revolve)));
    doc.apply(Command::RemoveFeature(revolve));
    doc.sync();
    assert_eq!(doc.hovered_feature, None);
    assert!(doc.shown_errors().is_empty());
    answer(&mut doc, &requests);
    doc.update(Edit::Undo);
    answer(&mut doc, &requests);
    assert!(doc.failure_geometry(revolve).is_some());
    assert!(doc.shown_errors().is_empty());
}

#[test]
fn an_edited_failure_goes_once_its_draft_works() {
    let mut lathe = lathe();
    let across = crossing_line(&mut lathe);
    let rectangle = lathe.rectangle();
    let document = lathe.doc.editor.document();
    let FeatureKind::Sketch { sketch, .. } = &document.feature(lathe.sketch).unwrap().kind else {
        panic!("a sketch");
    };
    let regions = vec![sketch.profiles().unwrap().reference(rectangle).unwrap()];
    let revolve = Revolve {
        sketch: lathe.sketch,
        regions,
        axis: AxisLine::Curve(across),
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    lathe.doc.apply(document.add_feature(revolve.into()));
    lathe.doc.sync();
    lathe.answer();
    let revolve = lathe.doc.editor.document().features().last().unwrap().id;
    assert!(lathe.doc.failure_geometry(revolve).is_some());

    lathe.doc.look(Look::EditFeature(revolve));
    assert!(lathe.doc.revolve.is_some());
    assert!(!lathe.doc.shown_errors().is_empty());
    // An axis that works, answered: the preview is whole, and no red is
    // over it.
    let sketch = lathe.sketch;
    let axis = AxisLine::Curve(lathe.construction);
    lathe.revolve(RevolveLook::PickAxis { sketch, axis });
    lathe.answer();
    assert_eq!(lathe.doc.feed.draft_error(), None);
    assert!(lathe.doc.shown_errors().is_empty());
}

#[test]
fn an_edited_feature_s_draft_on_its_way_shows_no_failure() {
    let (mut doc, requests, revolve, sketch) = failing_revolve();
    doc.look(Look::EditFeature(revolve));
    // The draft answered: failing as it was.
    answer(&mut doc, &requests);
    assert!(doc.feed.draft_geometry().is_some());
    // Changed, on its way: the panel shows no error, nor the viewport
    // the draft before's, selected or not.
    let axis = AxisLine::SketchX;
    doc.look(Look::Revolve(RevolveLook::PickAxis { sketch, axis }));
    assert_eq!(doc.feed.draft_error(), None);
    assert_eq!(doc.selected_feature, Some(revolve));
    assert!(doc.shown_errors().is_empty());
    doc.selected_feature = None;
    doc.refresh_errors();
    assert!(doc.shown_errors().is_empty());
}

/// A sketch on the example plate's hole wall, which isn't flat, fails
/// showing the wall: drawn while its Timeline row is selected or
/// hovered, framed by Show, not otherwise.
#[test]
fn a_sketch_on_a_curved_face_shows_the_face_while_its_row_is_selected() {
    let (mut doc, requests) = crate::tests::example();
    let index = doc.feed.pick_index();
    let mesh = index.mesh();
    let wall = (index.picking().faces().iter())
        .position(|face| matches!(face.summary, varde_regen::Summary::Cylinder { .. }))
        .expect("the plate's hole");
    let first = mesh.face_indices(wall).unwrap().start;
    let near = (mesh.indices()[first..first + 3].iter())
        .map(|&i| glam::Vec3::from(mesh.positions()[i as usize]).as_dvec3())
        .sum::<glam::DVec3>()
        / 3.0;
    let face = index.face_ref(wall as u32, near).expect("the wall's name");
    doc.apply((doc.editor.document()).add_sketch(varde_document::Plane::Face(face)));
    let sketch = doc.editor.document().features().last().unwrap().id;
    doc.sync();
    answer(&mut doc, &requests);
    let failed: Vec<_> = (doc.feed.failed_features().iter())
        .map(|failed| (failed.feature, failed.message.as_str()))
        .collect();
    assert_eq!(failed, [(sketch, "its face isn't flat")]);
    let geometry = failure(&doc, sketch);
    assert!(geometry.mesh().triangle_count() > 0);
    doc.selected_feature = None;
    doc.refresh_errors();
    assert!(doc.shown_errors().is_empty());

    doc.look(Look::SelectFeature(sketch));
    assert!(shows(&doc, &[&geometry]));
    doc.look(Look::HoverFeature(Some(sketch)));
    assert!(shows(&doc, &[&geometry]));
}

/// A revolve about a line of no length fails, showing the line's place;
/// while its sketch is edited, the line is marked in it and the place
/// still shown.
#[test]
fn editing_the_sketch_of_a_revolve_about_a_line_of_no_length_marks_the_line() {
    let mut editor = Editor::new(Document::default());
    editor
        .apply(
            (editor.document()).add_sketch(varde_document::Plane::Origin(
                varde_document::OriginPlane::XZ,
            )),
        )
        .unwrap();
    let sketch = editor.document().features()[0].id;
    let mut drawn = varde_sketch::Sketch::default();
    let corners = [(5.0, 0.0), (10.0, 0.0), (10.0, 4.0), (5.0, 4.0)]
        .map(|(x, y)| drawn.add_point(DVec2::new(x, y)).unwrap());
    for k in 0..4 {
        let (start, end) = (corners[k], corners[(k + 1) % 4]);
        drawn.add_curve(Curve::Line { start, end }, false).unwrap();
    }
    let [start, end] = [0, 1].map(|_| drawn.add_point(DVec2::new(0.0, 2.0)).unwrap());
    let line = drawn.add_curve(Curve::Line { start, end }, true).unwrap();
    let profiles = drawn.profiles().unwrap();
    editor
        .apply(Command::SetSketch {
            feature: sketch,
            sketch: Box::new(drawn),
        })
        .unwrap();
    let revolve = Revolve {
        sketch,
        regions: vec![profiles.reference(0).unwrap()],
        axis: AxisLine::Curve(line),
        extent: Turn::Full,
        flip: false,
        operation: Operation::NewBody(BodyId::NEW),
    };
    editor
        .apply(editor.document().add_feature(revolve.into()))
        .unwrap();
    let revolve = editor.document().features().last().unwrap().id;
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(editor.document().clone())));
    doc.sync();
    answer(&mut doc, &requests);
    let geometry = failure(&doc, revolve);
    assert_eq!(geometry.sketch_curves(), [u64::from(line.get())]);
    assert_eq!(geometry.points().len(), 1);

    doc.look(Look::EditFeature(sketch));
    answer(&mut doc, &requests);
    assert_eq!(failing(&doc), BTreeSet::from([line]));
    // Its place is shown in 3D, the line left to the sketch.
    let shown: Vec<_> = doc.shown_errors().shown().collect();
    assert!(
        matches!(shown[..], [only] if !only.lines && Arc::ptr_eq(&only.geometry, &failure(&doc, revolve))),
        "{}",
        shown.len()
    );
}

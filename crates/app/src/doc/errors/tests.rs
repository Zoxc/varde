use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use glam::DVec2;
use varde_document::{
    AxisLine, BodyId, Command, Document, Editor, FeatureId, FeatureKind, Operation, Revolve, Turn,
};
use varde_regen::{ErrorGeometry, Regenerator, Request};
use varde_sketch::Curve;
use varde_view::{Edit, Look, RevolveLook};

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
/// `Arc`s.
fn shows(doc: &Doc, geometry: &[&Arc<ErrorGeometry>]) -> bool {
    let shown: Vec<_> = doc.shown_errors().geometry().collect();
    shown.len() == geometry.len() && (shown.iter().zip(geometry)).all(|(a, b)| Arc::ptr_eq(a, b))
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

#[test]
fn the_timeline_row_hovered_and_its_show_button_send_their_messages() {
    let (mut doc, _requests, revolve, _) = failing_revolve();
    doc.look(Look::SelectPanel(varde_view::Panel::Timeline));
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view_in(varde_view::Mode::Light), size, &mut renderer);
    let found = texts(&mut ui, &renderer);
    let show = (found.iter())
        .find(|shown| shown.text == "Show" && !shown.hidden())
        .expect("a Show button on the failed row");
    let sent = clicked(&mut ui, &mut renderer, show.bounds.center());
    use varde_view::Message as Ui;
    assert!(
        sent.iter()
            .any(|sent| matches!(sent, Ui::Look(Look::HoverFeature(Some(id))) if *id == revolve)),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|sent| matches!(sent, Ui::Look(Look::ShowFailure(Some(id))) if *id == revolve)),
        "{sent:?}"
    );
    // The button takes the click: the row isn't selected by it.
    assert!(
        !(sent.iter()).any(|sent| matches!(sent, Ui::Look(Look::SelectFeature(_)))),
        "{sent:?}"
    );
}

#[test]
fn show_frames_the_camera_on_the_failure_s_box() {
    let (mut doc, _requests, revolve, _) = failing_revolve();
    let bounds = failure(&doc, revolve).bounds().expect("a box");
    doc.look(Look::ShowFailure(Some(revolve)));
    let to = doc.animation.as_ref().expect("the camera turns").to;
    assert_eq!(to.target(), bounds.center());
    let diagonal = (bounds.max - bounds.min).length();
    assert!((to.view_height() - diagonal * FRAME_MARGIN).abs() < 1e-3 * diagonal);
    // Its direction is kept.
    assert_eq!(to.backward(), doc.camera.backward());

    // A feature that didn't fail frames nothing.
    let (mut doc, _requests, _, sketch) = failing_revolve();
    doc.look(Look::ShowFailure(Some(sketch)));
    assert!(doc.animation.is_none());
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
fn crossing_line(lathe: &mut crate::doc::revolve::tests::Lathe) -> varde_document::Id {
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
    assert!(lathe.doc.revolve_state().unwrap().show_error);

    // Show beside the panel's error frames the draft's.
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let view = lathe.doc.view_in(varde_view::Mode::Light);
    let mut ui = shown(view, size, &mut renderer);
    let found = texts(&mut ui, &renderer);
    let show = (found.iter())
        .find(|shown| shown.text == "Show" && !shown.hidden())
        .expect("a Show button beside the panel's error");
    let sent = clicked(&mut ui, &mut renderer, show.bounds.center());
    assert!(
        matches!(
            sent[..],
            [varde_view::Message::Look(Look::ShowFailure(None))]
        ),
        "{sent:?}"
    );
    drop(ui);
    lathe.doc.look(Look::ShowFailure(None));
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
    assert!(!lathe.doc.revolve_state().unwrap().show_error);

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

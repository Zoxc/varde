use std::cell::RefCell;
use std::rc::Rc;

use iced::keyboard::{self, key};
use varde_document::{BodyId, FeatureId};
use varde_regen::Request;
use varde_view::{Edit, ExtentKind, ExtrudeLook, Look, Mode, OperationKind};

use super::*;
use crate::doc::Dialog;
use crate::tests::{key_in, press_in};

/// A document holding the example: "Sketch 1", and "Extrude 1" making
/// "Body 1" of it, answered by the regeneration lane; the three ids.
fn example() -> (Doc, FeatureId, FeatureId, BodyId) {
    let (doc, _) = crate::tests::example();
    let document = doc.editor.document();
    let [sketch, extrude] = [0, 1].map(|k| document.features()[k].id);
    let body = document.bodies()[0].id;
    (doc, sketch, extrude, body)
}

/// The names of the features and bodies of `doc`'s document.
fn names(doc: &Doc) -> (Vec<&str>, Vec<&str>) {
    let document = doc.editor.document();
    let features = document.features().iter().map(|f| f.name.as_str());
    let bodies = document.bodies().iter().map(|b| b.name.as_str());
    (features.collect(), bodies.collect())
}

fn delete_key() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Delete)
}

fn enter_key() -> keyboard::Key {
    keyboard::Key::Named(key::Named::Enter)
}

#[test]
fn deleting_a_sketch_an_extrude_uses_asks_first() {
    let (mut doc, sketch, _, _) = example();
    let before = doc.editor.document().clone();
    doc.look(Look::SelectFeature(sketch));
    key_in(&mut doc, delete_key());

    // Nothing's gone yet: the prompt lists the sketch, the extrude using
    // it and the body that makes, in the Timeline's order.
    assert_eq!(*doc.editor.document(), before);
    assert_eq!(doc.dialog(), Some(Dialog::Delete));
    let prompt = doc.delete_prompt().expect("the prompt shows");
    assert_eq!(prompt.name, "Sketch 1");
    let listed: Vec<_> = prompt.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(listed, ["Sketch 1", "Extrude 1"]);
    let listed: Vec<_> = prompt.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(listed, ["Body 1"]);
    let _ = doc.view(false, Mode::default(), true);
    // No shortcut acts behind it, `Enter` included.
    assert!(doc.keys().is_none());
    assert!(press_in(&doc, enter_key()).is_none());
    assert!(press_in(&doc, delete_key()).is_none());

    // Cancel changes nothing.
    doc.look(Look::CancelDelete);
    assert!(doc.delete_prompt().is_none());
    assert_eq!(*doc.editor.document(), before);
    assert!(!doc.editor.can_redo());
    // Nor does `Esc`, which backs out of the prompt first.
    doc.update(Edit::RemoveFeature(sketch));
    doc.look(Look::Escape);
    assert!(doc.deleting.is_none());
    assert_eq!(doc.selected_feature, Some(sketch));
    assert_eq!(*doc.editor.document(), before);

    // Delete removes exactly what was listed, as one undo step.
    doc.update(Edit::RemoveFeature(sketch));
    doc.update(Edit::ConfirmDelete);
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc), (vec![], vec![]));
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

#[test]
fn deleting_what_nothing_depends_on_doesnt_ask() {
    let (mut doc, sketch, extrude, body) = example();
    let before = doc.editor.document().clone();

    // An extrude goes with its body.
    doc.update(Edit::RemoveFeature(extrude));
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc), (vec!["Sketch 1"], vec![]));
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);

    // A body with the extrude making it.
    doc.update(Edit::RemoveBody(body));
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc), (vec!["Sketch 1"], vec![]));

    // A sketch nothing uses.
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc), (vec![], vec![]));
    doc.update(Edit::Undo);
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

#[test]
fn the_prompt_goes_when_the_document_changes_under_it() {
    let (mut doc, sketch, extrude, _) = example();
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.deleting.is_some());
    // Say recovery: anything but the prompt or a sketch edit committing.
    doc.apply(Command::SetFeatureVisible(extrude, false));
    doc.sync();
    assert!(doc.deleting.is_none());
    assert_eq!(doc.dialog(), None);
    doc.update(Edit::ConfirmDelete);
    assert_eq!(names(&doc).0, ["Sketch 1", "Extrude 1"]);

    // Undo drops it too, and a prompt of an older document never shows.
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.deleting.is_some());
    doc.update(Edit::Undo);
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc).0, ["Sketch 1", "Extrude 1"]);
}

#[test]
fn a_read_only_document_asks_nothing() {
    let (mut doc, sketch, _, _) = example();
    doc.read_only = Some("test".to_owned());
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.deleting.is_none());
    assert_eq!(names(&doc).0, ["Sketch 1", "Extrude 1"]);
}

/// The texts of `doc`'s status bar, left to right, at 1280 × 800, with
/// its hints of the mouse if `mouse_hints`.
fn status_bar_of(doc: &Doc, mouse_hints: bool) -> Vec<String> {
    use crate::tests::{shown, texts};
    let size = iced::Size::new(1280.0, 800.0);
    let top = size.height - varde_view::STATUS_BAR_ROOM;
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(
        doc.view(false, Mode::Light, mouse_hints),
        size,
        &mut renderer,
    );
    // It floats over the viewport, right of the side panel.
    let mut bar: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|text| text.bounds.y >= top && text.bounds.x >= varde_view::SIDE_PANEL_WIDTH)
        .collect();
    bar.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    bar.into_iter().map(|text| text.text).collect()
}

/// [`status_bar_of`] with the hints of the mouse.
fn status_bar(doc: &Doc) -> Vec<String> {
    status_bar_of(doc, true)
}

#[test]
fn the_status_bar_says_what_is_selected_and_under_the_prompt_only_esc() {
    use crate::tests::{shown, texts};

    let (mut doc, sketch, extrude, _) = example();
    // Nothing selected, nothing going on: only the hints.
    let bar = status_bar(&doc);
    assert_eq!(
        bar,
        [
            "Select",
            "Body",
            "Drag to orbit",
            "Pan",
            "Zoom",
            "Click to set pivot"
        ]
    );
    assert!(status_bar_of(&doc, false).is_empty());
    // The feature selected, in its box, with the key clearing it, then
    // the hints, those of the keys without the mouse's.
    doc.look(Look::SelectFeature(extrude));
    let bar = status_bar(&doc);
    assert_eq!(
        bar,
        [
            "Extrude 1",
            "Distance 10 mm · New body",
            "Space",
            "Clear",
            "Enter",
            "Edit",
            "Del",
            "Delete",
            "Drag to orbit",
            "Pan",
            "Zoom",
            "Click to set pivot"
        ],
    );
    let bar = status_bar_of(&doc, false);
    assert_eq!(bar[bar.len() - 4..], ["Enter", "Edit", "Del", "Delete"]);

    // Under the delete prompt, which counts the body too, only `Esc`
    // does anything.
    doc.look(Look::SelectFeature(sketch));
    doc.update(Edit::RemoveFeature(sketch));
    assert!(doc.delete_prompt().is_some());
    let bar = status_bar(&doc);
    assert_eq!(bar[0], "Sketch 1", "{bar:?}");
    assert_eq!(bar[bar.len() - 2..], ["Esc", "Cancel"], "{bar:?}");
    assert!(!bar.contains(&"Delete".to_owned()), "{bar:?}");
    assert!(!bar.contains(&"Pan".to_owned()), "{bar:?}");
    let size = iced::Size::new(1280.0, 800.0);
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light, true), size, &mut renderer);
    let question = "Delete Sketch 1 with the 1 feature and 1 body that depend on it?";
    assert!(
        texts(&mut ui, &renderer)
            .iter()
            .any(|text| text.text == question)
    );
}

/// The example of [`example`] with "Sketch 2" after it, a circle about
/// (-20, 10) on the plate, and "Extrude 2" cutting it through all from
/// Body 1, answered by the regeneration lane; the regeneration requests,
/// which wait for the test, and the cut's id.
fn example_and_a_cut() -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId) {
    let (doc, requests, cut) = example_and_a_cut_of(false);
    let body = doc.editor.document().bodies()[0].id;
    assert_eq!(doc.feed.touched_features(), [(cut, vec![body])]);
    (doc, requests, cut)
}

/// [`example_and_a_cut`], with "Extrude 2" a boss of a circle of
/// radius 5 about the same centre, in "Sketch 3", making "Body 2" before
/// the cut, "Extrude 3", if `boss`, which then cuts both bodies (the
/// boss to a ring, as a cut leaving nothing of a body fails).
fn example_and_a_cut_of(boss: bool) -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId) {
    let (mut doc, requests, cut) = a_cut(boss);
    crate::tests::answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    (doc, requests, cut)
}

/// [`example_and_a_cut_of`], with the cut committed but its model not
/// answered yet: the model shown doesn't know what it touches.
fn example_and_an_unanswered_cut(boss: bool) -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId) {
    let (doc, requests, cut) = a_cut(boss);
    requests.take();
    assert!(
        (doc.feed.touched_features().iter()).all(|(id, _)| *id != cut),
        "{:?}",
        doc.feed.touched_features()
    );
    (doc, requests, cut)
}

/// [`example_and_a_cut_of`] up to the cut committed, its model asked
/// for.
fn a_cut(boss: bool) -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId) {
    let (mut doc, sketch, requests) = crate::tests::example_and_a_hole();
    let extrude = |doc: &mut Doc, look| doc.look(Look::Extrude(look));
    if boss {
        let plane = varde_document::Plane::Origin(varde_document::OriginPlane::XY);
        doc.apply(doc.editor.document().add_sketch(plane));
        let ring = doc.editor.document().features().last().unwrap().id;
        let mut drawn = varde_sketch::Sketch::default();
        let center = drawn.add_point(glam::DVec2::new(-20.0, 10.0)).unwrap();
        drawn
            .add_curve(
                varde_sketch::Curve::Circle {
                    center,
                    radius: 5.0,
                },
                false,
            )
            .unwrap();
        doc.apply(Command::SetSketch {
            feature: ring,
            sketch: Box::new(drawn),
        });
        doc.sync();
        crate::tests::answer(&mut doc, &requests);
        doc.look(Look::SelectFeature(ring));
        doc.look(Look::StartExtrude);
        extrude(
            &mut doc,
            ExtrudeLook::PickRegion {
                sketch: ring,
                region: 0,
            },
        );
        doc.update(Edit::CommitExtrude);
        assert!(doc.extrude.is_none());
        crate::tests::answer(&mut doc, &requests);
        assert_eq!(doc.editor.document().bodies().len(), 2);
    }
    doc.look(Look::SelectFeature(sketch));
    doc.look(Look::StartExtrude);
    extrude(&mut doc, ExtrudeLook::PickRegion { sketch, region: 0 });
    extrude(&mut doc, ExtrudeLook::Operation(OperationKind::Cut));
    extrude(&mut doc, ExtrudeLook::Extent(ExtentKind::ThroughAll));
    doc.update(Edit::CommitExtrude);
    assert!(doc.extrude.is_none());
    doc.sync();
    let cut = doc.editor.document().features().last().unwrap().id;
    (doc, requests, cut)
}

/// The names of what the delete prompt warns of: the features that may
/// fail, and the bodies they worked on.
fn warned(doc: &Doc) -> (Vec<&str>, Vec<&str>) {
    let prompt = doc.delete_prompt().expect("the prompt shows");
    let features = prompt.worked.iter().map(|f| f.name.as_str());
    let bodies = prompt.worked_on.iter().map(|b| b.name.as_str());
    (features.collect(), bodies.collect())
}

const CUT_WARNING: &str =
    "Extrude 2 works on Body 1 and stays, so it may fail with nothing to work on.";

#[test]
fn deleting_a_body_a_cut_worked_on_warns_and_the_cut_then_fails() {
    let (mut doc, requests, cut) = example_and_a_cut();
    let before = doc.editor.document().clone();
    let body = before.bodies()[0].id;
    doc.update(Edit::RemoveBody(body));

    // Only its own feature goes with it, but the cut that stays is
    // warned of.
    assert_eq!(*doc.editor.document(), before);
    assert_eq!(doc.dialog(), Some(Dialog::Delete));
    let prompt = doc.delete_prompt().unwrap();
    assert_eq!(prompt.name, "Body 1");
    assert!(prompt.body);
    let listed: Vec<_> = prompt.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(listed, ["Extrude 1"]);
    let listed: Vec<_> = prompt.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(listed, ["Body 1"]);
    assert_eq!(warned(&doc), (vec!["Extrude 2"], vec!["Body 1"]));
    let texts = crate::tests::screen_texts(&doc);
    let question = "Delete Body 1 and Extrude 1, which makes it?";
    assert!(texts.iter().any(|text| text == question), "{texts:?}");
    assert!(texts.iter().any(|text| text == CUT_WARNING), "{texts:?}");

    // Delete takes the body and its extrude; the cut stays, and fails
    // as warned.
    doc.update(Edit::ConfirmDelete);
    assert_eq!(
        names(&doc),
        (vec!["Sketch 1", "Sketch 2", "Extrude 2"], vec![])
    );
    crate::tests::answer(&mut doc, &requests);
    // Through all, it has nothing to go through either.
    assert_eq!(
        doc.feed.failed_features(),
        [(cut, "there's no body to go through".to_owned())]
    );
    doc.update(Edit::Undo);
    assert_eq!(*doc.editor.document(), before);
}

#[test]
fn deleting_the_sketch_under_a_body_a_cut_worked_on_warns_too() {
    let (mut doc, _, _) = example_and_a_cut();
    let sketch = doc.editor.document().features()[0].id;
    doc.update(Edit::RemoveFeature(sketch));
    let listed: Vec<_> = (doc.delete_prompt().unwrap().features.iter())
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(listed, ["Sketch 1", "Extrude 1"]);
    assert_eq!(warned(&doc), (vec!["Extrude 2"], vec!["Body 1"]));
    assert!(
        crate::tests::screen_texts(&doc)
            .iter()
            .any(|text| text == CUT_WARNING)
    );

    // Not when the cut goes too, with its sketch: deleting the cut's
    // sketch warns of nothing, and goes without asking but for the cut.
    doc.look(Look::CancelDelete);
    let second = doc.editor.document().features()[2].id;
    doc.update(Edit::RemoveFeature(second));
    assert_eq!(warned(&doc), (vec![], vec![]));
    assert!(
        !crate::tests::screen_texts(&doc)
            .iter()
            .any(|text| text.contains("may fail"))
    );
}

#[test]
fn a_cut_that_took_the_body_out_isnt_warned_of() {
    let (mut doc, requests, cut) = example_and_a_cut();
    let body = doc.editor.document().bodies()[0].id;
    let Some(varde_document::FeatureKind::Extrude(extrude)) =
        doc.editor.document().feature(cut).map(|f| f.kind.clone())
    else {
        panic!("the cut is an extrude");
    };
    let mut extrude = Box::new(extrude);
    extrude.operation = varde_document::Operation::Cut(varde_document::Targets {
        excluded: vec![body],
    });
    doc.apply(Command::SetExtrude {
        feature: cut,
        extrude,
    });
    doc.sync();
    crate::tests::answer(&mut doc, &requests);
    assert_eq!(doc.feed.touched_features(), [(cut, vec![])]);

    // The body goes with its extrude without asking.
    doc.update(Edit::RemoveBody(body));
    assert!(doc.deleting.is_none());
    assert_eq!(
        names(&doc),
        (vec!["Sketch 1", "Sketch 2", "Extrude 2"], vec![])
    );
}

#[test]
fn a_cut_left_a_body_to_work_on_isnt_warned_of() {
    let (mut doc, _, cut) = example_and_a_cut_of(true);
    let bodies: Vec<_> = (doc.editor.document().bodies().iter())
        .map(|body| body.id)
        .collect();
    assert_eq!(doc.feed.touched_features(), [(cut, bodies.clone())]);
    // Either body goes with its extrude without asking: the cut still
    // has the other to work on.
    for body in bodies {
        doc.update(Edit::RemoveBody(body));
        assert!(doc.deleting.is_none());
        assert_eq!(doc.editor.document().bodies().len(), 1);
        doc.update(Edit::Undo);
        assert_eq!(doc.editor.document().bodies().len(), 2);
    }
}

/// A cut committed before its model comes back, deleting the only body
/// before it: it's warned of, as it surely has nothing left to work on.
#[test]
fn a_cut_not_answered_yet_is_warned_of_when_every_body_before_it_goes() {
    let (mut doc, requests, cut) = example_and_an_unanswered_cut(false);
    let body = doc.editor.document().bodies()[0].id;
    doc.update(Edit::RemoveBody(body));
    assert_eq!(doc.dialog(), Some(Dialog::Delete));
    assert_eq!(warned(&doc), (vec!["Extrude 2"], vec!["Body 1"]));
    assert!(
        crate::tests::screen_texts(&doc)
            .iter()
            .any(|text| text == CUT_WARNING)
    );
    doc.update(Edit::ConfirmDelete);
    crate::tests::answer(&mut doc, &requests);
    assert_eq!(
        doc.feed.failed_features(),
        [(cut, "there's no body to go through".to_owned())]
    );
}

/// With another body before it that stays, a cut not answered yet isn't
/// warned of: it may work on that one.
#[test]
fn a_cut_not_answered_yet_isnt_warned_of_while_a_body_stays() {
    let (mut doc, _, _) = example_and_an_unanswered_cut(true);
    let bodies: Vec<_> = (doc.editor.document().bodies().iter())
        .map(|body| body.id)
        .collect();
    assert_eq!(bodies.len(), 2);
    for body in bodies {
        doc.update(Edit::RemoveBody(body));
        assert!(doc.deleting.is_none());
        doc.update(Edit::Undo);
        assert_eq!(doc.editor.document().bodies().len(), 2);
    }
}

/// Across a replacement, before its model comes back, the cut isn't
/// known to the model shown; deleting the body still warns of it.
#[test]
fn a_cut_is_warned_of_across_a_replacement_before_its_model() {
    let (mut doc, _, _) = example_and_a_cut();
    // The same features and bodies, one sketch shown or hidden: a replacement
    // that changes something.
    let mut other = varde_document::Editor::new(doc.editor.document().clone());
    let sketch = &other.document().features()[0];
    let shown = Command::SetFeatureVisible(sketch.id, !sketch.visible);
    other.apply(shown).unwrap();
    doc.apply(Command::Replace(Box::new(other.document().clone())));
    doc.sync();
    assert!(doc.feed.touched_features().is_empty());
    let body = doc.editor.document().bodies()[0].id;
    doc.update(Edit::RemoveBody(body));
    assert_eq!(warned(&doc), (vec!["Extrude 2"], vec!["Body 1"]));
}

/// [`crate::tests::merged_plates`] with "Extrude 4" after the join
/// cutting a disc about (-20, 10) through both plates, which touches only
/// Body 1, the merged body; the document and the cut.
fn merged_and_cut() -> (Doc, Rc<RefCell<Vec<Request>>>, FeatureId) {
    let (mut editor, [top, below], _) = crate::tests::merged_plates();
    let through = crate::tests::two_sides(editor.document(), "20", "10");
    let cut = varde_document::Operation::Cut(varde_document::Targets::default());
    let cut = crate::tests::add_disc(&mut editor, (-20.0, 10.0), through, cut);
    let (doc, requests) = crate::tests::holding(editor.document().clone());
    assert!(doc.feed.failed_features().is_empty());
    assert_eq!(doc.feed.merged_bodies(), [(below, top)]);
    assert!(doc.feed.touched_features().contains(&(cut, vec![top])));
    (doc, requests, cut)
}

#[test]
fn a_cut_of_a_merged_body_isn_t_warned_of_while_a_body_merged_into_it_stays() {
    let (mut doc, requests, cut) = merged_and_cut();
    let top = doc.editor.document().bodies()[0].id;
    doc.update(Edit::RemoveBody(top));
    // Body 2 then holds the join and the cut: nothing to warn of, so
    // it's deleted at once.
    assert_eq!(doc.dialog(), None);
    assert_eq!(names(&doc).1, ["Body 2"]);
    crate::tests::answer(&mut doc, &requests);
    assert!(doc.feed.failed_features().is_empty());
    let below = doc.editor.document().bodies()[0].id;
    assert!(doc.feed.touched_features().contains(&(cut, vec![below])));
}

#[test]
fn a_cut_of_a_merged_body_is_warned_of_when_every_body_in_it_goes() {
    let (mut doc, _, _) = merged_and_cut();
    let sketch = doc.editor.document().features()[0].id;
    doc.update(Edit::RemoveFeature(sketch));
    // Both plates are of Sketch 1: the join and the cut stay with
    // nothing to work on.
    assert_eq!(doc.dialog(), Some(Dialog::Delete));
    assert_eq!(
        warned(&doc),
        (vec!["Extrude 3", "Extrude 4"], vec!["Body 1", "Body 2"])
    );
}

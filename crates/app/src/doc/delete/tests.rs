use iced::keyboard::{self, key};
use varde_document::{BodyId, Document, FeatureId};
use varde_view::{Edit, Look, Mode};

use super::*;
use crate::doc::Dialog;
use crate::tests::{answer, deferred, key_in, press_in};

/// A document holding the example: "Sketch 1", and "Extrude 1" making
/// "Body 1" of it, answered by the regeneration lane; the three ids.
fn example() -> (Doc, FeatureId, FeatureId, BodyId) {
    let (mut doc, requests) = deferred();
    doc.apply(Command::Replace(Box::new(Document::example())));
    doc.sync();
    answer(&mut doc, &requests);
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
    let _ = doc.view(false, Mode::default());
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
    // Say recovery, or a proposal committing: anything but the prompt.
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

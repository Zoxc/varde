use iced::keyboard::{self, key};
use varde_document::{BodyId, FeatureId};
use varde_view::{Edit, Look, Mode};

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

/// The texts of `doc`'s status bar, left to right, at 1280 × 800.
fn status_bar(doc: &Doc) -> Vec<String> {
    use crate::tests::{shown, texts};
    let size = iced::Size::new(1280.0, 800.0);
    let top = size.height - varde_view::STATUS_BAR_HEIGHT;
    let mut renderer = varde_view::probe::renderer();
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let mut bar: Vec<_> = texts(&mut ui, &renderer)
        .into_iter()
        .filter(|text| text.bounds.y >= top)
        .collect();
    bar.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
    bar.into_iter().map(|text| text.text).collect()
}

#[test]
fn the_status_bar_says_what_is_selected_and_under_the_prompt_only_esc() {
    use crate::tests::{shown, texts};

    let (mut doc, sketch, extrude, _) = example();
    let bar = status_bar(&doc);
    assert_eq!(bar[0], "No selection · 1 body · 2 features · mm", "{bar:?}");
    assert!(
        !bar.iter().any(|text| text.contains("triangles")),
        "{bar:?}"
    );
    doc.look(Look::SelectFeature(extrude));
    let bar = status_bar(&doc);
    assert_eq!(
        bar[..2],
        ["Extrude 1", "Distance 10 mm · New body"],
        "{bar:?}"
    );
    assert!(bar.contains(&"Edit".to_owned()), "{bar:?}");

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
    let mut ui = shown(doc.view(false, Mode::Light), size, &mut renderer);
    let question = "Delete Sketch 1 with the 1 feature and 1 body that depend on it?";
    assert!(
        texts(&mut ui, &renderer)
            .iter()
            .any(|text| text.text == question)
    );
}

//! Renaming features, sketches and bodies in the side panel.

use varde_document::Named;

use super::*;

/// The name of `target` in `doc`'s document.
fn name(doc: &Doc, target: Named) -> String {
    doc.editor.document().name_of(target).unwrap().to_owned()
}

/// Renames `target` to `text` through the rename field, as `F2` or the
/// context menu opens it and `Enter` takes it.
fn rename(doc: &mut Doc, target: Named, text: &str) {
    doc.look(Look::StartRename(target));
    assert!(doc.take_rename_focus());
    doc.look(Look::RenameInput(text.to_owned()));
    doc.update(Edit::CommitRename);
}

/// A feature renamed in its row is undone and redone like any edit.
#[test]
fn a_feature_is_renamed_undoably() {
    let (mut doc, feature, _) = with_sketch();
    let target = Named::Feature(feature);
    let old = name(&doc, target);
    doc.look(Look::StartRename(target));
    // The field holds the name to start with.
    assert_eq!(doc.renaming.as_ref().unwrap().text, old);
    doc.look(Look::RenameInput("Outline".into()));
    doc.update(Edit::CommitRename);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), "Outline");
    assert_eq!(doc.take_toast(), None);
    doc.update(Edit::Undo);
    assert_eq!(name(&doc, target), old);
    doc.update(Edit::Redo);
    assert_eq!(name(&doc, target), "Outline");
}

/// A name another feature or body has gets a number, and a toast says
/// the name was taken.
#[test]
fn a_name_taken_is_numbered_and_said() {
    let (mut doc, _) = example();
    let body = Named::Body(doc.editor.document().bodies()[0].id);
    let sketch = doc.editor.document().features()[0].name.clone();
    rename(&mut doc, body, &sketch);
    assert_eq!(name(&doc, body), format!("{sketch} (1)"));
    let toast = doc.take_toast().expect("a toast");
    assert!(toast.contains(&sketch) && toast.contains("already exists"));
    assert_eq!(doc.take_toast(), None);
}

/// `Esc` closes the field renaming nothing; anything done but hovering
/// renames, an empty name nothing.
#[test]
fn escape_cancels_and_other_actions_rename() {
    let (mut doc, feature, _) = with_sketch();
    let target = Named::Feature(feature);
    let old = name(&doc, target);
    doc.look(Look::StartRename(target));
    doc.look(Look::RenameInput("Gone".into()));
    doc.look(Look::Escape);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), old);

    doc.look(Look::StartRename(target));
    doc.look(Look::RenameInput("Kept".into()));
    doc.look(Look::HoverFeature(Some(feature)));
    assert!(doc.renaming.is_some());
    doc.look(Look::ClearSelection);
    assert!(doc.renaming.is_none());
    assert_eq!(name(&doc, target), "Kept");

    rename(&mut doc, target, "   ");
    assert_eq!(name(&doc, target), "Kept");
}

/// `F2` renames the feature selected in the Timeline, and nothing is
/// renamed in a read-only document.
#[test]
fn f2_renames_the_feature_selected() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectFeature(feature));
    let keys = doc.keys().unwrap();
    assert_eq!(keys.rename, Some(Named::Feature(feature)));
    let f2 = iced::keyboard::Key::Named(iced::keyboard::key::Named::F2);
    let sent = varde_view::pressed(
        varde_view::document_bindings(keys),
        &f2,
        iced::keyboard::Modifiers::empty(),
    );
    assert!(
        matches!(sent, Some(varde_view::Message::Look(Look::StartRename(Named::Feature(f)))) if f == feature),
        "{sent:?}"
    );

    doc.read_only = Some("read-only".into());
    doc.look(Look::StartRename(Named::Feature(feature)));
    assert!(doc.renaming.is_none());
}

/// The field opens in the row in place of its name and note (the
/// status bar still names the feature selected).
#[test]
fn the_rename_field_shows_in_the_row() {
    let (mut doc, feature, _) = with_sketch();
    doc.look(Look::SelectPanel(Panel::Timeline));
    let old = name(&doc, Named::Feature(feature));
    let count = |shown: &[String]| shown.iter().filter(|text| **text == old).count();
    let before = screen_texts(&doc);
    doc.look(Look::StartRename(Named::Feature(feature)));
    let after = screen_texts(&doc);
    // The row is drawn plain and hovered.
    assert_eq!(count(&after) + 2, count(&before), "{before:?} {after:?}");
}

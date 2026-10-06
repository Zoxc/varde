use super::*;
use crate::testing::with_body;
use crate::{Editor, MAX_NAME_LEN};

/// The editor of a document with a sketch, an extrude and its body, and
/// the sketch's, the extrude's and the body's targets.
fn named() -> (Editor, Named, Named, Named) {
    let editor = Editor::new(with_body());
    let document = editor.document();
    let sketch = Named::Feature(document.features()[0].id);
    let extrude = Named::Feature(document.features()[1].id);
    let body = Named::Body(document.bodies()[0].id);
    (editor, sketch, extrude, body)
}

fn rename(editor: &mut Editor, target: Named, wanted: &str) -> Option<String> {
    let rename = editor.document().rename(target, wanted).unwrap();
    editor.apply(rename.command).unwrap();
    rename.taken
}

#[test]
fn a_rename_is_undone_and_redone() {
    let (mut editor, sketch, _, body) = named();
    let old = editor.document().name_of(sketch).unwrap().to_owned();
    assert_eq!(rename(&mut editor, sketch, "  Base  "), None);
    assert_eq!(editor.document().name_of(sketch), Some("Base"));
    assert_eq!(rename(&mut editor, body, "Plate"), None);
    assert_eq!(editor.document().name_of(body), Some("Plate"));
    editor.undo();
    editor.undo();
    assert_eq!(editor.document().name_of(sketch), Some(old.as_str()));
    editor.redo();
    assert_eq!(editor.document().name_of(sketch), Some("Base"));
}

#[test]
fn a_name_taken_gets_the_first_free_number() {
    let (mut editor, sketch, extrude, body) = named();
    let extrude_name = editor.document().name_of(extrude).unwrap().to_owned();
    // Across features and bodies.
    assert_eq!(
        rename(&mut editor, body, &extrude_name),
        Some(extrude_name.clone())
    );
    assert_eq!(
        editor.document().name_of(body),
        Some(format!("{extrude_name} (1)").as_str())
    );
    // A number ending the name asked for is taken off first.
    let wanted = format!("{extrude_name} (1)");
    assert_eq!(rename(&mut editor, sketch, &wanted), Some(wanted));
    assert_eq!(
        editor.document().name_of(sketch),
        Some(format!("{extrude_name} (2)").as_str())
    );
}

#[test]
fn nothing_is_made_of_an_empty_name_its_own_or_a_missing_target() {
    let (editor, sketch, _, _) = named();
    let document = editor.document();
    assert_eq!(document.rename(sketch, "   "), None);
    let own = document.name_of(sketch).unwrap();
    assert_eq!(document.rename(sketch, own), None);
    assert_eq!(document.rename(Named::Body(crate::BodyId::NEW), "A"), None);
}

#[test]
fn a_long_name_is_cut_to_fit_numbered_too() {
    let (mut editor, sketch, extrude, _) = named();
    let long = "é".repeat(MAX_NAME_LEN);
    assert_eq!(rename(&mut editor, sketch, &long), None);
    let cut = editor.document().name_of(sketch).unwrap().to_owned();
    assert!(cut.len() <= MAX_NAME_LEN);
    assert!(rename(&mut editor, extrude, &long).is_some());
    let numbered = editor.document().name_of(extrude).unwrap();
    assert!(numbered.len() <= MAX_NAME_LEN && numbered.ends_with(" (1)"));
}

#[test]
fn numbers_are_stripped_only_when_they_are_numbers() {
    assert_eq!(strip_number("A (12)"), "A");
    assert_eq!(strip_number("A (x)"), "A (x)");
    assert_eq!(strip_number("A ()"), "A ()");
    assert_eq!(strip_number("A(1)"), "A(1)");
}

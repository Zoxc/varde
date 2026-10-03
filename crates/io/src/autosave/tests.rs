use std::fs::{File, OpenOptions};
use std::sync::Arc;

use super::*;
use crate::tests::{TempDir, with_sketches};

/// A plain file in `dir` holding a design with as many sketches as each of
/// `records` says, auto-saved in turn. The rules of [`Held::end`] are the
/// web's too, which holds an entry through another [`Storage`].
fn held(dir: &TempDir, records: &[usize]) -> Held<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.0.join("entry"))
        .unwrap();
    let mut held = Held::new(file);
    for &sketches in records {
        held.append(None, None, &Arc::new(with_sketches(sketches)))
            .unwrap();
    }
    held
}

/// The number of sketches of the design `held` holds, if any.
fn sketches(held: &mut Held<File>) -> Option<usize> {
    held.read()
        .unwrap()
        .map(|saved| saved.document.features().len())
}

/// The clean close empties it to be deleted, whatever is in it.
#[test]
fn ending_with_close_empties_it() {
    let dir = TempDir::new("held-close");
    let mut entry = held(&dir, &[1, 0]);
    assert!(entry.end(Ending::Close).unwrap());
    assert_eq!(sketches(&mut entry), None);
}

/// Released, it's deleted only if there's nothing in it, which stays.
#[test]
fn ending_with_release_keeps_what_is_in_it() {
    let dir = TempDir::new("held-release");
    let mut entry = held(&dir, &[1, 0]);
    assert!(!entry.end(Ending::Release).unwrap());
    assert_eq!(sketches(&mut entry), Some(0));
    entry.clear().unwrap();
    assert!(entry.end(Ending::Release).unwrap());
}

/// An auto-save an older web build marked as downloaded still reads, as
/// any other, and the clean close empties it like any other: nothing
/// writes the mark now.
#[test]
fn an_auto_save_marked_downloaded_still_reads() {
    let dir = TempDir::new("held-downloaded");
    let mut entry = held(&dir, &[1]);
    entry
        .append_saved(&AutoSaved {
            base: None,
            name: Some("bracket.vrdp".to_owned()),
            document: Arc::new(with_sketches(2)),
            origin: Origin::Downloaded,
        })
        .unwrap();
    let saved = entry.read().unwrap().unwrap();
    assert_eq!(saved.origin, Origin::Downloaded);
    assert_eq!(saved.name.as_deref(), Some("bracket.vrdp"));
    assert_eq!(saved.document.features().len(), 2);
    assert!(entry.end(Ending::Close).unwrap());
    assert_eq!(sketches(&mut entry), None);
}

/// Checking an auto-save says which part of it is wrong.
#[test]
fn checking_an_auto_save_says_what_is_wrong() {
    let unchecked = |name: Option<String>, document: &[u8]| UncheckedAutoSaved {
        base: None,
        name,
        document: postcard::from_bytes(document).unwrap(),
        origin: Origin::Edited,
    };
    let document = with_sketches(1).to_postcard();
    assert!(AutoSaved::check(unchecked(Some("a".into()), &document)).is_ok());
    assert_eq!(
        AutoSaved::check(unchecked(Some("é".repeat(70_000)), &document)).err(),
        Some(AutoSavedError::Name(140_000))
    );
    // The next id, now 0, which the sketch's id 0 isn't below.
    let mut invalid = document;
    *invalid.last_mut().unwrap() = 0;
    assert!(matches!(
        AutoSaved::check(unchecked(None, &invalid)),
        Err(AutoSavedError::Document(CheckError::FeatureNextId(_)))
    ));
}

/// What a payload holds deepest, a spline's handle in an auto-saved
/// sketch, is within the depth decoding allows.
#[test]
fn a_spline_s_handle_is_read_back() {
    use glam::DVec2;
    use varde_document::{Command, Document, Editor, OriginPlane, Plane};
    use varde_sketch::{Curve, Handle, Sketch, Spline};

    let mut sketch = Sketch::default();
    let fit = [(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)]
        .map(|(x, y)| sketch.add_point(DVec2::new(x, y)).unwrap());
    let tip = sketch.add_point(DVec2::new(13.0, 8.0)).unwrap();
    let mut through = Spline::through(fit.to_vec(), false);
    through.handles.push(Handle { at: fit[1], tip });
    sketch.add_curve(Curve::Spline(through), false).unwrap();
    let mut editor = Editor::new(Document::default());
    editor
        .apply(Command::AddSketch {
            name: "Sketch 1".into(),
            plane: Plane::Origin(OriginPlane::XY),
        })
        .unwrap();
    let feature = editor.document().features()[0].id;
    editor
        .apply(Command::SetSketch {
            feature,
            sketch: Box::new(sketch),
        })
        .unwrap();
    let document = Arc::new(editor.document().clone());

    let dir = TempDir::new("held-spline-handle");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.0.join("entry"))
        .unwrap();
    let mut held = Held::new(file);
    held.append(None, None, &document).unwrap();
    assert_eq!(held.read().unwrap().unwrap().document, document);
}

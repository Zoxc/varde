//! The sketch face: the Project link of the face a sketch is on, added
//! with the sketch, named so in its own group, construction until made
//! otherwise, and never removed while the sketch is on its face.

use varde_document::{OutsideRef, sketch_face};
use varde_view::RowMenu;

use super::*;
use crate::doc::Origin;
use crate::tests::Deferred;

/// The sketch face of sketch `id`, with what it comes from.
fn face_of(doc: &Doc, id: FeatureId) -> Option<(varde_sketch::Link, OutsideRef)> {
    let FeatureKind::Sketch {
        plane,
        sketch,
        sources,
    } = &doc.editor.document().feature(id)?.kind
    else {
        return None;
    };
    let link = sketch_face(plane, sketch, sources)?;
    let from = sources.iter().find(|from| from.link == link)?;
    Some((sketch.link(link)?.clone(), from.source))
}

#[test]
fn a_sketch_on_a_face_has_its_sketch_face_named_and_construction() {
    let (doc, id, _) = on_the_top();
    let (link, source) = face_of(&doc, id).expect("a sketch face");
    let Plane::Face(face) = plane(&doc, id) else {
        panic!("on a face");
    };
    assert_eq!(source, OutsideRef::Face(face));
    // Found on the model: the plate's top's outline, construction.
    assert!(!link.curves.is_empty());
    assert!(!link.profiles);
    let sketch = sketch_of(&doc, id);
    for &curve in &link.curves {
        assert!(sketch.curve(curve).unwrap().construction);
    }
    // One undo step with the sketch.
    let mut doc = doc;
    doc.look(Look::FinishSketch);
    doc.update(Edit::Undo);
    assert!(doc.editor.document().feature(id).is_none());
    // Its row, named so, in a group of its own.
    doc.update(Edit::Redo);
    doc.look(Look::EditFeature(id));
    let rows = &doc.sketch.as_ref().unwrap().links;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, "Sketch face");
    assert!(rows[0].sketch_face);
    let shown = all_texts(&doc);
    assert!(shown.iter().any(|t| t == "Sketch face"), "{shown:?}");
    assert!(!shown.iter().any(|t| t == "Projected"), "{shown:?}");
}

#[test]
fn the_sketch_face_cant_be_removed_and_x_turns_it() {
    let (mut doc, id, requests) = on_the_top();
    let lane = SolveLane::connect(&mut doc);
    let mut doc = Answered { doc, lane };
    let (link, _) = face_of(&doc, id).unwrap();
    let revision = doc.editor.revision();

    // Its menu has Construction, ticked, and no Remove.
    doc.look(Look::OpenMenu(RowMenu::Link(link.id)));
    let shown = all_texts(&doc);
    assert!(shown.iter().any(|t| t == "Construction"), "{shown:?}");
    assert!(!shown.iter().any(|t| t == "Remove"), "{shown:?}");
    doc.look(Look::CloseMenu);

    // Removing it is refused, saying why.
    doc.update(Edit::RemoveLink(link.id));
    assert_eq!(doc.editor.revision(), revision);
    assert!(doc.notice.as_deref().unwrap().contains("sketch face"));

    // Deleting its geometry leaves it.
    doc.notice = None;
    doc.look(Look::ClickLink(link.id));
    doc.update(Edit::DeleteSelection);
    answer(&mut doc, &requests);
    assert_eq!(doc.editor.revision(), revision);
    assert_eq!(face_of(&doc, id).unwrap().0, link);
    assert!(doc.notice.is_some());

    // X turns it out of construction, into profiles, and back.
    doc.look(Look::ClickLink(link.id));
    doc.update(Edit::ToggleConstruction);
    answer(&mut doc, &requests);
    let (now, _) = face_of(&doc, id).unwrap();
    assert!(now.profiles);
    let sketch = sketch_of(&doc, id);
    assert!(!sketch.curve(now.curves[0]).unwrap().construction);
    doc.update(Edit::ToggleConstruction);
    answer(&mut doc, &requests);
    assert!(!face_of(&doc, id).unwrap().0.profiles);
}

#[test]
fn opening_a_file_without_its_sketch_face_adds_it_unedited() {
    // What reading a file whose sketch on a face lacks its sketch face
    // gives (`varde_document`'s tests show it added): one making nothing
    // yet, as a sketch just added has.
    let (doc, id, _) = on_the_top();
    let on = plane(&doc, id);
    let mut editor = Editor::new(Document::example());
    editor.apply(editor.document().add_sketch(on)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    let document = Document::from_postcard(&editor.document().to_postcard()).unwrap();
    assert!(face_of_document(&document, id).is_some());

    let requests: Requests = Rc::default();
    let mut doc = Doc::new(
        document,
        Origin::new(
            crate::doc::Target::None,
            varde_io::Access::Edit,
            "part".into(),
        ),
    );
    doc.feed.connect(Deferred(Rc::clone(&requests)));
    doc.sync();
    answer(&mut doc, &requests);
    // Found on the model, and the document is as opened: not edited,
    // nothing to undo, nothing to auto-save.
    assert!(!face_of(&doc, id).unwrap().0.curves.is_empty());
    assert!(!doc.edited());
    assert!(!doc.editor.can_undo());
}

fn face_of_document(document: &Document, id: FeatureId) -> Option<varde_sketch::Id> {
    let FeatureKind::Sketch {
        plane,
        sketch,
        sources,
    } = &document.feature(id)?.kind
    else {
        return None;
    };
    sketch_face(plane, sketch, sources)
}

#[test]
fn opening_a_file_with_a_stale_link_besides_its_missing_sketch_face_is_edited() {
    // A sketch on the plate's top lacking its sketch face (added as read,
    // empty), with a Project link of the plate's bottom line filled as
    // it was, and the plate widened since: the first answer fills the
    // sketch face and moves the link, leaving the document edited.
    let (doc, id, _) = on_the_top();
    let on = plane(&doc, id);
    let mut editor = Editor::new(Document::example());
    let plate = editor.document().features()[0].id;
    editor.apply(editor.document().add_sketch(on)).unwrap();
    let id = editor.document().features().last().unwrap().id;
    let design = editor.document().design();
    let line = sketch_of_editor(&editor, plate).curves[0].id;
    let mut linked = varde_sketch::SketchEdit::AddLink {
        kind: varde_sketch::LinkKind::Project,
    }
    .apply(sketch_of_editor(&editor, id), &design)
    .unwrap();
    let link = linked.links.last().unwrap().id;
    let mut shape = varde_sketch::LinkShape::default();
    let start = shape.point(DVec2::new(-30.0, -20.0));
    let end = shape.point(DVec2::new(30.0, -20.0));
    shape.curve(Curve::Line { start, end });
    linked = varde_sketch::SketchEdit::Relink(vec![(link, shape)])
        .apply(&linked, &design)
        .unwrap();
    editor
        .apply(Command::AddLink {
            feature: id,
            sketch: Box::new(linked),
            source: varde_document::LinkSource {
                link,
                source: OutsideRef::Sketch {
                    sketch: plate,
                    item: line,
                },
            },
        })
        .unwrap();
    // The plate widened to the left, its bottom line with it.
    let mut wider = sketch_of_editor(&editor, plate).clone();
    let corners: Vec<_> = wider.points.iter().take(4).map(|point| point.id).collect();
    for corner in [corners[0], corners[3]] {
        wider.point_mut(corner).unwrap().at.x -= 5.0;
    }
    editor
        .apply(Command::SetSketch {
            feature: plate,
            sketch: Box::new(wider),
        })
        .unwrap();
    let document = Document::from_postcard(&editor.document().to_postcard()).unwrap();
    assert!(face_of_document(&document, id).is_some());

    let requests: Requests = Rc::default();
    let mut doc = Doc::new(
        document,
        Origin::new(
            crate::doc::Target::None,
            varde_io::Access::Edit,
            "part".into(),
        ),
    );
    doc.feed.connect(Deferred(Rc::clone(&requests)));
    doc.sync();
    answer(&mut doc, &requests);
    assert!(!face_of(&doc, id).unwrap().0.curves.is_empty());
    let moved = |doc: &Doc| {
        let sketch = sketch_of(doc, id);
        let held = sketch.link(link).unwrap().clone();
        let [curve] = held.curves[..] else {
            panic!("{held:?}");
        };
        let Curve::Line { start, end } = sketch.curve(curve).unwrap().curve else {
            panic!("a line");
        };
        let xs = [start, end].map(|p| sketch.point(p).unwrap().at.x);
        xs.into_iter().fold(f64::MAX, f64::min)
    };
    assert_eq!(moved(&doc), -35.0);
    // Edited, so saved or auto-saved, but with no step to undo: relinking
    // is folded into the change it follows from (`Editor::amend`), and
    // a document as opened has none.
    assert!(doc.edited());
    assert!(!doc.editor.can_undo());
}

fn sketch_of_editor(editor: &Editor, id: FeatureId) -> &varde_sketch::Sketch {
    let FeatureKind::Sketch { sketch, .. } = &editor.document().feature(id).unwrap().kind else {
        panic!("a sketch");
    };
    sketch
}

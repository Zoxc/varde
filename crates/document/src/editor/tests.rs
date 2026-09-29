use super::*;
use crate::testing::{add_body, with_body};
use crate::{CheckError, Design, FeatureId, FeatureKind, Plane, Sketch};

#[test]
fn undo_redo_roundtrip() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;

    editor.apply(Command::RemoveBody(id)).unwrap();
    assert!(editor.document().bodies.is_empty());

    editor.undo();
    assert_eq!(editor.document().bodies.len(), 1);

    editor.redo();
    assert!(editor.document().bodies.is_empty());
    assert!(!editor.can_redo());
}

#[test]
fn undo_and_redo_give_a_state_back_its_revision() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;
    let loaded = editor.revision();
    let generation = editor.generation();

    editor.apply(Command::SetVisible(id, false)).unwrap();
    let hidden = editor.revision();
    assert_ne!(hidden, loaded);
    editor.undo();
    assert_eq!(editor.revision(), loaded);
    editor.redo();
    assert_eq!(editor.revision(), hidden);

    // A new edit is a new state, even one undone to make room for it.
    editor.undo();
    editor.apply(Command::RemoveBody(id)).unwrap();
    assert_ne!(editor.revision(), loaded);
    assert_ne!(editor.revision(), hidden);

    // The generation grows through all of it.
    assert_eq!(
        editor.generation(),
        Generation::from(u64::from(generation) + 5)
    );
}

#[test]
fn snapshot_is_shared_and_unaffected_by_edits() {
    let mut editor = Editor::new(with_body());
    let snapshot = editor.snapshot();
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));

    let id = editor.document().bodies[0].id;
    editor.apply(Command::RemoveBody(id)).unwrap();
    assert_eq!(snapshot.bodies.len(), 1);
    assert!(editor.document().bodies.is_empty());
}

#[test]
fn edits_that_change_nothing_are_dropped() {
    let mut editor = Editor::new(with_body());
    let snapshot = editor.snapshot();
    let id = editor.document().bodies[0].id;
    let missing = BodyId(id.0 + 1);

    editor.apply(Command::RemoveBody(missing)).unwrap();
    editor.apply(Command::SetVisible(missing, false)).unwrap();
    editor.apply(Command::SetVisible(id, true)).unwrap();
    editor
        .apply(Command::Replace(Box::new(with_body())))
        .unwrap();

    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));
    assert_eq!(editor.revision(), Revision(0));
    assert!(!editor.can_undo());
}

#[test]
fn check_refuses_ids_a_new_body_could_reuse() {
    let mut document = with_body();
    assert_eq!(document.check(), Ok(()));
    document.next_id = 0;
    let first = document.bodies[0].id;
    let feature = document.features[0].id;
    // Bodies and features take ids from one counter.
    assert_eq!(first.0, feature.0 + 1);
    assert_eq!(document.check(), Err(CheckError::NextId(first)));
    assert!(Document::from_postcard(&document.to_postcard()).is_err());

    let mut document = with_body();
    let second = add_body(&mut document, "Twin", feature);
    assert_eq!(document.check(), Ok(()));
    assert_eq!(document.body(second).map(|b| b.name.as_str()), Some("Twin"));

    // Bodies out of id order would break lookups by id.
    let mut swapped = document.clone();
    swapped.bodies.swap(0, 1);
    let first = document.bodies[0].id;
    assert_eq!(swapped.check(), Err(CheckError::Order(first, second)));
    assert!(Document::from_postcard(&swapped.to_postcard()).is_err());

    document.bodies[1].id = document.bodies[0].id;
    assert!(document.check().is_err());
}

#[test]
fn check_refuses_a_body_no_feature_makes() {
    let mut document = with_body();
    let body = document.bodies[0].id;
    let missing = FeatureId(document.next_id);
    document.bodies[0].created_by = missing;
    assert_eq!(document.check(), Err(CheckError::Creator(body, missing)));
    assert!(Document::from_postcard(&document.to_postcard()).is_err());
    // A body's id isn't a feature's.
    document.bodies[0].created_by = FeatureId(body.0);
    assert_eq!(
        document.check(),
        Err(CheckError::Creator(body, FeatureId(body.0)))
    );
}

#[test]
fn removing_a_feature_removes_the_bodies_it_makes() {
    let mut document = with_body();
    let first = document.features[0].id;
    let second = document
        .add_feature(
            "Sketch 2",
            FeatureKind::Sketch {
                plane: XY,
                sketch: Sketch::default(),
            },
        )
        .unwrap();
    let kept = add_body(&mut document, "Body 2", second);
    let mut editor = Editor::new(document.clone());
    editor.apply(Command::RemoveFeature(first)).unwrap();
    let bodies: Vec<_> = editor.document().bodies().iter().map(|b| b.id).collect();
    assert_eq!(bodies, [kept]);
    // One step to undo.
    editor.undo();
    assert_eq!(*editor.document(), document);
}

#[test]
fn check_refuses_long_names() {
    let mut document = with_body();
    document.bodies[0].name = "é".repeat(crate::MAX_NAME_LEN / 2);
    assert_eq!(document.check(), Ok(()));
    // Long enough to overflow text shaping, yet a small file.
    document.bodies[0].name = "é".repeat(70_000);
    assert!(matches!(
        document.check(),
        Err(CheckError::NameLength(_, 140_000))
    ));
    assert!(matches!(
        Document::from_postcard(&document.to_postcard()),
        Err(crate::DecodeError { .. })
    ));
}

#[test]
fn replace_is_one_undoable_edit() {
    let mut editor = Editor::new(with_body());
    editor.apply(Command::Replace(Box::default())).unwrap();
    assert_eq!(*editor.document(), Document::default());
    assert_eq!(editor.revision(), Revision(1));
    editor.undo();
    assert_eq!(*editor.document(), with_body());

    // Replacing it with what it is changes nothing.
    editor
        .apply(Command::Replace(Box::new(with_body())))
        .unwrap();
    assert_eq!(editor.revision(), Revision(0));
    assert!(editor.can_redo());
}

#[test]
fn the_lineage_changes_only_across_a_replacement() {
    let mut editor = Editor::new(with_body());
    let first = editor.lineage();
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    editor.undo();
    editor.redo();
    assert_eq!(editor.lineage(), first);

    editor.apply(Command::Replace(Box::default())).unwrap();
    let replaced = editor.lineage();
    assert_ne!(replaced, first);
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    assert_eq!(editor.lineage(), replaced);
    editor.undo();
    assert_eq!(editor.lineage(), replaced);
    editor.undo();
    assert_eq!(editor.lineage(), first);
    editor.redo();
    assert_eq!(editor.lineage(), replaced);
}

#[test]
fn edits_that_fail_check_are_refused() {
    let mut editor = Editor::new(with_body());
    let add = Command::AddSketch {
        name: "é".repeat(crate::MAX_NAME_LEN),
        plane: XY,
    };
    assert!(matches!(
        editor.apply(add),
        Err(EditError::Invalid(CheckError::FeatureNameLength(..)))
    ));

    let mut replacement = with_body();
    replacement.next_id = 0;
    assert!(matches!(
        editor.apply(Command::Replace(Box::new(replacement))),
        Err(EditError::Invalid(_))
    ));

    assert_eq!(*editor.document(), with_body());
    assert_eq!(editor.revision(), Revision(0));
    assert!(!editor.can_undo());
}

#[test]
fn undo_forgets_the_oldest_edits_past_the_cap() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;
    for i in 0..MAX_UNDO + 1 {
        editor.apply(Command::SetVisible(id, i % 2 == 1)).unwrap();
    }

    let mut undone = 0;
    while editor.can_undo() {
        editor.undo();
        undone += 1;
    }
    assert_eq!(undone, MAX_UNDO);
    // The first edit is forgotten, so undo stops after it.
    assert!(!editor.document().bodies[0].visible);

    while editor.can_redo() {
        editor.redo();
    }
    assert_eq!(editor.undo.len(), MAX_UNDO);
}

const XY: Plane = Plane::Origin(crate::OriginPlane::XY);

/// A sketch holding a line from the origin to `end`.
fn line_to(end: glam::DVec2) -> Sketch {
    let mut sketch = Sketch::default();
    let start = sketch.add_point(glam::DVec2::ZERO).unwrap();
    let end = sketch.add_point(end).unwrap();
    sketch
        .add_curve(varde_sketch::Curve::Line { start, end }, false)
        .unwrap();
    sketch
}

/// An editor on a new document with "Sketch 1" on XY added, and its id.
fn sketched() -> (Editor, FeatureId) {
    let mut editor = Editor::new(Document::default());
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    let id = editor.document().features()[0].id;
    (editor, id)
}

fn sketch_of(editor: &Editor, id: FeatureId) -> &Sketch {
    match &editor
        .document()
        .feature(id)
        .expect("the feature exists")
        .kind
    {
        FeatureKind::Sketch { sketch, .. } => sketch,
    }
}

#[test]
fn a_sketch_is_added_visible_and_empty_and_removed() {
    let (mut editor, id) = sketched();
    let feature = &editor.document().features()[0];
    assert_eq!(feature.name, "Sketch 1");
    assert!(feature.visible);
    assert_eq!(
        feature.kind,
        FeatureKind::Sketch {
            plane: XY,
            sketch: Sketch::default()
        }
    );
    assert_eq!(editor.document().next_id, id.0 + 1);
    assert_eq!(editor.revision(), Revision(1));

    editor.apply(Command::RemoveFeature(id)).unwrap();
    assert!(editor.document().features().is_empty());
    assert!(editor.document().feature(id).is_none());
    editor.undo();
    assert_eq!(
        editor.document().feature(id).map(|f| f.name.as_str()),
        Some("Sketch 1")
    );

    // Removing what isn't there changes nothing.
    let revision = editor.revision();
    editor
        .apply(Command::RemoveFeature(FeatureId(id.0 + 1)))
        .unwrap();
    assert_eq!(editor.revision(), revision);
}

#[test]
fn add_sketch_numbers_past_the_sketches() {
    let (mut editor, _) = sketched();
    assert_eq!(
        editor.document().add_sketch(XY),
        Command::AddSketch {
            name: "Sketch 2".to_owned(),
            plane: XY,
        }
    );
    for name in ["Sketch 7", "Sketch", "Sketchy 9", "Sketch x", "Cube 20"] {
        let plane = Plane::Origin(crate::OriginPlane::YZ);
        editor
            .apply(Command::AddSketch {
                name: name.to_owned(),
                plane,
            })
            .unwrap();
    }
    let Command::AddSketch { name, plane } = editor.document().add_sketch(XY) else {
        panic!("add_sketch adds a sketch");
    };
    assert_eq!((name.as_str(), plane), ("Sketch 8", XY));
    // Features keep the order they were added in, which is by id.
    let ids: Vec<_> = editor.document().features().iter().map(|f| f.id).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn set_sketch_replaces_a_sketch_as_one_edit() {
    let (mut editor, id) = sketched();
    let line = line_to(glam::DVec2::new(10.0, 5.0));
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(line.clone()),
        })
        .unwrap();
    assert_eq!(*sketch_of(&editor, id), line);
    assert_eq!(editor.revision(), Revision(2));
    editor.undo();
    assert_eq!(*sketch_of(&editor, id), Sketch::default());
    editor.redo();

    // The same sketch again, or one for a feature that isn't there,
    // changes nothing.
    let snapshot = editor.snapshot();
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(line.clone()),
        })
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature: FeatureId(id.0 + 1),
            sketch: Box::new(line),
        })
        .unwrap();
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));
}

#[test]
fn set_sketch_refuses_a_sketch_failing_its_check() {
    let (mut editor, id) = sketched();
    let snapshot = editor.snapshot();
    let far = f64::from(crate::MAX_COORD) * 2.0;
    for end in [glam::DVec2::new(far, 0.0), glam::DVec2::new(0.0, f64::NAN)] {
        assert!(matches!(
            editor.apply(Command::SetSketch {
                feature: id,
                sketch: Box::new(line_to(end)),
            }),
            Err(EditError::Invalid(CheckError::Sketch(
                feature,
                crate::SketchError::Coordinate { .. }
            ))) if feature == id
        ));
    }
    // A line whose end isn't a point.
    let mut dangling = line_to(glam::DVec2::ONE);
    dangling.points.pop();
    assert!(matches!(
        editor.apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(dangling),
        }),
        Err(EditError::Invalid(CheckError::Sketch(
            _,
            crate::SketchError::Reference { .. }
        )))
    ));
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));
    assert_eq!(editor.revision(), Revision(1));
}

#[test]
fn a_sketch_feature_is_shown_and_hidden() {
    let (mut editor, id) = sketched();
    editor.apply(Command::SetFeatureVisible(id, false)).unwrap();
    assert!(!editor.document().features()[0].visible);
    assert_eq!(editor.revision(), Revision(2));

    // Hiding it again, or a feature that isn't there, changes nothing.
    let snapshot = editor.snapshot();
    editor.apply(Command::SetFeatureVisible(id, false)).unwrap();
    editor
        .apply(Command::SetFeatureVisible(FeatureId(id.0 + 1), true))
        .unwrap();
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));

    editor.undo();
    assert!(editor.document().features()[0].visible);
}

#[test]
fn adding_a_sketch_past_the_last_id_fails() {
    let mut editor = Editor::new(Document {
        next_id: u64::MAX,
        ..Document::default()
    });
    assert_eq!(
        editor.apply(editor.document().add_sketch(XY)),
        Err(EditError::OutOfIds)
    );
    assert!(editor.document().features().is_empty());
    assert!(!editor.can_undo());
    assert_eq!(
        EditError::OutOfIds.to_string(),
        "the document has no ids left"
    );
}

#[test]
fn check_refuses_features_a_file_could_get_wrong() {
    let refused = |document: &Document, error: CheckError| {
        assert_eq!(document.check(), Err(error));
        assert!(matches!(
            Document::from_postcard(&document.to_postcard()),
            Err(crate::DecodeError { .. })
        ));
    };
    let (mut editor, first) = sketched();
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    let document = editor.document().clone();
    let second = document.features[1].id;
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document.clone())
    );

    let mut swapped = document.clone();
    swapped.features.swap(0, 1);
    refused(&swapped, CheckError::FeatureOrder(first, second));
    let mut twins = document.clone();
    twins.features[1].id = first;
    refused(&twins, CheckError::FeatureOrder(first, first));

    let mut reused = document.clone();
    reused.next_id = second.0;
    refused(&reused, CheckError::FeatureNextId(second));

    let mut long = document.clone();
    long.features[0].name = "é".repeat(crate::MAX_NAME_LEN);
    refused(
        &long,
        CheckError::FeatureNameLength(first, 2 * crate::MAX_NAME_LEN),
    );

    // A sketch is checked against the coordinate limit, and named by its
    // feature.
    let mut far = document;
    let max = f64::from(crate::MAX_COORD);
    far.features[1].kind = FeatureKind::Sketch {
        plane: XY,
        sketch: line_to(glam::DVec2::new(max, -max)),
    };
    assert_eq!(far.check(), Ok(()));
    let sketch = line_to(glam::DVec2::new(max.next_up(), 0.0));
    far.features[1].kind = FeatureKind::Sketch { plane: XY, sketch };
    assert!(matches!(
        far.check(),
        Err(CheckError::Sketch(id, crate::SketchError::Coordinate { .. })) if id == second
    ));
    assert!(Document::from_postcard(&far.to_postcard()).is_err());
    assert!(
        far.check()
            .unwrap_err()
            .to_string()
            .starts_with(&format!("feature {}: sketch point", second.0))
    );
}

/// `line_to((10, 0))` with a driving dimension of the line's length, typed
/// as `text`, in `units`.
fn dimensioned(text: &str, units: LengthUnit) -> Sketch {
    use varde_sketch::{Dimension, Measure, Side};
    let mut sketch = line_to(glam::DVec2::new(10.0, 0.0));
    let line = sketch.curves[0].id;
    let design = Design {
        max: f64::from(crate::MAX_COORD),
        units,
    };
    let measure = Measure::Length(line);
    let value = varde_expr::Value::new(text, &measure.ask(&design)).unwrap();
    sketch
        .add_dimension(Dimension {
            measure,
            value,
            driving: true,
            label: glam::DVec2::ZERO,
            side: Side::Positive,
        })
        .unwrap();
    sketch
}

fn dimension_text(editor: &Editor, id: FeatureId) -> &str {
    &sketch_of(editor, id).dimensions[0].dimension.value.text
}

#[test]
fn a_sketch_s_values_are_checked_in_the_design_s_units() {
    let (mut editor, id) = sketched();
    assert_eq!(editor.document().units(), LengthUnit::Mm);
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(dimensioned("4 + 6", LengthUnit::Mm)),
        })
        .unwrap();
    // "4 + 6" read in inches isn't the millimetres stored.
    let snapshot = editor.snapshot();
    assert!(matches!(
        editor.apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(dimensioned("4 + 6.5", LengthUnit::In)),
        }),
        Err(EditError::Invalid(CheckError::Sketch(
            feature,
            crate::SketchError::Value(_)
        ))) if feature == id
    ));
    assert!(Arc::ptr_eq(&snapshot, &editor.snapshot()));

    // A file whose values its units don't give isn't opened.
    let mut document = editor.document().clone();
    document.units = LengthUnit::In;
    assert!(matches!(
        Document::from_postcard(&document.to_postcard()),
        Err(error) if error.to_string().contains("doesn't give")
    ));
}

#[test]
fn set_units_pins_bare_numbers_and_changes_no_value() {
    let (mut editor, id) = sketched();
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(dimensioned("4 + 6", LengthUnit::Mm)),
        })
        .unwrap();
    let before = editor.document().clone();
    let revision = editor.revision();

    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    assert_eq!(editor.document().units(), LengthUnit::In);
    assert_eq!(dimension_text(&editor, id), "(4 + 6) mm");
    let value = |editor: &Editor| sketch_of(editor, id).dimensions[0].dimension.value.value;
    assert_eq!(value(&editor), 10.0);
    assert_eq!(
        sketch_of(&editor, id).points,
        sketch_of(&Editor::new(before.clone()), id).points
    );
    // It's still a document a file can hold.
    assert_eq!(
        Document::from_postcard(&editor.document().to_postcard()).as_ref(),
        Ok(editor.document())
    );

    // Back to millimetres, already pinned, nothing more is written.
    editor.apply(Command::SetUnits(LengthUnit::Mm)).unwrap();
    assert_eq!(dimension_text(&editor, id), "(4 + 6) mm");
    // The same units change nothing.
    let now = editor.revision();
    editor.apply(Command::SetUnits(LengthUnit::Mm)).unwrap();
    assert_eq!(editor.revision(), now);

    // One step each to undo.
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    assert_eq!(editor.revision(), revision);
}

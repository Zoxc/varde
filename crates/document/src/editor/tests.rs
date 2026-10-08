use super::*;
use crate::testing::{extrude_again, extrude_of, plate, with_body};
use crate::{
    CheckError, Design, ExtrudeError, FeatureId, FeatureKind, Opacity, Operation, Plane, Removable,
    Removal, Sketch, Targets,
};

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
fn set_opacity_undoes_and_redoes() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;
    assert!(editor.document().bodies[0].opacity.is_opaque());
    let half = Opacity::new(50).unwrap();

    editor.apply(Command::SetOpacity(id, half)).unwrap();
    assert_eq!(editor.document().bodies[0].opacity, half);
    let set = editor.revision();
    editor.undo();
    assert_eq!(editor.document().bodies[0].opacity, Opacity::MAX);
    assert_eq!(editor.revision(), Revision(0));
    editor.redo();
    assert_eq!(editor.document().bodies[0].opacity, half);
    assert_eq!(editor.revision(), set);
}

/// A body added later starts opaque, whatever the others are.
#[test]
fn new_bodies_are_opaque() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;
    editor.apply(Command::SetOpacity(id, Opacity::MIN)).unwrap();
    let (_, body) = extrude_again(&mut editor);
    assert!(editor.document().body(body).unwrap().opacity.is_opaque());
}

/// A body's colour is set and taken back to the theme's as one undo step
/// each; setting it as it is adds none.
#[test]
fn set_color_undoes_and_redoes() {
    let mut editor = Editor::new(with_body());
    let id = editor.document().bodies[0].id;
    assert_eq!(editor.document().bodies[0].color, None);
    let teal = Tint::new(180, 30);
    editor.apply(Command::SetColor(id, teal)).unwrap();
    assert_eq!(editor.document().bodies[0].color, teal);
    let set = editor.revision();
    editor.apply(Command::SetColor(id, teal)).unwrap();
    assert_eq!(editor.revision(), set);
    editor.apply(Command::SetColor(id, None)).unwrap();
    assert_eq!(editor.document().bodies[0].color, None);
    editor.undo();
    assert_eq!(editor.document().bodies[0].color, teal);
    assert_eq!(editor.revision(), set);
    editor.undo();
    assert_eq!(editor.document().bodies[0].color, None);
    editor.redo();
    assert_eq!(editor.document().bodies[0].color, teal);
}

#[test]
fn check_refuses_a_color_out_of_range() {
    let mut document = with_body();
    let id = document.bodies[0].id;
    for (hue, saturation) in [(360, 0), (0, Tint::MAX_SATURATION + 1), (u16::MAX, u8::MAX)] {
        // Deserializing doesn't check the range: the document does.
        let bytes = postcard::to_allocvec(&(hue, saturation)).unwrap();
        document.bodies[0].color = Some(postcard::from_bytes(&bytes).unwrap());
        assert_eq!(document.check(), Err(CheckError::Tint(id, hue, saturation)));
    }
    document.bodies[0].color = Tint::new(359, Tint::MAX_SATURATION);
    assert_eq!(document.check(), Ok(()));
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
        .apply(Command::SetOpacity(missing, Opacity::MIN))
        .unwrap();
    editor
        .apply(Command::SetOpacity(id, Opacity::default()))
        .unwrap();
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
    let extrude = document.features[1].id;
    // Bodies and features take ids from one counter: the extrude's, then
    // its body's.
    assert_eq!(first.0, extrude.0 + 1);
    assert_eq!(document.check(), Err(CheckError::NextId(first)));
    assert!(Document::from_postcard(&document.to_postcard()).is_err());

    let mut editor = Editor::new(with_body());
    let (_, second) = extrude_again(&mut editor);
    let mut document = editor.document().clone();
    assert_eq!(document.check(), Ok(()));
    assert_eq!(
        document.body(second).map(|b| b.name.as_str()),
        Some("Body 2")
    );

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
fn check_refuses_a_body_no_extrude_makes() {
    let mut document = with_body();
    let body = document.bodies[0].id;
    let [sketch, extrude] = [0, 1].map(|index| document.features[index].id);
    for maker in [FeatureId(document.next_id), FeatureId(body.0), sketch] {
        document.bodies[0].created_by = maker;
        assert_eq!(document.check(), Err(CheckError::Creator(body, maker)));
        assert!(Document::from_postcard(&document.to_postcard()).is_err());
    }
    // An extrude making another body doesn't make this one.
    let mut editor = Editor::new(with_body());
    let (_, second) = extrude_again(&mut editor);
    let mut twins = editor.document().clone();
    twins.bodies[1].created_by = extrude;
    assert_eq!(twins.check(), Err(CheckError::Creator(second, extrude)));

    // Nor may an extrude name a body that isn't its.
    let mut orphan = with_body();
    orphan.bodies.clear();
    assert_eq!(
        orphan.check(),
        Err(CheckError::Extrude(extrude, ExtrudeError::NewBody(body)))
    );
}

#[test]
fn removing_a_sketch_removes_the_extrudes_using_it_and_their_bodies() {
    let mut editor = Editor::new(with_body());
    let (second, body) = extrude_again(&mut editor);
    // A third, cutting, but not from the first body.
    let first_body = editor.document().bodies[0].id;
    let cut = plate(Operation::Cut(Targets {
        excluded: vec![first_body, body],
        held: None,
    }));
    editor
        .apply(editor.document().add_feature(cut.into()))
        .unwrap();
    let document = editor.document().clone();
    let [sketch, first, _, third] = [0, 1, 2, 3].map(|index| document.features[index].id);
    // A sketch the extrudes don't use.
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    let other = editor.document().features.last().unwrap().id;
    let document = editor.document().clone();

    let removal = document.removal(Removable::Feature(sketch));
    assert_eq!(
        removal,
        Removal {
            features: vec![sketch, first, second, third],
            bodies: vec![first_body, body],
        }
    );
    // A body takes its extrude with it, and nothing else here; the cut
    // drops it from those it excludes.
    let removal = document.removal(Removable::Body(body));
    assert_eq!(
        removal,
        Removal {
            features: vec![second],
            bodies: vec![body],
        }
    );
    assert_eq!(document.removal(Removable::Feature(second)), removal);
    assert!(
        document
            .removal(Removable::Feature(other))
            .bodies
            .is_empty()
    );
    assert!(
        document
            .removal(Removable::Feature(FeatureId(document.next_id)))
            .is_empty()
    );
    assert!(
        document
            .removal(Removable::Body(BodyId(document.next_id)))
            .is_empty()
    );

    // The commands remove what `removal` lists, as one step to undo.
    editor.apply(Command::RemoveBody(body)).unwrap();
    let after = editor.document();
    assert_eq!(after.features.len(), 4);
    assert!(after.feature(second).is_none());
    assert_eq!(after.bodies.len(), 1);
    assert_eq!(extrude_of(after, third).operation.excluded(), [first_body]);
    editor.undo();
    assert_eq!(*editor.document(), document);

    editor.apply(Command::RemoveFeature(sketch)).unwrap();
    let ids: Vec<_> = editor.document().features.iter().map(|f| f.id).collect();
    assert_eq!(ids, [other]);
    assert!(editor.document().bodies.is_empty());
    editor.undo();
    assert_eq!(*editor.document(), document);
    assert_eq!(editor.revision(), Revision(3));
}

#[test]
fn removing_only_a_sketch_keeps_the_extrudes_using_it() {
    let mut editor = Editor::new(with_body());
    let (second, body) = extrude_again(&mut editor);
    let document = editor.document().clone();
    let [sketch, first, _] = [0, 1, 2].map(|index| document.features[index].id);
    let first_body = document.bodies[0].id;

    // The sketch alone; a body takes its maker and that's bodies.
    let removal = document.breaking_removal(&[Removable::Feature(sketch)]);
    assert_eq!(
        removal,
        Removal {
            features: vec![sketch],
            bodies: vec![],
        }
    );
    let removal = document.breaking_removal(&[Removable::Body(first_body)]);
    assert_eq!(
        removal,
        Removal {
            features: vec![first],
            bodies: vec![first_body],
        }
    );
    assert!(
        document
            .breaking_removal(&[Removable::Feature(FeatureId(document.next_id))])
            .is_empty()
    );

    // The extrudes stay, naming a sketch that isn't there, which the
    // check takes; one undo step.
    editor.apply(Command::RemoveOnly(vec![sketch])).unwrap();
    let after = editor.document();
    let ids: Vec<_> = after.features.iter().map(|f| f.id).collect();
    assert_eq!(ids, [first, second]);
    assert_eq!(extrude_of(after, second).sketch, sketch);
    assert_eq!(after.bodies.len(), 2);
    assert_eq!(after.check(), Ok(()));
    assert!(Document::from_postcard(&after.to_postcard()).is_ok());
    editor.undo();
    assert_eq!(*editor.document(), document);
    assert_eq!(
        editor.document().removal(Removable::Body(body)).features,
        [second]
    );
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
        _ => panic!("feature {} isn't a sketch", id.0),
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
            sketch: Sketch::default(),
            sources: Vec::new(),
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
    // Not rolled back, features keep the order they were added in.
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

    // The last id there is is taken, and then no more.
    let mut editor = Editor::new(Document {
        next_id: u64::MAX - 1,
        ..Document::default()
    });
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    assert_eq!(editor.document().features()[0].id, FeatureId(u64::MAX - 1));
    assert_eq!(
        editor.apply(editor.document().add_sketch(XY)),
        Err(EditError::OutOfIds)
    );
    assert_eq!(editor.document().features().len(), 1);
    assert_eq!(editor.revision(), Revision(1));
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

    // The Timeline's order is the list's, not the ids'.
    let mut swapped = document.clone();
    swapped.features.swap(0, 1);
    swapped.reindex();
    swapped.check().unwrap();
    let mut twins = document.clone();
    twins.features[1].id = first;
    twins.reindex();
    refused(&twins, CheckError::FeatureTwice(first));

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
        sources: Vec::new(),
    };
    assert_eq!(far.check(), Ok(()));
    let sketch = line_to(glam::DVec2::new(max.next_up(), 0.0));
    far.features[1].kind = FeatureKind::Sketch {
        plane: XY,
        sketch,
        sources: Vec::new(),
    };
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
        params: varde_expr::Params::EMPTY,
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

/// The sources of `id`'s links.
fn sources_of(editor: &Editor, id: FeatureId) -> &[crate::LinkSource] {
    match &editor.document().feature(id).unwrap().kind {
        FeatureKind::Sketch { sources, .. } => sources,
        _ => panic!("not a sketch"),
    }
}

#[test]
fn a_link_is_added_with_its_source_and_goes_with_its_link() {
    use crate::{LinkSource, OutsideRef};
    use varde_sketch::{LinkKind, SketchEdit};
    let (mut editor, first) = sketched();
    editor
        .apply(Command::SetSketch {
            feature: first,
            sketch: Box::new(line_to(glam::DVec2::new(5.0, 0.0))),
        })
        .unwrap();
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    let second = editor.document().features()[1].id;
    // A copy, as the edits below change the document.
    let held = editor.document().clone();
    let design = held.design();
    let linked = SketchEdit::AddLink {
        kind: LinkKind::Project,
    }
    .apply(sketch_of(&editor, second), &design)
    .unwrap();
    let link = linked.links[0].id;
    let line = sketch_of(&editor, first).curves[0].id;
    let source = LinkSource {
        link,
        source: OutsideRef::Sketch {
            sketch: first,
            item: line,
        },
    };
    // A link with no source is refused.
    assert!(matches!(
        editor.apply(Command::SetSketch {
            feature: second,
            sketch: Box::new(linked.clone()),
        }),
        Err(EditError::Invalid(CheckError::SketchLink(_, _)))
    ));
    editor
        .apply(Command::AddLink {
            feature: second,
            sketch: Box::new(linked.clone()),
            source,
        })
        .unwrap();
    assert_eq!(sources_of(&editor, second), [source]);

    // Undone and redone as one change.
    editor.undo();
    assert!(sources_of(&editor, second).is_empty());
    editor.redo();
    assert_eq!(sources_of(&editor, second), [source]);

    // Deleting the link drops its source.
    let unlinked = SketchEdit::Delete(vec![link])
        .apply(&linked, &design)
        .unwrap();
    editor
        .apply(Command::SetSketch {
            feature: second,
            sketch: Box::new(unlinked),
        })
        .unwrap();
    assert!(sources_of(&editor, second).is_empty());

    // A link may only come from what's before its sketch: the first
    // sketch can't project the second's.
    let design = editor.document().design();
    let backwards = SketchEdit::AddLink {
        kind: LinkKind::Project,
    }
    .apply(sketch_of(&editor, first), &design)
    .unwrap();
    let link = backwards.links[0].id;
    assert!(matches!(
        editor.apply(Command::AddLink {
            feature: first,
            sketch: Box::new(backwards.clone()),
            source: LinkSource {
                link,
                source: OutsideRef::Sketch {
                    sketch: second,
                    item: line,
                },
            },
        }),
        Err(EditError::Invalid(CheckError::SketchLink(
            _,
            crate::LinkError::Later(_)
        )))
    ));
    // Nor can Intersect take a sketch's item.
    let mut intersect = backwards;
    intersect.links[0].kind = LinkKind::Intersect;
    assert!(matches!(
        editor.apply(Command::AddLink {
            feature: second,
            sketch: Box::new(intersect),
            source: LinkSource {
                link,
                source: OutsideRef::Sketch {
                    sketch: first,
                    item: line,
                },
            },
        }),
        Err(EditError::Invalid(CheckError::SketchLink(_, _)))
    ));
}

#[test]
fn an_amended_change_is_undone_with_the_one_before() {
    let (mut editor, id) = sketched();
    editor.apply(editor.document().add_sketch(XY)).unwrap();
    let before = editor.document().clone();
    editor
        .apply(Command::SetSketch {
            feature: id,
            sketch: Box::new(line_to(glam::DVec2::new(5.0, 0.0))),
        })
        .unwrap();
    let generation = editor.generation();
    let revision = editor.revision();
    editor
        .amend(Command::SetSketch {
            feature: id,
            sketch: Box::new(line_to(glam::DVec2::new(6.0, 0.0))),
        })
        .unwrap();
    assert!(editor.generation() > generation);
    assert_ne!(editor.revision(), revision);
    assert_eq!(sketch_of(&editor, id), &line_to(glam::DVec2::new(6.0, 0.0)));
    editor.undo();
    assert_eq!(editor.document(), &before);
    editor.redo();
    assert_eq!(sketch_of(&editor, id), &line_to(glam::DVec2::new(6.0, 0.0)));
    // The redo history stays through an amend.
    editor.undo();
    editor
        .amend(Command::SetSketch {
            feature: id,
            sketch: Box::new(line_to(glam::DVec2::new(7.0, 0.0))),
        })
        .unwrap();
    assert!(editor.can_redo());
}

/// The Timeline's rollback: set and undone as an edit, refused for a
/// feature that isn't there, moved on to the next feature kept when its
/// own is removed, and kept by a new feature, which goes in before it.
#[test]
fn the_rollback_is_an_edit_kept_on_a_feature() {
    let mut editor = Editor::new(Document::example());
    let ids: Vec<FeatureId> = editor.document().features().iter().map(|f| f.id).collect();
    assert!(ids.len() >= 2, "{ids:?}");
    editor.apply(Command::SetRollback(Some(ids[1]))).unwrap();
    assert_eq!(editor.document().rollback(), Some(ids[1]));
    let before = editor.document().before(ids[1]);
    assert_eq!(before.features().len(), 1);
    assert!(before.bodies().iter().all(|body| body.created_by == ids[0]));
    assert_eq!(before.rollback(), None);
    before.check().unwrap();

    let missing = FeatureId(editor.document().next_id);
    let revision = editor.revision();
    editor.apply(Command::SetRollback(Some(missing))).unwrap();
    assert_eq!(editor.revision(), revision);
    editor.undo();
    assert_eq!(editor.document().rollback(), None);
    editor.redo();

    // Its feature removed: to before the next one kept, or the end.
    let mut removed = editor.document().clone();
    removed.remove(&removed.removal_of(&[Removable::Feature(ids[1])]));
    let next = (removed.features().iter())
        .map(|f| f.id)
        .find(|&id| ids[2..].contains(&id));
    assert_eq!(removed.rollback(), next);
    removed.check().unwrap();

    // A file naming a feature that isn't there is refused.
    let mut wrong = editor.document().clone();
    wrong.rollback = Some(missing);
    assert!(matches!(wrong.check(), Err(CheckError::Rollback(id)) if id == missing));

    // A new feature goes in where it's rolled back to, the last shown.
    let sketch = (editor.document()).add_sketch(Plane::Origin(crate::OriginPlane::XY));
    editor.apply(sketch).unwrap();
    let document = editor.document();
    assert_eq!(document.rollback(), Some(ids[1]));
    let added = document.features()[1].id;
    assert!(!ids.contains(&added));
    assert_eq!(
        &document.features()[2..],
        &Document::example().features()[1..]
    );
    assert_eq!(
        document.before(ids[1]).features().last().map(|f| f.id),
        Some(added)
    );
    // Rolled to the end, the next goes last.
    editor.apply(Command::SetRollback(None)).unwrap();
    let sketch = (editor.document()).add_sketch(Plane::Origin(crate::OriginPlane::XY));
    editor.apply(sketch).unwrap();
    let last = editor.document().features().last().unwrap().id;
    assert!(last > added);
    // Read back in the same order.
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

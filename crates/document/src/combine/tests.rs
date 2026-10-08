use super::*;
use crate::testing::{extrude_again, plate, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, FeatureId, FeatureKind, LengthUnit,
    Operation, Removable, Removal, Targets,
};

/// The example's body and two more plates, each its own body: the editor,
/// the bodies in the order they were made, and their makers.
fn three_bodies() -> (Editor, [BodyId; 3], [FeatureId; 3]) {
    let example = with_body();
    let first = (example.bodies[0].id, example.features[1].id);
    let mut editor = Editor::new(example);
    let second = extrude_again(&mut editor);
    let third = extrude_again(&mut editor);
    (
        editor,
        [first.0, second.1, third.1],
        [first.1, second.0, third.0],
    )
}

fn combine(target: BodyId, tools: &[BodyId], op: BodyOp) -> Combine {
    Combine {
        target,
        tools: tools.to_vec(),
        op,
        keep_tools: false,
    }
}

/// Adds `combine` to `editor` as one edit: the new feature's id.
fn add(editor: &mut Editor, combine: Combine) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(combine.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

fn combine_of(document: &Document, feature: FeatureId) -> &Combine {
    match &document.feature(feature).unwrap().kind {
        FeatureKind::Combine(combine) => combine,
        _ => panic!("feature {} isn't a combine", feature.0),
    }
}

#[test]
fn adding_a_combine_is_one_step_and_makes_no_body() {
    let (mut editor, [a, b, c], _) = three_bodies();
    let before = editor.document().clone();
    let id = add(&mut editor, combine(a, &[b, c], BodyOp::Union)).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Combine 1");
    assert!(feature.visible);
    assert_eq!(document.bodies, before.bodies, "no body added or hidden");
    // The sketches' visibility is left alone: a combine takes no regions.
    for (old, new) in before.features.iter().zip(&document.features) {
        assert_eq!(old, new);
    }
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.new_body(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.bodies(), vec![a, b, c]);
    assert_eq!(feature.kind.noun(), "Combine");

    let second = add(&mut editor, combine(b, &[c], BodyOp::Subtract)).unwrap();
    assert_eq!(editor.document().feature(second).unwrap().name, "Combine 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(combine_of(editor.document(), id).tools, [b, c]);
}

#[test]
fn setting_a_combine_changes_its_op_tools_and_keep() {
    let (mut editor, [a, b, c], _) = three_bodies();
    let id = add(&mut editor, combine(a, &[b], BodyOp::Union)).unwrap();
    let revision = editor.revision();
    let same = combine(a, &[b], BodyOp::Union);
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(same.into()),
        })
        .unwrap();
    assert_eq!(editor.revision(), revision, "nothing changed");
    let set = Combine {
        keep_tools: true,
        ..combine(c, &[a, b], BodyOp::Intersect)
    };
    editor
        .apply(Command::SetFeature {
            feature: id,
            kind: Box::new(set.clone().into()),
        })
        .unwrap();
    assert_eq!(*combine_of(editor.document(), id), set);
    editor.undo();
    assert_eq!(combine_of(editor.document(), id).op, BodyOp::Union);
}

#[test]
fn tools_are_bounded_sorted_and_not_the_target() {
    let (mut editor, [a, b, c], _) = three_bodies();
    let refused = |editor: &mut Editor, combine: Combine, why: CombineError| {
        let next = editor.document().next_id;
        let error = add(editor, combine).unwrap_err();
        assert_eq!(
            error,
            EditError::Invalid(CheckError::Combine(FeatureId(next), why))
        );
    };
    refused(
        &mut editor,
        combine(a, &[], BodyOp::Union),
        CombineError::Tools(0),
    );
    refused(
        &mut editor,
        combine(a, &[c, b], BodyOp::Union),
        CombineError::ToolOrder,
    );
    refused(
        &mut editor,
        combine(a, &[b, b], BodyOp::Union),
        CombineError::ToolOrder,
    );
    refused(
        &mut editor,
        combine(b, &[a, b], BodyOp::Subtract),
        CombineError::TargetIsTool(b),
    );
    // Over the limit: the count is checked before the bodies.
    let many: Vec<BodyId> = (0..=MAX_FEATURE_BODIES as u64)
        .map(|k| BodyId(1000 + k))
        .collect();
    refused(
        &mut editor,
        combine(a, &many, BodyOp::Union),
        CombineError::Tools(MAX_FEATURE_BODIES + 1),
    );
    assert_eq!(
        combine(a, &many[..MAX_FEATURE_BODIES], BodyOp::Union).check_own(),
        Ok(())
    );
    // A body that isn't there.
    let missing = BodyId(editor.document().next_id + 5);
    refused(
        &mut editor,
        combine(a, &[b, missing], BodyOp::Union),
        CombineError::Body(missing),
    );
    // Nothing was added.
    assert_eq!(editor.document().features.len(), 4);
    // One gone, with an id no later body can take, is taken, as deleting
    // its maker but keeping the combine leaves it.
    let gone = BodyId(1);
    assert!(editor.document().body(gone).is_none(), "a feature's id");
    add(&mut editor, combine(gone, &[b], BodyOp::Union)).unwrap();
}

/// A combine's bodies are made before it: one set in place of an earlier
/// feature can't name a body made after.
#[test]
fn a_combine_names_only_bodies_made_before_it() {
    let (mut editor, [_, b, c], [first, ..]) = three_bodies();
    let before = editor.document().clone();
    let error = editor
        .apply(Command::SetFeature {
            feature: first,
            kind: Box::new(combine(b, &[c], BodyOp::Union).into()),
        })
        .unwrap_err();
    assert_eq!(
        error,
        EditError::Invalid(CheckError::Combine(first, CombineError::Body(b)))
    );
    assert_eq!(*editor.document(), before);
}

/// Setting a tool's or the target's maker to stop making it leaves the
/// combine naming a body that isn't there, which regenerating fails: an
/// edit is never refused for what later features name.
#[test]
fn a_maker_named_by_a_combine_may_stop_making_its_body() {
    let (mut editor, [a, b, c], [_, second, third]) = three_bodies();
    let id = add(&mut editor, combine(a, &[b, c], BodyOp::Union)).unwrap();
    let before = editor.document().clone();
    for maker in [second, third] {
        editor
            .apply(Command::SetFeature {
                feature: maker,
                kind: Box::new(plate(Operation::Join(Targets::default())).into()),
            })
            .unwrap();
        let document = editor.document();
        assert!(document.feature(id).is_some());
        assert_eq!(document.check(), Ok(()));
        editor.undo();
        assert_eq!(*editor.document(), before);
    }
    // Changing it but still making the body is fine.
    let mut thicker = plate(Operation::NewBody(BodyId::NEW));
    thicker.flip = true;
    editor
        .apply(Command::SetFeature {
            feature: third,
            kind: Box::new(thicker.into()),
        })
        .unwrap();
    assert_eq!(editor.document().body(c).unwrap().created_by, third);
}

#[test]
fn removing_a_named_body_or_its_maker_removes_the_combine() {
    let (mut editor, [a, b, c], [first, second, third]) = three_bodies();
    let id = add(&mut editor, combine(a, &[b], BodyOp::Subtract)).unwrap();
    // A later feature on the combine's target (an extrude joining) isn't
    // taken: it doesn't name it.
    let join = plate(Operation::Join(Targets::default()));
    editor
        .apply(editor.document().add_feature(join.into()))
        .unwrap();
    let after = editor.document().features.last().unwrap().id;
    let document = editor.document();
    // The tool's maker.
    assert_eq!(
        document.removal(Removable::Feature(second)),
        Removal {
            features: vec![second, id],
            bodies: vec![b],
        }
    );
    // The tool itself.
    assert_eq!(
        document.removal(Removable::Body(b)),
        document.removal(Removable::Feature(second))
    );
    // The target's maker, with its sketch's removal taking the lot made
    // from it (all three plates use the example's sketch).
    assert_eq!(
        document.removal(Removable::Body(a)),
        Removal {
            features: vec![first, id],
            bodies: vec![a],
        }
    );
    let sketch = document.features[0].id;
    assert_eq!(
        document.removal(Removable::Feature(sketch)),
        Removal {
            features: vec![sketch, first, second, third, id, after],
            bodies: vec![a, b, c],
        }
    );
    // A body not named leaves it.
    assert_eq!(
        document.removal(Removable::Body(c)),
        Removal {
            features: vec![third],
            bodies: vec![c],
        }
    );
    let before = editor.document().clone();
    editor.apply(Command::RemoveBody(b)).unwrap();
    let document = editor.document();
    assert!(document.feature(id).is_none());
    assert!(document.feature(after).is_some());
    assert!(document.body(b).is_none());
    editor.undo();
    assert_eq!(*editor.document(), before);
    // Removing the combine itself takes nothing else.
    assert_eq!(
        editor.document().removal(Removable::Feature(id)),
        Removal {
            features: vec![id],
            bodies: vec![],
        }
    );
    editor.apply(Command::RemoveFeature(id)).unwrap();
    assert_eq!(editor.document().bodies, before.bodies);
}

/// Removing one combine leaves a later one on the same bodies: removal
/// follows what a feature makes, and a combine makes no body.
#[test]
fn removing_one_combine_leaves_another_on_the_same_bodies() {
    let (mut editor, [a, b, c], _) = three_bodies();
    let first = add(&mut editor, combine(a, &[b], BodyOp::Union)).unwrap();
    let second = add(&mut editor, combine(a, &[c], BodyOp::Union)).unwrap();
    assert_eq!(
        editor.document().removal(Removable::Feature(first)),
        Removal {
            features: vec![first],
            bodies: vec![],
        }
    );
    editor.apply(Command::RemoveFeature(first)).unwrap();
    assert!(editor.document().feature(second).is_some());
}

#[test]
fn set_units_leaves_a_combine_alone() {
    let (mut editor, [a, b, _], _) = three_bodies();
    let id = add(&mut editor, combine(a, &[b], BodyOp::Intersect)).unwrap();
    let kind = editor.document().feature(id).unwrap().kind.clone();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    assert_eq!(editor.document().feature(id).unwrap().kind, kind);
}

#[test]
fn combines_round_trip() {
    let (mut editor, [a, b, c], _) = three_bodies();
    for (op, keep_tools) in [
        (BodyOp::Union, false),
        (BodyOp::Subtract, true),
        (BodyOp::Intersect, false),
    ] {
        let combine = Combine {
            keep_tools,
            ..combine(c, &[a, b], op)
        };
        add(&mut editor, combine).unwrap();
    }
    let document = editor.document().clone();
    assert_eq!(document.features.len(), 7);
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// A file whose combine is wrong is refused as it's read.
#[test]
fn a_wrong_combine_is_refused_when_read() {
    let (mut editor, [a, b, _], _) = three_bodies();
    let id = add(&mut editor, combine(a, &[b], BodyOp::Union)).unwrap();
    let mut document = editor.document().clone();
    let index = document.feature_index(id).unwrap();
    if let FeatureKind::Combine(combine) = &mut document.features[index].kind {
        combine.tools = vec![a];
    }
    let bytes = document.to_postcard();
    let error = Document::from_postcard(&bytes).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "feature {}: its target, body {}, is one of its tools too",
            id.0, a.0
        )
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the combine comes after the revolve.
#[test]
fn a_combine_is_the_fourth_kind() {
    let kind = FeatureKind::Combine(combine(BodyId(0), &[BodyId(1)], BodyOp::Subtract));
    assert_eq!(postcard::to_stdvec(&kind).unwrap(), [3, 0, 1, 1, 1, 0]);
}

#[test]
fn combine_errors_say_what_is_wrong() {
    assert_eq!(
        CombineError::Tools(0).to_string(),
        "combines 0 tool bodies, not 1 to 256"
    );
    assert_eq!(
        CombineError::ToolOrder.to_string(),
        "its tool bodies are out of order or repeated"
    );
    assert_eq!(
        CombineError::Body(BodyId(7)).to_string(),
        "combines body 7, which isn't there or no earlier feature makes"
    );
    assert_eq!(BodyOp::Subtract.label(), "Subtract");
}

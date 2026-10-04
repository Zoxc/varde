use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, extrude_of, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, FeatureKind, LengthUnit, OriginPlane,
    Removable, Scale, ScaleFactor,
};

/// The example's body and another plate: the editor, the bodies and
/// their makers.
fn two_bodies() -> (Editor, [BodyId; 2], [FeatureId; 2]) {
    let example = with_body();
    let first = (example.bodies[0].id, example.features[1].id);
    let mut editor = Editor::new(example);
    let (maker, body) = extrude_again(&mut editor);
    (editor, [first.0, body], [first.1, maker])
}

/// The top face of the plate `maker` made, on `body`.
fn top(body: BodyId, maker: FeatureId) -> FaceRef {
    FaceRef {
        body,
        key: FaceKey {
            feature: maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(1.0, 2.0, 10.0),
    }
}

/// `body` split by XY, both sides kept.
fn by_xy(body: BodyId) -> Split {
    Split {
        body,
        tool: SplitTool::Plane(PlaneRef::Origin(OriginPlane::XY)),
        original: Side::Front,
        keep: Keep::Both,
        new_body: Some(BodyId::NEW),
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

fn set(
    editor: &mut Editor,
    feature: FeatureId,
    kind: impl Into<FeatureKind>,
) -> Result<(), EditError> {
    editor.apply(Command::SetFeature {
        feature,
        kind: Box::new(kind.into()),
    })
}

fn split_of(document: &Document, id: FeatureId) -> &Split {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Split(split) => split,
        other => panic!("{other:?}"),
    }
}

/// `split` refused as the next feature of `editor`'s document, for `why`.
fn refused(editor: &mut Editor, split: Split, why: SplitError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, split),
        Err(EditError::Invalid(CheckError::Split(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// Keeping both sides makes a new body, "Body 2", which the split
/// makes; undo takes both away.
#[test]
fn a_split_keeping_both_sides_makes_a_new_body() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let body = before.bodies[0].id;
    // The new body's id is given whatever the command held.
    let split = Split {
        new_body: None,
        ..by_xy(body)
    };
    let id = add(&mut editor, split).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Split 1");
    assert_eq!(feature.kind.noun(), "Split");
    let made = document.bodies.last().unwrap();
    assert_eq!((made.name.as_str(), made.created_by), ("Body 2", id));
    assert_eq!(split_of(document, id).new_body, Some(made.id));
    assert_eq!(feature.kind.new_body(), Some(made.id));
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(split_of(document, id).kept(), Side::Front);
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(editor.document().features.last().unwrap().id, id);
}

/// Keeping one side is a trim: no new body, whatever the command held,
/// and that side keeps the id whatever `original` says.
#[test]
fn keeping_one_side_makes_no_body() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let body = before.bodies[0].id;
    let trim = Split {
        keep: Keep::Back,
        original: Side::Front,
        ..by_xy(body)
    };
    let id = add(&mut editor, trim).unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, before.bodies);
    let split = split_of(document, id);
    assert_eq!((split.new_body, split.kept()), (None, Side::Back));
}

/// Editing a split keeps its new body while it keeps both sides, takes
/// it away when it keeps one, and makes another when it keeps both
/// again; a later feature naming the new body refuses taking it away.
#[test]
fn editing_what_is_kept_adds_and_removes_the_new_body() {
    let mut editor = Editor::new(with_body());
    let body = editor.document().bodies[0].id;
    let id = add(&mut editor, by_xy(body)).unwrap();
    let made = split_of(editor.document(), id).new_body.unwrap();
    // `original` switched: the same new body, now the front.
    let back = Split {
        original: Side::Back,
        new_body: Some(BodyId::NEW),
        ..by_xy(body)
    };
    set(&mut editor, id, back.clone()).unwrap();
    assert_eq!(split_of(editor.document(), id).new_body, Some(made));
    assert_eq!(split_of(editor.document(), id).kept(), Side::Back);
    // One side: the body goes.
    let front = Split {
        keep: Keep::Front,
        ..back.clone()
    };
    set(&mut editor, id, front.clone()).unwrap();
    assert!(editor.document().body(made).is_none());
    assert_eq!(split_of(editor.document(), id).new_body, None);
    // Both again: a new one.
    set(&mut editor, id, back.clone()).unwrap();
    let again = split_of(editor.document(), id).new_body.unwrap();
    assert_ne!(again, made);
    assert_eq!(editor.document().body(again).unwrap().created_by, id);
    // A scale of the new body holds it.
    let ask = Scale::factor_ask(&editor.document().design());
    let scale = Scale {
        bodies: vec![again],
        about: crate::PointRef::Origin,
        factor: ScaleFactor::Uniform(varde_expr::Value::new("2", &ask).unwrap()),
    };
    let scaled = add(&mut editor, scale).unwrap();
    let before = editor.document().clone();
    let refused = set(&mut editor, id, front);
    assert_eq!(
        refused,
        Err(EditError::Invalid(CheckError::Scale(
            scaled,
            crate::ScaleError::Body(again)
        )))
    );
    assert_eq!(*editor.document(), before);
    // Setting what's there changes nothing.
    let revision = editor.revision();
    set(&mut editor, id, back).unwrap();
    assert_eq!(editor.revision(), revision);
}

/// A sketch tool hides its sketch and builds on it; its regions and
/// curves are checked as an extrude's.
#[test]
fn sketch_tools_use_their_sketch() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let sketch = document.features[0].id;
    let body = document.bodies[0].id;
    editor
        .apply(Command::SetFeatureVisible(sketch, true))
        .unwrap();
    let regions = extrude_of(&document, document.features[1].id)
        .regions
        .clone();
    let split = Split {
        tool: SplitTool::Regions {
            sketch,
            regions: regions.clone(),
        },
        ..by_xy(body)
    };
    let id = add(&mut editor, split).unwrap();
    let feature = editor.document().feature(id).unwrap();
    assert_eq!(feature.kind.uses(), [sketch]);
    assert_eq!(feature.kind.sketch(), Some(sketch));
    assert!(!editor.document().feature(sketch).unwrap().visible);
    // Removing the sketch removes the split and its new body.
    let removal = editor.document().removal(Removable::Feature(sketch));
    assert!(removal.features.contains(&id));
    let made = split_of(editor.document(), id).new_body.unwrap();
    assert!(removal.bodies.contains(&made));

    let FeatureKind::Sketch { sketch: drawn, .. } = &document.features[0].kind else {
        unreachable!()
    };
    let mut curves: Vec<Id> = drawn.curves.iter().map(|entry| entry.id).collect();
    curves.sort_unstable();
    let chain = |curves: Vec<Id>| Split {
        tool: SplitTool::Chain { sketch, curves },
        ..by_xy(body)
    };
    add(&mut editor, chain(vec![curves[0]])).unwrap();
    // Curves: none, out of order, repeated, not in the sketch.
    refused(&mut editor, chain(Vec::new()), SplitError::Curves(0));
    refused(
        &mut editor,
        chain(vec![curves[1], curves[0]]),
        SplitError::CurveOrder,
    );
    refused(
        &mut editor,
        chain(vec![curves[0], curves[0]]),
        SplitError::CurveOrder,
    );
    // A point's id: no curve has it.
    let missing = drawn.points[0].id;
    refused(
        &mut editor,
        chain(vec![missing]),
        SplitError::Curve(missing),
    );
    // Regions: none, or a sketch that isn't one before it.
    let none = Split {
        tool: SplitTool::Regions {
            sketch,
            regions: Vec::new(),
        },
        ..by_xy(body)
    };
    refused(&mut editor, none, SplitError::Regions(0));
    let extrude = document.features[1].id;
    let not_a_sketch = Split {
        tool: SplitTool::Regions {
            sketch: extrude,
            regions,
        },
        ..by_xy(body)
    };
    refused(&mut editor, not_a_sketch, SplitError::Sketch(extrude));
}

/// Its body, tool body, face tool and plane face are checked as a
/// combine's bodies and a mirror's plane are.
#[test]
fn bodies_and_faces_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    // The split takes the next id and its new body the one after.
    let later = FeatureId(editor.document().next_id + 2);
    // The body: made before, there.
    refused(
        &mut editor,
        by_xy(BodyId(999)),
        SplitError::Body(BodyId(999)),
    );
    // The tool body: not the body, made before, there.
    let with = |tool| Split { tool, ..by_xy(a) };
    refused(
        &mut editor,
        with(SplitTool::Body(a)),
        SplitError::ToolIsBody,
    );
    refused(
        &mut editor,
        with(SplitTool::Body(BodyId(999))),
        SplitError::ToolBody(BodyId(999)),
    );
    // A face tool's body is one it depends on: there.
    refused(
        &mut editor,
        with(SplitTool::Face(top(BodyId(999), first))),
        SplitError::FaceBody(BodyId(999)),
    );
    // A face named by a later feature.
    refused(
        &mut editor,
        with(SplitTool::Face(top(b, later))),
        SplitError::RefMaker(later),
    );
    // A plane face's point out of bounds.
    let mut far = top(b, second);
    far.near.x = f64::INFINITY;
    refused(
        &mut editor,
        with(SplitTool::Plane(PlaneRef::Face(far))),
        SplitError::Face(PlaneError::Near(far.near)),
    );
    // A plane face's body made later.
    let next_body = BodyId(editor.document().next_id + 2);
    refused(
        &mut editor,
        with(SplitTool::Plane(PlaneRef::Face(top(next_body, first)))),
        SplitError::RefBody(next_body),
    );
    // Those that are right: by the other body, by a face of either, by
    // a plane face of the other.
    for tool in [
        SplitTool::Body(b),
        SplitTool::Face(top(a, first)),
        SplitTool::Face(top(b, second)),
        SplitTool::Plane(PlaneRef::Face(top(b, second))),
    ] {
        add(&mut editor, with(tool)).unwrap();
    }
    editor.document().check().unwrap();
}

/// A split depends on its body, its tool body and its face tool's body,
/// not on a plane face's body; removing the split takes its new body and
/// the features naming it.
#[test]
fn removal_follows_the_bodies_it_names() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let by_body = add(
        &mut editor,
        Split {
            tool: SplitTool::Body(b),
            ..by_xy(a)
        },
    )
    .unwrap();
    let removal = editor.document().removal(Removable::Feature(second));
    assert_eq!(removal.features, [second, by_body]);
    let piece = split_of(editor.document(), by_body).new_body.unwrap();
    assert_eq!(removal.bodies, [b, piece]);
    // A split of the new body goes with the first split.
    let again = add(&mut editor, by_xy(piece)).unwrap();
    let removal = editor.document().removal(Removable::Feature(by_body));
    assert_eq!(removal.features, [by_body, again]);
    // Removing the new body removes the split that made it.
    let removal = editor.document().removal(Removable::Body(piece));
    assert_eq!(removal.features, [by_body, again]);
    // A face tool's body is depended on.
    let by_face = add(
        &mut editor,
        Split {
            tool: SplitTool::Face(top(b, second)),
            ..by_xy(a)
        },
    )
    .unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert!(removal.features.contains(&by_face));
    // A plane face's body isn't: the split stays, to fail regenerating.
    let by_plane = add(
        &mut editor,
        Split {
            tool: SplitTool::Plane(PlaneRef::Face(top(b, second))),
            ..by_xy(a)
        },
    )
    .unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert!(!removal.features.contains(&by_plane));
    editor.apply(Command::RemoveBody(b)).unwrap();
    assert!(editor.document().feature(by_plane).is_some());
    assert!(editor.document().feature(by_face).is_none());
    let _ = first;
    editor.document().check().unwrap();
}

/// Splits have no values: changing the units leaves them as they are.
#[test]
fn set_units_leaves_splits() {
    let mut editor = Editor::new(with_body());
    let body = editor.document().bodies[0].id;
    let id = add(&mut editor, by_xy(body)).unwrap();
    let split = split_of(editor.document(), id).clone();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    assert_eq!(*split_of(editor.document(), id), split);
}

#[test]
fn splits_round_trip() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    add(&mut editor, by_xy(a)).unwrap();
    let trim = Split {
        tool: SplitTool::Face(top(a, first)),
        original: Side::Back,
        keep: Keep::Back,
        ..by_xy(b)
    };
    add(&mut editor, trim).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose split is wrong are refused as they're read: a new body
/// though it keeps one side, none though it keeps both, one it doesn't
/// make, and a body made by a split that doesn't make it.
#[test]
fn wrong_splits_are_refused_when_read() {
    let (mut editor, [a, b], _) = two_bodies();
    let id = add(&mut editor, by_xy(a)).unwrap();
    let made = split_of(editor.document(), id).new_body.unwrap();
    let read = |change: &dyn Fn(&mut Split)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Split(split) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(split);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(
        read(&|s| s.keep = Keep::Front),
        Err(format!(
            "feature {n}: makes a new body exactly when it keeps both sides"
        ))
    );
    assert_eq!(
        read(&|s| s.new_body = None),
        Err(format!(
            "body {} is made by feature {n}, which doesn't make it",
            made.0
        ))
    );
    assert_eq!(
        read(&|s| s.new_body = Some(b)),
        Err(format!(
            "body {} is made by feature {n}, which doesn't make it",
            made.0
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the split comes after the scale.
#[test]
fn a_split_is_the_tenth_kind() {
    let split = FeatureKind::from(by_xy(BodyId(1)));
    let bytes = postcard::to_stdvec(&split).unwrap();
    // The kind, the body, the tool's kind (a plane), the plane's kind (an
    // origin plane), XY, the sides.
    assert_eq!(bytes[..5], [9, 1, 0, 0, 0]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        SplitError::ToolIsBody.to_string(),
        "splits a body with itself"
    );
    assert_eq!(
        CheckError::Split(FeatureId(3), SplitError::Body(BodyId(2))).to_string(),
        "feature 3: splits body 2, which isn't there or no earlier feature makes"
    );
    let sketch = &with_body().features[0].kind;
    let FeatureKind::Sketch { sketch, .. } = sketch else {
        unreachable!()
    };
    let id = sketch.curves[0].id;
    assert_eq!(
        SplitError::Curve(id).to_string(),
        format!("its line names curve {id}, which its sketch doesn't have")
    );
}

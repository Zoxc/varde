use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{CheckError, Command, Document, EditError, Editor, FeatureKind, LengthUnit, Removable};

/// The example's body and another plate: the editor, the bodies and
/// their makers.
fn two_bodies() -> (Editor, [BodyId; 2], [FeatureId; 2]) {
    let example = with_body();
    let first = (example.bodies[0].id, example.features[1].id);
    let mut editor = Editor::new(example);
    let (maker, body) = extrude_again(&mut editor);
    (editor, [first.0, body], [first.1, maker])
}

/// The face of `body` made by `maker` as `part`, picked at `near`.
fn face(body: BodyId, maker: FeatureId, part: PartKey, near: DVec3) -> FaceRef {
    FaceRef {
        body,
        key: FaceKey {
            feature: maker.get(),
            part,
            instance: 0,
        },
        near,
    }
}

fn thickness(document: &Document, text: &str) -> Value {
    Value::new(text, &Shell::thickness_ask(&document.design())).unwrap()
}

/// `body`, made by `maker`, shelled 1 mm inward, open at its top and
/// one wall.
fn open_top(document: &Document, body: BodyId, maker: FeatureId) -> Shell {
    let mut open = vec![
        face(body, maker, PartKey::EndCap, DVec3::new(2.0, 2.0, 10.0)),
        face(
            body,
            maker,
            PartKey::Side { curve: 1 },
            DVec3::new(5.0, 0.0, 5.0),
        ),
    ];
    open.sort_by(FaceRef::order);
    Shell {
        body,
        open,
        thickness: thickness(document, "1 mm"),
        outward: false,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn shell_of(document: &Document, id: FeatureId) -> &Shell {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Shell(shell) => shell,
        other => panic!("{other:?}"),
    }
}

/// `shell` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, shell: Shell, why: ShellError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, shell),
        Err(EditError::Invalid(CheckError::Shell(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A shell is "Shell 1", makes no body, names its body and uses no
/// sketch; undo takes it away and redo puts it back.
#[test]
fn a_shell_is_added_and_undone() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let (body, maker) = (before.bodies[0].id, before.features[1].id);
    let shell = open_top(&before, body, maker);
    let id = add(&mut editor, shell.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Shell 1");
    assert_eq!(feature.kind.noun(), "Shell");
    assert_eq!(*shell_of(document, id), shell);
    assert_eq!(document.bodies, before.bodies);
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.new_body(), None);
    // A closed one, opening nothing, is "Shell 2".
    let closed = Shell {
        open: Vec::new(),
        ..shell.clone()
    };
    let again = add(&mut editor, closed.clone()).unwrap();
    assert_eq!(editor.document().feature(again).unwrap().name, "Shell 2");
    assert_eq!(*shell_of(editor.document(), again), closed);
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*shell_of(editor.document(), id), shell);
}

/// Editing a shell's thickness, direction and faces keeps its id and
/// name, one undo step each; setting what's there changes nothing.
#[test]
fn a_shell_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, open_top(&document, body, maker)).unwrap();
    let set = |editor: &mut Editor, shell: Shell| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(shell.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, open_top(&document, body, maker)).unwrap();
    assert_eq!(editor.generation(), generation);
    let mut edited = Shell {
        thickness: thickness(&document, "2.5"),
        outward: true,
        ..open_top(&document, body, maker)
    };
    edited.open.pop();
    set(&mut editor, edited.clone()).unwrap();
    let now = editor.document();
    assert_eq!(*shell_of(now, id), edited);
    assert_eq!(now.feature(id).unwrap().name, "Shell 1");
    editor.undo();
    assert_eq!(
        *shell_of(editor.document(), id),
        open_top(&document, body, maker)
    );
}

/// Its own parts: the count, the order, every face's own check, one
/// body, and the thickness by its ask.
#[test]
fn its_own_parts_are_checked() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let design = document.design();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let good = open_top(&document, body, maker);
    assert_eq!(good.check_own(&design), Ok(()));
    let check = |change: &dyn Fn(&mut Shell)| {
        let mut shell = good.clone();
        change(&mut shell);
        shell.check_own(&design)
    };
    // None is a closed hollow body.
    assert_eq!(check(&|s| s.open.clear()), Ok(()));
    let many = |s: &mut Shell| {
        s.open = (0..=MAX_SHELL_FACES)
            .map(|i| {
                face(
                    body,
                    maker,
                    PartKey::EndCap,
                    DVec3::new(i as f64, 0.0, 10.0),
                )
            })
            .collect();
    };
    assert_eq!(check(&many), Err(ShellError::Faces(MAX_SHELL_FACES + 1)));
    // As many as there may be.
    assert_eq!(
        check(&|s| {
            many(s);
            s.open.pop();
        }),
        Ok(())
    );
    assert_eq!(check(&|s| s.open.reverse()), Err(ShellError::FaceOrder));
    assert_eq!(
        check(&|s| s.open[1] = s.open[0]),
        Err(ShellError::FaceOrder)
    );
    // One key picked at two points: two faces (a face cut in two).
    assert_eq!(
        check(&|s| {
            s.open[1] = s.open[0];
            s.open[1].near.x += 1.0;
        }),
        Ok(())
    );
    // NaN isn't equal to itself: matched.
    for far in [
        DVec3::new(f64::NAN, 0.0, 0.0),
        DVec3::new(0.0, f64::INFINITY, 0.0),
        DVec3::new(0.0, 0.0, 2.0 * f64::from(crate::MAX_COORD)),
    ] {
        assert!(matches!(
            check(&|s| s.open[0].near = far),
            Err(ShellError::Face(PlaneError::Near(_)))
        ));
    }
    assert_eq!(
        check(&|s| s.open[1].body = BodyId(s.body.0 + 1)),
        Err(ShellError::Bodies)
    );
    assert_eq!(
        check(&|s| s.body = BodyId(s.body.0 + 1)),
        Err(ShellError::Bodies)
    );
    // The thickness: a length as an extrude's.
    let ask = Shell::thickness_ask(&design);
    let zero = Value {
        text: "0".into(),
        value: 0.0,
    };
    assert!(zero.check(&ask).is_err());
    assert_eq!(
        check(&|s| s.thickness = zero.clone()),
        Err(ShellError::Thickness)
    );
    assert!(Value::new("-1", &ask).is_err());
    assert!(Value::new("1e9", &ask).is_err());
    // A value whose text doesn't give its value, as a file could hold.
    let lying = Value {
        text: "2 mm".into(),
        value: 3.0,
    };
    assert_eq!(
        check(&|s| s.thickness = lying.clone()),
        Err(ShellError::Thickness)
    );
    // Refused by the editor too.
    refused(
        &mut editor,
        Shell {
            thickness: zero,
            ..good.clone()
        },
        ShellError::Thickness,
    );
}

/// What it names: its body there and made before it, its faces' makers
/// before it or gone with ids no later feature can take.
#[test]
fn bodies_and_makers_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    // The shell takes the next id.
    let later = FeatureId(editor.document().next_id + 1);
    refused(
        &mut editor,
        Shell {
            open: Vec::new(),
            ..open_top(&document, BodyId(999), first)
        },
        ShellError::Body(BodyId(999)),
    );
    refused(
        &mut editor,
        open_top(&document, a, later),
        ShellError::RefMaker(later),
    );
    let id = add(&mut editor, open_top(&document, b, second)).unwrap();
    // On the other body's faces named by the first body's maker: the
    // keys don't have to be the body's maker's.
    add(&mut editor, open_top(&document, b, first)).unwrap();
    let document = editor.document();
    document.check().unwrap();
    let index = document.feature_index(id).unwrap();
    let shell = shell_of(document, id);
    assert_eq!(
        document.check_shell_faces(index, shell.body, &shell.open),
        Ok(())
    );
    // Made by the shell itself or later: refused.
    assert_eq!(
        document.check_shell_faces(1, shell.body, &shell.open),
        Err(ShellError::Body(b))
    );
    assert_eq!(
        document.check_shell_faces(index, a, &open_top(document, a, later).open),
        Err(ShellError::RefMaker(later))
    );
}

/// A shell depends on its body (removing it or its maker removes the
/// shell) but not on the features that made its open faces: those go,
/// and the shell stays, to fail regenerating.
#[test]
fn removal_follows_the_body_not_the_faces() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let on_b = add(&mut editor, open_top(&document, b, second)).unwrap();
    // Faces on body a, named by the second extrude's keys.
    let on_a = add(&mut editor, open_top(&document, a, second)).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [second, on_b]);
    let removal = editor.document().removal(Removable::Feature(first));
    assert!(removal.features.contains(&on_a));
    assert!(!removal.features.contains(&on_b));
    editor.apply(Command::RemoveBody(b)).unwrap();
    let document = editor.document();
    assert!(document.feature(on_b).is_none());
    // The shell whose faces' maker went stays.
    assert!(document.feature(on_a).is_some());
    document.check().unwrap();
    let removal = document.removal(Removable::Body(a));
    assert!(removal.features.contains(&on_a));
    // Undone, both are back.
    editor.undo();
    assert!(editor.document().feature(on_b).is_some());
}

/// Changing the units pins a shell's thickness in the units it was
/// typed in, as an extrude's distances.
#[test]
fn set_units_pins_its_thickness() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let shell = Shell {
        thickness: thickness(&document, "2"),
        ..open_top(&document, body, maker)
    };
    let id = add(&mut editor, shell).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let thickness = &shell_of(editor.document(), id).thickness;
    assert_eq!((thickness.text.as_str(), thickness.value), ("2 mm", 2.0));
    editor.document().check().unwrap();
}

#[test]
fn shells_round_trip() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, open_top(&document, a, first)).unwrap();
    let outward = Shell {
        open: Vec::new(),
        thickness: thickness(&document, "0.5"),
        outward: true,
        ..open_top(&document, b, second)
    };
    add(&mut editor, outward).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose shell is wrong are refused as they're read.
#[test]
fn wrong_shells_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, open_top(&document, a, first)).unwrap();
    let read = |change: &dyn Fn(&mut Shell)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Shell(shell) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(shell);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|s| s.open[1].body = b),
        Err(format!("feature {n}: a face it opens is on another body"))
    );
    assert_eq!(
        read(&|s| s.open.reverse()),
        Err(format!(
            "feature {n}: its faces are out of order or repeated"
        ))
    );
    // Moved to the other body with no faces: a shell the document takes.
    assert!(
        read(&|s| {
            s.body = b;
            s.open.clear();
        })
        .is_ok()
    );
    assert_eq!(
        read(&|s| s.body = BodyId(999)),
        Err(format!("feature {n}: a face it opens is on another body"))
    );
    assert_eq!(
        read(&|s| {
            s.body = BodyId(999);
            s.open.clear();
        }),
        Err(format!(
            "feature {n}: shells body 999, which isn't there or no earlier feature makes"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the shell comes after the chamfer.
#[test]
fn a_shell_is_the_twelfth_kind() {
    let document = with_body();
    let shell = open_top(&document, BodyId(1), FeatureId(2));
    let bytes = postcard::to_stdvec(&FeatureKind::from(shell)).unwrap();
    // The kind, the body, the face count, the first face's body.
    assert_eq!(bytes[..4], [11, 1, 2, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::Shell(FeatureId(3), ShellError::Body(BodyId(2))).to_string(),
        "feature 3: shells body 2, which isn't there or no earlier feature makes"
    );
    assert_eq!(
        ShellError::Faces(300).to_string(),
        "opens 300 faces, more than 256"
    );
    assert_eq!(
        ShellError::RefMaker(FeatureId(7)).to_string(),
        "a face it opens was made by feature 7, which doesn't come before it"
    );
    assert_eq!(
        ShellError::Face(PlaneError::Near(DVec3::new(f64::NAN, 0.0, 0.0))).to_string(),
        "its face's point [NaN, 0, 0] is out of bounds"
    );
}

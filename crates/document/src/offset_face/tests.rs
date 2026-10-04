use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, FaceSetError, FeatureKind, LengthUnit,
    Removable,
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

fn distance(document: &Document, text: &str) -> Value {
    Value::new(text, &OffsetFace::distance_ask(&document.design())).unwrap()
}

/// `body`'s top and one wall, made by `maker`, moved 1 mm out, tangent
/// faces taken in.
fn top_and_wall(document: &Document, body: BodyId, maker: FeatureId) -> OffsetFace {
    let mut faces = vec![
        face(body, maker, PartKey::EndCap, DVec3::new(2.0, 2.0, 10.0)),
        face(
            body,
            maker,
            PartKey::Side { curve: 1 },
            DVec3::new(5.0, 0.0, 5.0),
        ),
    ];
    faces.sort_by(FaceRef::order);
    OffsetFace {
        faces,
        distance: distance(document, "1 mm"),
        inward: false,
        tangent: true,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn offset_of(document: &Document, id: FeatureId) -> &OffsetFace {
    match &document.feature(id).unwrap().kind {
        FeatureKind::OffsetFace(offset) => offset,
        other => panic!("{other:?}"),
    }
}

/// `offset` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, offset: OffsetFace, why: OffsetFaceError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, offset),
        Err(EditError::Invalid(CheckError::OffsetFace(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// An offset face is "Offset face 1", makes no body, names its faces' body
/// and uses no sketch; undo takes it away and redo puts it back.
#[test]
fn an_offset_face_is_added_and_undone() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let (body, maker) = (before.bodies[0].id, before.features[1].id);
    let offset = top_and_wall(&before, body, maker);
    let id = add(&mut editor, offset.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Offset face 1");
    assert_eq!(feature.kind.noun(), "Offset face");
    assert_eq!(*offset_of(document, id), offset);
    assert_eq!(document.bodies, before.bodies);
    assert_eq!(offset.body(), Some(body));
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.new_body(), None);
    assert_eq!(offset.signed_distance(), 1.0);
    // Inward, "Offset face 2", the distance taken off.
    let inward = OffsetFace {
        inward: true,
        tangent: false,
        ..offset.clone()
    };
    assert_eq!(inward.signed_distance(), -1.0);
    let again = add(&mut editor, inward.clone()).unwrap();
    assert_eq!(
        editor.document().feature(again).unwrap().name,
        "Offset face 2"
    );
    assert_eq!(*offset_of(editor.document(), again), inward);
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*offset_of(editor.document(), id), offset);
}

/// Editing an offset face's distance, side, tangent faces and faces
/// keeps its id and name, one undo step each; setting what's there
/// changes nothing.
#[test]
fn an_offset_face_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, top_and_wall(&document, body, maker)).unwrap();
    let set = |editor: &mut Editor, offset: OffsetFace| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(offset.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, top_and_wall(&document, body, maker)).unwrap();
    assert_eq!(editor.generation(), generation);
    let mut edited = OffsetFace {
        distance: distance(&document, "2.5"),
        inward: true,
        tangent: false,
        ..top_and_wall(&document, body, maker)
    };
    edited.faces.pop();
    set(&mut editor, edited.clone()).unwrap();
    let now = editor.document();
    assert_eq!(*offset_of(now, id), edited);
    assert_eq!(now.feature(id).unwrap().name, "Offset face 1");
    editor.undo();
    assert_eq!(
        *offset_of(editor.document(), id),
        top_and_wall(&document, body, maker)
    );
}

/// Its own parts: at least one face and at most the limit, the order,
/// every face's own check, one body, and the distance by its ask.
#[test]
fn its_own_parts_are_checked() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let design = document.design();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let good = top_and_wall(&document, body, maker);
    assert_eq!(good.check_own(&design), Ok(()));
    let check = |change: &dyn Fn(&mut OffsetFace)| {
        let mut offset = good.clone();
        change(&mut offset);
        offset.check_own(&design)
    };
    assert_eq!(check(&|o| o.faces.clear()), Err(OffsetFaceError::NoFaces));
    let many = |o: &mut OffsetFace| {
        o.faces = (0..=MAX_OFFSET_FACES)
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
    assert_eq!(
        check(&many),
        Err(OffsetFaceError::Faces(MAX_OFFSET_FACES + 1))
    );
    assert_eq!(
        check(&|o| {
            many(o);
            o.faces.pop();
        }),
        Ok(())
    );
    assert_eq!(
        check(&|o| o.faces.reverse()),
        Err(OffsetFaceError::FaceOrder)
    );
    assert_eq!(
        check(&|o| o.faces[1] = o.faces[0]),
        Err(OffsetFaceError::FaceOrder)
    );
    // One key picked at two points: two faces (a face cut in two).
    assert_eq!(
        check(&|o| {
            o.faces[1] = o.faces[0];
            o.faces[1].near.x += 1.0;
        }),
        Ok(())
    );
    for far in [
        DVec3::new(f64::NAN, 0.0, 0.0),
        DVec3::new(0.0, f64::INFINITY, 0.0),
        DVec3::new(0.0, 0.0, 2.0 * f64::from(crate::MAX_COORD)),
    ] {
        assert!(matches!(
            check(&|o| o.faces[0].near = far),
            Err(OffsetFaceError::Face(PlaneError::Near(_)))
        ));
    }
    // The second face on another body: the list's order is by body
    // first, so it's still in order.
    assert_eq!(
        check(&|o| o.faces[1].body = BodyId(body.0 + 1)),
        Err(OffsetFaceError::Bodies)
    );
    // The distance: a length as an extrude's, above zero (the side is
    // `inward`'s).
    let ask = OffsetFace::distance_ask(&design);
    let zero = Value {
        text: "0".into(),
        value: 0.0,
    };
    assert_eq!(
        check(&|o| o.distance = zero.clone()),
        Err(OffsetFaceError::Distance)
    );
    assert!(Value::new("-1", &ask).is_err());
    assert!(Value::new("1e9", &ask).is_err());
    let lying = Value {
        text: "2 mm".into(),
        value: 3.0,
    };
    assert_eq!(
        check(&|o| o.distance = lying.clone()),
        Err(OffsetFaceError::Distance)
    );
    refused(
        &mut editor,
        OffsetFace {
            distance: zero,
            ..good.clone()
        },
        OffsetFaceError::Distance,
    );
    refused(
        &mut editor,
        OffsetFace {
            faces: Vec::new(),
            ..good
        },
        OffsetFaceError::NoFaces,
    );
}

/// What it names: its faces' body there and made before it, its faces'
/// makers before it or gone with ids no later feature can take.
#[test]
fn bodies_and_makers_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let later = FeatureId(editor.document().next_id + 1);
    refused(
        &mut editor,
        top_and_wall(&document, BodyId(999), first),
        OffsetFaceError::Body(BodyId(999)),
    );
    refused(
        &mut editor,
        top_and_wall(&document, a, later),
        OffsetFaceError::RefMaker(later),
    );
    let id = add(&mut editor, top_and_wall(&document, b, second)).unwrap();
    // On the other body's faces named by the first body's maker.
    add(&mut editor, top_and_wall(&document, b, first)).unwrap();
    let document = editor.document();
    document.check().unwrap();
    let index = document.feature_index(id).unwrap();
    let offset = offset_of(document, id);
    assert_eq!(document.check_face_set(index, b, &offset.faces), Ok(()));
    assert_eq!(
        document.check_face_set(1, b, &offset.faces),
        Err(FaceSetError::Body(b))
    );
    assert_eq!(
        OffsetFaceError::from(FaceSetError::RefMaker(later)),
        OffsetFaceError::RefMaker(later)
    );
}

/// An offset face depends on its faces' body (removing it or its maker
/// removes the offset) but not on the features that made its faces
/// otherwise: those go, and the offset stays, to fail regenerating.
#[test]
fn removal_follows_the_body_not_the_faces() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let on_b = add(&mut editor, top_and_wall(&document, b, second)).unwrap();
    let on_a = add(&mut editor, top_and_wall(&document, a, second)).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [second, on_b]);
    let removal = editor.document().removal(Removable::Feature(first));
    assert!(removal.features.contains(&on_a));
    assert!(!removal.features.contains(&on_b));
    editor.apply(Command::RemoveBody(b)).unwrap();
    let document = editor.document();
    assert!(document.feature(on_b).is_none());
    assert!(document.feature(on_a).is_some());
    document.check().unwrap();
    editor.undo();
    assert!(editor.document().feature(on_b).is_some());
}

/// Changing the units pins an offset's distance in the units it was
/// typed in, as an extrude's distances.
#[test]
fn set_units_pins_its_distance() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let offset = OffsetFace {
        distance: distance(&document, "2"),
        ..top_and_wall(&document, body, maker)
    };
    let id = add(&mut editor, offset).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let distance = &offset_of(editor.document(), id).distance;
    assert_eq!((distance.text.as_str(), distance.value), ("2 mm", 2.0));
    editor.document().check().unwrap();
}

#[test]
fn offset_faces_round_trip() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, top_and_wall(&document, a, first)).unwrap();
    let inward = OffsetFace {
        distance: distance(&document, "0.5"),
        inward: true,
        tangent: false,
        ..top_and_wall(&document, b, second)
    };
    add(&mut editor, inward).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose offset face is wrong are refused as they're read.
#[test]
fn wrong_offset_faces_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, top_and_wall(&document, a, first)).unwrap();
    let read = |change: &dyn Fn(&mut OffsetFace)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::OffsetFace(offset) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(offset);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|o| o.faces[1].body = b),
        Err(format!("feature {n}: its faces aren't all on one body"))
    );
    assert_eq!(
        read(&|o| o.faces.reverse()),
        Err(format!(
            "feature {n}: its faces are out of order or repeated"
        ))
    );
    assert_eq!(
        read(&|o| o.faces.clear()),
        Err(format!("feature {n}: it moves no face"))
    );
    // Moved to the other body as a whole: one the document takes.
    assert!(
        read(&|o| {
            for face in &mut o.faces {
                face.body = b;
            }
        })
        .is_ok()
    );
    assert_eq!(
        read(&|o| {
            for face in &mut o.faces {
                face.body = BodyId(999);
            }
        }),
        Err(format!(
            "feature {n}: moves faces of body 999, which isn't there or no earlier feature makes"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the offset face comes after the shell. (A kind
/// added in parallel would move it on: this test holds its index.)
#[test]
fn an_offset_face_is_the_fourteenth_kind() {
    let document = with_body();
    let offset = top_and_wall(&document, BodyId(1), FeatureId(2));
    let bytes = postcard::to_stdvec(&FeatureKind::from(offset)).unwrap();
    // The kind, the face count, the first face's body.
    assert_eq!(bytes[..3], [13, 2, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::OffsetFace(FeatureId(3), OffsetFaceError::Body(BodyId(2))).to_string(),
        "feature 3: moves faces of body 2, which isn't there or no earlier feature makes"
    );
    assert_eq!(
        OffsetFaceError::Faces(300).to_string(),
        "moves 300 faces, more than 256"
    );
    assert_eq!(
        OffsetFaceError::RefMaker(FeatureId(7)).to_string(),
        "a face it moves was made by feature 7, which doesn't come before it"
    );
    assert_eq!(OffsetFaceError::NoFaces.to_string(), "it moves no face");
}

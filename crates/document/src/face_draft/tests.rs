use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, FaceSetError, FeatureKind, LengthUnit,
    OriginPlane, Removable,
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

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &FaceDraft::angle_ask(&document.design())).unwrap()
}

/// `body`'s bottom, made by `maker`, as a neutral plane.
fn bottom(body: BodyId, maker: FeatureId) -> PlaneRef {
    PlaneRef::Face(face(
        body,
        maker,
        PartKey::StartCap,
        DVec3::new(2.0, 2.0, 0.0),
    ))
}

/// Two walls of `body`, made by `maker`, drafted 3° from its bottom,
/// tangent faces taken in.
fn two_walls(document: &Document, body: BodyId, maker: FeatureId) -> FaceDraft {
    let mut faces = vec![
        face(
            body,
            maker,
            PartKey::Side { curve: 1 },
            DVec3::new(5.0, 0.0, 5.0),
        ),
        face(
            body,
            maker,
            PartKey::Side { curve: 2 },
            DVec3::new(10.0, 5.0, 5.0),
        ),
    ];
    faces.sort_by(FaceRef::order);
    FaceDraft {
        faces,
        neutral: bottom(body, maker),
        angle: angle(document, "3"),
        flip: false,
        tangent: true,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn draft_of(document: &Document, id: FeatureId) -> &FaceDraft {
    match &document.feature(id).unwrap().kind {
        FeatureKind::FaceDraft(draft) => draft,
        other => panic!("{other:?}"),
    }
}

/// `draft` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, draft: FaceDraft, why: FaceDraftError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, draft),
        Err(EditError::Invalid(CheckError::FaceDraft(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A draft is "Draft 1", makes no body, names its faces' body and uses
/// no sketch; undo takes it away and redo puts it back.
#[test]
fn a_draft_is_added_and_undone() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let (body, maker) = (before.bodies[0].id, before.features[1].id);
    let draft = two_walls(&before, body, maker);
    let id = add(&mut editor, draft.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Draft 1");
    assert_eq!(feature.kind.noun(), "Draft");
    assert_eq!(*draft_of(document, id), draft);
    assert_eq!(document.bodies, before.bodies);
    assert_eq!(draft.body(), Some(body));
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.new_body(), None);
    assert!((draft.angle.value - 3f64.to_radians()).abs() < 1e-15);
    // From the XY plane, flipped: "Draft 2".
    let flipped = FaceDraft {
        neutral: PlaneRef::Origin(OriginPlane::XY),
        flip: true,
        tangent: false,
        ..draft.clone()
    };
    assert_eq!(flipped.neutral_face(), None);
    let again = add(&mut editor, flipped.clone()).unwrap();
    assert_eq!(editor.document().feature(again).unwrap().name, "Draft 2");
    assert_eq!(*draft_of(editor.document(), again), flipped);
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*draft_of(editor.document(), id), draft);
}

/// Editing a draft's angle, flip, tangent faces, neutral plane and faces
/// keeps its id and name, one undo step each; setting what's there
/// changes nothing.
#[test]
fn a_draft_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, two_walls(&document, body, maker)).unwrap();
    let set = |editor: &mut Editor, draft: FaceDraft| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(draft.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, two_walls(&document, body, maker)).unwrap();
    assert_eq!(editor.generation(), generation);
    let mut edited = FaceDraft {
        angle: angle(&document, "7.5 deg"),
        neutral: PlaneRef::Origin(OriginPlane::XZ),
        flip: true,
        tangent: false,
        ..two_walls(&document, body, maker)
    };
    edited.faces.pop();
    set(&mut editor, edited.clone()).unwrap();
    let now = editor.document();
    assert_eq!(*draft_of(now, id), edited);
    assert_eq!(now.feature(id).unwrap().name, "Draft 1");
    editor.undo();
    assert_eq!(
        *draft_of(editor.document(), id),
        two_walls(&document, body, maker)
    );
}

/// Its own parts: at least one face and at most the limit, the order,
/// every face's own check, one body, the neutral face's own check, and
/// the angle by its ask (above 0, under 90°).
#[test]
fn its_own_parts_are_checked() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let design = document.design();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let good = two_walls(&document, body, maker);
    assert_eq!(good.check_own(&design), Ok(()));
    let check = |change: &dyn Fn(&mut FaceDraft)| {
        let mut draft = good.clone();
        change(&mut draft);
        draft.check_own(&design)
    };
    assert_eq!(check(&|d| d.faces.clear()), Err(FaceDraftError::NoFaces));
    let many = |d: &mut FaceDraft| {
        d.faces = (0..=MAX_DRAFT_FACES)
            .map(|i| {
                face(
                    body,
                    maker,
                    PartKey::Side { curve: 1 },
                    DVec3::new(i as f64, 0.0, 5.0),
                )
            })
            .collect();
    };
    assert_eq!(
        check(&many),
        Err(FaceDraftError::Faces(MAX_DRAFT_FACES + 1))
    );
    assert_eq!(
        check(&|d| {
            many(d);
            d.faces.pop();
        }),
        Ok(())
    );
    assert_eq!(
        check(&|d| d.faces.reverse()),
        Err(FaceDraftError::FaceOrder)
    );
    assert_eq!(
        check(&|d| d.faces[1] = d.faces[0]),
        Err(FaceDraftError::FaceOrder)
    );
    for far in [
        DVec3::new(f64::NAN, 0.0, 0.0),
        DVec3::new(0.0, f64::INFINITY, 0.0),
        DVec3::new(0.0, 0.0, 2.0 * f64::from(crate::MAX_COORD)),
    ] {
        assert!(matches!(
            check(&|d| d.faces[0].near = far),
            Err(FaceDraftError::Face(PlaneError::Near(_)))
        ));
        assert!(matches!(
            check(&|d| {
                let PlaneRef::Face(face) = &mut d.neutral else {
                    unreachable!()
                };
                face.near = far;
            }),
            Err(FaceDraftError::Neutral(PlaneError::Near(_)))
        ));
    }
    assert_eq!(
        check(&|d| d.faces[1].body = BodyId(body.0 + 1)),
        Err(FaceDraftError::Bodies)
    );
    // The angle: above 0 and under 90°.
    let ask = FaceDraft::angle_ask(&design);
    assert!(Value::new("0", &ask).is_err());
    assert!(Value::new("-3", &ask).is_err());
    assert!(Value::new("90", &ask).is_err());
    assert!(Value::new("89.9", &ask).is_ok());
    let zero = Value {
        text: "0".into(),
        value: 0.0,
    };
    assert_eq!(
        check(&|d| d.angle = zero.clone()),
        Err(FaceDraftError::Angle)
    );
    let right = Value {
        text: "90".into(),
        value: std::f64::consts::FRAC_PI_2,
    };
    assert_eq!(
        check(&|d| d.angle = right.clone()),
        Err(FaceDraftError::Angle)
    );
    let lying = Value {
        text: "3".into(),
        value: 0.5,
    };
    assert_eq!(
        check(&|d| d.angle = lying.clone()),
        Err(FaceDraftError::Angle)
    );
    refused(
        &mut editor,
        FaceDraft {
            angle: zero,
            ..good.clone()
        },
        FaceDraftError::Angle,
    );
    refused(
        &mut editor,
        FaceDraft {
            faces: Vec::new(),
            ..good
        },
        FaceDraftError::NoFaces,
    );
}

/// What it names: its faces' body there and made before it, its faces'
/// makers before it or gone with ids no later feature can take; the
/// neutral face's body likewise made before it, its maker before it.
#[test]
fn bodies_and_makers_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let later = FeatureId(editor.document().next_id + 1);
    refused(
        &mut editor,
        two_walls(&document, BodyId(999), first),
        FaceDraftError::Body(BodyId(999)),
    );
    refused(
        &mut editor,
        FaceDraft {
            neutral: bottom(a, later),
            ..two_walls(&document, a, first)
        },
        FaceDraftError::NeutralMaker(later),
    );
    refused(
        &mut editor,
        FaceDraft {
            neutral: bottom(BodyId(999), first),
            ..two_walls(&document, a, first)
        },
        FaceDraftError::NeutralBody(BodyId(999)),
    );
    let mut wrong = two_walls(&document, a, later);
    wrong.neutral = PlaneRef::Origin(OriginPlane::XY);
    refused(&mut editor, wrong, FaceDraftError::RefMaker(later));
    // b's walls from a's bottom: both bodies named.
    let across = FaceDraft {
        neutral: bottom(a, first),
        ..two_walls(&document, b, second)
    };
    let id = add(&mut editor, across.clone()).unwrap();
    assert_eq!(across.bodies(), [a, b]);
    let document = editor.document();
    document.check().unwrap();
    let index = document.feature_index(id).unwrap();
    let draft = draft_of(document, id);
    assert_eq!(document.check_face_set(index, b, &draft.faces), Ok(()));
    assert_eq!(document.check_neutral_plane(index, &draft.neutral), Ok(()));
    assert_eq!(
        document.check_neutral_plane(1, &draft.neutral),
        Err(FaceDraftError::NeutralBody(a))
    );
    assert_eq!(
        document.check_neutral_plane(1, &PlaneRef::Origin(OriginPlane::YZ)),
        Ok(())
    );
    assert_eq!(
        FaceDraftError::from(FaceSetError::RefMaker(later)),
        FaceDraftError::RefMaker(later)
    );
}

/// A draft depends on its faces' body and its neutral face's body
/// (removing either, or its maker, removes the draft) but not on the
/// features that made its faces otherwise: those go, and the draft
/// stays, to fail regenerating.
#[test]
fn removal_follows_the_bodies_not_the_faces() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let on_b = add(&mut editor, two_walls(&document, b, second)).unwrap();
    let from_b = add(
        &mut editor,
        FaceDraft {
            neutral: bottom(b, second),
            ..two_walls(&document, a, second)
        },
    )
    .unwrap();
    let on_a = add(&mut editor, two_walls(&document, a, second)).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [second, on_b, from_b]);
    let removal = editor.document().removal(Removable::Feature(first));
    assert!(removal.features.contains(&on_a));
    assert!(removal.features.contains(&from_b));
    assert!(!removal.features.contains(&on_b));
    editor.apply(Command::RemoveBody(b)).unwrap();
    let document = editor.document();
    assert!(document.feature(on_b).is_none());
    assert!(document.feature(from_b).is_none());
    assert!(document.feature(on_a).is_some());
    document.check().unwrap();
    editor.undo();
    assert!(editor.document().feature(from_b).is_some());
}

/// Changing the units pins a draft's angle as it was typed.
#[test]
fn set_units_pins_its_angle() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, two_walls(&document, body, maker)).unwrap();
    let before = draft_of(editor.document(), id).angle.clone();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let angle = &draft_of(editor.document(), id).angle;
    assert_eq!(angle.value, before.value);
    editor.document().check().unwrap();
}

#[test]
fn drafts_round_trip() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, two_walls(&document, a, first)).unwrap();
    let flipped = FaceDraft {
        angle: angle(&document, "0.5"),
        neutral: PlaneRef::Origin(OriginPlane::YZ),
        flip: true,
        tangent: false,
        ..two_walls(&document, b, second)
    };
    add(&mut editor, flipped).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose draft is wrong are refused as they're read.
#[test]
fn wrong_drafts_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, two_walls(&document, a, first)).unwrap();
    let read = |change: &dyn Fn(&mut FaceDraft)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::FaceDraft(draft) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(draft);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|d| d.faces[1].body = b),
        Err(format!("feature {n}: its faces aren't all on one body"))
    );
    assert_eq!(
        read(&|d| d.faces.clear()),
        Err(format!("feature {n}: it drafts no face"))
    );
    assert_eq!(
        read(&|d| d.angle.value = 2.0),
        Err(format!(
            "feature {n}: its angle's expression doesn't give its value, or it isn't an angle it takes"
        ))
    );
    // The neutral face on the other body: one the document takes.
    assert!(read(&|d| d.neutral = bottom(b, first)).is_ok());
    assert_eq!(
        read(&|d| d.neutral = bottom(BodyId(999), first)),
        Err(format!(
            "feature {n}: its neutral plane is a face of body 999, which isn't there or no earlier feature makes"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the draft comes after the offset face. (A kind
/// added in parallel moves it on: this one test holds its index.)
#[test]
fn a_draft_is_the_fifteenth_kind() {
    let document = with_body();
    let draft = two_walls(&document, BodyId(1), FeatureId(2));
    let bytes = postcard::to_stdvec(&FeatureKind::from(draft)).unwrap();
    // The kind, the face count, the first face's body.
    assert_eq!(bytes[..3], [14, 2, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::FaceDraft(FeatureId(3), FaceDraftError::Body(BodyId(2))).to_string(),
        "feature 3: drafts faces of body 2, which isn't there or no earlier feature makes"
    );
    assert_eq!(
        FaceDraftError::Faces(300).to_string(),
        "drafts 300 faces, more than 256"
    );
    assert_eq!(
        FaceDraftError::NeutralMaker(FeatureId(7)).to_string(),
        "its neutral plane's face was made by feature 7, which doesn't come before it"
    );
    assert_eq!(FaceDraftError::NoFaces.to_string(), "it drafts no face");
}

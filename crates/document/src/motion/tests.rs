use glam::DVec3;
use varde_expr::Value;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{
    CheckError, Command, Document, EditError, Editor, FeatureKind, LengthUnit, Removable, Removal,
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

fn length(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::offset_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::angle_ask(&document.design())).unwrap()
}

/// A move of `bodies` by `x, y, z` (typed), with no turn.
fn shift(document: &Document, bodies: &[BodyId], [x, y, z]: [&str; 3]) -> Move {
    Move {
        bodies: bodies.to_vec(),
        offset: [x, y, z].map(|text| length(document, text)),
        turn: None,
    }
}

fn mirror(bodies: &[BodyId], plane: PlaneRef) -> Mirror {
    Mirror {
        bodies: bodies.to_vec(),
        plane,
        keep_original: true,
    }
}

/// Adds `kind` as one edit: its id.
fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

/// The top face of the example's plate, made by `maker`, on `body`.
fn face(body: BodyId, maker: FeatureId) -> FaceRef {
    FaceRef {
        body,
        key: FaceKey {
            feature: maker.get(),
            part: PartKey::EndCap,
            instance: 0,
        },
        near: DVec3::new(0.0, 0.0, 10.0),
    }
}

#[test]
fn adding_a_move_and_a_mirror_makes_no_body() {
    let (mut editor, [a, b], _) = two_bodies();
    let before = editor.document().clone();
    let moved = shift(editor.document(), &[a, b], ["10", "-5", "0"]);
    let id = add(&mut editor, moved.clone()).unwrap();
    let flipped = mirror(&[b], PlaneRef::Origin(OriginPlane::XZ));
    let other = add(&mut editor, flipped.clone()).unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, before.bodies, "no body added");
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Move 1");
    assert_eq!(feature.kind, FeatureKind::Move(moved.clone()));
    assert_eq!(feature.kind.noun(), "Move");
    assert_eq!(feature.kind.bodies(), vec![a, b]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.new_body(), None);
    assert_eq!(moved.offset_vector(), DVec3::new(10.0, -5.0, 0.0));
    let feature = document.feature(other).unwrap();
    assert_eq!(feature.name, "Mirror 1");
    assert_eq!(feature.kind.bodies(), vec![b]);
    assert_eq!(feature.kind.operation(), None);
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
}

#[test]
fn bodies_are_bounded_sorted_and_made_before() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let refused = |editor: &mut Editor, kind: FeatureKind, why: MotionError| {
        let next = FeatureId(editor.document().next_id);
        let error = add(editor, kind.clone()).unwrap_err();
        let expected = match kind {
            FeatureKind::Move(_) => CheckError::Move(next, why),
            _ => CheckError::Mirror(next, why),
        };
        assert_eq!(error, EditError::Invalid(expected));
    };
    let document = editor.document().clone();
    let plane = PlaneRef::Origin(OriginPlane::XY);
    refused(
        &mut editor,
        shift(&document, &[], ["1", "0", "0"]).into(),
        MotionError::Bodies(0),
    );
    refused(
        &mut editor,
        shift(&document, &[b, a], ["1", "0", "0"]).into(),
        MotionError::BodyOrder,
    );
    refused(
        &mut editor,
        mirror(&[a, a], plane).into(),
        MotionError::BodyOrder,
    );
    let many: Vec<BodyId> = (0..=MAX_FEATURE_BODIES as u64)
        .map(|k| BodyId(1000 + k))
        .collect();
    refused(
        &mut editor,
        mirror(&many, plane).into(),
        MotionError::Bodies(MAX_FEATURE_BODIES + 1),
    );
    let missing = BodyId(document.next_id + 3);
    refused(
        &mut editor,
        shift(&document, &[a, missing], ["1", "0", "0"]).into(),
        MotionError::Body(missing),
    );
    // A move set in place of the first extrude can't move a body made
    // after it.
    let error = editor
        .apply(Command::SetFeature {
            feature: first,
            kind: Box::new(shift(&document, &[b], ["1", "0", "0"]).into()),
        })
        .unwrap_err();
    assert!(
        matches!(error, EditError::Invalid(CheckError::Move(_, _))),
        "{error:?}"
    );
    assert_eq!(*editor.document(), document);
}

#[test]
fn offsets_and_angles_are_checked() {
    let (editor, [a, _], [first, _]) = two_bodies();
    let document = editor.document();
    let design = document.design();
    // Zero and negative offsets are fine; past the limit isn't a length
    // it takes.
    assert_eq!(
        shift(document, &[a], ["0", "-1e6", "1e6"]).check_own(&design),
        Ok(())
    );
    assert!(Value::new("1e6 + 1", &Move::offset_ask(&design)).is_err());
    // A value changed behind its text, or past the limit, is refused.
    let mut tampered = shift(document, &[a], ["1", "2", "3"]);
    tampered.offset[1].value = 2.5;
    assert_eq!(tampered.check_own(&design), Err(MotionError::Offset));
    tampered.offset[1] = Value {
        text: "2e6".into(),
        value: 2e6,
    };
    assert_eq!(tampered.check_own(&design), Err(MotionError::Offset));
    tampered.offset[1] = Value {
        text: "nan".into(),
        value: f64::NAN,
    };
    assert_eq!(tampered.check_own(&design), Err(MotionError::Offset));
    // Angles within a turn either way.
    let mut turned = shift(document, &[a], ["0", "0", "0"]);
    turned.turn = Some((AxisRef::Origin(Axis3::Z), angle(document, "-90")));
    assert_eq!(turned.check_own(&design), Ok(()));
    assert!(Value::new("361", &Move::angle_ask(&design)).is_err());
    turned.turn = Some((
        AxisRef::Origin(Axis3::Z),
        Value {
            text: "400".into(),
            value: 400f64.to_radians(),
        },
    ));
    assert_eq!(turned.check_own(&design), Err(MotionError::Angle));
    // An axis edge's own parts.
    let key = face(a, first).key;
    let edge = EdgeRef {
        body: a,
        faces: [key, key],
        near: DVec3::ZERO,
    };
    turned.turn = Some((AxisRef::Edge(edge), angle(document, "30")));
    assert_eq!(
        turned.check_own(&design),
        Err(MotionError::Edge(EdgeError::Faces))
    );
    let mut far = face(a, first);
    far.near.x = f64::INFINITY;
    turned.turn = Some((AxisRef::Face(far), angle(document, "30")));
    assert_eq!(turned.check_own(&design), Err(MotionError::Near(far.near)));
    assert_eq!(
        mirror(&[a], PlaneRef::Face(far)).check_own(),
        Err(MotionError::Near(far.near))
    );
}

/// An axis or plane names a body and faces' makers before it, as a
/// sketch's face does: one that isn't there is allowed if no later body
/// or feature can take its id.
#[test]
fn references_name_what_comes_before() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    // Mirroring the first body in the second's top face, and turning it
    // about the first's own top face.
    let id = add(&mut editor, mirror(&[a], PlaneRef::Face(face(b, second)))).unwrap();
    let mut turned = shift(editor.document(), &[a], ["0", "0", "0"]);
    turned.turn = Some((
        AxisRef::Face(face(a, first)),
        angle(editor.document(), "45"),
    ));
    add(&mut editor, turned.clone()).unwrap();
    // A body or maker that comes later is refused.
    let later = FeatureId(editor.document().next_id + 1);
    let mut wrong = turned.clone();
    wrong.turn = Some((
        AxisRef::Face(face(a, later)),
        angle(editor.document(), "45"),
    ));
    let next = FeatureId(editor.document().next_id);
    assert_eq!(
        add(&mut editor, wrong).unwrap_err(),
        EditError::Invalid(CheckError::Move(next, MotionError::RefMaker(later)))
    );
    let body = BodyId(editor.document().next_id + 1);
    assert_eq!(
        add(&mut editor, mirror(&[a], PlaneRef::Face(face(body, first)))).unwrap_err(),
        EditError::Invalid(CheckError::Mirror(next, MotionError::RefBody(body)))
    );
    // Removing the plane's body leaves the mirror, which then fails to
    // regenerate; removing the moved body takes the move and mirror.
    let document = editor.document();
    assert_eq!(
        document.removal(Removable::Body(b)),
        Removal {
            features: vec![second],
            bodies: vec![b],
        }
    );
    editor.apply(Command::RemoveBody(b)).unwrap();
    assert!(editor.document().feature(id).is_some());
    let removal = editor.document().removal(Removable::Body(a));
    assert_eq!(removal.bodies, [a]);
    assert_eq!(
        removal.features.len(),
        3,
        "the plate, the mirror and the move"
    );
}

#[test]
fn set_units_pins_offsets_and_the_angle() {
    let (mut editor, [a, _], _) = two_bodies();
    let mut moved = shift(editor.document(), &[a], ["10", "-2.5", "0"]);
    moved.turn = Some((AxisRef::Origin(Axis3::X), angle(editor.document(), "90")));
    let id = add(&mut editor, moved.clone()).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let FeatureKind::Move(pinned) = &editor.document().feature(id).unwrap().kind else {
        panic!("a move");
    };
    let values = |m: &Move| m.offset.clone().map(|v| v.value);
    assert_eq!(values(pinned), values(&moved));
    assert_eq!(pinned.offset[0].text, "10 mm");
    assert_eq!(pinned.offset[1].text, "-2.5 mm");
    let (_, turn) = pinned.turn.as_ref().unwrap();
    assert_eq!(turn.value, moved.turn.unwrap().1.value);
    editor.document().check().unwrap();
}

#[test]
fn moves_and_mirrors_round_trip() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let mut moved = shift(editor.document(), &[a, b], ["1", "2", "3"]);
    moved.turn = Some((
        AxisRef::Face(face(a, first)),
        angle(editor.document(), "30"),
    ));
    add(&mut editor, moved).unwrap();
    let edge = EdgeRef {
        body: a,
        faces: [face(a, first).key, face(b, first).key.copy(7, 1)],
        near: DVec3::new(1.0, 2.0, 3.0),
    };
    let mut about_edge = shift(editor.document(), &[b], ["0", "0", "-4"]);
    about_edge.turn = Some((AxisRef::Edge(edge), angle(editor.document(), "-15")));
    add(&mut editor, about_edge).unwrap();
    add(&mut editor, mirror(&[a], PlaneRef::Face(face(a, first)))).unwrap();
    let mut plain = mirror(&[a, b], PlaneRef::Origin(OriginPlane::YZ));
    plain.keep_original = false;
    add(&mut editor, plain).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose move or mirror is wrong are refused as they're read.
#[test]
fn wrong_moves_and_mirrors_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let shifted = shift(editor.document(), &[a], ["1", "0", "0"]);
    let id = add(&mut editor, shifted).unwrap();
    let other = add(&mut editor, mirror(&[b], PlaneRef::Face(face(a, first)))).unwrap();
    fn moved(kind: &mut FeatureKind) -> &mut Move {
        match kind {
            FeatureKind::Move(moved) => moved,
            _ => unreachable!(),
        }
    }
    fn mirrored(kind: &mut FeatureKind) -> &mut Mirror {
        match kind {
            FeatureKind::Mirror(mirror) => mirror,
            _ => unreachable!(),
        }
    }
    let read = |change: &dyn Fn(&mut FeatureKind, &mut FeatureKind)| {
        let mut document = editor.document().clone();
        let [i, j] = [id, other].map(|f| document.feature_index(f).unwrap());
        let (head, tail) = document.features.split_at_mut(j);
        change(&mut head[i].kind, &mut tail[0].kind);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(
        read(&|m, _| moved(m).offset[2].value = f64::NAN),
        Err(format!(
            "feature {n}: an offset's expression doesn't give its value"
        ))
    );
    assert_eq!(
        read(&|m, _| moved(m).offset[0] = Value {
            text: "1e300".into(),
            value: 1e300
        }),
        Err(format!(
            "feature {n}: an offset's expression doesn't give its value"
        ))
    );
    assert_eq!(
        read(&|m, _| moved(m).bodies = vec![b, a]),
        Err(format!(
            "feature {n}: its bodies are out of order or repeated"
        ))
    );
    assert_eq!(
        read(&|m, _| moved(m).bodies = vec![BodyId(u64::MAX - 1)]),
        Err(format!(
            "feature {n}: names body {}, which isn't there or no earlier feature makes",
            u64::MAX - 1
        ))
    );
    let o = other.get();
    assert_eq!(
        read(&|_, m| mirrored(m).bodies.clear()),
        Err(format!("feature {o}: names 0 bodies, not 1 to 256"))
    );
    assert_eq!(
        read(&|_, m| {
            if let PlaneRef::Face(face) = &mut mirrored(m).plane {
                face.near.y = f64::NAN;
            }
        }),
        Err(format!(
            "feature {o}: its face's point [0, NaN, 10] is out of bounds"
        ))
    );
    assert_eq!(
        read(&|_, m| {
            if let PlaneRef::Face(face) = &mut mirrored(m).plane {
                face.key.feature = o;
            }
        }),
        Err(format!(
            "feature {o}: its axis or plane is on a face made by feature {o}, which doesn't \
             come before it"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the move and the mirror come after the combine.
#[test]
fn a_move_and_a_mirror_are_the_fifth_and_sixth_kinds() {
    let mirror = FeatureKind::Mirror(Mirror {
        bodies: vec![BodyId(1)],
        plane: PlaneRef::Origin(OriginPlane::YZ),
        keep_original: false,
    });
    assert_eq!(postcard::to_stdvec(&mirror).unwrap(), [5, 1, 1, 0, 2, 0]);
    let zero = || Value {
        text: "0".into(),
        value: 0.0,
    };
    let moved = FeatureKind::Move(Move {
        bodies: vec![BodyId(2)],
        offset: [zero(), zero(), zero()],
        turn: Some((AxisRef::Origin(Axis3::Y), zero())),
    });
    let bytes = postcard::to_stdvec(&moved).unwrap();
    assert_eq!(bytes[..3], [4, 1, 2]);
    // Three offsets, then the turn: some, origin axis Y, the angle.
    assert_eq!(bytes[bytes.len() - 13..bytes.len() - 10], [1, 0, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        MotionError::Bodies(300).to_string(),
        "names 300 bodies, not 1 to 256"
    );
    assert_eq!(
        MotionError::RefBody(BodyId(4)).to_string(),
        "its axis or plane is on body 4, which isn't made before it"
    );
    assert_eq!(
        MotionError::Edge(EdgeError::Faces).to_string(),
        "its axis: its edge's faces are out of order or the same"
    );
    assert_eq!(Axis3::Y.name(), "Y");
    assert_eq!(Axis3::Z.direction(), DVec3::Z);
    assert_eq!(PlaneRef::Origin(OriginPlane::XZ).name(), "XZ");
}

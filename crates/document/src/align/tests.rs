use glam::DVec3;
use varde_expr::Value;
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

fn key(maker: FeatureId, part: PartKey) -> FaceKey {
    FaceKey {
        feature: maker.get(),
        part,
        instance: 0,
    }
}

/// The face `part` of the plate made by `maker`, on `body`.
fn face(body: BodyId, maker: FeatureId, part: PartKey) -> FaceRef {
    FaceRef {
        body,
        key: key(maker, part),
        near: DVec3::new(0.0, 0.0, 10.0),
    }
}

/// The edge between the plate's top and its side of curve 0.
fn rim(body: BodyId, maker: FeatureId) -> EdgeRef {
    let mut faces = [
        key(maker, PartKey::EndCap),
        key(maker, PartKey::Side { curve: 0 }),
    ];
    faces.sort();
    EdgeRef {
        body,
        faces,
        near: DVec3::new(1.0, 2.0, 10.0),
    }
}

/// A corner of the plate: its top and the sides of curves 0 and 1.
fn corner(body: BodyId, maker: FeatureId) -> PointRef {
    let mut faces = [
        key(maker, PartKey::EndCap),
        key(maker, PartKey::Side { curve: 0 }),
        key(maker, PartKey::Side { curve: 1 }),
    ];
    faces.sort();
    PointRef::Corner {
        body,
        faces,
        near: DVec3::new(1.0, 2.0, 10.0),
    }
}

fn length(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::offset_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Move::angle_ask(&document.design())).unwrap()
}

/// The second body's corner and top onto the first's, with a secondary
/// pair (the sides of curve 0), offset 2 and turned 90°.
fn full(document: &Document, [a, b]: [BodyId; 2], [first, second]: [FeatureId; 2]) -> Align {
    let side = PartKey::Side { curve: 0 };
    Align {
        body: b,
        from: AlignRefs {
            point: corner(b, second),
            primary: Some(DirRef::Normal(face(b, second, PartKey::EndCap))),
            secondary: Some(DirRef::Normal(face(b, second, side))),
        },
        to: AlignRefs {
            point: corner(a, first),
            primary: Some(DirRef::Normal(face(a, first, PartKey::EndCap))),
            secondary: Some(DirRef::Axis(AxisRef::Edge(rim(a, first)))),
        },
        flip: true,
        offset: Some(length(document, "2")),
        turn: Some(angle(document, "90")),
    }
}

/// A point alone: the second body's rim's middle onto the origin.
fn point_only([_, b]: [BodyId; 2], [_, second]: [FeatureId; 2]) -> Align {
    Align {
        body: b,
        from: AlignRefs {
            point: PointRef::Middle(rim(b, second)),
            primary: None,
            secondary: None,
        },
        to: AlignRefs {
            point: PointRef::Origin,
            primary: None,
            secondary: None,
        },
        flip: false,
        offset: None,
        turn: None,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

/// `align` refused as the next feature of `editor`'s document, for `why`.
fn refused(editor: &mut Editor, align: Align, why: AlignError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, align),
        Err(EditError::Invalid(CheckError::Align(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

#[test]
fn adding_an_align_makes_no_body() {
    let (mut editor, bodies, makers) = two_bodies();
    let before = editor.document().clone();
    let align = full(editor.document(), bodies, makers);
    let id = add(&mut editor, align.clone()).unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, before.bodies, "no body added");
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Align 1");
    assert_eq!(feature.kind, FeatureKind::from(align));
    assert_eq!(feature.kind.noun(), "Align");
    assert_eq!(feature.kind.bodies(), vec![bodies[1]]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    // A point alone, onto the origin.
    add(&mut editor, point_only(bodies, makers)).unwrap();
    assert_eq!(editor.document().features.last().unwrap().name, "Align 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
}

/// Both sides pair up: a primary on both or neither, a secondary
/// likewise, a secondary only with a primary; flip, offset and turn only
/// with primaries.
#[test]
fn the_sides_must_pair() {
    let (mut editor, bodies, makers) = two_bodies();
    let good = full(editor.document(), bodies, makers);
    let mut one = good.clone();
    one.to.secondary = None;
    refused(&mut editor, one.clone(), AlignError::Unpaired);
    one.from.secondary = None;
    add(&mut editor, one.clone()).unwrap();
    editor.undo();
    let mut lone = one.clone();
    lone.to.primary = None;
    refused(&mut editor, lone, AlignError::Unpaired);
    // A secondary without a primary, on both sides.
    let mut no_primary = good.clone();
    no_primary.from.primary = None;
    no_primary.to.primary = None;
    refused(&mut editor, no_primary, AlignError::Unpaired);
    let plain = point_only(bodies, makers);
    let document = editor.document().clone();
    let mut flipped = plain.clone();
    flipped.flip = true;
    refused(&mut editor, flipped, AlignError::Options);
    let mut offset = plain.clone();
    offset.offset = Some(length(&document, "0"));
    refused(&mut editor, offset, AlignError::Options);
    let mut turned = plain;
    turned.turn = Some(angle(&document, "0"));
    refused(&mut editor, turned, AlignError::Options);
}

#[test]
fn the_offset_and_turn_are_checked() {
    let (mut editor, bodies, makers) = two_bodies();
    let document = editor.document().clone();
    let design = document.design();
    let mut align = full(&document, bodies, makers);
    align.offset = Some(length(&document, "-1e6"));
    align.turn = Some(angle(&document, "-360"));
    assert_eq!(align.check_own(&design), Ok(()));
    for (text, value) in [("2", f64::NAN), ("2e6", 2e6), ("2", 2.5)] {
        let mut tampered = align.clone();
        tampered.offset = Some(Value {
            text: text.into(),
            value,
        });
        refused(&mut editor, tampered, AlignError::Offset);
    }
    let mut tampered = align.clone();
    tampered.turn = Some(Value {
        text: "400".into(),
        value: 400f64.to_radians(),
    });
    refused(&mut editor, tampered, AlignError::Angle);
    let mut tampered = align;
    tampered.turn = Some(Value {
        text: "30".into(),
        value: f64::INFINITY,
    });
    refused(&mut editor, tampered, AlignError::Angle);
}

/// References are checked as a move's axis is, and each side names the
/// bodies it must.
#[test]
fn references_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let good = full(editor.document(), [a, b], [first, second]);
    // A corner's keys sorted and different.
    let mut align = good.clone();
    if let PointRef::Corner { faces, .. } = &mut align.from.point {
        faces.swap(0, 2);
    }
    refused(&mut editor, align, AlignError::Corner);
    let mut align = good.clone();
    if let PointRef::Corner { faces, .. } = &mut align.to.point {
        faces[1] = faces[0];
    }
    refused(&mut editor, align, AlignError::Corner);
    // Points finite and within bounds.
    let mut align = good.clone();
    if let PointRef::Corner { near, .. } = &mut align.to.point {
        near.z = f64::NAN;
    }
    let mut far = face(a, first, PartKey::EndCap);
    far.near.x = 2e6;
    let mut other = good.clone();
    other.to.primary = Some(DirRef::Normal(far));
    refused(&mut editor, other, AlignError::Near(far.near));
    let at = match align.to.point {
        PointRef::Corner { near, .. } => near,
        _ => unreachable!(),
    };
    let check = align.check_own(&editor.document().design());
    assert!(matches!(check, Err(AlignError::Near(p)) if p.z.is_nan() && p.x == at.x));
    // An edge's keys sorted.
    let mut edge = rim(a, first);
    edge.faces.swap(0, 1);
    let mut align = good.clone();
    align.to.secondary = Some(DirRef::Axis(AxisRef::Edge(edge)));
    refused(&mut editor, align, AlignError::Edge(EdgeError::Faces));
    // No origin on the moved side.
    let mut align = good.clone();
    align.from.point = PointRef::Origin;
    refused(&mut editor, align, AlignError::FromOrigin);
    let mut align = good.clone();
    align.from.secondary = Some(DirRef::Origin(Axis3::X));
    refused(&mut editor, align, AlignError::FromOrigin);
    let mut align = good.clone();
    align.from.secondary = Some(DirRef::Axis(AxisRef::Origin(Axis3::X)));
    refused(&mut editor, align, AlignError::FromOrigin);
    // The moved side on the moved body, the target side off it.
    let mut align = good.clone();
    align.from.primary = Some(DirRef::Normal(face(a, first, PartKey::EndCap)));
    refused(&mut editor, align, AlignError::FromBody(a));
    let mut align = good.clone();
    align.to.point = PointRef::Centre(rim(b, second));
    refused(&mut editor, align, AlignError::OnMoved);
    // The moved body made before.
    let missing = BodyId(editor.document().next_id + 3);
    let mut align = good.clone();
    align.body = missing;
    refused(&mut editor, align, AlignError::Body(missing));
    // Target bodies and makers before it.
    let later = FeatureId(editor.document().next_id + 1);
    let mut align = good.clone();
    align.to.primary = Some(DirRef::Axis(AxisRef::Face(face(a, later, PartKey::EndCap))));
    refused(&mut editor, align, AlignError::RefMaker(later));
    let body = BodyId(editor.document().next_id + 1);
    let mut align = good.clone();
    align.to.point = PointRef::Middle(rim(body, first));
    refused(&mut editor, align, AlignError::RefBody(body));
    // A target on the origin's axes and point is fine.
    let mut align = good.clone();
    align.to.point = PointRef::Origin;
    align.to.primary = Some(DirRef::Origin(Axis3::Z));
    align.to.secondary = Some(DirRef::Axis(AxisRef::Origin(Axis3::Y)));
    add(&mut editor, align).unwrap();
    // The checks a panel runs on one side.
    let document = editor.document();
    let end = document.features.len();
    assert_eq!(document.check_align_refs(end, &good.to), Ok(()));
    let early = document.feature_index(second).unwrap();
    assert_eq!(
        document.check_align_refs(early, &good.from),
        Err(AlignError::RefBody(b))
    );
}

/// Removing the moved body takes the align; removing the target's leaves
/// it, to fail regenerating until it's given another.
#[test]
fn removing_bodies() {
    let (mut editor, [a, b], makers) = two_bodies();
    let align = full(editor.document(), [a, b], makers);
    let id = add(&mut editor, align).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [makers[1], id]);
    editor.apply(Command::RemoveBody(a)).unwrap();
    assert!(editor.document().feature(id).is_some());
    editor.document().check().unwrap();
}

#[test]
fn set_units_pins_the_offset_and_turn() {
    let (mut editor, bodies, makers) = two_bodies();
    let align = full(editor.document(), bodies, makers);
    let id = add(&mut editor, align.clone()).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let FeatureKind::Align(pinned) = &editor.document().feature(id).unwrap().kind else {
        panic!("an align");
    };
    let offset = pinned.offset.as_ref().unwrap();
    assert_eq!(offset.text, "2 mm");
    assert_eq!(offset.value, 2.0);
    assert_eq!(
        pinned.turn.as_ref().unwrap().value,
        align.turn.unwrap().value
    );
    editor.document().check().unwrap();
}

#[test]
fn aligns_round_trip() {
    let (mut editor, bodies, makers) = two_bodies();
    let align = full(editor.document(), bodies, makers);
    add(&mut editor, align).unwrap();
    let mut onto_origin = point_only(bodies, makers);
    onto_origin.from.point = PointRef::Centre(rim(bodies[1], makers[1]));
    onto_origin.from.primary = Some(DirRef::Axis(AxisRef::Face(face(
        bodies[1],
        makers[1],
        PartKey::Side { curve: 0 },
    ))));
    onto_origin.to.primary = Some(DirRef::Origin(Axis3::X));
    add(&mut editor, onto_origin).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose align is wrong are refused as they're read.
#[test]
fn wrong_aligns_are_refused_when_read() {
    let (mut editor, bodies, makers) = two_bodies();
    let align = full(editor.document(), bodies, makers);
    let id = add(&mut editor, align).unwrap();
    let read = |change: &dyn Fn(&mut Align)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Align(align) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(align);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(
        read(&|a| a.offset.as_mut().unwrap().value = f64::NAN),
        Err(format!(
            "feature {n}: its offset's expression doesn't give its value"
        ))
    );
    assert_eq!(
        read(&|a| a.to.secondary = None),
        Err(format!("feature {n}: its directions don't pair up"))
    );
    assert_eq!(
        read(&|a| a.body = bodies[0]),
        Err(format!(
            "feature {n}: the moved side names body {}, not the body moved",
            bodies[1].0
        ))
    );
    assert_eq!(
        read(&|a| a.to.point = PointRef::Middle(rim(bodies[1], makers[1]))),
        Err(format!("feature {n}: the target side names the body moved"))
    );
    assert_eq!(
        read(&|a| a.body = BodyId(u64::MAX - 1)),
        Err(format!(
            "feature {n}: moves body {}, which isn't there or no earlier feature makes",
            u64::MAX - 1
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the align comes after the pattern.
#[test]
fn an_align_is_the_eighth_kind() {
    let align = FeatureKind::from(Align {
        body: BodyId(1),
        from: AlignRefs {
            point: PointRef::Middle(EdgeRef {
                body: BodyId(1),
                faces: [
                    key(FeatureId(0), PartKey::StartCap),
                    key(FeatureId(0), PartKey::EndCap),
                ],
                near: DVec3::ZERO,
            }),
            primary: None,
            secondary: None,
        },
        to: AlignRefs {
            point: PointRef::Origin,
            primary: None,
            secondary: None,
        },
        flip: false,
        offset: None,
        turn: None,
    });
    let bytes = postcard::to_stdvec(&align).unwrap();
    // The kind, the body, the point's kind: the middle of an edge.
    assert_eq!(bytes[..4], [7, 1, 2, 1]);
    // The target's point (the origin), no directions, no flip, offset or
    // turn.
    assert_eq!(bytes[bytes.len() - 7..], [0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        AlignError::RefBody(BodyId(4)).to_string(),
        "a reference is on body 4, which isn't made before it"
    );
    assert_eq!(
        AlignError::Edge(EdgeError::Faces).to_string(),
        "its edge's faces are out of order or the same"
    );
    assert_eq!(
        CheckError::Align(FeatureId(3), AlignError::Unpaired).to_string(),
        "feature 3: its directions don't pair up"
    );
}

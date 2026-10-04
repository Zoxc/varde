use glam::DVec3;
use varde_kernel::mesh::{FaceKey, PartKey};

use super::*;
use crate::testing::{extrude_again, with_body};
use crate::{
    BlendEdgesError, CheckError, Command, Document, EdgeError, EditError, Editor, FeatureId,
    FeatureKind, LengthUnit, MAX_BLEND_EDGES, Removable,
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

fn key(maker: FeatureId, part: PartKey) -> FaceKey {
    FaceKey {
        feature: maker.get(),
        part,
        instance: 0,
    }
}

/// The edge on `body` between the top `maker` made and its wall of
/// `curve`, picked at `near`.
fn top_edge(body: BodyId, maker: FeatureId, curve: u64, near: DVec3) -> EdgeRef {
    let mut faces = [
        key(maker, PartKey::EndCap),
        key(maker, PartKey::Side { curve }),
    ];
    faces.sort();
    EdgeRef { body, faces, near }
}

fn distance(document: &Document, text: &str) -> Value {
    Value::new(text, &Chamfer::distance_ask(&document.design())).unwrap()
}

fn angle(document: &Document, text: &str) -> Value {
    Value::new(text, &Chamfer::angle_ask(&document.design())).unwrap()
}

/// Two top edges of `body`, made by `maker`, chamfered 1 mm, tangent
/// chains on.
fn two_edges(document: &Document, body: BodyId, maker: FeatureId) -> Chamfer {
    let mut edges = vec![
        top_edge(body, maker, 1, DVec3::new(5.0, 0.0, 10.0)),
        top_edge(body, maker, 0, DVec3::new(0.0, 5.0, 10.0)),
    ];
    edges.sort_by(EdgeRef::order);
    Chamfer {
        edges,
        distances: ChamferSize::Equal(distance(document, "1 mm")),
        chains: true,
        flip: false,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn chamfer_of(document: &Document, id: FeatureId) -> &Chamfer {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Chamfer(chamfer) => chamfer,
        other => panic!("{other:?}"),
    }
}

/// `chamfer` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, chamfer: Chamfer, why: ChamferError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, chamfer),
        Err(EditError::Invalid(CheckError::Chamfer(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A chamfer is "Chamfer 1", makes no body, names its edges' body and
/// uses no sketch; undo takes it away and redo puts it back.
#[test]
fn a_chamfer_is_added_and_undone() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let (body, maker) = (before.bodies[0].id, before.features[1].id);
    let chamfer = two_edges(&before, body, maker);
    let id = add(&mut editor, chamfer.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Chamfer 1");
    assert_eq!(feature.kind.noun(), "Chamfer");
    assert_eq!(*chamfer_of(document, id), chamfer);
    assert_eq!(document.bodies, before.bodies);
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.new_body(), None);
    // The next is "Chamfer 2".
    let again = add(&mut editor, chamfer.clone()).unwrap();
    assert_eq!(editor.document().feature(again).unwrap().name, "Chamfer 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*chamfer_of(editor.document(), id), chamfer);
}

/// Editing a chamfer's size, flip and chains keeps its id and name, one
/// undo step each; setting what's there changes nothing.
#[test]
fn a_chamfer_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, two_edges(&document, body, maker)).unwrap();
    let set = |editor: &mut Editor, chamfer: Chamfer| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(chamfer.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, two_edges(&document, body, maker)).unwrap();
    assert_eq!(editor.generation(), generation);
    let angled = Chamfer {
        distances: ChamferSize::Angle(distance(&document, "2"), angle(&document, "30")),
        flip: true,
        chains: false,
        ..two_edges(&document, body, maker)
    };
    set(&mut editor, angled.clone()).unwrap();
    let edited = editor.document();
    assert_eq!(*chamfer_of(edited, id), angled);
    assert_eq!(edited.feature(id).unwrap().name, "Chamfer 1");
    editor.undo();
    assert_eq!(
        *chamfer_of(editor.document(), id),
        two_edges(&document, body, maker)
    );
}

/// Its own parts: the count, the order, every edge's own check, one
/// body, and the values each by its ask.
#[test]
fn its_own_parts_are_checked() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let design = document.design();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let good = two_edges(&document, body, maker);
    assert_eq!(good.check_own(&design), Ok(()));
    let check = |change: &dyn Fn(&mut Chamfer)| {
        let mut chamfer = good.clone();
        change(&mut chamfer);
        chamfer.check_own(&design)
    };
    assert_eq!(
        check(&|c| c.edges.clear()),
        Err(ChamferError::Edges(BlendEdgesError::Count(0)))
    );
    let many = |c: &mut Chamfer| {
        c.edges = (0..=MAX_BLEND_EDGES)
            .map(|i| top_edge(body, maker, 0, DVec3::new(i as f64, 0.0, 10.0)))
            .collect();
    };
    assert_eq!(
        check(&many),
        Err(ChamferError::Edges(BlendEdgesError::Count(
            MAX_BLEND_EDGES + 1
        )))
    );
    // As many as there may be.
    assert_eq!(
        check(&|c| {
            many(c);
            c.edges.pop();
        }),
        Ok(())
    );
    assert_eq!(
        check(&|c| c.edges.reverse()),
        Err(ChamferError::Edges(BlendEdgesError::Order))
    );
    assert_eq!(
        check(&|c| c.edges[1] = c.edges[0]),
        Err(ChamferError::Edges(BlendEdgesError::Order))
    );
    // The same faces picked at two points: two edges between them.
    assert_eq!(
        check(&|c| {
            c.edges[1] = c.edges[0];
            c.edges[1].near.x += 1.0;
        }),
        Ok(())
    );
    assert_eq!(
        check(&|c| c.edges[0].faces.reverse()),
        Err(ChamferError::Edges(BlendEdgesError::Edge(EdgeError::Faces)))
    );
    // NaN isn't equal to itself: matched.
    let far = DVec3::new(f64::NAN, 0.0, 0.0);
    assert!(matches!(
        check(&|c| c.edges[0].near = far),
        Err(ChamferError::Edges(BlendEdgesError::Edge(EdgeError::Near(
            _
        ))))
    ));
    assert_eq!(
        check(&|c| {
            let other = editor_body_after(c.edges[0].body);
            c.edges[1].body = other;
        }),
        Err(ChamferError::Edges(BlendEdgesError::Bodies))
    );
    // Values: lengths as an extrude's, an angle above 0 and under 90°.
    let length_ask = Chamfer::distance_ask(&design);
    let zero = Value {
        text: "0".into(),
        value: 0.0,
    };
    assert!(zero.check(&length_ask).is_err());
    assert_eq!(
        check(&|c| c.distances = ChamferSize::Equal(zero.clone())),
        Err(ChamferError::Distance)
    );
    assert_eq!(
        check(&|c| {
            c.distances = ChamferSize::Two(distance(&document, "1"), zero.clone());
        }),
        Err(ChamferError::Distance)
    );
    assert_eq!(
        check(&|c| {
            c.distances = ChamferSize::Two(distance(&document, "1"), distance(&document, "2"));
        }),
        Ok(())
    );
    let right = Value {
        text: "90".into(),
        value: std::f64::consts::FRAC_PI_2,
    };
    assert_eq!(
        check(&|c| c.distances = ChamferSize::Angle(distance(&document, "1"), right.clone())),
        Err(ChamferError::Angle)
    );
    assert!(Value::new("90", &Chamfer::angle_ask(&design)).is_err());
    assert!(Value::new("0", &Chamfer::angle_ask(&design)).is_err());
    assert_eq!(
        check(&|c| c.distances = ChamferSize::Angle(zero.clone(), angle(&document, "45"))),
        Err(ChamferError::Distance)
    );
    // A value whose text doesn't give its value, as a file could hold.
    let lying = Value {
        text: "2 mm".into(),
        value: 3.0,
    };
    assert_eq!(
        check(&|c| c.distances = ChamferSize::Equal(lying.clone())),
        Err(ChamferError::Distance)
    );
    // Refused by the editor too.
    refused(
        &mut editor,
        Chamfer {
            edges: Vec::new(),
            ..good.clone()
        },
        ChamferError::Edges(BlendEdgesError::Count(0)),
    );
}

/// A body id other than `body`, for a chamfer's own check (which looks
/// at ids only).
fn editor_body_after(body: BodyId) -> BodyId {
    BodyId(body.0 + 1)
}

/// What its edges name: their body there and made before it, their
/// faces' makers before it or gone with ids no later feature can take.
#[test]
fn bodies_and_makers_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    // The chamfer takes the next id.
    let later = FeatureId(editor.document().next_id + 1);
    refused(
        &mut editor,
        two_edges(&document, BodyId(999), first),
        ChamferError::Edges(BlendEdgesError::Body(BodyId(999))),
    );
    refused(
        &mut editor,
        two_edges(&document, a, later),
        ChamferError::Edges(BlendEdgesError::RefMaker(later)),
    );
    let id = add(&mut editor, two_edges(&document, b, second)).unwrap();
    // On the other body's faces named by the first body's maker: the
    // keys don't have to be the body's maker's.
    add(&mut editor, two_edges(&document, b, first)).unwrap();
    let document = editor.document();
    document.check().unwrap();
    let index = document.feature_index(id).unwrap();
    assert_eq!(
        document.check_blend_edges(index, &chamfer_of(document, id).edges),
        Ok(())
    );
    // Made by the chamfer itself or later: refused.
    assert_eq!(
        document.check_blend_edges(1, &chamfer_of(document, id).edges),
        Err(BlendEdgesError::Body(b))
    );
}

/// A chamfer depends on its body (removing it or its maker removes the
/// chamfer) but not on the features that made its edges' faces: those
/// go, and the chamfer stays, to fail regenerating.
#[test]
fn removal_follows_the_body_not_the_faces() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let on_b = add(&mut editor, two_edges(&document, b, second)).unwrap();
    // Edges on body a, between faces the second extrude's keys name.
    let on_a = add(&mut editor, two_edges(&document, a, second)).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [second, on_b]);
    let removal = editor.document().removal(Removable::Feature(first));
    assert!(removal.features.contains(&on_a));
    assert!(!removal.features.contains(&on_b));
    editor.apply(Command::RemoveBody(b)).unwrap();
    let document = editor.document();
    assert!(document.feature(on_b).is_none());
    // The chamfer whose faces' maker went stays.
    assert!(document.feature(on_a).is_some());
    document.check().unwrap();
    // And a later edit taking its body away from its maker is refused.
    let removal = document.removal(Removable::Body(a));
    assert!(removal.features.contains(&on_a));
}

/// Changing the units pins a chamfer's values in the units they were
/// typed in, as an extrude's distances.
#[test]
fn set_units_pins_its_values() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let chamfer = Chamfer {
        distances: ChamferSize::Angle(distance(&document, "2"), angle(&document, "30")),
        ..two_edges(&document, body, maker)
    };
    let id = add(&mut editor, chamfer).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let ChamferSize::Angle(d, a) = &chamfer_of(editor.document(), id).distances else {
        panic!("an angle");
    };
    assert_eq!((d.text.as_str(), d.value), ("2 mm", 2.0));
    assert_eq!(a.text, "30");
    editor.document().check().unwrap();
}

#[test]
fn chamfers_round_trip() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, two_edges(&document, a, first)).unwrap();
    let two = Chamfer {
        distances: ChamferSize::Two(distance(&document, "1"), distance(&document, "2.5")),
        flip: true,
        chains: false,
        ..two_edges(&document, b, second)
    };
    add(&mut editor, two).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose chamfer is wrong are refused as they're read.
#[test]
fn wrong_chamfers_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, two_edges(&document, a, first)).unwrap();
    let read = |change: &dyn Fn(&mut Chamfer)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Chamfer(chamfer) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(chamfer);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|c| c.edges[1].body = b),
        Err(format!("feature {n}: its edges are on more than one body"))
    );
    assert_eq!(
        read(&|c| c.edges.reverse()),
        Err(format!(
            "feature {n}: its edges are out of order or repeated"
        ))
    );
    assert_eq!(
        read(&|c| c.edges.clear()),
        Err(format!(
            "feature {n}: names 0 edges, not 1 to {MAX_BLEND_EDGES}"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the chamfer comes after the split.
#[test]
fn a_chamfer_is_the_eleventh_kind() {
    let document = with_body();
    let chamfer = two_edges(&document, BodyId(1), FeatureId(2));
    let bytes = postcard::to_stdvec(&FeatureKind::from(chamfer)).unwrap();
    // The kind, the edge count, the first edge's body.
    assert_eq!(bytes[..3], [10, 2, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::Chamfer(
            FeatureId(3),
            ChamferError::Edges(BlendEdgesError::Body(BodyId(2)))
        )
        .to_string(),
        "feature 3: its edges are on body 2, which isn't there or no earlier feature makes"
    );
    assert_eq!(
        ChamferError::Angle.to_string(),
        "its angle's expression doesn't give its value, or it isn't above 0° and under 90°"
    );
    assert_eq!(
        ChamferError::Edges(BlendEdgesError::Edge(EdgeError::Faces)).to_string(),
        "its edge's faces are out of order or the same"
    );
    assert_eq!(ChamferSize::Two(zero(), zero()).name(), "Two distances");
}

fn zero() -> Value {
    Value {
        text: "0".into(),
        value: 0.0,
    }
}

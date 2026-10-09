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

fn radius(document: &Document, text: &str) -> Value {
    Value::new(text, &Fillet::radius_ask(&document.design())).unwrap()
}

/// Two top edges of `body`, made by `maker`, rounded 2 mm, tangent
/// chains on.
fn two_edges(document: &Document, body: BodyId, maker: FeatureId) -> Fillet {
    let mut edges = vec![
        top_edge(body, maker, 1, DVec3::new(5.0, 0.0, 10.0)),
        top_edge(body, maker, 0, DVec3::new(0.0, 5.0, 10.0)),
    ];
    edges.sort_by(EdgeRef::order);
    Fillet {
        edges,
        faces: Vec::new(),
        radius: radius(document, "2 mm"),
        chains: true,
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features().last().unwrap().id)
}

fn fillet_of(document: &Document, id: FeatureId) -> &Fillet {
    match &document.feature(id).unwrap().kind {
        FeatureKind::Fillet(fillet) => fillet,
        other => panic!("{other:?}"),
    }
}

/// `fillet` refused as the next feature of `editor`'s document, for
/// `why`, leaving the document as it was.
fn refused(editor: &mut Editor, fillet: Fillet, why: FilletError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, fillet),
        Err(EditError::Invalid(CheckError::Fillet(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

/// A fillet is "Fillet 1", makes no body, names its edges' body and
/// uses no sketch; undo takes it away and redo puts it back.
#[test]
fn a_fillet_is_added_and_undone() {
    let mut editor = Editor::new(with_body());
    let before = editor.document().clone();
    let (body, maker) = (before.bodies[0].id, before.features[1].id);
    let fillet = two_edges(&before, body, maker);
    let id = add(&mut editor, fillet.clone()).unwrap();
    let document = editor.document();
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Fillet 1");
    assert_eq!(feature.kind.noun(), "Fillet");
    assert_eq!(*fillet_of(document, id), fillet);
    assert_eq!(document.bodies, before.bodies);
    assert_eq!(feature.kind.bodies(), [body]);
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    assert_eq!(feature.kind.new_body(), None);
    let again = add(&mut editor, fillet.clone()).unwrap();
    assert_eq!(editor.document().feature(again).unwrap().name, "Fillet 2");
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(*fillet_of(editor.document(), id), fillet);
}

/// Editing a fillet's radius and chains keeps its id and name, one undo
/// step each; setting what's there changes nothing.
#[test]
fn a_fillet_is_edited() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let id = add(&mut editor, two_edges(&document, body, maker)).unwrap();
    let set = |editor: &mut Editor, fillet: Fillet| {
        editor.apply(Command::SetFeature {
            feature: id,
            kind: Box::new(fillet.into()),
        })
    };
    let generation = editor.generation();
    set(&mut editor, two_edges(&document, body, maker)).unwrap();
    assert_eq!(editor.generation(), generation);
    let edited = Fillet {
        radius: radius(&document, "3.5"),
        chains: false,
        ..two_edges(&document, body, maker)
    };
    set(&mut editor, edited.clone()).unwrap();
    assert_eq!(*fillet_of(editor.document(), id), edited);
    assert_eq!(editor.document().feature(id).unwrap().name, "Fillet 1");
    editor.undo();
    assert_eq!(
        *fillet_of(editor.document(), id),
        two_edges(&document, body, maker)
    );
}

/// Its own parts: the edges as a chamfer's (count, order, each edge's
/// own check, one body) and the radius by its ask.
#[test]
fn its_own_parts_are_checked() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let design = document.design();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let good = two_edges(&document, body, maker);
    assert_eq!(good.check_own(&design), Ok(()));
    let check = |change: &dyn Fn(&mut Fillet)| {
        let mut fillet = good.clone();
        change(&mut fillet);
        fillet.check_own(&design)
    };
    let edges = |why| Err(FilletError::Edges(why));
    assert_eq!(
        check(&|f| f.edges.clear()),
        edges(BlendEdgesError::Count(0))
    );
    let many = |f: &mut Fillet| {
        f.edges = (0..=MAX_BLEND_EDGES)
            .map(|i| top_edge(body, maker, 0, DVec3::new(i as f64, 0.0, 10.0)))
            .collect();
    };
    assert_eq!(
        check(&many),
        edges(BlendEdgesError::Count(MAX_BLEND_EDGES + 1))
    );
    assert_eq!(
        check(&|f| {
            many(f);
            f.edges.pop();
        }),
        Ok(())
    );
    assert_eq!(check(&|f| f.edges.reverse()), edges(BlendEdgesError::Order));
    assert_eq!(
        check(&|f| f.edges[1] = f.edges[0]),
        edges(BlendEdgesError::Order)
    );
    assert_eq!(
        check(&|f| f.edges[0].faces.reverse()),
        edges(BlendEdgesError::Edge(EdgeError::Faces))
    );
    assert!(matches!(
        check(&|f| f.edges[0].near = DVec3::new(f64::INFINITY, 0.0, 0.0)),
        Err(FilletError::Edges(BlendEdgesError::Edge(EdgeError::Near(
            _
        ))))
    ));
    assert_eq!(
        check(&|f| f.edges[1].body = BodyId(f.edges[0].body.0 + 1)),
        edges(BlendEdgesError::Bodies)
    );
    // The radius: a length as an extrude's, so not zero, nor negative,
    // nor a value its text doesn't give.
    for (text, value) in [("0", 0.0), ("-1", -1.0), ("2 mm", 3.0)] {
        let wrong = Value {
            text: text.into(),
            value,
        };
        assert_eq!(
            check(&|f| f.radius = wrong.clone()),
            Err(FilletError::Radius)
        );
    }
    refused(
        &mut editor,
        Fillet {
            edges: Vec::new(),
            faces: Vec::new(),
            ..good.clone()
        },
        FilletError::Edges(BlendEdgesError::Count(0)),
    );
    refused(
        &mut editor,
        Fillet {
            radius: Value {
                text: "0".into(),
                value: 0.0,
            },
            ..good
        },
        FilletError::Radius,
    );
}

/// What its edges name, as a chamfer's: their body there and made
/// before it, their faces' makers before it or gone with ids no later
/// feature can take.
#[test]
fn bodies_and_makers_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let later = FeatureId(editor.document().next_id + 1);
    refused(
        &mut editor,
        two_edges(&document, BodyId(999), first),
        FilletError::Edges(BlendEdgesError::Body(BodyId(999))),
    );
    refused(
        &mut editor,
        two_edges(&document, a, later),
        FilletError::Edges(BlendEdgesError::RefMaker(later)),
    );
    let id = add(&mut editor, two_edges(&document, b, second)).unwrap();
    add(&mut editor, two_edges(&document, b, first)).unwrap();
    let document = editor.document();
    document.check().unwrap();
    let index = document.feature_index(id).unwrap();
    assert_eq!(
        document.check_blend_edges(index, &fillet_of(document, id).edges, &[]),
        Ok(())
    );
    assert_eq!(
        document.check_blend_edges(1, &fillet_of(document, id).edges, &[]),
        Err(BlendEdgesError::Body(b))
    );
}

/// A fillet depends on its body but not on the features that made its
/// edges' faces.
#[test]
fn removal_follows_the_body_not_the_faces() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let on_b = add(&mut editor, two_edges(&document, b, second)).unwrap();
    let on_a = add(&mut editor, two_edges(&document, a, second)).unwrap();
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
}

/// Changing the units pins its radius in the units it was typed in.
#[test]
fn set_units_pins_its_radius() {
    let mut editor = Editor::new(with_body());
    let document = editor.document().clone();
    let (body, maker) = (document.bodies[0].id, document.features[1].id);
    let fillet = Fillet {
        radius: radius(&document, "2"),
        ..two_edges(&document, body, maker)
    };
    let id = add(&mut editor, fillet).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let radius = &fillet_of(editor.document(), id).radius;
    assert_eq!((radius.text.as_str(), radius.value), ("2 mm", 2.0));
    editor.document().check().unwrap();
}

#[test]
fn fillets_round_trip() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, two_edges(&document, a, first)).unwrap();
    let other = Fillet {
        radius: radius(&document, "0.5"),
        chains: false,
        ..two_edges(&document, b, second)
    };
    add(&mut editor, other).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose fillet is wrong are refused as they're read.
#[test]
fn wrong_fillets_are_refused_when_read() {
    let (mut editor, [a, b], [first, _]) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, two_edges(&document, a, first)).unwrap();
    let read = |change: &dyn Fn(&mut Fillet)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Fillet(fillet) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(fillet);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(read(&|_| {}).as_ref(), Ok(editor.document()));
    assert_eq!(
        read(&|f| f.edges[1].body = b),
        Err(format!("feature {n}: its edges are on more than one body"))
    );
    assert_eq!(
        read(&|f| f.edges.reverse()),
        Err(format!(
            "feature {n}: its edges or faces are out of order or repeated"
        ))
    );
    assert_eq!(
        read(&|f| f.radius.value = 7.0),
        Err(format!(
            "feature {n}: its radius's expression doesn't give its value, or it isn't a length \
             it takes"
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the fillet comes after the shell.
#[test]
fn a_fillet_is_the_thirteenth_kind() {
    let document = with_body();
    let fillet = two_edges(&document, BodyId(1), FeatureId(2));
    let bytes = postcard::to_stdvec(&FeatureKind::from(fillet)).unwrap();
    // The kind, the edge count, the first edge's body.
    assert_eq!(bytes[..3], [12, 2, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        CheckError::Fillet(
            FeatureId(3),
            FilletError::Edges(BlendEdgesError::RefMaker(FeatureId(5)))
        )
        .to_string(),
        "feature 3: an edge is on a face made by feature 5, which doesn't come before it"
    );
    assert_eq!(
        FilletError::Edges(BlendEdgesError::Count(300)).to_string(),
        "names 300 edges and faces, not 1 to 256 of each"
    );
}

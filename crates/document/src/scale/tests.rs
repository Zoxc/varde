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

fn factor(document: &Document, text: &str) -> Value {
    Value::new(text, &Scale::factor_ask(&document.design())).unwrap()
}

fn length(document: &Document, text: &str) -> Value {
    Value::new(text, &Scale::length_ask(&document.design())).unwrap()
}

/// Both bodies scaled by 2 about the first's rim's middle.
fn uniform(document: &Document, bodies: [BodyId; 2], [first, _]: [FeatureId; 2]) -> Scale {
    Scale {
        bodies: bodies.to_vec(),
        about: PointRef::Middle(rim(bodies[0], first)),
        factor: ScaleFactor::Uniform(factor(document, "2")),
    }
}

/// The second body scaled so its rim is 50 long, along its axis only.
fn to_edge(document: &Document, [_, b]: [BodyId; 2], [_, second]: [FeatureId; 2]) -> Scale {
    Scale {
        bodies: vec![b],
        about: PointRef::Origin,
        factor: ScaleFactor::EdgeLength {
            edge: rim(b, second),
            length: length(document, "50"),
            axis_only: true,
        },
    }
}

fn add(editor: &mut Editor, kind: impl Into<FeatureKind>) -> Result<FeatureId, EditError> {
    editor.apply(editor.document().add_feature(kind.into()))?;
    Ok(editor.document().features.last().unwrap().id)
}

/// `scale` refused as the next feature of `editor`'s document, for `why`.
fn refused(editor: &mut Editor, scale: Scale, why: ScaleError) {
    let next = FeatureId(editor.document().next_id);
    let before = editor.document().clone();
    assert_eq!(
        add(editor, scale),
        Err(EditError::Invalid(CheckError::Scale(next, why)))
    );
    assert_eq!(*editor.document(), before);
}

#[test]
fn adding_a_scale_makes_no_body() {
    let (mut editor, bodies, makers) = two_bodies();
    let before = editor.document().clone();
    let scale = uniform(editor.document(), bodies, makers);
    let id = add(&mut editor, scale.clone()).unwrap();
    let document = editor.document();
    assert_eq!(document.bodies, before.bodies, "no body added");
    let feature = document.feature(id).unwrap();
    assert_eq!(feature.name, "Scale 1");
    assert_eq!(feature.kind, FeatureKind::from(scale));
    assert_eq!(feature.kind.noun(), "Scale");
    assert_eq!(feature.kind.bodies(), bodies.to_vec());
    assert_eq!(feature.kind.uses(), Vec::new());
    assert_eq!(feature.kind.sketch(), None);
    assert_eq!(feature.kind.operation(), None);
    let per_axis = Scale {
        factor: ScaleFactor::PerAxis(["1", "0.5", "1000"].map(|t| factor(&before, t))),
        ..uniform(&before, bodies, makers)
    };
    add(&mut editor, per_axis).unwrap();
    add(&mut editor, to_edge(&before, bodies, makers)).unwrap();
    assert_eq!(editor.document().features.last().unwrap().name, "Scale 3");
    editor.undo();
    editor.undo();
    editor.undo();
    assert_eq!(*editor.document(), before);
    editor.redo();
    assert_eq!(editor.document().features.last().unwrap().id, id);
}

#[test]
fn factors_and_lengths_are_checked() {
    let (mut editor, bodies, makers) = two_bodies();
    let document = editor.document().clone();
    let design = document.design();
    let good = uniform(&document, bodies, makers);
    for (text, value) in [
        ("1001", 1001.0),
        ("0.0009", 0.0009),
        ("0", 0.0),
        ("-2", -2.0),
        ("2", f64::NAN),
        ("2", 2.5),
        ("2 mm", 2.0),
    ] {
        let tampered = Scale {
            factor: ScaleFactor::Uniform(Value {
                text: text.into(),
                value,
            }),
            ..good.clone()
        };
        refused(&mut editor, tampered, ScaleError::Factor);
    }
    let mut factors = ["1", "2", "3"].map(|t| factor(&document, t));
    factors[2].value = 1e4;
    let tampered = Scale {
        factor: ScaleFactor::PerAxis(factors),
        ..good.clone()
    };
    refused(&mut editor, tampered, ScaleError::Factor);
    // The bounds themselves are taken.
    for text in ["1000", "0.001", "1e3", "1/1000"] {
        let scale = Scale {
            factor: ScaleFactor::Uniform(factor(&document, text)),
            ..good.clone()
        };
        assert_eq!(scale.check_own(&design), Ok(()), "{text}");
    }
    // An edge length is a length as an extrude's distance.
    let edge = to_edge(&document, bodies, makers);
    for (text, value) in [("0", 0.0), ("2e6", 2e6), ("50", 51.0), ("-5", -5.0)] {
        let mut tampered = edge.clone();
        if let ScaleFactor::EdgeLength { length, .. } = &mut tampered.factor {
            *length = Value {
                text: text.into(),
                value,
            };
        }
        refused(&mut editor, tampered, ScaleError::Length);
    }
}

#[test]
fn bodies_and_references_are_checked() {
    let (mut editor, [a, b], [first, second]) = two_bodies();
    let document = editor.document().clone();
    let good = uniform(&document, [a, b], [first, second]);
    refused(
        &mut editor,
        Scale {
            bodies: vec![],
            ..good.clone()
        },
        ScaleError::Bodies(0),
    );
    refused(
        &mut editor,
        Scale {
            bodies: vec![b, a],
            ..good.clone()
        },
        ScaleError::BodyOrder,
    );
    let missing = BodyId(document.next_id + 3);
    refused(
        &mut editor,
        Scale {
            bodies: vec![a, missing],
            ..good.clone()
        },
        ScaleError::Body(missing),
    );
    // The point's own parts, its body and makers before it.
    let mut edge = rim(a, first);
    edge.faces.swap(0, 1);
    refused(
        &mut editor,
        Scale {
            about: PointRef::Middle(edge),
            ..good.clone()
        },
        ScaleError::About(AlignError::Edge(EdgeError::Faces)),
    );
    let later = FeatureId(document.next_id + 1);
    refused(
        &mut editor,
        Scale {
            about: PointRef::Centre(rim(a, later)),
            ..good.clone()
        },
        ScaleError::RefMaker(later),
    );
    let body = BodyId(document.next_id + 1);
    refused(
        &mut editor,
        Scale {
            about: PointRef::Middle(rim(body, first)),
            ..good.clone()
        },
        ScaleError::RefBody(body),
    );
    // The point may be on a body it doesn't scale.
    add(
        &mut editor,
        Scale {
            bodies: vec![b],
            ..good.clone()
        },
    )
    .unwrap();
    editor.undo();
    // The edge on a scaled body, its own parts, its makers before it.
    let edge = to_edge(&document, [a, b], [first, second]);
    let mut other = edge.clone();
    other.bodies = vec![a];
    refused(&mut editor, other, ScaleError::EdgeBody(b));
    let mut tampered = edge.clone();
    if let ScaleFactor::EdgeLength { edge, .. } = &mut tampered.factor {
        edge.near.y = f64::INFINITY;
    }
    let near = DVec3::new(1.0, f64::INFINITY, 10.0);
    refused(
        &mut editor,
        tampered,
        ScaleError::Edge(EdgeError::Near(near)),
    );
    let mut tampered = edge.clone();
    if let ScaleFactor::EdgeLength { edge, .. } = &mut tampered.factor {
        *edge = rim(b, later);
    }
    refused(&mut editor, tampered, ScaleError::RefMaker(later));
    // The checks a panel runs.
    let end = editor.document().features.len();
    assert_eq!(editor.document().check_scale_refs(end, &edge), Ok(()));
    let early = editor.document().feature_index(second).unwrap();
    assert_eq!(
        editor.document().check_scale_refs(early, &edge),
        Err(ScaleError::RefBody(b))
    );
}

/// Removing a scaled body takes the scale; removing the point's body
/// leaves it, to fail regenerating until it's given another.
#[test]
fn removing_bodies() {
    let (mut editor, [a, b], makers) = two_bodies();
    let document = editor.document().clone();
    let scale = Scale {
        bodies: vec![b],
        ..uniform(&document, [a, b], makers)
    };
    let id = add(&mut editor, scale).unwrap();
    let removal = editor.document().removal(Removable::Body(b));
    assert_eq!(removal.features, [makers[1], id]);
    editor.apply(Command::RemoveBody(a)).unwrap();
    assert!(editor.document().feature(id).is_some());
    editor.document().check().unwrap();
}

/// Factors have no unit and keep their text; an edge length is pinned
/// as an extrude's distance.
#[test]
fn set_units_leaves_factors_and_pins_edge_lengths() {
    let (mut editor, bodies, makers) = two_bodies();
    let document = editor.document().clone();
    let per_axis = Scale {
        factor: ScaleFactor::PerAxis(["2", "0.5", "10 mm / 4 mm"].map(|t| factor(&document, t))),
        ..uniform(&document, bodies, makers)
    };
    let scaled = add(&mut editor, per_axis.clone()).unwrap();
    let edged = add(&mut editor, to_edge(&document, bodies, makers)).unwrap();
    editor.apply(Command::SetUnits(LengthUnit::In)).unwrap();
    let kind = |id| editor.document().feature(id).unwrap().kind.clone();
    assert_eq!(kind(scaled), FeatureKind::from(per_axis));
    let FeatureKind::Scale(pinned) = kind(edged) else {
        panic!("a scale");
    };
    let ScaleFactor::EdgeLength { length, .. } = &pinned.factor else {
        panic!("an edge length");
    };
    assert_eq!(length.text, "50 mm");
    assert_eq!(length.value, 50.0);
    editor.document().check().unwrap();
}

#[test]
fn scales_round_trip() {
    let (mut editor, bodies, makers) = two_bodies();
    let document = editor.document().clone();
    add(&mut editor, uniform(&document, bodies, makers)).unwrap();
    add(&mut editor, to_edge(&document, bodies, makers)).unwrap();
    let document = editor.document().clone();
    assert_eq!(
        Document::from_postcard(&document.to_postcard()),
        Ok(document)
    );
}

/// Bytes whose scale is wrong are refused as they're read.
#[test]
fn wrong_scales_are_refused_when_read() {
    let (mut editor, bodies, makers) = two_bodies();
    let document = editor.document().clone();
    let id = add(&mut editor, to_edge(&document, bodies, makers)).unwrap();
    let read = |change: &dyn Fn(&mut Scale)| {
        let mut document = editor.document().clone();
        let index = document.feature_index(id).unwrap();
        let FeatureKind::Scale(scale) = &mut document.features[index].kind else {
            unreachable!()
        };
        change(scale);
        Document::from_postcard(&document.to_postcard()).map_err(|e| e.to_string())
    };
    let n = id.get();
    assert_eq!(
        read(&|s| s.factor = ScaleFactor::Uniform(Value {
            text: "5000".into(),
            value: 5000.0
        })),
        Err(format!(
            "feature {n}: a factor's expression doesn't give its value, or it isn't from 0.001 \
             to 1000"
        ))
    );
    assert_eq!(
        read(&|s| s.bodies = vec![bodies[0]]),
        Err(format!(
            "feature {n}: its edge is on body {}, which it doesn't scale",
            bodies[1].0
        ))
    );
    assert_eq!(
        read(&|s| s.bodies = vec![BodyId(u64::MAX - 1)]),
        Err(format!(
            "feature {n}: its edge is on body {}, which it doesn't scale",
            bodies[1].0
        ))
    );
}

/// Kinds are stored by name in files, but the workers' postcard keeps
/// the variants' order: the scale comes after the align.
#[test]
fn a_scale_is_the_ninth_kind() {
    let scale = FeatureKind::from(Scale {
        bodies: vec![BodyId(1)],
        about: PointRef::Origin,
        factor: ScaleFactor::PerAxis([1.0, 2.0, 3.0].map(|value| Value {
            text: format!("{value}"),
            value,
        })),
    });
    let bytes = postcard::to_stdvec(&scale).unwrap();
    // The kind, one body, its id, the point's kind (the origin), the
    // factor's kind (per axis).
    assert_eq!(bytes[..5], [8, 1, 1, 0, 1]);
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        ScaleError::RefBody(BodyId(4)).to_string(),
        "its point or edge is on body 4, which isn't made before it"
    );
    assert_eq!(
        CheckError::Scale(FeatureId(3), ScaleError::Length).to_string(),
        "feature 3: its edge length's expression doesn't give its value"
    );
    assert_eq!(
        ScaleError::About(AlignError::Corner).to_string(),
        "its point: a corner's faces are out of order or repeated"
    );
}

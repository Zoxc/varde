//! Documents for the crate's tests.

use crate::{BodyId, Document, Editor, Extrude, FeatureId, FeatureKind, Operation};

/// The example design: "Sketch 1", "Extrude 1" and "Body 1", which the
/// extrude makes.
pub(crate) fn with_body() -> Document {
    Document::example()
}

/// Extrude `feature` of `document`'s extrude.
pub(crate) fn extrude_of(document: &Document, feature: FeatureId) -> &Extrude {
    match &document
        .feature(feature)
        .expect("the feature is there")
        .kind
    {
        FeatureKind::Extrude(extrude) => extrude,
        FeatureKind::Sketch { .. } => panic!("feature {} is a sketch", feature.0),
    }
}

/// The example's extrude with `operation`.
pub(crate) fn plate(operation: Operation) -> Extrude {
    let example = Document::example();
    Extrude {
        operation,
        ..extrude_of(&example, example.features[1].id).clone()
    }
}

/// Adds another extrude of the example's plate making a new body, as one
/// edit: the extrude's id and the body's.
pub(crate) fn extrude_again(editor: &mut Editor) -> (FeatureId, BodyId) {
    let extrude = plate(Operation::NewBody(BodyId::NEW));
    editor
        .apply(editor.document().add_extrude(extrude))
        .unwrap();
    let document = editor.document();
    let feature = document.features.last().unwrap().id;
    let body = document.bodies.last().unwrap().id;
    assert_eq!(document.body(body).unwrap().created_by, feature);
    (feature, body)
}

//! Documents for the crate's tests.

use crate::{Body, BodyId, Document, FeatureId, FeatureKind, OriginPlane, Plane, Sketch};

/// A document with "Sketch 1" on XY and "Body 1", made by it: no feature
/// makes bodies yet, but a file could name any feature as a body's maker.
pub(crate) fn with_body() -> Document {
    let mut document = Document::default();
    let feature = document
        .add_feature(
            "Sketch 1",
            FeatureKind::Sketch {
                plane: Plane::Origin(OriginPlane::XY),
                sketch: Sketch::default(),
            },
        )
        .unwrap();
    add_body(&mut document, "Body 1", feature);
    document.check().unwrap();
    document
}

/// Adds a body made by `feature` with a new id.
pub(crate) fn add_body(document: &mut Document, name: &str, feature: FeatureId) -> BodyId {
    let id = BodyId(document.new_id().unwrap());
    document.bodies.push(Body {
        id,
        name: name.to_owned(),
        visible: true,
        created_by: feature,
    });
    id
}

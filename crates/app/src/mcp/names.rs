//! Finding a design's features, bodies and sketches by the names the
//! tools take: one lookup each, saying why not as a `String`, and thin
//! adaptors named by what they give, for the edit tools' [`Refused`] and
//! the queries' [`ToolReply`].

use varde_document::{Body, BodyId, Document, Feature, FeatureId};
use varde_mcp::tools::ToolReply;

use super::edit::Refused;
use crate::doc::{Doc, sketch_of};

/// The feature named `name`, by its Timeline name, or why not.
pub(super) fn feature_by_name<'a>(
    document: &'a Document,
    name: &str,
) -> Result<&'a Feature, String> {
    (document.features().iter())
        .find(|feature| feature.name == *name)
        .ok_or_else(|| not_named("feature", name))
}

/// The body named `name`, by its Objects name, or why not.
pub(super) fn body_by_name<'a>(document: &'a Document, name: &str) -> Result<&'a Body, String> {
    (document.bodies().iter())
        .find(|body| body.name == *name)
        .ok_or_else(|| not_named("body", name))
}

/// The sketch feature named `name`, or why not, also when it isn't one.
pub(super) fn sketch_by_name<'a>(
    document: &'a Document,
    name: &str,
) -> Result<&'a Feature, String> {
    let feature = feature_by_name(document, name)?;
    if sketch_of(document, feature.id).is_none() {
        return Err(format!("\"{}\" isn't a sketch", varde_mcp::shown(name)));
    }
    Ok(feature)
}

/// The id of the feature named `name`, or the edit refused.
pub(super) fn feature_id(document: &Document, name: &str) -> Result<FeatureId, Refused> {
    feature_by_name(document, name)
        .map(|feature| feature.id)
        .map_err(Refused::failed)
}

/// The id of the body named `name`, or the edit refused.
pub(super) fn body_id(document: &Document, name: &str) -> Result<BodyId, Refused> {
    body_by_name(document, name)
        .map(|body| body.id)
        .map_err(Refused::failed)
}

/// The id of the sketch feature named `name`, or the edit refused.
pub(super) fn sketch_id(document: &Document, name: &str) -> Result<FeatureId, Refused> {
    sketch_by_name(document, name)
        .map(|feature| feature.id)
        .map_err(Refused::failed)
}

/// The feature of `doc`'s design named `name`, or the answer saying
/// there's none.
pub(super) fn feature_reply<'a>(doc: &'a Doc, name: &str) -> Result<&'a Feature, ToolReply> {
    feature_by_name(doc.editor.document(), name).map_err(ToolReply::error)
}

/// The body of `doc`'s design named `name`, or the answer saying
/// there's none.
pub(super) fn body_reply<'a>(doc: &'a Doc, name: &str) -> Result<&'a Body, ToolReply> {
    body_by_name(doc.editor.document(), name).map_err(ToolReply::error)
}

/// Why a name was found nowhere.
pub(super) fn not_named(what: &str, name: &str) -> String {
    format!("no {what} is named \"{}\"", varde_mcp::shown(name))
}

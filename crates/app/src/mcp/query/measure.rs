//! `measure`: what the measure tool measures of one item or between two,
//! on the committed model, measured by regen apart
//! ([`varde_regen::Query::Measure`]) so the user's own measure, which
//! rides on the regeneration, is neither replaced nor replacing.

use serde_json::{Map, Value, json};
use varde_document::{BodyId, FeatureId, Generation};
use varde_expr::LengthUnit;
use varde_mcp::tools::{Measure as Args, Measured, ToolReply};
use varde_regen::{Entity, Inspect, InspectPick, Inspected, Measure, Probed};
use varde_view::MeasureValue;

use crate::doc::Doc;
use crate::mcp::key::{self, Ref};

/// What a measure says of its model: the design moved on while it was
/// measured, so what's out of date is the values, not (as a summary's
/// note has it) failures; and what a rollback leaves out.
const MEASURE_NOTES: super::ModelNotes = super::ModelNotes {
    stale: "The model is of an earlier state of the design: the design changed while it was \
            measured, so the values may be out of date.",
    rolled_back: Some(
        "The model is rolled back: the features from that point on aren't built, so what \
         they make isn't measured.",
    ),
};

/// The picks `args` names, resolved on `doc`'s committed model, for
/// regen to measure, or the answer now if one isn't there: a key is
/// taken as [`key::found`] takes it (of this design, of the kind it
/// says, naming what's in the model at its point), a body by its Objects
/// name. With them, for each item a body a join merged into another,
/// the body holding it, which regen measures in its place ([`Held`]).
pub(crate) fn measure(doc: &Doc, args: &Args) -> Result<(Inspect, Held), ToolReply> {
    super::committed(doc).map_err(ToolReply::error)?;
    let pick = |which: &str, item: &Measured| {
        resolve(doc, item).map_err(|why| ToolReply::error(format!("{which}: {why}")))
    };
    let inspect = Inspect {
        // The answer comes tagged by the call: the revision says nothing.
        revision: 0,
        first: pick("a", &args.a)?,
        second: args.b.as_ref().map(|b| pick("b", b)).transpose()?,
    };
    // The committed model's merges, which the request is of.
    let merged = doc
        .feed
        .committed_marks()
        .map_or(&[][..], |marks| marks.merged);
    let held = |pick: Option<&InspectPick>| {
        let pick = pick.filter(|pick| pick.entity == Entity::Body)?;
        varde_regen::merged_into(merged, pick.body)
    };
    let held = [held(Some(&inspect.first)), held(inspect.second.as_ref())];
    Ok((inspect, held))
}

/// For `a` and `b`, the body holding the body named, where a join
/// merged it into another: regen measures the holder whole in its place
/// ([`varde_regen::Evaluation::holder`]), and the answer says so.
pub(crate) type Held = [Option<BodyId>; 2];

/// What `item` names, as the measure tool keeps a pick, or why not.
fn resolve(doc: &Doc, item: &Measured) -> Result<InspectPick, String> {
    let item = match item {
        Measured::Body(name) => {
            let document = doc.editor.document();
            let body = super::super::names::body_by_name(document, name)?;
            return Ok(InspectPick {
                body: body.id,
                entity: Entity::Body,
                near: [0.0; 3],
            });
        }
        Measured::Item(item) => item,
    };
    let found = key::found(doc, &item.key, item.kind)?;
    Ok(match found.item {
        Ref::Face(face) => InspectPick {
            body: face.body,
            entity: Entity::Face(face.key),
            near: face.near.to_array(),
        },
        Ref::Edge(edge) => InspectPick {
            body: edge.body,
            entity: Entity::Edge(edge.faces),
            near: edge.near.to_array(),
        },
        Ref::Corner { body, faces, point } => InspectPick {
            body,
            entity: Entity::Corner(faces),
            near: point,
        },
    })
}

/// The answer to `measure` from regen's, `inspected` (or why there's
/// none), measured on the committed model of `generation` rolled back
/// to before `until`: each item's values as the measure tool shows them,
/// typed text in the design's units, keyed by their labels in
/// snake case, and between two the distance, its parts and the angle.
pub(crate) fn measured(
    doc: &Doc,
    args: &Args,
    (generation, until): (Generation, Option<FeatureId>),
    held: Held,
    inspected: Result<&Inspected, &str>,
) -> ToolReply {
    let inspected = match inspected {
        Ok(inspected) => inspected,
        Err(why) => return ToolReply::error(format!("the measure failed: {why}")),
    };
    let document = doc.editor.document();
    let units = document.units();
    let mut answer = json!({ "units": units.symbol() });
    let holder = |body: Option<BodyId>| {
        let body = document.body(body?)?;
        Some(body.name.as_str())
    };
    answer["a"] = item(&args.a, &inspected.first, holder(held[0]), units);
    if let (Some(b), Some(second)) = (&args.b, &inspected.second) {
        answer["b"] = item(b, second, holder(held[1]), units);
    }
    if let Some(between) = &inspected.between {
        let mut said = values(&varde_view::between_values(between, units));
        if let Err(why) = &between.distance {
            said.insert("distance_error".to_owned(), json!(why));
        }
        answer["between"] = Value::Object(said);
    }
    answer["model"] = super::model_state(doc, (generation, until), &MEASURE_NOTES);
    answer["note"] = json!(
        "Measured exactly on the solid. Lengths, areas and volumes are typed text in the \
         design's units, angles in degrees."
    );
    ToolReply::data(answer)
}

/// One item's answer: what it is and its values, or why it wasn't
/// measured. A body merged into `holder` says it was measured as that.
fn item(
    asked: &Measured,
    probed: &Result<Probed, String>,
    holder: Option<&str>,
    units: LengthUnit,
) -> Value {
    let measure = match probed {
        Ok(Probed {
            measure: Ok(measure),
            ..
        }) => measure,
        Ok(Probed {
            measure: Err(why), ..
        })
        | Err(why) => return json!({ "error": why }),
    };
    let kind = match (asked, measure) {
        (Measured::Body(name), _) => format!("body {name}"),
        (_, Measure::Face { summary, .. }) => varde_view::face_kind(summary).to_owned(),
        (_, Measure::Edge { shape, .. }) => varde_view::edge_kind(shape).to_owned(),
        (_, Measure::Point(_)) => "Corner".to_owned(),
        (_, Measure::Body { .. }) => "Body".to_owned(),
    };
    let mut said = json!({
        "kind": kind,
        "values": values(&varde_view::measure_values(measure, units)),
    });
    if let Measure::Edge { closed: true, .. } = measure {
        said["closed"] = json!(true);
    }
    if let (Measured::Body(_), Measure::Body { .. }, Some(holder)) = (asked, measure, holder) {
        said["merged_into"] = json!(holder);
        said["note"] = json!(format!(
            "A join merged it into {holder}: these are the values of {holder} whole."
        ));
    }
    said
}

/// `values` as an object: each label in snake case ("Box size" as
/// `box_size`, "ΔX" as `dx`), its text as the panel shows it.
fn values(values: &[MeasureValue]) -> Map<String, Value> {
    (values.iter())
        .map(|value| (label(value.label), json!(value.shown)))
        .collect()
}

fn label(label: &str) -> String {
    label
        .chars()
        .map(|c| match c {
            'Δ' => 'd',
            ' ' | '-' => '_',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

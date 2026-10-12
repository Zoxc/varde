//! The queries, answered from the open [`Doc`] once its model has caught
//! up with the design asked about (`waiting.rs`, `Wait::Model`):
//! `design_summary`, `feature`, `body_topology`, `sketch`, `measure` and `render`. Values are shown as the UI shows them, in the design's
//! units ([`varde_expr::Shown::current`]); a feature's settings as their
//! typed text.

mod feature;
mod measure;
mod render;
mod sketch;
mod topology;

use serde_json::{Value, json};
use varde_document::{Document, Feature, FeatureId, Generation};
use varde_expr::{LengthUnit, Power, Unit};
use varde_mcp::tools::{Page, ToolReply};
use varde_regen::FeatureFailure;

use crate::doc::{Doc, Shown};

pub(crate) use feature::{Shape, feature, tool_of};
pub(crate) use measure::{Held, measure, measured};
pub(crate) use render::{Shot, render};
pub(crate) use sketch::{Read, analysed, sketch};
pub(crate) use topology::{Asked, body_topology, listed};

/// The answer to every call while no design is open.
pub(crate) const NO_DESIGN: &str = "no design is open";

/// `design_summary`: see `notes/AgentSupport.md`. Its bodies, Timeline and
/// parameters are each paged alone by `page`.
pub(crate) fn design_summary(doc: &Doc, page: Page) -> ToolReply {
    let document = doc.editor.document();
    let committed = doc.feed.committed();
    let failures = committed.as_ref().map_or(&[][..], |model| model.failures);
    // Where the model was cut short: the features from it on weren't built.
    let cut_at = (committed.as_ref())
        .and_then(|model| model.until)
        .and_then(|until| document.feature_index(until));
    let timeline: Vec<Value> = (document.features().iter().enumerate())
        .map(|(at, feature)| {
            let mut row = json!({
                "kind": feature.kind.noun(),
                "name": feature.name,
                "id": feature.id.get(),
            });
            if let Some(failure) = failure(failures, feature) {
                row["failure"] = json!(failure.message);
            }
            if cut_at.is_some_and(|cut| at >= cut) {
                row["built"] = json!(false);
            }
            row
        })
        .collect();
    let bodies: Vec<Value> = (document.bodies().iter())
        .map(|body| json!({ "name": body.name, "visible": body.visible }))
        .collect();
    let rollback = rollback_name(document);
    let model = committed
        .as_ref()
        .map(|model| model_state(doc, (model.generation, model.until), &FAILURES_NOTES));
    let answer = json!({
        "units": document.units().symbol(),
        "tolerance": varde_view::tolerance_label(document.tolerance().fit()),
        "revision": serde_json::to_value(doc.editor.revision()).unwrap_or(Value::Null),
        "rollback": rollback,
        "model": model,
    });
    let lists = vec![
        ("bodies", bodies),
        ("timeline", timeline),
        ("parameters", parameters(document)),
    ];
    ToolReply::data(paged_into(&answer, lists, page, BUDGET))
}

/// The most bytes of JSON a query's answer takes, its pages shortened
/// to keep it there ([`pages`]): the cap less [`changes::ROOM`], for
/// the revision and the user's changes at their briefest, and less
/// [`PENDING_BYTES`] for the `pending_edit` added after (with its key).
/// The changes' full form gets no room kept: [`changes::fitted`] falls
/// back to the brief form when it won't fit, the changes said in full
/// by a later reply.
///
/// [`PENDING_BYTES`]: super::waiting::PENDING_BYTES
/// [`changes::ROOM`]: super::changes::ROOM
/// [`changes::fitted`]: super::changes::fitted
pub(super) const BUDGET: usize =
    ToolReply::CAP - super::changes::ROOM - super::waiting::PENDING_BYTES - 32;

/// `items` from `offset`, at most `limit` of them, with how many there
/// are and how many were left out.
pub(super) fn page(items: &[Value], offset: usize, limit: usize) -> Value {
    let total = items.len();
    let from = offset.min(total);
    let to = from.checked_add(limit).map_or(total, |to| to.min(total));
    let mut listing = json!({
        "items": &items[from..to],
        "total": total,
        "left_out": total - (to - from),
    });
    if offset > total {
        listing["note"] = json!(format!(
            "offset {offset} is past the end: there are {total}"
        ));
    }
    listing
}

/// Whether `value` is a page as [`page`] gives one.
pub(super) fn is_page(value: &Value) -> bool {
    value.as_object().is_some_and(|page| {
        ["items", "total", "left_out"]
            .iter()
            .all(|key| page.contains_key(*key))
    })
}

/// `answer` with each of `lists`' pages by `at` under its key, shortened
/// to `budget` as [`pages`] does.
pub(super) fn paged_into(
    answer: &Value,
    lists: Vec<(&str, Vec<Value>)>,
    at: Page,
    budget: usize,
) -> Value {
    let (keys, lists): (Vec<&str>, Vec<Vec<Value>>) = lists.into_iter().unzip();
    pages(&lists, at, budget, |pages| {
        let mut answer = answer.clone();
        for (key, page) in keys.iter().zip(pages) {
            answer[*key] = page;
        }
        answer
    })
}

/// The answer `answer` makes of `lists`' pages by `at` ([`page`]'s, in
/// `lists`' order), each page shortened past `at.limit` until the
/// answer's JSON is within `budget`: the page of the most bytes loses
/// items first, and none goes below one item, so an answer of long
/// items may still be over (and is then cut, as any reply is).
pub(super) fn pages(
    lists: &[Vec<Value>],
    at: Page,
    budget: usize,
    answer: impl Fn(Vec<Value>) -> Value,
) -> Value {
    let bytes = |value: &Value| serde_json::to_string(value).map_or(usize::MAX, |text| text.len());
    // Each item's bytes with its comma, measured once.
    let sizes: Vec<Vec<usize>> = (lists.iter())
        .map(|items| {
            items
                .iter()
                .map(|item| bytes(item).saturating_add(1))
                .collect()
        })
        .collect();
    let from = |items: &[Value]| at.offset.min(items.len());
    let mut shown: Vec<usize> = (lists.iter())
        .map(|items| (items.len() - from(items)).min(at.limit))
        .collect();
    loop {
        let pages = (lists.iter().zip(&shown))
            .map(|(items, &shown)| page(items, at.offset, shown))
            .collect();
        let said = answer(pages);
        let mut over = bytes(&said).saturating_sub(budget);
        if over == 0 {
            return said;
        }
        // Items off the longest pages until as many bytes are gone.
        let mut cut = false;
        while over > 0 {
            let page_bytes = |list: usize| -> usize {
                let start = from(&lists[list]);
                sizes[list][start..start + shown[list]].iter().sum()
            };
            let Some(longest) = (0..lists.len())
                .filter(|&list| shown[list] > 1)
                .max_by_key(|&list| page_bytes(list))
            else {
                break;
            };
            shown[longest] -= 1;
            let last = from(&lists[longest]) + shown[longest];
            over = over.saturating_sub(sizes[longest][last]);
            cut = true;
        }
        if !cut {
            return said;
        }
    }
}

/// The feature of `doc`'s design named `name`, or the answer saying
/// there's none.
pub(super) fn named<'a>(doc: &'a Doc, name: &str) -> Result<&'a Feature, ToolReply> {
    super::edit::feature_by_name(doc.editor.document(), name).map_err(ToolReply::error)
}

/// The name of the feature `document` is rolled back to, if it is.
pub(super) fn rollback_name(document: &Document) -> Option<String> {
    (document.rollback())
        .and_then(|id| document.feature(id))
        .map(|feature| feature.name.clone())
}

/// How lengths, areas and points are shown in the design's `units`, as
/// the UI shows them.
#[derive(Debug, Clone, Copy)]
pub(super) struct Numbers {
    pub(super) units: LengthUnit,
}

impl Numbers {
    /// `p`, in mm, as bare numbers in the design's units.
    pub(super) fn point<const N: usize>(self, p: [f64; N]) -> Value {
        let shown = varde_expr::Shown::current();
        let unit = Some(Unit::Length(self.units));
        let numbers =
            p.map(|x| (shown.format_number(x, unit).parse::<f64>()).unwrap_or(x / self.units.mm()));
        Value::Array(numbers.iter().map(|&x| json!(x)).collect())
    }

    /// `mm`, a length, as typed text.
    pub(super) fn length(self, mm: f64) -> String {
        varde_expr::Shown::current().format_power(mm, self.units, Power::Length)
    }

    /// `mm2`, an area, as typed text.
    pub(super) fn area(self, mm2: f64) -> String {
        varde_expr::Shown::current().format_power(mm2, self.units, Power::Area)
    }
}

/// Why there's no committed model to name bodies by, before there's
/// been one: a fallback, as calls wait for the model.
pub(super) const NOT_BUILT: &str = "the model isn't built yet: ask again shortly";

/// Why the committed model can't name bodies, while it's being rebuilt:
/// a fallback, as calls wait for the model.
pub(super) const REBUILDING: &str = "the model is being rebuilt: ask again shortly";

/// Whether the committed `model` is being rebuilt: its mesh is of a
/// design since replaced, so it names no bodies yet.
pub(super) fn rebuilding(model: &Shown) -> bool {
    model.parts.is_empty() && !model.mesh.part_ends().is_empty()
}

/// `doc`'s committed model, which the client's names and keys are
/// resolved against, or why there's none to name by ([`NOT_BUILT`],
/// [`REBUILDING`]).
pub(super) fn committed(doc: &Doc) -> Result<Shown<'_>, &'static str> {
    let model = doc.feed.committed().ok_or(NOT_BUILT)?;
    match rebuilding(&model) {
        true => Err(REBUILDING),
        false => Ok(model),
    }
}

/// What a query says of a model that's out of date or rolled back, in
/// [`model_state`].
pub(super) struct ModelNotes {
    /// Its `note` while the model is of an earlier state of the design.
    stale: &'static str,
    /// Its `rolled_back` while the model is built only up to a feature,
    /// if it says so.
    rolled_back: Option<&'static str>,
}

/// What a model's failures being out of date means: `design_summary`'s
/// and `render`'s.
pub(super) const FAILURES_NOTES: ModelNotes = ModelNotes {
    stale: "The model is of an earlier state of the design: regeneration is still running, \
            so failures may be out of date.",
    rolled_back: None,
};

/// How far the committed model of `generation`, built up to before
/// `until`, got and whether it's of the design as it is, with `notes`
/// on it: `design_summary`'s, `body_topology`'s, `measure`'s and
/// `render`'s `model`.
fn model_state(
    doc: &Doc,
    (at, until): (Generation, Option<FeatureId>),
    notes: &ModelNotes,
) -> Value {
    let document = doc.editor.document();
    let mut shown = json!({
        "generation": generation(at),
        "current": at == doc.editor.generation(),
    });
    let until = until
        .and_then(|id| document.feature(id))
        .map(|feature| feature.name.as_str());
    shown["built_up_to"] = match until {
        Some(name) => json!(format!("before {name}")),
        None => json!("the end of the Timeline"),
    };
    if at != doc.editor.generation() {
        shown["note"] = json!(notes.stale);
    }
    if let Some(rolled_back) = notes.rolled_back.filter(|_| until.is_some()) {
        shown["rolled_back"] = json!(rolled_back);
    }
    shown
}

/// The failure of `feature` in the committed model, if it failed.
fn failure<'a>(failures: &'a [FeatureFailure], feature: &Feature) -> Option<&'a FeatureFailure> {
    failures
        .iter()
        .find(|failure| failure.feature == feature.id)
}

/// A generation as a JSON number.
fn generation(generation: Generation) -> Value {
    serde_json::to_value(generation).unwrap_or(Value::Null)
}

/// The design's parameters: name, expression and value as the
/// Parameters tab shows it, or why it has none.
fn parameters(document: &Document) -> Vec<Value> {
    (0..document.params().len())
        .filter_map(|index| parameter(document, index))
        .collect()
}

/// The parameter at `index`, as [`parameters`] lists it.
pub(super) fn parameter(document: &Document, index: usize) -> Option<Value> {
    let param = document.params().get(index)?;
    let resolved = document.params_resolved();
    let shown = varde_expr::Shown::current();
    let mut row = json!({ "name": param.name, "expression": param.text });
    match resolved.result(index) {
        Some(Ok(value)) => {
            let unit = shown.unit(value.quantity, document.units());
            row["value"] = json!(shown.format(value.value, unit));
        }
        Some(Err(why)) => row["error"] = json!(why.to_string()),
        None => {}
    }
    Some(row)
}

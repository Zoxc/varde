//! `sketch`: a sketch's points, curves, constraints, dimensions, plane
//! and regions, read off the committed document, and its degrees of
//! freedom, which the solve lane's client analysis answers
//! (`Request::Analyse` with a `query`): the call waits for it (`waiting.rs`,
//! `Wait::Analysis`). Any sketch, open or not.
//!
//! Points and a region's point inside are bare numbers in the design's
//! units, as `body_topology`'s points and the tools' `profiles` points
//! are; radii, areas and measured values typed text. A dimension's value
//! is its text as typed. Regions are listed in [`Sketch::profiles`]'
//! order, by the index the feature tools take in `profiles`
//! (`make::pick`), so an index read here picks the same region there.

use std::sync::Arc;

use serde_json::{Value, json};
use varde_document::FeatureKind;
use varde_mcp::tools::{SketchQuery, ToolReply};
use varde_sketch::{Analysis, Constraint, Curve, Id, Measure, Region, Sketch};

use super::{BUDGET, Numbers, Shape, pages};
use crate::doc::Doc;

/// The most redundant constraints and dimensions an answer names: past
/// that, how many more.
const REDUNDANT_SHOWN: usize = 50;

/// The most curves listed of a region's loop, and the most holes of a
/// region: past that, how many more. A page of one region must fit the
/// reply's cap, and a loop may have thousands of curves.
const LOOP_SHOWN: usize = 50;

/// The bytes a `sketch` answer leaves under [`BUDGET`] for its analysis
/// ([`analysed`]): the counts and at most [`REDUNDANT_SHOWN`] ids.
const ANALYSIS_BYTES: usize = 1024;

/// A `sketch` call's answer before its degrees of freedom: what to send
/// the solve lane, and the data the analysis completes ([`analysed`]).
#[derive(Debug)]
pub(crate) struct Read {
    /// The committed sketch, to analyse.
    pub(crate) sketch: Arc<Sketch>,
    pub(crate) data: Value,
}

/// `sketch {name, offset?, limit?}`: the sketch named, read off the
/// document, or why not (no feature of that name, or one that isn't a
/// sketch). Each listing is paged alone.
pub(crate) fn sketch(doc: &Doc, args: &SketchQuery) -> Result<Read, ToolReply> {
    let document = doc.editor.document();
    let feature = super::named(doc, &args.name)?;
    let FeatureKind::Sketch { plane, sketch, .. } = &feature.kind else {
        let noun = feature.kind.noun().to_lowercase();
        return Err(ToolReply::error(format!(
            "\"{}\" is {} {noun}, not a sketch",
            varde_mcp::shown(&args.name),
            crate::words::article(&noun)
        )));
    };
    let units = document.units();
    let numbers = Numbers { units };
    let points: Vec<Value> = (sketch.points.iter())
        .map(|point| {
            json!({
                "id": point.id.get(),
                "name": point.name(),
                "at": numbers.point(point.at.to_array()),
            })
        })
        .collect();
    let curves: Vec<Value> = (sketch.curves.iter())
        .map(|entry| {
            let mut row = json!({
                "id": entry.id.get(),
                "name": entry.name(),
                "kind": entry.noun().to_lowercase(),
                "construction": entry.construction,
            });
            match &entry.curve {
                Curve::Line { start, end } => {
                    row["start"] = json!(start.get());
                    row["end"] = json!(end.get());
                }
                Curve::Circle { center, radius } => {
                    row["center"] = json!(center.get());
                    row["radius"] = json!(numbers.length(*radius));
                }
                Curve::Arc { center, start, end } => {
                    row["center"] = json!(center.get());
                    row["start"] = json!(start.get());
                    row["end"] = json!(end.get());
                }
                Curve::Spline(spline) => {
                    row["points"] = ids(spline.points.iter().copied());
                    row["closed"] = json!(spline.closed);
                }
            }
            row
        })
        .collect();
    let constraints: Vec<Value> = (sketch.constraints.iter())
        .map(|entry| {
            json!({
                "id": entry.id.get(),
                "kind": constraint_kind(&entry.constraint),
                "items": ids(entry.constraint.items().map(|(id, _)| id)),
            })
        })
        .collect();
    let shown = varde_expr::Shown::current();
    let dimensions: Vec<Value> = (sketch.dimensions.iter())
        .map(|entry| {
            let dimension = &entry.dimension;
            let mut row = json!({
                "id": entry.id.get(),
                "measure": measure_kind(&dimension.measure),
                "items": ids(dimension.measure.items().map(|(id, _)| id)),
                "value": dimension.value.text,
                "driving": dimension.driving,
            });
            if let Some(measured) = sketch.measure(&dimension.measure, dimension.side) {
                let unit = shown.unit(dimension.measure.quantity(), units);
                row["measured"] = json!(shown.format(measured, unit));
            }
            row
        })
        .collect();
    let regions = match sketch.profiles() {
        Ok(profiles) => {
            let regions: Vec<Value> = (profiles.regions.iter().enumerate())
                .map(|(index, region)| listed_region(numbers, index, region))
                .collect();
            Ok(regions)
        }
        Err(why) => Err(json!({ "error": format!("the sketch is {why}") })),
    };
    let answer = json!({
        "name": feature.name,
        "id": feature.id.get(),
        "units": units.symbol(),
        "plane": Shape::new(doc).sketch_plane(plane),
        "revision": serde_json::to_value(doc.editor.revision()).unwrap_or(Value::Null),
    });
    let mut keys = vec!["points", "curves", "constraints", "dimensions"];
    let mut lists = vec![points, curves, constraints, dimensions];
    let mut answer = answer;
    match regions {
        Ok(regions) => {
            keys.push("regions");
            lists.push(regions);
        }
        Err(error) => answer["regions"] = error,
    }
    let data = pages(&lists, args.page, BUDGET - ANALYSIS_BYTES, |pages| {
        let mut answer = answer.clone();
        for (key, page) in keys.iter().zip(pages) {
            answer[*key] = page;
        }
        answer
    });
    Ok(Read {
        sketch: Arc::new(sketch.clone()),
        data,
    })
}

/// `data`, a [`Read`]'s, completed by the solve lane's `analysis` of its
/// sketch: its degrees of freedom, whether it's solved, and the
/// constraints and dimensions in conflict or redundant, if any.
pub(crate) fn analysed(mut data: Value, analysis: &Analysis) -> ToolReply {
    data["degrees_of_freedom"] = json!(analysis.freedom);
    data["solved"] = json!(analysis.solved);
    if !analysis.redundant.is_empty() {
        let total = analysis.redundant.len();
        data["redundant"] = ids(analysis.redundant.iter().copied().take(REDUNDANT_SHOWN));
        if total > REDUNDANT_SHOWN {
            data["redundant_left_out"] = json!(total - REDUNDANT_SHOWN);
        }
    }
    ToolReply::data(data)
}

fn ids(ids: impl Iterator<Item = Id>) -> Value {
    Value::Array(ids.map(|id| json!(id.get())).collect())
}

/// What a constraint is, in a word.
fn constraint_kind(constraint: &Constraint) -> &'static str {
    match constraint {
        Constraint::Coincident(..) => "coincident",
        Constraint::PointOnCurve { .. } => "point_on_curve",
        Constraint::Horizontal(_) => "horizontal",
        Constraint::Vertical(_) => "vertical",
        Constraint::HorizontalPoints(..) => "horizontal_points",
        Constraint::VerticalPoints(..) => "vertical_points",
        Constraint::Parallel(..) => "parallel",
        Constraint::Perpendicular(..) => "perpendicular",
        Constraint::Tangent { .. } => "tangent",
        Constraint::Smooth { .. } => "smooth",
        Constraint::Equal(..) => "equal",
        Constraint::Concentric(..) => "concentric",
        Constraint::Midpoint { .. } => "midpoint",
        Constraint::Symmetric { .. } => "symmetric",
        Constraint::EqualOffset { .. } => "equal_offset",
        Constraint::Fix(_) => "fix",
    }
}

/// What a dimension measures, in a word.
fn measure_kind(measure: &Measure) -> &'static str {
    match measure {
        Measure::Distance(..) => "distance",
        Measure::HorizontalDistance(..) => "horizontal_distance",
        Measure::VerticalDistance(..) => "vertical_distance",
        Measure::Length(_) => "length",
        Measure::Angle(..) => "angle",
        Measure::Radius(_) => "radius",
        Measure::Diameter(_) => "diameter",
        Measure::Offset(..) => "offset",
        Measure::EdgeDistance(..) => "edge_distance",
    }
}

/// The region at `index`: its area, a point inside, and the curves of
/// its outer loop and of each hole, each sorted with no repeats; at
/// most [`LOOP_SHOWN`] curves a loop and holes, with how many more.
fn listed_region(numbers: Numbers, index: usize, region: &Region) -> Value {
    let curves = |pieces: &[varde_sketch::Piece]| {
        let mut curves: Vec<Id> = pieces.iter().map(|piece| piece.curve).collect();
        curves.sort_unstable();
        curves.dedup();
        let left_out = curves.len().saturating_sub(LOOP_SHOWN);
        (ids(curves.into_iter().take(LOOP_SHOWN)), left_out)
    };
    let (outer, outer_left_out) = curves(&region.outer);
    let mut holes_left_out = 0;
    let holes: Vec<Value> = (region.holes.iter().take(LOOP_SHOWN))
        .map(|hole| {
            let (hole, left_out) = curves(hole);
            holes_left_out += left_out;
            hole
        })
        .collect();
    let mut row = json!({
        "index": index,
        "area": numbers.area(region.area),
        "outer": outer,
        "holes": holes,
    });
    if outer_left_out > 0 {
        row["outer_left_out"] = json!(outer_left_out);
    }
    if region.holes.len() > LOOP_SHOWN {
        row["holes_left_out"] = json!(region.holes.len() - LOOP_SHOWN);
    }
    if holes_left_out > 0 {
        row["hole_curves_left_out"] = json!(holes_left_out);
    }
    if let Some(inside) = region.inside() {
        row["inside"] = numbers.point(inside.to_array());
    }
    row
}

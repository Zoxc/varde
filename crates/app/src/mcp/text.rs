//! How calls are worded for the MCP panel's rows, their tooltips and
//! the log: a call's tool and arguments, what came of it, the user's own
//! edits and undos, and the text off the network made safe to show.

use varde_mcp::tools::{
    Axis, Call, Direction, LinkFrom, LinkKind, Profiles, Reference, SplitWith, SweepAlong, Target,
    ToolReply, Worded, World,
};
use varde_view::McpTool;

use super::waiting::Undid;
use super::{RESULT_BYTES, changes, edit};
use crate::words::{counted, lowercase_first, with_article};
/// The panel's rows of the user's own edits, `lines`, at most
/// [`changes::LINES`], the rest counted in one more.
pub(super) fn user_rows(
    lines: &[String],
    reverted: bool,
) -> impl Iterator<Item = varde_view::McpRow<'_>> {
    fn row(said: std::borrow::Cow<'_, str>, reverted: bool) -> varde_view::McpRow<'_> {
        varde_view::McpRow::User { said, reverted }
    }
    let more = lines.len().saturating_sub(changes::LINES);
    (lines.iter().take(changes::LINES))
        .map(move |line| row(line.as_str().into(), reverted))
        .chain((more > 0).then(|| {
            row(
                counted(more, "more change", "more changes").into(),
                reverted,
            )
        }))
}

/// An undo's row once applied, from what it took back, `undid`: its
/// arguments ("renamed feature Extrude 1 to Base", or "3 edits") and its
/// tooltip, listing them newest first after `summary`, the result's own
/// (its failures).
pub(super) fn undid_row(undid: &Undid, call: &Call, summary: Option<&str>) -> (String, String) {
    let args = match (call, undid.shown.as_slice()) {
        (Call::Undo { steps: 1 }, [one]) => lowercase_first(one),
        _ => counted(undid.shown.len(), "edit", "edits"),
    };
    let mut said = format!("Undid: {}", undid.shown.join("; "));
    // The result's failures, after "applied".
    if let Some(rest) = summary.and_then(|summary| summary.split_once(" · ")) {
        said.push_str(&format!(" · {}", rest.1));
    }
    (plain(&args), clipped(plain(&said), RESULT_BYTES))
}

/// The line under a call whose result told the client of the user's
/// changes, `lines`: "Told: since its last call, you renamed ...", cut
/// to [`RESULT_BYTES`].
pub(super) fn told_line(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let said = format!("Told: since its last call, you {}", lines.join(", "));
    Some(clipped(said, RESULT_BYTES))
}

/// The tool `call` is of, as the panel shows it.
pub(super) fn tool(call: &Call) -> McpTool {
    match call {
        Call::DesignSummary(_) => McpTool::DesignSummary,
        Call::Feature { .. } => McpTool::Feature,
        Call::BodyTopology(_) => McpTool::BodyTopology,
        Call::Sketch(_) => McpTool::Sketch,
        Call::Measure(_) => McpTool::Measure,
        Call::Render(_) => McpTool::Render,
        Call::Rename(_) => McpTool::Rename,
        Call::SetVisible { .. } => McpTool::SetVisible,
        Call::SetColor { .. } => McpTool::SetColor,
        Call::SetOpacity { .. } => McpTool::SetOpacity,
        Call::RemoveFeature { .. } => McpTool::RemoveFeature,
        Call::RemoveBody { .. } => McpTool::RemoveBody,
        Call::AddParameter { .. } => McpTool::AddParameter,
        Call::SetParameter { .. } => McpTool::SetParameter,
        Call::RenameParameter { .. } => McpTool::RenameParameter,
        Call::RemoveParameter { .. } => McpTool::RemoveParameter,
        Call::SetUnits(_) => McpTool::SetUnits,
        Call::SetTolerance { .. } => McpTool::SetTolerance,
        Call::SetRollback { .. } => McpTool::SetRollback,
        Call::AddSketch { .. } => McpTool::AddSketch,
        Call::SetSketchPlane { .. } => McpTool::SetSketchPlane,
        Call::AddLink(_) => McpTool::AddLink,
        Call::EditSketch(_) => McpTool::EditSketch,
        Call::SetFeature(_) => McpTool::SetFeature,
        Call::Extrude(_) => McpTool::Extrude,
        Call::Revolve(_) => McpTool::Revolve,
        Call::Combine(_) => McpTool::Combine,
        Call::MoveBody(_) => McpTool::MoveBody,
        Call::Mirror(_) => McpTool::Mirror,
        Call::Pattern(_) => McpTool::Pattern,
        Call::Align(_) => McpTool::Align,
        Call::Scale(_) => McpTool::Scale,
        Call::Split(_) => McpTool::Split,
        Call::Shell(_) => McpTool::Shell,
        Call::OffsetFace(_) => McpTool::OffsetFace,
        Call::Draft(_) => McpTool::Draft,
        Call::Chamfer(_) => McpTool::Chamfer,
        Call::Fillet(_) => McpTool::Fillet,
        Call::Sweep(_) => McpTool::Sweep,
        Call::Loft(_) => McpTool::Loft,
        Call::Undo { .. } => McpTool::Undo,
    }
}

/// `call`'s arguments as the panel shows them, as typed: names as given,
/// filters and pages as the client set them, what's left at its default
/// unsaid.
pub(super) fn args(call: &Call) -> String {
    let page = |offset: usize, limit: usize, default: usize| {
        let mut said = Vec::new();
        if offset != 0 {
            said.push(format!("from {offset}"));
        }
        if limit != default {
            said.push(format!("{limit} at most"));
        }
        said
    };
    let said: Vec<String> = match call {
        Call::DesignSummary(at) => page(at.offset, at.limit, varde_mcp::tools::SUMMARY_LIMIT),
        Call::Feature { name, .. } => vec![name.clone()],
        Call::BodyTopology(topology) => {
            let mut said = vec![topology.body.clone()];
            if let Some(normal) = topology.faces.and_then(|faces| faces.normal) {
                said.push(format!("faces facing {}", direction(normal)));
            }
            let edges = topology.edges.unwrap_or_default();
            match edges.convex {
                Some(true) => said.push("convex edges".to_owned()),
                Some(false) => said.push("concave edges".to_owned()),
                None => {}
            }
            if let Some(along) = edges.direction {
                said.push(format!("edges along {}", direction(along)));
            }
            said.extend(page(
                topology.page.offset,
                topology.page.limit,
                varde_mcp::tools::TOPOLOGY_LIMIT,
            ));
            said
        }
        Call::Sketch(args) => {
            let mut said = vec![args.name.clone()];
            said.extend(page(
                args.page.offset,
                args.page.limit,
                varde_mcp::tools::SKETCH_LIMIT,
            ));
            said
        }
        Call::Measure(args) => {
            let mut said = vec![measured(&args.a)];
            if let Some(b) = &args.b {
                said.push(format!("to {}", measured(b)));
            }
            said
        }
        Call::Render(args) => {
            let mut said = match args.bodies.is_empty() {
                true => vec!["the model".to_owned()],
                false => vec![args.bodies.join(", ")],
            };
            said.push(match &args.from {
                varde_mcp::tools::RenderFrom::View(view) => {
                    format!("from {}", format!("{view:?}").to_lowercase())
                }
                varde_mcp::tools::RenderFrom::Angles { yaw, pitch } => {
                    format!("from yaw {yaw}, pitch {pitch}")
                }
            });
            if args.perspective {
                said.push("in perspective".to_owned());
            }
            if args.zoom != 1.0 {
                said.push(format!("zoomed {}", args.zoom));
            }
            if !args.highlight.is_empty() {
                said.push(format!(
                    "highlighting {}",
                    counted(args.highlight.len(), "item", "items")
                ));
            }
            said
        }
        Call::Rename(rename) => {
            let (what, name) = match &rename.target {
                Target::Feature(name) => ("feature", name),
                Target::Body(name) => ("body", name),
            };
            vec![format!("{what} {name} to {}", rename.to)]
        }
        Call::SetVisible { body, visible } => {
            vec![format!(
                "{body} {}",
                if *visible { "shown" } else { "hidden" }
            )]
        }
        Call::SetColor { body, color } => vec![match color {
            Some(color) => format!(
                "{body} to hue {}°, saturation {}",
                varde_expr::format_number(color.hue, None),
                varde_expr::format_number(color.saturation, None)
            ),
            None => format!("{body} to the theme's"),
        }],
        Call::SetOpacity { body, opacity } => vec![format!(
            "{body} to {}",
            varde_expr::format_number(*opacity, None)
        )],
        Call::RemoveFeature { feature: name }
        | Call::RemoveBody { body: name }
        | Call::RemoveParameter { name } => vec![name.clone()],
        Call::AddParameter { name, value } | Call::SetParameter { name, value } => {
            vec![format!("{name} = {value}")]
        }
        Call::RenameParameter { name, to } => vec![format!("{name} to {to}")],
        Call::SetUnits(units) => vec![units.word().to_owned()],
        Call::SetTolerance { tolerance } => vec![tolerance.clone()],
        Call::SetRollback { before } => vec![match before {
            Some(name) => format!("before {name}"),
            None => "to the end".to_owned(),
        }],
        Call::AddSketch { plane, .. } => vec![reference(plane)],
        Call::SetSketchPlane { sketch, plane } => vec![format!("{sketch} to {}", reference(plane))],
        Call::AddLink(link) => {
            let from = match &link.from {
                LinkFrom::Item(item) => with_article(item.kind.word()),
                LinkFrom::Curve(curve) => format!("{}'s curve {}", curve.sketch, curve.curve),
                LinkFrom::Point { sketch, point } => format!("{sketch}'s point {point}"),
            };
            let kind = match link.kind {
                LinkKind::Project => "projected",
                LinkKind::Intersect => "intersected",
            };
            vec![format!("{} {from} {kind}", link.sketch)]
        }
        // The sketch and what's done in it, by the operations' names.
        Call::EditSketch(edit) => {
            let ops: Vec<&str> = (edit.operations.iter().take(8))
                .map(|operation| operation.op.word())
                .collect();
            let more = edit.operations.len().saturating_sub(ops.len());
            let mut said = format!("{}: {}", edit.sketch, ops.join(", "));
            if more > 0 {
                said.push_str(&format!(" and {more} more"));
            }
            vec![said]
        }
        // The fields changed, by name: their values may be keys.
        Call::SetFeature(set) => {
            let fields: Vec<&str> = set.patch.keys().map(String::as_str).collect();
            vec![format!("{}: {}", set.name, fields.join(", "))]
        }
        Call::Extrude(extrude) => {
            let mut said = vec![extrude.sketch.clone(), profiles(&extrude.profiles)];
            said.extend(extrude.distance.clone());
            said
        }
        Call::Revolve(revolve) => {
            let mut said = vec![revolve.sketch.clone(), profiles(&revolve.profiles)];
            said.push(match &revolve.axis {
                Axis::SketchX => "about x".to_owned(),
                Axis::SketchY => "about y".to_owned(),
                Axis::Line(id) => format!("about line {id}"),
                Axis::Edge(_) => "about an edge".to_owned(),
            });
            said.extend(revolve.angle.clone());
            said
        }
        Call::Combine(combine) => {
            vec![format!(
                "{} with {}",
                combine.target,
                combine.tools.join(", ")
            )]
        }
        Call::MoveBody(moved) => {
            let mut said = vec![moved.bodies.join(", ")];
            let offset: Vec<&str> = (moved.offset.iter())
                .map(|offset| offset.as_deref().unwrap_or("0"))
                .collect();
            if moved.offset.iter().any(Option::is_some) {
                said.push(format!("by {}", offset.join(", ")));
            }
            said.extend(moved.angle.clone());
            said
        }
        Call::Mirror(mirror) => vec![format!(
            "{} across {}",
            mirror.bodies.join(", "),
            reference(&mirror.plane)
        )],
        Call::Pattern(pattern) => {
            let mut said = vec![pattern.bodies.join(", ")];
            said.extend(pattern.count.as_ref().map(|count| format!("×{count}")));
            said.extend(pattern.spread.clone());
            said
        }
        Call::Align(align) => vec![format!(
            "{} to {}",
            reference(&align.from.point),
            reference(&align.to.point)
        )],
        Call::Scale(scale) => {
            let mut said = vec![scale.bodies.join(", ")];
            said.extend(scale.factor.clone());
            said.extend(scale.length.clone());
            said
        }
        Call::Split(split) => {
            let tool = match &split.tool {
                SplitWith::Surface(surface) => reference(surface),
                SplitWith::Body(body) => body.clone(),
                SplitWith::Regions {
                    sketch,
                    profiles: picked,
                } => {
                    format!("{sketch} {}", profiles(picked))
                }
                SplitWith::Line { sketch, curves } => {
                    format!("{sketch}'s {}", counted(curves.len(), "curve", "curves"))
                }
            };
            vec![format!("{} by {tool}", split.body)]
        }
        Call::Shell(shell) => {
            let mut said: Vec<String> = shell.body.iter().cloned().collect();
            said.push(faces(shell.faces.len()));
            said.extend(shell.thickness.clone());
            said
        }
        Call::OffsetFace(offset) => {
            let mut said = vec![faces(offset.faces.len())];
            said.extend(offset.distance.clone());
            said
        }
        Call::Draft(draft) => {
            let mut said = vec![faces(draft.faces.len())];
            said.extend(
                draft
                    .neutral
                    .as_ref()
                    .map(|neutral| format!("from {}", reference(neutral))),
            );
            said.extend(draft.angle.clone());
            said
        }
        Call::Chamfer(chamfer) => {
            let mut said = blend(&chamfer.edges, &chamfer.faces);
            said.extend(chamfer.distance.clone());
            said
        }
        Call::Fillet(fillet) => {
            let mut said = blend(&fillet.edges, &fillet.faces);
            said.extend(fillet.radius.clone());
            said
        }
        Call::Sweep(sweep) => {
            let along = match sweep.along {
                SweepAlong::Path { .. } => "a path",
                SweepAlong::Helix { .. } => "a helix",
            };
            vec![format!(
                "{} {} along {along}",
                sweep.sketch,
                profiles(&sweep.profiles)
            )]
        }
        Call::Loft(loft) => {
            let mut said = vec![counted(loft.sections.len(), "section", "sections")];
            if !loft.rails.is_empty() {
                said.push(counted(loft.rails.len(), "rail", "rails"));
            }
            said
        }
        // What it's to take back; once it has, its row says what it did
        // (`undid_row`).
        Call::Undo { steps: 1 } => vec!["its last edit".to_owned()],
        Call::Undo { steps } => vec![format!("its last {steps} edits")],
    };
    let mut said = said;
    if let Some(bodies) = (call.naming().map(|naming| &naming.bodies)).filter(|b| !b.is_empty()) {
        said.push(format!("making {}", bodies.join(", ")));
    }
    // What it's to be named, first: "Base: Sketch 1, all, 20 mm".
    match call.new_name() {
        Some(name) => format!("{name}: {}", said.join(", ")),
        None => said.join(", "),
    }
}

/// What a motion tool refers to, for its row: the world's by its word, a
/// key by what it names (its text means nothing to the user).
fn reference<W: World>(reference: &Reference<W>) -> String {
    match reference {
        Reference::World(world) => match world.word() {
            "origin" => "the origin".to_owned(),
            word => word.to_owned(),
        },
        Reference::Key(item) => with_article(item.kind.word()),
    }
}

/// A face tool's faces, for its row: how many (their keys mean nothing
/// to the user).
fn faces(count: usize) -> String {
    counted(count, "face", "faces")
}

/// A blend's edges and faces, for its row: how many of each it names.
fn blend(edges: &[String], faces: &[String]) -> Vec<String> {
    let mut said = Vec::new();
    if !edges.is_empty() {
        said.push(counted(edges.len(), "edge", "edges"));
    }
    if !faces.is_empty() {
        said.push(self::faces(faces.len()));
    }
    said
}

/// A feature tool's profiles, for its row.
fn profiles(profiles: &Profiles) -> String {
    match profiles {
        Profiles::All => "every region".to_owned(),
        Profiles::Picked(items) => counted(items.len(), "region", "regions"),
    }
}

/// What `measure` measures, as its row says it: a body's name, or what
/// a key names (not the key, which says nothing to the user).
fn measured(item: &varde_mcp::tools::Measured) -> String {
    match item {
        varde_mcp::tools::Measured::Body(name) => name.clone(),
        varde_mcp::tools::Measured::Item(item) => with_article(item.kind.word()),
    }
}

/// `direction` as a client would type it: an axis (`+z`, or `z` either
/// way), else its three numbers.
fn direction(direction: Direction) -> String {
    let [x, y, z] = direction.vector;
    let zeros = [x, y, z].iter().filter(|value| **value == 0.0).count();
    for (axis, value) in [("x", x), ("y", y), ("z", z)] {
        if value.abs() == 1.0 && zeros == 2 {
            return match (direction.signed, value > 0.0) {
                (false, _) => axis.to_owned(),
                (true, true) => format!("+{axis}"),
                (true, false) => format!("-{axis}"),
            };
        }
    }
    let number = |value: f64| varde_expr::format_number(value, None);
    format!("{}, {}, {}", number(x), number(y), number(z))
}

/// What came of `call`, summed up for its row's tooltip: what the answer
/// lists, or why it failed, cut to [`RESULT_BYTES`].
pub(super) fn summary(call: &Call, reply: &ToolReply) -> String {
    let total = |listing: &str| {
        let data = reply.structured.as_ref()?;
        usize::try_from(data.get(listing)?.get("total")?.as_u64()?).ok()
    };
    let totals = |listings: &[(&str, &str, &str)]| -> Option<String> {
        let said: Option<Vec<String>> = (listings.iter())
            .map(|(listing, one, many)| total(listing).map(|n| counted(n, one, many)))
            .collect();
        said.map(|said| said.join(" · "))
    };
    let said = match call {
        _ if reply.is_error => None,
        Call::DesignSummary(_) => totals(&[
            ("bodies", "body", "bodies"),
            ("timeline", "feature", "features"),
            ("parameters", "parameter", "parameters"),
        ]),
        Call::Feature { .. } => (reply.structured.as_ref()).and_then(|data| {
            let kind = data.get("kind")?.as_str()?;
            Some(
                match data.get("failure").and_then(|failure| failure.as_str()) {
                    Some(failure) => format!("{kind} · fails: {failure}"),
                    None => kind.to_owned(),
                },
            )
        }),
        Call::BodyTopology(_) => totals(&[
            ("faces", "face", "faces"),
            ("edges", "edge", "edges"),
            ("corners", "corner", "corners"),
        ]),
        Call::Sketch(_) => totals(&[
            ("points", "point", "points"),
            ("curves", "curve", "curves"),
            ("regions", "region", "regions"),
        ])
        .map(|said| {
            let free = (reply.structured.as_ref())
                .and_then(|data| data.get("degrees_of_freedom")?.as_u64());
            match free {
                Some(0) => format!("{said} · fully constrained"),
                Some(1) => format!("{said} · 1 degree of freedom"),
                Some(n) => format!("{said} · {n} degrees of freedom"),
                None => said,
            }
        }),
        Call::Measure(_) => (reply.structured.as_ref()).and_then(|data| {
            let first = |of: &str| -> Option<String> {
                let values = data.get(of)?.get("values")?.as_object()?;
                let (label, value) = values.iter().next()?;
                Some(format!("{}: {}", label.replace('_', " "), value.as_str()?))
            };
            let between = data.get("between").and_then(|between| {
                let said = |key: &str| between.get(key).and_then(serde_json::Value::as_str);
                match (said("distance"), said("angle")) {
                    (Some(d), Some(a)) => Some(format!("distance {d} · angle {a}")),
                    (Some(d), None) => Some(format!("distance {d}")),
                    (None, Some(a)) => Some(format!("angle {a}")),
                    (None, None) => None,
                }
            });
            between.or_else(|| first("a"))
        }),
        Call::Render(_) => (reply.structured.as_ref()).and_then(|data| {
            let size = data.get("size")?.as_array()?;
            let side = |i: usize| size.get(i)?.as_u64();
            Some(format!("{} × {} PNG", side(0)?, side(1)?))
        }),
        _ => edit::summary(reply),
    };
    clipped(
        plain(&said.unwrap_or_else(|| reply.text.clone())),
        RESULT_BYTES,
    )
}

/// `text` cut to at most `bytes` (on a character boundary), with an
/// ellipsis where it was cut.
pub(super) fn clipped(text: String, bytes: usize) -> String {
    let kept = varde_mcp::cut(&text, bytes);
    match kept.len() < text.len() {
        true => format!("{kept}…"),
        false => text,
    }
}

/// `text`, which may come off the network, made safe to show on one
/// line: line breaks, tabs and other whitespace controls as spaces,
/// other controls and the bidi marks, embeddings, overrides and isolates
/// (which would turn the text around them) as U+FFFD.
pub(super) fn plain(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            c if c.is_control() && c.is_whitespace() => ' ',
            '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}' => '\u{FFFD}',
            c if c.is_control() => '\u{FFFD}',
            c => c,
        })
        .collect()
}

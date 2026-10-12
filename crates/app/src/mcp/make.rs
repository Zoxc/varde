//! The MCP client's feature tools, `extrude`, `revolve` and `combine`
//! (see `notes/AgentSupport.md`, "Edits"): each set up in the panel's own
//! session, built from what the panel shows (typed text read by the
//! panel's fields, regions picked as its clicks pick them, bodies by
//! name), its defaults the panel's, and added where the panel's OK adds
//! it, at the rollback marker (`Document::add_feature`, applied as
//! `Command::AddFeature`). What the panel leaves to the document (its
//! references, its own check) the editor refuses.
//!
//! Profiles are named by their index in the sketch's regions, the order
//! [`Sketch::profiles`](varde_sketch::Sketch::profiles) finds them in,
//! which the panel's picks index too and `sketch` will list; `"all"`; or
//! a point inside one, in the sketch's coordinates
//! ([`Profiles::region_at`](varde_sketch::Profiles::region_at)).

use varde_document::{AxisLine, Command, FeatureId};
use varde_mcp::tools::{self, Axis, ExtrudeExtent, RevolveExtent};
use varde_view::{Angle, Distance, ExtentKind, OperationKind, TurnKind};

use super::args::{
    Given, added, curve_in, excluded, failed, gone_sketch, operation, picked_bodies, read, regions,
    sketch_named, tool_for,
};
use super::edit::{Made, Refused};
use crate::doc::{CombineSession, Doc, ExtrudeSession, RevolveSession, sketch_of};

/// Why an edited extrude's or revolve's Flip can't be set: its extent
/// ignores it (stored as it was).
const FLIP_EXTENT: &str = "flip goes with extent one_side or two_sides";

/// The commands `extrude` makes on `doc`'s design, and what it's to
/// make: see the module's docs.
pub(super) fn extrude(doc: &Doc, args: &tools::Extrude) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    const TOOL: &str = "extrude";
    let sketch = sketch_named(document, TOOL, &args.sketch, None)?;
    let mut session = ExtrudeSession::for_client(document, sketch);
    set_extrude(doc, &mut session, args, Given::All)?;
    let extrude =
        (session.extrude()).ok_or_else(|| failed(TOOL, "no region was picked".to_owned()))?;
    Ok(added(document, extrude.into()))
}

/// Takes `args`, those `given`, into the extrude `session` (a new one's
/// or one opened on the feature it edits) as the panel's messages do.
/// Editing, a value of an extent that doesn't show it is refused, as
/// the panel has no field for it.
pub(super) fn set_extrude(
    doc: &Doc,
    session: &mut ExtrudeSession,
    args: &tools::Extrude,
    given: Given,
) -> Result<(), Refused> {
    let document = doc.editor.document();
    let edited = session.feature;
    let tool = tool_for("extrude", edited);
    let wrong = |why: String| failed(tool, why);
    regions(
        doc,
        &mut session.regions,
        tool,
        edited,
        &args.sketch,
        &args.profiles,
        given,
    )?;
    if given.has("operation") {
        let operation = operation(args.operation);
        // The panel takes a cut through all made another as one side,
        // the client is told rather.
        let through_all = session.extent == ExtentKind::ThroughAll && !given.has("extent");
        if edited.is_some() && through_all && operation != OperationKind::Cut {
            return Err(wrong("through_all goes with operation cut only".to_owned()));
        }
        session.set_operation(operation);
    }
    if given.has("extent") {
        let extent = match args.extent {
            ExtrudeExtent::OneSide => ExtentKind::OneSide,
            ExtrudeExtent::Symmetric => ExtentKind::Symmetric,
            ExtrudeExtent::TwoSides => ExtentKind::TwoSides,
            ExtrudeExtent::ThroughAll => ExtentKind::ThroughAll,
        };
        if !session.set_extent(extent) {
            return Err(wrong("through_all goes with operation cut only".to_owned()));
        }
    }
    if edited.is_some() {
        let shows = session.extent.distances();
        if given.has("distance") && !shows.contains(&Distance::First) {
            let why = "distance goes with extent one_side, symmetric or two_sides";
            return Err(wrong(why.to_owned()));
        }
        if given.has("second_distance") && !shows.contains(&Distance::Second) {
            return Err(wrong(
                "second_distance goes with extent two_sides".to_owned(),
            ));
        }
        if given.has("flip") && !session.extent.flips() {
            return Err(wrong(FLIP_EXTENT.to_owned()));
        }
    }
    // Two sides' second is the first's unless given, as a new panel's
    // fields start alike; an edited one's keeps its own.
    let second = match edited {
        None => args.second_distance.as_ref().or(args.distance.as_ref()),
        Some(_) => args.second_distance.as_ref(),
    };
    for (distance, text, said) in [
        (Distance::First, args.distance.as_ref(), "distance"),
        (Distance::Second, second, "second_distance"),
    ] {
        if let Some(text) = text.filter(|_| given.has(said) || edited.is_none()) {
            session.input(distance, text.trim().to_owned(), document);
            read(session.sides.fields[distance.index()].error.as_ref(), said).map_err(wrong)?;
        }
    }
    if let Some(text) = args.taper.as_ref().filter(|_| given.has("taper")) {
        session.input_taper(text.trim().to_owned(), document);
        read(session.taper.error.as_ref(), "taper").map_err(wrong)?;
    }
    if given.has("flip") {
        session.sides.flip = args.flip;
    }
    if given.has("exclude") {
        let new_body = session.operation == OperationKind::NewBody;
        session.targets = excluded(doc, tool, &args.exclude, new_body, edited)?;
    }
    Ok(())
}

/// The commands `revolve` makes on `doc`'s design, and what it's to
/// make: see the module's docs. Its axis is a line of the sketch, one of
/// its axes, or a straight edge in its plane by key, checked as the
/// panel checks a click on one.
pub(super) fn revolve(doc: &Doc, args: &tools::Revolve) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    const TOOL: &str = "revolve";
    let sketch = sketch_named(document, TOOL, &args.sketch, None)?;
    let mut session = RevolveSession::new(document, Some(sketch));
    set_revolve(doc, &mut session, args, Given::All)?;
    let revolve = (session.revolve(document))
        .ok_or_else(|| failed(TOOL, "no region was picked".to_owned()))?;
    Ok(added(document, revolve.into()))
}

/// Takes `args`, those `given`, into the revolve `session`, as
/// [`set_extrude`] does an extrude's. A new sketch needs its axis given
/// too, as the old one's lines mean nothing in it.
pub(super) fn set_revolve(
    doc: &Doc,
    session: &mut RevolveSession,
    args: &tools::Revolve,
    given: Given,
) -> Result<(), Refused> {
    let document = doc.editor.document();
    let edited = session.feature;
    let tool = tool_for("revolve", edited);
    let wrong = |why: String| failed(tool, why);
    let before = session.regions.source;
    regions(
        doc,
        &mut session.regions,
        tool,
        edited,
        &args.sketch,
        &args.profiles,
        given,
    )?;
    let sketch = session.regions.source;
    if sketch != before && !given.has("axis") {
        return Err(wrong(
            "sketch goes with axis: one of the new sketch's".to_owned(),
        ));
    }
    if given.has("axis") {
        let sketch = sketch.ok_or_else(|| wrong(gone_sketch("revolve")))?;
        axis(doc, session, sketch, &args.axis).map_err(wrong)?;
    }
    if given.has("extent") {
        session.extent = match args.extent {
            RevolveExtent::Full => TurnKind::Full,
            RevolveExtent::OneSide => TurnKind::OneSide,
            RevolveExtent::Symmetric => TurnKind::Symmetric,
            RevolveExtent::TwoSides => TurnKind::TwoSides,
        };
    }
    if edited.is_some() {
        let shows = session.extent.angles();
        if given.has("angle") && !shows.contains(&Angle::First) {
            let why = "angle goes with extent one_side, symmetric or two_sides";
            return Err(wrong(why.to_owned()));
        }
        if given.has("second_angle") && !shows.contains(&Angle::Second) {
            return Err(wrong("second_angle goes with extent two_sides".to_owned()));
        }
        if given.has("flip") && !session.extent.flips() {
            return Err(wrong(FLIP_EXTENT.to_owned()));
        }
    }
    for (angle, text, said) in [
        (Angle::First, &args.angle, "angle"),
        (Angle::Second, &args.second_angle, "second_angle"),
    ] {
        if let Some(text) = text.as_ref().filter(|_| given.has(said)) {
            session.input(angle, text.trim().to_owned(), document);
            read(session.sides.fields[angle.index()].error.as_ref(), said).map_err(wrong)?;
        }
    }
    if given.has("flip") {
        session.sides.flip = args.flip;
    }
    if given.has("operation") {
        session.operation = operation(args.operation);
    }
    if given.has("exclude") {
        let new_body = session.operation == OperationKind::NewBody;
        session.targets = excluded(doc, tool, &args.exclude, new_body, edited)?;
    }
    Ok(())
}

/// The commands `combine` makes on `doc`'s design, and what it's to
/// make: its bodies by name, one merged into another before where it
/// goes as the body holding it (as a click on it picks), each one a
/// combine there can name.
pub(super) fn combine(doc: &Doc, args: &tools::Combine) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    let mut session = CombineSession::new(None, Vec::new());
    set_combine(doc, &mut session, args, Given::All)?;
    let combine = session
        .combine()
        .ok_or_else(|| failed("combine", "no tools were given".to_owned()))?;
    Ok(added(document, combine.into()))
}

/// Takes `args`, those `given`, into the combine `session`: the target
/// and tools by name as clicks pick them, the target not among the
/// tools.
pub(super) fn set_combine(
    doc: &Doc,
    session: &mut CombineSession,
    args: &tools::Combine,
    given: Given,
) -> Result<(), Refused> {
    let edited = session.feature;
    let tool = tool_for("combine", edited);
    let pick = |names: &[String]| picked_bodies(doc, tool, "combine", names, edited);
    let target = match given.has("target") {
        true => pick(std::slice::from_ref(&args.target))?.first().copied(),
        false => session.target,
    };
    let target = target.ok_or_else(|| failed(tool, "no target was given".to_owned()))?;
    let tools = match given.has("tools") {
        true => pick(&args.tools)?,
        false => session.tools.clone(),
    };
    // The panel's session takes the target out of the tools and each
    // once; the client is told rather than have them dropped.
    if tools.contains(&target) {
        let why = "the target can't be one of the tools".to_owned();
        return Err(failed(tool, why));
    }
    let mut set = CombineSession::new(Some(target), tools);
    set.feature = edited;
    set.op = match given.has("operation") {
        true => match args.operation {
            tools::BodyOp::Union => varde_document::BodyOp::Union,
            tools::BodyOp::Subtract => varde_document::BodyOp::Subtract,
            tools::BodyOp::Intersect => varde_document::BodyOp::Intersect,
        },
        false => session.op,
    };
    set.keep_tools = match given.has("keep_tools") {
        true => args.keep_tools,
        false => session.keep_tools,
    };
    *session = set;
    Ok(())
}

/// Picks `axis` as the axis of the revolve `session` of the sketch
/// `sketch` as the panel would: a line of the sketch with length, or one
/// of its axes; or a model edge by key, straight and in the sketch's
/// plane, made before where the revolve goes, as a click on it is
/// checked ([`Doc::edge_axis`]).
fn axis(
    doc: &Doc,
    session: &mut RevolveSession,
    sketch: FeatureId,
    axis: &Axis,
) -> Result<(), String> {
    let document = doc.editor.document();
    let line = match axis {
        Axis::SketchX => AxisLine::SketchX,
        Axis::SketchY => AxisLine::SketchY,
        Axis::Line(number) => {
            let id = curve_in(document, sketch, *number)?;
            let drawn = sketch_of(document, sketch).ok_or("the sketch is gone")?;
            let curve = drawn.curve(id).map(|entry| &entry.curve);
            if !matches!(curve, Some(varde_sketch::Curve::Line { .. })) {
                return Err(format!("curve {number} of the sketch isn't a line"));
            }
            AxisLine::Curve(id)
        }
        Axis::Edge(key) => {
            let found = super::key::edge(doc, key)?;
            let near = found.reference.near;
            let (reference, ends) = doc
                .edge_axis(&found.index, sketch, session.feature, found.edge, near)
                .map_err(|why| why.into_owned())?;
            session.pick_edge_axis(reference, ends, document);
            return Ok(());
        }
    };
    if !session.pick_axis(sketch, line, document) {
        return Err("that line has no length".to_owned());
    }
    Ok(())
}
